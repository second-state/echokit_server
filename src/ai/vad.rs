use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use reqwest::multipart::Part;
use reqwest_websocket::{RequestBuilderExt, WebSocket};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SpeechSampleIndex {
    pub start: i64,
    pub end: i64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct VadResponse {
    #[serde(default)]
    pub timestamps: Vec<SpeechSampleIndex>,
    #[serde(default)]
    pub error: Option<String>,
}

pub async fn vad_detect(
    client: &reqwest::Client,
    vad_url: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<VadResponse> {
    let form = reqwest::multipart::Form::new()
        .part("audio", Part::bytes(wav_audio).file_name("audio.wav"));

    let res = client.post(vad_url).multipart(form).send().await?;

    let r: serde_json::Value = res.json().await?;
    log::debug!("VAD response: {:#?}", r);

    let vad_result: VadResponse = serde_json::from_value(r)
        .map_err(|e| anyhow::anyhow!("Failed to parse ASR result: {}", e))?;
    Ok(vad_result)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum VadRealtimeEvent {
    Event { event: String },
    Error { error: String, message: String },
}

pub struct VadRealtimeClient(pub SplitSink<WebSocket, reqwest_websocket::Message>);
pub struct VadRealtimeRx(pub SplitStream<WebSocket>);

pub async fn vad_realtime_client(
    client: &reqwest::Client,
    vad_ws_url: String,
) -> anyhow::Result<(VadRealtimeClient, VadRealtimeRx)> {
    let response = client
        .get(&vad_ws_url)
        .upgrade() // Prepares the WebSocket upgrade.
        .send()
        .await?;

    let ws = response.into_websocket().await?;
    let (tx, rx) = ws.split();

    Ok((VadRealtimeClient(tx), VadRealtimeRx(rx)))
}

impl VadRealtimeClient {
    pub async fn push_audio_16k_chunk(&mut self, audio_16k: bytes::Bytes) -> anyhow::Result<()> {
        self.0
            .send(reqwest_websocket::Message::Binary(audio_16k))
            .await?;
        Ok(())
    }
}

impl VadRealtimeRx {
    pub async fn next_event(&mut self) -> anyhow::Result<VadRealtimeEvent> {
        loop {
            match self.0.next().await {
                Some(Ok(reqwest_websocket::Message::Text(text))) => {
                    let event: VadRealtimeEvent = serde_json::from_str(&text)
                        .map_err(|e| anyhow::anyhow!("Failed to parse VAD event: {}", e))?;
                    return Ok(event);
                }
                Some(Ok(reqwest_websocket::Message::Binary(_))) => {
                    // Ignore binary messages
                    continue;
                }
                Some(Ok(reqwest_websocket::Message::Close { .. })) => {
                    return Err(anyhow::anyhow!("WebSocket connection closed"));
                }
                Some(Err(e)) => {
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
                Some(_) => {
                    continue; // Ignore other message types
                }
                None => {
                    return Err(anyhow::anyhow!("WebSocket stream ended unexpectedly"));
                }
            }
        }
    }
}

pub type VadParams = crate::config::SileroVadconfig;

#[derive(Clone)]
pub struct SileroVADFactory {
    device: burn::backend::ndarray::NdArrayDevice,
    params: VadParams,
}

impl SileroVADFactory {
    pub fn new(params: VadParams) -> anyhow::Result<Self> {
        let device = burn::backend::ndarray::NdArrayDevice::default();

        Ok(SileroVADFactory { device, params })
    }

    pub fn create_session(&self) -> anyhow::Result<VadSession> {
        let vad = Box::new(silero_vad_burn::SileroVAD6Model::new(&self.device)?);
        VadSession::new(&self.params, vad, self.device.clone())
    }
}

pub struct VadSession {
    vad: Box<silero_vad_burn::SileroVAD6Model<burn::backend::NdArray>>,
    state: Option<silero_vad_burn::PredictState<burn::backend::NdArray>>,
    device: burn::backend::ndarray::NdArrayDevice,

    in_speech: bool,

    threshold: f32,
    neg_threshold: f32,

    silence_chunk_count: usize,
    max_silence_chunks: usize,
}

impl VadSession {
    const SAMPLE_RATE: usize = 16000;

    pub fn new(
        params: &VadParams,
        vad: Box<silero_vad_burn::SileroVAD6Model<burn::backend::NdArray>>,
        device: burn::backend::ndarray::NdArrayDevice,
    ) -> anyhow::Result<Self> {
        let state = Some(silero_vad_burn::PredictState::default(&device));

        let neg_threshold = params
            .neg_threshold
            .unwrap_or_else(|| params.threshold - 0.15)
            .max(0.05);

        let threshold = params.threshold.min(0.95);
        let max_silence_chunks = params.max_silence_duration_ms * (Self::SAMPLE_RATE / 1000)
            / silero_vad_burn::CHUNK_SIZE;

        Ok(VadSession {
            vad,
            state,
            device,

            in_speech: false,
            threshold,
            neg_threshold,

            silence_chunk_count: 0,
            max_silence_chunks,
        })
    }

    pub fn reset_state(&mut self) {
        self.state = Some(silero_vad_burn::PredictState::default(&self.device));
        self.in_speech = false;
        self.silence_chunk_count = 0;
    }

    pub fn detect(&mut self, audio16k_chunk_512: &[f32]) -> anyhow::Result<bool> {
        debug_assert!(
            audio16k_chunk_512.len() <= 512,
            "audio16k_chunk_512 length must be less than 512",
        );

        let audio_tensor =
            burn::Tensor::<_, 1>::from_floats(audio16k_chunk_512, &self.device).unsqueeze();
        let (state, prob) = self.vad.predict(self.state.take().unwrap(), audio_tensor)?;
        self.state = Some(state);

        let prob: Vec<f32> = prob.to_data().to_vec()?;

        if prob[0] > self.threshold {
            self.in_speech = true;
            self.silence_chunk_count = 0;
        } else if prob[0] < self.neg_threshold {
            self.silence_chunk_count += 1;
            if self.silence_chunk_count >= self.max_silence_chunks {
                self.in_speech = false;
            }
        } else {
        }

        Ok(self.in_speech)
    }

    pub const fn vad_chunk_size() -> usize {
        silero_vad_burn::CHUNK_SIZE
    }
}
