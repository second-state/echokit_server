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
pub struct GenaiConfig {
    pub api_key: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub sys_prompts: Option<Content>,
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
    #[serde(default)]
    pub lang: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub addr: String,

    pub hello_wav: Option<String>,
    pub background_gif: Option<String>,

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
    GenaiAndTTS {
        genai: GenaiConfig,
        tts: TTSConfig,
    },
    Genai {
        genai: GenaiConfig,
    },
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}
