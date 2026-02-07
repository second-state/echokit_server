use std::{collections::HashMap, sync::Arc};

use axum::{
    Extension,
    extract::{Path, Query, WebSocketUpgrade},
    response::IntoResponse,
};

use crate::{
    ai::openai::tool::{McpToolAdapter, ToolSet},
    config::{ASRConfig, LLMConfig, TTSConfig},
    services::ws::{
        self,
        stable::{
            llm::{ChunksRx, LLMConfigExt, LLMExt},
            tts::TTSRequestTx,
        },
    },
};

mod asr;
pub mod claude;
pub mod gemini;
mod llm;
mod tts;

pub struct StableWsSetting {
    pub sessions: tokio::sync::mpsc::UnboundedSender<Session>,
    pub hello_wav: Option<Vec<u8>>,
}

pub async fn ws_handler(
    Extension(pool): Extension<Arc<StableWsSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(params): Query<super::ConnectQueryParams>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().as_u128();
    log::info!("[Chat] {id}:{request_id:x} connected. {:?}", params);

    ws.on_upgrade(move |socket| async move {
        let id = id.clone();
        let pool = pool.clone();
        if let Err(e) = handle_socket(socket, &id, request_id, pool.clone(), params).await {
            log::warn!("{id}:{request_id:x} handle_socket error: {e}");
        };
        log::info!("{id}:{request_id:x} disconnected.");
    })
}

async fn handle_socket(
    mut socket: axum::extract::ws::WebSocket,
    id: &str,
    request_id: u128,
    pool: Arc<StableWsSetting>,
    params: super::ConnectQueryParams,
) -> anyhow::Result<()> {
    log::info!("`{}` starting socket io processing", id);

    if let Some(hello_wav) = &pool.hello_wav {
        if !hello_wav.is_empty() && !params.reconnect {
            super::send_hello_wav(&mut socket, hello_wav).await?;
        }
    }

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (client_tx, client_rx) = tokio::sync::mpsc::channel(1024);

    pool.sessions
        .send(Session {
            id: id.to_string(),
            request_id,
            cmd_tx,
            client_rx,
            is_reconnect: params.reconnect,
            stream_asr: params.stream_asr,
        })
        .map_err(|e| anyhow::anyhow!("send session error: {}", e))?;

    super::process_socket_io(
        &mut cmd_rx,
        client_tx,
        &mut socket,
        ws::ConnectConfig {
            enable_opus: params.opus,
            vowel: params.vowel,
        },
    )
    .await?;
    Ok(())
}

pub struct Session {
    id: String,
    request_id: u128,
    cmd_tx: super::WsTx,
    client_rx: super::ClientRx,
    is_reconnect: bool,
    stream_asr: bool,
}

impl Session {
    pub fn send_asr_result(&self, texts: Vec<String>) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::AsrResult(texts))
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending asr result ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_start_audio(&self, text: String) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::StartAudio(text))
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending start audio ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_audio_chunk(&self, data: Vec<u8>) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::Audio(data))
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending audio chunk ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_choice_prompt(&self, message: String, choices: Vec<String>) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::Choices(message, choices))
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending choice prompt ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_notify(&self, message: String) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::Action { action: message })
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending notify ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_display_text(&self, text: String) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::DisplayText(text))
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending display text ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_end_audio(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(super::WsCommand::EndAudio).map_err(|_| {
            anyhow::anyhow!(
                "{}:{:x} error sending end audio ws command",
                self.id,
                self.request_id
            )
        })
    }

    pub fn send_end_response(&self) -> anyhow::Result<()> {
        self.cmd_tx
            .send(super::WsCommand::EndResponse)
            .map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending end response ws command",
                    self.id,
                    self.request_id
                )
            })
    }

    pub fn send_end_vad(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(super::WsCommand::EndVad).map_err(|_| {
            anyhow::anyhow!(
                "{}:{:x} error sending vad end ws command",
                self.id,
                self.request_id
            )
        })
    }
}

