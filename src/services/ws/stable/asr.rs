use crate::{
    config::WhisperASRConfig,
    services::ws::{ClientMsg, ClientRx, SAMPLE_RATE},
};

pub type ParaformerASRSession = crate::ai::bailian::realtime_asr::ParaformerRealtimeV2Asr;

pub enum AsrSession {
    Whisper(WhisperASRSession),
    Paraformer(ParaformerASRSession),
}

pub struct WhisperASRSession {
    pub config: WhisperASRConfig,
    pub vad_session: crate::ai::vad::VadSession,
    pub client: reqwest::Client,
}

impl WhisperASRSession {
    pub fn new(config: WhisperASRConfig) -> anyhow::Result<Self> {
        let device = burn::backend::ndarray::NdArrayDevice::default();
        let vad = Box::new(silero_vad_burn::SileroVAD6Model::new(&device)?);
        let vad_session = crate::ai::vad::VadSession::new(&config.vad, vad, device)?;

        let client = reqwest::Client::new();
        Ok(Self {
            config,
            vad_session,
            client,
        })
    }

    pub async fn get_input(&mut self, id: &str, rx: &mut ClientRx) -> anyhow::Result<String> {
        let mut audio_buffer = bytes::BytesMut::new();
        let mut vad_started = false;

        loop {
            let msg = rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

            match msg {
                ClientMsg::Text(input) => {
                    return Ok(input);
                }
                ClientMsg::StartChat => {
                    log::info!("`{id}` starting whisper asr");
                }
                ClientMsg::Submit => {
                    if vad_started {
                        log::info!("`{id}` VAD detected speech, performing ASR");
                        let wav_audio = crate::util::pcm_to_wav(
                            &audio_buffer,
                            crate::util::WavConfig {
                                channels: 1,
                                sample_rate: 16000,
                                bits_per_sample: 16,
                            },
                        );

                        let st = std::time::Instant::now();

                        let text = crate::services::ws::retry_asr(
                            &self.client,
                            &self.config.url,
                            &self.config.api_key,
                            &self.config.model,
                            &self.config.lang,
                            &self.config.prompt,
                            wav_audio,
                            3,
                            std::time::Duration::from_secs(10),
                        )
                        .await;

                        log::info!("`{id}` ASR took: {:?}", st.elapsed());
                        let text = text.join("\n");
                        log::info!("ASR result: {:?}", text);
                        if text.is_empty() || text.trim().starts_with("(") {
                            break Ok(String::new());
                        }
                        break Ok(hanconv::tw2sp(text));
                    } else {
                        log::info!("`{id}` no speech detected by VAD, returning empty input");
                        break Ok(String::new());
                    }
                }
                ClientMsg::AudioChunk(data) => {
                    audio_buffer.extend_from_slice(&data);

                    for chunk in data.chunks(2 * crate::ai::vad::VadSession::vad_chunk_size()) {
                        let audio_chunk = crate::util::convert_samples_i16_bytes_to_f32(chunk);
                        debug_assert!(
                            audio_chunk.len() <= crate::ai::vad::VadSession::vad_chunk_size()
                        );
                        vad_started |= self.vad_session.detect(&audio_chunk)?;
                    }
                }
            }
        }
    }

    pub async fn get_input_with_server_vad(
        &mut self,
        session: &mut super::Session,
    ) -> anyhow::Result<String> {
        let id = &session.id;
        let mut audio_buffer = bytes::BytesMut::new();
        let mut speech_ms = 0;
        let min_speech_ms = 500;

        // wait a StartChat first
        loop {
            let msg = session
                .client_rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

            match msg {
                ClientMsg::StartChat => {
                    log::info!("`{id}` starting whisper asr");
                    self.vad_session.reset_state();
                    break;
                }
                ClientMsg::Text(input) => {
                    return Ok(input);
                }
                _ => {
                    log::warn!(
                        "`{id}` waiting for StartChat, but got other message, returning empty input"
                    );
                    return Ok(String::new());
                }
            }
        }

        // wait for audio and vad result
        let mut silence_ms = 0;

        'vad: loop {
            let msg = session
                .client_rx
                .recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

            match msg {
                ClientMsg::Text(input) => {
                    return Ok(input);
                }
                ClientMsg::StartChat => {
                    log::warn!("`{id}` received duplicate StartChat, ignoring");
                }
                ClientMsg::AudioChunk(data) => {
                    audio_buffer.extend_from_slice(&data);

                    for chunk in data.chunks(2 * crate::ai::vad::VadSession::vad_chunk_size()) {
                        let audio_chunk = crate::util::convert_samples_i16_bytes_to_f32(chunk);
                        debug_assert!(
                            audio_chunk.len() <= crate::ai::vad::VadSession::vad_chunk_size()
                        );

                        let chunk_ms =
                            audio_chunk.len() / (crate::services::ws::SAMPLE_RATE as usize / 1000);

                        let is_speech = self.vad_session.detect(&audio_chunk)?;
                        log::debug!(
                            "`{id}` server VAD detected speech: {}, {}",
                            is_speech,
                            speech_ms
                        );
                        if is_speech {
                            speech_ms += chunk_ms;
                            silence_ms = 0;
                        } else {
                            silence_ms += chunk_ms;
                            if speech_ms >= min_speech_ms || silence_ms > 1000 * 5 {
                                log::info!("`{id}` server VAD detected speech");
                                break 'vad;
                            }
                        }
                    }
                }
                ClientMsg::Submit => {
                    log::info!("`{id}` received Submit from client");
                    break;
                }
            }
        }

