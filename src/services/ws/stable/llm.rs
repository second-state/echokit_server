use std::collections::HashMap;

use lazy_regex::regex;

use crate::ai::{
    ChatSession, LLMResponsesChunk, ResponsesSession, StableLLMResponseChunk, llm::Content,
};

pub type ChunksTx = tokio::sync::mpsc::UnboundedSender<(String, super::tts::TTSResponseRx)>;
pub type ChunksRx = tokio::sync::mpsc::UnboundedReceiver<(String, super::tts::TTSResponseRx)>;

use tokio::time::Duration;

#[cached::proc_macro::cached(time = 60, size = 100, result = true)]
async fn load_url_content(url: String) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let res = client.get(&url).send().await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "load url content failed, status:{}, body:{}",
            status,
            body
        ));
    }
    let content = res
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("error reading content from {}: {}", url, e))?;
    Ok(content)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromptParts {
    pub sys_prompts: Vec<Content>,
    pub dynamic_prompts: std::collections::LinkedList<Content>,
}

impl crate::config::ChatConfig {
    pub async fn prompts(&self) -> PromptParts {
        if !self.prompts_url.is_empty() {
            match self.load_prompts().await {
                Ok(prompt_parts) => return prompt_parts,
                Err(e) => {
                    log::warn!("error loading prompt from url {}: {}", self.prompts_url, e);
                }
            }
        }

        let r = regex!(r"\{\{(?P<url>\s*https?://\S+?\s*)\}\}");

        let mut urls = vec![];
        let mut contents = HashMap::new();

        let mut sys_prompts = self.sys_prompts.clone();
        if let Some(sys_prompt) = sys_prompts.first_mut() {
            if sys_prompt.role == crate::ai::llm::Role::System {
                for cap in r.captures_iter(&sys_prompt.message) {
                    if let Some(url) = cap.name("url") {
                        let url = url.as_str().trim();
                        urls.push(url.to_string());
                    }
                }
                log::debug!("found urls in system prompt: {:?}", urls);

                for url in urls {
                    match load_url_content(url.clone()).await {
                        Ok(content) => {
                            contents.insert(url, content);
                        }
                        Err(e) => {
                            log::warn!("error loading prompt content from {}: {}", url, e);
                        }
                    }
                }

                let new_message =
                    r.replace_all(&sys_prompt.message, |caps: &lazy_regex::Captures| {
                        let url = caps.name("url").unwrap().as_str().trim();
                        contents.get(url).cloned().unwrap_or(url.to_string())
                    });

                sys_prompt.message = new_message.to_string();
            }
        }

        {
            PromptParts {
                sys_prompts,
                dynamic_prompts: self.dynamic_prompts.clone(),
            }
        }
    }

    async fn load_prompts(&self) -> anyhow::Result<PromptParts> {
        let prompt_url = &self.prompts_url;
        let client = reqwest::Client::new();
        let res = client.get(prompt_url).send().await?;
        let status = res.status();
        if status != 200 {
            let body = res.text().await?;
            return Err(anyhow::anyhow!(
                "load prompt failed, status:{}, body:{}",
                status,
                body
            ));
        }
        let prompt = res
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("error parsing prompt json from {}: {}", prompt_url, e))?;
        Ok(prompt)
    }
}

#[tokio::test]
async fn test_load_prompts() {
    let message =
        "You are a helpful assistant. {{ https://langchain-ai.github.io/langgraph/llms.txt?a=1 }}";

    let llm_config = crate::config::ChatConfig {
        model: "test-model".to_string(),
        api_key: None,
        sys_prompts: vec![Content {
            role: crate::ai::llm::Role::System,
            message: message.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }],
        dynamic_prompts: std::collections::LinkedList::new(),
        prompts_url: "".to_string(),
        llm_chat_url: "".to_string(),
        history: 5,
        mcp_server: vec![],
        extra: None,
    };

    let now = tokio::time::Instant::now();
    let _prompts = llm_config.prompts().await;
    let elapsed = now.elapsed();
    println!("First Time elapsed to load prompts: {:?}", elapsed);

    let now = tokio::time::Instant::now();
    let prompts = llm_config.prompts().await;
    let elapsed = now.elapsed();
    println!("Second Time elapsed to load prompts: {:?}", elapsed);

    assert!(!prompts.sys_prompts.first().unwrap().message.contains("{{"));
    assert!(prompts.sys_prompts.first().unwrap().message.len() > message.len());
}

