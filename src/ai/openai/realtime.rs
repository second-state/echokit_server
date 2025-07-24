use serde::{Deserialize, Serialize};

// ============================================================================
// CLIENT EVENTS (发送到服务器的事件)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        session: SessionConfig,
    },

    #[serde(rename = "input_audio_buffer.append")]
    InputAudioBufferAppend {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        audio: String, // Base64 encoded audio
    },

    #[serde(rename = "input_audio_buffer.commit")]
    InputAudioBufferCommit {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },

    #[serde(rename = "input_audio_buffer.clear")]
    InputAudioBufferClear {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },

    #[serde(rename = "conversation.item.create")]
    ConversationItemCreate {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_item_id: Option<String>,
        item: ConversationItem,
    },

    #[serde(rename = "conversation.item.truncate")]
    ConversationItemTruncate {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        item_id: String,
        content_index: u32,
        audio_end_ms: u32,
    },

    #[serde(rename = "conversation.item.delete")]
    ConversationItemDelete {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        item_id: String,
    },

    #[serde(rename = "response.create")]
    ResponseCreate {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<ResponseConfig>,
    },

    #[serde(rename = "response.cancel")]
    ResponseCancel {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
    },
}

// ============================================================================
// SERVER EVENTS (从服务器接收的事件)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "error")]
    Error {
        event_id: String,
        error: ErrorDetails,
    },

    #[serde(rename = "session.created")]
    SessionCreated { event_id: String, session: Session },

    #[serde(rename = "session.updated")]
    SessionUpdated { event_id: String, session: Session },

    #[serde(rename = "conversation.created")]
    ConversationCreated {
        event_id: String,
        conversation: Conversation,
    },

    #[serde(rename = "conversation.item.created")]
    ConversationItemCreated {
        event_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_item_id: Option<String>,
        item: ConversationItem,
    },

    #[serde(rename = "conversation.item.input_audio_transcription.completed")]
    ConversationItemInputAudioTranscriptionCompleted {
        event_id: String,
        item_id: String,
        content_index: u32,
        transcript: String,
    },

    #[serde(rename = "conversation.item.input_audio_transcription.failed")]
    ConversationItemInputAudioTranscriptionFailed {
        event_id: String,
        item_id: String,
        content_index: u32,
        error: ErrorDetails,
    },

    #[serde(rename = "conversation.item.truncated")]
    ConversationItemTruncated {
        event_id: String,
        item_id: String,
        content_index: u32,
        audio_end_ms: u32,
    },

    #[serde(rename = "conversation.item.deleted")]
    ConversationItemDeleted { event_id: String, item_id: String },

    #[serde(rename = "input_audio_buffer.committed")]
    InputAudioBufferCommitted {
        event_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_item_id: Option<String>,
        item_id: String,
    },

    #[serde(rename = "input_audio_buffer.cleared")]
    InputAudioBufferCleared { event_id: String },

    #[serde(rename = "input_audio_buffer.speech_started")]
    InputAudioBufferSpeechStarted {
        event_id: String,
        audio_start_ms: u32,
        item_id: String,
    },

    #[serde(rename = "input_audio_buffer.speech_stopped")]
    InputAudioBufferSpeechStopped {
        event_id: String,
        audio_end_ms: u32,
        item_id: String,
    },

    #[serde(rename = "response.created")]
    ResponseCreated {
        event_id: String,
        response: Response,
    },

    #[serde(rename = "response.done")]
    ResponseDone {
        event_id: String,
        response: Response,
    },

    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded {
        event_id: String,
        response_id: String,
        output_index: u32,
        item: ConversationItem,
    },

    #[serde(rename = "response.output_item.done")]
    ResponseOutputItemDone {
        event_id: String,
        response_id: String,
        output_index: u32,
        item: ConversationItem,
    },

    #[serde(rename = "response.content_part.added")]
    ResponseContentPartAdded {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        part: ContentPart,
    },

    #[serde(rename = "response.content_part.done")]
    ResponseContentPartDone {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        part: ContentPart,
    },

    #[serde(rename = "response.text.delta")]
    ResponseTextDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },

    #[serde(rename = "response.text.done")]
    ResponseTextDone {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        text: String,
    },

    #[serde(rename = "response.audio.delta")]
    ResponseAudioDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String, // Base64 encoded audio
    },

    #[serde(rename = "response.audio.done")]
    ResponseAudioDone {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
    },

    #[serde(rename = "response.audio.transcript.delta")]
    ResponseAudioTranscriptDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },

    #[serde(rename = "response.audio.transcript.done")]
    ResponseAudioTranscriptDone {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        transcript: String,
    },

    #[serde(rename = "response.function_call_arguments.delta")]
    ResponseFunctionCallArgumentsDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },

    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        arguments: String,
    },

    #[serde(rename = "conversation.interrupted")]
    ConversationInterrupted { event_id: String },
}

