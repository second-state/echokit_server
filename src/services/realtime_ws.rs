#![allow(deprecated)]

use axum::{
    extract::{Extension, WebSocketUpgrade, ws::WebSocket},
    response::IntoResponse,
};
use base64::Engine;
use bytes::{BufMut, BytesMut};
use futures_util::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
use std::{sync::Arc, vec};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ai::{ChatSession, bailian::cosyvoice, elevenlabs, openai::realtime::*, vad::VadSession},
    config::*,
};

fn encode_base64(data: &[u8]) -> String {
    base64::prelude::BASE64_STANDARD.encode(data)
}

fn decode_base64(data: &str) -> anyhow::Result<Vec<u8>> {
    base64::prelude::BASE64_STANDARD
        .decode(data)
        .map_err(|e| anyhow::anyhow!("Base64 decode error: {}", e))
}

pub struct RealtimeSession {
    pub client: reqwest::Client,
    pub chat_session: ChatSession,
    pub id: String,
    pub config: SessionConfig,
    // pub conversation: Vec<ConversationItem>,
    pub input_audio_buffer: BytesMut,
    pub triggered: bool,
    pub is_generating: bool,
    pub vad_session: Option<VadSession>,
}

impl RealtimeSession {
    pub fn new(chat_session: ChatSession, vad_session: Option<VadSession>) -> Self {
        Self {
            client: reqwest::Client::new(),
            chat_session,
            id: Uuid::new_v4().to_string(),
            config: SessionConfig::default(),
            // conversation: Vec::new(),
            input_audio_buffer: BytesMut::new(),
            triggered: false,
            is_generating: false,
            vad_session,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StableRealtimeConfig {
    pub llm: ChatConfig,
    pub tts: TTSConfig,
    pub asr: WhisperASRConfig,
}

enum RealtimeEvent {
    ClientEvent(ClientEvent),
}

pub async fn ws_handler(
    Extension(config): Extension<Arc<StableRealtimeConfig>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    log::info!("WebSocket connection requested");
    ws.on_upgrade(|socket| handle_socket(config, socket))
}

async fn handle_socket(config: Arc<StableRealtimeConfig>, socket: WebSocket) {
    log::info!("Handling realtime WebSocket connection");
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerEvent>(1024);

    let mut chat_session = ChatSession::new(
        config.llm.llm_chat_url.clone(),
        config.llm.api_key.clone().unwrap_or_default(),
        config.llm.model.clone(),
        None,
        config.llm.history,
        crate::ai::openai::tool::ToolSet::default(),
    );
    let parts = config.llm.prompts().await;
    chat_session.system_prompts = parts.sys_prompts;
    chat_session.messages = parts.dynamic_prompts;

    // Initialize built-in silero VAD session
    let device = burn::backend::ndarray::NdArrayDevice::default();
    let vad_session = match silero_vad_burn::SileroVAD6Model::new(&device) {
        Ok(vad_model) => {
            match crate::ai::vad::VadSession::new(&config.asr.vad, Box::new(vad_model), device) {
                Ok(session) => {
                    log::info!("Initialized built-in silero VAD session");
                    Some(session)
                }
                Err(e) => {
                    log::error!("Failed to create VAD session: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            log::error!(
                "Failed to load silero VAD model: {}. \
                This may be due to missing model files or insufficient memory.",
                e
            );
            None
        }
    };

    // 创建新的 Realtime 会话
    let has_vad = vad_session.is_some();
    let mut session = RealtimeSession::new(chat_session, vad_session);

    let turn_detection = if has_vad {
        TurnDetection::server_vad()
    } else {
        TurnDetection::none()
    };

    log::debug!(
        "Starting realtime session with ID: {}, turn detection: {:?}",
        session.id,
        turn_detection
    );

    let tts_voice = match &config.tts {
        TTSConfig::GSV(tts) => tts.speaker.clone(),
        TTSConfig::Fish(fish) => fish.speaker.clone(),
        TTSConfig::Openai(openai) => openai.voice.clone(),
        TTSConfig::Groq(groq) => groq.voice.clone(),
        TTSConfig::StreamGSV(stream_tts) => stream_tts.speaker.clone(),
        TTSConfig::CosyVoice(cosyvoice) => {
            cosyvoice.speaker.clone().unwrap_or("default".to_string())
        }
        TTSConfig::Elevenlabs(elevenlabs_tts) => elevenlabs_tts.voice.clone(),
    };

    session.config.turn_detection = Some(turn_detection.clone());
    session.config.input_audio_format = Some(AudioFormat::Pcm16);
    session.config.output_audio_format = Some(AudioFormat::Pcm16);
    session.config.modalities = Some(vec![Modality::Text, Modality::Audio]);
    session.config.instructions = config
        .llm
        .sys_prompts
        .first()
        .map(|prompt| prompt.message.clone());
    session.config.voice = Some(tts_voice.clone());

    // 发送初始 session.created 事件
    let session_created = ServerEvent::SessionCreated {
        event_id: Uuid::new_v4().to_string(),
        session: Session {
            id: session.id.clone(),
            object: "realtime.session".to_string(),
            model: "gpt-4o-realtime-preview".to_string(),
            modalities: vec![Modality::Text, Modality::Audio],
            instructions: session
                .config
                .instructions
                .clone()
                .unwrap_or_else(|| "You are a helpful assistant.".to_string()),
            voice: tts_voice,
            input_audio_format: AudioFormat::Pcm16,
            output_audio_format: AudioFormat::Pcm16,
            input_audio_transcription: None,
            turn_detection: Some(turn_detection),
            tools: None,
            tool_choice: Some(ToolChoice::Auto),
            temperature: Some(0.8),
            max_output_tokens: None,
        },
    };

    if let Ok(json) = serde_json::to_string(&session_created) {
        if sender
            .send(axum::extract::ws::Message::Text(json.into()))
            .await
            .is_err()
        {
            return;
        }
    }

    // 发送 conversation.created 事件
    let conversation_created = ServerEvent::ConversationCreated {
        event_id: Uuid::new_v4().to_string(),
        conversation: Conversation {
            id: Uuid::new_v4().to_string(),
            object: "realtime.conversation".to_string(),
        },
    };

    if let Ok(json) = serde_json::to_string(&conversation_created) {
        if sender
            .send(axum::extract::ws::Message::Text(json.into()))
            .await
            .is_err()
        {
            return;
        }
    }

    // 处理从服务器发送到客户端的消息
    let send_task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender
                    .send(axum::extract::ws::Message::Text(json.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    async fn recv_client_event(socket: &mut SplitStream<WebSocket>) -> Option<ClientEvent> {
        while let Some(msg) = socket.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    match serde_json::from_str::<ClientEvent>(&text) {
                        Ok(event) => return Some(event),
                        Err(e) => {
                            log::error!("Failed to parse client event: {}", e);
                            return None;
                        }
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => return None,
                Ok(_) => continue, // Ignore other message types
                Err(e) => {
                    log::error!("WebSocket error: {}", e);
                    return None;
                }
            }
        }
        None
    }

    while let Some(event) = recv_client_event(&mut receiver)
        .await
        .map(RealtimeEvent::ClientEvent)
    {
        if let Err(e) = handle_client_message(
            event,
            &mut session,
            &tx,
            &config.llm,
            &config.tts,
            &config.asr,
        )
        .await
        {
            log::error!("Error handling client message: {}", e);
        }
    }

    // 等待发送任务完成
    drop(tx);
    if let Err(e) = send_task.await {
        log::error!("Send task error: {}", e);
    }
}

async fn handle_client_message(
    client_event: RealtimeEvent,
    session: &mut RealtimeSession,
    tx: &mpsc::Sender<ServerEvent>,
    llm: &ChatConfig,
    tts: &TTSConfig,
    asr: &WhisperASRConfig,
) -> anyhow::Result<()> {
    match client_event {
        RealtimeEvent::ClientEvent(client_event) => {
            match client_event {
                ClientEvent::SessionUpdate {
                    event_id: _,
                    session: config,
                } => {
                    if let Some(ref input_format) = config.input_audio_format {
                        if *input_format != AudioFormat::Pcm16 {
                            let error_event = ServerEvent::Error {
                                event_id: Uuid::new_v4().to_string(),
                                error: ErrorDetails {
                                    error_type: "invalid_request_error".to_string(),
                                    code: Some("unsupported_audio_format".to_string()),
                                    message: "Only PCM16 input audio format is supported"
                                        .to_string(),
                                    param: Some("input_audio_format".to_string()),
                                    event_id: None,
                                },
                            };
                            let _ = tx.send(error_event).await;
                            return Ok(());
                        }
                    }

                    if let Some(ref output_format) = config.output_audio_format {
                        if *output_format != AudioFormat::Pcm16 {
                            let error_event = ServerEvent::Error {
                                event_id: Uuid::new_v4().to_string(),
                                error: ErrorDetails {
                                    error_type: "invalid_request_error".to_string(),
                                    code: Some("unsupported_audio_format".to_string()),
                                    message: "Only PCM16 output audio format is supported"
                                        .to_string(),
                                    param: Some("output_audio_format".to_string()),
                                    event_id: None,
                                },
                            };
                            let _ = tx.send(error_event).await;
                            return Ok(());
                        }
                    }

                    if let Some(ref turn_detection) = config.turn_detection {
                        if turn_detection.turn_type == TurnDetectionType::SemanticVad {
                            let error_event = ServerEvent::Error {
                                event_id: Uuid::new_v4().to_string(),
                                error: ErrorDetails {
                                    error_type: "invalid_request_error".to_string(),
                                    code: Some("unsupported_turn_detection".to_string()),
                                    message: "Semantic VAD turn detection is not supported"
                                        .to_string(),
                                    param: Some("turn_detection.type".to_string()),
                                    event_id: None,
                                },
                            };
                            let _ = tx.send(error_event).await;
                            return Ok(());
                        }
                        if turn_detection.turn_type == TurnDetectionType::ServerVad
                            && session.vad_session.is_none()
                        {
                            let error_event = ServerEvent::Error {
                                event_id: Uuid::new_v4().to_string(),
                                error: ErrorDetails {
                                    error_type: "invalid_request_error".to_string(),
                                    code: Some("vad_not_available".to_string()),
                                    message: "VAD session is not available".to_string(),
                                    param: Some("turn_detection.type".to_string()),
                                    event_id: None,
                                },
                            };
                            let _ = tx.send(error_event).await;
                            return Ok(());
                        }
                    }

                    session.config.merge(config);
                    log::debug!("Session updated: config = {:?}", session.config);

                    // 发送 session.updated 确认
                    let updated_session = Session {
                        id: session.id.clone(),
                        object: "realtime.session".to_string(),
                        model: llm.model.clone(),
                        modalities: session
                            .config
                            .modalities
                            .clone()
                            .unwrap_or_else(|| vec![Modality::Text, Modality::Audio]),
                        instructions: session
                            .config
                            .instructions
                            .clone()
                            .unwrap_or_else(|| "You are a helpful assistant.".to_string()),
                        voice: session
                            .config
                            .voice
                            .clone()
                            .unwrap_or_else(|| "default".to_string()),
                        input_audio_format: session
                            .config
                            .input_audio_format
                            .clone()
                            .unwrap_or(AudioFormat::Pcm16),
                        output_audio_format: session
                            .config
                            .output_audio_format
                            .clone()
                            .unwrap_or(AudioFormat::Pcm16),
                        input_audio_transcription: session.config.input_audio_transcription.clone(),
                        turn_detection: session.config.turn_detection.clone(),
                        tools: session.config.tools.clone(),
                        tool_choice: session.config.tool_choice.clone(),
                        temperature: session.config.temperature,
                        max_output_tokens: session.config.max_output_tokens,
                    };

                    let event = ServerEvent::SessionUpdated {
                        event_id: Uuid::new_v4().to_string(),
                        session: updated_session,
                    };
                    let _ = tx.send(event).await;
                }

                ClientEvent::InputAudioBufferAppend { event_id: _, audio } => {
                    let audio_data = decode_base64(&audio)?;
                    let server_vad = session
                        .config
                        .turn_detection
                        .as_ref()
                        .map(|t| t.turn_type == TurnDetectionType::ServerVad)
                        .unwrap_or_default()
                        && session.vad_session.is_some();

                    log::debug!(
                        "Server VAD status: {} {:?}",
                        session.vad_session.is_some(),
                        session.config.turn_detection
                    );

                    if !server_vad || session.triggered {
                        log::debug!(
                            "Appending audio chunk to input buffer, length: {}, server VAD: {}",
                            audio_data.len(),
                            server_vad
                        );
                        session.input_audio_buffer.extend_from_slice(&audio_data);
                    } else {
                        log::debug!(
                            "Audio chunk received but not triggered, length: {}, server VAD: {}",
                            audio_data.len(),
                            server_vad
                        );
                        let prefix_padding_ms = session
                            .config
                            .turn_detection
                            .as_ref()
                            .map(|td| td.prefix_padding_ms)
                            .flatten()
                            .unwrap_or(300);
                        let prefix_padding_samples_len =
                            2 * prefix_padding_ms as usize * 24000 / 1000;
                        if session.input_audio_buffer.len() + audio_data.len()
                            < prefix_padding_samples_len
                        {
                            session.input_audio_buffer.extend_from_slice(&audio_data);
                        } else {
                            session.input_audio_buffer.clear();
                            session.input_audio_buffer.extend_from_slice(&audio_data);
                        }
                    }

                    // Process audio through built-in silero VAD
                    if server_vad {
                        if let Some(vad_session) = session.vad_session.as_mut() {
                            // Convert 24kHz PCM16 to 16kHz f32 for VAD
                            let samples_24k: Vec<f32> = audio_data
                                .chunks_exact(2)
                                .map(|chunk| {
                                    i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32
                                })
                                .collect();
                            let samples_16k =
                                wav_io::resample::linear(samples_24k, 1, 24000, 16000);

                            // Process through VAD in chunks
                            let chunk_size = VadSession::vad_chunk_size();
                            let mut speech_detected = false;
                            for chunk in samples_16k.chunks(chunk_size) {
                                if let Ok(is_speech) = vad_session.detect(chunk) {
                                    if is_speech {
                                        speech_detected = true;
                                    } else if session.triggered {
                                        // Speech ended - trigger commit
                                        log::info!("VAD detected speech end, triggering commit");
                                        if handle_audio_buffer_commit(session, tx, None, asr)
                                            .await?
                                        {
                                            generate_response(session, tx, tts).await?;
                                        }
                                        session.triggered = false;
                                        if let Some(vs) = session.vad_session.as_mut() {
                                            vs.reset_state();
                                        }
                                        break;
                                    }
                                }
                            }

                            if speech_detected && !session.triggered {
                                log::info!("VAD detected speech start");
                                session.triggered = true;
                                // Send speech started event
                                let event = ServerEvent::InputAudioBufferSpeechStarted {
                                    event_id: Uuid::new_v4().to_string(),
                                    audio_start_ms: 0,
                                    item_id: Uuid::new_v4().to_string(),
                                };
                                let _ = tx.send(event).await;
                            }
                        }
                    }
                }

                ClientEvent::InputAudioBufferCommit { event_id: _ } => {
                    if handle_audio_buffer_commit(session, tx, None, asr).await? {
                        log::debug!("Audio buffer committed, generating response");
                        generate_response(session, tx, tts).await?;
                    }
                }

                ClientEvent::InputAudioBufferClear { event_id: _ } => {
                    session.input_audio_buffer.clear();

                    let event = ServerEvent::InputAudioBufferCleared {
                        event_id: Uuid::new_v4().to_string(),
                    };
                    let _ = tx.send(event).await;
                }

                ClientEvent::ConversationItemCreate {
                    event_id: _,
                    previous_item_id,
                    item,
                } => {
                    match item.item_type.as_str() {
                        "message" => match item.role.as_deref() {
                            Some("user") => {
                                if let Some(content) = &item.content {
                                    let text = extract_text_from_content(content);
                                    session.chat_session.add_user_message(text);
                                }
                            }
                            Some("assistant") => {
                                if let Some(content) = &item.content {
                                    let text = extract_text_from_content(content);
                                    session.chat_session.add_assistant_message(text);
                                }
                            }
                            Some("system") => {
                                if let Some(content) = &item.content {
                                    let text = extract_text_from_content(content);
                                    session
                                        .chat_session
                                        .system_prompts
                                        .first_mut()
                                        .map(|prompt| prompt.message = text);
                                }
                            }
                            _ => {
                                log::warn!(
                                    "Unsupported role in conversation item: {:?}",
                                    item.role
                                );
                            }
                        },
                        "function_call" => {
                            if let Some(arguments) = &item.arguments {
                                session
                                    .chat_session
                                    .messages
                                    .push_back(crate::ai::llm::Content {
                                        role: crate::ai::llm::Role::Assistant,
                                        message: String::new(),
                                        tool_calls: Some(vec![crate::ai::llm::ToolCall {
                                            id: item.id.clone().unwrap_or_default(),
                                            type_: "function".to_string(),
                                            function: crate::ai::llm::ToolFunction {
                                                name: item.name.clone().unwrap_or_default(),
                                                arguments: arguments.clone(),
                                            },
                                        }]),
                                        tool_call_id: None,
                                    });
                            }
                        }
                        "function_call_output" => {
                            if let Some(output) = &item.output {
                                session
                                    .chat_session
                                    .messages
                                    .push_back(crate::ai::llm::Content {
                                        role: crate::ai::llm::Role::Tool,
                                        message: output.clone(),
                                        tool_calls: None,
                                        tool_call_id: item.id.clone(),
                                    });
                            }
                        }
                        _ => {
                            log::warn!("Unsupported item type: {}", item.item_type);
                        }
                    }

                    let event = ServerEvent::ConversationItemCreated {
                        event_id: Uuid::new_v4().to_string(),
                        previous_item_id,
                        item,
                    };
                    let _ = tx.send(event).await;
                }

                ClientEvent::ResponseCreate {
                    event_id: _,
                    response: _,
                } => {
                    if session.is_generating {
                        let error_event = ServerEvent::Error {
                            event_id: Uuid::new_v4().to_string(),
                            error: ErrorDetails {
                                error_type: "invalid_request_error".to_string(),
                                code: Some("response_in_progress".to_string()),
                                message: "A response is already being generated".to_string(),
                                param: None,
                                event_id: None,
                            },
                        };
                        let _ = tx.send(error_event).await;
                        return Ok(());
                    }
                    log::debug!("Generating response for session: {}", session.id);
                    generate_response(session, tx, tts).await?;
                }

                ClientEvent::ResponseCancel { event_id: _ } => {
                    session.is_generating = false;

                    let event = ServerEvent::ConversationInterrupted {
                        event_id: Uuid::new_v4().to_string(),
                    };
                    let _ = tx.send(event).await;
                }

                _ => {
                    log::warn!("Unhandled client event: {:?}", client_event);
                }
            }
        }
    }

    Ok(())
}

async fn handle_audio_buffer_commit(
    session: &mut RealtimeSession,
    tx: &mpsc::Sender<ServerEvent>,
    item_id: Option<String>,
    config: &WhisperASRConfig,
) -> anyhow::Result<bool> {
    let audio_data = &session.input_audio_buffer;

    let item_id = item_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    if audio_data.is_empty() {
        return Ok(false);
    }

    // 24k pcm to wav
    let wav_audio = crate::util::pcm_to_wav(&audio_data, crate::util::WavConfig::default());

    // 发送 input_audio_buffer.committed 事件
    let committed_event = ServerEvent::InputAudioBufferCommitted {
        event_id: Uuid::new_v4().to_string(),
        previous_item_id: None,
        item_id: item_id.clone(),
    };
    let _ = tx.send(committed_event).await;

    // Check for speech using built-in silero VAD
    if let Some(vad_session) = session.vad_session.as_mut() {
        // Convert 24kHz PCM16 to 16kHz f32 for VAD
        let samples_24k: Vec<f32> = audio_data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32)
            .collect();
        let samples_16k = wav_io::resample::linear(samples_24k, 1, 24000, 16000);

        // Process through VAD to check if there's any speech
        let chunk_size = VadSession::vad_chunk_size();
        let mut has_speech = false;
        vad_session.reset_state();
        for chunk in samples_16k.chunks(chunk_size) {
            if let Ok(is_speech) = vad_session.detect(chunk) {
                if is_speech {
                    has_speech = true;
                    break;
                }
            }
        }

        if !has_speech {
            log::debug!("No speech detected in audio buffer, skipping ASR");
            let transcription_completed =
                ServerEvent::ConversationItemInputAudioTranscriptionCompleted {
                    event_id: Uuid::new_v4().to_string(),
                    item_id: item_id.clone(),
                    content_index: 0,
                    transcript: String::new(),
                };
            let _ = tx.send(transcription_completed).await;
            return Ok(false);
        }
    }

    // 执行 ASR
    let text_results = crate::ai::asr(
        &session.client,
        &config.url,
        &config.api_key,
        &config.model,
        &config.lang,
        &config.prompt,
        wav_audio.clone(),
    )
    .await?;
    let transcript = text_results.join("\n");

    // 创建用户消息项
    let user_item = ConversationItem {
        id: Some(item_id.clone()),
        object: Some("realtime.item".to_string()),
        item_type: "message".to_string(),
        status: Some("completed".to_string()),
        role: Some("user".to_string()),
        content: Some(vec![ContentPart::InputAudio {
            audio: encode_base64(&audio_data),
            transcript: Some(transcript.clone()),
        }]),
        call_id: None,
        name: None,
        arguments: None,
        output: None,
    };

    // 添加到对话历史
    session.chat_session.add_user_message(transcript.clone());

    // 发送 conversation.item.created 事件
    let item_created = ServerEvent::ConversationItemCreated {
        event_id: Uuid::new_v4().to_string(),
        previous_item_id: None,
        item: user_item,
    };
    let _ = tx.send(item_created).await;

    // 发送转录完成事件
    let transcription_completed = ServerEvent::ConversationItemInputAudioTranscriptionCompleted {
        event_id: Uuid::new_v4().to_string(),
        item_id: item_id.clone(),
        content_index: 0,
        transcript,
    };
    let _ = tx.send(transcription_completed).await;

    session.input_audio_buffer.clear();

    // 如果启用自动响应生成，开始生成响应
    let should_generate_response = session
        .config
        .turn_detection
        .as_ref()
        .and_then(|td| td.create_response)
        .unwrap_or(true);

    Ok(should_generate_response)
}

async fn generate_response(
    session: &mut RealtimeSession,
    tx: &mpsc::Sender<ServerEvent>,
    tts_config: &TTSConfig,
) -> anyhow::Result<()> {
    if let Some(last_message) = session.chat_session.messages.back() {
        if last_message.role == crate::ai::llm::Role::Assistant {
            log::debug!("Skipping response generation, last message is from assistant");
            return Ok(());
        }
    }
    // 检查是否需要生成音频
    let should_generate_audio = session
        .config
        .modalities
        .as_ref()
        .map(|m| m.contains(&Modality::Audio))
        .unwrap_or(false);

    if session.is_generating {
        return Ok(());
    }
    session.is_generating = true;

    let response_id = Uuid::new_v4().to_string();

    // 发送 response.created 事件
    let response_created = ServerEvent::ResponseCreated {
        event_id: Uuid::new_v4().to_string(),
        response: Response {
            id: response_id.clone(),
            object: "realtime.response".to_string(),
            status: "in_progress".to_string(),
            status_details: None,
            output: None,
            usage: None,
        },
    };
    let _ = tx.send(response_created).await;

    let item_id = Uuid::new_v4().to_string();

    // 发送 response.output_item.added 事件
    let assistant_item = ConversationItem {
        id: Some(item_id.clone()),
        object: Some("realtime.item".to_string()),
        item_type: "message".to_string(),
        status: Some("in_progress".to_string()),
        role: Some("assistant".to_string()),
        content: Some(vec![ContentPart::Text {
            text: String::new(),
        }]),
        call_id: None,
        name: None,
        arguments: None,
        output: None,
    };

    let output_item_added = ServerEvent::ResponseOutputItemAdded {
        event_id: Uuid::new_v4().to_string(),
        response_id: response_id.clone(),
        output_index: 0,
        item: assistant_item.clone(),
    };
    let _ = tx.send(output_item_added).await;

    // 发送 response.content_part.added 事件
    let content_part_added = ServerEvent::ResponseContentPartAdded {
        event_id: Uuid::new_v4().to_string(),
        response_id: response_id.clone(),
        item_id: item_id.clone(),
        output_index: 0,
        content_index: 0,
        part: ContentPart::Text {
            text: String::new(),
        },
    };
    let _ = tx.send(content_part_added).await;

    if should_generate_audio {
        // 发送 response.content_part.added 事件用于音频
        let audio_part_added = ServerEvent::ResponseContentPartAdded {
            event_id: Uuid::new_v4().to_string(),
            response_id: response_id.clone(),
            item_id: item_id.clone(),
            output_index: 0,
            content_index: 1,
            part: ContentPart::Audio {
                audio: None,
                transcript: None,
            },
        };
        let _ = tx.send(audio_part_added).await;
    }

    // 调用 LLM 生成文本响应
    let llm_response = {
        let mut response = session.chat_session.complete().await?;
        let mut full_response = String::new();

        loop {
            match response.next_chunk().await {
                Ok(crate::ai::StableLLMResponseChunk::Text(chunk)) => {
                    full_response.push_str(&chunk);
                    // 发送 response.text.delta 事件
                    let text_delta = ServerEvent::ResponseTextDelta {
                        event_id: Uuid::new_v4().to_string(),
                        response_id: response_id.clone(),
                        item_id: Uuid::new_v4().to_string(), // 使用新的 UUID
                        output_index: 0,
                        content_index: 0,
                        delta: chunk.clone(),
                    };
                    let _ = tx.send(text_delta).await;
                    if should_generate_audio {
                        // 发送 TTS 事件
                        if let Err(e) = tts_and_send(
                            tx,
                            tts_config,
                            response_id.clone(),
                            Some(item_id.clone()),
                            chunk.clone(),
                        )
                        .await
                        {
                            log::error!("Error during TTS: {}", e);
                        }
                    }
                }
                Ok(crate::ai::StableLLMResponseChunk::Stop) => break,
                Ok(crate::ai::StableLLMResponseChunk::Functions(_)) => {
                    // 跳过函数调用
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }

        full_response
    };

    // send response.text.done event
    let text_done = ServerEvent::ResponseTextDone {
        event_id: Uuid::new_v4().to_string(),
        response_id: response_id.clone(),
        item_id: Uuid::new_v4().to_string(), // 使用新的 UUID
        output_index: 0,
        content_index: 0,
        text: llm_response.clone(),
    };
    let _ = tx.send(text_done).await;

    // send response.part.done event done
    let text_part_done = ServerEvent::ResponseContentPartDone {
        event_id: Uuid::new_v4().to_string(),
        response_id: response_id.clone(),
        item_id: item_id.clone(),
        output_index: 0,
        content_index: 0,
        part: ContentPart::Text {
            text: llm_response.clone(),
        },
    };
    let _ = tx.send(text_part_done).await;

    if should_generate_audio {
        let audio_done = ServerEvent::ResponseAudioDone {
            event_id: Uuid::new_v4().to_string(),
            response_id: response_id.clone(),
            item_id: item_id.clone(),
            output_index: 0,
            content_index: 1,
        };
        let _ = tx.send(audio_done).await;

        let audio_part_done = ServerEvent::ResponseContentPartDone {
            event_id: Uuid::new_v4().to_string(),
            response_id: response_id.clone(),
            item_id: item_id.clone(),
            output_index: 0,
            content_index: 1,
            part: ContentPart::Audio {
                audio: None,
                transcript: Some(llm_response.clone()),
            },
        };
        let _ = tx.send(audio_part_done).await;
    }

    // 更新对话历史
    let final_item = ConversationItem {
        id: Some(item_id.clone()),
        object: Some("realtime.item".to_string()),
        item_type: "message".to_string(),
        status: Some("completed".to_string()),
        role: Some("assistant".to_string()),
        content: Some(if should_generate_audio {
            vec![
                ContentPart::Text {
                    text: llm_response.clone(),
                },
                ContentPart::Audio {
                    audio: None,
                    transcript: Some(llm_response.clone()),
                },
            ]
        } else {
            vec![ContentPart::Text {
                text: llm_response.clone(),
            }]
        }),
        call_id: None,
        name: None,
        arguments: None,
        output: None,
    };

    session
        .chat_session
        .add_assistant_message(llm_response.clone());
    session.is_generating = false;

    // 发送 response.output_item.done 事件
    let output_item_done = ServerEvent::ResponseOutputItemDone {
        event_id: Uuid::new_v4().to_string(),
        response_id: response_id.clone(),
        output_index: 0,
        item: final_item,
    };
    let _ = tx.send(output_item_done).await;

    // 发送 response.done 事件
    let response_done = ServerEvent::ResponseDone {
        event_id: Uuid::new_v4().to_string(),
        response: Response {
            id: response_id,
            object: "realtime.response".to_string(),
            status: "completed".to_string(),
            status_details: None,
            output: None,
            usage: None,
        },
    };
    let _ = tx.send(response_done).await;

    Ok(())
}

fn extract_text_from_content(content: &[ContentPart]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(text.clone()),
            ContentPart::InputText { text } => Some(text.clone()),
            ContentPart::InputAudio { transcript, .. } => transcript.clone(),
            ContentPart::Audio { transcript, .. } => transcript.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn send_wav(
    tx: &mpsc::Sender<ServerEvent>,
    response_id: String,
    item_id: Option<String>,
    text: String,
    wav_data: bytes::Bytes,
) -> anyhow::Result<std::time::Duration> {
    let mut reader = wav_io::reader::Reader::from_vec(wav_data.into())
        .map_err(|e| anyhow::anyhow!("wav_io reader error: {e}"))?;

    let header = reader.read_header()?;
    let mut samples = reader.get_samples_f32()?;
    let duration_sec = samples.len() as f32 / (header.sample_rate as f32 * header.channels as f32);
    let duration_sec = std::time::Duration::from_secs_f32(duration_sec);

    let out_hz = 24000;

    if header.sample_rate != out_hz {
        // resample to 24000
        log::info!("resampling from {} to 24000", header.sample_rate);
        samples = wav_io::resample::linear(samples, header.channels, header.sample_rate, out_hz);
    }
    let audio_24k = wav_io::convert_samples_f32_to_i16(&samples);

    log::info!("llm chunk:{:?}", text);

    for chunk in audio_24k.chunks(5 * out_hz as usize / 10) {
        let buff = if cfg!(target_endian = "big") {
            let mut buff = Vec::with_capacity(chunk.len() * 2);
            for i in chunk {
                buff.extend_from_slice(&i.to_le_bytes());
            }
            buff
        } else {
            let chunk_bytes =
                unsafe { std::slice::from_raw_parts(chunk.as_ptr() as *const u8, chunk.len() * 2) };
            chunk_bytes.to_vec()
        };

        //send to server
        tx.send(ServerEvent::ResponseAudioDelta {
            event_id: Uuid::new_v4().to_string(),
            response_id: response_id.clone(),
            item_id: item_id.clone().unwrap_or_default(),
            output_index: 0,
            content_index: 1,
            delta: encode_base64(&buff),
        })
        .await
        .map_err(|_| anyhow::anyhow!("send audio error"))?;
    }

    Ok(duration_sec)
}

async fn send_stream_chunk(
    tx: &mpsc::Sender<ServerEvent>,
    response_id: String,
    item_id: Option<String>,
    text: String,
    resp: reqwest::Response,
) -> anyhow::Result<()> {
    log::info!("llm chunk:{:?}", text);

    let in_hz = 24000;
    let mut stream = resp.bytes_stream();
    let mut rest = bytes::BytesMut::new();
    let read_chunk_size = 2 * 5 * in_hz as usize / 10; // 0.5 seconds of audio at 24kHz

    'next_chunk: while let Some(item) = stream.next().await {
        // 小端字节序
        let mut chunk = item?;

        log::trace!("Received audio chunk of size: {}", chunk.len());

        if rest.len() > 0 {
            log::trace!("chunk size: {}, rest size: {}", chunk.len(), rest.len());
            if chunk.len() + rest.len() > read_chunk_size {
                let n = read_chunk_size - rest.len();
                rest.put(chunk.slice(..n));
                debug_assert_eq!(rest.len(), read_chunk_size);
                let audio_16k = rest.to_vec();
                log::trace!("Sending audio chunk of size: {}", audio_16k.len());

                // send server audio delta
                tx.send(ServerEvent::ResponseAudioDelta {
                    event_id: Uuid::new_v4().to_string(),
                    response_id: response_id.clone(),
                    item_id: item_id.clone().unwrap_or_default(),
                    output_index: 0,
                    content_index: 1,
                    delta: encode_base64(&audio_16k),
                })
                .await
                .map_err(|_| anyhow::anyhow!("send audio error"))?;

                rest.clear();
                chunk = chunk.slice(n..);
            } else {
                rest.extend_from_slice(&chunk);
                continue 'next_chunk;
            }
        }

        for samples_24k_data in chunk.chunks(read_chunk_size) {
            if samples_24k_data.len() < read_chunk_size {
                log::trace!("Received audio chunk with odd length, skipping");
                rest.extend_from_slice(&samples_24k_data);
                continue 'next_chunk;
            }
            let audio_24k = samples_24k_data.to_vec();
            log::trace!("Sending audio chunk of size: {}", audio_24k.len());
            // send server audio delta
            tx.send(ServerEvent::ResponseAudioDelta {
                event_id: Uuid::new_v4().to_string(),
                response_id: response_id.clone(),
                item_id: item_id.clone().unwrap_or_default(),
                output_index: 0,
                content_index: 1,
                delta: encode_base64(&audio_24k),
            })
            .await
            .map_err(|_| anyhow::anyhow!("send audio error"))?;
        }
    }

    if rest.len() > 0 {
        let audio_24k = rest.to_vec();
        log::trace!("Sending audio chunk of size: {}", audio_24k.len());
        // send server audio delta
        tx.send(ServerEvent::ResponseAudioDelta {
            event_id: Uuid::new_v4().to_string(),
            response_id: response_id.clone(),
            item_id: item_id.clone().unwrap_or_default(),
            output_index: 0,
            content_index: 1,
            delta: encode_base64(&audio_24k),
        })
        .await
        .map_err(|_| anyhow::anyhow!("send audio error"))?;
    }

    Ok(())
}

async fn tts_and_send(
    tx: &mpsc::Sender<ServerEvent>,
    tts_config: &TTSConfig,
    response_id: String,
    item_id: Option<String>,
    text: String,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    match tts_config {
        crate::config::TTSConfig::GSV(tts) => {
            let wav_data = crate::ai::tts::gsv(
                &client,
                &tts.url,
                &tts.speaker,
                &text,
                Some(24000),
                tts.text_optimization.as_ref(),
            )
            .await?;
            let duration_sec = send_wav(tx, response_id, item_id, text, wav_data).await?;
            log::info!("Stable TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::Fish(fish) => {
            let wav_data =
                crate::ai::tts::fish_tts(&fish.url, &fish.api_key, &fish.speaker, &text).await?;
            let duration_sec = send_wav(tx, response_id, item_id, text, wav_data).await?;
            log::info!("Fish TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::Openai(openai) => {
            let wav_data = crate::ai::tts::openai_tts(
                &client,
                &openai.url,
                &openai.model,
                &openai.api_key,
                &openai.voice,
                &text,
            )
            .await?;
            let duration_sec = send_wav(tx, response_id, item_id, text, wav_data).await?;
            log::info!("OpenAI TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::Groq(groq) => {
            let wav_data = crate::ai::tts::groq(
                &client,
                &groq.url,
                &groq.model,
                &groq.api_key,
                &groq.voice,
                &text,
            )
            .await?;
            let duration_sec = send_wav(tx, response_id, item_id, text, wav_data).await?;
            log::info!("Groq TTS duration: {:?}", duration_sec);
            Ok(())
        }
        crate::config::TTSConfig::StreamGSV(stream_tts) => {
            let resp = crate::ai::tts::stream_gsv(
                &client,
                &stream_tts.url,
                &stream_tts.speaker,
                &text,
                Some(24000),
                stream_tts.text_optimization.as_ref(),
            )
            .await?;

            send_stream_chunk(tx, response_id, item_id, text, resp).await?;
            log::info!("Stream GSV TTS sent");
            Ok(())
        }
        crate::config::TTSConfig::CosyVoice(cosyvoice) => {
            let mut tts =
                cosyvoice::CosyVoiceTTS::connect(&cosyvoice.url, cosyvoice.token.clone()).await?;

            tts.start_synthesis(
                cosyvoice.version,
                cosyvoice.speaker.as_deref(),
                Some(24000),
                &text,
            )
            .await?;

            while let Ok(Some(chunk)) = tts.next_audio_chunk().await {
                tx.send(ServerEvent::ResponseAudioDelta {
                    event_id: Uuid::new_v4().to_string(),
                    response_id: response_id.clone(),
                    item_id: item_id.clone().unwrap_or_default(),
                    output_index: 0,
                    content_index: 1,
                    delta: encode_base64(&chunk),
                })
                .await
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
            }
            Ok(())
        }
        crate::config::TTSConfig::Elevenlabs(elevenlabs_tts) => {
            let mut tts = elevenlabs::tts::ElevenlabsTTS::new(
                &elevenlabs_tts.url,
                elevenlabs_tts.token.clone(),
                elevenlabs_tts.voice.clone(),
                elevenlabs::tts::OutputFormat::Pcm24000,
                &elevenlabs_tts.model_id,
                &elevenlabs_tts.language_code,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Elevenlabs TTS init error: {e}"))?;

            tts.initialize_connection()
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS connection error: {e}"))?;

            tts.send_text(&text, true)
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS send text error: {e}"))?;

            tts.close_connection()
                .await
                .map_err(|e| anyhow::anyhow!("Elevenlabs TTS close connection error: {e}"))?;

            while let Ok(Some(resp)) = tts.next_audio_response().await {
                tx.send(ServerEvent::ResponseAudioDelta {
                    event_id: Uuid::new_v4().to_string(),
                    response_id: response_id.clone(),
                    item_id: item_id.clone().unwrap_or_default(),
                    output_index: 0,
                    content_index: 1,
                    delta: resp.audio.unwrap(),
                })
                .await
                .map_err(|e| anyhow::anyhow!("send audio error: {e}"))?;
            }
            Ok(())
        }
    }
}
