use crate::ai::{llm::Content, ChatSession, StableLLMResponseChunk};

pub type ChunksTx = tokio::sync::mpsc::UnboundedSender<(String, super::tts::TTSResponseRx)>;
pub type ChunksRx = tokio::sync::mpsc::UnboundedReceiver<(String, super::tts::TTSResponseRx)>;

pub async fn chat(
    tts_tx: &mut super::tts::TTSRequestTx,
    chunks_tx: ChunksTx,
    chat_session: &mut ChatSession,
    asr_result: String,
) -> anyhow::Result<()> {
    let message = asr_result;

    if matches!(
        chat_session.messages.back(),
        Some(Content {
            role: crate::ai::llm::Role::User,
            ..
        })
    ) {
        chat_session.messages.pop_back();
    }

    chat_session.add_user_message(message);

    log::info!("start llm");

    let mut resp = chat_session
        .complete()
        .await
        .map_err(|e| anyhow::anyhow!("error completing chat session for message: {e}"))?;

    let mut llm_response = String::with_capacity(128);

    loop {
        match resp.next_chunk().await {
            Ok(StableLLMResponseChunk::Text(chunk)) => {
                let chunk_ = chunk.trim();
                log::debug!("llm chunk: {chunk_:?}");

                llm_response.push_str(&chunk);
                if chunk_.is_empty() {
                    continue;
                }

                let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();

                tts_tx
                    .send((chunk_.to_string(), tts_resp_tx))
                    .await
                    .map_err(|e| anyhow::anyhow!("error sending tts request for llm chunk: {e}"))?;

                chunks_tx
                    .send((chunk_.to_string(), tts_resp_rx))
                    .map_err(|e| {
                        anyhow::anyhow!("error sending tts chunks receiver for llm chunk: {e}")
                    })?;
            }
            Ok(StableLLMResponseChunk::Functions(functions)) => {
                log::info!("llm functions: {:#?}", functions);
                chat_session.add_assistant_tool_call(functions.clone());
                for function in functions {
                    chat_session.execute_tool(&function).await?
                }
                resp = chat_session.complete().await?;
                continue;
            }
            Ok(StableLLMResponseChunk::Stop) => {
                log::info!("llm done");

                if !llm_response.is_empty() {
                    chat_session.add_assistant_message(llm_response);
                }

                break;
            }
            Err(e) => {
                log::error!("llm error: {:#?}", e);
                break;
            }
        }
    }
    Ok(())
}
