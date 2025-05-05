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

use crate::{ai::llm::Content, config::Config};

pub enum WsCommand {
    AsrResult(Vec<String>),
    Action { action: String },
    Audio(Bytes),
    StartAudio(String),
    EndAudio,
    Video(Vec<Vec<u8>>),
    InitSetting,
}
type WsTx = tokio::sync::mpsc::UnboundedSender<WsCommand>;
type WsRx = tokio::sync::mpsc::UnboundedReceiver<WsCommand>;

#[derive(Debug)]
pub struct WsPool {
    pub config: Config,
    pub connections: tokio::sync::RwLock<HashMap<String, (u128, WsTx)>>,
}

impl WsPool {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            connections: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl WsPool {
    pub async fn send(&self, id: &str, cmd: WsCommand) -> anyhow::Result<()> {
        let pool = self.connections.read().await;
        let ws_tx = pool
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("`{id}` not found"))?;
        ws_tx.1.send(cmd)?;

        Ok(())
    }
}

pub async fn ws_handler(
    Extension(pool): Extension<Arc<WsPool>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().as_u128();
    log::info!("{id}:{request_id:x} connected.");

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WsCommand>();
    {
        pool.connections
            .write()
            .await
            .insert(id.clone(), (request_id, tx));
    }

    ws.on_upgrade(move |socket| async move {
        let id = id.clone();
        let pool = pool.clone();
        if let Err(e) = handle_socket(socket, &id, rx, pool.clone()).await {
            log::error!("{id}:{request_id:x} error: {e}");
        };
        log::info!("{id}:{request_id:x} disconnected.");
        {
            let mut pool = pool.connections.write().await;
            let (uuid_, _) = pool.get(&id).unwrap();
            if request_id == *uuid_ {
                pool.remove(&id);
            }
        }
    })
}

enum WsEvent {
    Message(anyhow::Result<Message>),
    Command(WsCommand),
}

async fn submit_to_ai(
    pool: &WsPool,
    id: &str,
    wav_audio: Vec<u8>,
    only_asr: bool,
    sys_prompts: &[Content],
    dynamic_prompts: &mut std::collections::LinkedList<Content>,
) -> anyhow::Result<()> {
    // ASR
    let asr_url = &pool.config.asr.url;
    // let lang = "zh";
    let lang = pool.config.asr.lang.as_str();
    std::fs::write(format!("asr.{id}.wav"), &wav_audio).unwrap();
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
    let token = if let Some(t) = &pool.config.llm.api_key {
        t.as_str()
    } else {
        ""
    };

    if matches!(
        dynamic_prompts.back(),
        Some(Content {
            role: crate::ai::llm::Role::User,
            ..
        })
    ) {
        dynamic_prompts.pop_back();
    }

    while dynamic_prompts.len() > pool.config.llm.history * 2 {
        dynamic_prompts.pop_front();
    }

    dynamic_prompts.push_back(Content {
        role: crate::ai::llm::Role::User,
        message,
    });

    let prompts = sys_prompts
        .iter()
        .chain(dynamic_prompts.iter())
        .collect::<Vec<_>>();

    log::info!("start llm");
    let llm_url = &pool.config.llm.llm_chat_url;
    let mut resp = crate::ai::llm_stable(llm_url, token, None, prompts).await?;

    let mut deadline = None;

    let (tts_url, speaker) = match &pool.config.tts {
        crate::config::TTSConfig::Stable(tts) => (&tts.url, &tts.speaker),
        crate::config::TTSConfig::Fish(_) => {
            return Err(anyhow::anyhow!("Fish TTS is not implemented yet"));
        }
    };

    let mut llm_response = String::with_capacity(128);

    let mut first_chunk = true;

    loop {
        match resp.next_chunk().await {
            Ok(Some(chunk)) => {
                log::info!("start tts: {chunk:?}");

                let chunk_ = chunk.trim();
                log::debug!("llm chunk: {chunk_:?}");
                if first_chunk && chunk_.starts_with("[") && chunk_.ends_with("]") {
                    first_chunk = false;
                    let action = chunk[1..chunk.len() - 1].to_string();
                    log::info!("llm action: {action}");
                    pool.send(id, WsCommand::Action { action }).await?;
                    continue;
                }
                llm_response.push_str(&chunk);
                if chunk_.is_empty() {
                    continue;
                }
                match crate::ai::tts(tts_url, speaker, &chunk).await {
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
                                + tokio::time::Duration::from_secs_f32(duration_sec + 1.0),
                        );

                        let mut samples = reader.into_samples::<i16>();

                        log::info!("llm chunk:{:?}", chunk);

                        pool.send(id, WsCommand::StartAudio(chunk)).await?;

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
                        continue;
                    }
                }
            }
            Ok(None) => {
                log::info!("llm done");
                if !llm_response.is_empty() {
                    dynamic_prompts.push_back(Content {
                        role: crate::ai::llm::Role::Assistant,
                        message: llm_response,
                    });
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

// return: wav data
async fn process_socket_io(
    rx: &mut WsRx,
    socket: &mut WebSocket,
) -> anyhow::Result<(bool, Vec<u8>)> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer_writer = std::io::Cursor::new(Vec::new());
    let mut wav_writer = hound::WavWriter::new(&mut buffer_writer, spec)?;

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
            Some(WsEvent::Command(cmd)) => process_command(socket, cmd).await?,
            Some(WsEvent::Message(Ok(msg))) => match process_message(msg) {
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
                    wav_writer
                        .finalize()
                        .map_err(|e| anyhow::anyhow!("wav finalize error: {e}"))?;
                    return Ok((s == 2, buffer_writer.into_inner()));
                }
                ProcessMessageResult::Close => {
                    return Err(anyhow::anyhow!("ws close"));
                }
            },
            Some(WsEvent::Message(Err(e))) => {
                return Err(anyhow::anyhow!("ws error: {e}"));
            }
            None => {
                return Err(anyhow::anyhow!("ws channel close"));
            }
        }
    }
}