        session
            .cmd_tx
            .send(crate::services::ws::WsCommand::EndVad)?;

        if speech_ms <= min_speech_ms {
            log::info!("`{id}` no speech detected by server VAD, returning empty input");
            return Ok(String::new());
        }

        // start asr
        let wav_audio = crate::util::pcm_to_wav(
            &audio_buffer,
            crate::util::WavConfig {
                channels: 1,
                sample_rate: SAMPLE_RATE as u32,
                bits_per_sample: 16,
            },
        );

        let st = std::time::Instant::now();

        let text = crate::services::ws::retry_asr(
            &self.client,
            &self.config.url,
            &self.config.api_key,
            &self.config.model,
            &self.config.lang,
            &self.config.prompt,
            wav_audio,
            3,
            std::time::Duration::from_secs(10),
        )
        .await;

        log::info!("`{id}` ASR took: {:?}", st.elapsed());
        let text = text.join("\n");
        log::info!("ASR result: {:?}", text);

        if text.is_empty() || text.trim().starts_with("(") {
            Ok(String::new())
        } else {
            Ok(hanconv::tw2sp(text))
        }
    }
}

impl ParaformerASRSession {
    pub async fn get_input(&mut self, id: &str, rx: &mut ClientRx) -> anyhow::Result<String> {
        while let Some(chunk) = rx.recv().await {
            match chunk {
                ClientMsg::Text(input) => {
                    return Ok(input);
                }
                ClientMsg::AudioChunk(data) => {
                    self.send_audio(data).await.map_err(|e| {
                        anyhow::anyhow!("`{id}` error sending paraformer asr audio: {e}")
                    })?;
                }
                ClientMsg::Submit => {
                    break;
                }
                ClientMsg::StartChat => {
                    log::info!("`{id}` starting paraformer asr");
                    if let Err(e) = self.start_pcm_recognition().await {
                        log::warn!(
                            "`{id}` error starting paraformer asr: {e}, attempting to reconnect..."
                        );
                        self.reconnect().await.map_err(|e| {
                            anyhow::anyhow!("`{id}` error reconnecting paraformer asr: {e}")
                        })?;
                        log::info!("`{id}` paraformer asr reconnected successfully");
                        self.start_pcm_recognition().await.map_err(|e| {
                            anyhow::anyhow!("`{id}` error starting paraformer asr: {e}")
                        })?;
                    }

                    continue;
                }
            }
        }

        self.finish_task()
            .await
            .map_err(|e| anyhow::anyhow!("`{id}` error finishing paraformer asr task: {e}"))?;

        let mut text = String::new();
        while let Some(sentence) = self
            .next_result()
            .await
            .map_err(|e| anyhow::anyhow!("`{id}` error getting paraformer asr result: {e}"))?
        {
            if sentence.sentence_end {
                text = sentence.text;
                log::info!("paraformer ASR final result: {:?}", text);
                break;
            }
        }
        Ok(text)
    }
}

impl AsrSession {
    pub async fn new_from_config(config: &crate::config::ASRConfig) -> anyhow::Result<AsrSession> {
        match config {
            crate::config::ASRConfig::Whisper(whisper_config) => Ok(AsrSession::Whisper(
                WhisperASRSession::new(whisper_config.clone())?,
            )),
            crate::config::ASRConfig::ParaformerV2(paraformer_config) => {
                let session = ParaformerASRSession::connect(
                    &paraformer_config.url,
                    paraformer_config.paraformer_token.clone(),
                    16000,
                )
                .await
                .map_err(|e| anyhow::anyhow!("error connecting paraformer asr websocket: {e}"))?;
                Ok(AsrSession::Paraformer(session))
            }
        }
    }

    pub async fn get_input(&mut self, id: &str, rx: &mut ClientRx) -> anyhow::Result<String> {
        match self {
            AsrSession::Whisper(session) => session.get_input(id, rx).await,
            AsrSession::Paraformer(session) => match session.get_input(id, rx).await {
                Ok(text) => Ok(text),
                Err(e) => {
                    log::error!("`{id}` paraformer asr error: {e}, attempting to reconnect...");
                    session.reconnect().await.map_err(|e| {
                        anyhow::anyhow!("`{id}` error reconnecting paraformer asr: {e}")
                    })?;
                    log::info!("`{id}` paraformer asr reconnected successfully");
                    Ok(String::new())
                }
            },
        }
    }

    pub async fn get_input_with_server_vad(
        &mut self,
        session: &mut super::Session,
    ) -> anyhow::Result<String> {
        match self {
            AsrSession::Whisper(asr_session) => {
                asr_session.get_input_with_server_vad(session).await
            }
            AsrSession::Paraformer(asr_session) => {
                let id = &session.id;
                match asr_session.get_input(id, &mut session.client_rx).await {
                    Ok(text) => Ok(text),
                    Err(e) => {
                        log::error!("`{id}` paraformer asr error: {e}, attempting to reconnect...");
                        asr_session.reconnect().await.map_err(|e| {
                            anyhow::anyhow!("`{id}` error reconnecting paraformer asr: {e}")
                        })?;
                        log::info!("`{id}` paraformer asr reconnected successfully");
                        Ok(String::new())
                    }
                }
            }
        }
    }
}
