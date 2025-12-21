use std::collections::LinkedList;

use openai::tool::{McpToolAdapter, ToolSet};
use reqwest::multipart::Part;
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation},
    transport::{SseClientTransport, StreamableHttpClientTransport},
    ServiceExt,
};

/// 阿里百炼
pub mod bailian;
pub mod elevenlabs;
pub mod gemini;
pub mod openai;
pub mod store;
pub mod tts;
pub mod vad;

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
    client: &reqwest::Client,
    asr_url: &str,
    api_key: &str,
    model: &str,
    lang: &str,
    prompt: &str,
    wav_audio: Vec<u8>,
) -> anyhow::Result<Vec<String>> {
    let mut form =
        reqwest::multipart::Form::new().part("file", Part::bytes(wav_audio).file_name("audio.wav"));

    if !lang.is_empty() {
        form = form.text("language", lang.to_string());
    }

    if !model.is_empty() {
        form = form.text("model", model.to_string());
    }

    if !prompt.is_empty() {
        form = form.text("prompt", prompt.to_string());
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
    let asr_url = "http://34.44.85.57:9092/v1/audio/transcriptions";
    let lang = "zh";
    let wav_audio = std::fs::read("./resources/test/out.wav").unwrap();
    let client = reqwest::Client::new();
    let text = asr(
        &client,
        asr_url,
        "",
        "",
        lang,
        "你好\n(click)\n(Music)\n(bgm)",
        wav_audio,
    )
    .await
    .unwrap();
    println!("ASR result: {:?}", text);
}

#[tokio::test]
async fn test_groq_asr() {
    env_logger::init();
    let groq_api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
    let asr_url = "https://api.groq.com/openai/v1/audio/transcriptions";
    let lang = "zh";
    let wav_audio = std::fs::read("./resources/test/out.wav").unwrap();
    let client = reqwest::Client::new();

    let text = asr(
        &client,
        asr_url,
        &groq_api_key,
        "whisper-large-v3",
        lang,
        "",
        wav_audio,
    )
    .await
    .unwrap();
    println!("ASR result: {:?}", text);
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StableLlmRequest {
    stream: bool,
    #[serde(flatten)]
    extra: serde_json::Value,
    messages: Vec<llm::Content>,
    #[serde(skip_serializing_if = "String::is_empty")]
    model: String,
}

#[test]
fn test_stable_llm_request_json() {
    let request = StableLlmRequest {
        stream: true,
        extra: serde_json::json!({
            "chat_id": "test-chat-id",
        }),
        messages: vec![],
        model: "test-model".to_string(),
    };

    let json_str = serde_json::to_string_pretty(&request).unwrap();
    println!("StableLlmRequest json: {}", json_str);
}

pub enum StableLLMResponseChunk {
    Functions(Vec<llm::ToolCall>),
    Text(String),
    Stop,
}

pub struct StableLlmResponse {
    stopped: bool,
    response: reqwest::Response,
    string_buffer: String,
}

impl StableLlmResponse {
    const CHUNK_SIZE: usize = 50;

    fn return_string_buffer(&mut self) -> anyhow::Result<StableLLMResponseChunk> {
        self.stopped = true;
        if !self.string_buffer.is_empty() {
            let mut new_str = String::new();
            std::mem::swap(&mut new_str, &mut self.string_buffer);
            return Ok(StableLLMResponseChunk::Text(new_str));
        } else {
            return Ok(StableLLMResponseChunk::Stop);
        }
    }

    fn push_str(string_buffer: &mut String, s: &str) -> Option<String> {
        let mut ret = s;

        loop {
            if let Some(i) = ret.find(&['.', '!', '?', ';', '。', '！', '？', '；', '\n']) {
                let ((chunk, ret_), char_len) = if ret.is_char_boundary(i + 1) {
                    (ret.split_at(i + 1), 1)
                } else {
                    (ret.split_at(i + 3), 3)
                };

                string_buffer.push_str(chunk);
                ret = ret_;
                if ret.chars().next().is_some_and(|c| c.is_numeric()) {
                    continue;
                }
                if char_len == 1 && ret.len() > 0 && !ret.starts_with(&[' ', '\n']) {
                    continue;
                }

                if string_buffer.len() > Self::CHUNK_SIZE || string_buffer.ends_with("\n") {
                    let mut new_str = ret.to_string();
                    std::mem::swap(&mut new_str, string_buffer);
                    return Some(new_str);
                }
            } else {
                string_buffer.push_str(ret);
                return None;
            }
        }
    }

    pub async fn next_chunk(&mut self) -> anyhow::Result<StableLLMResponseChunk> {
        let mut chunk_ret = String::new();
        loop {
            if self.stopped {
                return Ok(StableLLMResponseChunk::Stop);
            }

            let body = self.response.chunk().await?;
            if body.is_none() {
                return self.return_string_buffer();
            }
            let body = body.unwrap();
            let body = if chunk_ret.is_empty() {
                String::from_utf8_lossy(&body).to_string()
            } else {
                chunk_ret.push_str(&String::from_utf8_lossy(&body));
                let new_body = chunk_ret;
                chunk_ret = String::new();
                new_body
            };

            log::trace!("llm response chunk body: {body}");

            let mut chunks = String::new();
            let mut tools = Vec::new();
            body.split("data: ").for_each(|s| {
                if s.is_empty() || s.starts_with("[DONE]") {
                    return;
                }
                log::trace!("llm response body.split: {s}");

                if let Ok(mut chunk) = serde_json::from_str::<llm::StableStreamChunk>(s.trim()) {
                    log::trace!("llm response chunk: {:#?}", chunk);
                    if chunk.choices.is_empty() {
                        return;
                    }
                    if let Some(content) = &chunk.choices[0].delta.content {
                        log::trace!("llm response content: {content}");
                        chunks.push_str(&content);
                    }
                    if !chunk.choices[0].delta.tool_calls.is_empty() {
                        std::mem::swap(&mut chunk.choices[0].delta.tool_calls, &mut tools);
                    }
                } else {
                    chunk_ret.push_str(s);
                }
            });

            log::trace!("llm response chunks: {chunks}");
            log::trace!("llm response tools: {:#?}", tools);

            if tools.is_empty() {
                if let Some(new_str) = Self::push_str(&mut self.string_buffer, &chunks) {
                    log::trace!("llm response text: {new_str}");
                    return Ok(StableLLMResponseChunk::Text(new_str));
                }
            } else {
                log::trace!("llm response tools: {:#?}", tools);
                return Ok(StableLLMResponseChunk::Functions(tools));
            }
        }
    }
}

#[test]
fn test_push_str() {
    let mut string_buffer = String::new();
    let s = StableLlmResponse::push_str(&mut string_buffer, "Hello world!");
    println!("string_buffer: {string_buffer}");
    println!("s: {s:?}");
    let s = StableLlmResponse::push_str(&mut string_buffer, " This is a test.");
    println!("string_buffer: {string_buffer}");
    println!("s: {s:?}");
    let s = StableLlmResponse::push_str(
        &mut string_buffer,
        " This is a long test string that my email is example@gmail.com .",
    );
    println!("string_buffer: {string_buffer}");
    println!("s: {s:?}");
    let s = StableLlmResponse::push_str(&mut string_buffer, "This is a long test string that should be split into multiple chunks. It contains several sentences, and it should be able to handle punctuation marks like periods, exclamation points, and question marks. Let's see how it works with different types of sentences!");
    println!("string_buffer: {string_buffer}");
    println!("s: {s:?}");
    let s = StableLlmResponse::push_str(
        &mut string_buffer,
        "One thousand is 1,000, and two thousand is 2,000. This should be handled correctly.",
    );
    println!("string_buffer: {string_buffer}");
    println!("s: {s:?}");
}

pub mod llm {
    use std::fmt::Display;

    #[derive(Debug, Clone, serde::Serialize)]
    pub struct Function {
        pub name: String,
        pub description: String,
        pub parameters: serde_json::Value,
    }

    #[derive(Debug, Clone, serde::Serialize)]
    pub struct Tool {
        #[serde(rename = "type")]
        pub type_: &'static str,
        pub function: Function,
    }

    impl Into<Tool> for Function {
        fn into(self) -> Tool {
            Tool {
                type_: "function",
                function: self,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub enum Role {
        #[serde(rename = "system")]
        System,
        #[serde(rename = "user")]
        User,
        #[serde(rename = "assistant")]
        Assistant,
        #[serde(rename = "tool")]
        Tool,
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
                Role::Tool => "tool",
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

        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_calls: Option<Vec<ToolCall>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_call_id: Option<String>,
    }

    impl AsRef<Content> for Content {
        fn as_ref(&self) -> &Content {
            self
        }
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct Delta {
        #[serde(default)]
        pub content: Option<String>,
        #[serde(default)]
        pub role: Option<Role>,
        #[serde(default)]
        pub tool_calls: Vec<ToolCall>,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
    pub struct ToolCall {
        pub id: String,
        #[serde(rename = "type")]
        pub type_: String,
        pub function: ToolFunction,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
    pub struct ToolFunction {
        pub name: String,
        pub arguments: String,
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct StableStreamChunkChoices {
        pub delta: Delta,
        pub finish_reason: Option<String>,
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct StableStreamChunk {
        pub choices: Vec<StableStreamChunkChoices>,
    }

    #[test]
    fn test_json() {
        let json_str = r#"{"role":"user","content":null}"#;
        let content = serde_json::from_str::<Content>(json_str);
        println!("content: {:#?}", content);
    }
}

fn merge_tool_into_extra(extra: &mut serde_json::Value, tools: &[llm::Tool]) {
    let mut tool_choice = "";

    let tools = tools
        .iter()
        .map(|t| serde_json::to_value(&t).unwrap())
        .collect::<Vec<_>>();

    if let Some(extra) = extra.as_object_mut() {
        match extra.entry("tools") {
            serde_json::map::Entry::Vacant(e) => {
                if !tools.is_empty() {
                    e.insert(serde_json::Value::Array(tools));
                    tool_choice = "auto";
                }
            }
            serde_json::map::Entry::Occupied(mut e) => {
                if let serde_json::Value::Array(arr) = e.get_mut() {
                    tool_choice = "auto";

                    if !tools.is_empty() {
                        arr.extend(tools);
                    }
                }
            }
        }

        if !tool_choice.is_empty() {
            extra.insert("tool_choice".to_string(), serde_json::json!(tool_choice));
        }
    }
}

pub async fn llm_stable<'p, I: IntoIterator<Item = C>, C: AsRef<llm::Content>>(
    llm_url: &str,
    token: &str,
    model: &str,
    extra: Option<serde_json::Value>,
    prompts: I,
    tools: Vec<llm::Tool>,
) -> anyhow::Result<StableLlmResponse> {
    let messages = prompts
        .into_iter()
        .map(|c| c.as_ref().clone())
        .collect::<Vec<_>>();

    let mut response_builder = reqwest::Client::new().post(llm_url);
    if !token.is_empty() {
        response_builder = response_builder.bearer_auth(token);
    };

    let tool_name = tools
        .iter()
        .map(|t| t.function.name.as_str())
        .collect::<Vec<_>>();

    log::debug!(
        "#### send to llm:\n{}\n#####",
        serde_json::to_string_pretty(&serde_json::json!(
            {
                "stream": true,
                "messages": messages,
                "model": model.to_string(),
                "tools": tool_name,
                "extra": extra,
            }
        ))?
    );

    let mut extra = extra.unwrap_or(serde_json::json!({}));

    merge_tool_into_extra(&mut extra, &tools);

    let request = StableLlmRequest {
        stream: true,
        messages,
        model: model.to_string(),
        extra,
    };

    let response = response_builder
        .header(reqwest::header::USER_AGENT, "curl/7.81.0")
        .json(&request)
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

// cargo test --package esp_assistant --bin esp_assistant -- ai::test_stable_llm --exact --show-output
#[tokio::test]
async fn test_stable_llm() {
    env_logger::init();
    let token = std::env::var("API_KEY").ok().map(|k| format!("Bearer {k}"));

    let prompts = vec![
        llm::Content {
            role: llm::Role::System,
            message: "你是一个聪明的AI助手，你叫做胡桃".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
        llm::Content {
            role: llm::Role::User,
            message: "给我介绍一下妲己".to_string(),
            tool_calls: None,
            tool_call_id: None,
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
        vec![],
    )
    .await
    .unwrap();

    loop {
        match resp.next_chunk().await {
            Ok(StableLLMResponseChunk::Text(chunk)) => {
                println!("{}", chunk);
            }
            Ok(StableLLMResponseChunk::Functions(functions)) => {
                for function in functions {
                    println!("Tool call: {:#?}", function);
                }
            }
            Ok(StableLLMResponseChunk::Stop) => {
                break;
            }
            Err(e) => {
                println!("error: {:#?}", e);
                break;
            }
        }
    }
}

pub struct ChatSession {
    pub api_key: String,
    pub model: String,
    pub extra: Option<serde_json::Value>,
    pub url: String,

    pub history: usize,

    pub system_prompts: Vec<llm::Content>,
    pub messages: LinkedList<llm::Content>,
    pub tools: ToolSet<McpToolAdapter>,
}

impl ChatSession {
    pub fn new(
        url: String,
        api_key: String,
        model: String,
        extra: Option<serde_json::Value>,
        history: usize,
        tools: ToolSet<McpToolAdapter>,
    ) -> Self {
        Self {
            api_key,
            model,
            url,
            extra,
            history,
            system_prompts: Vec::new(),
            messages: LinkedList::new(),
            tools,
        }
    }

    pub fn add_user_message(&mut self, message: String) {
        self.messages.push_back(llm::Content {
            role: llm::Role::User,
            message,
            tool_calls: None,
            tool_call_id: None,
        });
        if self.messages.len() > self.history * 2 {
            self.messages.pop_front();
        }
    }

    pub fn add_assistant_message(&mut self, message: String) {
        self.messages.push_back(llm::Content {
            role: llm::Role::Assistant,
            message,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    pub fn add_assistant_tool_call(&mut self, tool_call: Vec<llm::ToolCall>) {
        self.messages.push_back(llm::Content {
            role: llm::Role::Assistant,
            message: String::new(),
            tool_calls: Some(tool_call),
            tool_call_id: None,
        });
    }

    pub async fn complete(&mut self) -> anyhow::Result<StableLlmResponse> {
        use crate::ai::openai::tool::Tool;

        let prompts = self.system_prompts.iter().chain(self.messages.iter());

        let tools = self
            .tools
            .tools()
            .iter()
            .map(|tool| {
                llm::Function {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters().clone(),
                }
                .into()
            })
            .collect::<Vec<llm::Tool>>();

        let response = llm_stable(
            self.url.as_str(),
            &self.api_key,
            &self.model,
            self.extra.clone(),
            prompts,
            tools,
        )
        .await?;

        Ok(response)
    }

    pub fn get_tool_call_message(&self, tool_call: &llm::ToolCall) -> Option<String> {
        let tool = self.tools.get_tool(tool_call.function.name.as_str())?;
        Some(tool.call_mcp_message().to_string())
    }

    pub async fn execute_tool(&mut self, tool_call: &llm::ToolCall) -> anyhow::Result<()> {
        use crate::ai::openai::tool::Tool;

        let tool = self.tools.get_tool(tool_call.function.name.as_str());
        if let Some(tool) = tool {
            let args: serde_json::Value =
                serde_json::from_str(&tool_call.function.arguments).unwrap_or_default();
            let result = tool.call(args).await?;
            log::debug!("Tool call {} result: {:?}", tool_call.function.name, result);
            if result.is_error.is_some_and(|b| b) {
                log::error!("Tool call {} failed", tool_call.function.name,);
                self.messages.push_back(llm::Content {
                    role: llm::Role::Tool,
                    message: format!(
                        "Tool call {} failed, mcp call error",
                        tool_call.function.name
                    ),
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id.clone()),
                });
            } else {
                result.content.iter().for_each(|content| {
                    if let Some(content_text) = content.as_text() {
                        if let Ok(json_result) =
                            serde_json::from_str::<serde_json::Value>(&content_text.text)
                        {
                            let pretty_result = serde_json::to_string_pretty(&json_result).unwrap();
                            log::info!(
                                "call tool {} result: {}",
                                tool_call.function.name,
                                pretty_result
                            );
                            self.messages.push_back(llm::Content {
                                role: llm::Role::Tool,
                                message: pretty_result,
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                            });
                        } else {
                            log::info!(
                                "call tool {} result: {}",
                                tool_call.function.name,
                                &content_text.text
                            );
                            self.messages.push_back(llm::Content {
                                role: llm::Role::Tool,
                                message: content_text.text.to_string(),
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                            });
                        }
                    } else {
                        if content.as_image().is_some() {
                            log::warn!(
                                "Tool call {} returned an image, which is not supported yet",
                                tool_call.function.name
                            );
                        }
                        if content.as_resource().is_some() {
                            log::warn!(
                                "Tool call {} returned a resource, which is not supported yet",
                                tool_call.function.name
                            );
                        }
                    }
                });
            }
            Ok(())
        } else {
            log::error!(
                "Tool call {} failed, tool not found",
                tool_call.function.name
            );
            self.messages.push_back(llm::Content {
                role: llm::Role::Tool,
                message: format!(
                    "Tool call {} failed, tool not found",
                    tool_call.function.name
                ),
                tool_calls: None,
                tool_call_id: Some(tool_call.id.clone()),
            });
            Ok(())
        }
    }
}

pub async fn load_sse_tools(
    tool_set: &mut ToolSet<McpToolAdapter>,
    clients: &mut Vec<
        rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::InitializeRequestParam>,
    >,
    mcp_servers_url: &str,
    call_mcp_message: &str,
) -> anyhow::Result<()> {
    // load MCP
    let transport = SseClientTransport::start(mcp_servers_url).await?;
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test sse client".to_string(),
            version: "0.0.1".to_string(),
        },
    };
    let client = client_info.serve(transport).await.inspect_err(|e| {
        log::error!("client error: {:?}", e);
    })?;

    let tools = client.list_all_tools().await?;
    for tool in tools {
        let server = client.peer().clone();
        log::info!("add tool: {}", tool.name);
        tool_set.add_tool(McpToolAdapter::new(
            tool,
            call_mcp_message.to_string(),
            server,
        ));
    }
    clients.push(client);
    Ok(())
}

pub async fn load_http_streamable_tools(
    tool_set: &mut ToolSet<McpToolAdapter>,
    clients: &mut Vec<
        rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::InitializeRequestParam>,
    >,
    mcp_servers_url: &str,
    call_mcp_message: &str,
) -> anyhow::Result<()> {
    // load MCP
    let transport = StreamableHttpClientTransport::from_uri(mcp_servers_url);
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test http_streamable client".to_string(),
            version: "0.0.1".to_string(),
        },
    };
    let client = client_info.serve(transport).await.inspect_err(|e| {
        log::error!("client error: {:?}", e);
    })?;

    let tools = client.list_all_tools().await?;
    for tool in tools {
        let server = client.peer().clone();
        log::info!("add tool: {}", tool.name);
        tool_set.add_tool(McpToolAdapter::new(
            tool,
            call_mcp_message.to_string(),
            server,
        ));
    }

    clients.push(client);

    Ok(())
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::test_chat_session --exact --show-output
#[tokio::test]
async fn test_chat_session() {
    env_logger::init();
    let token = std::env::var("API_KEY").ok();

    let prompts = vec![
        llm::Content {
            role: llm::Role::System,
            message: "你是一个聪明的AI助手，你叫做胡桃".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
        llm::Content {
            role: llm::Role::User,
            message: "身高1米6体重180".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let mut clients = vec![];

    let mut tools = ToolSet::default();
    load_http_streamable_tools(&mut tools, &mut clients, "http://localhost:8000/mcp", "")
        .await
        .unwrap();

    log::info!("token: {:#?}", token);

    let mut chat_session = ChatSession::new(
        "https://api.groq.com/openai/v1/chat/completions".to_string(),
        token.unwrap_or_default(),
        "qwen/qwen3-32b".to_string(),
        None,
        10,
        tools,
    );

    chat_session.system_prompts = prompts;

    let mut resp = chat_session
        .complete()
        .await
        .expect("Failed to complete chat session");

    loop {
        match resp.next_chunk().await {
            Ok(StableLLMResponseChunk::Text(chunk)) => {
                log::info!("{}", chunk);
            }
            Ok(StableLLMResponseChunk::Functions(functions)) => {
                for function in functions {
                    log::info!("Tool call: {:#?}", function);
                    chat_session
                        .execute_tool(&function)
                        .await
                        .expect("Failed to execute tool");
                }
                resp = chat_session
                    .complete()
                    .await
                    .expect("Failed to complete chat session after tool call");
            }
            Ok(StableLLMResponseChunk::Stop) => {
                break;
            }
            Err(e) => {
                log::info!("error: {:#?}", e);
                break;
            }
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ResponsesChatRequest<'a> {
    pub model: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub previous_response_id: &'a str,
    pub input: &'a str,
    #[serde(flatten)]
    pub extra: serde_json::Value,
    pub stream: bool,
}

pub struct ResponsesSession {
    pub api_key: String,
    pub model: String,
    pub url: String,

    pub instructions: String,
    pub previous_response_id: String,

    pub extra: Option<serde_json::Value>,
    pub tools: ToolSet<McpToolAdapter>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum ResponsesChunk {
    #[serde(rename = "response.created")]
    Created {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        created_response: serde_json::Value,
    },
    #[serde(rename = "response.in_progress")]
    InProgress {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        in_progress_response: serde_json::Value,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        item_added_response: serde_json::Value,
    },
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        part_added_response: serde_json::Value,
    },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        delta: String,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        output_text_delta_response: serde_json::Value,
    },
    #[serde(rename = "response.output_text.annotation.added")]
    OutputTextAnnotationAdded {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        output_text_annotation_added_response: serde_json::Value,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        text: String,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        output_text_done_response: serde_json::Value,
    },
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        part_done_response: serde_json::Value,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        item: ResponsesOutputItem,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        item_done_response: serde_json::Value,
    },
    #[serde(rename = "response.web_search_call.in_progress")]
    WebSearchCallInProgress {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        web_search_call_in_progress_response: serde_json::Value,
    },
    #[serde(rename = "response.web_search_call.searching")]
    WebSearchCallSearching {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        web_search_call_searching_response: serde_json::Value,
    },
    #[serde(rename = "response.web_search_call.completed")]
    WebSearchCallCompleted {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        web_search_call_completed_response: serde_json::Value,
    },

    #[serde(rename = "response.mcp_call_arguments.delta")]
    McpCallArgumentsDelta {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_arguments_delta_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_call_arguments.done")]
    McpCallArgumentsDone {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_arguments_done_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_call.completed")]
    McpCallCompleted {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_completed_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_call.failed")]
    McpCallFailed {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_failed_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_call.in_progress")]
    McpCallInProgress {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_in_progress_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_list_tools.completed")]
    McpListToolsCompleted {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_list_tools_completed_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_list_tools.failed")]
    McpListToolsFailed {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_list_tools_failed_response: serde_json::Value,
    },
    #[serde(rename = "response.mcp_list_tools.in_progress")]
    McpListToolsInProgress {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_list_tools_in_progress_response: serde_json::Value,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallDelta {
        #[serde(flatten)]
        function_call_response: serde_json::Value,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallDone {
        #[serde(flatten)]
        function_call_done_response: serde_json::Value,
    },
    #[serde(rename = "response.queued")]
    Queued {
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        queued_response: serde_json::Value,
    },
    #[serde(rename = "response.completed")]
    Completed {
        #[serde(default)]
        response: ResponsesCompleted,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum ResponsesOutputItem {
    #[serde(rename = "function_call")]
    Function {
        id: String,
        arguments: String,
        call_id: String,
        name: String,
    },
    #[serde(rename = "message")]
    Message {
        id: String,
        role: llm::Role,
        content: serde_json::Value,
    },
    #[serde(rename = "web_search_call")]
    WebSearch {
        id: String,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        web_search_response: serde_json::Value,
    },
    #[serde(rename = "mcp_call")]
    McpCall {
        id: String,
        name: String,
        arguments: String,
        approval_request_id: String,
        status: String,
        #[serde(default)]
        output: String,
        #[serde(default)]
        error: String,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_call_response: serde_json::Value,
    },
    #[serde(rename = "mcp_list_tools")]
    McpListTools {
        id: String,
        #[serde(default)]
        error: String,
        #[serde(default)]
        tools: Vec<serde_json::Value>,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_list_tools_response: serde_json::Value,
    },
    #[serde(rename = "mcp_approval_request")]
    McpCallApprovalRequest {
        id: String,
        name: String,
        arguments: String,
        #[cfg(debug_assertions)]
        #[serde(flatten)]
        mcp_approval_request_response: serde_json::Value,
    },

    #[serde(other)]
    Other,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct ResponsesCompleted {
    pub id: String,
    #[cfg(debug_assertions)]
    #[serde(flatten)]
    pub completed_response: serde_json::Value,
}

impl ResponsesSession {
    pub fn new(
        url: String,
        api_key: String,
        model: String,
        instructions: String,
        extra: Option<serde_json::Value>,
        tools: ToolSet<McpToolAdapter>,
    ) -> Self {
        let model = if model.is_empty() {
            "gpt-4.1".to_string()
        } else {
            model
        };

        Self {
            api_key,
            model,
            url,
            previous_response_id: String::new(),
            instructions,
            extra,
            tools,
        }
    }

    pub async fn submit_text(&mut self, input: &str) -> anyhow::Result<ResponsesLLmResponse> {
        use crate::ai::openai::tool::Tool;

        let tools = self
            .tools
            .tools()
            .iter()
            .map(|tool| {
                llm::Function {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters().clone(),
                }
                .into()
            })
            .collect::<Vec<llm::Tool>>();

        let mut extra = self.extra.clone().unwrap_or(serde_json::json!({}));
        merge_tool_into_extra(&mut extra, &tools);

        let req = ResponsesChatRequest {
            model: &self.model,
            previous_response_id: &self.previous_response_id,
            input,
            extra,
            stream: true,
        };

        let mut response_builder = reqwest::Client::new().post(&self.url);
        if !self.api_key.is_empty() {
            response_builder = response_builder.bearer_auth(&self.api_key);
        }
        let response = response_builder
            .header(reqwest::header::USER_AGENT, "curl/7.81.0")
            .json(&req)
            .send()
            .await?;

        Ok(ResponsesLLmResponse {
            stopped: false,
            response,
            string_buffer: String::new(),
            previous_response_id: String::new(),
        })
    }

    pub async fn submit_function_output(
        &mut self,
        function_outputs: &[serde_json::Value],
    ) -> anyhow::Result<ResponsesLLmResponse> {
        use crate::ai::openai::tool::Tool;

        let tools = self
            .tools
            .tools()
            .iter()
            .map(|tool| {
                llm::Function {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters().clone(),
                }
                .into()
            })
            .collect::<Vec<llm::Tool>>();

        let mut extra = self.extra.clone().unwrap_or(serde_json::json!({}));
        merge_tool_into_extra(&mut extra, &tools);

        let req = ResponsesChatRequest {
            model: &self.model,
            previous_response_id: &self.previous_response_id,
            input: "",
            extra,
            stream: true,
        };

        let mut req = serde_json::to_value(req)
            .map_err(|e| anyhow::anyhow!("Failed to serialize request: {}", e))?;

        let obj = req.as_object_mut().unwrap();
        obj.insert("input".to_string(), serde_json::json!(function_outputs));

        // obj.insert(
        //     "input".to_string(),
        //     serde_json::json!([{
        //         "type": "function_call_output",
        //         "call_id": call_id,
        //         "output": output,
        //     }]),
        // );

        log::debug!(
            "#### send to responses llm:\n{}\n#####",
            serde_json::to_string_pretty(&req)?
        );

        let mut response_builder = reqwest::Client::new().post(&self.url);
        if !self.api_key.is_empty() {
            response_builder = response_builder.bearer_auth(&self.api_key);
        }
        let response = response_builder
            .header(reqwest::header::USER_AGENT, "curl/7.81.0")
            .json(&req)
            .send()
            .await?;

        Ok(ResponsesLLmResponse {
            stopped: false,
            response,
            string_buffer: String::new(),
            previous_response_id: String::new(),
        })
    }

    pub fn get_tool_call_message(&self, tool_call: &llm::ToolCall) -> Option<String> {
        let tool = self.tools.get_tool(tool_call.function.name.as_str())?;
        Some(tool.call_mcp_message().to_string())
    }

    pub async fn execute_tool(&mut self, tool_call: &llm::ToolCall) -> serde_json::Value {
        use crate::ai::openai::tool::Tool;

        let tool = self.tools.get_tool(tool_call.function.name.as_str());

        if let Some(tool) = tool {
            let args: serde_json::Value =
                serde_json::from_str(&tool_call.function.arguments).unwrap_or_default();
            let result = tool.call(args).await;
            if let Err(e) = &result {
                log::error!(
                    "Tool call {} failed with error: {:?}",
                    tool_call.function.name,
                    e
                );
                return serde_json::json!({
                    "type": "function_call_output",
                    "call_id": &tool_call.id,
                    "output": format!("Error: Tool call {} failed with error: {:?}", tool_call.function.name, e)
                });
            }
            let result = result.unwrap();
            log::debug!("Tool call {} result: {:?}", tool_call.function.name, result);
            if result.is_error.is_some_and(|b| b) {
                log::error!("Tool call {} failed", tool_call.function.name,);
                serde_json::json!({
                    "type": "function_call_output",
                    "call_id": &tool_call.id,
                    "output": format!("Error: Tool call {} failed", tool_call.function.name)
                })
            } else {
                log::debug!("Tool call {} succeeded", tool_call.function.name);
                let content = result
                    .content
                    .iter()
                    .map(|content| {
                        if let Some(content_text) = content.as_text() {
                            content_text.text.clone()
                        } else {
                            "".to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                serde_json::json!({
                    "type": "function_call_output",
                    "call_id": &tool_call.id,
                    "output": content
                })
            }
        } else {
            log::error!(
                "Tool call {} failed, tool not found",
                tool_call.function.name
            );
            serde_json::json!({
                "type": "function_call_output",
                "call_id": &tool_call.id,
                "output": format!("Error: Tool call {} failed, tool not found", tool_call.function.name)
            })
        }
    }
}

pub enum LLMResponsesChunk {
    Functions(Vec<llm::ToolCall>),
    Text(String),

    Stop(String),
}

pub struct ResponsesLLmResponse {
    previous_response_id: String,
    stopped: bool,
    response: reqwest::Response,
    string_buffer: String,
}

impl ResponsesLLmResponse {
    const CHUNK_SIZE: usize = 50;

    fn return_string_buffer(&mut self) -> anyhow::Result<LLMResponsesChunk> {
        self.stopped = true;
        if !self.string_buffer.is_empty() {
            let mut new_str = String::new();
            std::mem::swap(&mut new_str, &mut self.string_buffer);
            return Ok(LLMResponsesChunk::Text(new_str));
        } else {
            return Ok(LLMResponsesChunk::Stop(self.previous_response_id.clone()));
        }
    }

    fn push_str(string_buffer: &mut String, s: &str) -> Option<String> {
        let mut ret = s;

        loop {
            if let Some(i) = ret.find(&['.', '!', '?', ';', '。', '！', '？', '；', '\n']) {
                let ((chunk, ret_), char_len) = if ret.is_char_boundary(i + 1) {
                    (ret.split_at(i + 1), 1)
                } else {
                    (ret.split_at(i + 3), 3)
                };

                string_buffer.push_str(chunk);
                ret = ret_;
                if ret.chars().next().is_some_and(|c| c.is_numeric()) {
                    continue;
                }
                if char_len == 1 && ret.len() > 0 && !ret.starts_with(&[' ', '\n']) {
                    continue;
                }

                if string_buffer.len() > Self::CHUNK_SIZE || string_buffer.ends_with("\n") {
                    let mut new_str = ret.to_string();
                    std::mem::swap(&mut new_str, string_buffer);
                    return Some(new_str);
                }
            } else {
                string_buffer.push_str(ret);
                return None;
            }
        }
    }

    pub async fn next_chunk(&mut self) -> anyhow::Result<LLMResponsesChunk> {
        let mut chunk_ret = String::new();
        loop {
            if self.stopped {
                return Ok(LLMResponsesChunk::Stop(self.previous_response_id.clone()));
            }

            let body = self.response.chunk().await?;
            if body.is_none() {
                return self.return_string_buffer();
            }
            let body = body.unwrap();
            let body = if chunk_ret.is_empty() {
                String::from_utf8_lossy(&body).to_string()
            } else {
                chunk_ret.push_str(&String::from_utf8_lossy(&body));
                let new_body = chunk_ret;
                chunk_ret = String::new();
                new_body
            };
            log::trace!("llm response chunk body: {body}");

            let mut chunks = String::new();
            let mut tools = Vec::new();
            body.split("event: ").for_each(|s| {
                if s.is_empty() || s.starts_with("[DONE]") {
                    return;
                }

                let s_ = s.split_once("data: ");
                let s_ = if let Some((_, data)) = s_ {
                    data
                } else {
                    chunk_ret.push_str("event: ");
                    chunk_ret.push_str(s);
                    return;
                };
                log::trace!("llm response body.split: {s_}");

                if let Ok(chunk) = serde_json::from_str::<ResponsesChunk>(s_.trim()) {
                    // log::debug!("llm response chunk: {:#?}", chunk);
                    match chunk {
                        ResponsesChunk::Completed { response } => {
                            log::debug!("llm response completed: {:#?}", response);
                            self.previous_response_id = response.id;
                            // self.stopped = true;
                            return;
                        }
                        ResponsesChunk::OutputTextDelta { delta, .. } => {
                            log::trace!("llm response delta: {}", delta);
                            chunks.push_str(&delta);
                        }
                        ResponsesChunk::OutputTextDone { text, .. } => {
                            log::trace!("llm response text done: {}", text);
                        }

                        ResponsesChunk::OutputItemDone {
                            item,
                            item_done_response,
                        } => {
                            log::debug!("llm response output item done: {:#?}", item_done_response);
                            match item {
                                ResponsesOutputItem::Function {
                                    id,
                                    name,
                                    arguments,
                                    call_id,
                                } => {
                                    log::info!(
                                        "llm response function call: id={}, call_id={}, name={}, arguments={}",
                                        id,
                                        call_id,
                                        name,
                                        arguments
                                    );
                                    tools.push(llm::ToolCall {
                                        id: call_id,
                                        type_: "function".to_string(),
                                        function: llm::ToolFunction { name, arguments },
                                    });
                                }
                                ResponsesOutputItem::Message { id, role, content } => {
                                    log::info!(
                                        "llm response message: id={}, role={}, content={:#?}",
                                        id,
                                        role,
                                        content
                                    );
                                }
                                ResponsesOutputItem::Other => {
                                    log::warn!("llm response output item other: {:#?}", s_);
                                }
                                other => {
                                    log::warn!("llm response output item not handled: {}", serde_json::to_string_pretty(&other).unwrap());
                                }
                            }
                        }
                        ResponsesChunk::Unknown => {
                            log::error!("llm response unknown chunk: {:#?}", s_);
                        }
                        other => {
                            log::warn!("llm response output item not handled: {}", serde_json::to_string_pretty(&other).unwrap());
                            return;
                        }
                    };
                } else {
                    chunk_ret.push_str("event: ");
                    chunk_ret.push_str(s);
                }
            });

            log::trace!("llm response chunks: {chunks}");
            log::trace!("llm response tools: {:#?}", tools);

            if tools.is_empty() {
                if let Some(new_str) = Self::push_str(&mut self.string_buffer, &chunks) {
                    log::trace!("llm response text: {new_str}");
                    return Ok(LLMResponsesChunk::Text(new_str));
                }
            } else {
                log::trace!("llm response tools: {:#?}", tools);
                return Ok(LLMResponsesChunk::Functions(tools));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_responses_llm_previous_response_id() {
        env_logger::init();
        let token = std::env::var("OPENAI_API_KEY").unwrap();

        log::info!("token: {:#?}", token);

        let mut responses_session = ResponsesSession::new(
            "https://api.openai.com/v1/responses".to_string(),
            token,
            "gpt-4.1".to_string(),
            "You are a helpful assistant. Your name is Echokit.".to_string(),
            None,
            ToolSet::default(),
        );

        let mut tools = vec![];

        for q in &["Hello, who are you?", "What is last thing I asked you?"] {
            let mut resp = responses_session.submit_text(q).await.unwrap();

            let mut chunk_i = 0;

            loop {
                match resp.next_chunk().await {
                    Ok(LLMResponsesChunk::Text(chunk)) => {
                        println!("{chunk_i}:{}", chunk);
                        chunk_i += 1;
                    }
                    Ok(LLMResponsesChunk::Functions(functions)) => {
                        for function in functions {
                            println!("Tool call: {:#?}", function);
                            tools.push(function);
                        }
                    }
                    Ok(LLMResponsesChunk::Stop(previous_response_id)) => {
                        responses_session.previous_response_id = previous_response_id;
                        break;
                    }
                    Err(e) => {
                        println!("error: {:#?}", e);
                        break;
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_responses_llm_function_call() {
        env_logger::init();
        let token = std::env::var("OPENAI_API_KEY").unwrap();

        let mut responses_session = ResponsesSession::new(
            "https://api.openai.com/v1/responses".to_string(),
            token,
            "gpt-4.1".to_string(),
            "You are a helpful assistant. Your name is Echokit.".to_string(),
            Some(serde_json::json!({
                    "tools": [
              {
                "type": "function",
                "name": "get_current_weather",
                "description": "Get the current weather in a given location",
                "parameters": {
                  "type": "object",
                  "properties": {
                    "location": {
                      "type": "string",
                      "description": "The city and state, e.g. San Francisco, CA"
                    },
                    "unit": {
                      "type": "string",
                      "enum": ["celsius", "fahrenheit"]
                    }
                  },
                  "required": ["location", "unit"]
                }
              }
            ],
                })),
            ToolSet::default(),
        );

        let mut tools = vec![];

        let mut resp = responses_session
            .submit_text("What is the weather like in Boston today?")
            .await
            .unwrap();

        let mut chunk_i = 0;

        loop {
            match resp.next_chunk().await {
                Ok(LLMResponsesChunk::Text(chunk)) => {
                    println!("{chunk_i}:{}", chunk);
                    chunk_i += 1;
                }
                Ok(LLMResponsesChunk::Functions(functions)) => {
                    for function in functions {
                        println!("Tool call: {:#?}", function);
                        tools.push(function);
                    }
                }
                Ok(LLMResponsesChunk::Stop(previous_response_id)) => {
                    responses_session.previous_response_id = previous_response_id;
                    break;
                }
                Err(e) => {
                    println!("error: {:#?}", e);
                    break;
                }
            }
        }

        let mut resp = responses_session
            .submit_function_output(&[serde_json::json!({
                "type": "function_call_output",
                "call_id": &tools[0].id,
                "output": serde_json::to_string_pretty(&serde_json::json!({
                    "temperature": "22",
                    "unit": "celsius",
                    "condition": "sunny"
                }))
                .unwrap(),
            })])
            .await
            .unwrap();

        let mut chunk_i = 0;

        loop {
            match resp.next_chunk().await {
                Ok(LLMResponsesChunk::Text(chunk)) => {
                    println!("{chunk_i}:{}", chunk);
                    chunk_i += 1;
                }
                Ok(LLMResponsesChunk::Functions(functions)) => {
                    for function in functions {
                        println!("Tool call: {:#?}", function);
                    }
                }
                Ok(LLMResponsesChunk::Stop(previous_response_id)) => {
                    responses_session.previous_response_id = previous_response_id;
                    break;
                }
                Err(e) => {
                    println!("error: {:#?}", e);
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_responses_llm_web_search() {
        env_logger::init();
        let token = std::env::var("OPENAI_API_KEY").unwrap();
        let mut responses_session = ResponsesSession::new(
            "https://api.openai.com/v1/responses".to_string(),
            token,
            "gpt-4.1".to_string(),
            "You are a helpful assistant. Your name is Echokit.".to_string(),
            Some(serde_json::json!({
                "tools": [{"type": "web_search"}],
            })),
            ToolSet::default(),
        );

        let mut resp = responses_session
            .submit_text("What is Echokit")
            .await
            .unwrap();

        let mut chunk_i = 0;
        loop {
            match resp.next_chunk().await {
                Ok(LLMResponsesChunk::Text(chunk)) => {
                    println!("{chunk_i}:{}", chunk);
                    chunk_i += 1;
                }
                Ok(LLMResponsesChunk::Functions(functions)) => {
                    for function in functions {
                        println!("Tool call: {:#?}", function);
                    }
                }
                Ok(LLMResponsesChunk::Stop(previous_response_id)) => {
                    responses_session.previous_response_id = previous_response_id;
                    break;
                }
                Err(e) => {
                    println!("error: {:#?}", e);
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_responses_llm_mcp_call() {
        env_logger::init();
        let token = std::env::var("OPENAI_API_KEY").unwrap();
        let mut responses_session = ResponsesSession::new(
            "https://api.openai.com/v1/responses".to_string(),
            token,
            "gpt-4.1".to_string(),
            "You are a helpful assistant. Your name is Echokit.".to_string(),
            Some(serde_json::json!({
                "tools": [{
                    "type": "mcp",
                    "server_label": "tavily",
                    "server_url": "https://mcp.tavily.com/mcp/?tavilyApiKey=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",
                    "require_approval": "never"
                }],
            })),
            ToolSet::default(),
        );

        let mut resp = responses_session
            .submit_text("What is Echokit")
            .await
            .unwrap();

        let mut chunk_i = 0;
        loop {
            match resp.next_chunk().await {
                Ok(LLMResponsesChunk::Text(chunk)) => {
                    println!("{chunk_i}:{}", chunk);
                    chunk_i += 1;
                }
                Ok(LLMResponsesChunk::Functions(functions)) => {
                    for function in functions {
                        println!("Tool call: {:#?}", function);
                    }
                }
                Ok(LLMResponsesChunk::Stop(previous_response_id)) => {
                    responses_session.previous_response_id = previous_response_id;
                    break;
                }
                Err(e) => {
                    println!("error: {:#?}", e);
                    break;
                }
            }
        }
    }
}
