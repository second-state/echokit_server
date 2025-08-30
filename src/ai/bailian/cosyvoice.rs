use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{RequestBuilderExt, WebSocket};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ResponseHeader {
    event: String,
    #[allow(dead_code)]
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    header: ResponseHeader,
}

impl ResponseMessage {
    fn is_task_started(&self) -> bool {
        self.header.event == "task-started"
    }

    fn is_result_generated(&self) -> bool {
        self.header.event == "result-generated"
    }

    fn is_task_finished(&self) -> bool {
        self.header.event == "task-finished"
    }
}

pub struct CosyVoiceTTS {
    #[allow(unused)]
    token: String,
    websocket: WebSocket,
    synthesis_started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CosyVoiceVersion {
    V1,
    V2,
}

impl CosyVoiceVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            CosyVoiceVersion::V1 => "cosyvoice-v1",
            CosyVoiceVersion::V2 => "cosyvoice-v2",
        }
    }
}

impl Default for CosyVoiceVersion {
    fn default() -> Self {
        CosyVoiceVersion::V2
    }
}

impl CosyVoiceTTS {
    pub async fn connect(token: String) -> anyhow::Result<Self> {
        let url = format!("wss://dashscope.aliyuncs.com/api-ws/v1/inference");

        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .bearer_auth(&token)
            .header("X-DashScope-DataInspection", "enable")
            .upgrade()
            .send()
            .await?;
        let websocket = response.into_websocket().await?;

        Ok(Self {
            token,
            websocket,
            synthesis_started: false,
        })
    }

    pub async fn start_synthesis(
        &mut self,
        model: CosyVoiceVersion,
        voice: Option<&str>,
        sample_rate: Option<u32>,
        text: &str,
    ) -> anyhow::Result<()> {
        let voice = if let Some(v) = voice {
            v
        } else {
            match model {
                CosyVoiceVersion::V1 => "longwan",
                CosyVoiceVersion::V2 => "longwan_v2",
            }
        };

        let task_id = Uuid::new_v4().to_string();
        log::info!("Starting synthesis task with ID: {}", task_id);

        let start_message = serde_json::json!({
             "header": {
                "action": "run-task",
                "task_id": &task_id,
                "streaming": "duplex"
            },
            "payload": {
                "task_group": "audio",
                "task": "tts",
                "function": "SpeechSynthesizer",
                "model": model.as_str(),
                "parameters": {
                    "text_type": "PlainText",
                    "voice": voice,
                    "format": "pcm",
                    "sample_rate": sample_rate.unwrap_or(24000),
                },
                "input": {
                    "text": text
                }
            },
        });

        let message_json = serde_json::to_string(&start_message)?;
        self.websocket
            .send(reqwest_websocket::Message::Text(message_json))
            .await?;

        while let Some(message) = self.websocket.next().await {
            match message? {
                reqwest_websocket::Message::Text(text) => {
                    log::debug!("Received message: {:?}", text);

                    let response: ResponseMessage = serde_json::from_str(&text)?;

                    if response.is_task_started() {
                        log::info!("Synthesis task started");
                        self.synthesis_started = true;
                        break;
                    } else {
                        return Err(anyhow::anyhow!("Synthesis error: {:?}", text));
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

        let finish_task = serde_json::json!({
            "header": {
                "action": "finish-task",
                "task_id": &task_id,
                "streaming": "duplex"
            },
            "payload": {
                "input": {}
            }
        });
        let finish_message_json = serde_json::to_string(&finish_task)?;
        self.websocket
            .send(reqwest_websocket::Message::Text(finish_message_json))
            .await?;

        Ok(())
    }

    pub async fn next_audio_chunk(&mut self) -> anyhow::Result<Option<Bytes>> {
        while let Some(message) = self.websocket.next().await {
            match message? {
                reqwest_websocket::Message::Binary(data) => {
                    return Ok(Some(data));
                }
                reqwest_websocket::Message::Text(text) => {
                    let response: ResponseMessage = serde_json::from_str(&text)?;

                    if response.is_task_finished() {
                        log::debug!("Synthesis task finished");
                        return Ok(None);
                    } else if response.is_result_generated() {
                        log::info!("Result generated:{text}");
                    } else {
                        return Err(anyhow::anyhow!("Synthesis error: {:?}", response));
                    }
                }
                msg => {
                    if cfg!(debug_assertions) {
                        log::debug!("Received non-binary/text message: {:?}", msg);
                    }
                }
            }
        }

        Ok(None)
    }
}

#[tokio::test]
async fn test_cosyvoice_tts() {
    env_logger::init();
    let token = std::env::var("COSYVOICE_TOKEN").unwrap();
    let text = "你好,我是CosyVoice V2";

    let mut tts = CosyVoiceTTS::connect(token).await.unwrap();
    tts.start_synthesis(CosyVoiceVersion::V2, None, Some(24000), text)
        .await
        .unwrap();

    let mut audio_data = bytes::BytesMut::new();
    while let Ok(Some(chunk)) = tts.next_audio_chunk().await {
        audio_data.extend_from_slice(&chunk);
    }

    println!("Audio data size: {} bytes", audio_data.len());
    let config = crate::util::WavConfig {
        channels: 1,
        sample_rate: 24000,
        bits_per_sample: 16,
    };
    let wav = crate::util::pcm_to_wav(&audio_data, config);
    std::fs::write("./resources/test/cosyvoice_out.wav", wav).unwrap();
}
