use std::{
    collections::{HashMap, LinkedList},
    sync::{Arc, RwLock, atomic::AtomicBool},
};

use axum::{Extension, extract::Path, response::IntoResponse};

use crate::{
    config::{ASRConfig, EchokitCC, TTSConfig},
    services::ws::{ClientMsg, stable::tts::TTSRequestTx},
};

use super::Session;

#[derive(Default, Clone)]
pub struct ClaudeNotification {
    notification: Arc<AtomicBool>,
}

impl ClaudeNotification {
    pub fn mark(&self) {
        self.notification
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn value(&self) -> bool {
        self.notification.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.notification
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct ClaudeNotifications {
    pub sessions: HashMap<String, ClaudeNotification>,
}

async fn get_input(
    session: &mut Session,
    asr_session: &mut super::asr::AsrSession,
) -> anyhow::Result<String> {
    loop {
        log::info!(
            "{}:{:x} waiting for asr input",
            session.id,
            session.request_id
        );
        let text = if session.stream_asr {
            match asr_session.stream_get_input(session).await {
                Ok(t) => t,
                Err(e) => {
                    log::error!(
                        "{}:{:x} error getting asr input: {}",
                        session.id,
                        session.request_id,
                        e
                    );
                    session.send_end_vad().map_err(|_| {
                        anyhow::anyhow!(
                            "{}:{:x} error sending end vad ws command after asr error",
                            session.id,
                            session.request_id
                        )
                    })?;
                    return Err(e);
                }
            }
        } else {
            asr_session
                .get_input(&session.id, &mut session.client_rx)
                .await?
        };
        if text.is_empty() {
            log::info!(
                "{}:{:x} empty asr result, ending session",
                session.id,
                session.request_id
            );

            session.send_end_response().map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending end response ws command for empty asr result",
                    session.id,
                    session.request_id
                )
            })?;

            continue;
        } else {
            log::info!(
                "{}:{:x} asr result: {}",
                session.id,
                session.request_id,
                text
            );
            session.send_asr_result(vec![text.clone()]).map_err(|_| {
                anyhow::anyhow!(
                    "{}:{:x} error sending asr result ws command for message `{}`",
                    session.id,
                    session.request_id,
                    text
                )
            })?;
            return Ok(text);
        }
    }
}