async fn handle_audio(
    id: String,
    pool: Arc<WsPool>,
    mut rx: tokio::sync::mpsc::Receiver<(bool, Vec<u8>)>,
) -> anyhow::Result<()> {
    let (mut only_asr, mut wav_audio) = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("handle_audio rx closed"))?;

    let sys_prompts = &pool.config.llm.sys_prompts;
    let mut dynamic_prompts = pool.config.llm.dynamic_prompts.clone();

    loop {
        let rx_recv = tokio::select! {
            r = rx.recv() =>{
                r
            }
            r = submit_to_ai(&pool, &id, wav_audio, only_asr,sys_prompts,&mut dynamic_prompts) => {
                if let Err(e) = r {
                    log::error!("`{id}` error: {e}");
                }
                rx.recv().await
            }
        };
        let r = rx_recv.ok_or_else(|| anyhow::anyhow!("handle_audio rx closed"))?;
        only_asr = r.0;
        wav_audio = r.1;
    }
}

async fn handle_socket(
    mut socket: WebSocket,
    id: &str,
    mut rx: WsRx,
    pool: Arc<WsPool>,
) -> anyhow::Result<()> {
    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<(bool, Vec<u8>)>(1);
    tokio::spawn(handle_audio(id.to_string(), pool.clone(), audio_rx));

    loop {
        let wav_audio = process_socket_io(&mut rx, &mut socket).await;
        match wav_audio {
            Ok((only_asr, wav_audio)) => {
                audio_tx
                    .send((only_asr, wav_audio))
                    .await
                    .map_err(|_| anyhow::anyhow!("{id} audio_tx closed"))?;
            }
            Err(e) => {
                log::error!("`{id}` process_socket_io error: {e}");
                break;
            }
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

        WsCommand::Action { action } => {
            let json = serde_json::to_string(&crate::protocol::JsonCommand::Action { action })
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(json.into())).await?;
        }
        WsCommand::StartAudio(text) => {
            let start_audio =
                serde_json::to_string(&crate::protocol::JsonCommand::StartAudio { text })
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
        WsCommand::InitSetting => {
            let init_setting = serde_json::to_string(&crate::protocol::JsonCommand::InitSetting)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(init_setting.into())).await?;
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

fn process_message(msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(t) => {
            if t.as_str() == "End:Normal" {
                ProcessMessageResult::Submit(1)
            } else if t.as_str() == "End:Interrupt" {
                ProcessMessageResult::Submit(2)
            } else {
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
