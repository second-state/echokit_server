use bytes::{BufMut, Bytes};
use std::time::{Duration, Instant};
use std::{cell::Cell, sync::Mutex};

use crate::config::{ElevenlabsTTS, FishTTS, GSVTTS, GroqTTS, OpenaiTTS, StreamGSV};

pub type TTSRequest = (String, TTSResponseTx, TTSRequestAckTx);

pub type TTSRequestTx = tokio::sync::mpsc::Sender<TTSRequest>;
pub type TTSRequestRx = tokio::sync::mpsc::Receiver<TTSRequest>;
pub type TTSRequestAckTx = tokio::sync::oneshot::Sender<anyhow::Result<()>>;

pub type TTSResponseRx = tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>;
pub type TTSResponseTx = tokio::sync::mpsc::UnboundedSender<Vec<u8>>;

pub async fn submit_request(tts_tx: &TTSRequestTx, text: String) -> anyhow::Result<TTSResponseRx> {
    let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();
    let (request_ack_tx, request_ack_rx) = tokio::sync::oneshot::channel();

    tts_tx
        .send((text, tts_resp_tx, request_ack_tx))
        .await
        .map_err(|e| anyhow::anyhow!("error sending tts request: {e}"))?;

    request_ack_rx
        .await
        .map_err(|e| anyhow::anyhow!("TTS pool dropped request acknowledgement: {e}"))??;

    Ok(tts_resp_rx)
}

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

/// Default number of sessions kept ready while the pool is idle.
pub const DEFAULT_TTS_IDLE_WORKERS: usize = 1;

/// Default upper bound for concurrently leased TTS sessions.
pub const DEFAULT_TTS_MAX_WORKERS: usize = 4;

/// Default time after which an idle session is considered stale and reaped.
pub const DEFAULT_TTS_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Default hard cap on how long a session may live before being discarded on
/// the next recycle check.
pub const DEFAULT_TTS_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// Pooled entry wrapping a `TTSSession` together with the timestamps used to
/// enforce idle / max-lifetime eviction.
pub struct TtsSessionEntry {
    pub session: TTSSession,
    pub created_at: Instant,
    pub last_used: Mutex<Instant>,
}

impl TtsSessionEntry {
    pub fn new(session: TTSSession) -> Self {
        let now = Instant::now();
        Self {
            session,
            created_at: now,
            last_used: Mutex::new(now),
        }
    }

    pub fn touch(&self) {
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }
    }
}

pub struct TTSManager {
    config: crate::config::TTSConfig,
    idle_timeout: Duration,
    max_lifetime: Duration,
}

impl TTSManager {
    fn new(
        config: crate::config::TTSConfig,
        idle_timeout: Duration,
        max_lifetime: Duration,
    ) -> Self {
        Self {
            config,
            idle_timeout,
            max_lifetime,
        }
    }
}

#[async_trait::async_trait]
impl deadpool::managed::Manager for TTSManager {
    type Type = TtsSessionEntry;
    type Error = anyhow::Error;

    async fn create(&self) -> Result<TtsSessionEntry, anyhow::Error> {
        let session = TTSSession::new_from_config(&self.config).await?;
        Ok(TtsSessionEntry::new(session))
    }

    async fn recycle(
        &self,
        obj: &mut TtsSessionEntry,
        _metrics: &deadpool::managed::Metrics,
    ) -> deadpool::managed::RecycleResult<anyhow::Error> {
        let now = Instant::now();
        let age = now.saturating_duration_since(obj.created_at);
        if age >= self.max_lifetime {
            log::info!("TTS session exceeded max lifetime ({:?}); discarding", age);
            return Err(deadpool::managed::RecycleError::Message(
                "max lifetime exceeded".to_string(),
            ));
        }

        let last_used = *obj
            .last_used
            .lock()
            .map_err(|e| deadpool::managed::RecycleError::Message(e.to_string()))?;
        let idle = now.saturating_duration_since(last_used);
        if idle >= self.idle_timeout {
            log::info!("TTS session idle for {:?}; discarding", idle);
            return Err(deadpool::managed::RecycleError::Message(
                "idle timeout exceeded".to_string(),
            ));
        }

        Ok(())
    }
}

pub struct TTSSessionPool {
    pool: deadpool::managed::Pool<TTSManager>,
    idle_workers: usize,
    idle_timeout: Duration,
}

impl TTSSessionPool {
    pub fn new(
        config: crate::config::TTSConfig,
        idle_workers: usize,
        max_workers: usize,
        idle_timeout: Duration,
    ) -> Self {
        Self::with_timeouts(
            config,
            idle_workers,
            max_workers,
            idle_timeout,
            DEFAULT_TTS_MAX_LIFETIME,
        )
    }

    pub fn with_timeouts(
        config: crate::config::TTSConfig,
        idle_workers: usize,
        max_workers: usize,
        idle_timeout: Duration,
        max_lifetime: Duration,
    ) -> Self {
        let max_workers = max_workers.max(idle_workers);
        let manager = TTSManager::new(config, idle_timeout, max_lifetime);
        let pool = deadpool::managed::Pool::builder(manager)
            .max_size(max_workers)
            .timeouts(deadpool::managed::Timeouts {
                wait: Some(Duration::from_secs(30)),
                create: Some(Duration::from_secs(30)),
                // Cap how long a single recycle() call may take; should be
                // cheap but a stuck call must not stall pool.get().
                recycle: Some(Duration::from_secs(5)),
            })
            .runtime(deadpool::Runtime::Tokio1)
            .build()
            .expect("Failed to create TTS session pool");
        TTSSessionPool {
            pool,
            idle_workers,
            idle_timeout,
        }
    }