async fn get_choice(session: &mut Session) -> anyhow::Result<usize> {
    while let Some(evt) = session.client_rx.recv().await {
        if let ClientMsg::Select(choice) = evt {
            return Ok(choice);
        }
        log::debug!(
            "{}:{:x} ignoring non-select client message during select prompt",
            session.id,
            session.request_id
        );
    }
    Err(anyhow::anyhow!(
        "client disconnected before making a choice"
    ))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AskUserQuestionToolArgs {
    questions: Vec<AskUserQuestion>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AskUserQuestion {
    header: String,
    question: String,
    options: Vec<AskUserQuestionItem>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AskUserQuestionItem {
    label: String,
    description: String,
}

struct RunSessionState {
    cc_session: cc_session::ClaudeSession,
    session: Session,
    rx: tokio::sync::mpsc::UnboundedReceiver<Session>,
    notify: ClaudeNotification,
}

enum RunSessionSelectResult {
    Session(Option<Session>),
    ClientMsg(Option<ClientMsg>),
    ClaudeMsg(Option<cc_session::WsOutputMessage>),
}

enum SendStateError {
    ClaudeError,
    ClientError,
}

impl RunSessionState {
    async fn recv(&mut self) -> anyhow::Result<RunSessionSelectResult> {
        async fn recv_client_msg(session: &mut Session) -> Option<ClientMsg> {
            struct PendingClientMsg;
            impl Future for PendingClientMsg {
                type Output = Option<ClientMsg>;

                fn poll(
                    self: std::pin::Pin<&mut Self>,
                    _cx: &mut std::task::Context<'_>,
                ) -> std::task::Poll<Self::Output> {
                    std::task::Poll::Pending
                }
            }

            if session.client_rx.is_closed() {
                PendingClientMsg.await
            } else {
                session.client_rx.recv().await
            }
        }

        let r = tokio::select! {
            new_session = self.rx.recv() => {
                RunSessionSelectResult::Session(new_session)
            }
            client_msg = recv_client_msg(&mut self.session) => {
                RunSessionSelectResult::ClientMsg(client_msg)
            }
            claude_msg = self.cc_session.receive_message() => {
                RunSessionSelectResult::ClaudeMsg(claude_msg?)
            }
        };

        Ok(r)
    }

    async fn send_input(&mut self, input: &str) -> anyhow::Result<()> {
        self.cc_session
            .send_message(&cc_session::WsInputMessage::Input {
                input: input.to_string(),
            })
            .await
    }

    async fn send_display(&mut self, output: &str) -> anyhow::Result<()> {
        self.session.send_display_text(output.to_string())?;
        Ok(())
    }

    async fn send_output_with_tts(
        &mut self,
        output: &str,
        tts_req_tx: &TTSRequestTx,
    ) -> anyhow::Result<()> {
        let mut text_splitter = crate::ai::TextSplitter::new();
        text_splitter.push_chunk(&output);

        let finished_output = text_splitter.finish();

        let mut rx_list = LinkedList::new();
        for chunk in finished_output {
            let (tts_response_tx, tts_response_rx) = tokio::sync::mpsc::unbounded_channel();
            if let Err(e) = tts_req_tx.send((chunk.to_string(), tts_response_tx)).await {
                log::error!(
                    "{}:{:x} error sending tts request: {}",
                    self.session.id,
                    self.session.request_id,
                    e
                );
            } else {
                rx_list.push_back((chunk, tts_response_rx));
            }
        }

        for (text_chunk, mut tts_response_rx) in rx_list {
            self.session.send_start_audio(text_chunk)?;
            while let Some(chunk) = tts_response_rx.recv().await {
                self.session.send_audio_chunk(chunk)?;
            }
            self.session.send_end_audio()?;
        }

        self.session.send_end_response()?;
        Ok(())
    }

    async fn send_confirm(&mut self) -> anyhow::Result<()> {
        self.cc_session
            .send_message(&cc_session::WsInputMessage::Confirm {})
            .await
    }

    async fn send_select(&mut self, index: usize) -> anyhow::Result<()> {
        self.cc_session
            .send_message(&cc_session::WsInputMessage::Select { index })
            .await
    }

    async fn send_cancel(&mut self) -> anyhow::Result<()> {
        self.cc_session
            .send_message(&cc_session::WsInputMessage::Cancel {})
            .await
    }

    async fn sync_cc_state(&mut self) -> anyhow::Result<()> {
        self.cc_session
            .send_message(&cc_session::WsInputMessage::CurrentState {})
            .await
    }

    fn set_session(&mut self, session: Session) {
        self.session = session;
    }

    async fn send_self_state(
        &mut self,
        tts_req_tx: &mut TTSRequestTx,
        asr_session: &mut super::asr::AsrSession,
    ) -> Result<(), SendStateError> {
        let state = self.cc_session.state.clone();
        log::debug!(
            "{}:{:x} sending self state: {:?}",
            self.session.id,
            self.session.request_id,
            state
        );
        match state {
            cc_session::ClaudeCodeState::Output {
                output,
                is_thinking,
            } => {
                if is_thinking {
                    log::debug!(
                        "{}:{:x} sending thinking output",
                        self.session.id,
                        self.session.request_id
                    );
                    let _ = self.send_display(&output).await;
                } else {
                    // self.cc_session.state = cc_session::ClaudeCodeState::Idle;

                    if let Err(e) = self.send_output_with_tts(&output, tts_req_tx).await {
                        log::warn!(
                            "{}:{:x} error sending tts output: {}",
                            self.session.id,
                            self.session.request_id,
                            e
                        );
                    }

                    self.cc_session.last_output = output;
                }
            }
            cc_session::ClaudeCodeState::Idle => {
                if !self.cc_session.last_output.is_empty() {
                    if let Err(e) = self
                        .session
                        .send_display_text(self.cc_session.last_output.clone())
                    {
                        log::warn!(
                            "{}:{:x} error sending display output: {}",
                            self.session.id,
                            self.session.request_id,
                            e
                        );
                        return Err(SendStateError::ClientError);
                    }
                }

                log::info!(
                    "{}:{:x} waiting for user input",
                    self.session.id,
                    self.session.request_id
                );

                match self.wait_input(asr_session).await {
                    Ok(text) => {
                        let _ = self.send_input(&text).await;
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        let _ = self.send_confirm().await;
                        self.cc_session.state = cc_session::ClaudeCodeState::Idle;
                    }
                    Err(e) => {
                        log::warn!(
                            "{}:{:x} error getting input: {}",
                            self.session.id,
                            self.session.request_id,
                            e
                        );
                        return Err(SendStateError::ClientError);
                    }
                }
            }
            cc_session::ClaudeCodeState::PreUseTool {
                request,
                is_pending,
            } => {
                if is_pending {
                    for (i, tool) in request.into_iter().enumerate() {
                        if tool.done {
                            continue;
                        }
                        match self.wait_tool_use_choice(tool.name, tool.input).await {
                            Ok(-1) => {
                                self.send_cancel().await.map_err(|e| {
                                    log::error!(
                                        "{}:{:x} error sending tool use cancel: {}",
                                        self.session.id,
                                        self.session.request_id,
                                        e
                                    );
                                    SendStateError::ClaudeError
                                })?;
                            }
                            Ok(n) => {
                                self.send_select(n as usize).await.map_err(|e| {
                                    log::error!(
                                        "{}:{:x} error sending tool use confirm: {}",
                                        self.session.id,
                                        self.session.request_id,
                                        e
                                    );
                                    SendStateError::ClaudeError
                                })?;
                                if let cc_session::ClaudeCodeState::PreUseTool { request, .. } =
                                    &mut self.cc_session.state
                                {
                                    request[i].submited = true;
                                }

                                break;
                            }
                            Err(e) => {
                                log::warn!(
                                    "{}:{:x} error sending tool use choice prompt: {}",
                                    self.session.id,
                                    self.session.request_id,
                                    e
                                );
                                return Err(SendStateError::ClientError);
                            }
                        }
                    }
                }
            }
            cc_session::ClaudeCodeState::StopUseTool { is_error } => {
                if is_error {
                    let _ = self.send_display(&"Tool use stopped with error.").await;
                } else {
                    let _ = self.send_display(&"Tool use completed successfully.").await;
                }
                self.session.send_end_response().map_err(|e| {
                    log::error!(
                        "{}:{:x} error sending end response after tool use stop: {}",
                        self.session.id,
                        self.session.request_id,
                        e
                    );
                    SendStateError::ClientError
                })?;
            }
        };

        Ok(())
    }

    async fn wait_input(
        &mut self,
        asr_session: &mut super::asr::AsrSession,
    ) -> anyhow::Result<String> {
        loop {
            log::debug!(
                "{}:{:x} waiting for user input confirmation",
                self.session.id,
                self.session.request_id
            );
            let text = get_input(&mut self.session, asr_session).await?;
            self.session.send_choice_prompt(
                text.clone(),
                vec!["Confirm".to_string(), "Cancel".to_string()],
            )?;
            log::debug!(
                "{}:{:x} waiting for user input choice",
                self.session.id,
                self.session.request_id
            );
            let choice = get_choice(&mut self.session).await?;
            match choice {
                0 => {
                    return Ok(text);
                }
                _ => {
                    self.session
                        .send_notify("Input cancelled, please provide input again".to_string())?;
                    self.session.send_end_response()?;
                    continue;
                }
            }
        }
    }

    async fn wait_tool_use_choice(
        &mut self,
        tool_name: String,
        tool_args: serde_json::Value,
    ) -> anyhow::Result<i32> {
        if tool_name == "AskUserQuestion" {
            let tool_args = serde_json::from_value::<AskUserQuestionToolArgs>(tool_args);
            if tool_args.is_err() {
                self.session.send_notify(format!(
                    "Claude requested to use tool `AskUserQuestion` with invalid args: {}",
                    tool_args.err().unwrap()
                ))?;
                return Ok(-1);
            } else {
                let tool_args = tool_args.unwrap();
                for question in tool_args.questions {
                    let options: Vec<String> = question
                        .options
                        .iter()
                        .map(|item| format!("{}: {}", item.label, item.description))
                        .collect();
                    self.session.send_choice_prompt(
                        format!(
                            "{}\n{}\nPlease select one of the following options:",
                            question.header, question.question
                        ),
                        options,
                    )?;
                    let choice = get_choice(&mut self.session).await?;
                    return Ok(choice as i32);
                }
                Ok(-1)
            }
        } else {
            let tool_args_string = if let serde_json::Value::String(ref s) = tool_args {
                s.clone()
            } else if let serde_json::Value::Object(ref map) = tool_args {
                map.iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect::<Vec<String>>()
                    .join("\n")
            } else {
                tool_args.to_string()
            };
            self.session.send_choice_prompt(
                format!(
                    "Claude is requesting to use tool `{}` \n with args:\n{}",
                    tool_name, tool_args_string
                ),
                vec!["Confirm".to_string(), "Cancel".to_string()],
            )?;
            match get_choice(&mut self.session).await? {
                0 => Ok(0),
                _ => Ok(-1),
            }
        }
    }
}

fn update_state(
    cc_state: &mut cc_session::ClaudeCodeState,
    new_state: cc_session::ClaudeCodeState,
) -> bool {
    match (cc_state, new_state) {
        (
            cc_session::ClaudeCodeState::PreUseTool {
                request,
                is_pending,
            },
            cc_session::ClaudeCodeState::PreUseTool {
                request: new_request,
                is_pending: new_is_pending,
            },
        ) => {
            if *is_pending != new_is_pending {
                *request = new_request;
                *is_pending = new_is_pending;
                return true;
            }

            if request.len() != new_request.len() {
                *request = new_request;
                return true;
            }

            for (r, nr) in request.iter_mut().zip(new_request.into_iter()) {
                if r.submited && !nr.done {
                    log::debug!("Received PreUseTool state without done tool after submited");
                    return false;
                }
                *r = nr;
            }
            return true;
        }
        (cc_state, new_state) => {
            if cc_state != &new_state {
                *cc_state = new_state;
                return true;
            }
        }
    }
    false
}

async fn run_session(
    id: uuid::Uuid,
    url: &str,
    tts_req_tx: &mut TTSRequestTx,
    asr_session: &mut super::asr::AsrSession,
    notify: ClaudeNotification,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Session>,
) -> anyhow::Result<()> {
    use cc_session::*;

    let mut cc_session = ClaudeSession::new(id.to_string(), url)
        .await
        .map_err(|e| anyhow::anyhow!("error creating claude session for id `{}`: {}", id, e))?;

    cc_session
        .send_message(&WsInputMessage::CreateSession {})
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "error sending create session message for id `{}`: {}",
                id,
                e
            )
        })?;

    let session = rx.recv().await.ok_or_else(|| {
        anyhow::anyhow!(
            "session channel closed before receiving session for id `{}`",
            id
        )
    })?;

    let mut run_session_state = RunSessionState {
        cc_session,
        session,
        rx,
        notify,
    };

    loop {
        log::debug!("Claude session {} waiting for events", id);
        let r = run_session_state.recv().await?;

        match r {
            RunSessionSelectResult::ClaudeMsg(Some(log)) => {
                log::debug!("Claude session {} received message: {:?}", id, log);
                match log {
                    WsOutputMessage::SessionPtyOutput { .. } => {
                        continue;
                    }
                    WsOutputMessage::SessionEnded { session_id } => {
                        log::warn!("Claude session {} ended by server", session_id);
                        return Ok(());
                    }
                    WsOutputMessage::SessionIdle { session_id } => {
                        log::info!("Claude session {} is idle", session_id);
                        if run_session_state.cc_session.state != cc_session::ClaudeCodeState::Idle {
                            run_session_state.cc_session.state = cc_session::ClaudeCodeState::Idle;
                        } else {
                            continue;
                        }
                    }
                    WsOutputMessage::SessionState {
                        session_id: _,
                        current_state,
                    } => {
                        log::debug!(
                            "Claude session {} received state update: {:?}",
                            id,
                            current_state
                        );
                        if !update_state(&mut run_session_state.cc_session.state, current_state) {
                            log::debug!("Claude session {} state unchanged after update", id,);
                            continue;
                        }
                    }
                    WsOutputMessage::SessionError { session_id, code } => {
                        log::error!(
                            "Claude session {} received error from server: {:?}",
                            session_id,
                            code
                        );
                        return Err(anyhow::anyhow!(
                            "claude session error for id `{}`: {:?}",
                            id,
                            code
                        ));
                    }
                }

                match run_session_state
                    .send_self_state(tts_req_tx, asr_session)
                    .await
                {
                    Ok(_) => {}
                    Err(SendStateError::ClientError) => {
                        if !matches!(
                            run_session_state.cc_session.state,
                            cc_session::ClaudeCodeState::Idle
                        ) {
                            run_session_state.notify.mark();
                        } else {
                            log::info!(
                                "Claude session {} client disconnected during idle state, ending session",
                                id
                            );
                        }
                    }
                    Err(SendStateError::ClaudeError) => {
                        return Err(anyhow::anyhow!(
                            "claude session error for id `{}` during state send",
                            id
                        ));
                    }
                }
            }
            RunSessionSelectResult::ClaudeMsg(None) => {
                log::warn!("Claude session {} closed by server", id);
                cc_session = ClaudeSession::new(id.to_string(), url).await.map_err(|e| {
                    anyhow::anyhow!("error recreating claude session for id `{}`: {}", id, e)
                })?;

                cc_session
                    .send_message(&WsInputMessage::CreateSession {})
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "error sending create session message for id `{}`: {}",
                            id,
                            e
                        )
                    })?;
            }
            RunSessionSelectResult::ClientMsg(Some(_)) => {
                let _ = run_session_state.session.send_end_vad();
                let _ = run_session_state.session.send_end_response();
            }
            RunSessionSelectResult::ClientMsg(None) => {
                log::warn!("Claude session {} client disconnected", id);
            }
            RunSessionSelectResult::Session(Some(new_session)) => {
                log::info!("Claude session {} switching to new session", id);
                run_session_state.set_session(new_session);
                match run_session_state
                    .send_self_state(tts_req_tx, asr_session)
                    .await
                {
                    Ok(_) => {
                        run_session_state.notify.clear();
                    }
                    Err(SendStateError::ClientError) => {
                        run_session_state.notify.mark();
                    }
                    Err(SendStateError::ClaudeError) => {
                        return Err(anyhow::anyhow!(
                            "claude session error for id `{}` during state send",
                            id
                        ));
                    }
                }
            }
            RunSessionSelectResult::Session(None) => {
                log::error!("Claude session {} session channel closed", id);
                return Err(anyhow::anyhow!("session channel closed for id `{}`", id));
            }
        }
    }
}

