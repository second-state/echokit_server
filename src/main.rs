use std::sync::Arc;

use axum::{routing::any, Router};
use config::Config;

pub mod ai;
pub mod config;
pub mod protocol;
pub mod services;

#[tokio::main]
async fn main() {
    env_logger::init();
    let config_path = std::env::args().nth(1).unwrap_or("config.toml".to_string());
    let config = config::Config::load(&config_path).unwrap();

    let listener = tokio::net::TcpListener::bind(&config.addr).await.unwrap();
    if let Err(e) = axum::serve(listener, routes(config)).await {
        log::error!("Server error: {}", e);
    } else {
        log::warn!("Server exit");
    }
}

fn routes(config: Config) -> Router {
    log::info!("Start with: {:#?}", config);
    Router::new()
        // .route("/", get(handler))
        .route("/ws/{id}", any(services::ws::ws_handler))
        .layer(axum::Extension(Arc::new(services::ws::WsPool::new(
            config.config,
        ))))
}
