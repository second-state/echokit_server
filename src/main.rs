use std::sync::Arc;

use axum::{
    Router,
    routing::{any, get},
};
use clap::Parser;
use config::Config;


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

    // todo: support other configs
    match &config.config {
        config::AIConfig::Stable {
            llm: config::LLMConfig::OpenAIChat(chat_llm),
            tts: _,
            asr: _,
        } => {
            for server in &chat_llm.mcp_server {
                match server.type_ {
                    config::MCPType::SSE => {
                        if let Err(e) = ai::load_sse_tools(
                            &mut tool_set,
                            clients,
                            &server.server,
                            &server.call_mcp_message,
                        )
                        .await
                        {
                            log::error!("Failed to load tools from {}: {}", &server.server, e);
                        }
                    }
                    config::MCPType::HttpStreamable => {
                        if let Err(e) = ai::load_http_streamable_tools(
                            &mut tool_set,
                            clients,
                            &server.server,
                            &server.call_mcp_message,
                        )
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

    let record_config = Arc::new(services::ws_record::WsRecordSetting {
        record_callback_url: config.record.callback_url,
    });

    let ws_setting = Arc::new(services::ws::WsSetting::new(
        hello_wav.clone(),
        config.config.clone(),
        tool_set.clone(),
    ));

    let mut router = Router::new()
        // .route("/", get(handler))
        .route("/v1/record/{id}", any(services::ws_record::ws_handler))
        .nest("/downloads", services::file::new_file_service("./record"))
        .layer(axum::Extension(ws_setting.clone()))
        .layer(axum::Extension(record_config.clone()));

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    match config.config {
        config::AIConfig::Stable { llm, tts, asr } => {
            // let tool_set = tool_set;
            tokio::spawn(async move {
                if let Err(e) = crate::services::ws::stable::run_session_manager(
                    &llm, &tts, &asr, &tool_set, rx,
                )
                .await
                {
                    log::error!("Stable session manager exited with error: {}", e);
                }
            });
        }
        config::AIConfig::GeminiAndTTS { gemini, tts } => {
            tokio::spawn(async move {
                if let Err(e) = crate::services::ws::stable::gemini::run_session_manager(
                    &gemini,
                    Some(&tts),
                    rx,
                )
                .await
                {
                    log::error!("Gemini session manager exited with error: {}", e);
                }
            });
        }
        config::AIConfig::Gemini { gemini } => {
            // let tool_set = tool_set;
            tokio::spawn(async move {
                if let Err(e) =
                    crate::services::ws::stable::gemini::run_session_manager(&gemini, None, rx)
                        .await
                {
                    log::error!("Gemini session manager exited with error: {}", e);
                }
            });
        }
    }

    router = router
        .route("/ws/{id}", any(services::v2_mixed_handler))
        .route("/v2/stable_ws/{id}", any(services::ws::stable::ws_handler))
        .layer(axum::Extension(Arc::new(
            services::ws::stable::StableWsSetting {
                sessions: tx,
                hello_wav,
            },
        )))
        .layer(axum::Extension(record_config.clone()));

    router.route(
        "/version",
        get(|| async {
            axum::response::Json(serde_json::json!(
            {
                "version": env!("CARGO_PKG_VERSION"),
            }))
        }),
    )
}
