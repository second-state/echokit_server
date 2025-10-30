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

pub use crate::ai::bailian::cosyvoice::CosyVoiceVersion;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CosyVoiceTTS {
    pub token: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub version: CosyVoiceVersion,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElevenlabsTTS {
    pub token: String,
    pub voice: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AliyunTTS {
    #[serde(default)]
    pub appkey: String,
    pub url: String,
    pub token: String,
    pub voice: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "platform")]
pub enum TTSConfig {
    Stable(StableTTS),
    Fish(FishTTS),
    Groq(GroqTTS),
    StreamGSV(StreamGSV),
    CosyVoice(CosyVoiceTTS),
    Elevenlabs(ElevenlabsTTS),
    Aliyun(AliyunTTS),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VADConfig {
    #[serde(default)]
    pub vad_url: Option<String>,
    #[serde(default)]
    pub vad_realtime_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WhisperASRConfig {
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AliyunASR {
    pub url: String,
    #[serde(default)]
    pub appkey: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParaformerV2AsrConfig {
    pub paraformer_token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ASRConfig {
    Whisper(WhisperASRConfig),
    ParaformerV2(ParaformerV2AsrConfig),
    Aliyun(AliyunASR),
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
        vad: VADConfig,
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
