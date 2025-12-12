use std::fmt::Display;

use base64::prelude::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{RequestBuilderExt, WebSocket};

#[derive(Debug, Default, serde::Deserialize)]
pub struct Alignment {
    pub chars: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Response {
    #[serde(default)]
    pub alignment: Option<Alignment>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default, rename = "isFinal")]
    pub is_final: Option<bool>,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub message: String,
}

impl Response {
    pub fn is_error(&self) -> bool {
        !self.error.is_empty()
    }

    pub fn get_audio_bytes(&self) -> Option<Vec<u8>> {
        self.audio
            .as_ref()
            .and_then(|audio_base64| BASE64_STANDARD.decode(audio_base64).ok())
    }

    pub fn is_final(&self) -> bool {
        self.is_final.unwrap_or(false)
    }
}

#[test]
fn test_response_deserialize() {
    let json_data = r#"
    {
        "alignment": null,
        "audio": "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YRAAAAAA",
        "isFinal": null
    }
    "#;

    let response: Response = serde_json::from_str(json_data).unwrap();
    println!("{:?}", response);
    assert!(!response.is_error());
    assert!(!response.is_final());
    assert!(response.get_audio_bytes().is_none());

    let json_data_with_audio = r#"
    {
        "alignment": {},
        "audio": "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YRAAAAAA",
        "isFinal": true
    }
    "#;

    let response_with_audio: Response = serde_json::from_str(json_data_with_audio).unwrap();
    println!("{:?}", response_with_audio);
    assert!(!response_with_audio.is_error());
    assert!(response_with_audio.is_final());
    assert!(response_with_audio.get_audio_bytes().is_some());
}

pub struct ElevenlabsTTS {
    pub token: String,
    pub voice: String,
    websocket: WebSocket,
}

const DEFAULT_MODEL_ID: &str = "eleven_flash_v2_5";

pub enum OutputFormat {
    Pcm16000,
    Pcm24000,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Pcm16000 => write!(f, "pcm_16000"),
            OutputFormat::Pcm24000 => write!(f, "pcm_24000"),
        }
    }
}

impl ElevenlabsTTS {
    pub async fn new(
        url: &str,
        token: String,
        voice: String,
        output_format: OutputFormat,
        model_id: &str,
        language_code: &str,
    ) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();
        Self::new_with_client(
            url,
            &client,
            token,
            voice,
            output_format,
            model_id,
            language_code,
        )
        .await
    }

    pub async fn new_with_client(
        url: &str,
        client: &reqwest::Client,
        token: String,
        voice: String,
        output_format: OutputFormat,
        model_id: &str,
        language_code: &str,
    ) -> anyhow::Result<Self> {
        let model_id = if model_id.is_empty() {
            DEFAULT_MODEL_ID
        } else {
            model_id
        };

        let mut url = if url.is_empty() {
            "wss://api.elevenlabs.io/v1/text-to-speech".to_string()
        } else {
            url.to_string()
        };

        if !url.ends_with('/') {
            url.push('/');
        }

        let language_code = language_code.to_ascii_lowercase();

        let mut url =
            format!("{url}{voice}/stream-input?model_id={model_id}&output_format={output_format}");

        if !language_code.is_empty() {
            url.push_str(&format!("&language_code={}", language_code));
        }

        log::debug!("Connect Elevenlabs TTS WebSocket URL: {}", url);

        let response = client
            .get(url)
            .header("xi-api-key", &token)
            .upgrade()
            .send()
            .await?;

        let websocket = response.into_websocket().await?;

        Ok(Self {
            token,
            voice,
            websocket,
        })
    }

    pub async fn initialize_connection(&mut self) -> anyhow::Result<()> {
        let init_message = serde_json::json!({
            "text": " ",
        });

        let message_json = serde_json::to_string(&init_message)?;
        self.websocket
            .send(reqwest_websocket::Message::Text(message_json))
            .await?;

        Ok(())
    }

    pub async fn send_text(&mut self, text: &str, flush: bool) -> anyhow::Result<()> {
        let text_message = serde_json::json!({
            "text": text,
            "flush": flush,
        });

        let message_json = serde_json::to_string(&text_message)?;
        self.websocket
            .send(reqwest_websocket::Message::Text(message_json))
            .await?;

        Ok(())
    }

    pub async fn close_connection(&mut self) -> anyhow::Result<()> {
        let close_message = serde_json::json!({
            "text": "",
        });
        self.websocket
            .send(reqwest_websocket::Message::Text(close_message.to_string()))
            .await?;
        Ok(())
    }

    pub async fn next_audio_response(&mut self) -> anyhow::Result<Option<Response>> {
        while let Some(message) = self.websocket.next().await {
            match message.map_err(|e| anyhow::anyhow!("Elevenlabs TTS WebSocket error: {}", e))? {
                reqwest_websocket::Message::Text(text) => {
                    let response: Response = serde_json::from_str(&text).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to parse Elevenlabs TTS response: {}, error: {}",
                            text,
                            e
                        )
                    })?;

                    if response.is_error() {
                        log::error!("Elevenlabs TTS error response: {:?}", response);
                        return Err(anyhow::anyhow!(
                            "Elevenlabs TTS error: {}",
                            response.message
                        ));
                    }

                    if response.audio.is_some() {
                        log::trace!(
                            "Elevenlabs TTS audio chunk received, size: {}",
                            response.audio.as_ref().unwrap().len()
                        );
                        return Ok(Some(response));
                    }

                    if response.is_final() {
                        log::trace!("TTS stream ended");
                        return Ok(None);
                    }
                }
                reqwest_websocket::Message::Binary(_) => {}
                msg => {
                    if cfg!(debug_assertions) {
                        log::debug!("Received non-text message: {:?}", msg);
                    }
                }
            }
        }
        Ok(None)
    }
}

