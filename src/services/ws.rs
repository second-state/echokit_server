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

/// return: (wav_data,is_recording)
async fn recv_audio_to_wav(
    audio: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
) -> anyhow::Result<Vec<u8>> {
    let mut samples = bytes::BytesMut::new();

    while let Some(chunk) = audio.recv().await {
        match chunk {
            ClientMsg::AudioChunk(data) => {
                samples.extend_from_slice(&data);
            }
            ClientMsg::Submit => {
                log::info!("end audio");
                break;
            }
            _ => {}
        }
    }

    if samples.is_empty() {
        return Err(anyhow::anyhow!("no audio received"));
    }

    let wav_audio = crate::util::pcm_to_wav(
        &samples,
        WavConfig {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
        },
    );

    if std::option_env!("DEBUG_WAV").is_some() {
        if let Err(e) = std::fs::write("./recv_wav.wav", &wav_audio) {
            log::error!("write recv_wav.wav error: {e}");
        }
    }

    Ok(wav_audio)
}

pub async fn get_whisper_asr_text(
    client: &reqwest::Client,
    id: &str,
    asr: &crate::config::WhisperASRConfig,
    audio: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
) -> anyhow::Result<String> {
    loop {
        let msg = audio
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

        match msg {
            ClientMsg::Text(input) => {
                return Ok(input);
            }
            ClientMsg::StartChat => {
                // start chat
                let wav_data = recv_audio_to_wav(audio).await?;
                if let Some(vad_url) = &asr.vad_url {
                    let response =
                        crate::ai::vad::vad_detect(client, vad_url, wav_data.clone()).await;

                    let is_speech = response.map(|r| !r.timestamps.is_empty()).unwrap_or(true);
                    if !is_speech {
                        log::info!("VAD detected no speech, ignore this audio");
                        return Ok(String::new());
                    }
                }

                let st = std::time::Instant::now();
                let text = retry_asr(
                    client,
                    &asr.url,
                    &asr.api_key,
                    &asr.model,
                    &asr.lang,
                    &asr.prompt,
                    wav_data,
                    3,
                    std::time::Duration::from_secs(10),
                )
                .await;
                log::info!("`{id}` ASR took: {:?}", st.elapsed());
                let text = text.join("\n");
                log::info!("ASR result: {:?}", text);
                if text.is_empty() || text.trim().starts_with("(") {
                    return Ok(String::new());
                }
                return Ok(hanconv::tw2sp(text));
            }
            ClientMsg::Submit => {
                continue;
            }
            ClientMsg::AudioChunk(_) => {
                continue;
            }
        }
    }
}

pub async fn get_paraformer_v2_text(
    id: &str,
    asr: &crate::config::ParaformerV2AsrConfig,
    audio: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
) -> anyhow::Result<String> {
    let paraformer_token = asr.paraformer_token.clone();
    let paraformer_url = asr.url.clone();
    let mut asr: Option<crate::ai::bailian::realtime_asr::ParaformerRealtimeV2Asr> = None;
    loop {
        while let Some(chunk) = audio.recv().await {
            match chunk {
                ClientMsg::Text(input) => {
                    return Ok(input);
                }
                ClientMsg::AudioChunk(data) => {
                    if let Some(asr) = asr.as_mut() {
                        asr.send_audio(data).await.map_err(|e| {
                            anyhow::anyhow!("`{id}` error sending paraformer asr audio: {e}")
                        })?;
                    }
                }
                ClientMsg::Submit => {
                    break;
                }
                ClientMsg::StartChat => {
                    log::info!("`{id}` starting paraformer asr");
                    let mut paraformer_asr =
                        crate::ai::bailian::realtime_asr::ParaformerRealtimeV2Asr::connect(
                            &paraformer_url,
                            paraformer_token.clone(),
                            16000,
                        )
                        .await?;

                    paraformer_asr.start_pcm_recognition().await.map_err(|e| {
                        anyhow::anyhow!("`{id}` error starting paraformer asr: {e}")
                    })?;
                    asr = Some(paraformer_asr);
                    continue;
                }
            }
        }

        if let Some(mut asr) = asr.take() {
            asr.finish_task()
                .await
                .map_err(|e| anyhow::anyhow!("`{id}` error finishing paraformer asr task: {e}"))?;
            let mut text = String::new();
            while let Some(sentence) = asr
                .next_result()
                .await
                .map_err(|e| anyhow::anyhow!("`{id}` error getting paraformer asr result: {e}"))?
            {
                if sentence.sentence_end {
                    text = sentence.text;
                    log::info!("ASR final result: {:?}", text);
                    break;
                }
            }
            return Ok(text);
        } else {
            return Err(anyhow::anyhow!("`{id}` no paraformer asr session"));
        }
    }
}

pub enum ClientMsg {
    StartChat,
    /// 16000 16bit le
    AudioChunk(Bytes),
    Submit,
    Text(String),
}

// return: wav data
async fn process_socket_io(
    rx: &mut WsRx,
    audio_tx: ClientTx,
    socket: &mut WebSocket,
    enable_opus: bool,
) -> anyhow::Result<()> {
    let mut opus_encoder =
        opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| anyhow::anyhow!("opus encoder error: {e}"))?;
    let mut ret_audio = Vec::with_capacity(sample_120ms(SAMPLE_RATE));

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
                if enable_opus {
                    process_command_with_opus(socket, cmd, &mut opus_encoder, &mut ret_audio)
                        .await?
                } else {
                    process_command(socket, cmd).await?
                }
            }
            Some(WsEvent::Message(Ok(msg))) => match process_message(msg) {
                ProcessMessageResult::Audio(d) => audio_tx
                    .send(ClientMsg::AudioChunk(d))
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Submit => audio_tx
                    .send(ClientMsg::Submit)
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Text(input) => audio_tx
                    .send(ClientMsg::Text(input))
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Skip => {}
                ProcessMessageResult::StartChat => audio_tx
                    .send(ClientMsg::StartChat)
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
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
    }
    Ok(())
}

async fn process_command_with_opus(
    ws: &mut WebSocket,
    cmd: WsCommand,
    opus_encode: &mut opus::Encoder,
    ret_audio: &mut Vec<i16>,
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
            for chunk in ret_audio.chunks(sample_120ms(SAMPLE_RATE)) {
                if chunk.len() < sample_120ms(SAMPLE_RATE) {
                    *ret_audio = chunk.to_vec();
                    break;
                }
                let data = opus_encode.encode_vec(chunk, 2 * sample_120ms(SAMPLE_RATE) / 3)?;

                let audio_chunk =
                    rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk { data })
                        .expect("Failed to serialize AudioChunk ServerEvent");
                ws.send(Message::binary(audio_chunk)).await?;
            }

            ret_audio.clear();
        }
        WsCommand::EndAudio => {
            log::trace!("EndAudio");
            if !ret_audio.is_empty() {
                let padded_audio_len = sample_120ms(SAMPLE_RATE) - ret_audio.len();
                ret_audio.extend(std::iter::repeat(0i16).take(padded_audio_len));
                let data = opus_encode.encode_vec(&ret_audio, 2 * sample_120ms(SAMPLE_RATE) / 3)?;
                let audio_chunk =
                    rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk { data })
                        .expect("Failed to serialize AudioChunk ServerEvent");
                log::info!("Sending final audio chunk of size: {}", audio_chunk.len());
                ws.send(Message::binary(audio_chunk)).await?;
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
