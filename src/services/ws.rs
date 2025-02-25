use std::{collections::HashMap, sync::Arc, vec};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path,
    },
    response::IntoResponse,
    Extension,
};

pub enum WsCommand {
    AsrResult(Vec<String>),
    Action { action: String, text: String },
    Audio(Bytes),
    StartAudio,
    EndAudio,
    Video(Vec<Vec<u8>>),
}
type WsTx = tokio::sync::mpsc::UnboundedSender<WsCommand>;
type WsRx = tokio::sync::mpsc::UnboundedReceiver<WsCommand>;

#[derive(Debug, Default)]
pub struct WsPool {
    pub connections: tokio::sync::RwLock<HashMap<String, WsTx>>,
}

impl WsPool {
    pub async fn send(&self, id: &str, cmd: WsCommand) -> anyhow::Result<()> {
        let pool = self.connections.read().await;
        let ws_tx = pool
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("`{id}` not found"))?;
        ws_tx.send(cmd)?;

        Ok(())
    }
}

pub async fn ws_handler(
    Extension(pool): Extension<Arc<WsPool>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    // user_agent: Option<TypedHeader<headers::UserAgent>>,
) -> impl IntoResponse {
    log::info!("{id} connected.");

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WsCommand>();
    {
        pool.connections.write().await.insert(id.clone(), tx);
    }
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, &id, rx, pool).await {
            log::error!("`{id}` error: {e}");
        };
    })
}

enum WsEvent {
    Message(anyhow::Result<Message>),
    Command(WsCommand),
}

