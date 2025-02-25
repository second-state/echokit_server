use std::sync::Arc;

use axum::{routing::any, Router};

pub mod ai;
pub mod protocol;
pub mod services;

#[tokio::main]
async fn main() {
    env_logger::init();
    let addr = "0.0.0.0:8080";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    if let Err(e) = axum::serve(listener, routes()).await {
        log::error!("Server error: {}", e);
    } else {
        log::warn!("Server exit");
    }
}

fn routes() -> Router {
    Router::new()
        // .route("/", get(handler))
        .route("/ws/{id}", any(services::ws::ws_handler))
        .layer(axum::Extension(Arc::new(services::ws::WsPool::default())))
}