// cargo test --package echokit_server --bin echokit_server -- ai::elevenlabs::tts::test_elevenlabs_tts --exact --show-output
#[tokio::test]
async fn test_elevenlabs_tts() {
    env_logger::init();
    let token = std::env::var("ELEVENLABS_API_KEY").unwrap();
    let voice = std::env::var("ELEVENLABS_VOICE_ID").unwrap();

    let mut tts = ElevenlabsTTS::new("", token, voice, OutputFormat::Pcm16000, "", "")
        .await
        .expect("Failed to create ElevenlabsTTS");

    tts.send_text("Hello, this is a test of Elevenlabs TTS.", true)
        .await
        .expect("Failed to send text");

    tts.close_connection()
        .await
        .expect("Failed to close connection");

    let mut samples = Vec::new();

    while let Ok(Some(resp)) = tts.next_audio_response().await {
        if let Some(audio) = resp.get_audio_bytes() {
            println!("Received audio chunk of size: {}", audio.len());
            samples.extend_from_slice(&audio);
        }
    }

    let wav = crate::util::pcm_to_wav(
        &samples,
        crate::util::WavConfig {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
        },
    );
    std::fs::write("./resources/test/elevenlabs_out.wav", wav).unwrap();
}

// cargo test --package echokit_server --bin echokit_server -- ai::elevenlabs::tts::test_elevenlabs_tts_with_language_code --exact --show-output
#[tokio::test]
async fn test_elevenlabs_tts_with_language_code() {
    env_logger::init();
    let token = std::env::var("ELEVENLABS_API_KEY").unwrap();
    let voice = std::env::var("ELEVENLABS_VOICE_ID").unwrap();

    let mut tts = ElevenlabsTTS::new(
        "",
        token,
        voice,
        OutputFormat::Pcm16000,
        "eleven_multilingual_v2",
        "ZH",
    )
    .await
    .expect("Failed to create ElevenlabsTTS");

    tts.send_text("你好，这里是 elevenlabs TTS 的测试。", true)
        .await
        .expect("Failed to send text");

    tts.close_connection()
        .await
        .expect("Failed to close connection");

    let mut samples = Vec::new();

    while let Ok(Some(resp)) = tts.next_audio_response().await {
        if let Some(audio) = resp.get_audio_bytes() {
            println!("Received audio chunk of size: {}", audio.len());
            samples.extend_from_slice(&audio);
        }
    }

    let wav = crate::util::pcm_to_wav(
        &samples,
        crate::util::WavConfig {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
        },
    );
    std::fs::write("./resources/test/elevenlabs_out.zh.wav", wav).unwrap();
}
