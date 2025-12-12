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
    #[serde(default)]
    pub call_mcp_message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMConfig {
    pub llm_chat_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompts_url: String,
    #[serde(default)]
    pub sys_prompts: Vec<Content>,
    #[serde(default)]
    pub dynamic_prompts: LinkedList<Content>,
    pub history: usize,
    #[serde(default)]
    pub mcp_server: Vec<MCPServerConfig>,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeminiConfig {
    pub api_key: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub sys_prompts: Vec<Content>,
}

fn default_fish_tts_url() -> String {
    "https://api.fish.audio/v1/tts".to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FishTTS {
    #[serde(default = "default_fish_tts_url")]
    pub url: String,
    pub api_key: String,
    pub speaker: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GSVTTS {
    #[serde(default)]
    pub api_key: String,
    pub url: String,
    pub speaker: String,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
}

fn default_groq_tts_model() -> String {
    "playai-tts".to_string()
}

fn default_groq_tts_model_url() -> String {
    "https://api.groq.com/openai/v1/audio/speech".to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroqTTS {
    pub api_key: String,
    #[serde(default = "default_groq_tts_model")]
    pub model: String,
    pub voice: String,
    #[serde(default = "default_groq_tts_model_url")]
    pub url: String,
}

fn default_openai_tts_model() -> String {
    "gpt-4o-tts".to_string()
}

fn default_openai_tts_model_url() -> String {
    "https://api.openai.com/v1/audio/speech".to_string()
}

fn default_openai_tts_voice() -> String {
    "alloy".to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenaiTTS {
    pub api_key: String,
    #[serde(default = "default_openai_tts_model")]
    pub model: String,
    #[serde(default = "default_openai_tts_voice")]
    pub voice: String,
    #[serde(default = "default_openai_tts_model_url")]
    pub url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamGSV {
    #[serde(default)]
    pub api_key: String,
    pub url: String,
    pub speaker: String,
}

pub use crate::ai::bailian::cosyvoice::{CosyVoiceTTS as CosyVoice, CosyVoiceVersion};

fn default_cosyvoice_tts_url() -> String {
    CosyVoice::WEBSOCKET_URL.to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CosyVoiceTTS {
    #[serde(default = "default_cosyvoice_tts_url")]
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub version: CosyVoiceVersion,
}

fn default_elevenlabs_tts_url() -> String {
    "wss://api.elevenlabs.io/v1/text-to-speech".to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElevenlabsTTS {
    #[serde(default = "default_elevenlabs_tts_url")]
    pub url: String,
    pub token: String,
    pub voice: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub language_code: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "platform")]
pub enum TTSConfig {
    #[serde(alias = "OpenAI")]
    Openai(OpenaiTTS),
    #[serde(alias = "Stable")]
    GSV(GSVTTS),
    Fish(FishTTS),
    Groq(GroqTTS),
    StreamGSV(StreamGSV),
    CosyVoice(CosyVoiceTTS),
    Elevenlabs(ElevenlabsTTS),
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
    #[serde(default)]
    pub vad_url: Option<String>,
    #[serde(default)]
    pub vad_realtime_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParaformerV2AsrConfig {
    pub paraformer_token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ASRConfig {
    Whisper(WhisperASRConfig),
    ParaformerV2(ParaformerV2AsrConfig),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub addr: String,

    #[serde(default)]
    pub hello_wav: Option<String>,

    #[serde(default)]
    pub record: RecordConfig,

    #[serde(flatten)]
    pub config: AIConfig,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordConfig {
    #[serde(default)]
    pub callback_url: Option<String>,
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
