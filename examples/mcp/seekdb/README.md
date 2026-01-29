# SeekDB MCP Server

A Rust MCP server for SeekDB vector search. It connects to SeekDB, receives query strings, performs vector similarity search, and returns relevant text segments.

**Key Features:**
- Uses `fastembed` (all-MiniLM-L6-v2, 384 dimensions) for client-side embeddings — no API keys needed
- Compatible with [pyseekdb](https://github.com/oceanbase/pyseekdb) embedding behavior
- Built with [rmcp 0.14.0](https://docs.rs/rmcp/0.14.0/) — the latest stable MCP SDK

## Quick Start

### 1. Start SeekDB

SeekDB runs as a Docker container. For first-time setup:

```bash
# On Linux or MacOS
mkdir -p seekdb
docker run -d -p 2881:2881 -p 2886:2886 \
  -v $PWD/seekdb:/var/lib/oceanbase \
  --name seekdb oceanbase/seekdb

# Or:
# On Windows
docker volume create seekdb
docker run -d -p 2881:2881 -p 2886:2886 -v seekdb:/var/lib/oceanbase --name seekdb oceanbase/seekdb
```
Please refer to https://github.com/oceanbase/docker-images/blob/main/seekdb/README.md .

After the first run, use these commands to manage SeekDB:

```bash

# Start existing container
docker start seekdb

# Wait for SeekDB to initialize (check logs for "Initialization complete"):
docker logs seekdb --tail 10
# Expected: "Initialization complete."
#           "Starting observer health check..."

# Stop container
docker stop seekdb
```

### 2. Build & Run MCP Server

```bash
cd examples/mcp/seekdb
cargo build --release
```

Create the database first:

```bash
docker exec seekdb mysql -h127.0.0.1 -P2881 -uroot -e "CREATE DATABASE IF NOT EXISTS test_mcp"
```

Start the MCP server in a separate terminal:

```bash
RUST_LOG=info \
SEEKDB_DATABASE=test_mcp \
./target/release/seekdb-mcp-server
```

Expected output:

```
INFO seekdb_mcp_server: SeekDB MCP Server starting...
INFO seekdb_mcp_server: Connecting to SeekDB at 127.0.0.1:2881
INFO seekdb_mcp_server: Initializing embedding model...
INFO seekdb_mcp_server::embeddings: Embedding model initialized successfully
INFO seekdb_mcp_server: Using embedding model: AllMiniLML6V2 (384 dimensions)
INFO seekdb_mcp_server: Database connection successful
INFO seekdb_mcp_server: MCP Server listening on http://127.0.0.1:8000/mcp
```

### 3. Configure EchoKit (Optional)

To use this MCP server with the main EchoKit server, add this to EchoKit configuration file `config.toml`:

```toml
[[llm.mcp_server]]
server = "http://localhost:8000/mcp"
type = "http_streamable"
call_mcp_message = "Searching knowledge base..."
```

See [examples/mcp/config.toml](../config.toml) for an example.

## MCP Tools

This server exposes 5 tools via the MCP protocol:

| Tool | Parameters | Description |
|------|------------|-------------|
| `create_collection` | `collection_name` | Create a table with 384-dim HNSW vector index |
| `add_documents` | `collection_name`, `documents[]` | Insert documents with auto-generated embeddings |
| `search_collection` | `collection_name`, `query`, `n_results` | Perform vector similarity search |
| `list_collections` | — | List all tables with vector indexes |
| `collection_info` | `collection_name` | Get schema, row count, and embedding info |

## Usage Guide

This section walks through using the MCP server with `curl`. The MCP protocol uses **session-based HTTP Streamable transport**, which requires:

1. **Initialize** — Start a session and get a session ID from the response header
2. **Send `notifications/initialized`** — Required before any tool calls
3. **Use session ID** — Include `mcp-session-id` header in all subsequent requests

### Step 1: Initialize Session

**Note:** If you're testing multiple times, first kill any existing server instances:
```bash
pkill -f seekdb-mcp-server
```

In another terminal than the MCP server, start a session and capture the session ID:

```bash
SESSION_ID=$(curl -s -D - http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
    "protocolVersion":"2024-11-05",
    "capabilities":{},
    "clientInfo":{"name":"curl-test","version":"1.0"}
  }}' 2>/dev/null | grep -i "mcp-session-id" | cut -d: -f2 | tr -d ' \r\n')

echo "Session ID: $SESSION_ID"
```

**Note:** The session ID is unique for each session and varies every time.
It's recommended to keep using the same terminal for all subsequent commands (Steps 2-7) so the `$SESSION_ID` variable remains available.

Expected output:
```
Session ID: cd64e4e3-6dc4-4494-bcc0-bfaab99de935
```

The response body is in SSE format and contains:

```json
{
  "jsonrpc":"2.0",
  "id":1,
  "result":{
    "protocolVersion":"2024-11-05",
    "capabilities":{"tools":{}},
    "serverInfo":{"name":"rmcp","version":"0.14.0"},
    "instructions":"SeekDB MCP Server - Provides vector similarity search..."
  }
}
```

### Step 2: Send Initialized Notification

**Important:** The MCP protocol requires sending a `notifications/initialized` message before any other requests. This tells the server that the client is ready.

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
```

This returns an empty response (202 Accepted). Now you can make tool calls.

### Step 3: List Available Tools

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

Response includes all 5 tools: `create_collection`, `add_documents`, `search_collection`, `list_collections`, `collection_info`.

### Step 4: Create a Collection

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
    "name":"create_collection",
    "arguments":{"collection_name":"my_docs"}
  }}'
```

Response:

```json
{
  "success": true,
  "collection_name": "my_docs",
  "embedding_dimension": 384,
  "message": "Collection 'my_docs' created successfully"
}
```

### Step 5: Add Documents

Documents are automatically embedded using the all-MiniLM-L6-v2 model:

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
    "name":"add_documents",
    "arguments":{
      "collection_name":"my_docs",
      "documents":[
        {"id":"doc1","document":"Rust is a systems programming language focused on safety and performance."},
        {"id":"doc2","document":"Python is popular for data science and machine learning."},
        {"id":"doc3","document":"SeekDB is an AI-native database with vector search capabilities."}
      ]
    }
  }}'
