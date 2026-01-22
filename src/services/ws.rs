use std::{collections::HashMap, vec};

use axum::{
    body::Bytes,
    extract::ws::{Message, WebSocket},
};

use crate::{
    ai::openai::tool::{McpToolAdapter, ToolSet},
    config::AIConfig,
    util::WavConfig,
};

pub mod stable;

pub enum WsCommand {
    AsrResult(Vec<String>),
    Action {
        action: String,
    },
    /// 16k, 16bit le, single-channel audio data
    Audio(Vec<u8>),
    StartAudio(String),
    EndAudio,
    Video(Vec<Vec<u8>>),
    EndResponse,
    EndVad,
}
type WsTx = tokio::sync::mpsc::UnboundedSender<WsCommand>;
type WsRx = tokio::sync::mpsc::UnboundedReceiver<WsCommand>;

type ClientTx = tokio::sync::mpsc::Sender<ClientMsg>;
type ClientRx = tokio::sync::mpsc::Receiver<ClientMsg>;

type CtrlTx = tokio::sync::mpsc::Sender<(WsTx, ClientRx)>;

#[derive(Debug)]
pub struct WsSetting {
    pub config: AIConfig,
    pub hello_wav: Option<Vec<u8>>,
    pub tool_set: ToolSet<McpToolAdapter>,

    pub sessions: tokio::sync::Mutex<HashMap<String, CtrlTx>>,
}

