# SeekDB MCP example

This example connects EchoKit Server to the official
[`seekdb-mcp-server`](https://github.com/oceanbase/awesome-oceanbase-mcp/tree/main/src/seekdb_mcp_server)
over Streamable HTTP.

## Start the SeekDB MCP server

1. Copy `.env.example` to `.env` in the directory where the MCP server will
   run, then fill in the SeekDB database connection values.
2. Start the MCP server:

   ```bash
   source .env
   uvx seekdb-mcp-server --transport streamable-http --host 127.0.0.1 --port 6000
   ```

The MCP endpoint is `http://127.0.0.1:6000/mcp`. The SeekDB database endpoint
uses port `2881` by default; it is not the MCP endpoint.

## Configure EchoKit

Use `config.toml` as the starting point for EchoKit Server. Replace the
`YOUR_API_KEY_HERE` values with credentials for the selected ASR, TTS, and LLM
services. Keep the SeekDB database credentials in the MCP server environment,
not in this EchoKit configuration file.
