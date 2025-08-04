use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
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
