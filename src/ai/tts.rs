use bytes::Bytes;

/// return: wav_audio: 16bit,32k,single-channel.
pub async fn gsv(
    client: &reqwest::Client,
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
) -> anyhow::Result<Bytes> {
    log::debug!("speaker: {speaker}, text: {text}");
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
    let client = reqwest::Client::new();
    let wav_audio = gsv(&client, tts_url, speaker, text, Some(16000))
        .await
        .unwrap();
    let header = hound::WavReader::new(wav_audio.as_ref()).unwrap();
    let spec = header.spec();
    println!("wav header: {:?}", spec);
    assert_eq!(spec.sample_rate, 16000);
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}

/// return: pcm_chunk: 16bit,32k,single-channel.
pub async fn stream_gsv(
    client: &reqwest::Client,
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
) -> anyhow::Result<reqwest::Response> {
    log::debug!("speaker: {speaker}, text: {text}");
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
pub async fn groq(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    token: &str,
    voice: &str,
    text: &str,
) -> anyhow::Result<Bytes> {
    let url = if url.is_empty() {
        "https://api.groq.com/openai/v1/audio/speech"
    } else {
        url
    };

    log::debug!("groq tts. voice: {voice}, text: {text}");
    let res = client
        .post(url)
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

// cargo test --package echokit_server --bin echokit_server -- ai::tts::test_groq --exact --show-output
#[tokio::test]
async fn test_groq() {
    let token = std::env::var("GROQ_API_KEY").unwrap();
    let speaker = "Aaliyah-PlayAI";
    let text = "你好，我是胡桃";
    let client = reqwest::Client::new();
    let wav_audio = groq(&client, "", "playai-tts", &token, speaker, text)
        .await
        .unwrap();
    let mut reader = wav_io::reader::Reader::from_vec(wav_audio.to_vec()).unwrap();
    let head = reader.read_header().unwrap();
    println!("wav header: {:?}", head);
    std::fs::write("./resources/test/groq_out.wav", wav_audio).unwrap();
    let samples = crate::util::get_samples_f32(&mut reader).unwrap();
    println!("samples len: {}", samples.len());
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
