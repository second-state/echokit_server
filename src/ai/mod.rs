use reqwest::multipart::Part;

pub mod gemini;
pub mod store;
pub mod tts;

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
            } else {
                texts.push(line.to_string());
            }
        }
        texts
    }
}

/// wav_audio: 16bit,16k,single-channel.
pub async fn asr(
    asr_url: &str,
    api_key: &str,
    model: &str,
    lang: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::new();
    let mut form =
        reqwest::multipart::Form::new().part("file", Part::bytes(wav_audio).file_name("audio.wav"));

    if !lang.is_empty() {
        form = form.text("language", lang.to_string());
    }

    if !model.is_empty() {
        form = form.text("model", model.to_string());
    }

    let builder = client.post(asr_url).multipart(form);

    let res = if !api_key.is_empty() {
        builder
            .bearer_auth(api_key)
            .header(reqwest::header::USER_AGENT, "curl/7.81.0")
    } else {
        builder
    }
    .send()
    .await?;

    let r: serde_json::Value = res.json().await?;
    log::debug!("ASR response: {:#?}", r);

    let asr_result: AsrResult = serde_json::from_value(r)
        .map_err(|e| anyhow::anyhow!("Failed to parse ASR result: {}", e))?;
    Ok(asr_result.parse_text())
}

#[tokio::test]
async fn test_asr() {
    let asr_url = "https://whisper.gaia.domains/v1/audio/transcriptions";
    let lang = "zh";
    let wav_audio = std::fs::read("./resources/test/out.wav").unwrap();
    let text = asr(asr_url, "", "", lang, wav_audio).await.unwrap();
    println!("ASR result: {:?}", text);
}

#[tokio::test]
async fn test_groq_asr() {
    env_logger::init();
    let groq_api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
    let asr_url = "https://api.groq.com/openai/v1/audio/transcriptions";
    let lang = "zh";
    let wav_audio = std::fs::read("./resources/test/out.wav").unwrap();
    let text = asr(asr_url, &groq_api_key, "whisper-large-v3", lang, wav_audio)
        .await
        .unwrap();
    println!("ASR result: {:?}", text);
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StableLlmRequest {
    stream: bool,
    #[serde(rename = "chatId")]
    #[serde(skip_serializing_if = "String::is_empty")]
    chat_id: String,
    messages: Vec<llm::Content>,
    #[serde(skip_serializing_if = "String::is_empty")]
    model: String,
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
    const CHUNK_SIZE: usize = 50;

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

    fn push_str(&mut self, s: &str) -> Option<String> {
        let mut ret = s;

        loop {
            if let Some(i) = ret.find(&['.', '!', '?', ';', '。', '！', '？', '；', '\n']) {
                let (chunk, ret_) = if ret.is_char_boundary(i + 1) {
                    ret.split_at(i + 1)
                } else {
                    ret.split_at(i + 3)
                };

                self.string_buffer.push_str(chunk);
                ret = ret_;
                if self.string_buffer.len() > Self::CHUNK_SIZE || self.string_buffer.ends_with("\n")
                {
                    let mut new_str = ret.to_string();
                    std::mem::swap(&mut new_str, &mut self.string_buffer);
                    return Some(new_str);
                }
            } else {
                self.string_buffer.push_str(ret);
                return None;
            }
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

            let mut chunks = String::new();
            body.split("data: ").for_each(|s| {
                if s.is_empty() || s.starts_with("[DONE]") {
                    return;
                }

                if let Ok(chunk) = serde_json::from_str::<StableStreamChunk>(s.trim()) {
                    if !chunk.choices[0].finish_reason.is_some() {
                        chunks.push_str(&chunk.choices[0].delta.message);
                    }
                }
            });

            if let Some(new_str) = self.push_str(&chunks) {
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
    model: &str,
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
        response_builder = response_builder.bearer_auth(token);
    };

    let response = response_builder
        .header(reqwest::header::USER_AGENT, "curl/7.81.0")
        .json(&StableLlmRequest {
            stream: true,
            chat_id: chat_id.unwrap_or_default(),
            messages,
            model: model.to_string(),
        })
        .send()
        .await?;

    let state = response.status();
    if !state.is_success() {
        let headers = response.headers().clone();
        let body = response.text().await?;
        return Err(anyhow::anyhow!(
            "llm failed, status:{},\nheader:{:?}\n body:{}",
            state,
            headers,
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
            message: "给我介绍一下妲己".to_string(),
        },
    ];

    let token = if let Some(t) = token.as_ref() {
        t.as_str()
    } else {
        ""
    };
    log::info!("token: {:#?}", token);

    let mut resp = llm_stable(
        "https://cloud.fastgpt.cn/api/v1/chat/completions",
        token,
        "",
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
