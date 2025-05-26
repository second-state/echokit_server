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

use fon::{chan::Samp16, Audio};

use crate::{
    ai::{
        genai::{
            self,
            types::{Blob, GenerationConfig, RealtimeAudio},
        },
        llm::Content,
    },
    config::AIConfig,
};

pub enum WsCommand {
    AsrResult(Vec<String>),
    Action { action: String },
    Audio(Bytes),
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
}

impl WsPool {
    pub fn new(config: AIConfig) -> Self {
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

async fn retry_asr(
    url: &str,
    lang: &str,
    wav_audio: Vec<u8>,
    retry: usize,
    timeout: std::time::Duration,
) -> Vec<String> {
    for i in 0..retry {
        let r = tokio::time::timeout(timeout, crate::ai::asr(url, lang, wav_audio.clone())).await;
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
    // let wav_reader = hound::WavReader::new(wav_audio)?;
    // let spec = wav_reader.spec();
    // let in_hz = spec.sample_rate;
    // let out_hz = target_sr;

    // let samples = wav_reader.into_samples::<i16>();
    // let mut samples_vec = Vec::with_capacity(samples.len());
    // for sample in samples {
    //     let sample = sample?;
    //     samples_vec.push(sample)
    // }

    let audio = Audio::<Samp16, 1>::with_i16_buffer(in_hz, audio_samples);
    let audio = Audio::<Samp16, 1>::with_audio(out_hz, &audio);

    Ok(audio)
}

async fn retry_tts(
    url: &str,
    speaker: &str,
    text: &str,
    retry: usize,
    timeout: std::time::Duration,
) -> anyhow::Result<Bytes> {
    for i in 0..retry {
        let r = tokio::time::timeout(timeout, crate::ai::tts(url, speaker, text)).await;
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

async fn submit_to_ai(
    pool: &WsPool,
    asr: &crate::config::ASRConfig,
    llm: &crate::config::LLMConfig,
    tts: &crate::config::TTSConfig,
    id: &str,
    wav_audio: Vec<u8>,
    only_asr: bool,
    sys_prompts: &[Content],
    dynamic_prompts: &mut std::collections::LinkedList<Content>,
) -> anyhow::Result<()> {
    // ASR
    let asr_url = &asr.url;
    let lang = asr.lang.as_str();
    std::fs::write(format!("asr.{id}.wav"), &wav_audio).unwrap();
    let text = retry_asr(
        asr_url,
        lang,
        wav_audio,
        3,
        std::time::Duration::from_secs(10),
    )
    .await;

    log::info!("ASR result: {:?}", text);

    if text.is_empty() {
        pool.send(id, WsCommand::AsrResult(vec![])).await?;
        return Ok(());
    }

    let message = hanconv::tw2sp(text.join("\n"));
    pool.send(id, WsCommand::AsrResult(vec![message.clone()]))
        .await?;

    if only_asr {
        return Ok(());
    }

    // LLM
    let token = if let Some(t) = &llm.api_key {
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

    while dynamic_prompts.len() > llm.history * 2 {
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
    let llm_url = &llm.llm_chat_url;
    let mut resp = crate::ai::llm_stable(llm_url, token, None, prompts).await?;

    let mut deadline = None;

    let (tts_url, speaker) = match &tts {
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
                match retry_tts(
                    tts_url,
                    speaker,
                    &chunk,
                    3,
                    std::time::Duration::from_secs(15),
                )
                .await
                {
                    Ok(wav_data) => {
                        if let Some(deadline) = deadline {
                            tokio::time::sleep_until(deadline).await;
                            log::info!("end audio");
                        }

                        let mut buff = Vec::with_capacity(5 * 1600 * 2);
                        let reader = hound::WavReader::new(wav_data.as_ref())
                            .map_err(|e| anyhow::anyhow!("hound error:{e}"))?;

                        let duration_sec =
                            reader.duration() as f32 / reader.spec().sample_rate as f32;

                        deadline = Some(
                            tokio::time::Instant::now()
                                + tokio::time::Duration::from_secs_f32(duration_sec),
                        );

                        let in_hz = reader.spec().sample_rate;
                        let out_hz = 16000;
                        let samples = reader.into_samples::<i16>();
                        let mut samples_vec = Vec::with_capacity(samples.len());
                        for sample in samples {
                            if let Ok(sample) = sample {
                                samples_vec.push(sample)
                            } else {
                                log::warn!("sample error: {}", sample.unwrap_err());
                            }
                        }
                        let mut audio_16k = resample(&samples_vec, in_hz, out_hz)
                            .map_err(|e| anyhow::anyhow!("resample error:{e}"))?;

                        log::info!("llm chunk:{:?}", chunk);

                        pool.send(id, WsCommand::StartAudio(chunk)).await?;

                        let samples = audio_16k.as_i16_slice();
                        for chunk in samples.chunks(5 * out_hz as usize / 10) {
                            for i in chunk {
                                buff.extend_from_slice(&i.to_le_bytes());
                            }
                            // std::mem::swap(&mut send_data, &mut buff);
                            pool.send(id, WsCommand::Audio(buff.into())).await?;
                            buff = Vec::with_capacity(5 * 1600 * 2);
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

async fn submit_to_genai_and_tts(
    pool: &WsPool,
    client: &mut genai::LiveClient,
    tts: &crate::config::TTSConfig,
    id: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<()> {
    // Genai live api
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

    client
        .send_realtime_audio(RealtimeAudio {
            data: Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        })
        .await?;

    let mut text = String::new();
    loop {
        match client.receive().await? {
            genai::types::ServerContent::ModelTurn(turn) => {
                turn.parts.iter().for_each(|part| {
                    if let genai::types::Parts::Text(text_part) = part {
                        text.push_str(&text_part);
                    }
                });
            }
            genai::types::ServerContent::GenerationComplete(_) => {}
            genai::types::ServerContent::Interrupted(_) => {}
            genai::types::ServerContent::TurnComplete(_) => {
                break;
            }
        }
    }

    let message = hanconv::tw2sp(text);
    pool.send(id, WsCommand::AsrResult(vec![message.clone()]))
        .await?;

    log::info!("start llm");

    let (tts_url, speaker) = match &tts {
        crate::config::TTSConfig::Stable(tts) => (&tts.url, &tts.speaker),
        crate::config::TTSConfig::Fish(_) => {
            return Err(anyhow::anyhow!("Fish TTS is not implemented yet"));
        }
    };

    match retry_tts(
        tts_url,
        speaker,
        &message,
        3,
        std::time::Duration::from_secs(15),
    )
    .await
    {
        Ok(wav_data) => {
            let mut buff = Vec::with_capacity(5 * 1600 * 2);
            let reader = hound::WavReader::new(wav_data.as_ref())
                .map_err(|e| anyhow::anyhow!("hound error:{e}"))?;

            let in_hz = reader.spec().sample_rate;
            let out_hz = 16000;
            let samples = reader.into_samples::<i16>();
            let mut samples_vec = Vec::with_capacity(samples.len());
            for sample in samples {
                if let Ok(sample) = sample {
                    samples_vec.push(sample)
                } else {
                    log::warn!("sample error: {}", sample.unwrap_err());
                }
            }
            let mut audio_16k = resample(&samples_vec, in_hz, out_hz)
                .map_err(|e| anyhow::anyhow!("resample error:{e}"))?;

            log::info!("llm response:{:?}", message);

            pool.send(id, WsCommand::StartAudio(message)).await?;

            let samples = audio_16k.as_i16_slice();
            for chunk in samples.chunks(5 * out_hz as usize / 10) {
                for i in chunk {
                    buff.extend_from_slice(&i.to_le_bytes());
                }
                // std::mem::swap(&mut send_data, &mut buff);
                pool.send(id, WsCommand::Audio(buff.into())).await?;
                buff = Vec::with_capacity(5 * 1600 * 2);
            }

            pool.send(id, WsCommand::EndAudio).await?;
        }
        Err(e) => {
            log::error!("tts error:{e}");
        }
    }

    Ok(())
}

async fn submit_to_genai(
    pool: &WsPool,
    client: &mut genai::LiveClient,
    id: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<()> {
    // Genai live api
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

    client
        .send_realtime_audio(RealtimeAudio {
            data: Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        })
        .await?;

    pool.send(id, WsCommand::AsrResult(vec![format!("Wait genai")]))
        .await?;

    let mut buff = Vec::with_capacity(5 * 1600 * 2);

    loop {
        match client.receive().await? {
            genai::types::ServerContent::ModelTurn(turn) => {
                for item in turn.parts {
                    if let genai::types::Parts::InlineData { data, mime_type } = item {
                        if mime_type.starts_with("audio/pcm") {
                            let audio_data = data.into_inner();
                            let sample = unsafe {
                                std::slice::from_raw_parts(
                                    audio_data.as_ptr() as *const i16,
                                    audio_data.len() / 2,
                                )
                            };
                            let mut audio_16k = resample(sample, 24000, 16000)?;
                            let samples = audio_16k.as_i16_slice();
                            for chunk in samples.chunks(5 * 16000 / 10) {
                                for i in chunk {
                                    buff.extend_from_slice(&i.to_le_bytes());
                                }
                                // std::mem::swap(&mut send_data, &mut buff);
                                pool.send(id, WsCommand::Audio(buff.into())).await?;
                                buff = Vec::with_capacity(5 * 1600 * 2);
                            }
                        }
                    }
                }
            }
            genai::types::ServerContent::GenerationComplete(_) => {}
            genai::types::ServerContent::Interrupted(_) => {}
            genai::types::ServerContent::TurnComplete(_) => {
                break;
            }
        }
    }
    pool.send(id, WsCommand::EndAudio).await?;

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

    match &pool.config {
        AIConfig::Stable { llm, tts, asr } => {
            let sys_prompts = &llm.sys_prompts;
            let mut dynamic_prompts = llm.dynamic_prompts.clone();

            loop {
                let rx_recv = tokio::select! {
                    r = rx.recv() =>{
                        r
                    }
                    r = submit_to_ai(&pool, asr, llm, tts, &id, wav_audio, only_asr,sys_prompts,&mut dynamic_prompts) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                        }
                        if let Err(e) = pool.send(&id, WsCommand::EndResponse).await{
                            log::error!("`{id}` error: {e}");
                        };

                        rx.recv().await
                    }
                };
                let r = rx_recv.ok_or_else(|| anyhow::anyhow!("handle_audio rx closed"))?;
                only_asr = r.0;
                wav_audio = r.1;
            }
        }
        AIConfig::GenaiAndTTS { genai, tts } => {
            let mut client = genai::LiveClient::connect(&genai.api_key).await?;
            let model = genai
                .model
                .clone()
                .unwrap_or("models/gemini-2.0-flash-live-001".to_string());

            let mut generation_config = GenerationConfig::default();
            generation_config.response_modalities = Some(vec![genai::types::Modality::TEXT]);

            let system_instruction = if let Some(sys_prompts) = genai.sys_prompts.clone() {
                Some(genai::types::Content {
                    parts: vec![genai::types::Parts::Text(sys_prompts.message)],
                })
            } else {
                None
            };

            let setup = genai::types::Setup {
                model,
                generation_config: Some(generation_config),
                system_instruction,
            };

            client.setup(setup).await?;

            loop {
                let rx_recv = tokio::select! {
                    r = rx.recv() =>{
                        r
                    }
                    r = submit_to_genai_and_tts(&pool, &mut client, tts, &id, wav_audio) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                        }
                        if let Err(e) = pool.send(&id, WsCommand::EndResponse).await{
                            log::error!("`{id}` error: {e}");
                        };

                        rx.recv().await
                    }
                };
                let r = rx_recv.ok_or_else(|| anyhow::anyhow!("handle_audio rx closed"))?;
                wav_audio = r.1;
            }
        }
        AIConfig::Genai { genai } => {
            let mut client = genai::LiveClient::connect(&genai.api_key).await?;
            let model = genai
                .model
                .clone()
                .unwrap_or("models/gemini-2.0-flash-live-001".to_string());

            let mut generation_config = GenerationConfig::default();
            generation_config.response_modalities = Some(vec![genai::types::Modality::AUDIO]);

            let system_instruction = if let Some(sys_prompts) = genai.sys_prompts.clone() {
                Some(genai::types::Content {
                    parts: vec![genai::types::Parts::Text(sys_prompts.message)],
                })
            } else {
                None
            };

            let setup = genai::types::Setup {
                model,
                generation_config: Some(generation_config),
                system_instruction,
            };

            client.setup(setup).await?;

            loop {
                let rx_recv = tokio::select! {
                    r = rx.recv() =>{
                        r
                    }
                    r = submit_to_genai(&pool, &mut client, &id, wav_audio) => {
                        if let Err(e) = r {
                            log::error!("`{id}` error: {e}");
                        }
                        if let Err(e) = pool.send(&id, WsCommand::EndResponse).await{
                            log::error!("`{id}` error: {e}");
                        };

                        rx.recv().await
                    }
                };
                let r = rx_recv.ok_or_else(|| anyhow::anyhow!("handle_audio rx closed"))?;
                wav_audio = r.1;
            }
        }
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
        WsCommand::EndResponse => {
            let end_response = serde_json::to_string(&crate::protocol::JsonCommand::EndResponse)
                .expect("Failed to serialize JsonCommand");
            ws.send(Message::Text(end_response.into())).await?;
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