impl WsSetting {
    pub fn new(
        hello_wav: Option<Vec<u8>>,
        config: AIConfig,
        tool_set: ToolSet<McpToolAdapter>,
    ) -> Self {
        Self {
            config,
            hello_wav,
            tool_set,
            sessions: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ConnectQueryParams {
    #[serde(default)]
    pub reconnect: bool,
    #[serde(default)]
    pub opus: bool,
    #[serde(default)]
    pub vowel: bool,
    #[serde(default)]
    pub stream_asr: bool,
}

enum WsEvent {
    Message(anyhow::Result<Message>),
    Command(WsCommand),
}

async fn retry_asr(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    model: &str,
    lang: &str,
    prompt: &str,
    wav_audio: Vec<u8>,
    retry: usize,
    timeout: std::time::Duration,
) -> Vec<String> {
    for i in 0..retry {
        let r = tokio::time::timeout(
            timeout,
            crate::ai::asr(client, url, api_key, model, lang, prompt, wav_audio.clone()),
        )
        .await;
        match r {
            Ok(Ok(v)) => return v,
            Ok(Err(e)) => {
                log::error!("asr error: {e}");
                continue;
            }
            Err(_) => {
                log::error!("asr timeout, retry {i}");
                continue;
            }
        }
    }
    vec![]
}

pub enum ClientMsg {
    StartChat,
    /// 16000 16bit le
    AudioChunk(Bytes),
    Submit,
    Text(String),
}

pub struct ConnectConfig {
    pub enable_opus: bool,
    pub vowel: bool,
}

// return: wav data
async fn process_socket_io(
    rx: &mut WsRx,
    audio_tx: ClientTx,
    socket: &mut WebSocket,
    config: ConnectConfig,
) -> anyhow::Result<()> {
    let mut opus_encoder =
        opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| anyhow::anyhow!("opus encoder error: {e}"))?;
    let mut ret_audio = Vec::with_capacity(sample_120ms(SAMPLE_RATE));

    const DEBUG_WAV: bool = std::option_env!("DEBUG_WAV").is_some();

    let mut debug_wav_data = bytes::BytesMut::new();

    loop {
        let r = tokio::select! {
            cmd = rx.recv() => {
                cmd.map(|cmd| WsEvent::Command(cmd))
            }
            message = socket.recv() => {
                message.map(|message| match message {
                    Ok(message) => WsEvent::Message(Ok(message)),
                    Err(e) => WsEvent::Message(Err(anyhow::anyhow!("recv ws error: {e}"))),
                })
            }
        };

        match r {
            Some(WsEvent::Command(cmd)) => {
                if config.enable_opus {
                    process_command_with_opus(
                        socket,
                        cmd,
                        &mut opus_encoder,
                        &mut ret_audio,
                        config.vowel,
                    )
                    .await?
                } else {
                    process_command(socket, cmd).await?
                }
            }
            Some(WsEvent::Message(Ok(msg))) => match process_message(msg) {
                ProcessMessageResult::Audio(d) => {
                    if DEBUG_WAV {
                        debug_wav_data.extend_from_slice(&d);
                    }
                    audio_tx
                        .send(ClientMsg::AudioChunk(d))
                        .await
                        .map_err(|_| anyhow::anyhow!("audio_tx closed"))?
                }
                ProcessMessageResult::Submit => {
                    audio_tx
                        .send(ClientMsg::Submit)
                        .await
                        .map_err(|_| anyhow::anyhow!("audio_tx closed"))?;
                }
                ProcessMessageResult::Text(input) => audio_tx
                    .send(ClientMsg::Text(input))
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Skip => {}
                ProcessMessageResult::StartChat => {
                    audio_tx
                        .send(ClientMsg::StartChat)
                        .await
                        .map_err(|_| anyhow::anyhow!("audio_tx closed"))?;

                    if DEBUG_WAV {
                        if !debug_wav_data.is_empty() {
                            let wav_data = crate::util::pcm_to_wav(
                                &debug_wav_data,
                                WavConfig {
                                    channels: 1,
                                    sample_rate: 16000,
                                    bits_per_sample: 16,
                                },
                            );
                            log::debug!(
                                "Writing pre-chat debug wav file with size: {} bytes",
                                wav_data.len()
                            );
                            std::fs::write("./recv_input.wav", wav_data)?;
                            debug_wav_data.clear();
                        }
                    }
                }
                ProcessMessageResult::Close => {
                    return Err(anyhow::anyhow!("ws closed"));
                }
            },
            Some(WsEvent::Message(Err(e))) => {
                return Err(e);
            }
            None => {
                return Err(anyhow::anyhow!("ws channel closed"));
            }
        }
    }
}

async fn send_hello_wav(socket: &mut WebSocket, hello: &[u8]) -> anyhow::Result<()> {
    let hello_start = rmp_serde::to_vec(&crate::protocol::ServerEvent::HelloStart)
        .expect("Failed to serialize HelloStart ServerEvent");
    socket.send(Message::binary(hello_start)).await?;

    for chunk in hello.chunks(1024 * 2) {
        let hello_chunk = rmp_serde::to_vec(&crate::protocol::ServerEvent::HelloChunk {
            data: chunk.to_vec(),
        })
        .expect("Failed to serialize HelloChunk ServerEvent");
        socket.send(Message::binary(hello_chunk)).await?;
    }

    let hello_end = rmp_serde::to_vec(&crate::protocol::ServerEvent::HelloEnd)
        .expect("Failed to serialize HelloEnd ServerEvent");
    socket.send(Message::binary(hello_end)).await?;

    Ok(())
}

pub const SAMPLE_RATE: u32 = 16000;
pub const SAMPLE_RATE_BUFFER_SIZE: usize = 2 * (SAMPLE_RATE as usize) / 10;

pub const fn sample_120ms(sample_rate: u32) -> usize {
    (sample_rate as usize) * 12 / 100
}

pub const SAMPLE_RATE_120MS: usize = sample_120ms(SAMPLE_RATE);

async fn process_command(ws: &mut WebSocket, cmd: WsCommand) -> anyhow::Result<()> {
    match cmd {
        WsCommand::AsrResult(texts) => {
            let asr = rmp_serde::to_vec(&crate::protocol::ServerEvent::ASR {
                text: texts.join("\n"),
            })
            .expect("Failed to serialize ASR ServerEvent");
            ws.send(Message::binary(asr)).await?;
        }

        WsCommand::Action { action } => {
            let action = rmp_serde::to_vec(&crate::protocol::ServerEvent::Action { action })
                .expect("Failed to serialize Action ServerEvent");
            ws.send(Message::binary(action)).await?;
        }
        WsCommand::StartAudio(text) => {
            log::trace!("StartAudio: {text:?}");
            let start_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::StartAudio { text })
                .expect("Failed to serialize StartAudio ServerEvent");
            ws.send(Message::binary(start_audio)).await?;
        }
        WsCommand::Audio(data) => {
            log::trace!("Audio chunk size: {}", data.len());
            // 1s per chunk
            for chunk in data.chunks(SAMPLE_RATE_BUFFER_SIZE * 10) {
                let audio_chunk = rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk {
                    data: chunk.to_vec(),
                })
                .expect("Failed to serialize AudioChunk ServerEvent");
                ws.send(Message::binary(audio_chunk)).await?;
            }
        }
        WsCommand::EndAudio => {
            log::trace!("EndAudio");
            let end_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndAudio)
                .expect("Failed to serialize EndAudio ServerEvent");
            ws.send(Message::binary(end_audio)).await?;
        }
        WsCommand::Video(_) => {
            log::warn!("video command is not implemented yet");
        }
        WsCommand::EndResponse => {
            log::debug!("EndResponse");
            let end_response = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndResponse)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::binary(end_response)).await?;
        }
        WsCommand::EndVad => {
            log::debug!("EndVad");
            let end_vad = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndVad)
                .expect("Failed to serialize EndVad ServerEvent");
            ws.send(Message::binary(end_vad)).await?;
        }
    }
    Ok(())
}

