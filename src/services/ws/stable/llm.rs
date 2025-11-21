use std::collections::HashMap;

use lazy_regex::regex;

use crate::ai::{llm::Content, ChatSession, StableLLMResponseChunk};

pub type ChunksTx = tokio::sync::mpsc::UnboundedSender<(String, super::tts::TTSResponseRx)>;
pub type ChunksRx = tokio::sync::mpsc::UnboundedReceiver<(String, super::tts::TTSResponseRx)>;

use tokio::time::Duration;

#[cached::proc_macro::cached(time = 60, size = 100, result = true)]
async fn load_url_content(url: String) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let res = client.get(&url).send().await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "load url content failed, status:{}, body:{}",
            status,
            body
        ));
    }
    let content = res
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("error reading content from {}: {}", url, e))?;
    Ok(content)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromptParts {
    pub sys_prompts: Vec<Content>,
    pub dynamic_prompts: std::collections::LinkedList<Content>,
}

impl crate::config::LLMConfig {
    pub async fn prompts(&self) -> PromptParts {
        if !self.prompts_url.is_empty() {
            match self.load_prompts().await {
                Ok(prompt_parts) => return prompt_parts,
                Err(e) => {
                    log::warn!("error loading prompt from url {}: {}", self.prompts_url, e);
                }
            }
        }

        let r = regex!(r"\{\{(?P<url>\s*https?://\S+?\s*)\}\}");

        let mut urls = vec![];
        let mut contents = HashMap::new();

        let mut sys_prompts = self.sys_prompts.clone();
        if let Some(sys_prompt) = sys_prompts.first_mut() {
            if sys_prompt.role == crate::ai::llm::Role::System {
                for cap in r.captures_iter(&sys_prompt.message) {
                    if let Some(url) = cap.name("url") {
                        let url = url.as_str().trim();
                        urls.push(url.to_string());
                    }
                }
                log::debug!("found urls in system prompt: {:?}", urls);

                for url in urls {
                    match load_url_content(url.clone()).await {
                        Ok(content) => {
                            contents.insert(url, content);
                        }
                        Err(e) => {
                            log::warn!("error loading prompt content from {}: {}", url, e);
                        }
                    }
                }

                let new_message =
                    r.replace_all(&sys_prompt.message, |caps: &lazy_regex::Captures| {
                        let url = caps.name("url").unwrap().as_str().trim();
                        contents.get(url).cloned().unwrap_or(url.to_string())
                    });

                sys_prompt.message = new_message.to_string();
            }
        }

        {
            PromptParts {
                sys_prompts,
                dynamic_prompts: self.dynamic_prompts.clone(),
            }
        }
    }

    async fn load_prompts(&self) -> anyhow::Result<PromptParts> {
        let prompt_url = &self.prompts_url;
        let client = reqwest::Client::new();
        let res = client.get(prompt_url).send().await?;
        let status = res.status();
        if status != 200 {
            let body = res.text().await?;
            return Err(anyhow::anyhow!(
                "load prompt failed, status:{}, body:{}",
                status,
                body
            ));
        }
        let prompt = res
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("error parsing prompt json from {}: {}", prompt_url, e))?;
        Ok(prompt)
    }
}

#[tokio::test]
async fn test_load_prompts() {
    let message =
        "You are a helpful assistant. {{ https://langchain-ai.github.io/langgraph/llms.txt?a=1 }}";

    let llm_config = crate::config::LLMConfig {
        model: "test-model".to_string(),
        api_key: None,
        sys_prompts: vec![Content {
            role: crate::ai::llm::Role::System,
            message: message.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }],
        dynamic_prompts: std::collections::LinkedList::new(),
        prompts_url: "".to_string(),
        llm_chat_url: "".to_string(),
        history: 5,
        mcp_server: vec![],
    };

    let now = tokio::time::Instant::now();
    let _prompts = llm_config.prompts().await;
    let elapsed = now.elapsed();
    println!("First Time elapsed to load prompts: {:?}", elapsed);

    let now = tokio::time::Instant::now();
    let prompts = llm_config.prompts().await;
    let elapsed = now.elapsed();
    println!("Second Time elapsed to load prompts: {:?}", elapsed);

    assert!(!prompts.sys_prompts.first().unwrap().message.contains("{{"));
    assert!(prompts.sys_prompts.first().unwrap().message.len() > message.len());
}

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
