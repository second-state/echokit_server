use std::collections::HashMap;

use crate::{
    ai::gemini,
    config::{GeminiConfig, TTSConfig},
    services::ws::{ClientMsg, WsCommand},
};

use super::Session;

// not support yet
pub struct GeminiPrompts;

pub struct GeminiSession {
    config: GeminiConfig,
    client: gemini::LiveClient,
}

pub async fn run_session_manager(
    gemini: &GeminiConfig,
    tts: Option<&TTSConfig>,
    mut session_rx: tokio::sync::mpsc::UnboundedReceiver<Session>,
) -> anyhow::Result<()> {
    let mut sessions: HashMap<
        String,
        tokio::sync::mpsc::UnboundedSender<(Session, Option<GeminiPrompts>)>,
    > = HashMap::new();

    let tts_req_tx = if let Some(tts) = tts {
        let mut tts_session_pool = super::tts::TTSSessionPool::new(tts.clone(), 4);
        let (tts_req_tx, tts_req_rx) = tokio::sync::mpsc::channel(128);

        tokio::spawn(async move {
            if let Err(e) = tts_session_pool.run_loop(tts_req_rx).await {
                log::error!("tts session pool exit by error: {}", e);
            }
        });

        Some(tts_req_tx)
    } else {
        None
    };

    while let Some(session) = session_rx.recv().await {
        let prompts;
        if !session.is_reconnect {
            prompts = Some(GeminiPrompts);
        } else {
            prompts = None;
        }

        let (session, mut prompts) = if let Some(tx) = sessions.get(&session.id) {
            if let Err(e) = tx.send((session, prompts)) {
                e.0
            } else {
                continue;
            }
        } else {
            (session, prompts)
        };
        // start new session

        if prompts.is_some() {
            prompts = None;
        }

        // run session
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let id = session.id.clone();
        log::info!("Starting new session for id: {}", id);
        let _ = tx.send((session, prompts));

        // let mut chat_session = llm::LLMSession::init_session(&llm, tools.clone());
        let mut client = gemini::LiveClient::connect(&gemini.api_key).await?;
        let model = gemini
            .model
            .clone()
            .unwrap_or("models/gemini-2.0-flash-exp".to_string());
        let mut generation_config = gemini::types::GenerationConfig::default();
        generation_config.response_modalities = if tts_req_tx.is_some() {
            Some(vec![gemini::types::Modality::TEXT])
        } else {
            Some(vec![gemini::types::Modality::AUDIO])
        };

        let system_instruction = if let Some(sys_prompts) = gemini.sys_prompts.first() {
            Some(gemini::types::Content {
                parts: vec![gemini::types::Parts::Text(sys_prompts.message.clone())],
            })
        } else {
            None
        };

        let setup = gemini::types::Setup {
            model,
            generation_config: Some(generation_config),
            system_instruction,
            input_audio_transcription: Some(gemini::types::AudioTranscriptionConfig {}),
            output_audio_transcription: if tts_req_tx.is_none() {
                Some(gemini::types::AudioTranscriptionConfig {})
            } else {
                None
            },
            realtime_input_config: Some(gemini::types::RealtimeInputConfig {
                automatic_activity_detection: Some(
                    gemini::types::AutomaticActivityDetectionConfig { disabled: true },
                ),
            }),
        };

        client.setup(setup.clone()).await?;

        let mut gemini_session = GeminiSession {
            config: gemini.clone(),
            client,
        };

        sessions.insert(id.clone(), tx);

        let mut tts_req_tx_ = tts_req_tx.clone();

        tokio::spawn(async move {
            let (mut session, mut prompts) = rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("no session received for id `{}`", id))?;

            loop {
                log::info!("Running session for id `{}`", id);
                // If prompts exist, set them to chat session. This case is happening when reconnect is false.
                if let Some(_) = prompts.take() {
                    log::info!("Setting up new client for session id `{}`", id);
                    if let Ok(mut client) =
                        gemini::LiveClient::connect(&gemini_session.config.api_key).await
                    {
                        client.setup(setup.clone()).await.ok();
                        gemini_session.client = client;
                    }
                }

                let run_fut = async {
                    if let Some(tts_req_tx) = tts_req_tx_.as_mut() {
                        run_session_with_tts(&mut gemini_session, tts_req_tx, &mut session).await
                    } else {
                        run_session(&mut gemini_session, &mut session).await
                    }
                };

                // Wait for either the session to complete or a new connection for the same id (interrupt)
                let result = tokio::select! {
                    res = run_fut => {
                        Ok(res)
                    },
                    // interrupted by new session
                    new_session = rx.recv() => {
                        Err(new_session)
                    }
                };

                session.cmd_tx.send(WsCommand::EndResponse).ok();

                match result {
                    Ok(Ok(())) => {
                        log::info!("session for id `{}` completed successfully", id);
                    }
                    Ok(Err(e)) => {
                        log::error!("session for id `{}` error: {}", id, e);
                    }
                    Err(Some((new_session, new_prompts))) => {
                        log::info!("received new session for id `{}`, restarting session", id);
                        session = new_session;
                        prompts = new_prompts;
                        continue;
                    }
                    Err(None) => {
                        log::info!("no more sessions for id `{}`, exiting", id);
                        break;
                    }
                }

                match rx.recv().await {
                    Some(s) => {
                        session = s.0;
                        prompts = s.1;
                    }
                    None => {
                        log::info!("no more sessions for id `{}`, exiting", id);
                        break;
                    }
                };
            }

            anyhow::Result::<()>::Ok(())
        });
    }
    log::warn!("session manager exiting");
    Ok(())
}