fn vowel_from_i16(samples: &[i16]) -> Option<lip_sync::vowel::Vowel> {
    let samples = crate::util::convert_samples_i16_to_f32(&samples[..1024]);
    lip_sync::vowel::recognize_vowel_from_pcm(&samples, SAMPLE_RATE)
}

fn vowel_to_u8(vowel: Option<lip_sync::vowel::Vowel>) -> u8 {
    match vowel {
        Some(lip_sync::vowel::Vowel::A) => 1,
        Some(lip_sync::vowel::Vowel::E) => 2,
        Some(lip_sync::vowel::Vowel::I) => 3,
        Some(lip_sync::vowel::Vowel::O) => 4,
        Some(lip_sync::vowel::Vowel::U) => 5,
        _ => 0,
    }
}

async fn process_command_with_opus(
    ws: &mut WebSocket,
    cmd: WsCommand,
    opus_encode: &mut opus::Encoder,
    ret_audio: &mut Vec<i16>,
    vowel: bool,
) -> anyhow::Result<()> {
    match cmd {
        WsCommand::AsrResult(texts) => {
            let asr = rmp_serde::to_vec(&crate::protocol::ServerEvent::ASR {
                text: texts.join("\n"),
            })
            .expect("Failed to serialize ASR ServerEvent");
            ws.send(Message::binary(asr)).await?;
        }

        WsCommand::Action { action } => {
            let action = rmp_serde::to_vec(&crate::protocol::ServerEvent::Action { action })
                .expect("Failed to serialize Action ServerEvent");
            ws.send(Message::binary(action)).await?;
        }
        WsCommand::StartAudio(text) => {
            log::trace!("StartAudio: {text:?}");
            opus_encode
                .reset_state()
                .map_err(|e| anyhow::anyhow!("opus reset state error: {e}"))?;
            let start_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::StartAudio { text })
                .expect("Failed to serialize StartAudio ServerEvent");
            ws.send(Message::binary(start_audio)).await?;
        }
        WsCommand::Audio(data) => {
            log::trace!(
                "Audio chunk size: {}, ret_audio size: {}",
                data.len(),
                ret_audio.len()
            );
            for chunk in data.chunks_exact(2) {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                ret_audio.push(sample);
            }

            // 120ms per chunk
            for chunk in ret_audio.chunks(SAMPLE_RATE_120MS) {
                if chunk.len() < SAMPLE_RATE_120MS {
                    *ret_audio = chunk.to_vec();
                    break;
                }
                let vowel_u8;
                if vowel {
                    let v = vowel_from_i16(chunk);
                    vowel_u8 = vowel_to_u8(v);
                } else {
                    vowel_u8 = 0;
                }

                let data = opus_encode.encode_vec(chunk, 2 * SAMPLE_RATE_120MS / 3)?;

                if vowel {
                    let audio_chunk =
                        rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunkWithVowel {
                            data,
                            vowel: vowel_u8,
                        })
                        .expect("Failed to serialize AudioChunkWithVowel ServerEvent");
                    ws.send(Message::binary(audio_chunk)).await?;
                } else {
                    let audio_chunk =
                        rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk { data })
                            .expect("Failed to serialize AudioChunk ServerEvent");
                    ws.send(Message::binary(audio_chunk)).await?;
                }
            }

            ret_audio.clear();
        }
        WsCommand::EndAudio => {
            log::trace!("EndAudio");
            if !ret_audio.is_empty() {
                let padded_audio_len = SAMPLE_RATE_120MS - ret_audio.len();
                ret_audio.extend(std::iter::repeat(0i16).take(padded_audio_len));

                let vowel_u8;
                if vowel {
                    let v = vowel_from_i16(ret_audio);
                    vowel_u8 = vowel_to_u8(v);
                } else {
                    vowel_u8 = 0;
                }

                let data = opus_encode.encode_vec(&ret_audio, 2 * SAMPLE_RATE_120MS / 3)?;
                if vowel {
                    let audio_chunk =
                        rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunkWithVowel {
                            data,
                            vowel: vowel_u8,
                        })
                        .expect("Failed to serialize AudioChunkWithVowel ServerEvent");
                    log::info!("Sending final audio chunk of size: {}", audio_chunk.len());
                    ws.send(Message::binary(audio_chunk)).await?;
                } else {
                    let audio_chunk =
                        rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk { data })
                            .expect("Failed to serialize AudioChunk ServerEvent");
                    log::info!("Sending final audio chunk of size: {}", audio_chunk.len());
                    ws.send(Message::binary(audio_chunk)).await?;
                }
                ret_audio.clear();
            }
            let end_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndAudio)
                .expect("Failed to serialize EndAudio ServerEvent");
            ws.send(Message::binary(end_audio)).await?;
        }
        WsCommand::Video(_) => {
            log::warn!("video command is not implemented yet");
        }
        WsCommand::EndResponse => {
            log::debug!("EndResponse");
            let end_response = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndResponse)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::binary(end_response)).await?;
        }
        WsCommand::EndVad => {
            log::debug!("EndVad");
            let end_vad = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndVad)
                .expect("Failed to serialize EndVad ServerEvent");
            ws.send(Message::binary(end_vad)).await?;
        }
    }
    Ok(())
}

enum ProcessMessageResult {
    Audio(Bytes),
    Submit,
    Text(String),
    StartChat,
    Close,
    Skip,
}

fn process_message(msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(t) => {
            log::debug!("Received text message: {}", t);
            if let Ok(cmd) = serde_json::from_str::<crate::protocol::ClientCommand>(&t) {
                match cmd {
                    crate::protocol::ClientCommand::StartRecord => ProcessMessageResult::Skip,
                    crate::protocol::ClientCommand::StartChat => ProcessMessageResult::StartChat,
                    crate::protocol::ClientCommand::Submit => ProcessMessageResult::Submit,
                    crate::protocol::ClientCommand::Text { input } => {
                        ProcessMessageResult::Text(input)
                    }
                }
            } else {
                ProcessMessageResult::Skip
            }
        }
        Message::Binary(d) => {
            log::debug!("Received binary message of size: {}", d.len());
            ProcessMessageResult::Audio(d)
        }
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
