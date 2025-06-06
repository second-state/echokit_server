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
        let r = tokio::time::timeout(std::time::Duration::from_secs(5), self.ws.next()).await;
        if r.is_err() {
            return Ok(types::ServerContent::Timeout);
        }
        if let Some(msg) = r.unwrap() {
            log::debug!("Received message: {:?}", msg);
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
                    log::debug!("Parsed binary message: {:?}", server_content);
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

    // cargo test --package esp_assistant --bin esp_assistant -- ai::gemini::test::test_live_client --exact --show-output
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
            model: "models/gemini-2.0-flash-live-001".to_string(),
            generation_config: Some(cfg),
            system_instruction: Some(types::Content {
                parts: vec![types::Parts::Text(
                    "You are a helpful assistant and answer in a friendly tone.".to_string(),
                )],
            }),
            input_audio_transcription: Some(types::AudioTranscriptionConfig {}),
            proactivity: None,
        };
        client.setup(setup).await?;
        log::info!("Setup completed");

        // let submit_data = std::fs::read("sample.pcm").unwrap();
        let data = std::fs::read("asr.fc012ccfcd71.wav").unwrap();
        let mut reader = wav_io::reader::Reader::from_vec(data).unwrap();
        let header = reader.read_header().unwrap();
        log::info!("WAV Header: {:?}", header);
        let x = reader.get_samples_f32().unwrap();
        let x = wav_io::resample::linear(x, 1, header.sample_rate, 16000);
        let data = wav_io::convert_samples_f32_to_i16(&x);

        let mut submit_data = Vec::with_capacity(data.len() * 2);
        for sample in data {
            submit_data.extend_from_slice(&sample.to_le_bytes());
        }

        // let input = types::RealtimeInput {
        //     audio: None,
        //     text: Some("你是谁".to_string()),
        // };
        let input = types::RealtimeInput::Audio(types::RealtimeAudio {
            data: types::Blob::new(submit_data),
            mime_type: "audio/pcm;rate=16000".to_string(),
        });
        client.send_realtime_input(input).await?;
        client
            .send_realtime_input(types::RealtimeInput::AudioStreamEnd(true))
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
}
