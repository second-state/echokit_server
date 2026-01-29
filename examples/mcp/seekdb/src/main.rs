//! SeekDB MCP Server
//!
//! A Rust MCP Server providing vector search functionality for SeekDB.
//! Uses client-side embeddings (fastembed) similar to pyseekdb's DefaultEmbeddingFunction.

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use mysql_async::{Pool, Row, prelude::*};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use tracing::{info, error};

mod config;
mod db;
mod embeddings;

use config::ServerConfig;
use embeddings::{EmbeddingService, EMBEDDING_DIM};

/// Search result document
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchResult {
    pub id: String,
    pub document: String,
    pub metadata: Option<serde_json::Value>,
    pub distance: f64,
}

/// Tool response for search operations
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchResponse {
    pub success: bool,
    pub collection_name: String,
    pub results: Vec<SearchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Tool parameters for search_collection
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchCollectionParams {
    /// Name of the collection to search
    pub collection_name: String,
    /// Query text to search for
    pub query: String,
    /// Maximum number of results to return (default: 5)
    #[serde(default = "default_n_results")]
    pub n_results: i32,
}

fn default_n_results() -> i32 {
    5
}

/// Tool parameters for collection_info
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CollectionInfoParams {
    /// Name of the collection to get info for
    pub collection_name: String,
}

/// Tool parameters for add_documents
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddDocumentsParams {
    /// Name of the collection to add documents to
    pub collection_name: String,
    /// List of documents to add
    pub documents: Vec<DocumentInput>,
}

/// A document to be added to a collection
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocumentInput {
    /// Unique identifier for the document
    pub id: String,
    /// Document text content
    pub document: String,
    /// Optional metadata as JSON
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// SeekDB MCP Server handler
#[derive(Clone)]
pub struct SeekDbServer {
    pool: Arc<Pool>,
    embeddings: EmbeddingService,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SeekDbServer {
    /// Create a new SeekDB server instance
    pub fn new(pool: Pool, embeddings: EmbeddingService) -> Self {
        Self {
            pool: Arc::new(pool),
            embeddings,
            tool_router: Self::tool_router(),
        }
    }

    /// Search documents in a SeekDB collection using vector similarity
    #[tool(description = "Search documents in a SeekDB collection using vector similarity. \
        The query text is converted to a vector using the built-in embedding model (all-MiniLM-L6-v2), \
        then matched against stored document vectors using cosine distance.")]
    async fn search_collection(
        &self,
        Parameters(params): Parameters<SearchCollectionParams>,
    ) -> Result<CallToolResult, McpError> {
        let collection_name = params.collection_name;
        let query = params.query;
        let n_results = params.n_results;
        
        info!(
            "Searching collection '{}' with query '{}' (n_results={})",
            collection_name, query, n_results
        );

        // Generate query embedding client-side
        let query_embedding = self.embeddings.embed_one(&query).await.map_err(|e| {
            error!("Failed to generate query embedding: {}", e);
            McpError::internal_error(format!("Embedding generation failed: {}", e), None)
        })?;
        
        let query_vector = EmbeddingService::format_for_sql(&query_embedding);

        // Get a connection from the pool
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            error!("Failed to get database connection: {}", e);
            McpError::internal_error(format!("Database connection error: {}", e), None)
        })?;

        // Build the vector search query using pre-computed embedding
        let sql = format!(
            r#"SELECT id, document, metadata, 
                      COSINE_DISTANCE(embedding, '{}') as distance
               FROM {}
               ORDER BY distance ASC
               APPROXIMATE
               LIMIT ?"#,
            query_vector, collection_name
        );

        // Execute the query
        let results: Vec<Row> = conn
            .exec(&sql, (n_results,))
            .await
            .map_err(|e| {
                error!("Query execution failed: {}", e);
                McpError::internal_error(format!("Query failed: {}", e), None)
            })?;

        // Format results
        let search_results: Vec<SearchResult> = results
            .into_iter()
            .map(|row| {
                let id: String = row.get(0).unwrap_or_default();
                let document: String = row.get(1).unwrap_or_default();
                let metadata: Option<String> = row.get(2);
                let distance: f64 = row.get(3).unwrap_or(0.0);
                SearchResult {
                    id,
                    document,
                    metadata: metadata.and_then(|m| serde_json::from_str(&m).ok()),
                    distance,
                }
            })
            .collect();

        let response = SearchResponse {
            success: true,
            collection_name,
            results: search_results,
            error: None,
        };

        let json = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::internal_error(format!("JSON serialization error: {}", e), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Add documents to a collection with automatic embedding generation
    #[tool(description = "Add documents to a SeekDB collection. \
        Embeddings are automatically generated using the built-in embedding model (all-MiniLM-L6-v2).")]
    async fn add_documents(
        &self,
        Parameters(params): Parameters<AddDocumentsParams>,
    ) -> Result<CallToolResult, McpError> {
        let collection_name = params.collection_name;
        let documents = params.documents;
        
        info!(
            "Adding {} documents to collection '{}'",
            documents.len(), collection_name
        );

        // Generate embeddings for all documents
        let texts: Vec<&str> = documents.iter().map(|d| d.document.as_str()).collect();
        let embeddings = self.embeddings.embed_many(texts).await.map_err(|e| {
            error!("Failed to generate embeddings: {}", e);
            McpError::internal_error(format!("Embedding generation failed: {}", e), None)
        })?;

        // Get a connection from the pool
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            error!("Failed to get database connection: {}", e);
            McpError::internal_error(format!("Database connection error: {}", e), None)
        })?;

        // Insert documents with embeddings
        let mut inserted = 0;
        for (doc, embedding) in documents.iter().zip(embeddings.iter()) {
            let embedding_sql = EmbeddingService::format_for_sql(embedding);
            let metadata_json = doc.metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());

            let sql = format!(
                r#"INSERT INTO {} (id, document, embedding, metadata) 
                   VALUES (?, ?, '{}', ?)
                   ON DUPLICATE KEY UPDATE 
                   document = VALUES(document),
                   embedding = VALUES(embedding),
                   metadata = VALUES(metadata)"#,
                collection_name, embedding_sql
            );

            conn.exec_drop(&sql, (&doc.id, &doc.document, &metadata_json))
                .await
                .map_err(|e| {
                    error!("Failed to insert document '{}': {}", doc.id, e);
                    McpError::internal_error(format!("Insert failed for '{}': {}", doc.id, e), None)
                })?;
            
            inserted += 1;
        }

        let response = serde_json::json!({
            "success": true,
            "collection_name": collection_name,
            "documents_added": inserted
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    /// Create a new collection with vector index
    #[tool(description = "Create a new collection (table) with vector index for storing documents. \
        Uses 384-dimensional vectors (all-MiniLM-L6-v2 embedding model).")]
    async fn create_collection(
        &self,
        Parameters(params): Parameters<CollectionInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let collection_name = params.collection_name;
        
        info!("Creating collection '{}'", collection_name);

        let mut conn = self.pool.get_conn().await.map_err(|e| {
            error!("Failed to get database connection: {}", e);
            McpError::internal_error(format!("Database connection error: {}", e), None)
        })?;

        // Create table with vector column
        let sql = format!(
            r#"CREATE TABLE IF NOT EXISTS {} (
                id VARCHAR(255) PRIMARY KEY,
                document TEXT NOT NULL,
                embedding VECTOR({}) NOT NULL,
                metadata JSON,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                VECTOR INDEX idx_embedding(embedding) WITH (distance=cosine, type=hnsw)
            )"#,
            collection_name, EMBEDDING_DIM
        );

        conn.query_drop(&sql).await.map_err(|e| {
            error!("Failed to create collection: {}", e);
            McpError::internal_error(format!("Create collection failed: {}", e), None)
        })?;

        let response = serde_json::json!({
            "success": true,
            "collection_name": collection_name,
            "embedding_dimension": EMBEDDING_DIM,
            "message": format!("Collection '{}' created successfully", collection_name)
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    /// List all collections (tables with vector columns) in the database
    #[tool(description = "List all collections (tables with vector indexes) in the SeekDB database")]
    async fn list_collections(&self) -> Result<CallToolResult, McpError> {
        info!("Listing collections");

        let mut conn = self.pool.get_conn().await.map_err(|e| {
            error!("Failed to get database connection: {}", e);
            McpError::internal_error(format!("Database connection error: {}", e), None)
        })?;

        // Query for tables that have vector indexes
        let sql = r#"
            SELECT DISTINCT TABLE_NAME 
            FROM INFORMATION_SCHEMA.STATISTICS 
            WHERE INDEX_TYPE = 'VECTOR'
            AND TABLE_SCHEMA = DATABASE()
        "#;

        let rows: Vec<Row> = conn.query(sql).await.map_err(|e| {
            error!("Failed to list collections: {}", e);
            McpError::internal_error(format!("Query failed: {}", e), None)
        })?;

        let collections: Vec<String> = rows
            .into_iter()
            .filter_map(|row| row.get(0))
            .collect();

        let response = serde_json::json!({
            "success": true,
            "collections": collections,
            "count": collections.len()
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    /// Get information about a specific collection
    #[tool(description = "Get information about a specific collection including row count and schema")]
    async fn collection_info(
        &self,
        Parameters(params): Parameters<CollectionInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let collection_name = params.collection_name;
        info!("Getting info for collection '{}'", collection_name);

        let mut conn = self.pool.get_conn().await.map_err(|e| {
            error!("Failed to get database connection: {}", e);
            McpError::internal_error(format!("Database connection error: {}", e), None)
        })?;

        // Get row count
        let count_sql = format!("SELECT COUNT(*) FROM {}", collection_name);
        let rows: Vec<Row> = conn.query(&count_sql).await.map_err(|e| {
            error!("Failed to get row count: {}", e);
            McpError::internal_error(format!("Query failed: {}", e), None)
        })?;
        let count: i64 = rows.first().and_then(|r| r.get(0)).unwrap_or(0);

        // Get column info
        let schema_sql = format!(
            "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = '{}' AND TABLE_SCHEMA = DATABASE()",
            collection_name
        );
        let schema_rows: Vec<Row> = conn.query(&schema_sql).await.map_err(|e| {
            error!("Failed to get schema: {}", e);
            McpError::internal_error(format!("Query failed: {}", e), None)
        })?;

        let columns: Vec<serde_json::Value> = schema_rows
            .into_iter()
            .map(|row| {
                let name: String = row.get(0).unwrap_or_default();
                let dtype: String = row.get(1).unwrap_or_default();
                serde_json::json!({ "name": name, "type": dtype })
            })
            .collect();

        let response = serde_json::json!({
            "success": true,
            "collection_name": collection_name,
            "row_count": count,
            "embedding_dimension": EMBEDDING_DIM,
            "embedding_model": self.embeddings.model_name(),
            "columns": columns
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for SeekDbServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "SeekDB MCP Server - Provides vector similarity search for SeekDB collections. \
                 Uses built-in embedding model (all-MiniLM-L6-v2, 384 dimensions) for automatic \
                 embedding generation. No external API keys required."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8000";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,seekdb_mcp_server=debug".into()),
        )
        .init();

    // Load configuration from environment
    dotenvy::dotenv().ok();
    let config = ServerConfig::from_env()?;
    
    info!("SeekDB MCP Server starting...");
    info!("Connecting to SeekDB at {}:{}", config.host, config.port);

    // Initialize embedding service (downloads model on first run)
    info!("Initializing embedding model...");
    let embeddings = EmbeddingService::new()?;
    info!("Using embedding model: {} ({} dimensions)", 
        embeddings.model_name(), EMBEDDING_DIM);

    // Create database connection pool
    let pool = db::create_pool(&config)?;

    // Test connection
    {
        let mut conn = pool.get_conn().await?;
        let _: Option<i32> = conn.query_first("SELECT 1").await?;
        info!("Database connection successful");
    }

    // Create cancellation token for graceful shutdown
    let ct = tokio_util::sync::CancellationToken::new();

    // Create MCP service
    let service = StreamableHttpService::new(
        {
            let pool = pool.clone();
            let embeddings = embeddings.clone();
            move || Ok(SeekDbServer::new(pool.clone(), embeddings.clone()))
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            ..Default::default()
        },
    );

    // Create router with MCP endpoint
    let router = Router::new().nest_service("/mcp", service);

    let bind_address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_string());
    let tcp_listener = tokio::net::TcpListener::bind(&bind_address).await?;
    
    info!("MCP Server listening on http://{}/mcp", bind_address);

    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.unwrap();
            info!("Shutdown signal received");
            ct.cancel();
        })
        .await?;

    Ok(())
}
