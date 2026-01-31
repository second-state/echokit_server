use bytes::{BufMut, Bytes};

use crate::config::{ElevenlabsTTS, FishTTS, GSVTTS, GroqTTS, OpenaiTTS, StreamGSV};

pub type TTSRequest = (String, TTSResponseTx);

pub type TTSRequestTx = tokio::sync::mpsc::Sender<TTSRequest>;
pub type TTSRequestRx = tokio::sync::mpsc::Receiver<TTSRequest>;

pub type TTSResponseRx = tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>;
pub type TTSResponseTx = tokio::sync::mpsc::UnboundedSender<Vec<u8>>;

pub enum TTSSession {
    GsvStable {
        config: GSVTTS,
        client: reqwest::Client,
    },
    GsvStream {
        config: StreamGSV,
        client: reqwest::Client,
    },
    OpenAI {
        config: OpenaiTTS,
        client: reqwest::Client,
    },
    Groq {
        config: GroqTTS,
        client: reqwest::Client,
    },
    Fish {
        config: FishTTS,
    },
    CosyVoice {
        session: crate::ai::bailian::cosyvoice::CosyVoiceTTS,
        version: crate::ai::bailian::cosyvoice::CosyVoiceVersion,
        speaker: Option<String>,
    },
    Elevenlabs {
        config: ElevenlabsTTS,
        client: reqwest::Client,
    },
}

impl TTSSession {
    pub async fn new_from_config(config: &crate::config::TTSConfig) -> anyhow::Result<Self> {
        match config {
            crate::config::TTSConfig::GSV(stable_tts) => Ok(TTSSession::GsvStable {
                config: stable_tts.clone(),
                client: reqwest::Client::new(),
            }),
            crate::config::TTSConfig::Fish(fish_tts) => Ok(TTSSession::Fish {
                config: fish_tts.clone(),
            }),
            crate::config::TTSConfig::Openai(openai_tts) => Ok(TTSSession::OpenAI {
                config: openai_tts.clone(),
                client: reqwest::Client::new(),
            }),
            crate::config::TTSConfig::Groq(groq_tts) => Ok(TTSSession::Groq {
                config: groq_tts.clone(),
                client: reqwest::Client::new(),
            }),
            crate::config::TTSConfig::StreamGSV(stream_gsv) => Ok(TTSSession::GsvStream {
                config: stream_gsv.clone(),
                client: reqwest::Client::new(),
            }),
            crate::config::TTSConfig::CosyVoice(cosy_voice_tts) => {
                let tts = crate::ai::bailian::cosyvoice::CosyVoiceTTS::connect(
                    &cosy_voice_tts.url,
                    cosy_voice_tts.token.clone(),
                )
                .await?;
                Ok(TTSSession::CosyVoice {
                    session: tts,
                    version: cosy_voice_tts.version,
                    speaker: cosy_voice_tts.speaker.clone(),
                })
            }
            crate::config::TTSConfig::Elevenlabs(elevenlabs_tts) => Ok(TTSSession::Elevenlabs {
                config: elevenlabs_tts.clone(),
                client: reqwest::Client::new(),
            }),
        }
    }

    pub async fn synthesize(
        &mut self,
        text: &str,
        tts_resp_tx: &TTSResponseTx,
    ) -> anyhow::Result<()> {
        match self {
            TTSSession::GsvStable { config, client } => {
                gsv_stable_tts(config, client, text, tts_resp_tx).await
            }
            TTSSession::GsvStream { config, client } => {
                gsv_stream_tts(config, client, text, tts_resp_tx).await
            }
            TTSSession::Groq { config, client } => {
                groq_tts(config, client, text, tts_resp_tx).await
            }
            TTSSession::OpenAI { config, client } => {
                openai_tts(config, client, text, tts_resp_tx).await
            }
            TTSSession::Fish { config } => fish_tts(&config.url, config, text, tts_resp_tx).await,
            TTSSession::CosyVoice {
                session,
                version,
                speaker,
            } => {
                let first = cosyvoice_tts(session, *version, speaker, text, tts_resp_tx).await;
                if first.is_err() && !tts_resp_tx.is_closed() {
                    log::warn!("CosyVoice TTS error, reconnecting and retrying...");
                    session.reconnect().await?;
                    cosyvoice_tts(session, *version, speaker, text, tts_resp_tx).await
                } else {
                    first
                }
            }
            TTSSession::Elevenlabs { config, client } => {
                elevenlabs_tts(client, config, text, tts_resp_tx).await
            }
        }
    }
}

pub struct TTSManager {
    config: crate::config::TTSConfig,
}

impl deadpool::managed::Manager for TTSManager {
    type Type = TTSSession;
    type Error = anyhow::Error;

    async fn create(&self) -> Result<TTSSession, anyhow::Error> {
        TTSSession::new_from_config(&self.config).await
    }

    async fn recycle(
        &self,
        _obj: &mut TTSSession,
        _metrics: &deadpool::managed::Metrics,
    ) -> deadpool::managed::RecycleResult<anyhow::Error> {
        Ok(())
    }
}

pub struct TTSSessionPool {
    pool: deadpool::managed::Pool<TTSManager>,
}

impl TTSSessionPool {
    pub fn new(config: crate::config::TTSConfig, workers: usize) -> Self {
        let manager = TTSManager { config };
        let pool = deadpool::managed::Pool::builder(manager)
            .max_size(workers)
            .build()
            .expect("Failed to create TTS session pool");
        TTSSessionPool { pool }
    }

