use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, WebSocketUpgrade,
    },
    response::{IntoResponse, Response},
    Extension,
};
use bytes::Bytes;

pub struct WsRecordSetting {
    pub record_callback_url: Option<String>,
}

async fn post_to_callback_url(callback_url: &str, id: &str, file_path: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(callback_url)
        .json(&serde_json::json!({
            "id": id,
            "download_uri": format!("/downloads/{}", file_path),
        }))
        .send()
        .await?;

    log::info!(
        "[Record] {} callback to {} success: {}",
        id,
        callback_url,
        resp.status()
    );

    Ok(())
}

pub async fn ws_handler(
    Extension(setting): Extension<Arc<WsRecordSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().as_u128();
    log::info!("[Record] {id}:{request_id:x} connected.");
    if let Err(e) = std::fs::create_dir_all(format!("./record/{}", id)) {
        log::error!("[Record] {} create dir failed: {}", id, e);
        return Response::builder()
            .status(500)
            .body("Internal Server Error".into())
            .unwrap();
    }

    ws.on_upgrade(move |socket| async move {
        match handle_socket(socket, &id).await {
            Ok(file_path) => {
                if let Some(callback_url) = &setting.record_callback_url {
                    if let Err(e) = post_to_callback_url(callback_url, &id, &file_path).await {
                        log::error!("[Record] {} callback to {} failed: {}", id, callback_url, e);
                    }
                }
            }
            Err(e) => {
                log::error!("{id}:{request_id:x} error: {e}");
            }
        }
        log::info!("{id}:{request_id:x} disconnected.");
    })
}

enum ProcessMessageResult {
    Audio(Bytes),
    Close,
    Skip,
}

fn process_message(msg: Message) -> ProcessMessageResult {
    match msg {
        Message::Text(_) => ProcessMessageResult::Skip,
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

async fn handle_socket(mut socket: WebSocket, id: &str) -> anyhow::Result<String> {
    let now = chrono::Local::now();
    let date_str = now.format("%Y%m%d_%H%M%S%z").to_string();
    let file_path = format!("{id}/record_{date_str}.wav");
    let path = format!("./record/{file_path}");

    let mut wav_file = crate::util::UnlimitedWavFileWriter::new(
        &path,
        crate::util::WavConfig {
            sample_rate: 16000,
            channels: 1,
            bits_per_sample: 16,
        },
    )
    .await?;

    wav_file.write_wav_header().await?;

    while let Ok(Some(Ok(message))) =
        tokio::time::timeout(std::time::Duration::from_secs(60), socket.recv()).await
    {
        match process_message(message) {
            ProcessMessageResult::Audio(chunk) => {
                wav_file.write_pcm_data(&chunk).await?;
            }
            ProcessMessageResult::Close => {
                return Err(anyhow::anyhow!("ws closed"));
            }
            ProcessMessageResult::Skip => {}
        }
    }

    Ok(file_path)
}