    async fn prewarm(&self) -> anyhow::Result<()> {
        let mut entries = Vec::with_capacity(self.idle_workers);
        for worker in 0..self.idle_workers {
            let entry = self
                .pool
                .get()
                .await
                .map_err(|e| anyhow::anyhow!("create idle TTS session[{worker}] error: {e}"))?;
            entries.push(entry);
        }
        drop(entries);
        Ok(())
    }

    fn reap_idle(&self) {
        let status = self.pool.status();
        let removable = Cell::new(status.available.saturating_sub(self.idle_workers));
        if removable.get() == 0 {
            return;
        }

        let now = Instant::now();
        let idle_timeout = self.idle_timeout;
        self.pool.retain(|entry, _| {
            let remaining = removable.get();
            if remaining == 0 {
                return true;
            }

            let idle = entry
                .last_used
                .lock()
                .map(|last_used| now.saturating_duration_since(*last_used) >= idle_timeout)
                .unwrap_or(true);
            if idle {
                removable.set(remaining - 1);
                false
            } else {
                true
            }
        });
    }

    pub async fn run_loop(&mut self, mut rx: TTSRequestRx) -> anyhow::Result<()> {
        if let Err(e) = self.prewarm().await {
            log::warn!("initial TTS pool prewarm failed; retrying on demand: {e}");
        }

        let reap_period = self.idle_timeout.max(Duration::from_millis(1));
        let mut reap_interval = tokio::time::interval(reap_period);
        reap_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                request = rx.recv() => {
                    let Some((text, tts_resp_tx, request_ack_tx)) = request else {
                        break;
                    };

                    match self.pool.get().await {
                        Ok(mut entry) => {
                            let _ = request_ack_tx.send(Ok(()));
                            tokio::spawn(async move {
                                log::info!("Processing TTS request: {}", text);
                                let result = entry.session.synthesize(&text, &tts_resp_tx).await;
                                entry.touch();
                                if let Err(e) = result {
                                    log::error!("TTS synthesis error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            let message = format!("Failed to get TTS session from pool: {e}");
                            log::error!("{message}");
                            let _ = request_ack_tx.send(Err(anyhow::anyhow!(message)));
                        }
                    }
                }
                _ = reap_interval.tick() => self.reap_idle(),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GSVTTS, TTSConfig};

    fn test_config() -> TTSConfig {
        TTSConfig::GSV(GSVTTS {
            api_key: String::new(),
            url: String::new(),
            speaker: String::new(),
            timeout_sec: None,
            text_optimization: None,
        })
    }

    #[tokio::test]
    async fn prewarms_idle_workers_and_scales_to_max_workers() {
        let pool = TTSSessionPool::with_timeouts(
            test_config(),
            2,
            3,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );

        pool.prewarm().await.unwrap();
        assert_eq!(pool.pool.status().size, 2);

        let (first, second, third) =
            tokio::join!(pool.pool.get(), pool.pool.get(), pool.pool.get(),);
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert!(third.is_ok());
        assert_eq!(pool.pool.status().size, 3);
    }

    #[tokio::test]
    async fn reaper_keeps_idle_workers_and_removes_excess_idle_sessions() {
        let pool = TTSSessionPool::with_timeouts(
            test_config(),
            1,
            3,
            Duration::from_millis(10),
            Duration::from_secs(60),
        );
        let (first, second, third) =
            tokio::join!(pool.pool.get(), pool.pool.get(), pool.pool.get(),);
        drop((first, second, third));

        tokio::time::sleep(Duration::from_millis(20)).await;
        pool.reap_idle();
        assert_eq!(pool.pool.status().size, 1);
    }

    #[tokio::test]
    async fn touching_a_session_after_use_prevents_premature_idle_reaping() {
        let pool = TTSSessionPool::with_timeouts(
            test_config(),
            0,
            1,
            Duration::from_millis(100),
            Duration::from_secs(60),
        );
        let entry = pool.pool.get().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        entry.touch();
        drop(entry);

        tokio::time::sleep(Duration::from_millis(20)).await;
        pool.reap_idle();
        assert_eq!(pool.pool.status().size, 1);
    }

    #[tokio::test]
    async fn recycle_replaces_expired_sessions() {
        let pool = TTSSessionPool::with_timeouts(
            test_config(),
            0,
            1,
            Duration::from_secs(60),
            Duration::from_millis(10),
        );
        let entry = pool.pool.get().await.unwrap();
        let created_at = entry.created_at;
        drop(entry);

        tokio::time::sleep(Duration::from_millis(20)).await;
        let replacement = pool.pool.get().await.unwrap();
        assert!(replacement.created_at > created_at);
    }

    #[tokio::test]
    async fn submit_request_propagates_pool_acquisition_errors() {
        let (tts_tx, mut tts_rx) = tokio::sync::mpsc::channel(1);
        let request =
            tokio::spawn(async move { submit_request(&tts_tx, "hello".to_string()).await });

        let (_, _, request_ack_tx) = tts_rx.recv().await.unwrap();
        request_ack_tx
            .send(Err(anyhow::anyhow!("pool unavailable")))
            .unwrap();

        let result = request.await.unwrap();
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("pool unavailable")
        );
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