// ============================================================================
// 支持数据结构
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<InputAudioTranscription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<TurnDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

// ============================================================================
// 枚举定义
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Pcm16,
    G711Ulaw,
    G711Alaw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    #[serde(untagged)]
    Function {
        #[serde(rename = "type")]
        tool_type: String, // "function"
        function: FunctionChoice,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionChoice {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDetectionType {
    ServerVad,
    SemanticVad,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub object: String,
    pub model: String,
    pub modalities: Vec<Modality>,
    pub instructions: String,
    pub voice: String,
    pub input_audio_format: AudioFormat,
    pub output_audio_format: AudioFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<InputAudioTranscription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<TurnDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAudioTranscription {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>, // "whisper-1"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnDetection {
    #[serde(rename = "type")]
    pub turn_type: TurnDetectionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_response: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: ToolType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolType {
    Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String, // "message", "function_call", "function_call_output"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>, // "completed", "in_progress", "incomplete"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>, // "user", "assistant", "system"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ContentPart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "input_audio")]
    InputAudio {
        audio: String, // Base64 encoded
        #[serde(skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
    },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "audio")]
    Audio {
        #[serde(skip_serializing_if = "Option::is_none")]
        audio: Option<String>, // Base64 encoded
        #[serde(skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: String,
    pub status: String, // "in_progress", "completed", "cancelled", "failed", "incomplete"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<ConversationItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token_details: Option<InputTokenDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetails {
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

// ============================================================================
// 辅助方法实现
// ============================================================================

impl ClientEvent {
    /// 创建会话更新事件
    pub fn session_update(session: SessionConfig) -> Self {
        Self::SessionUpdate {
            event_id: None,
            session,
        }
    }

    /// 创建音频缓冲区追加事件
    pub fn input_audio_buffer_append(audio: String) -> Self {
        Self::InputAudioBufferAppend {
            event_id: None,
            audio,
        }
    }

    /// 创建响应生成事件
    pub fn response_create() -> Self {
        Self::ResponseCreate {
            event_id: None,
            response: None,
        }
    }

    /// 设置事件ID
    pub fn with_event_id(mut self, event_id: String) -> Self {
        match &mut self {
            Self::SessionUpdate { event_id: id, .. } => *id = Some(event_id),
            Self::InputAudioBufferAppend { event_id: id, .. } => *id = Some(event_id),
            Self::InputAudioBufferCommit { event_id: id, .. } => *id = Some(event_id),
            Self::InputAudioBufferClear { event_id: id, .. } => *id = Some(event_id),
            Self::ConversationItemCreate { event_id: id, .. } => *id = Some(event_id),
            Self::ConversationItemTruncate { event_id: id, .. } => *id = Some(event_id),
            Self::ConversationItemDelete { event_id: id, .. } => *id = Some(event_id),
            Self::ResponseCreate { event_id: id, .. } => *id = Some(event_id),
            Self::ResponseCancel { event_id: id, .. } => *id = Some(event_id),
        }
        self
    }
}

impl ServerEvent {
    /// 获取事件ID
    pub fn event_id(&self) -> &str {
        match self {
            Self::Error { event_id, .. } => event_id,
            Self::SessionCreated { event_id, .. } => event_id,
            Self::SessionUpdated { event_id, .. } => event_id,
            Self::ConversationCreated { event_id, .. } => event_id,
            Self::ConversationItemCreated { event_id, .. } => event_id,
            Self::ConversationItemInputAudioTranscriptionCompleted { event_id, .. } => event_id,
            Self::ConversationItemInputAudioTranscriptionFailed { event_id, .. } => event_id,
            Self::ConversationItemTruncated { event_id, .. } => event_id,
            Self::ConversationItemDeleted { event_id, .. } => event_id,
            Self::InputAudioBufferCommitted { event_id, .. } => event_id,
            Self::InputAudioBufferCleared { event_id, .. } => event_id,
            Self::InputAudioBufferSpeechStarted { event_id, .. } => event_id,
            Self::InputAudioBufferSpeechStopped { event_id, .. } => event_id,
            Self::ResponseCreated { event_id, .. } => event_id,
            Self::ResponseDone { event_id, .. } => event_id,
            Self::ResponseOutputItemAdded { event_id, .. } => event_id,
            Self::ResponseOutputItemDone { event_id, .. } => event_id,
            Self::ResponseContentPartAdded { event_id, .. } => event_id,
            Self::ResponseContentPartDone { event_id, .. } => event_id,
            Self::ResponseTextDelta { event_id, .. } => event_id,
            Self::ResponseTextDone { event_id, .. } => event_id,
            Self::ResponseAudioDelta { event_id, .. } => event_id,
            Self::ResponseAudioDone { event_id, .. } => event_id,
            Self::ResponseAudioTranscriptDelta { event_id, .. } => event_id,
            Self::ResponseAudioTranscriptDone { event_id, .. } => event_id,
            Self::ResponseFunctionCallArgumentsDelta { event_id, .. } => event_id,
            Self::ResponseFunctionCallArgumentsDone { event_id, .. } => event_id,
            Self::ConversationInterrupted { event_id, .. } => event_id,
        }
    }
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_event_serialization() {
        let event = ClientEvent::session_update(SessionConfig {
            modalities: Some(vec![Modality::Text, Modality::Audio]),
            voice: Some("default".to_string()),
            instructions: Some("You are a helpful assistant.".to_string()),
            input_audio_format: Some(AudioFormat::Pcm16),
            output_audio_format: Some(AudioFormat::Pcm16),
            tool_choice: Some(ToolChoice::Auto),
            ..Default::default()
        });

        let json = serde_json::to_string(&event).unwrap();
        println!("Client event JSON: {}", json);

        let _: ClientEvent = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_server_event_deserialization() {
        let json = r#"{
            "type": "response.text.delta",
            "event_id": "event_123",
            "response_id": "resp_001",
            "item_id": "item_001", 
            "output_index": 0,
            "content_index": 0,
            "delta": "Hello"
        }"#;

        let event: ServerEvent = serde_json::from_str(json).unwrap();
        println!("Server event: {:?}", event);
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            modalities: None,
            instructions: None,
            voice: None,
            input_audio_format: None,
            output_audio_format: None,
            input_audio_transcription: None,
            turn_detection: None,
            tools: None,
            tool_choice: None,
            temperature: None,
            max_output_tokens: None,
        }
    }
}

// ============================================================================
// 枚举便捷方法
// ============================================================================

impl Modality {
    pub fn text() -> Self {
        Self::Text
    }

    pub fn audio() -> Self {
        Self::Audio
    }

    pub fn all() -> Vec<Self> {
        vec![Self::Text, Self::Audio]
    }
}

impl AudioFormat {
    pub fn pcm16() -> Self {
        Self::Pcm16
    }

    pub fn all() -> Vec<Self> {
        vec![Self::Pcm16, Self::G711Ulaw, Self::G711Alaw]
    }
}

impl ToolChoice {
    pub fn auto() -> Self {
        Self::Auto
    }

    pub fn none() -> Self {
        Self::None
    }

    pub fn required() -> Self {
        Self::Required
    }

    pub fn function(name: String) -> Self {
        Self::Function {
            tool_type: "function".to_string(),
            function: FunctionChoice { name },
        }
    }
}

impl TurnDetectionType {
    pub fn server_vad() -> Self {
        Self::ServerVad
    }

    pub fn semantic_vad() -> Self {
        Self::SemanticVad
    }

    pub fn none() -> Self {
        Self::None
    }
}

impl TurnDetection {
    pub fn server_vad() -> Self {
        Self {
            turn_type: TurnDetectionType::ServerVad,
            threshold: None,
            prefix_padding_ms: None,
            silence_duration_ms: None,
            create_response: Some(true),
        }
    }

    pub fn semantic_vad() -> Self {
        Self {
            turn_type: TurnDetectionType::SemanticVad,
            threshold: None,
            prefix_padding_ms: None,
            silence_duration_ms: None,
            create_response: Some(true),
        }
    }

    pub fn none() -> Self {
        Self {
            turn_type: TurnDetectionType::None,
            threshold: None,
            prefix_padding_ms: None,
            silence_duration_ms: None,
            create_response: None,
        }
    }
}