const NAMESPACE: uuid::Uuid = uuid::uuid!("8e1f6eb8-d389-4e62-9cfd-f1964e499c25"); // Namespace UUID for generating session UUIDs

pub async fn run_session_manager(
    tts: &TTSConfig,
    asr: &ASRConfig,
    claude: &EchokitCC,
    mut session_rx: tokio::sync::mpsc::UnboundedReceiver<Session>,
    notifications: Arc<RwLock<ClaudeNotifications>>,
) -> anyhow::Result<()> {
    let mut tts_session_pool = super::tts::TTSSessionPool::new(tts.clone(), 4);
    let (tts_req_tx, tts_req_rx) = tokio::sync::mpsc::channel(128);

    let mut sessions: HashMap<
        String,
        (
            tokio::sync::mpsc::UnboundedSender<Session>,
            crate::services::ws::WsTx,
            uuid::Uuid,
        ),
    > = HashMap::new();

    tokio::spawn(async move {
        if let Err(e) = tts_session_pool.run_loop(tts_req_rx).await {
            log::error!("tts session pool exit by error: {}", e);
        }
    });

    while let Some(session) = session_rx.recv().await {
        let (session, cmd_tx, session_id) =
            if let Some((tx, cmd_tx, id)) = sessions.get_mut(&session.id) {
                let _ = cmd_tx.send(crate::services::ws::WsCommand::Close);

                let new_tx = session.cmd_tx.clone();

                if let Err(e) = tx.send(session) {
                    (e.0, new_tx, id.clone())
                } else {
                    let _ = cmd_tx.send(crate::services::ws::WsCommand::Close);
                    *cmd_tx = new_tx;

                    continue;
                }
            } else {
                let cmd_tx = session.cmd_tx.clone();
                let id = session.id.clone();
                (
                    session,
                    cmd_tx,
                    uuid::Uuid::new_v5(&NAMESPACE, id.as_bytes()),
                )
            };

        // start new session

        let notify = {
            let notify = ClaudeNotification::default();
            let mut notifications_lock = notifications.write().unwrap();
            notifications_lock
                .sessions
                .insert(session.id.clone(), notify.clone());
            notify
        };

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let id = session.id.clone();
        log::info!("Starting new session for id: {}", id);
        let _ = tx.send(session);

        sessions.insert(id.clone(), (tx, cmd_tx, session_id));

        // run session

        let asr = asr.clone();

        let mut tts_req_tx = tts_req_tx.clone();

        let url = claude.url.clone();

        tokio::spawn(async move {
            let mut asr_session = super::asr::AsrSession::new_from_config(&asr)
                .await
                .map_err(|e| {
                    log::error!("error creating asr session for id `{}`: {}", id, e);
                    anyhow::anyhow!("error creating asr session for id `{}`: {}", id, e)
                })?;

            if let Err(e) = run_session(
                session_id,
                &url,
                &mut tts_req_tx,
                &mut asr_session,
                notify,
                rx,
            )
            .await
            {
                log::error!("session `{}` exited with error: {}", id, e);
            }

            anyhow::Result::<()>::Ok(())
        });
    }
    log::warn!("session manager exiting");
    Ok(())
}

