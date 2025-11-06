use std::{collections::HashMap, sync::Arc, vec};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query,
    },
    response::IntoResponse,
    Extension,
};

use bytes::BufMut;
use fon::{chan::Samp16, Audio};
use futures_util::StreamExt;

use crate::{
    ai::{
        bailian::cosyvoice,
        elevenlabs,
        gemini::{
            self,
            types::{Blob, GenerationConfig, RealtimeAudio},
        },
        llm::Content,
        openai::tool::{McpToolAdapter, ToolSet},
        ChatSession, StableLLMResponseChunk,
    },
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
}

pub async fn ws_handler(
    Extension(pool): Extension<Arc<WsSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(params): Query<ConnectQueryParams>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().as_u128();
    log::info!("[Chat] {id}:{request_id:x} connected. {:?}", params);

    ws.on_upgrade(move |socket| async move {
        let id = id.clone();
        let pool = pool.clone();
        if let Err(e) = handle_socket(socket, &id, pool.clone(), params).await {
            log::error!("{id}:{request_id:x} handle_socket error: {e}");
        };
        log::info!("{id}:{request_id:x} disconnected.");
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
    let client = reqwest::Client::new();
    for i in 0..retry {
        let r = tokio::time::timeout(
            timeout,
            crate::ai::tts::gsv(&client, url, speaker, text, sample_rate),
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
    tx: &mut WsTx,
    text: String,
    wav_data: Bytes,
) -> anyhow::Result<std::time::Duration> {
    let mut reader = wav_io::reader::Reader::from_vec(wav_data.into())
        .map_err(|e| anyhow::anyhow!("wav_io reader error: {e}"))?;

    let header = reader.read_header()?;
    let mut samples = crate::util::get_samples_f32(&mut reader)
        .map_err(|e| anyhow::anyhow!("get_samples_f32 error: {e}"))?;
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
        tx.send(WsCommand::Audio(buff))?;
    }

    Ok(duration_sec)
}

async fn send_stream_chunk(
    tx: &mut WsTx,
    text: String,
    resp: reqwest::Response,
) -> anyhow::Result<f32> {
    log::debug!("[GSV_Stream] llm chunk:{:?}", text);

    let in_hz = 16000;
    let mut stream = resp.bytes_stream();
    let mut rest = bytes::BytesMut::new();
    let read_chunk_size = 2 * 5 * in_hz as usize / 10; // 0.5 seconds of audio at 32kHz

    let mut duration_sec = 0.0;

    'next_chunk: while let Some(item) = stream.next().await {
        // little-endian
        // chunk len may be not odd number
        let mut chunk = item?;

        log::trace!("Received audio chunk of size: {}", chunk.len());

        if rest.len() > 0 {
            log::trace!("chunk size: {}, rest size: {}", chunk.len(), rest.len());
            if chunk.len() + rest.len() > read_chunk_size {
                let n = read_chunk_size - rest.len();
                rest.put(chunk.slice(..n));
                debug_assert_eq!(rest.len(), read_chunk_size);
                let audio_16k = rest.to_vec();
                duration_sec += audio_16k.len() as f32 / 32000.0;
                log::trace!("Sending audio chunk of size: {}", audio_16k.len());
                tx.send(WsCommand::Audio(audio_16k))
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
            duration_sec += audio_16k.len() as f32 / 32000.0;
            tx.send(WsCommand::Audio(audio_16k))
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
        }
    }

    if rest.len() > 0 {
        let audio_16k = rest.to_vec();
        log::trace!("Sending audio chunk of size: {}", audio_16k.len());
        duration_sec += audio_16k.len() as f32 / 32000.0;
        tx.send(WsCommand::Audio(audio_16k))
            .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
    }

    Ok(duration_sec)
}

async fn tts_and_send(
    pool: &WsSetting,
    tx: &mut WsTx,
    text: String,
) -> anyhow::Result<std::time::Duration> {
    let tts_config = match &pool.config {
        AIConfig::Stable { tts, .. } => tts,
        AIConfig::GeminiAndTTS { tts, .. } => tts,
        AIConfig::Gemini { .. } => {
            return Err(anyhow::anyhow!("Gemini does not support TTS yet"));
        }
    };

    let client = reqwest::Client::new();

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
            let duration_sec = send_wav(tx, text, wav_data).await?;
            log::info!("Stable TTS duration: {:?}", duration_sec);
            Ok(duration_sec)
        }
        crate::config::TTSConfig::Fish(fish) => {
            let wav_data = crate::ai::tts::fish_tts(&fish.api_key, &fish.speaker, &text).await?;
            let duration_sec = send_wav(tx, text, wav_data).await?;
            log::info!("Fish TTS duration: {:?}", duration_sec);
            Ok(duration_sec)
        }
        crate::config::TTSConfig::Groq(groq) => {
            let wav_data =
                crate::ai::tts::groq(&client, &groq.model, &groq.api_key, &groq.voice, &text)
                    .await?;
            let duration_sec = send_wav(tx, text, wav_data).await?;
            log::info!("Groq TTS duration: {:?}", duration_sec);
            Ok(duration_sec)
        }
        crate::config::TTSConfig::StreamGSV(stream_tts) => {
            let resp = crate::ai::tts::stream_gsv(
                &client,
                &stream_tts.url,
                &stream_tts.speaker,
                &text,
                Some(16000),
            )
            .await?;

            let duration_sec = send_stream_chunk(tx, text, resp).await?;
            log::info!(
                "Stream TTS total duration: {:?}",
                std::time::Duration::from_secs_f32(duration_sec)
            );
            Ok(std::time::Duration::from_secs_f32(duration_sec))
        }
        crate::config::TTSConfig::CosyVoice(cosyvoice) => {
            let mut tts = cosyvoice::CosyVoiceTTS::connect(cosyvoice.token.clone()).await?;

            tts.start_synthesis(
                cosyvoice.version,
                cosyvoice.speaker.as_deref(),
                Some(16000),
                &text,
            )
            .await
            .unwrap();

            let mut duration_sec = 0.0;

            while let Ok(Some(chunk)) = tts.next_audio_chunk().await {
                duration_sec += chunk.len() as f32 / 32000.0;

                tx.send(WsCommand::Audio(chunk.into()))
                    .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
            }

            let duration_sec = std::time::Duration::from_secs_f32(duration_sec);
            log::info!("CosyVoice TTS duration: {:?}", duration_sec);
            Ok(duration_sec)
        }
        crate::config::TTSConfig::Elevenlabs(elevenlabs_tts) => {
            let mut tts = elevenlabs::tts::ElevenlabsTTS::new(
                elevenlabs_tts.token.clone(),
                elevenlabs_tts.voice.clone(),
                elevenlabs::tts::OutputFormat::Pcm16000,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Elevenlabs TTS init error: {e}"))?;

            tts.initialize_connection()
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS initialize connection error: {e}"))?;

            tts.send_text(&text, true)
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS send text error: {e}"))?;

            tts.close_connection()
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS close connection error: {e}"))?;

            let mut duration_sec = 0.0;

            while let Ok(Some(resp)) = tts.next_audio_response().await {
                if let Some(audio) = resp.get_audio_bytes() {
                    duration_sec += audio.len() as f32 / 32000.0;
                    tx.send(WsCommand::Audio(audio))
                        .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
                }
            }
            let duration_sec = std::time::Duration::from_secs_f32(duration_sec);
            log::info!("Elevenlabs TTS duration: {:?}", duration_sec);
            Ok(duration_sec)
        }
    }
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

async fn get_input(
    client: &reqwest::Client,
    id: &str,
    asr: &crate::config::ASRConfig,
    rx: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
) -> anyhow::Result<String> {
    match asr {
        crate::config::ASRConfig::Whisper(asr) => get_whisper_asr_text(client, id, asr, rx).await,
        crate::config::ASRConfig::ParaformerV2(asr) => get_paraformer_v2_text(id, asr, rx).await,
    }
}

async fn submit_to_ai(
    pool: &WsSetting,
    tx: &mut WsTx,
    chat_session: &mut ChatSession,
    asr_result: String,
) -> anyhow::Result<()> {
    let message = asr_result;
    if message.is_empty() {
        return Err(anyhow::anyhow!("empty asr result"));
    }

    tx.send(WsCommand::AsrResult(vec![message.clone()]))
        .map_err(|e| {
            anyhow::anyhow!(
                "error sending asr result ws command for message `{}`: {e}",
                message
            )
        })?;

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
                    tx.send(WsCommand::Action { action })?;
                    continue;
                }
                llm_response.push_str(&chunk);
                if chunk_.is_empty() {
                    continue;
                }

                tx.send(WsCommand::StartAudio(chunk.clone())).map_err(|_| {
                    anyhow::anyhow!("error sending start audio ws command for chunk `{}`", chunk)
                })?;

                let st = std::time::Instant::now();
                let r = tts_and_send(pool, tx, chunk.clone()).await;
                log::info!("tts took: {:?}", st.elapsed());

                if let Err(e) = r {
                    log::error!("tts error:{e}");
                };

                tx.send(WsCommand::EndAudio).map_err(|_| {
                    anyhow::anyhow!("error sending end audio ws command for chunk `{}`", chunk)
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

async fn submit_to_gemini_and_tts(
    pool: &WsSetting,
    client: &mut gemini::LiveClient,
    tx: &mut WsTx,
    setup: gemini::types::Setup,
    audio: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
) -> anyhow::Result<()> {
    // Gemini live api

    enum GeminiEvent {
        AudioChunk(ClientMsg),
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
                    tx.send(WsCommand::StartAudio(text.clone()))?;
                    match tts_and_send(pool, tx, text).await {
                        Ok(_) => {}
                        Err(e) => {
                            log::error!("tts error:{e}");
                        }
                    }
                    tx.send(WsCommand::EndAudio)?;
                    asr_text.clear();
                    text = String::new();
                    if let Err(e) = tx.send(WsCommand::EndResponse) {
                        log::error!("send error: {e}");
                    }
                }
                gemini::types::ServerContent::InputTranscription { text } => {
                    let message = hanconv::tw2sp(text);
                    asr_text.push_str(&message);

                    log::info!("gemini input transcription: {asr_text}");
                    // If the input transcription is not empty, we can use it as the ASR result
                    tx.send(WsCommand::AsrResult(vec![asr_text.clone()]))?;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                gemini::types::ServerContent::Timeout => {}
                gemini::types::ServerContent::GoAway {} => {
                    log::warn!("gemini GoAway");
                    tx.send(WsCommand::Action {
                        action: "GoAway".to_string(),
                    })?;
                    return Err(anyhow::anyhow!("Gemini GoAway"));
                }
            },
            GeminiEvent::AudioChunk(ClientMsg::AudioChunk(sample)) => {
                client
                    .send_realtime_input(gemini::types::RealtimeInput::Audio(RealtimeAudio {
                        data: Blob::new(sample.to_vec()),
                        mime_type: "audio/pcm;rate=16000".to_string(),
                    }))
                    .await?;
            }
            GeminiEvent::AudioChunk(ClientMsg::Submit) => {}
            GeminiEvent::AudioChunk(ClientMsg::StartChat) => {}
            GeminiEvent::AudioChunk(ClientMsg::Text(input)) => {
                client
                    .send_realtime_input(gemini::types::RealtimeInput::Text(input.clone()))
                    .await?;
            }
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
            log::error!("gemini connect error: {e}");
            if let Err(e) = tx.send(WsCommand::AsrResult(vec![])) {
                log::error!("send error: {e}");
            }
            return Ok(());
        }
        recv = recv_.unwrap();
    }
}

async fn submit_to_gemini(
    client: &mut gemini::LiveClient,
    tx: &mut WsTx,
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

    tx.send(WsCommand::AsrResult(vec![format!("Wait gemini")]))?;

    let mut buff = Vec::with_capacity(5 * 1600 * 2);

    loop {
        log::info!("waiting gemini response");
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
                                tx.send(WsCommand::Audio(buff))?;
                                buff = Vec::with_capacity(5 * 1600 * 2);
                            }
                        }
                    }
                }
            }
            gemini::types::ServerContent::GenerationComplete(_) => {
                log::info!("gemini generation complete");
            }
            gemini::types::ServerContent::Interrupted(_) => {
                log::info!("gemini interrupted");
            }
            gemini::types::ServerContent::TurnComplete(_) => {
                break;
            }
            gemini::types::ServerContent::InputTranscription { text } => {
                let message = hanconv::tw2sp(text);

                log::info!("gemini input transcription: {message}");
                // If the input transcription is not empty, we can use it as the ASR result
                tx.send(WsCommand::AsrResult(vec![message]))?;
            }
            gemini::types::ServerContent::Timeout => {
                log::warn!("gemini timeout");
                tx.send(WsCommand::AsrResult(vec![]))?;
                break;
            }
            gemini::types::ServerContent::GoAway {} => {
                log::warn!("gemini GoAway");
                tx.send(WsCommand::Action {
                    action: "GoAway".to_string(),
                })?;
                break;
            }
        }
    }
    tx.send(WsCommand::EndAudio)?;

    Ok(())
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
) -> anyhow::Result<()> {
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

async fn handle_audio(
    id: String,
    pool: Arc<WsSetting>,
    chat_session: &mut ChatSession,
    rx: &mut tokio::sync::mpsc::Receiver<ClientMsg>,
    mut ws_tx: WsTx,
) -> anyhow::Result<()> {
    match &pool.config {
        AIConfig::Stable { asr, .. } => {
            let client = reqwest::Client::new();

            loop {
                let asr_result = get_input(&client, &id, asr, rx).await?;
                log::info!("`{id}` ASR result: {asr_result}");

                let r = submit_to_ai(&pool, &mut ws_tx, chat_session, asr_result).await;
                if let Err(e) = r {
                    log::error!("`{id}` error: {e}");
                }
                ws_tx.send(WsCommand::EndResponse).map_err(|e| {
                    anyhow::anyhow!("`{id}` error sending EndResponse ws command: {e}")
                })?;
            }
        }
        // TODO: fix reconnect
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

            submit_to_gemini_and_tts(&pool, &mut client, &mut ws_tx, setup, rx).await?;
        },
        // TODO: fix reconnect
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

            let mut wav_audio = recv_audio_to_wav(rx).await?;

            loop {
                wav_audio = tokio::select! {
                    r = recv_audio_to_wav(rx) =>{
                        r?
                    }
                    r = submit_to_gemini(&mut client, &mut ws_tx, wav_audio) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                            if let Err(e) = ws_tx.send(WsCommand::AsrResult(vec![])) {
                                log::error!("`{id}` error: {e}");
                            };
                        }
                        if let Err(e) = ws_tx.send(WsCommand::EndResponse) {
                            log::error!("`{id}` error: {e}");
                        };

                        recv_audio_to_wav(rx).await?
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
    pool: Arc<WsSetting>,
    connect_params: ConnectQueryParams,
) -> anyhow::Result<()> {
    if let Some(hello_wav) = &pool.hello_wav {
        if !hello_wav.is_empty() && !connect_params.reconnect {
            send_hello_wav(&mut socket, hello_wav).await?;
        }
    }

    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<ClientMsg>(128);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WsCommand>();

    let ctrl_rx = {
        let mut session = pool.sessions.lock().await;

        let new_ctrl_channel = if let Some(ctrl_tx_) = session.get(&id.to_string()) {
            if ctrl_tx_.is_closed() {
                let (ctrl_tx, ctrl_rx) = tokio::sync::mpsc::channel(1);
                ctrl_tx.send((cmd_tx, audio_rx)).await.map_err(|_| {
                    anyhow::anyhow!("`{}` error sending initial cmd_tx to ctrl channel.", id)
                })?;
                Err((ctrl_tx, ctrl_rx))
            } else {
                if let Err(_) = ctrl_tx_.send((cmd_tx, audio_rx)).await {
                    log::error!("`{}` error sending reconnect cmd_tx", id);
                    return Err(anyhow::anyhow!("error sending reconnect cmd_tx"));
                }
                Ok(())
            }
        } else {
            let (ctrl_tx, ctrl_rx) = tokio::sync::mpsc::channel(1);
            ctrl_tx.send((cmd_tx, audio_rx)).await.map_err(|_| {
                anyhow::anyhow!("`{}` error sending initial cmd_tx to ctrl channel.", id)
            })?;
            Err((ctrl_tx, ctrl_rx))
        };

        if let Err((ctrl_tx, ctrl_rx)) = new_ctrl_channel {
            session.insert(id.to_string(), ctrl_tx);
            Some(ctrl_rx)
        } else {
            None
        }
    };

    if let Some(ctrl_rx) = ctrl_rx {
        log::info!("`{}` starting audio handler task", id);
        let id_ = id.to_string();
        let pool_ = pool.clone();
        let mut chat_session = ChatSession::create_from_config(&pool.config, pool.tool_set.clone());

        tokio::spawn(async move {
            let mut ctrl_rx = ctrl_rx;
            let r = ctrl_rx.recv().await;
            if r.is_none() {
                log::error!(
                    "`{}` ctrl channel closed immediately, exiting audio handler",
                    id_
                );
                return;
            }
            let (mut cmd_tx, mut audio_rx) = r.unwrap();

            loop {
                let f = handle_audio(
                    id_.clone(),
                    pool_.clone(),
                    &mut chat_session,
                    &mut audio_rx,
                    cmd_tx,
                );

                tokio::select! {
                    r = f =>{
                        if let Err(e) = r {
                            log::error!("`{id_}` handle audio error: {e}");
                        }

                    }
                    Some((cmd_tx_, audio_rx_)) = ctrl_rx.recv() =>{
                        log::info!("`{id_}` received new ctrl channel, switching audio handler");
                        cmd_tx = cmd_tx_;
                        audio_rx = audio_rx_;
                        continue;
                    }
                    else =>{
                        log::error!("`{}` ctrl channel closed, exiting audio handler", id_);
                        break;
                    }
                }

                match ctrl_rx.recv().await {
                    Some((cmd_tx_, audio_rx_)) => {
                        log::info!("`{id_}` received new ctrl channel, switching audio handler");
                        cmd_tx = cmd_tx_;
                        audio_rx = audio_rx_;
                    }
                    None => {
                        log::error!("`{}` ctrl channel closed, exiting audio handler", id_);
                        break;
                    }
                }
            }
        });
    }

    log::info!("`{}` starting socket io processing", id);
    process_socket_io(&mut cmd_rx, audio_tx, &mut socket).await?;

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
