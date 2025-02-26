use std::collections::LinkedList;

use crate::ai::llm::Content;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMConfig {
    pub llm_chat_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub sys_prompts: Vec<Content>,
    #[serde(default)]
    pub dynamic_prompts: LinkedList<Content>,
    pub history: usize,
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
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "platform")]
pub enum TTSConfig {
    Stable(StableTTS),
    Fish(FishTTS),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ASRConfig {
    pub url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub addr: String,
    pub llm: LLMConfig,
    pub tts: TTSConfig,
    pub asr: ASRConfig,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}
