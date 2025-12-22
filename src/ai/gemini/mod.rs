use futures_util::{SinkExt, StreamExt};
use http::Method;
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use types::ServerContent_;

pub mod types;

pub struct LiveClient {
    ws: WebSocket,
}

impl LiveClient {
    pub async fn connect(api_key: &str) -> anyhow::Result<Self> {
        let uri = format!("wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={api_key}");
        log::info!("Connecting to Gemini Live Client at {}", uri);

        let response = reqwest::Client::default()
            .request(Method::GET, uri)
            .header("x-goog-api-key", api_key)
            .upgrade() // Prepares the WebSocket upgrade.
            .send()
            .await?;

        let ws = response.into_websocket().await?;

        Ok(LiveClient { ws })
    }

    pub async fn setup(&mut self, setup: types::Setup) -> anyhow::Result<()> {
        self.ws
            .send(Message::Text(serde_json::to_string(
                &serde_json::json!({"setup":setup}),
            )?))
            .await?;
        let x = self
            .ws
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to receive setup confirmation"))??;
        if let Message::Text(text) = x {
            println!("Setup confirmation: {}", text);
        }

        Ok(())
    }

    pub async fn send_realtime_input(&mut self, input: types::RealtimeInput) -> anyhow::Result<()> {
        self.ws
            .send(Message::Text(serde_json::to_string(
                &serde_json::json!({"realtime_input": input}),
            )?))
            .await?;
        Ok(())
    }

    pub async fn send_realtime_audio(&mut self, audio: types::RealtimeAudio) -> anyhow::Result<()> {
        self.send_realtime_input(types::RealtimeInput::Audio(audio))
            .await?;
        self.send_realtime_input(types::RealtimeInput::AudioStreamEnd(true))
            .await?;
        Ok(())
    }

    pub async fn receive(&mut self) -> anyhow::Result<types::ServerContent> {
        if let Some(msg) = self.ws.next().await {
            log::trace!("Received message: {:?}", msg);
            let msg = msg?;
            match msg {
                Message::Text(text) => {
                    let server_content: ServerContent_ =
                        serde_json::from_str(&text).map_err(|e| {
                            anyhow::anyhow!("Failed to parse text message: {} {text}", e)
                        })?;
                    log::debug!("Parsed text message: {:?}", server_content);
                    Ok(server_content.server_content)
                }
                Message::Binary(bin) => {
                    let server_content: ServerContent_ =
                        serde_json::from_slice(&bin).map_err(|e| {
                            anyhow::anyhow!("Failed to parse binary message: {} {:?}", e, bin)
                        })?;
                    log::trace!("Parsed binary message: {:?}", server_content);
                    Ok(server_content.server_content)
                }
                Message::Close { code, reason } => Err(anyhow::anyhow!(
                    "WebSocket closed with code: {:?}, reason: {:?}",
                    code,
                    reason
                )),
                _ => Err(anyhow::anyhow!("Unexpected message type: {:?}", msg)),
            }
        } else {
            Err(anyhow::anyhow!("WebSocket stream ended unexpectedly"))
        }
    }
}

#[cfg(test)]
mod test {
    use super::types;
    use super::*;

