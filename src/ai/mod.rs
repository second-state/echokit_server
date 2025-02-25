use axum::body::Bytes;
use reqwest::multipart::Part;

/// return: wav_audio: 16bit,32k,single-channel.
pub async fn tts(tts_url: &str, speaker: &str, text: &str) -> anyhow::Result<Bytes> {
    let client = reqwest::Client::new();
    let res = client
        .post(tts_url)
        .json(&serde_json::json!({"speaker": speaker, "input": text}))
        // .body(serde_json::json!({"speaker": speaker, "input": text}).to_string())
        .send()
        .await?;
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::test_tts --exact --show-output
#[tokio::test]
async fn test_tts() {
    let tts_url = "http://localhost:3000/tts";
    let speaker = "ht";
    let text = "你好，我是胡桃";
    let wav_audio = tts(tts_url, speaker, text).await.unwrap();
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}

#[derive(Debug, serde::Deserialize)]
struct AsrResult {
    #[serde(default)]
    text: String,
}

impl AsrResult {
    fn parse_text(self) -> Vec<String> {
        let mut texts = vec![];
        for line in self.text.lines() {
            if let Some((_, t)) = line.split_once("] ") {
                texts.push(t.to_string());
            }
        }
        texts
    }
}

/// wav_audio: 16bit,16k,single-channel.
pub async fn asr(asr_url: &str, lang: &str, wav_audio: Vec<u8>) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::new();
    let mut form =
        reqwest::multipart::Form::new().part("file", Part::bytes(wav_audio).file_name("audio.wav"));
    if !lang.is_empty() {
        form = form.text("language", lang.to_string());
    }

    let res = client.post(asr_url).multipart(form).send().await?;
    let asr_result: AsrResult = res.json().await?;
    Ok(asr_result.parse_text())
}

#[tokio::test]
async fn test_asr() {
    let asr_url = "https://whisper.gaia.domains/v1/audio/transcriptions";
    let lang = "zh";
    let wav_audio = std::fs::read("./resources/test/out.wav").unwrap();
    let text = asr(asr_url, lang, wav_audio).await.unwrap();
    println!("ASR result: {:?}", text);
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StableLlmRequest {
    stream: bool,
    #[serde(rename = "chatId")]
    chat_id: String,
    messages: Vec<llm::Content>,
}

pub struct StableLlmResponse {
    stopped: bool,
    response: reqwest::Response,
    string_buffer: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StableStreamChunkChoices {
    delta: llm::Content,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StableStreamChunk {
    choices: Vec<StableStreamChunkChoices>,
}

impl StableLlmResponse {
    const CHUNK_SIZE: usize = 20;

    fn return_string_buffer(&mut self) -> anyhow::Result<Option<String>> {
        self.stopped = true;
        if !self.string_buffer.is_empty() {
            let mut new_str = String::new();
            std::mem::swap(&mut new_str, &mut self.string_buffer);
            return Ok(Some(new_str));
        } else {
            return Ok(None);
        }
    }

    pub async fn next_chunk(&mut self) -> anyhow::Result<Option<String>> {
        loop {
            if self.stopped {
                return Ok(None);
            }

            let body = self.response.chunk().await?;
            if body.is_none() {
                return self.return_string_buffer();
            }
            let body = body.unwrap();
            let body = String::from_utf8_lossy(&body);

            let mut chunks = vec![];
            body.split("data: ").for_each(|s| {
                if s.is_empty() || s.starts_with("[DONE]") {
                    return;
                }

                if let Ok(chunk) = serde_json::from_str::<StableStreamChunk>(s.trim()) {
                    chunks.push(chunk);
                }
            });

            for body in chunks {
                if body.choices[0].finish_reason.is_some() {
                    return self.return_string_buffer();
                }

                self.string_buffer.push_str(&body.choices[0].delta.message);
            }

            if self
                .string_buffer
                .ends_with(&[',', '.', '!', '?', ';', '，', '。', '！', '？', '；', '\n'])
                && self.string_buffer.len() > Self::CHUNK_SIZE
            {
                let mut new_str = String::new();
                std::mem::swap(&mut new_str, &mut self.string_buffer);
                return Ok(Some(new_str));
            }
        }
    }
}

pub mod llm {
    use std::fmt::Display;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub enum Role {
        #[serde(rename = "system")]
        System,
        #[serde(rename = "user")]
        User,
        #[serde(rename = "assistant")]
        Assistant,
    }

    impl Display for Role {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let role = self.as_ref();
            write!(f, "{role}")
        }
    }

    impl AsRef<str> for Role {
        fn as_ref(&self) -> &str {
            match self {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            }
        }
    }

    impl Default for Role {
        fn default() -> Self {
            Self::Assistant
        }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Content {
        #[serde(default)]
        pub role: Role,

        #[serde(rename = "content")]
        #[serde(default)]
        pub message: String,
    }

    impl AsRef<Content> for Content {
        fn as_ref(&self) -> &Content {
            self
        }
    }
}

pub async fn llm_stable<'p, I: IntoIterator<Item = C>, C: AsRef<llm::Content>>(
    llm_url: &str,
    token: &str,
    chat_id: Option<String>,
    prompts: I,
) -> anyhow::Result<StableLlmResponse> {
    let messages = prompts
        .into_iter()
        .map(|c| c.as_ref().clone())
        .collect::<Vec<_>>();

    log::debug!("##### llm_stable prompts:\n {:#?}\n#####", messages);

    let mut response_builder = reqwest::Client::new().post(llm_url);
    if !token.is_empty() {
        response_builder = response_builder.header(reqwest::header::AUTHORIZATION, token);
    };

    let response = response_builder
        .json(&StableLlmRequest {
            stream: true,
            chat_id: chat_id.unwrap_or_default(),
            messages,
        })
        .send()
        .await?;

    let state = response.status();
    if !state.is_success() {
        let body = response.text().await?;
        return Err(anyhow::anyhow!(
            "llm failed, status:{}, body:{}",
            state,
            body
        ));
    }

    Ok(StableLlmResponse {
        stopped: false,
        response,
        string_buffer: String::new(),
    })
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::test_statble_llm --exact --show-output
#[tokio::test]
async fn test_statble_llm() {
    env_logger::init();
    let token = std::env::var("API_KEY").ok().map(|k| format!("Bearer {k}"));

    let prompts = vec![
        llm::Content {
            role: llm::Role::System,
            message: "你是一个聪明的AI助手，你叫做胡桃".to_string(),
        },
        llm::Content {
            role: llm::Role::User,
            message: "给我解释一下鸡兔同笼".to_string(),
        },
        llm::Content {
            role: llm::Role::Assistant,
            message: "".to_string(),
        },
        llm::Content {
            role: llm::Role::User,
            message: "给我解释一下鸡兔同笼吗吗吗".to_string(),
        },
    ];

    let token = if let Some(t) = token.as_ref() {
        t.as_str()
    } else {
        ""
    };

    let mut resp = llm_stable(
        "https://cloud.fastgpt.cn/api/v1/chat/completions",
        token,
        None,
        prompts,
    )
    .await
    .unwrap();

    loop {
        match resp.next_chunk().await {
            Ok(Some(chunk)) => {
                println!("{}", chunk);
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                println!("error: {:#?}", e);
                break;
            }
        }
    }
}