pub trait LLMConfigExt {
    type Prompts;
    async fn get_prompts(&self) -> Self::Prompts;
}

pub trait LLMExt {
    type Prompts: 'static + Send;
    type Config: LLMConfigExt<Prompts = Self::Prompts>;

    async fn handle_asr_result(
        &mut self,
        tts_tx: &mut super::tts::TTSRequestTx,
        chunks_tx: ChunksTx,
        asr_result: String,
    ) -> anyhow::Result<()>;

    fn set_prompts(&mut self, prompts: Self::Prompts);

    fn init_session(
        config: &Self::Config,
        tools: crate::ai::openai::tool::ToolSet<crate::ai::openai::tool::McpToolAdapter>,
    ) -> Self;
}

pub async fn chat(
    tts_tx: &mut super::tts::TTSRequestTx,
    chunks_tx: ChunksTx,
    chat_session: &mut ChatSession,
    asr_result: String,
) -> anyhow::Result<()> {
    let message = asr_result;

    if matches!(
        chat_session.messages.back(),
        Some(Content {
            role: crate::ai::llm::Role::User,
            ..
        })
    ) {
        chat_session.messages.pop_back();
    }

    chat_session.add_user_message(message);

    log::info!("start llm");

    let mut resp = chat_session
        .complete()
        .await
        .map_err(|e| anyhow::anyhow!("error completing chat session for message: {e}"))?;

    let mut llm_response = String::with_capacity(128);

    loop {
        match resp.next_chunk().await {
            Ok(StableLLMResponseChunk::Text(chunk)) => {
                let chunk_ = chunk.trim();
                log::debug!("llm chunk: {chunk_:?}");

                llm_response.push_str(&chunk);
                if chunk_.is_empty() {
                    continue;
                }

                let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();

                tts_tx
                    .send((chunk_.to_string(), tts_resp_tx))
                    .await
                    .map_err(|e| anyhow::anyhow!("error sending tts request for llm chunk: {e}"))?;

                chunks_tx
                    .send((chunk_.to_string(), tts_resp_rx))
                    .map_err(|e| {
                        anyhow::anyhow!("error sending tts chunks receiver for llm chunk: {e}")
                    })?;
            }
            Ok(StableLLMResponseChunk::Functions(functions)) => {
                log::info!("llm functions: {:#?}", functions);
                chat_session.add_assistant_tool_call(functions.clone());
                for function in functions {
                    if let Some(message) = chat_session.get_tool_call_message(&function) {
                        log::info!("tool {} call message: {}", &function.function.name, message);
                        if !message.is_empty() {
                            let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();

                            tts_tx
                                .send((message.to_string(), tts_resp_tx))
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("error sending tts request for llm chunk: {e}")
                                })?;

                            chunks_tx.send((message, tts_resp_rx)).map_err(|e| {
                                anyhow::anyhow!(
                                    "error sending tts chunks receiver for llm chunk: {e}"
                                )
                            })?;
                        }
                    }
                    chat_session.execute_tool(&function).await?
                }
                resp = chat_session.complete().await?;
                continue;
            }
            Ok(StableLLMResponseChunk::Stop) => {
                log::info!("llm done");

                if !llm_response.is_empty() {
                    chat_session.add_assistant_message(llm_response);
                }

                break;
            }
            Err(e) => {
                log::error!("llm error: {:#?}", e);
                break;
            }
        }
    }
    Ok(())
}

impl LLMConfigExt for crate::config::ChatConfig {
    type Prompts = PromptParts;

    async fn get_prompts(&self) -> Self::Prompts {
        self.prompts().await
    }
}

impl LLMExt for ChatSession {
    type Prompts = PromptParts;
    type Config = crate::config::ChatConfig;

