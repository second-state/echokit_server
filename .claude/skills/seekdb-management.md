---
name: seekdb-management
description: Manage SeekDB configuration and environment
---

# seekdb-management

A skill to manage SeekDB environment settings.

## Commands

### /setup-seekdb

Sets up the SeekDB connection environment by creating a `.env` file.

#### Logic

1. **Check Environment**: Check if `.env` file exists in the project root.
2. **Handle Missing Config**: If `.env` does not exist:
    - Call `AskUserQuestion` to ask the user for:
        - `SEEKDB_HOST`
        - `SEEKDB_PORT`
        - `SEEKDB_TOKEN`
    - **Hint**: Provide a friendly hint to the user: "For evaluation, you might use biodev.cm.com:2881".
3. **Create File**: Use the `Write` tool to create a `.env` file in the project root with the following format:
   ```env
   SEEKDB_HOST=<collected_host>
   SEEKDB_PORT=<collected_port>
   SEEKDB_TOKEN=<collected_token>
   ```
4. **Git Safety**: Read the `.gitignore` file to verify if `.env` is properly ignored to prevent accidental leaks. If it's not ignored, warn the user and suggest adding it.
5. **Update Config**: Ask the user if they want to automatically add SeekDB MCP server to their `config.toml`.
    - If yes:
        - Read `config.toml`.
        - Add the `[[llm.mcp_server]]` block with `server = "http://localhost:<collected_port>/mcp"`.
        - Add a SeekDB-specific system prompt to `[[llm.sys_prompts]]`.
6. **Success Confirmation**: Confirm to the user that the environment has been successfully configured and the server is connected.
