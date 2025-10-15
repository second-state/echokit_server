use std::sync::Arc;

use axum::{routing::any, Router};
use clap::Parser;
use config::Config;

use crate::{config::ASRConfig, services::realtime_ws::StableRealtimeConfig};

pub mod ai;
pub mod config;
pub mod protocol;
pub mod services;
pub mod util;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[clap(
        default_value = "config.toml",
        help = "Path to config file",
        env = "CONFIG_PATH"
    )]
    config: String,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    let config = config::Config::load(&args.config).unwrap();

    let listener = tokio::net::TcpListener::bind(&config.addr).await.unwrap();
    let mut mcp_clients = vec![];
    if let Err(e) = axum::serve(listener, routes(config, &mut mcp_clients).await).await {
        log::error!("Server error: {}", e);
    } else {
        log::warn!("Server exit");
    }
}

async fn routes(
    config: Config,
    clients: &mut Vec<
        rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::InitializeRequestParam>,
    >,
) -> Router {
    log::info!("Start with: {:#?}", config);

    let hello_wav = config.hello_wav.as_ref().and_then(|wav| {
        log::info!("Hello WAV: {}", wav);
        std::fs::read(wav).ok()
    });

    let mut tool_set = ai::openai::tool::ToolSet::default();
    let mut real_config: Option<StableRealtimeConfig> = None;
    match &config.config {
        config::AIConfig::Stable {
            llm,
            tts,
            asr: ASRConfig::Whisper(asr),
        } => {
            real_config = Some(StableRealtimeConfig {
                llm: llm.clone(),
                tts: tts.clone(),
                asr: asr.clone(),
            });
            for server in &llm.mcp_server {
                match server.type_ {
                    config::MCPType::SSE => {
                        if let Err(e) =
                            ai::load_sse_tools(&mut tool_set, clients, &server.server).await
                        {
                            log::error!("Failed to load tools from {}: {}", &server.server, e);
                        }
                    }
                    config::MCPType::HttpStreamable => {
                        if let Err(e) =
                            ai::load_http_streamable_tools(&mut tool_set, clients, &server.server)
                                .await
                        {
                            log::error!("Failed to load tools from {}: {}", &server.server, e);
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut router = Router::new()
        // .route("/", get(handler))
        .route("/ws/{id}", any(services::ws::ws_handler))
        .nest("/record", services::file::new_file_service("./record"))
        .layer(axum::Extension(Arc::new(services::ws::WsSetting::new(
            hello_wav,
            config.config,
            tool_set,
        ))));

    if let Some(real_config) = real_config {
        log::info!(
            "Adding realtime WebSocket handler with config: {:?}",
            real_config
        );
        router = router
            .route("/v1/realtime", any(services::realtime_ws::ws_handler))
            .layer(axum::Extension(Arc::new(real_config)));
    }

    router
}