```

Response:

```json
{"success": true, "collection_name": "my_docs", "documents_added": 3}
```

### Step 6: Search Documents

Semantic search finds documents by meaning, not just keywords:

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
    "name":"search_collection",
    "arguments":{"collection_name":"my_docs","query":"vector database for AI","n_results":3}
  }}'
```

Response (ordered by relevance — lower distance = more similar):

```json
{
  "success": true,
  "collection_name": "my_docs",
  "results": [
    {"id": "doc3", "document": "SeekDB is an AI-native database with vector search capabilities.", "distance": 0.29},
    {"id": "doc2", "document": "Python is popular for data science and machine learning.", "distance": 0.64},
    {"id": "doc1", "document": "Rust is a systems programming language focused on safety and performance.", "distance": 0.79}
  ]
}
```

### Step 7: Get Collection Info

```bash
curl -s http://127.0.0.1:8000/mcp -X POST \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
    "name":"collection_info",
    "arguments":{"collection_name":"my_docs"}
  }}'
```

Response:

```json
{
  "success": true,
  "collection_name": "my_docs",
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

## Testing

### Unit Tests

```bash
cd examples/mcp/seekdb
cargo test -- --test-threads=1
```

**Note:** The `--test-threads=1` flag is required because fastembed's model initialization uses file locking that conflicts when tests run in parallel.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SEEKDB_HOST` | `127.0.0.1` | SeekDB server address |
| `SEEKDB_PORT` | `2881` | SeekDB port |
| `SEEKDB_USER` | `root` | Database username |
| `SEEKDB_PASSWORD` | (empty) | Database password |
| `SEEKDB_DATABASE` | — | Database name (required) |
| `BIND_ADDRESS` | `127.0.0.1:8000` | MCP server bind address |
| `RUST_LOG` | — | Log level (`info`, `debug`, `trace`) |

## Architecture

```
┌─────────────────┐     MCP Protocol      ┌───────────────────┐     MySQL      ┌─────────┐
│  EchoKit Server │ ←───────────────────→ │ seekdb-mcp-server │ ←────────────→ │ SeekDB  │
│  (rmcp client)  │   http_streamable     │   (rmcp server)   │   port 2881    │ (Docker)│
└─────────────────┘                       └───────────────────┘                └─────────┘
```

## Alternative: Database-Side Embeddings

This implementation uses **client-side embeddings** for simplicity (no API keys needed). SeekDB also supports **database-side embeddings** via the `AI_EMBED()` SQL function with external AI providers (OpenAI, SiliconFlow, etc.).

See [Use the AI_EMBED function](https://www.oceanbase.ai/docs/ai-embed-function) for details.

## Resources

- [SeekDB Documentation](https://www.oceanbase.ai/docs/)
- [rmcp SDK Documentation](https://docs.rs/rmcp/0.14.0/)
- [MCP Specification](https://modelcontextprotocol.io/)
