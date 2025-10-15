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
    payload: ResponsePayload,
}

impl ResponseMessage {
    fn is_task_started(&self) -> bool {
        self.header.event == "task-started"
    }

    fn is_task_finished(&self) -> bool {
        self.header.event == "task-finished"
    }
}

#[derive(Debug, Deserialize)]
struct ResponsePayload {
    output: Option<ResponsePayloadOutput>,
}

#[derive(Debug, Deserialize)]
struct ResponsePayloadOutput {
    #[serde(default)]
    sentence: ResponsePayloadOutputSentence,
}

#[derive(Default, Debug, Deserialize)]
pub struct ResponsePayloadOutputSentence {
    pub text: String,
    pub sentence_end: bool,
}

pub struct ParaformerRealtimeV2Asr {
    #[allow(unused)]
    token: String,
    task_id: String,
    sample_rate: u32,
    websocket: WebSocket,
}

impl ParaformerRealtimeV2Asr {
    pub async fn connect(token: String, sample_rate: u32) -> anyhow::Result<Self> {
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
        let task_id = String::new();

        Ok(Self {
            token,
            task_id,
            sample_rate,
            websocket,
        })
    }

    pub async fn start_pcm_recognition(&mut self) -> anyhow::Result<()> {
        let task_id = Uuid::new_v4().to_string();
        log::info!("Starting asr task with ID: {}", task_id);
        self.task_id = task_id;

        let start_message = serde_json::json!({
             "header": {
                "action": "run-task",
                "task_id": &self.task_id,
                "streaming": "duplex"
            },
            "payload": {
                "task_group": "audio",
                "task": "asr",
                "function": "recognition",
                "model": "paraformer-realtime-v2",
                "parameters": {
                    "format": "pcm",
                    "sample_rate": self.sample_rate,
                },
                "input": {}
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
                        log::info!("Recognition task started");
                        break;
                    } else {
                        return Err(anyhow::anyhow!("Recognition error: {:?}", text));
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

        Ok(())
    }

    pub async fn finish_task(&mut self) -> anyhow::Result<()> {
        let finish_task = serde_json::json!({
            "header": {
                "action": "finish-task",
                "task_id": &self.task_id,
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

    pub async fn send_audio(&mut self, audio_pcm_data: Bytes) -> anyhow::Result<()> {
        self.websocket
            .send(reqwest_websocket::Message::Binary(audio_pcm_data))
            .await?;
        Ok(())
    }

    pub async fn next_result(&mut self) -> anyhow::Result<Option<ResponsePayloadOutputSentence>> {
        while let Some(message) = self.websocket.next().await {
            match message? {
                reqwest_websocket::Message::Binary(_) => {
                    log::debug!("Received unexpected binary message");
                }
                reqwest_websocket::Message::Text(text) => {
                    let response: ResponseMessage = serde_json::from_str(&text).map_err(|e| {
                        anyhow::anyhow!("Failed to parse response message: {}, error: {}", text, e)
                    })?;

                    if response.is_task_finished() {
                        log::debug!("ASR task finished");
                        return Ok(None);
                    } else if let Some(output) = response.payload.output {
                        return Ok(Some(output.sentence));
                    } else {
                        return Err(anyhow::anyhow!("ASR error: {:?}", text));
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
async fn test_paraformer_asr() {
    env_logger::init();
    let token = std::env::var("COSYVOICE_TOKEN").unwrap();
    let (head, samples) =
        wav_io::read_from_file(std::fs::File::open("./resources/test/out.wav").unwrap()).unwrap();

    let samples = crate::util::convert_samples_f32_to_i16_bytes(&samples);
    let audio_data = bytes::Bytes::from(samples);

    let mut asr = ParaformerRealtimeV2Asr::connect(token, head.sample_rate)
        .await
        .unwrap();
    asr.start_pcm_recognition().await.unwrap();

    asr.send_audio(audio_data).await.unwrap();
    asr.finish_task().await.unwrap();

    loop {
        if let Ok(Some(sentence)) = asr.next_result().await {
            println!("{:?}", sentence);
            if sentence.sentence_end {
                println!();
            }
        } else {
            break;
        }
    }
}
