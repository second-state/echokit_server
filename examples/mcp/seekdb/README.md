# SeekDB MCP Server (Rust)

A Rust implementation of MCP (Model Context Protocol) server for SeekDB vector search, designed for EchoKit voice AI platform integration (Issue #34).

## Features

- **Client-side embeddings**: Uses `fastembed` (all-MiniLM-L6-v2, 384 dims) - no API keys needed
- **Three MCP tools**: `search_collection`, `list_collections`, `collection_info`
- **rmcp 0.14.0**: Latest stable MCP SDK from crates.io
- **MySQL protocol**: Compatible with SeekDB via `mysql_async`

## Quick Start

### 1. Start SeekDB

```bash
docker run -d --name seekdb -p 2881:2881 oceanbase/seekdb:latest
```

### 2. Build & Run MCP Server

```bash
cd examples/mcp/seekdb
cargo build --release

# Start server
SEEKDB_HOST=127.0.0.1 \
SEEKDB_PORT=2881 \
SEEKDB_DATABASE=test \
./target/release/seekdb-mcp-server
```

Server listens on `http://localhost:8000/mcp`

### 3. Configure EchoKit

Add to your EchoKit `config.toml`:

```toml
[[llm.mcp_server]]
server = "http://localhost:8000/mcp"
type = "http_streamable"
call_mcp_message = "Searching knowledge base..."
```

## MCP Tools

| Tool | Parameters | Description |
|------|------------|-------------|
| `create_collection` | `collection_name` | Create table with 384-dim HNSW vector index |
| `add_documents` | `collection_name`, `documents[]` | Insert documents with auto-generated embeddings |
| `search_collection` | `collection_name`, `query`, `n_results` | Vector similarity search |
| `list_collections` | - | List tables with vector indexes |
| `collection_info` | `collection_name` | Get schema and row count |

## Testing

### Unit Tests

```bash
# Note: --test-threads=1 required due to fastembed model initialization
cargo test -- --test-threads=1
```

**Why single-threaded?** The fastembed model download and initialization use file locking that conflicts when tests run in parallel.

### Manual Testing with curl

The MCP server uses session-based HTTP Streamable transport. Each session requires:
1. **Initialize** - Get session ID from response header
2. **Use session ID** - Include `mcp-session-id` header in subsequent requests

#### Step 1: Start the Server

```bash
RUST_LOG=info SEEKDB_DATABASE=test_mcp SEEKDB_USER=root \
  ./target/release/seekdb-mcp-server
```

**Expected output:**
```
2026-01-29T16:08:57.359807Z  INFO seekdb_mcp_server: SeekDB MCP Server starting...
2026-01-29T16:08:57.359847Z  INFO seekdb_mcp_server: Connecting to SeekDB at 127.0.0.1:2881
2026-01-29T16:08:57.359852Z  INFO seekdb_mcp_server: Initializing embedding model...
2026-01-29T16:08:57.695229Z  INFO seekdb_mcp_server::embeddings: Embedding model initialized successfully
2026-01-29T16:08:57.695553Z  INFO seekdb_mcp_server: Using embedding model: AllMiniLML6V2 (384 dimensions)
2026-01-29T16:08:57.705735Z  INFO seekdb_mcp_server: Database connection successful
2026-01-29T16:08:57.705917Z  INFO seekdb_mcp_server: MCP Server listening on http://127.0.0.1:8000/mcp
```

#### Step 2: Initialize Session and Get Session ID

```bash
curl -v http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
    "protocolVersion":"2024-11-05",
    "capabilities":{},
    "clientInfo":{"name":"test","version":"1.0"}
  }}'
```

**Response header contains session ID:**
```
< mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77
```

**Response body (SSE format):**
```
data: {"jsonrpc":"2.0","id":1,"result":{
  "protocolVersion":"2024-11-05",
  "capabilities":{"tools":{}},
  "serverInfo":{"name":"rmcp","version":"0.14.0"},
  "instructions":"SeekDB MCP Server - Provides vector similarity search..."
}}
```

#### Step 3: List Available Tools

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

**Response:** Returns 5 tools with JSON schemas.

#### Step 4: Create a Collection

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
    "name":"create_collection",
    "arguments":{"collection_name":"test_docs"}
  }}'