    async fn handle_asr_result(
        &mut self,
        tts_tx: &mut super::tts::TTSRequestTx,
        chunks_tx: ChunksTx,
        asr_result: String,
    ) -> anyhow::Result<()> {
        chat(tts_tx, chunks_tx, self, asr_result).await
    }

    fn set_prompts(&mut self, prompts: Self::Prompts) {
        self.system_prompts = prompts.sys_prompts;
        self.messages = prompts.dynamic_prompts;
    }

    fn init_session(
        config: &Self::Config,
        tools: crate::ai::openai::tool::ToolSet<crate::ai::openai::tool::McpToolAdapter>,
    ) -> Self {
        ChatSession::new(
            config.llm_chat_url.clone(),
            config.api_key.clone().unwrap_or_default(),
            config.model.clone(),
            config.extra.clone(),
            config.history,
            tools,
        )
    }
}

impl crate::config::ResponsesConfig {
    pub async fn instructions(&self) -> String {
        let r = regex!(r"\{\{(?P<url>\s*https?://\S+?\s*)\}\}");

        let instructions;

        if let Some(system) = self.sys_prompts.first() {
            if system.role == crate::ai::llm::Role::System {
                instructions = &system.message;
            } else {
                instructions = &self.instructions;
            }
        } else {
            instructions = &self.instructions;
        }

        let mut urls = vec![];
        let mut contents = HashMap::new();
        for cap in r.captures_iter(&instructions) {
            if let Some(url) = cap.name("url") {
                let url = url.as_str().trim();
                urls.push(url.to_string());
            }
        }
        log::debug!("found urls in instructions: {:?}", urls);
        for url in urls {
            match load_url_content(url.clone()).await {
                Ok(content) => {
                    contents.insert(url, content);
                }
                Err(e) => {
                    log::warn!("error loading instruction content from {}: {}", url, e);
                }
            }
        }

        let new_instructions = r.replace_all(&instructions, |caps: &lazy_regex::Captures| {
            let url = caps.name("url").unwrap().as_str().trim();
            contents.get(url).cloned().unwrap_or(url.to_string())
        });
        log::debug!("replaced instructions: {}", new_instructions);
        new_instructions.to_string()
    }
}

pub async fn responses(
    tts_tx: &mut super::tts::TTSRequestTx,
    chunks_tx: ChunksTx,
    responses_session: &mut ResponsesSession,
    asr_result: String,
) -> anyhow::Result<()> {
    log::info!("start llm responses");
    let mut resp = responses_session
        .submit_text(&asr_result)
        .await
        .map_err(|e| anyhow::anyhow!("error completing responses session for message: {e}"))?;

    loop {
        match resp.next_chunk().await? {
            LLMResponsesChunk::Text(chunk) => {
                let chunk_ = chunk.trim();
                log::debug!("llm responses chunk: {chunk_:?}");

                if chunk_.is_empty() {
                    continue;
                }

                let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();

                tts_tx
                    .send((chunk_.to_string(), tts_resp_tx))
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("error sending tts request for llm responses chunk: {e}")
                    })?;

                chunks_tx
                    .send((chunk_.to_string(), tts_resp_rx))
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "error sending tts chunks receiver for llm responses chunk: {e}"
                        )
                    })?;
            }
            LLMResponsesChunk::Functions(functions) => {
                log::info!("llm responses functions: {:#?}", functions);
                let mut function_responses = vec![];
                for function in functions {
                    if let Some(message) = responses_session.get_tool_call_message(&function) {
                        log::info!("tool {} call message: {}", &function.function.name, message);
                        if !message.is_empty() {
                            let (tts_resp_tx, tts_resp_rx) = tokio::sync::mpsc::unbounded_channel();

                            tts_tx
                                .send((message.to_string(), tts_resp_tx))
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("error sending tts request for llm chunk: {e}")
                                })?;

                            chunks_tx.send((message, tts_resp_rx)).map_err(|e| {
                                anyhow::anyhow!(
                                    "error sending tts chunks receiver for llm chunk: {e}"
                                )
                            })?;
                        }
                    }
                    let result = responses_session.execute_tool(&function).await;
                    function_responses.push(result);
                }
                responses_session
                    .submit_function_output(&function_responses)
                    .await?;
                continue;
            }
            LLMResponsesChunk::Stop(previous_response_id) => {
                log::info!("llm responses done");
                responses_session.previous_response_id = previous_response_id;
                break;
            }
        }
    }

    Ok(())
}

