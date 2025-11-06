use std::sync::Arc;

use axum::{
    extract::{Path, Query, WebSocketUpgrade},
    response::{IntoResponse, Response},
    Extension,
};

pub mod file;
pub mod realtime_ws;
pub mod ws;
pub mod ws_record;

#[derive(Debug, serde::Deserialize)]
pub struct ConnectQueryParams {
    #[serde(default)]
    reconnect: bool,
    #[serde(default)]
    record: bool,
}

pub async fn mixed_handler(
    Extension(pool): Extension<Arc<ws::WsSetting>>,
    Extension(record_setting): Extension<Arc<ws_record::WsRecordSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(params): Query<ConnectQueryParams>,
) -> Response {
    if params.record {
        ws_record::ws_handler(Extension(record_setting), ws, Path(id))
            .await
            .into_response()
    } else {
        ws::ws_handler(
            Extension(pool),
            ws,
            Path(id),
            Query(ws::ConnectQueryParams {
                reconnect: params.reconnect,
            }),
        )
        .await
        .into_response()
    }
}

pub async fn v2_mixed_handler(
    Extension(record_setting): Extension<Arc<ws_record::WsRecordSetting>>,
    Extension(pool): Extension<Arc<ws::stable::StableWsSetting>>,
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(params): Query<ConnectQueryParams>,
) -> Response {
    if params.record {
        ws_record::ws_handler(Extension(record_setting), ws, Path(id))
            .await
            .into_response()
    } else {
        ws::stable::ws_handler(
            Extension(pool),
            ws,
            Path(id),
            Query(ws::ConnectQueryParams {
                reconnect: params.reconnect,
            }),
        )
        .await
        .into_response()
    }
}
