use axum::{extract::Path, handler::HandlerWithoutStateExt, Router};

async fn handle_404() -> (http::StatusCode, &'static str) {
    (http::StatusCode::NOT_FOUND, "Not found")
}

async fn list_files(
    Path(id): Path<String>,
) -> (http::StatusCode, axum::Json<Vec<serde_json::Value>>) {
    log::info!("Listing files for id: {}", id);
    let path = format!("./record/{}", id);
    let entries = std::fs::read_dir(&path);
    match entries {
        Ok(entries) => {
            let mut file_list = vec![];
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let is_file = entry.metadata().map(|meta| meta.is_file()).unwrap_or(false);
                        if is_file {
                            if let Some(name) = entry.file_name().to_str() {
                                file_list.push(serde_json::json!({
                                    "file_name":name,
                                    "download_url": format!("/record/download/{}/{}", id, name),
                                }));
                            }
                        }
                    }
                    Err(e) => log::error!("Error reading entry: {}", e),
                }
            }
            log::info!("Found {} files for id: {}", file_list.len(), id);
            (http::StatusCode::OK, axum::Json(file_list))
        }
        Err(e) => {
            log::error!("Error reading directory {}: {}", path, e);
            (http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(vec![]))
        }
    }
}

pub fn new_file_service(path: &str) -> Router {
    let serve_dir =
        tower_http::services::ServeDir::new(path).not_found_service(handle_404.into_service());

    Router::new()
        .nest_service("/download", serve_dir)
        .route("/list/{id}", axum::routing::get(list_files))
}