pub async fn has_notification(
    Extension(sessions): Extension<Arc<RwLock<ClaudeNotifications>>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let state = {
        let sessions_lock = sessions.read().unwrap();
        sessions_lock.sessions.get(&id).map_or(false, |n| n.value())
    };

    axum::Json(serde_json::json!({ "has_notification": state }))
}

mod cc_session {
    use futures_util::{SinkExt, StreamExt};
    use reqwest_websocket::{RequestBuilderExt, WebSocket};

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type")]
    pub enum WsInputMessage {
        #[serde(alias = "create_session")]
        CreateSession {},
        #[serde(alias = "get_current_state")]
        CurrentState {},
        #[serde(alias = "input")]
        Input { input: String },
        #[serde(alias = "cancel")]
        Cancel {},
        #[serde(alias = "confirm")]
        Confirm {},
        #[serde(alias = "select")]
        Select { index: usize },
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "error_code")]
    pub enum WsOutputError {
        #[serde(rename = "session_not_found")]
        SessionNotFound,
        #[serde(rename = "invalid_input")]
        InvalidInput {
            error_message: String,
        },
        #[serde(rename = "invalid_input_for_state")]
        InvalidInputForState {
            error_state: String,
            error_input: String,
        },
        InternalError {
            error_message: String,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub struct UseTool {
        pub id: String,
        pub name: String,
        pub input: serde_json::Value,
        pub done: bool,
        #[serde(default)]
        pub submited: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "state")]
    pub enum ClaudeCodeState {
        PreUseTool {
            request: Vec<UseTool>,
            is_pending: bool,
        },
        Output {
            output: String,
            is_thinking: bool,
        },
        StopUseTool {
            is_error: bool,
        },
        Idle,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type")]
    pub enum WsOutputMessage {
        #[serde(rename = "session_pty_output")]
        SessionPtyOutput { output: String },
        #[serde(rename = "session_ended")]
        SessionEnded { session_id: String },
        #[serde(rename = "session_idle")]
        SessionIdle { session_id: String },
        #[serde(rename = "session_state")]
        SessionState {
            session_id: String,
            current_state: ClaudeCodeState,
        },
        #[serde(rename = "session_error")]
        SessionError {
            session_id: String,
            #[serde(flatten)]
            code: WsOutputError,
        },
    }

    pub struct ClaudeSession {
        pub id: String,
        pub socket: WebSocket,
        pub state: ClaudeCodeState,
        pub last_output: String,
    }

    impl ClaudeSession {
        pub async fn new(id: String, url: &str) -> anyhow::Result<Self> {
            let url = format!("{}/{}", url.trim_end_matches('/'), id);
            log::info!("Connecting to Claude WebSocket at {}", url);

            let client = reqwest::Client::new();
            let response = client.get(url).upgrade().send().await?;

            let websocket = response.into_websocket().await?;

            Ok(Self {
                id: id.to_string(),
                socket: websocket,
                state: ClaudeCodeState::Output {
                    output: String::new(),
                    is_thinking: false,
                },
                last_output: String::new(),
            })
        }

        pub async fn send_message(&mut self, message: &WsInputMessage) -> anyhow::Result<()> {
            let msg_text = serde_json::to_string(message)?;
            self.socket
                .send(reqwest_websocket::Message::Text(msg_text))
                .await?;
            Ok(())
        }

        pub async fn receive_message(&mut self) -> anyhow::Result<Option<WsOutputMessage>> {
            loop {
                let msg = self.socket.next().await;

                if msg.is_none() {
                    return Ok(None);
                }

                let msg = msg.unwrap()?;

                match msg {
                    reqwest_websocket::Message::Text(s) => {
                        let output_msg: WsOutputMessage = serde_json::from_str(&s)?;
                        if matches!(output_msg, WsOutputMessage::SessionPtyOutput { .. }) {
                            log::trace!(
                                "Received pty output message for session {}: {:?}",
                                self.id,
                                output_msg
                            );
                            continue;
                        }
                        return Ok(Some(output_msg));
                    }
                    reqwest_websocket::Message::Binary(bytes) => {
                        log::warn!(
                            "Received unexpected binary message for session {}: {} bytes",
                            self.id,
                            bytes.len()
                        );
                    }
                    reqwest_websocket::Message::Close { code, reason } => {
                        log::info!(
                            "WebSocket closed for session {}: code={:?}, reason={}",
                            self.id,
                            code,
                            reason
                        );
                        return Ok(None);
                    }
                    _ => {}
                }
            }
        }
    }
}