async fn run_session<S: llm::LLMExt + Send + 'static>(
    llm_session: &mut S,
    tts_req_tx: &mut TTSRequestTx,
    asr_session: &mut asr::AsrSession,
    session: &mut Session,
) -> anyhow::Result<()> {
    log::info!(
        "{}:{:x} starting session processing",
        session.id,
        session.request_id
    );

    loop {
        log::info!(
            "{}:{:x} waiting for asr input",
            session.id,
            session.request_id
        );
        let text = if session.stream_asr {
            let text = asr_session.stream_get_input(session).await;
            if text.is_err() {
                session.send_end_vad().map_err(|_| {
                    anyhow::anyhow!(
                        "{}:{:x} error sending end vad ws command after asr error",
                        session.id,
                        session.request_id
                    )
                })?;
            }
            text?
        } else {
            asr_session
                .get_input(&session.id, &mut session.client_rx)
                .await?
        };
        if text.is_empty() {
            log::info!(
                "{}:{:x} empty asr result, ending session",
                session.id,
                session.request_id
            );

            session.send_end_response().map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending end response ws command for empty asr result",
                    session.id,
                    session.request_id
                )
            })?;

            continue;
        } else {
            log::info!(
                "{}:{:x} asr result: {}",
                session.id,
                session.request_id,
                text
            );
            session.send_asr_result(vec![text.clone()]).map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending asr result ws command for message `{}`",
                    session.id,
                    session.request_id,
                    text
                )
            })?;
        }

        let (chunks_tx, chunks_rx) = tokio::sync::mpsc::unbounded_channel();

        log::info!(
            "{}:{:x} started llm and tts handling for this input",
            session.id,
            session.request_id
        );

        // let llm_fut = llm::chat(tts_req_tx, chunks_tx, llm_session, text);
        let llm_fut = llm_session.handle_asr_result(tts_req_tx, chunks_tx, text);
        let send_audio_fut = handle_tts_requests(chunks_rx, session);

        let r = tokio::try_join!(llm_fut, send_audio_fut);
        if let Err(e) = r {
            log::error!(
                "{}:{:x} error during llm or tts handling: {}",
                session.id,
                session.request_id,
                e
            );
        } else {
            log::info!(
                "{}:{:x} session processing done for this input",
                session.id,
                session.request_id
            );
        }

        session.send_end_response().map_err(|_| {
            anyhow::anyhow!(
                "{}:{:x} error sending end response ws command after session processing",
                session.id,
                session.request_id
            )
        })?;
    }
}

async fn handle_tts_requests(mut chunks_rx: ChunksRx, session: &mut Session) -> anyhow::Result<()> {
    while let Some((chunk, mut tts_resp_rx)) = chunks_rx.recv().await {
        log::info!(
            "{}:{:x} starting tts for chunk: {}",
            session.id,
            session.request_id,
            chunk
        );

        session.send_start_audio(chunk.clone()).map_err(|_| {
            anyhow::anyhow!(
                "{}:{:x} error sending start audio ws command for chunk `{}`",
                session.id,
                session.request_id,
                chunk
            )
        })?;

        while let Some(tts_chunk) = tts_resp_rx.recv().await {
            log::trace!(
                "{}:{:x} sending tts audio chunk of size {}",
                session.id,
                session.request_id,
                tts_chunk.len()
            );

            if tts_chunk.is_empty() {
                continue;
            }

            session.send_audio_chunk(tts_chunk.clone()).map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending audio chunk ws command for tts chunk",
                    session.id,
                    session.request_id
                )
            })?;
        }

        session.send_end_audio().map_err(|_| {
            anyhow::anyhow!(
                "{}:{:x} error sending end audio ws command after tts chunk",
                session.id,
                session.request_id
            )
        })?;

        log::info!(
            "{}:{:x} finished tts for chunk: {}",
            session.id,
            session.request_id,
            chunk
        );
    }
    Ok(())
}

pub async fn run_session_manager(
    llm: &LLMConfig,
    tts: &TTSConfig,
    asr: &ASRConfig,
    tools: &ToolSet<McpToolAdapter>,
    mut session_rx: tokio::sync::mpsc::UnboundedReceiver<Session>,
) -> anyhow::Result<()> {
    let mut sessions: HashMap<
        String,
        tokio::sync::mpsc::UnboundedSender<(Session, Option<llm::MixPrompts>)>,
    > = HashMap::new();

    let mut tts_session_pool = tts::TTSSessionPool::new(tts.clone(), 4);
    let (tts_req_tx, tts_req_rx) = tokio::sync::mpsc::channel(128);

    tokio::spawn(async move {
        if let Err(e) = tts_session_pool.run_loop(tts_req_rx).await {
            log::error!("tts session pool exit by error: {}", e);
        }
    });

    while let Some(session) = session_rx.recv().await {
        let prompts;
        if !session.is_reconnect {
            prompts = Some(llm.get_prompts().await)
        } else {
            prompts = None
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

        // device reconnects but server restarted
        if prompts.is_none() {
            prompts = Some(llm.get_prompts().await);
        }

        // run session
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let id = session.id.clone();
        log::info!("Starting new session for id: {}", id);
        let _ = tx.send((session, prompts));
        let asr = asr.clone();

        let mut chat_session = llm::LLMSession::init_session(&llm, tools.clone());

        sessions.insert(id.clone(), tx);

        let mut tts_req_tx = tts_req_tx.clone();

        tokio::spawn(async move {
            let mut asr_session = asr::AsrSession::new_from_config(&asr).await.map_err(|e| {
                log::error!("error creating asr session for id `{}`: {}", id, e);
                anyhow::anyhow!("error creating asr session for id `{}`: {}", id, e)
            })?;

            let (mut session, mut prompts) = rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("no session received for id `{}`", id))?;

            loop {
                log::info!("Running session for id `{}`", id);
                // If prompts exist, set them to chat session. This case is happening when reconnect is false.
                if let Some(prompts) = prompts.take() {
                    chat_session.set_prompts(prompts);
                }

                let run_fut = run_session(
                    &mut chat_session,
                    &mut tts_req_tx,
                    &mut asr_session,
                    &mut session,
                );

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

                session.cmd_tx.send(super::WsCommand::EndResponse).ok();

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