```

**Response:**
```json
{"success": true, "collection_name": "test_docs", "embedding_dimension": 384}
```

#### Step 5: Add Documents

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77" \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
    "name":"add_documents",
    "arguments":{
      "collection_name":"test_docs",
      "documents":[
        {"id":"doc1","document":"Rust is a systems programming language."},
        {"id":"doc2","document":"Python is popular for data science."},
        {"id":"doc3","document":"SeekDB is an AI-native database with vector search."}
      ]
    }
  }}'
```

**Response:**
```json
{"success": true, "collection_name": "test_docs", "documents_added": 3}
```

#### Step 6: Search Documents

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77" \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
    "name":"search_collection",
    "arguments":{"collection_name":"test_docs","query":"vector database for AI","n_results":3}
  }}'
```

**Response:** (semantic search finds most relevant document first)
```json
{
  "success": true,
  "collection_name": "test_docs",
  "results": [
    {"id": "doc3", "document": "SeekDB is an AI-native database with vector search.", "distance": 0.29},
    {"id": "doc2", "document": "Python is popular for data science.", "distance": 0.66},
    {"id": "doc1", "document": "Rust is a systems programming language.", "distance": 0.79}
  ]
}
```

#### Step 7: Get Collection Info

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: c616654b-7c59-4251-973f-3590820e3f77" \
  -d '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
    "name":"collection_info",
    "arguments":{"collection_name":"test_docs"}
  }}'
```

**Response:**
```json
{
  "success": true,
  "collection_name": "test_docs",
  "row_count": 3,
  "embedding_dimension": 384,
  "embedding_model": "AllMiniLML6V2",
  "columns": [
    {"name": "id", "type": "varchar"},
    {"name": "document", "type": "text"},
    {"name": "embedding", "type": "VECTOR(384)"},
    {"name": "metadata", "type": "json"},
    {"name": "created_at", "type": "timestamp"}
  ]
}
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SEEKDB_HOST` | `127.0.0.1` | SeekDB server address |
| `SEEKDB_PORT` | `2881` | SeekDB port |
| `SEEKDB_USER` | `root` | Database username |
| `SEEKDB_PASSWORD` | `` | Database password |
| `SEEKDB_DATABASE` | `test` | Database name |
| `BIND_ADDRESS` | `127.0.0.1:8000` | MCP server bind address |

## Architecture

```
EchoKit Server  ←MCP Protocol→  seekdb-mcp-server  ←MySQL→  SeekDB
(rmcp client)    (http_streamable)   (rmcp server)           (Docker)
```

## Alternative Approach: Database-Side Embeddings

This implementation uses **client-side embeddings** for simplicity. SeekDB also supports **database-side embeddings** via `AI_EMBED()` SQL function with registered AI models (OpenAI, SiliconFlow, etc.).

**Setup AI_EMBED():**
```sql
-- Register model
CALL DBMS_AI_SERVICE.CREATE_AI_MODEL('ob_embed', '{"type": "dense_embedding"}');

-- Add endpoint with API key
CALL DBMS_AI_MODEL_ENDPOINT('ob_embed_endpoint', '{
  "ai_model_name": "ob_embed",
  "url": "https://api.siliconflow.cn/v1/embeddings",
  "access_key": "sk-YOUR_KEY",
  "provider": "siliconflow"
}');

-- Use in queries
SELECT * FROM collection 
WHERE COSINE_DISTANCE(embedding, AI_EMBED('ob_embed', 'query')) < 0.5;
```

See [SeekDB AI Functions docs](https://www.oceanbase.ai/docs/ai-function) for complete examples.

## Resources

- [SeekDB Docs](https://www.oceanbase.ai/docs/)
- [rmcp SDK](https://docs.rs/rmcp/0.14.0/)
- [MCP Specification](https://modelcontextprotocol.io/)
