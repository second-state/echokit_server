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

use bytes::BufMut;
use fon::{chan::Samp16, Audio};
use futures_util::StreamExt;

use crate::{
    ai::{
        gemini::{
            self,
            types::{Blob, GenerationConfig, RealtimeAudio},
        },
        llm::Content,
        openai::tool::{McpToolAdapter, ToolSet},
        ChatSession, StableLLMResponseChunk,
    },
    config::AIConfig,
};

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

#[derive(Debug)]
pub struct WsPool {
    pub config: AIConfig,
    pub connections: tokio::sync::RwLock<HashMap<String, (u128, WsTx)>>,
    pub hello_wav: Option<Vec<u8>>,
    pub tool_set: ToolSet<McpToolAdapter>,
}

impl WsPool {
    pub fn new(
        hello_wav: Option<Vec<u8>>,
        config: AIConfig,
        tool_set: ToolSet<McpToolAdapter>,
    ) -> Self {
        Self {
            config,
            connections: tokio::sync::RwLock::new(HashMap::new()),
            hello_wav,
            tool_set,
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

fn resample(audio_samples: &[i16], in_hz: u32, out_hz: u32) -> anyhow::Result<Audio<Samp16, 1>> {
    let audio = Audio::<Samp16, 1>::with_i16_buffer(in_hz, audio_samples);
    let audio = Audio::<Samp16, 1>::with_audio(out_hz, &audio);

    Ok(audio)
}

async fn retry_tts(
    url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
    retry: usize,
    timeout: std::time::Duration,
) -> anyhow::Result<Bytes> {
    for i in 0..retry {
        let r = tokio::time::timeout(
            timeout,
            crate::ai::tts::gsv(url, speaker, text, sample_rate),
        )
        .await;
        match r {
            Ok(Ok(v)) => return Ok(v),
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("tts error: {e}"));
            }
            Err(_) => {
                log::error!("tts timeout, retry {i}");
                continue;
            }
        }
    }
    Err(anyhow::anyhow!("tts timeout"))
}

async fn send_wav(
    pool: &WsPool,
    id: &str,
    text: String,
    wav_data: Bytes,
) -> anyhow::Result<std::time::Duration> {
    let mut reader = wav_io::reader::Reader::from_vec(wav_data.into())
        .map_err(|e| anyhow::anyhow!("wav_io reader error: {e}"))?;

    let header = reader.read_header()?;
    let mut samples = reader.get_samples_f32()?;
    let duration_sec = samples.len() as f32 / (header.sample_rate as f32 * header.channels as f32);
    let duration_sec = std::time::Duration::from_secs_f32(duration_sec);

    let out_hz = 16000;

    if header.sample_rate != out_hz {
        // resample to 16000
        log::info!("resampling from {} to 16000", header.sample_rate);
        samples = wav_io::resample::linear(samples, header.channels, header.sample_rate, out_hz);
    }
    let audio_16k = wav_io::convert_samples_f32_to_i16(&samples);

    log::info!("llm chunk:{:?}", text);

    for chunk in audio_16k.chunks(5 * out_hz as usize / 10) {
        let buff = if cfg!(target_endian = "big") {
            let mut buff = Vec::with_capacity(chunk.len() * 2);
            for i in chunk {
                buff.extend_from_slice(&i.to_le_bytes());
            }
            buff
        } else {
            let chunk_bytes =
                unsafe { std::slice::from_raw_parts(chunk.as_ptr() as *const u8, chunk.len() * 2) };
            chunk_bytes.to_vec()
        };

        // std::mem::swap(&mut send_data, &mut buff);
        pool.send(id, WsCommand::Audio(buff)).await?;
    }

    Ok(duration_sec)
}

async fn send_stream_chunk(
    pool: &WsPool,
    id: &str,
    text: String,
    resp: reqwest::Response,
) -> anyhow::Result<()> {
    log::info!("llm chunk:{:?}", text);

    let in_hz = 16000;
    let mut stream = resp.bytes_stream();
    let mut rest = bytes::BytesMut::new();
    let read_chunk_size = 2 * 5 * in_hz as usize / 10; // 0.5 seconds of audio at 32kHz

    'next_chunk: while let Some(item) = stream.next().await {
        // 小端字节序
        let mut chunk = item?;

        log::trace!("Received audio chunk of size: {}", chunk.len());

        if rest.len() > 0 {
            log::trace!("chunk size: {}, rest size: {}", chunk.len(), rest.len());
            if chunk.len() + rest.len() > read_chunk_size {
                let n = read_chunk_size - rest.len();
                rest.put(chunk.slice(..n));
                debug_assert_eq!(rest.len(), read_chunk_size);
                let audio_16k = rest.to_vec();
                log::trace!("Sending audio chunk of size: {}", audio_16k.len());
                pool.send(id, WsCommand::Audio(audio_16k))
                    .await
                    .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
                rest.clear();
                chunk = chunk.slice(n..);
            } else {
                rest.extend_from_slice(&chunk);
                continue 'next_chunk;
            }
        }

        for samples_16k_data in chunk.chunks(read_chunk_size) {
            if samples_16k_data.len() < read_chunk_size {
                log::trace!("Received audio chunk with odd length, skipping");
                rest.extend_from_slice(&samples_16k_data);
                continue 'next_chunk;
            }
            let audio_16k = samples_16k_data.to_vec();
            log::trace!("Sending audio chunk of size: {}", audio_16k.len());
            pool.send(id, WsCommand::Audio(audio_16k))
                .await
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
        }
    }

