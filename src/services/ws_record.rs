use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, WebSocketUpgrade,
    },
    response::IntoResponse,
    Extension,
};
use bytes::Bytes;

use crate::services::ws::WsSetting;

pub async fn ws_handler(
    Extension(pool): Extension<Arc<WsSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().as_u128();
    log::info!("[Record] {id}:{request_id:x} connected.");

    ws.on_upgrade(move |socket| async move {
        let id = id.clone();
        let pool = pool.clone();
        if let Err(e) = handle_socket(socket, &id, pool.clone()).await {
            log::error!("{id}:{request_id:x} error: {e}");
        };
        log::info!("{id}:{request_id:x} disconnected.");
    })
}

enum ProcessMessageResult {
    Audio(Bytes),
    Submit,
    Text(String),
    StartRecord,
    StartChat,
    Close,
    Skip,
}

fn process_message(msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(t) => {
            if let Ok(cmd) = serde_json::from_str::<crate::protocol::ClientCommand>(&t) {
                match cmd {
                    crate::protocol::ClientCommand::StartRecord => {
                        ProcessMessageResult::StartRecord
                    }
                    crate::protocol::ClientCommand::StartChat => ProcessMessageResult::StartChat,
                    crate::protocol::ClientCommand::Submit => ProcessMessageResult::Submit,
                    crate::protocol::ClientCommand::Text { input } => {
                        ProcessMessageResult::Text(input)
                    }
                }
            } else {
                ProcessMessageResult::Skip
            }
        }
        Message::Binary(d) => ProcessMessageResult::Audio(d),
        Message::Close(c) => {
            if let Some(cf) = c {
                log::info!(
                    "sent close with code {} and reason `{}`",
                    cf.code,
                    cf.reason
                );
            } else {
                log::info!("somehow sent close message without CloseFrame");
            }
            ProcessMessageResult::Close
        }

        Message::Pong(_) | Message::Ping(_) => ProcessMessageResult::Skip,
    }
}

// TODO: implement recording logic
async fn handle_socket(
    mut socket: WebSocket,
    id: &str,
    pool: Arc<WsSetting>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(format!("./record/{id}"))?;

    while let Some(message) = socket.recv().await {
        let message = message.map_err(|e| anyhow::anyhow!("recv ws error: {e}"))?;

        match process_message(message) {
            ProcessMessageResult::Audio(_) => {}
            ProcessMessageResult::Submit => {}
            ProcessMessageResult::Text(_) => {}
            ProcessMessageResult::Skip => {}
            ProcessMessageResult::StartRecord => {}
            ProcessMessageResult::StartChat => {}
            ProcessMessageResult::Close => {
                return Err(anyhow::anyhow!("ws closed"));
            }
        }
    }

    Ok(())
}