async fn run_session(
    gemini_session: &mut GeminiSession,
    session: &mut Session,
) -> anyhow::Result<()> {
    log::info!(
        "{}:{:x} starting session processing",
        session.id,
        session.request_id
    );

    enum GeminiEvent {
        AudioChunk(ClientMsg),
        ServerEvent(gemini::types::ServerContent),
    }

    let mut recv = session
        .client_rx
        .recv()
        .await
        .map(|r| GeminiEvent::AudioChunk(r))
        .ok_or_else(|| anyhow::anyhow!("audio channel closed"))?;

    let mut llm_text = String::with_capacity(1024);
    let mut asr_text = String::with_capacity(1024);

    loop {
        match recv {
            GeminiEvent::AudioChunk(client_msg) => match client_msg {
                ClientMsg::StartChat => {
                    log::info!("{}:{:x} received StartChat", session.id, session.request_id);
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::ActivityStart {})
                        .await?;
                }
                ClientMsg::AudioChunk(sample) => {
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::Audio(
                            gemini::types::RealtimeAudio {
                                data: gemini::types::Blob::new(sample.to_vec()),
                                mime_type: "audio/pcm;rate=16000".to_string(),
                            },
                        ))
                        .await?;
                }
                ClientMsg::Submit => {
                    // gemini_session
                    //     .client
                    //     .send_realtime_input(gemini::types::RealtimeInput::AudioStreamEnd(true))
                    //     .await?;
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::ActivityEnd {})
                        .await?;
                    log::info!(
                        "{}:{:x} sent AudioStreamEnd",
                        session.id,
                        session.request_id
                    );
                }
                ClientMsg::Text(input) => {
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::Text(input))
                        .await?;
                }
                ClientMsg::Select(_) => {}
            },
            GeminiEvent::ServerEvent(server_content) => match server_content {
                gemini::types::ServerContent::ModelTurn(turn) => {
                    if !asr_text.is_empty() {
                        let asr = hanconv::tw2sp(&asr_text);
                        log::info!("gemini input transcription: {asr}");
                        session.cmd_tx.send(WsCommand::AsrResult(vec![asr]))?;
                        asr_text.clear();
                    }

                    for item in turn.parts {
                        match item {
                            gemini::types::Parts::Text(text_part) => {
                                llm_text.push_str(&text_part);
                            }
                            gemini::types::Parts::InlineData { data, mime_type } => {
                                if mime_type.starts_with("audio/pcm") {
                                    let audio_data = data.into_inner();
                                    let mut audio_samples =
                                        Vec::with_capacity(audio_data.len() / 2);

                                    if audio_data.len() % 2 != 0 {
                                        log::warn!(
                                            "Received audio chunk with odd length, skipping"
                                        );
                                        for i in audio_data[0..audio_data.len() - 1].chunks_exact(2)
                                        {
                                            let sample_value = i16::from_le_bytes([i[0], i[1]]);
                                            audio_samples
                                                .push(sample_value as f32 / i16::MAX as f32);
                                        }
                                    } else {
                                        for i in audio_data.chunks_exact(2) {
                                            let sample_value = i16::from_le_bytes([i[0], i[1]]);
                                            audio_samples
                                                .push(sample_value as f32 / i16::MAX as f32);
                                        }
                                    }

                                    let audio_16k =
                                        wav_io::resample::linear(audio_samples, 1, 24000, 16000);
                                    let samples =
                                        crate::util::convert_samples_f32_to_i16_bytes(&audio_16k);

                                    session.cmd_tx.send(WsCommand::Audio(samples)).map_err(
                                        |_| {
                                            anyhow::anyhow!(
                                                "{}:{:x} failed to send audio chunk",
                                                session.id,
                                                session.request_id
                                            )
                                        },
                                    )?;
                                }
                            }
                        }
                    }
                }
                gemini::types::ServerContent::GenerationComplete(_) => {}
                gemini::types::ServerContent::Interrupted(_) => {}
                gemini::types::ServerContent::TurnComplete(_) => {
                    session.cmd_tx.send(WsCommand::EndAudio)?;
                    llm_text.clear();
                    session.cmd_tx.send(WsCommand::EndResponse)?;
                }
                gemini::types::ServerContent::InputTranscription { text } => {
                    asr_text.push_str(&text);
                }
                gemini::types::ServerContent::OutputTranscription { text } => {
                    let message = hanconv::tw2sp(text);
                    llm_text.push_str(&message);
                    session
                        .cmd_tx
                        .send(WsCommand::StartAudio(llm_text.clone()))?;
                }
                gemini::types::ServerContent::Timeout => {}
                gemini::types::ServerContent::GoAway {} => {
                    log::warn!("gemini GoAway");
                    session.cmd_tx.send(WsCommand::Action {
                        action: "ByeBye".to_string(),
                    })?;
                    return Err(anyhow::anyhow!("Gemini GoAway"));
                }
            },
        }

        let recv_ = {
            tokio::select! {
                r = session.client_rx.recv() => {
                    Ok(r.map(|r|GeminiEvent::AudioChunk(r)).ok_or_else(||anyhow::anyhow!("audio channel closed"))?)
                }
                r = gemini_session.client.receive() => {
                    r.map(|r|GeminiEvent::ServerEvent(r))
                }
            }
        };

        if let Err(e) = recv_ {
            log::error!("gemini connect error: {e}");
            if let Err(e) = session.cmd_tx.send(WsCommand::AsrResult(vec![])) {
                log::error!("send error: {e}");
            }
            return Ok(());
        }
        recv = recv_.unwrap();
    }
}

