use crate::{
    config::WhisperASRConfig,
    services::ws::{ClientMsg, ClientRx},
};

pub type ParaformerASRSession = crate::ai::bailian::realtime_asr::ParaformerRealtimeV2Asr;

pub enum AsrSession {
    Whisper(WhisperASRSession),
    Paraformer(ParaformerASRSession),
}

pub struct WhisperASRSession {
    pub config: WhisperASRConfig,
    pub client: reqwest::Client,
}

impl WhisperASRSession {
    pub fn new(config: WhisperASRConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }

    pub async fn get_input(&self, id: &str, rx: &mut ClientRx) -> anyhow::Result<String> {
        crate::services::ws::get_whisper_asr_text(&self.client, id, &self.config, rx).await
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
                WhisperASRSession::new(whisper_config.clone()),
            )),
            crate::config::ASRConfig::ParaformerV2(paraformer_config) => {
                let session = ParaformerASRSession::connect(
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
}
