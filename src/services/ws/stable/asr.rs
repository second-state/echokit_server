use crate::{
    ai::bailian::realtime_asr::ResponsePayloadOutputSentence,
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

    pub async fn stream_get_input(
        &mut self,
        session: &mut super::Session,
    ) -> anyhow::Result<String> {
        let mut audio_buffer = bytes::BytesMut::new();

        async fn wait_for_chat_start(
            session: &mut super::Session,
        ) -> anyhow::Result<Option<String>> {
            let id = &session.id;
            loop {
                let msg = session
                    .client_rx
                    .recv()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

                match msg {
                    ClientMsg::StartChat => {
                        log::info!("`{id}` starting whisper asr");
                        return Ok(None);
                    }
                    ClientMsg::Text(input) => {
                        return Ok(Some(input));
                    }
                    ClientMsg::AudioChunk(_) => {
                        continue;
                    }
                    _ => {
                        log::warn!(
                            "`{id}` waiting for StartChat, but got other message, returning empty input"
                        );
                        return Ok(Some(String::new()));
                    }
                }
            }
        }

        async fn wait_audio(
            session: &mut super::Session,
            audio_buffer: &mut bytes::BytesMut,
            vad_session: &mut crate::ai::vad::VadSession,
            cancel_tx: tokio::sync::oneshot::Sender<()>,
            hangover_ms: usize,
            min_speech_ms: usize,
        ) -> anyhow::Result<Option<String>> {
            let mut speech_ms = 0;

            let id = &session.id;
            let mut silence_ms = 0;

            'vad: loop {
                let msg = session
                    .client_rx
                    .recv()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("client rx channel closed"))?;

                match msg {
                    ClientMsg::Text(input) => {
                        return Ok(Some(input));
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

                            let chunk_ms = audio_chunk.len()
                                / (crate::services::ws::SAMPLE_RATE as usize / 1000);

                            let is_speech = vad_session.detect(&audio_chunk)?;
                            if is_speech {
                                speech_ms += chunk_ms;
                                silence_ms = 0;
                            } else {
                                silence_ms += chunk_ms;
                                if speech_ms >= min_speech_ms {
                                    log::info!(
                                        "`{id}` server VAD detected speech end, returning input"
                                    );
                                    break 'vad;
                                }

                                if silence_ms > hangover_ms {
                                    log::info!(
                                        "`{id}` server VAD detected long silence, returning empty input"
                                    );
                                    break 'vad;
                                }
                            }
                        }
                    }
                    ClientMsg::Submit => {
                        log::warn!("`{id}` received a Unexpected Submit during Stream ASR");
                        return Err(anyhow::anyhow!("Unexpected Submit during Stream ASR"));
                    }
                }
            }

            if speech_ms <= min_speech_ms {
                log::info!("`{id}` no speech detected by server VAD, returning empty input");
                Ok(Some(String::new()))
            } else {
                let _ = cancel_tx.send(());
                Ok(None)
            }
        }

        async fn wait_asr(
            client: &reqwest::Client,
            config: &WhisperASRConfig,
            wav_audio: Vec<u8>,
            cancel_: tokio::sync::oneshot::Receiver<()>,
        ) -> String {
            if wav_audio.is_empty() {
                return String::new();
            }

            let now = std::time::Instant::now();
            log::info!(
                "`Starting ASR request, audio size: {} bytes",
                wav_audio.len()
            );

            let asr_text_fut = crate::services::ws::retry_asr(
                client,
                &config.url,
                &config.api_key,
                &config.model,
                &config.lang,
                &config.prompt,
                wav_audio,
                3,
                std::time::Duration::from_secs(10),
            );

            struct NeverReady;
            impl std::future::Future for NeverReady {
                type Output = ();
                fn poll(
                    self: std::pin::Pin<&mut Self>,
                    _: &mut std::task::Context<'_>,
                ) -> std::task::Poll<Self::Output> {
                    std::task::Poll::Pending
                }
            }

            let cancel_fut = async {
                let r = cancel_.await;
                if r.is_err() {
                    NeverReady.await
                } else {
                    log::info!("ASR request cancelled");
                }
            };

            let text = tokio::select! {
                text = asr_text_fut => {
                    log::info!("ASR request took: {:?}", now.elapsed());
                    log::info!("ASR result: {:?}", text);
                    text
                }
                _ = cancel_fut => {
                    vec![]
                }
            };

            let text = text.join("\n");

            if text.is_empty() || text.trim().starts_with("(") {
                String::new()
            } else {
                hanconv::tw2sp(text)
            }
        }

        // wait a StartChat first
        if let Some(input) = wait_for_chat_start(session).await? {
            return Ok(input);
        }
        self.vad_session.reset_state();

        let mut text;

        loop {
            // wait for audio and vad result

            let wav_audio = if audio_buffer.is_empty() {
                Vec::new()
            } else {
                crate::util::pcm_to_wav(
                    &audio_buffer,
                    crate::util::WavConfig {
                        channels: 1,
                        sample_rate: SAMPLE_RATE as u32,
                        bits_per_sample: 16,
                    },
                )
            };

            let (tx, rx) = tokio::sync::oneshot::channel();
            let asr_fut = wait_asr(&self.client, &self.config, wav_audio, rx);
            let wait_audio_fut = wait_audio(
                session,
                &mut audio_buffer,
                &mut self.vad_session,
                tx,
                self.config.vad.hangover_ms,
                self.config.vad.min_speech_duration_ms,
            );

            let (audio_r, asr_r) = tokio::join!(wait_audio_fut, asr_fut);
            let audio_r = audio_r?;
            log::debug!(
                "`{}` got audio_r: {:?}, asr_r: {}",
                &session.id,
                audio_r,
                asr_r
            );
            if let Some(t) = audio_r {
                if t.is_empty() {
                    text = asr_r;
                } else {
                    text = t;
                }
                break;
            } else {
                log::debug!("continuing to wait for more audio..., asr_r: {}", asr_r);
                if !asr_r.is_empty() {
                    text = asr_r;
                    session.send_asr_result(vec![text.clone()])?;
                }

                continue;
            }
        }

        session.send_end_vad()?;

        Ok(text)
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
                    if let Err(e) = self.start_pcm_recognition(false).await {
                        log::warn!(
                            "`{id}` error starting paraformer asr: {e}, attempting to reconnect..."
                        );
                        self.reconnect().await.map_err(|e| {
                            anyhow::anyhow!("`{id}` error reconnecting paraformer asr: {e}")
                        })?;
                        log::info!("`{id}` paraformer asr reconnected successfully");
                        self.start_pcm_recognition(false).await.map_err(|e| {
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

    pub async fn stream_get_input(
        &mut self,
        session: &mut super::Session,
    ) -> anyhow::Result<String> {
        log::info!("`{}` paraformer start stream asr", session.id);
        let mut start_submit = false;

        enum SelectResult {
            ClientMsg(Option<ClientMsg>),
            AsrResult(anyhow::Result<Option<ResponsePayloadOutputSentence>>),
        }

        let mut asr_text = String::new();
        let mut recv_any_asr_result = false;
        let mut recv_audio_bytes = 0;

        loop {
            let r = if start_submit {
                let client_fut = session.client_rx.recv();
                let asr_fut = self.next_result();
                tokio::select! {
                    asr = asr_fut => SelectResult::AsrResult(asr),
                    chunk = client_fut => SelectResult::ClientMsg(chunk),
                }
            } else {
                let chunk = session.client_rx.recv().await;
                SelectResult::ClientMsg(chunk)
            };

            match r {
                SelectResult::ClientMsg(chunk) => {
                    if let Some(chunk) = chunk {
                        match chunk {
                            ClientMsg::Text(input) => {
                                return Ok(input);
                            }
                            ClientMsg::AudioChunk(data) => {
                                recv_audio_bytes += data.len();
                                if !recv_any_asr_result && recv_audio_bytes >= 16000 * 10 {
                                    log::warn!(
                                        "`{}` paraformer asr received more than 30s audio without StartChat, starting automatically",
                                        session.id
                                    );
                                    break;
                                }
                                // skip sending audio if not started yet
                                if start_submit {
                                    self.send_audio(data).await.map_err(|e| {
                                        anyhow::anyhow!(
                                            "`{}` error sending paraformer asr audio: {e}",
                                            session.id
                                        )
                                    })?;
                                } else {
                                    log::debug!(
                                        "`{}` paraformer asr not started yet, ignoring audio chunk",
                                        session.id
                                    );
                                }
                            }
                            ClientMsg::Submit => {
                                log::warn!(
                                    "`{}` received a Unexpected Submit during Stream ASR",
                                    session.id
                                );
                                return Err(anyhow::anyhow!("Unexpected Submit during Stream ASR"));
                            }
                            ClientMsg::StartChat => {
                                log::info!("`{}` starting paraformer asr", session.id);
                                if start_submit {
                                    log::warn!(
                                        "`{}` paraformer asr already started, ignoring duplicate StartChat",
                                        session.id
                                    );
                                    continue;
                                }
                                if let Err(e) = self.start_pcm_recognition(true).await {
                                    log::warn!(
                                        "`{}` error starting paraformer asr: {e}, attempting to reconnect...",
                                        session.id
                                    );
                                    for i in 0..3 {
                                        if let Err(e) = self.reconnect().await {
                                            log::warn!(
                                                "`{}` attempt {} error reconnecting paraformer asr: {e}",
                                                session.id,
                                                i + 1
                                            );
                                            if i == 2 {
                                                return Err(anyhow::anyhow!(
                                                    "`{}` failed to reconnect paraformer asr after 3 attempts: {e}",
                                                    session.id
                                                ));
                                            }
                                        } else {
                                            break;
                                        }
                                    }

                                    log::info!(
                                        "`{}` paraformer asr reconnected successfully",
                                        session.id
                                    );
                                    self.start_pcm_recognition(true).await.map_err(|e| {
                                        anyhow::anyhow!(
                                            "`{}` error starting paraformer asr: {e}",
                                            session.id
                                        )
                                    })?;
                                }
                                start_submit = true;
                            }
                        }
                    } else {
                        log::warn!("`{}` client rx channel closed unexpectedly", session.id);
                        break;
                    }
                }
                SelectResult::AsrResult(Ok(sentence)) => {
                    if let Some(sentence) = sentence {
                        recv_any_asr_result = true;
                        if sentence.sentence_end {
                            log::info!(
                                "`{}` paraformer ASR final result: {:?}",
                                session.id,
                                sentence.text
                            );

                            asr_text = sentence.text;
                            break;
                        } else {
                            log::info!(
                                "`{}` paraformer ASR interim result: {:?}",
                                session.id,
                                sentence.text
                            );
                            if sentence.text.is_empty() {
                                continue;
                            }
                            if asr_text == sentence.text {
                                continue;
                            }
                            asr_text = sentence.text.clone();
                            session.send_asr_result(vec![sentence.text])?;
                        }
                    } else {
                        log::warn!(
                            "`{}` paraformer ASR result stream ended unexpectedly",
                            session.id
                        );
                        break;
                    }
                }
                SelectResult::AsrResult(Err(e)) => {
                    log::warn!(
                        "`{}` paraformer ASR error getting result: {e}, attempting to reconnect...",
                        session.id
                    );
                }
            }
        }

        if start_submit {
            let _ = self.finish_task().await.map_err(|e| {
                log::warn!("`{}` error finishing paraformer asr task: {e}", session.id);
            });

            loop {
                if let Ok(Some(sentence)) = self.next_result().await {
                    if sentence.sentence_end {
                        log::info!(
                            "`{}` paraformer ASR final result after finish: {:?}",
                            session.id,
                            sentence.text
                        );
                        asr_text += &sentence.text;
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        session.send_end_vad()?;
        Ok(asr_text)
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

    pub async fn stream_get_input(
        &mut self,
        session: &mut super::Session,
    ) -> anyhow::Result<String> {
        match self {
            AsrSession::Whisper(asr_session) => asr_session.stream_get_input(session).await,
            AsrSession::Paraformer(asr_session) => asr_session.stream_get_input(session).await,
        }
    }
}
