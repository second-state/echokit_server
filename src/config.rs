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
#[serde(tag = "platform")]
pub enum LLMConfig {
    #[serde(alias = "openai_chat")]
    OpenAIChat(ChatConfig),
    #[serde(alias = "openai_responses")]
    OpenAIResponses(ResponsesConfig),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatConfig {
    #[serde(alias = "url")]
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
pub struct ResponsesConfig {
    #[serde(alias = "url")]
    #[serde(default = "ResponsesConfig::default_responses_url")]
    pub llm_responses_url: String,
    #[serde(default = "ResponsesConfig::default_model")]
    pub model: String,
    #[serde(default = "ResponsesConfig::default_instructions")]
    pub instructions: String,
    #[serde(default)]
    pub api_key: String,
    // local mcp servers
    #[serde(default)]
    pub mcp_server: Vec<MCPServerConfig>,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

impl ResponsesConfig {
    fn default_model() -> String {
        "gpt-4.1".to_string()
    }

    fn default_instructions() -> String {
        "You are a helpful assistant.".to_string()
    }

    fn default_responses_url() -> String {
        "https://api.openai.com/v1/responses".to_string()
    }
}

fn default_gemini_model() -> Option<String> {
    Some("models/gemini-2.0-flash-exp".to_string())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeminiConfig {
    pub api_key: String,
    #[serde(default = "default_gemini_model")]
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
    #[serde(alias = "OpenAI", alias = "openai")]
    Openai(OpenaiTTS),
    #[serde(alias = "Stable", alias = "gsv")]
    GSV(GSVTTS),
    #[serde(alias = "fish")]
    Fish(FishTTS),
    #[serde(alias = "groq")]
    Groq(GroqTTS),
    #[serde(alias = "stream_gsv")]
    StreamGSV(StreamGSV),
    #[serde(alias = "cosyvoice")]
    CosyVoice(CosyVoiceTTS),
    #[serde(alias = "elevenlabs")]
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

fn default_paraformer_v2_url() -> String {
    "wss://dashscope.aliyuncs.com/api-ws/v1/inference".to_string()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParaformerV2AsrConfig {
    #[serde(default = "default_paraformer_v2_url")]
    pub url: String,
    pub paraformer_token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "platform")]
pub enum ASRConfig {
    #[serde(alias = "whisper", alias = "openai", alias = "OpenAI")]
    Whisper(WhisperASRConfig),
    #[serde(alias = "paraformer_v2")]
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