    if rest.len() > 0 {
        let audio_16k = rest.to_vec();
        log::trace!("Sending audio chunk of size: {}", audio_16k.len());
        pool.send(id, WsCommand::Audio(audio_16k))
            .await
            .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
    }

    Ok(())
}

async fn tts_and_send(pool: &WsPool, id: &str, text: String) -> anyhow::Result<()> {
    let tts_config = match &pool.config {
        AIConfig::Stable { tts, .. } => tts,
        AIConfig::GeminiAndTTS { tts, .. } => tts,
        AIConfig::Gemini { .. } => {
            return Err(anyhow::anyhow!("Gemini does not support TTS yet"));
        }
    };

    match tts_config {
        crate::config::TTSConfig::Stable(tts) => {
            let timeout_sec = tts.timeout_sec.unwrap_or(15);
            let wav_data = retry_tts(
                &tts.url,
                &tts.speaker,
                &text,
                Some(16000),
                3,
                std::time::Duration::from_secs(timeout_sec),
            )
            .await?;
            let duration_sec = send_wav(pool, id, text, wav_data).await?;
            log::info!("Stable TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::Fish(fish) => {
            let wav_data = crate::ai::tts::fish_tts(&fish.api_key, &fish.speaker, &text).await?;
            let duration_sec = send_wav(pool, id, text, wav_data).await?;
            log::info!("Fish TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::Groq(groq) => {
            let wav_data =
                crate::ai::tts::groq(&groq.model, &groq.api_key, &groq.voice, &text).await?;
            let duration_sec = send_wav(pool, id, text, wav_data).await?;
            log::info!("Fish TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::StreamGSV(stream_tts) => {
            let resp = crate::ai::tts::stream_gsv(
                &stream_tts.url,
                &stream_tts.speaker,
                &text,
                Some(16000),
            )
            .await?;

            send_stream_chunk(pool, id, text, resp).await?;
            log::info!("Stream GSV TTS sent");
            Ok(())
        }
    }
}

/// return: (wav_data,is_recording)
async fn recv_audio_to_wav(
    audio: &mut tokio::sync::mpsc::Receiver<AudioChunk>,
) -> anyhow::Result<(Vec<u8>, bool)> {
    let head = wav_io::new_header(16000, 16, false, true);
    let mut samples = Vec::new();
    let mut is_recording = false;

    while let Some(chunk) = audio.recv().await {
        match chunk {
            AudioChunk::Chunk(data) => {
                if data.len() % 2 != 0 {
                    log::warn!("Received audio chunk with odd length, skipping");
                    for i in data[0..data.len() - 1].chunks_exact(2) {
                        let sample = i16::from_le_bytes([i[0], i[1]]);
                        samples.push(sample as f32 / std::i16::MAX as f32);
                    }
                } else {
                    for i in data.chunks_exact(2) {
                        let sample = i16::from_le_bytes([i[0], i[1]]);
                        samples.push(sample as f32 / std::i16::MAX as f32);
                    }
                }
            }
            AudioChunk::Enb => {
                log::info!("end audio");
                break;
            }
            AudioChunk::Recording => {
                is_recording = true;
                break;
            }
        }
    }

    if samples.is_empty() {
        return Err(anyhow::anyhow!("no audio received"));
    }

    let wav_audio = wav_io::write_to_bytes(&head, &samples)?;

    Ok((wav_audio, is_recording))
}

async fn get_asr_text(
    client: &reqwest::Client,
    id: &str,
    asr: &crate::config::ASRConfig,
    audio: &mut tokio::sync::mpsc::Receiver<AudioChunk>,
) -> anyhow::Result<String> {
    std::fs::create_dir_all(format!("./record/{id}"))?;
    loop {
        let (wav_data, is_recording) = recv_audio_to_wav(audio).await?;

        std::fs::write(format!("./record/{id}/asr.last.wav"), &wav_data)?;

        if let Some(vad_url) = &asr.vad_url {
            match crate::ai::vad::vad_detect(client, vad_url, wav_data.clone()).await {
                Ok(r) => {
                    if let Some(err) = r.error {
                        log::error!("`{id}` vad error: {err}, skipping ASR");
                        continue;
                    }

                    if r.timestamps.is_empty() {
                        log::warn!("`{id}` vad returned empty timestamps, skipping ASR");
                        continue;
                    }
                }

                Err(e) => {
                    log::error!("`{id}` vad error: {e}, skipping ASR");
                    continue;
                }
            }
        }

        if is_recording {
            let now = chrono::Local::now().to_rfc3339();

            if let Err(e) = std::fs::write(format!("./record/{id}/recording_{now}.wav"), &wav_data)
            {
                log::error!("`{id}` error writing recording file {now}: {e}");
            };
            continue;
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
            continue;
        }
        return Ok(hanconv::tw2sp(text));
    }
}

async fn submit_to_ai(
    pool: &WsPool,
    id: &str,
    chat_session: &mut ChatSession,
    asr_result: String,
) -> anyhow::Result<()> {
    let message = asr_result;

    pool.send(id, WsCommand::AsrResult(vec![message.clone()]))
        .await?;

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
    let mut resp = chat_session.complete().await?;

    let mut llm_response = String::with_capacity(128);

    let mut first_chunk = true;

    loop {
        match resp.next_chunk().await {
            Ok(StableLLMResponseChunk::Text(chunk)) => {
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
                pool.send(id, WsCommand::StartAudio(chunk.clone())).await?;
                let st = std::time::Instant::now();
                match tts_and_send(pool, id, chunk).await {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("tts error:{e}");
                    }
                }
                log::info!("tts took: {:?}", st.elapsed());
                pool.send(id, WsCommand::EndAudio).await?;
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

async fn submit_to_gemini_and_tts(
    pool: &WsPool,
    client: &mut gemini::LiveClient,
    id: &str,
    setup: gemini::types::Setup,
    audio: &mut tokio::sync::mpsc::Receiver<AudioChunk>,
) -> anyhow::Result<()> {
    // Gemini live api

    enum GeminiEvent {
        AudioChunk(AudioChunk),
        ServerEvent(gemini::types::ServerContent),
    }

    let mut text = String::new();
    let mut asr_text = String::new();

    let r = audio.recv().await;

    let mut recv = r
        .map(|r| GeminiEvent::AudioChunk(r))
        .ok_or_else(|| anyhow::anyhow!("audio channel closed"))?;

    client.setup(setup).await?;

    loop {
        match recv {
            GeminiEvent::ServerEvent(server_event) => match server_event {
                gemini::types::ServerContent::ModelTurn(turn) => {
                    turn.parts.iter().for_each(|part| {
                        if let gemini::types::Parts::Text(text_part) = part {
                            text.push_str(&text_part);
                        }
                    });
                }
                gemini::types::ServerContent::GenerationComplete(_) => {}
                gemini::types::ServerContent::Interrupted(_) => {}
                gemini::types::ServerContent::TurnComplete(_) => {
                    pool.send(id, WsCommand::StartAudio(text.clone())).await?;
                    match tts_and_send(pool, id, text).await {
                        Ok(_) => {}
                        Err(e) => {
                            log::error!("tts error:{e}");
                        }
                    }
                    pool.send(id, WsCommand::EndAudio).await?;
                    asr_text.clear();
                    text = String::new();
                    if let Err(e) = pool.send(&id, WsCommand::EndResponse).await {
                        log::error!("`{id}` error: {e}");
                    }
                }
                gemini::types::ServerContent::InputTranscription { text } => {
                    let message = hanconv::tw2sp(text);
                    asr_text.push_str(&message);

                    log::info!("`{id}` gemini input transcription: {asr_text}");
                    // If the input transcription is not empty, we can use it as the ASR result
                    pool.send(id, WsCommand::AsrResult(vec![asr_text.clone()]))
                        .await?;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                gemini::types::ServerContent::Timeout => {}
                gemini::types::ServerContent::GoAway {} => {
                    log::warn!("`{id}` gemini GoAway");
                    pool.send(
                        id,
                        WsCommand::Action {
                            action: "GoAway".to_string(),
                        },
                    )
                    .await?;
                    return Err(anyhow::anyhow!("Gemini GoAway"));
                }
            },
            GeminiEvent::AudioChunk(AudioChunk::Chunk(sample)) => {
                client
                    .send_realtime_input(gemini::types::RealtimeInput::Audio(RealtimeAudio {
                        data: Blob::new(sample.to_vec()),
                        mime_type: "audio/pcm;rate=16000".to_string(),
                    }))
                    .await?;
            }
            GeminiEvent::AudioChunk(AudioChunk::Enb) => {}
            GeminiEvent::AudioChunk(AudioChunk::Recording) => {}
        }

        let recv_ = {
            tokio::select! {
                r = audio.recv()=>{
                    Ok(r.map(|r|GeminiEvent::AudioChunk(r)).ok_or_else(||anyhow::anyhow!("audio channel closed"))?)
                }
                r = client.receive() => {
                    r.map(|r|GeminiEvent::ServerEvent(r))
                }
            }
        };
        if let Err(e) = recv_ {
            log::error!("`{id}` gemini connect error: {e}");
            if let Err(e) = pool.send(&id, WsCommand::AsrResult(vec![])).await {
                log::error!("`{id}` error: {e}");
            }
            return Ok(());
        }
        recv = recv_.unwrap();
    }
}

async fn submit_to_gemini(
    pool: &WsPool,
    client: &mut gemini::LiveClient,
    id: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<()> {
    // Gemini live api
    let mut reader = wav_io::reader::Reader::from_vec(wav_audio)?;
    let header = reader.read_header()?;
    let mut samples = reader.get_samples_f32()?;
    if header.sample_rate != 16000 {
        samples = wav_io::resample::linear(samples, 1, header.sample_rate, 16000);
    }

    let data = wav_io::convert_samples_f32_to_i16(&samples);
    let mut submit_data = Vec::with_capacity(data.len() * 2);
    for sample in data {
        submit_data.extend_from_slice(&sample.to_le_bytes());
    }

    log::info!("start gemini");
    client
        .send_realtime_audio(RealtimeAudio {
            data: Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        })
        .await?;

    pool.send(id, WsCommand::AsrResult(vec![format!("Wait gemini")]))
        .await?;

    let mut buff = Vec::with_capacity(5 * 1600 * 2);

    loop {
        log::info!("`{id}` waiting gemini response");
        match client.receive().await? {
            gemini::types::ServerContent::ModelTurn(turn) => {
                for item in turn.parts {
                    if let gemini::types::Parts::InlineData { data, mime_type } = item {
                        if mime_type.starts_with("audio/pcm") {
                            let audio_data = data.into_inner();
                            let mut sample = Vec::with_capacity(audio_data.len() / 2);
                            if audio_data.len() % 2 != 0 {
                                log::warn!("Received audio chunk with odd length, skipping");
                                for i in audio_data[0..audio_data.len() - 1].chunks_exact(2) {
                                    let sample_value = i16::from_le_bytes([i[0], i[1]]);
                                    sample.push(sample_value);
                                }
                            } else {
                                for i in audio_data.chunks_exact(2) {
                                    let sample_value = i16::from_le_bytes([i[0], i[1]]);
                                    sample.push(sample_value);
                                }
                            }

                            let mut audio_16k = resample(&sample, 24000, 16000)?;
                            let samples = audio_16k.as_i16_slice();
                            for chunk in samples.chunks(5 * 16000 / 10) {
                                for i in chunk {
                                    buff.extend_from_slice(&i.to_le_bytes());
                                }
                                // std::mem::swap(&mut send_data, &mut buff);
                                pool.send(id, WsCommand::Audio(buff)).await?;
                                buff = Vec::with_capacity(5 * 1600 * 2);
                            }
                        }
                    }
                }
            }
            gemini::types::ServerContent::GenerationComplete(_) => {
                log::info!("`{id}` gemini generation complete");
            }
            gemini::types::ServerContent::Interrupted(_) => {
                log::info!("`{id}` gemini interrupted");
            }
            gemini::types::ServerContent::TurnComplete(_) => {
                break;
            }
            gemini::types::ServerContent::InputTranscription { text } => {
                let message = hanconv::tw2sp(text);

                log::info!("`{id}` gemini input transcription: {message}");
                // If the input transcription is not empty, we can use it as the ASR result
                pool.send(id, WsCommand::AsrResult(vec![message])).await?;
            }
            gemini::types::ServerContent::Timeout => {
                log::warn!("`{id}` gemini timeout");
                pool.send(id, WsCommand::AsrResult(vec![])).await?;
                break;
            }
            gemini::types::ServerContent::GoAway {} => {
                log::warn!("`{id}` gemini GoAway");
                pool.send(
                    id,
                    WsCommand::Action {
                        action: "GoAway".to_string(),
                    },
                )
                .await?;
                break;
            }
        }
    }
    pool.send(id, WsCommand::EndAudio).await?;

    Ok(())
}

pub enum AudioChunk {
    /// 16000 16bit le
    Chunk(Bytes),
    Enb,
    Recording,
}

// return: wav data
async fn process_socket_io(
    rx: &mut WsRx,
    audio_tx: tokio::sync::mpsc::Sender<AudioChunk>,
    socket: &mut WebSocket,
) -> anyhow::Result<Vec<u8>> {
    loop {
        let r = tokio::select! {
            cmd = rx.recv() => {
                cmd.map(|cmd| WsEvent::Command(cmd))
            }
            message = socket.recv() => {
                message.map(|message| match message{
                    Ok(message) => WsEvent::Message(Ok(message)),
                    Err(e) => WsEvent::Message(Err(anyhow::anyhow!("recv ws error: {e}"))),
                })
            }
        };

        match r {
            Some(WsEvent::Command(cmd)) => process_command(socket, cmd).await?,
            Some(WsEvent::Message(Ok(msg))) => match process_message(msg) {
                // i16 16000
                ProcessMessageResult::Ok(d) => audio_tx
                    .send(AudioChunk::Chunk(d))
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Skip => {}
                ProcessMessageResult::Submit => audio_tx
                    .send(AudioChunk::Enb)
                    .await
                    .map_err(|_| anyhow::anyhow!("audio_tx closed"))?,
                ProcessMessageResult::Recording => audio_tx
                    .send(AudioChunk::Recording)
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

async fn handle_audio(
    id: String,
    pool: Arc<WsPool>,
    mut rx: tokio::sync::mpsc::Receiver<AudioChunk>,
) -> anyhow::Result<()> {
    match &pool.config {
        AIConfig::Stable { llm, asr, .. } => {
            let client = reqwest::Client::new();
            let mut chat_session = ChatSession::new(
                llm.llm_chat_url.to_string(),
                llm.api_key.clone().unwrap_or_default(),
                llm.model.clone(),
                None,
                llm.history,
                pool.tool_set.clone(),
            );

            chat_session.system_prompts = llm.sys_prompts.clone();
            chat_session.messages = llm.dynamic_prompts.clone();

            let mut asr_result = get_asr_text(&client, &id, asr, &mut rx).await?;

            loop {
                asr_result = tokio::select! {
                    r = get_asr_text(&client, &id, asr, &mut rx) =>{
                        r?
                    }
                    r = submit_to_ai(&pool, &id,&mut chat_session, asr_result) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                            if let Err(e) = pool.send(&id, WsCommand::AsrResult(vec![])).await{
                                log::error!("`{id}` error: {e}");
                            };
                        }
                        if let Err(e) = pool.send(&id, WsCommand::EndResponse).await{
                            log::error!("`{id}` error: {e}");
                        };

                        get_asr_text(&client, &id, asr, &mut rx).await?
                    }
                };
            }
        }
        AIConfig::GeminiAndTTS { gemini, .. } => loop {
            let mut client = gemini::LiveClient::connect(&gemini.api_key).await?;
            let model = gemini
                .model
                .clone()
                .unwrap_or("models/gemini-2.0-flash-live-001".to_string());

            let mut generation_config = GenerationConfig::default();
            generation_config.response_modalities = Some(vec![gemini::types::Modality::TEXT]);

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
            };

            submit_to_gemini_and_tts(&pool, &mut client, &id, setup, &mut rx).await?;
        },
        AIConfig::Gemini { gemini } => {
            let mut client = gemini::LiveClient::connect(&gemini.api_key).await?;
            let model = gemini
                .model
                .clone()
                .unwrap_or("models/gemini-2.0-flash-live-001".to_string());

            let mut generation_config = GenerationConfig::default();
            generation_config.response_modalities = Some(vec![gemini::types::Modality::AUDIO]);

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
            };

            client.setup(setup).await?;

            let mut wav_audio = recv_audio_to_wav(&mut rx).await?.0;

            loop {
                wav_audio = tokio::select! {
                    r = recv_audio_to_wav(&mut rx) =>{
                        r?.0
                    }
                    r = submit_to_gemini(&pool, &mut client, &id, wav_audio) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                            if let Err(e) = pool.send(&id, WsCommand::AsrResult(vec![])).await{
                                log::error!("`{id}` error: {e}");
                            };
                        }
                        if let Err(e) = pool.send(&id, WsCommand::EndResponse).await{
                            log::error!("`{id}` error: {e}");
                        };

                        recv_audio_to_wav(&mut rx).await?.0
                    }
                };
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

async fn handle_socket(
    mut socket: WebSocket,
    id: &str,
    mut rx: WsRx,
    pool: Arc<WsPool>,
) -> anyhow::Result<()> {
    if let Some(hello_wav) = &pool.hello_wav {
        if !hello_wav.is_empty() {
            send_hello_wav(&mut socket, hello_wav).await?;
        }
    }

    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<AudioChunk>(1);
    let pool_ = pool.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        let id_ = id.clone();
        let r = handle_audio(id, pool_, audio_rx).await;
        if let Err(e) = r {
            log::error!("`{id_}` handle audio error: {e}");
        }
    });

    process_socket_io(&mut rx, audio_tx, &mut socket).await?;

    Ok(())
}

pub const SAMPLE_RATE: u32 = 16000;
pub const SAMPLE_RATE_BUFFER_SIZE: usize = 2 * (SAMPLE_RATE as usize) / 10;

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
            let start_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::StartAudio { text })
                .expect("Failed to serialize StartAudio ServerEvent");
            ws.send(Message::binary(start_audio)).await?;
        }
        WsCommand::Audio(data) => {
            let start_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::AudioChunk { data })
                .expect("Failed to serialize StartAudio ServerEvent");
            ws.send(Message::binary(start_audio)).await?;
        }
        WsCommand::EndAudio => {
            let end_audio = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndAudio)
                .expect("Failed to serialize EndAudio ServerEvent");
            ws.send(Message::binary(end_audio)).await?;
        }
        WsCommand::Video(_) => {
            log::warn!("video command is not implemented yet");
        }
        WsCommand::EndResponse => {
            let end_response = rmp_serde::to_vec(&crate::protocol::ServerEvent::EndResponse)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::binary(end_response)).await?;
        }
    }
    Ok(())
}

enum ProcessMessageResult {
    Ok(Bytes),
    Submit,
    Recording,
    Close,
    Skip,
}

fn process_message(msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(t) => {
            if t.as_str() == "End:Normal" {
                ProcessMessageResult::Submit
            } else if t.as_str() == "End:Recording" {
                ProcessMessageResult::Recording
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