async fn run_session_with_tts(
    gemini_session: &mut GeminiSession,
    tts_req_tx: &mut super::tts::TTSRequestTx,
    session: &mut Session,
) -> anyhow::Result<()> {
    log::info!(
        "{}:{:x} starting session processing",
        session.id,
        session.request_id
    );

    enum GeminiEvent {
        AudioChunk(ClientMsg),
        ServerEvent(gemini::types::ServerContent),
    }

    let mut recv = session
        .client_rx
        .recv()
        .await
        .map(|r| GeminiEvent::AudioChunk(r))
        .ok_or_else(|| anyhow::anyhow!("audio channel closed"))?;

    let mut llm_text = String::with_capacity(1024);
    let mut asr_text = String::with_capacity(1024);

    loop {
        match recv {
            GeminiEvent::AudioChunk(client_msg) => match client_msg {
                ClientMsg::StartChat => {
                    log::info!("{}:{:x} received StartChat", session.id, session.request_id);
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::ActivityStart {})
                        .await?;
                }
                ClientMsg::AudioChunk(sample) => {
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::Audio(
                            gemini::types::RealtimeAudio {
                                data: gemini::types::Blob::new(sample.to_vec()),
                                mime_type: "audio/pcm;rate=16000".to_string(),
                            },
                        ))
                        .await?;
                }
                ClientMsg::Submit => {
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::ActivityEnd {})
                        .await?;
                    log::info!(
                        "{}:{:x} sent AudioStreamEnd",
                        session.id,
                        session.request_id
                    );
                }
                ClientMsg::Text(input) => {
                    gemini_session
                        .client
                        .send_realtime_input(gemini::types::RealtimeInput::Text(input))
                        .await?;
                }
                ClientMsg::Select(_) => {}
            },
            GeminiEvent::ServerEvent(server_content) => match server_content {
                gemini::types::ServerContent::ModelTurn(turn) => {
                    if !asr_text.is_empty() {
                        let asr = hanconv::tw2sp(&asr_text);
                        log::info!("gemini input transcription: {asr}");
                        session.cmd_tx.send(WsCommand::AsrResult(vec![asr]))?;
                        asr_text.clear();
                    }

                    for item in turn.parts {
                        match item {
                            gemini::types::Parts::Text(text_part) => {
                                llm_text.push_str(&text_part);
                            }
                            gemini::types::Parts::InlineData { .. } => {
                                #[cfg(debug_assertions)]
                                unreachable!("Should not receive inline data when TTS is enabled");
                                #[cfg(not(debug_assertions))]
                                {
                                    log::warn!(
                                        "Received inline data when TTS is enabled, ignoring"
                                    );
                                }
                            }
                        }
                    }
                }
                gemini::types::ServerContent::GenerationComplete(_) => {}
                gemini::types::ServerContent::Interrupted(_) => {}
                gemini::types::ServerContent::TurnComplete(_) => {
                    let (chunks_tx, chunks_rx) = tokio::sync::mpsc::unbounded_channel();
                    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                    tts_req_tx.send((llm_text.clone(), tx)).await?;
                    chunks_tx.send((llm_text.clone(), rx))?;
                    asr_text.clear();
                    llm_text = String::with_capacity(1024);
                    drop(chunks_tx);

                    super::handle_tts_requests(chunks_rx, session).await?;
                    session.cmd_tx.send(WsCommand::EndResponse)?;
                }
                gemini::types::ServerContent::InputTranscription { text } => {
                    asr_text.push_str(&text);
                }
                gemini::types::ServerContent::OutputTranscription { .. } => {
                    #[cfg(debug_assertions)]
                    unreachable!("Should not receive output transcription when TTS is enabled");
                    #[cfg(not(debug_assertions))]
                    {
                        log::warn!("Received output transcription when TTS is enabled, ignoring");
                    }
                }
                gemini::types::ServerContent::Timeout => {}
                gemini::types::ServerContent::GoAway {} => {
                    log::warn!("gemini GoAway");
                    session.cmd_tx.send(WsCommand::Action {
                        action: "ByeBye".to_string(),
                    })?;
                    return Err(anyhow::anyhow!("Gemini GoAway"));
                }
            },
        }

        let recv_ = {
            tokio::select! {
                r = session.client_rx.recv() => {
                    Ok(r.map(|r|GeminiEvent::AudioChunk(r)).ok_or_else(||anyhow::anyhow!("audio channel closed"))?)
                }
                r = gemini_session.client.receive() => {
                    r.map(|r|GeminiEvent::ServerEvent(r))
                }

            }
        };

        if let Err(e) = recv_ {
            log::error!("gemini connect error: {e}");
            if let Err(e) = session.cmd_tx.send(WsCommand::AsrResult(vec![])) {
                log::error!("send error: {e}");
            }
            return Ok(());
        }
        recv = recv_.unwrap();
    }
}