    pub async fn run_loop(&mut self, mut rx: TTSRequestRx) -> anyhow::Result<()> {
        while let Some((text, tts_resp_tx)) = rx.recv().await {
            match self.pool.get().await {
                Ok(mut session) => {
                    tokio::spawn(async move {
                        log::info!("Processing TTS request: {}", text);
                        if let Err(e) = session.synthesize(&text, &tts_resp_tx).await {
                            log::error!("TTS synthesis error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("Failed to get TTS session from pool: {}", e);
                }
            }
        }
        Ok(())
    }
}

async fn retry_gsv_tts(
    client: &reqwest::Client,
    url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
    retry: usize,
    timeout: std::time::Duration,
    llm_voice_opt: Option<&crate::config::TTSTextOptimizationConfig>,
) -> anyhow::Result<Bytes> {
    for i in 0..retry {
        let r = tokio::time::timeout(
            timeout,
            crate::ai::tts::gsv(client, url, speaker, text, sample_rate, llm_voice_opt),
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

async fn gsv_stable_tts(
    tts: &GSVTTS,
    client: &reqwest::Client,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let wav_data = retry_gsv_tts(
        client,
        &tts.url,
        &tts.speaker,
        text,
        Some(16000),
        3,
        std::time::Duration::from_secs(tts.timeout_sec.unwrap_or(15)),
        tts.text_optimization.as_ref(),
    )
    .await?;

    send_wav(tts_resp_tx, wav_data).await?;
    Ok(())
}

async fn send_gsv_stream_chunk(
    tts_resp_tx: &TTSResponseTx,
    resp: reqwest::Response,
) -> anyhow::Result<f32> {
    use futures_util::StreamExt;

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
                tts_resp_tx
                    .send(audio_16k)
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
            tts_resp_tx
                .send(audio_16k)
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
        }
    }

    if rest.len() > 0 {
        let audio_16k = rest.to_vec();
        log::trace!("Sending audio chunk of size: {}", audio_16k.len());
        duration_sec += audio_16k.len() as f32 / 32000.0;
        tts_resp_tx
            .send(audio_16k)
            .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
    }

    Ok(duration_sec)
}

async fn gsv_stream_tts(
    tts: &StreamGSV,
    client: &reqwest::Client,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let resp = crate::ai::tts::stream_gsv(
        client,
        &tts.url,
        &tts.speaker,
        text,
        Some(16000),
        tts.text_optimization.as_ref(),
    )
    .await?;

    send_gsv_stream_chunk(tts_resp_tx, resp).await?;
    Ok(())
}

async fn openai_tts(
    tts: &OpenaiTTS,
    client: &reqwest::Client,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let wav_data =
        crate::ai::tts::openai_tts(client, &tts.url, &tts.model, &tts.api_key, &tts.voice, text)
            .await?;

    send_wav(tts_resp_tx, wav_data).await?;
    Ok(())
}

async fn groq_tts(
    tts: &GroqTTS,
    client: &reqwest::Client,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let wav_data =
        crate::ai::tts::groq(client, &tts.url, &tts.model, &tts.api_key, &tts.voice, text).await?;

    send_wav(tts_resp_tx, wav_data).await?;
    Ok(())
}

async fn fish_tts(
    url: &str,
    tts: &FishTTS,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let wav_data = crate::ai::tts::fish_tts(url, &tts.api_key, &tts.speaker, text).await?;

    send_wav(tts_resp_tx, wav_data).await?;
    Ok(())
}

async fn cosyvoice_tts(
    session: &mut crate::ai::bailian::cosyvoice::CosyVoiceTTS,
    version: crate::ai::bailian::cosyvoice::CosyVoiceVersion,
    speaker: &Option<String>,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    session
        .start_synthesis(version, speaker.as_deref(), Some(16000), text)
        .await?;

    while let Some(chunk) = session.next_audio_chunk().await? {
        tts_resp_tx
            .send(chunk.to_vec())
            .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
    }

    Ok(())
}

async fn elevenlabs_tts(
    client: &reqwest::Client,
    elevenlabs_tts: &ElevenlabsTTS,
    text: &str,
    tts_resp_tx: &TTSResponseTx,
) -> anyhow::Result<()> {
    let mut session = crate::ai::elevenlabs::tts::ElevenlabsTTS::new_with_client(
        &elevenlabs_tts.url,
        client,
        elevenlabs_tts.token.clone(),
        elevenlabs_tts.voice.clone(),
        crate::ai::elevenlabs::tts::OutputFormat::Pcm16000,
        &elevenlabs_tts.model_id,
        &elevenlabs_tts.language_code,
    )
    .await?;

    session.send_text(text, true).await?;
    session.close_connection().await?;

    while let Ok(Some(resp)) = session.next_audio_response().await {
        if let Some(audio) = resp.get_audio_bytes() {
            tts_resp_tx
                .send(audio.to_vec())
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
        }
    }

    Ok(())
}

async fn send_wav(tts_resp_tx: &TTSResponseTx, wav_data: Bytes) -> anyhow::Result<()> {
    let mut reader = wav_io::reader::Reader::from_vec(wav_data.into())
        .map_err(|e| anyhow::anyhow!("wav_io reader error: {e}"))?;

    let header = reader.read_header()?;
    let mut samples = crate::util::get_samples_f32(&mut reader)
        .map_err(|e| anyhow::anyhow!("get_samples_f32 error: {e}"))?;

    let out_hz = 16000;

    if header.sample_rate != out_hz {
        // resample to 16000
        log::debug!("resampling from {} to 16000", header.sample_rate);
        samples = wav_io::resample::linear(samples, header.channels, header.sample_rate, out_hz);
    }
    let audio_16k = wav_io::convert_samples_f32_to_i16(&samples);

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
        tts_resp_tx.send(buff)?;
    }

    Ok(())
}
