use bytes::Bytes;

/// return: wav_audio: 16bit,32k,single-channel.
pub async fn gsv(
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
) -> anyhow::Result<Bytes> {
    log::debug!("speaker: {speaker}, text: {text}");
    let client = reqwest::Client::new();
    let res = client
        .post(tts_url)
        .json(&serde_json::json!({"speaker": speaker, "input": text, "sample_rate": sample_rate}))
        // .body(serde_json::json!({"speaker": speaker, "input": text}).to_string())
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::tts::test_gsv --exact --show-output
#[tokio::test]
async fn test_gsv() {
    let tts_url = "http://localhost:8000/v1/audio/speech";
    let speaker = "ad";
    let text = "你好，我是胡桃";
    let wav_audio = gsv(tts_url, speaker, text, Some(16000)).await.unwrap();
    let header = hound::WavReader::new(wav_audio.as_ref()).unwrap();
    let spec = header.spec();
    println!("wav header: {:?}", spec);
    assert_eq!(spec.sample_rate, 16000);
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}

/// return: pcm_chunk: 16bit,32k,single-channel.
pub async fn stream_gsv(
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
) -> anyhow::Result<reqwest::Response> {
    log::debug!("speaker: {speaker}, text: {text}");
    let client = reqwest::Client::new();
    let res = client
        .post(tts_url)
        .json(&serde_json::json!({"speaker": speaker, "input": text, "sample_rate": sample_rate}))
        // .body(serde_json::json!({"speaker": speaker, "input": text}).to_string())
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    Ok(res)
}

/// return: wav_audio: 16bit,48k,single-channel.
pub async fn groq(model: &str, token: &str, voice: &str, text: &str) -> anyhow::Result<Bytes> {
    log::debug!("groq tts. voice: {voice}, text: {text}");
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.groq.com/openai/v1/audio/speech")
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model":model,
            "voice": voice,
            "input": text,
            "response_format": "wav"
        }))
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::tts:test_groq --exact --show-output
#[tokio::test]
async fn test_groq() {
    let token = std::env::var("GROQ_API_KEY").unwrap();
    let speaker = "Aaliyah-PlayAI";
    let text = "你好，我是胡桃";
    let wav_audio = groq("playai-tts", &token, speaker, text).await.unwrap();
    let mut reader = wav_io::reader::Reader::from_vec(wav_audio.to_vec()).unwrap();
    let head = reader.read_header().unwrap();
    println!("wav header: {:?}", head);
    std::fs::write("./resources/test/groq_out.wav", wav_audio).unwrap();
}

#[derive(Debug, serde::Serialize)]
struct FishTTSRequest {
    text: String,
    chunk_length: usize,
    format: String,
    mp3_bitrate: usize,
    reference_id: String,
    normalize: bool,
    latency: String,
}

impl FishTTSRequest {
    fn new(speaker: String, text: String, format: String) -> Self {
        Self {
            text,
            chunk_length: 200,
            format,
            mp3_bitrate: 128,
            reference_id: speaker,
            normalize: true,
            latency: "normal".to_string(),
        }
    }
}

pub async fn fish_tts(token: &str, speaker: &str, text: &str) -> anyhow::Result<Bytes> {
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.fish.audio/v1/tts")
        .header("content-type", "application/msgpack")
        .header("authorization", &format!("Bearer {}", token))
        .body(rmp_serde::to_vec_named(&FishTTSRequest::new(
            speaker.to_string(),
            text.to_string(),
            "wav".to_string(),
        ))?)
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{}, body:{}",
            status,
            body
        ));
    }
    let bytes = res.bytes().await?;
    Ok(bytes)
}

#[tokio::test]
async fn test_fish_tts() {
    let token = std::env::var("FISH_API_KEY").unwrap();
    let speaker = "256e1a3007a74904a91d132d1e9bf0aa";
    let text = "hello fish";

    let r = rmp_serde::to_vec_named(&FishTTSRequest::new(
        speaker.to_string(),
        text.to_string(),
        "wav".to_string(),
    ));
    println!("{:x?}", r);

    let wav_audio = fish_tts(&token, speaker, text).await.unwrap();
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}

/// bailian tts cosyvoice WebSocket implementation
pub mod cosyvoice {
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
                            log::info!("Result generated");
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
}
