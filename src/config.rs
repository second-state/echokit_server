use std::collections::LinkedList;

use crate::ai::llm::Content;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MCPType {
    #[serde(rename = "sse")]
    SSE,
    #[serde(rename = "http_streamable")]
    HttpStreamable,
}
impl Default for MCPType {
    fn default() -> Self {
        MCPType::HttpStreamable
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MCPServerConfig {
    pub server: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(rename = "type", default)]
    pub type_: MCPType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMConfig {
    pub llm_chat_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub sys_prompts: Vec<Content>,
    #[serde(default)]
    pub dynamic_prompts: LinkedList<Content>,
    pub history: usize,
    #[serde(default)]
    pub mcp_server: Vec<MCPServerConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeminiConfig {
    pub api_key: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub sys_prompts: Vec<Content>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FishTTS {
    pub api_key: String,
    pub speaker: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StableTTS {
    #[serde(default)]
    pub api_key: String,
    pub url: String,
    pub speaker: String,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroqTTS {
    pub api_key: String,
    pub model: String,
    pub voice: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamGSV {
    #[serde(default)]
    pub api_key: String,
    pub url: String,
    pub speaker: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "platform")]
pub enum TTSConfig {
    Stable(StableTTS),
    Fish(FishTTS),
    Groq(GroqTTS),
    StreamGSV(StreamGSV),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ASRConfig {
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub vad_url: Option<String>,
    #[serde(default)]
    pub vad_realtime_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub addr: String,

    pub hello_wav: Option<String>,

    #[serde(flatten)]
    pub config: AIConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AIConfig {
    Stable {
        llm: LLMConfig,
        tts: TTSConfig,
        asr: ASRConfig,
    },
    GeminiAndTTS {
        gemini: GeminiConfig,
        tts: TTSConfig,
    },
    Gemini {
        gemini: GeminiConfig,
    },
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}
