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

/// aliyun tts cosyvoice WebSocket implementation
pub mod cosyvoice {
    use bytes::Bytes;
    use futures_util::{SinkExt, StreamExt};
    use reqwest_websocket::{RequestBuilderExt, WebSocket};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Serialize)]
    struct Header {
        message_id: String,
        task_id: String,
        namespace: String,
        name: String,
        appkey: String,
    }

    #[derive(Debug, Serialize)]
    struct StartSynthesisPayload {
        voice: String,
        format: String,
        sample_rate: u32,
        volume: u32,
        speech_rate: i32,
        pitch_rate: i32,
        enable_subtitle: bool,
    }

    #[derive(Debug, Serialize)]
    struct StartSynthesisMessage {
        header: Header,
        payload: StartSynthesisPayload,
    }

    #[derive(Debug, Serialize)]
    struct RunSynthesisPayload {
        text: String,
    }

    #[derive(Debug, Serialize)]
    struct RunSynthesisMessage {
        header: Header,
        payload: RunSynthesisPayload,
    }

    #[derive(Debug, Serialize)]
    struct StopSynthesisMessage {
        header: Header,
    }

    #[derive(Debug, Deserialize)]
    struct ResponseHeader {
        name: String,
        status: u64,
        #[allow(dead_code)]
        message_id: String,
        #[allow(dead_code)]
        task_id: String,
    }

    #[derive(Debug, Deserialize)]
    struct ResponseMessage {
        header: ResponseHeader,
    }

    pub struct CosyVoiceTTS {
        appkey: String,
        #[allow(unused)]
        token: String,
        task_id: String,
        websocket: WebSocket,
        synthesis_started: bool,
    }

    impl CosyVoiceTTS {
        pub async fn connect(appkey: String, token: String) -> anyhow::Result<Self> {
            let url = format!(
                "wss://nls-gateway-cn-beijing.aliyuncs.com/ws/v1?token={}",
                token
            );

            let client = reqwest::Client::new();
            let response = client.get(&url).upgrade().send().await?;
            let websocket = response.into_websocket().await?;

            Ok(Self {
                appkey,
                token,
                task_id: Uuid::new_v4().to_string().replace('-', ""),
                websocket,
                synthesis_started: false,
            })
        }

        fn create_header(&self, name: &str) -> Header {
            Header {
                message_id: Uuid::new_v4().to_string().replace('-', ""),
                task_id: self.task_id.clone(),
                namespace: "FlowingSpeechSynthesizer".to_string(),
                name: name.to_string(),
                appkey: self.appkey.clone(),
            }
        }

        pub async fn start_synthesis(
            &mut self,
            voice: Option<&str>,
            sample_rate: Option<u32>,
        ) -> anyhow::Result<()> {
            let header = self.create_header("StartSynthesis");

            let start_message = StartSynthesisMessage {
                header,
                payload: StartSynthesisPayload {
                    voice: voice.unwrap_or("zhixiaoxia").to_string(),
                    format: "PCM".to_string(),
                    sample_rate: sample_rate.unwrap_or(24000),
                    volume: 100,
                    speech_rate: 0,
                    pitch_rate: 0,
                    enable_subtitle: true,
                },
            };

            let message_json = serde_json::to_string(&start_message)?;
            self.websocket
                .send(reqwest_websocket::Message::Text(message_json))
                .await?;

            while let Some(message) = self.websocket.next().await {
                match message? {
                    reqwest_websocket::Message::Text(text) => {
                        let response: ResponseMessage = serde_json::from_str(&text)?;
                        log::debug!("Received message: {:?}", response);

                        if response.header.name == "SynthesisStarted"
                            && response.header.status == 20000000
                        {
                            self.synthesis_started = true;
                            log::info!("Synthesis started successfully");
                            break;
                        } else if response.header.status != 20000000 {
                            return Err(anyhow::anyhow!(
                                "Failed to start synthesis, status: {}",
                                response.header.status
                            ));
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

        pub async fn synthesize_text(&mut self, text: &str) -> anyhow::Result<()> {
            if !self.synthesis_started {
                return Err(anyhow::anyhow!("Synthesis not started"));
            }

            let run_message = RunSynthesisMessage {
                header: self.create_header("RunSynthesis"),
                payload: RunSynthesisPayload {
                    text: text.to_string(),
                },
            };

            let message_json = serde_json::to_string(&run_message)?;
            self.websocket
                .send(reqwest_websocket::Message::Text(message_json))
                .await?;

            Ok(())
        }

        pub async fn stop_synthesis(&mut self) -> anyhow::Result<()> {
            let stop_message = StopSynthesisMessage {
                header: self.create_header("StopSynthesis"),
            };

            let message_json = serde_json::to_string(&stop_message)?;
            self.websocket
                .send(reqwest_websocket::Message::Text(message_json))
                .await?;

            self.synthesis_started = false;
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
                        log::debug!("Received message: {:?}", response);

                        if response.header.name == "SynthesisCompleted"
                            && response.header.status == 20000000
                        {
                            log::info!("Synthesis completed");
                            return Ok(None);
                        } else if response.header.status != 20000000 {
                            return Err(anyhow::anyhow!(
                                "Synthesis error, status: {}",
                                response.header.status
                            ));
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

    pub async fn synthesize(
        appkey: &str,
        token: &str,
        text: &str,
        voice: Option<&str>,
        sample_rate: Option<u32>,
    ) -> anyhow::Result<CosyVoiceTTS> {
        let mut tts = CosyVoiceTTS::connect(appkey.to_string(), token.to_string()).await?;

        tts.start_synthesis(voice, sample_rate).await?;
        tts.synthesize_text(text).await?;
        tts.stop_synthesis().await?;

        Ok(tts)
    }

    #[tokio::test]
    async fn test_cosyvoice_tts() {
        let appkey = std::env::var("COSYVOICE_APPKEY").unwrap();
        let token = std::env::var("COSYVOICE_TOKEN").unwrap();
        let text = "你好,我是CosyVoice";

        let mut tts = synthesize(&appkey, &token, text, Some("zhixiaoxia"), Some(24000))
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
