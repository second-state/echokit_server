// connect_config: {"setup": {"model": "models/gemini-2.0-flash-live-001", "generationConfig": {"responseModalities": ["TEXT"]}, "systemInstruction": {"parts": [{"text": "You are a helpful assistant and answer in a friendly tone."}]}}}

use base64::Engine;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Content {
    pub parts: Vec<Parts>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Parts {
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "inlineData")]
    InlineData {
        data: Blob,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Modality {
    TEXT,
    IMAGE,
    AUDIO,
}
impl Default for Modality {
    fn default() -> Self {
        Modality::TEXT
    }
}

#[derive(Debug, Clone)]
pub struct Blob(Vec<u8>);
impl Blob {
    pub fn new(data: Vec<u8>) -> Self {
        Blob(data)
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}
impl serde::Serialize for Blob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&base64::prelude::BASE64_STANDARD.encode(&self.0))
    }
}

impl<'de> serde::Deserialize<'de> for Blob {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let data = base64::prelude::BASE64_STANDARD
            .decode(s)
            .map_err(serde::de::Error::custom)?;
        Ok(Blob(data))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RealtimeInput {
    #[serde(rename = "audio")]
    Audio(RealtimeAudio),
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "audioStreamEnd")]
    AudioStreamEnd(bool),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RealtimeAudio {
    pub data: Blob,
    pub mime_type: String,
}

// response: {'serverContent': {'modelTurn': {'parts': [{'text': '是一个大型语言模型，由 Google 训练。'}]}}}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ServerContent {
    #[serde(rename = "modelTurn")]
    ModelTurn(Content),
    #[serde(rename = "generationComplete")]
    GenerationComplete(bool),
    #[serde(rename = "turnComplete")]
    TurnComplete(bool),
    #[serde(rename = "interrupted")]
    Interrupted(bool),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ServerContent_ {
    #[serde(rename = "serverContent")]
    pub server_content: ServerContent,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_content_serialization() {
        let content = Content {
            parts: vec![Parts::Text("Hello, world!".to_string())],
        };
        let serialized = serde_json::to_string(&content).unwrap();
        println!("Serialized Content: {}", serialized);
    }

    #[test]
    fn test_setup_serialization() {
        let setup = Setup {
            model: "models/gemini-2.0-flash-live-001".to_string(),
            generation_config: Some(GenerationConfig {
                stop_sequences: None,
                response_mime_type: None,
                response_modalities: Some(vec![Modality::AUDIO]),
                candidate_count: None,
                max_output_tokens: None,
                temperature: None,
                top_p: None,
                top_k: None,
                seed: None,
            }),
            system_instruction: Some(Content {
                parts: vec![Parts::Text(
                    "You are a helpful assistant and answer in a friendly tone.".to_string(),
                )],
            }),
        };
        let serialized = serde_json::to_string(&setup).unwrap();
        assert!(serialized.contains("gemini-2.0-flash-live-001"));
        println!("{serialized}");
    }

    #[test]
    fn test_audio_serialization() {
        let audio_data = vec![1, 2, 3, 4, 5];
        let blob = Blob::new(audio_data);
        let audio = RealtimeInput::Audio(RealtimeAudio {
            data: blob,
            mime_type: "audio/pcm;rate=16000".to_string(),
        });
        let serialized = serde_json::to_string(&audio).unwrap();
        println!("Serialized Audio: {}", serialized);
    }

    #[test]
    fn test_server_content_serialization() {
        let data = b"{\n  \"serverContent\": {\n    \"modelTurn\": {\n      \"parts\": [\n        {\n          \"text\": \"\xe6\x88\x91\xe6\x98\xaf\xe4\xb8\x80\xe4\xb8\xaa\xe5\xa4\xa7\xe5\x9e\x8b\xe8\xaf\xad\xe8\xa8\x80\"\n        }\n      ]\n    }\n  }\n}\n";
        let server_content: ServerContent_ =
            serde_json::from_slice(data).expect("Failed to parse server content");
        println!("Server Content: {:?}", server_content);
    }
}