    // cargo test --package echokit_server --bin echokit_server -- ai::gemini::test::test_live_client --exact --show-output
    #[tokio::test]
    async fn test_live_client() -> anyhow::Result<()> {
        env_logger::init();
        let api_key = std::env::var("GEMINI_API_KEY").unwrap();
        log::info!("api_key={api_key}");
        let mut client = LiveClient::connect(&api_key).await?;
        log::info!("Connected to Gemini Live Client");

        let mut cfg = types::GenerationConfig::default();
        cfg.response_modalities = Some(vec![types::Modality::TEXT]);

        let setup = types::Setup {
            model: "models/gemini-2.0-flash-exp".to_string(),
            generation_config: Some(cfg),
            system_instruction: Some(types::Content {
                parts: vec![types::Parts::Text(
                    "You are a helpful assistant and answer in a friendly tone.".to_string(),
                )],
            }),
            input_audio_transcription: Some(types::AudioTranscriptionConfig {}),
            output_audio_transcription: None,
            realtime_input_config: Some(types::RealtimeInputConfig {
                automatic_activity_detection: Some(types::AutomaticActivityDetectionConfig {
                    disabled: true,
                }),
            }),
        };
        client.setup(setup).await?;
        log::info!("Setup completed");

        // let submit_data = std::fs::read("sample.pcm").unwrap();
        let data = std::fs::read("tmp.wav").unwrap();
        let mut reader = wav_io::reader::Reader::from_vec(data).unwrap();
        let header = reader.read_header().unwrap();
        log::info!("WAV Header: {:?}", header);
        let x = crate::util::get_samples_f32(&mut reader).unwrap();
        let x = wav_io::resample::linear(x, 1, header.sample_rate, 16000);
        let submit_data = crate::util::convert_samples_f32_to_i16_bytes(&x);

        // let input = types::RealtimeInput {
        //     audio: None,
        //     text: Some("你是谁".to_string()),
        // };
        client
            .send_realtime_input(types::RealtimeInput::ActivityStart {})
            .await?;

        let input = types::RealtimeInput::Audio(types::RealtimeAudio {
            data: types::Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        });
        log::info!("Sending realtime input");
        client.send_realtime_input(input).await?;
        log::info!("Sent realtime input");
        // client
        //     .send_realtime_input(types::RealtimeInput::AudioStreamEnd(true))
        //     .await?;
        client
            .send_realtime_input(types::RealtimeInput::ActivityEnd {})
            .await?;

        log::info!("Sent realtime input");
        loop {
            let content = client.receive().await?;
            log::info!("Received content: {:?}", content);
            if let types::ServerContent::TurnComplete(true) = content {
                log::info!("Generation complete");
                break;
            }
        }

        Ok(())
    }

    // cargo test --package echokit_server --bin echokit_server -- ai::gemini::test::test_live_client_audio --exact --show-output
    #[tokio::test]
    async fn test_live_client_audio() -> anyhow::Result<()> {
        env_logger::init();
        let api_key = std::env::var("GEMINI_API_KEY").unwrap();
        log::info!("api_key={api_key}");
        let mut client = LiveClient::connect(&api_key).await?;
        log::info!("Connected to Gemini Live Client");

        let mut cfg = types::GenerationConfig::default();
        cfg.response_modalities = Some(vec![types::Modality::AUDIO]);

        let setup = types::Setup {
            model: "models/gemini-2.0-flash-exp".to_string(),
            generation_config: Some(cfg),
            system_instruction: Some(types::Content {
                parts: vec![types::Parts::Text(
                    "You are a helpful assistant and answer in a friendly tone.".to_string(),
                )],
            }),
            input_audio_transcription: Some(types::AudioTranscriptionConfig {}),
            output_audio_transcription: Some(types::AudioTranscriptionConfig {}),
            realtime_input_config: None,
        };
        client.setup(setup).await?;
        log::info!("Setup completed");

        // let submit_data = std::fs::read("sample.pcm").unwrap();
        let data = std::fs::read("tmp.wav").unwrap();
        let mut reader = wav_io::reader::Reader::from_vec(data).unwrap();
        let header = reader.read_header().unwrap();
        log::info!("WAV Header: {:?}", header);
        let x = crate::util::get_samples_f32(&mut reader).unwrap();
        let x = wav_io::resample::linear(x, 1, header.sample_rate, 16000);
        let submit_data = crate::util::convert_samples_f32_to_i16_bytes(&x);

        // let input = types::RealtimeInput {
        //     audio: None,
        //     text: Some("你是谁".to_string()),
        // };
        let input = types::RealtimeInput::Audio(types::RealtimeAudio {
            data: types::Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        });
        log::info!("Sending realtime input");
        client.send_realtime_input(input).await?;
        log::info!("Sent realtime input");
        client
            .send_realtime_input(types::RealtimeInput::AudioStreamEnd(true))
            .await?;

        log::info!("Sent realtime AudioStreamEnd");
        loop {
            let content = client.receive().await?;
            log::info!("Received content: {:?}", content);
            if let types::ServerContent::TurnComplete(true) = content {
                log::info!("Generation complete");
                break;
            }
        }

        Ok(())
    }
}