async fn submit_to_ai(
    pool: Arc<WsPool>,
    id: &str,
    wav_audio: Vec<u8>,
    only_asr: bool,
) -> anyhow::Result<()> {
    use crate::ai::llm;
    // ASR
    let asr_url = "http://localhost:3001/v1/audio/transcriptions";
    let lang = "zh";
    let text = crate::ai::asr(asr_url, lang, wav_audio).await?;
    log::info!("ASR result: {:?}", text);

    if text.is_empty() {
        pool.send(id, WsCommand::AsrResult(vec![])).await?;
        return Ok(());
    }

    let message = text.join("\n");
    pool.send(id, WsCommand::AsrResult(text)).await?;

    if only_asr {
        return Ok(());
    }

    // LLM
    let token = std::env::var("API_KEY").ok().map(|k| format!("Bearer {k}"));
    let prompts = vec![llm::Content {
        role: llm::Role::User,
        message,
    }];

    let token = match &token {
        Some(t) => t.as_str(),
        None => "",
    };

    log::info!("start llm");
    let llm_url = "https://cloud.fastgpt.cn/api/v1/chat/completions";
    let mut resp =
        crate::ai::llm_stable(llm_url, token, Some("esp32-test-1".to_string()), prompts).await?;

    let mut deadline = None;

    loop {
        match resp.next_chunk().await {
            Ok(Some(chunk)) => {
                log::info!("start tts");
                match crate::ai::tts("http://localhost:3000/tts", "ht", &chunk).await {
                    Ok(wav_data) => {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline).await;
                            log::info!("end audio");
                        }

                        let mut buff = Vec::with_capacity(5 * 3200 * 2);
                        let reader = hound::WavReader::new(wav_data.as_ref())?;

                        let duration_sec =
                            reader.duration() as f32 / reader.spec().sample_rate as f32;

                        deadline = Some(
                            tokio::time::Instant::now()
                                + tokio::time::Duration::from_secs_f32(duration_sec + 1.5),
                        );

                        let mut samples = reader.into_samples::<i16>();

                        log::info!("llm chunk:{}", chunk);

                        pool.send(
                            id,
                            WsCommand::Action {
                                action: "".to_string(),
                                text: chunk.clone(),
                            },
                        )
                        .await?;

                        pool.send(id, WsCommand::StartAudio).await?;

                        'a: loop {
                            for _ in 0..(5 * 3200) {
                                if let Some(Ok(sample)) = samples.next() {
                                    buff.extend_from_slice(&sample.to_le_bytes());
                                } else {
                                    break 'a;
                                }
                            }
                            {
                                let mut send_data = Vec::with_capacity(buff.capacity());
                                std::mem::swap(&mut send_data, &mut buff);
                                pool.send(id, WsCommand::Audio(send_data.into())).await?;
                            }
                        }
                        if buff.len() > 0 {
                            let mut send_data = Vec::with_capacity(buff.capacity());
                            std::mem::swap(&mut send_data, &mut buff);
                            pool.send(id, WsCommand::Audio(send_data.into())).await?;
                        }
                        pool.send(id, WsCommand::EndAudio).await?;
                    }
                    Err(e) => {
                        log::error!("tts error:{e}");
                        break;
                    }
                }
            }
            Ok(None) => {
                log::info!("llm done");
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

async fn handle_socket(
    mut socket: WebSocket,
    id: &str,
    mut rx: WsRx,
    pool: Arc<WsPool>,
) -> anyhow::Result<()> {
    // handle the socket here
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer_writer = std::io::Cursor::new(Vec::new());
    let mut wav_writer = hound::WavWriter::new(&mut buffer_writer, spec)?;

    let cancel = Arc::new(tokio::sync::Notify::new());

    let mut submit = 0;
    loop {
        let r = tokio::select! {
            cmd = rx.recv() => {
                cmd.map(|cmd| WsEvent::Command(cmd))
            }
            message = socket.recv() => {
                message.map(|message| match message{
                    Ok(message) => WsEvent::Message(Ok(message)),
                    Err(e) => WsEvent::Message(Err(anyhow::anyhow!("ws error: {e}"))),
                })
            }
        };

        match r {
            Some(WsEvent::Command(cmd)) => {
                if let Err(e) = process_command(&mut socket, cmd).await {
                    log::error!("`{id}` process_command error: {e}");
                    break;
                }
            }
            Some(WsEvent::Message(Ok(msg))) => match process_message(id, msg) {
                ProcessMessageResult::Ok(d) => {
                    for data in d.chunks(2) {
                        if data.len() == 2 {
                            let sample = i16::from_le_bytes([data[0], data[1]]);
                            wav_writer.write_sample(sample)?;
                        }
                    }
                }
                ProcessMessageResult::Skip => {}
                ProcessMessageResult::Submit(s) => {
                    log::info!("`{id}` submit: {s}");
                    submit = s;
                }
                ProcessMessageResult::Close => {
                    break;
                }
            },
            Some(WsEvent::Message(Err(e))) => {
                log::error!("`{id}` error: {e}");
                break;
            }
            None => {
                break;
            }
        }

        if submit > 0 {
            cancel.notify_waiters();

            if wav_writer.finalize().is_err() {
                buffer_writer = std::io::Cursor::new(Vec::new());
                wav_writer = hound::WavWriter::new(&mut buffer_writer, spec)?;
                continue;
            }
            let wav_audio = buffer_writer.into_inner();
            buffer_writer = std::io::Cursor::new(Vec::new());
            wav_writer = hound::WavWriter::new(&mut buffer_writer, spec)?;
            let pool_ = pool.clone();
            let id_ = id.to_string();
            let cancel_ = cancel.clone();

            tokio::spawn(async move {
                tokio::select! {
                    _ = cancel_.notified() => {
                        log::info!("`{id_}` canceled.");
                        return;
                    }
                    r = submit_to_ai(pool_, &id_, wav_audio, submit==2) => {
                        if let Err(e) = r {
                            log::error!("`{id_}` error: {e}");
                        }
                    }
                }
            });
            submit = 0;
        }
    }
    log::info!("`{id}` disconnected.");
    // remove the connection from the pool
    {
        let mut pool = pool.connections.write().await;
        if !rx.is_closed() {
            pool.remove(id);
        }
    }
    Ok(())
}

pub const SAMPLE_RATE: u32 = 16000;
pub const SAMPLE_RATE_BUFFER_SIZE: usize = 2 * (SAMPLE_RATE as usize) / 10;

async fn process_command(ws: &mut WebSocket, cmd: WsCommand) -> anyhow::Result<()> {
    match cmd {
        WsCommand::AsrResult(texts) => {
            let json = serde_json::to_string(&crate::protocol::JsonCommand::ASR {
                text: texts.join("\n"),
            })
            .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(json.into())).await?;
        }

        WsCommand::Action { action, text } => {
            let json =
                serde_json::to_string(&crate::protocol::JsonCommand::Action { action, text })
                    .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(json.into())).await?;
        }
        WsCommand::StartAudio => {
            let start_audio = serde_json::to_string(&crate::protocol::JsonCommand::StartAudio)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(start_audio.into())).await?;
        }
        WsCommand::Audio(d) => {
            ws.send(Message::Binary(d)).await?;
        }
        WsCommand::EndAudio => {
            let end_audio = serde_json::to_string(&crate::protocol::JsonCommand::EndAudio)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(end_audio.into())).await?;
        }
        WsCommand::Video(_) => {
            log::warn!("video command is not implemented yet");
        }
    }
    Ok(())
}

enum ProcessMessageResult {
    Ok(Bytes),
    Submit(u8),
    Close,
    Skip,
}

fn process_message(id: &str, msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(t) => {
            if t.as_str() == "End:Normal" {
                ProcessMessageResult::Submit(1)
            } else if t.as_str() == "End:Interrupt" {
                ProcessMessageResult::Submit(2)
            } else {
                log::warn!("{id} received unexpected text message: {t}");
                ProcessMessageResult::Skip
            }
        }
        Message::Binary(d) => ProcessMessageResult::Ok(d),
        Message::Close(c) => {
            if let Some(cf) = c {
                log::info!(
                    "sent close with code {} and reason `{}`",
                    cf.code,
                    cf.reason
                );
            } else {
                log::info!("somehow sent close message without CloseFrame");
            }
            ProcessMessageResult::Close
        }

        Message::Pong(_) | Message::Ping(_) => ProcessMessageResult::Skip,
    }
}
