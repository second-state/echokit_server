use std::collections::LinkedList;

use crate::ai::llm::Content;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub enum MCPType {
    #[serde(rename = "sse")]
    SSE,
    #[serde(rename = "http_streamable")]
    #[default]
    HttpStreamable,
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
#[serde(tag = "platform")]
pub enum TTSConfig {
    Stable(StableTTS),
    Fish(FishTTS),
    Groq(GroqTTS),
    StreamGSV(StreamGSV),
    CosyVoice(CosyVoiceTTS),
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
        let mut config: Config = toml::from_str(&content)?;
        config.load_api_key()?;
        Ok(config)
    }

    /// load api key from environment variable or .env file, key: LLM_API_KEY, TTS_API_KEY, ASR_API_KEY
    pub fn load_api_key(&mut self) -> anyhow::Result<()> {
        let api_key = std::env::var("LLM_API_KEY").expect("LLM_API_KEY is not set");
        match &mut self.config {
            AIConfig::Stable { llm, tts, asr } => {
                llm.api_key = Some(api_key);
                load_tts_api_key(tts);
                load_asr_api_key(asr);
            },
            AIConfig::GeminiAndTTS { gemini, tts } => {
                gemini.api_key = api_key;
                load_tts_api_key(tts);
            },
            AIConfig::Gemini { gemini } => {
                gemini.api_key = api_key;
            },
        };
        Ok(())
    }
}

fn load_tts_api_key(tts: &mut TTSConfig) {
    let tts_api_key = std::env::var("TTS_API_KEY").expect("TTS_API_KEY is not set");

    match tts {
        TTSConfig::Stable(tts) => {
            tts.api_key = tts_api_key;
        },
        TTSConfig::Fish(tts) => {
            tts.api_key = tts_api_key;
        },
        TTSConfig::Groq(tts) => {
            tts.api_key = tts_api_key;
        },
        TTSConfig::StreamGSV(tts) => {
            tts.api_key = tts_api_key;
        },
        TTSConfig::CosyVoice(tts) => {
            tts.token = tts_api_key;
        },
    }
}

fn load_asr_api_key(asr: &mut ASRConfig) {
    let asr_api_key = std::env::var("ASR_API_KEY").expect("ASR_API_KEY is not set");

    match asr {
        ASRConfig::Whisper(asr) => {
            asr.api_key = asr_api_key;
        },
        ASRConfig::ParaformerV2(asr) => {
            asr.paraformer_token = asr_api_key;
        },
    }
}