impl LLMConfigExt for crate::config::ResponsesConfig {
    type Prompts = String;

    async fn get_prompts(&self) -> Self::Prompts {
        self.instructions().await
    }
}

impl LLMExt for ResponsesSession {
    type Prompts = String;
    type Config = crate::config::ResponsesConfig;

    async fn handle_asr_result(
        &mut self,
        tts_tx: &mut super::tts::TTSRequestTx,
        chunks_tx: ChunksTx,
        asr_result: String,
    ) -> anyhow::Result<()> {
        responses(tts_tx, chunks_tx, self, asr_result).await
    }

    fn set_prompts(&mut self, prompts: Self::Prompts) {
        self.instructions = prompts;
        self.previous_response_id.clear();
    }

    fn init_session(
        config: &Self::Config,
        tools: crate::ai::openai::tool::ToolSet<crate::ai::openai::tool::McpToolAdapter>,
    ) -> Self {
        Self::new(
            config.llm_responses_url.clone(),
            config.api_key.clone(),
            config.model.clone(),
            config.instructions.clone(),
            config.extra.clone(),
            tools,
        )
    }
}

pub enum MixPrompts {
    Chat(PromptParts),
    Responses(String),
}

impl LLMConfigExt for crate::config::LLMConfig {
    type Prompts = MixPrompts;

    async fn get_prompts(&self) -> Self::Prompts {
        match self {
            crate::config::LLMConfig::OpenAIChat(chat_config) => {
                let prompts = chat_config.prompts().await;
                MixPrompts::Chat(prompts)
            }
            crate::config::LLMConfig::OpenAIResponses(responses_config) => {
                let instructions = responses_config.instructions().await;
                MixPrompts::Responses(instructions)
            }
        }
    }
}

pub enum LLMSession {
    Chat(ChatSession),
    Responses(ResponsesSession),
}

impl LLMExt for LLMSession {
    type Prompts = MixPrompts;

    type Config = crate::config::LLMConfig;

    async fn handle_asr_result(
        &mut self,
        tts_tx: &mut super::tts::TTSRequestTx,
        chunks_tx: ChunksTx,
        asr_result: String,
    ) -> anyhow::Result<()> {
        match self {
            LLMSession::Chat(chat_session) => {
                ChatSession::handle_asr_result(chat_session, tts_tx, chunks_tx, asr_result).await
            }
            LLMSession::Responses(responses_session) => {
                ResponsesSession::handle_asr_result(
                    responses_session,
                    tts_tx,
                    chunks_tx,
                    asr_result,
                )
                .await
            }
        }
    }

    fn set_prompts(&mut self, prompts: Self::Prompts) {
        match (self, prompts) {
            (LLMSession::Chat(chat_session), MixPrompts::Chat(prompt_parts)) => {
                chat_session.set_prompts(prompt_parts);
            }
            (LLMSession::Responses(responses_session), MixPrompts::Responses(instructions)) => {
                responses_session.set_prompts(instructions);
            }
            _ => {
                #[cfg(debug_assertions)]
                panic!("mismatched prompts and session types");
                #[cfg(not(debug_assertions))]
                log::error!("mismatched prompts and session types");
            }
        }
    }

    fn init_session(
        config: &Self::Config,
        tools: crate::ai::openai::tool::ToolSet<crate::ai::openai::tool::McpToolAdapter>,
    ) -> Self {
        match config {
            crate::config::LLMConfig::OpenAIChat(chat_config) => {
                Self::Chat(ChatSession::init_session(chat_config, tools))
            }
            crate::config::LLMConfig::OpenAIResponses(responses_config) => {
                Self::Responses(ResponsesSession::init_session(responses_config, tools))
            }
        }
    }
}
