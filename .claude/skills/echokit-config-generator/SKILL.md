---
name: EchoKit Config Generator
description: Generate config.toml for EchoKit servers with interactive setup for ASR, TTS, LLM services, MCP servers, API key entry, and server launch
version: 1.3.1
author: "EchoKit Config Generator Contributors"
repository: "https://github.com/second-state/echokit_server"
---

# EchoKit Config Generator

## Overview

This SKILL generates `config.toml` files for EchoKit servers through an interactive five-phase process that includes configuration generation, API key entry, and server launch.

**Announce at start:** "I'm using the EchoKit Config Generator to create your config.toml."

## Handling User Input

Throughout this SKILL, when asking questions with default values:

**Always phrase it as:** *"Question? (default: {VALUE})"*

**Handle responses:**
- **Empty response or Enter** → Use the default value
- **User provides value** → Use user's value
- **User enters "default"** → Use the default value explicitly

**Example:**
```
AI: How many messages should it remember? (default: 5)
User: [Enter]
AI: [Uses 5]

AI: How many messages should it remember? (default: 5)
User: 10
AI: [Uses 10]
```

This applies to ALL questions with defaults throughout the SKILL.

## Phase 1: Assistant Definition

Ask these questions **one at a time**:

1. *"What is your AI assistant's primary purpose? (Describe in 1-2 sentences)"*
2. *"What tone should it have? (professional, casual, friendly, expert, or describe your own)"*
3. *"What specific capabilities should it have?"*
   - Prompt with examples if needed: "code generation", "data analysis", "creative writing", "problem-solving", "teaching", etc.
4. *"Any response format requirements?"*
   - Examples: "short answers", "detailed explanations", "step-by-step", "conversational", "formal reports"
5. *"Any domain-specific knowledge areas?"* (Optional)
   - Examples: "programming", "medicine", "law", "finance", etc.
6. *"Any constraints or guidelines?"* (Optional)
   - Examples: "no bullet points", "always cite sources", "avoid jargon", "max 3 sentences"
7. *"Any additional instructions or preferences?"* (Optional)

**Generate sophisticated system prompt** using this structure:

```toml
[[llm.sys_prompts]]
role = "system"
content = """
You are a {TONE} AI assistant specialized in {PURPOSE}.

## Core Purpose
{PURPOSE_EXPANDED - elaborate based on user input}

## Your Capabilities
- List the capabilities provided by user
- Each capability on its own line
- Be specific about what you can do

## Response Style
{FORMAT_REQUIREMENTS}

## Domain Knowledge
{DOMAIN_KNOWLEDGE if provided, otherwise omit this section}

## Behavioral Guidelines
{BEHAVIORS_FROM_USER}
- Add any relevant default behaviors based on tone

## Constraints
{CONSTRAINTS_FROM_USER}
- Always maintain {TONE} tone
- {RESPONSE_FORMAT_RULES}

## Additional Instructions
{ADDITIONAL_NOTES if provided}

---
Remember: Stay in character as a {TONE} {DOMAIN} assistant. Always prioritize helpfulness and accuracy.
"""
```

**Enhanced defaults based on tone:**

If user doesn't provide specific behaviors, use these expanded defaults:

- **professional:**
  - Provide accurate, well-researched information
  - Maintain formal, business-appropriate language
  - Acknowledge limitations and uncertainty
  - Structure responses logically

- **casual:**
  - Be conversational and engaging
  - Use natural, relaxed language
  - Show personality when appropriate
  - Keep interactions friendly and approachable

- **friendly:**
  - Be warm and welcoming
  - Use simple, clear language
  - Show empathy and understanding
  - Make users feel comfortable

- **expert:**
  - Provide comprehensive technical details
  - Cite sources and references when relevant
  - Explain trade-offs and alternatives
  - Use appropriate terminology correctly
  - Acknowledge edge cases and limitations

## Phase 2: Platform Selection

For each service category (ASR, TTS, LLM):

1. **Read platform data** from `platforms/{category}.yml` using the Read tool
2. **Display available options** with this format:

```
Available {SERVICE} Services:

1. {PLATFORM_1.name}
   URL: {PLATFORM_1.url}
   Model: {PLATFORM_1.model}
   Get API key: {PLATFORM_1.api_key_url}
   Notes: {PLATFORM_1.notes}

2. {PLATFORM_2.name}
   URL: {PLATFORM_2.url}
   Model: {PLATFORM_2.model}
   Get API key: {PLATFORM_2.api_key_url}
   Notes: {PLATFORM_2.notes}

C. Custom - Specify your own platform/model

Your choice (1-{N} or C):
```

3. **User selection:**

   **If user selects a number (1-{N}):**
   - Store the selected platform data from the YAML file
   - Continue to next service

   **If user selects 'C' (Custom):**

   **Step 1: Get platform name**
   - Ask: *"What's the platform name?"* (e.g., "groq", "deepseek", "mistral", "together")

   **Step 2: Auto-fetch API information**
   - Use WebSearch to find API documentation
   - Search query: `"{PLATFORM_NAME} API endpoint {SERVICE_TYPE} 2025"`
     - For ASR: "speech to text API", "transcription API"
     - For TTS: "text to speech API"
     - For LLM: "chat completions API", "LLM API"
   - Extract from search results:
     - API endpoint URL
     - API documentation URL
     - Authentication method
     - Default model names

   **Step 3: Confirm with user**
   Display what was found:
   ```
   I found the following for {PLATFORM_NAME} {SERVICE}:

   API Endpoint: {FOUND_URL}
   Documentation: {FOUND_DOCS_URL}
   Authentication: {FOUND_AUTH_METHOD}
   Default Models: {FOUND_MODELS}

   Is this correct? (y/edit)
   ```

   **Step 4: Gather additional details**
   - Ask: *"What model should I use? (suggested: {FOUND_MODELS} or enter custom)"*
   - Ask for additional settings:
     - LLM: *"How many messages should it remember? (default: 5)"*
     - TTS: *"What voice should it use? (default: default)"*
     - ASR: *"What language? (default: en)"*

   **Step 5: Store custom platform**
   ```
   name: "{PLATFORM_NAME}"
   platform: "{INFERRED_TYPE from API docs or user}"
   url: "{CONFIRMED_URL}"
   model: "{USER_MODEL_CHOICE}"
   history/voice/lang: {USER_SETTINGS}
   api_key_url: "{FOUND_DOCS_URL}"
   notes: "Custom {PLATFORM_NAME} - auto-configured"
   ```

4. **Continue to next service**

**Load platforms in order:**
1. ASR from `platforms/asr.yml`
2. TTS from `platforms/tts.yml`
3. LLM from `platforms/llm.yml`

**Note on WebSearch:** Use WebSearch tool with year-specific queries (2025) to get current API information. For common platforms, you can also infer from patterns:
- OpenAI-compatible: `https://api.{platform}.com/v1/chat/completions`
- Anthropic-compatible: `https://api.{platform}.com/v1/messages`
- Together/Groq: OpenAI-compatible format

## Phase 3: MCP Server (Optional)

Ask: *"Do you need an MCP server? (y/n)"*

If yes, ask: *"What's your MCP server URL?"*

Default: `http://localhost:8000/mcp`

**Add MCP configuration to LLM section:**

The MCP server is configured within the LLM configuration as:

```toml
[[llm.mcp_server]]
server = "{USER_PROVIDED_URL or http://localhost:8000/mcp}"
type = "http_streamable"
call_mcp_message = "Please hold on a few seconds while I am searching for an answer!"
```

**Explain to user:**
- The MCP server will be added to the LLM section
- `type` can be: "http_streamable" or "http"
- `call_mcp_message` is shown to users when MCP is being called

## Phase 4: Generate Files

### Step 1: Preview config.toml

**IMPORTANT:** EchoKit server requires a specific TOML structure:
- Section order MUST be: `[tts]` → `[asr]` → `[llm]`
- No comments allowed at the beginning of the file
- Field names vary by platform (check platform-specific requirements)

Display complete configuration with this format:

```toml
addr = "0.0.0.0:8080"
hello_wav = "hello.wav"

[tts]
platform = "{SELECTED_TTS.platform}"
url = "{SELECTED_TTS.url}"
{TTS_API_KEY_FIELD} = "YOUR_API_KEY_HERE"
{TTS_MODEL_FIELD} = "{SELECTED_TTS.model}"
voice = "{SELECTED_TTS.voice}"

[asr]
platform = "{SELECTED_ASR.platform}"
url = "{SELECTED_ASR.url}"
{ASR_API_KEY_FIELD} = "YOUR_API_KEY_HERE"
model = "{SELECTED_ASR.model}"
lang = "{ASR_LANG}"
prompt = "Hello\\n你好\\n(noise)\\n(bgm)\\n(silence)\\n"
vad_url = "http://localhost:9093/v1/audio/vad"

[llm]
platform = "{SELECTED_LLM.platform}"
url = "{SELECTED_LLM.url}"
{LLM_API_KEY_FIELD} = "YOUR_API_KEY_HERE"
model = "{SELECTED_LLM.model}"
history = {SELECTED_LLM.history}

{GENERATED_SYSTEM_PROMPT}

{MCP_CONFIGURATION if enabled}
```

**Platform-specific field mappings:**

**TTS platforms:**
- `openai`: uses `api_key` and `model`
- `elevenlabs`: uses `token` and `model_id`
- `groq`: uses `api_key` and `model`

**ASR platforms:**
- `openai`/`whisper`: uses `api_key` and `model`

**LLM platforms:**
- `openai_chat`: uses `api_key` (optional, can be empty string)

When generating the config, replace `{TTS_API_KEY_FIELD}`, `{ASR_API_KEY_FIELD}`, and `{LLM_API_KEY_FIELD}` with the appropriate field name for the selected platform:
- For ElevenLabs TTS: use `token`
- For OpenAI/Groq TTS: use `api_key`
- For Whisper ASR: use `api_key`
- For OpenAI Chat LLM: use `api_key`

### Step 2: Ask for confirmation

*"Does this configuration look correct? (y/edit/regenerate)"*

- **y** - Proceed to write files
- **e** - Ask which section to edit (asr/tts/llm/system_prompt)
- **r** - Restart from Phase 1

### Step 3: Determine output location

Ask: *"Where should I save the config files? (press Enter for default: echokit_server/)"*

**Handle user input:**
- **Empty/Enter** → Use default: `echokit_server/`
- **Custom path** → Use user-provided path
- **Relative path** → Use as-is (e.g., `my_configs/`)
- **Absolute path** → Use as-is (e.g., `/Users/username/echokit/`)

**After path is determined:**
1. Check if directory exists
2. If not, ask: *"Directory '{OUTPUT_DIR}' doesn't exist. Create it? (y/n)"*
   - If **y**: Create with `mkdir -p {OUTPUT_DIR}`
   - If **n**: Ask for different path
3. Verify write permissions
   - Test by attempting to create a temporary file
   - If fails, ask for different location

### Step 4: Write files

Use the Write tool to create:

1. **`{OUTPUT_DIR}/config.toml`** - Main configuration (includes MCP server if enabled)
2. **`{OUTPUT_DIR}/SETUP_GUIDE.md`** - Setup instructions (use template from `templates/SETUP_GUIDE.md`)

### Step 5: Display success message

```
✓ Configuration generated successfully!

Files created:
  {OUTPUT_DIR}/config.toml
  {OUTPUT_DIR}/SETUP_GUIDE.md

Now proceeding to Phase 5: API Key Entry and Server Launch...
```

## Phase 5: API Key Entry and Server Launch

### Overview

This phase collects API keys from the user, updates the config.toml file, builds the server (if needed), and launches it.

### Step 1: Display API key URLs

Show the user where to get their API keys:

```
## Phase 5: API Key Entry and Server Launch

You'll need API keys for your selected services. Here's where to get them:

ASR ({SELECTED_ASR.name}):
  API Key URL: {SELECTED_ASR.api_key_url}

TTS ({SELECTED_TTS.name}):
  API Key URL: {SELECTED_TTS.api_key_url}

LLM ({SELECTED_LLM.name}):
  API Key URL: {SELECTED_LLM.api_key_url}

I'll prompt you for each key. You can also press Enter to skip and manually edit config.toml later.
```

### Step 2: Collect API keys

For each service (ASR, TTS, LLM), ask:

```
Enter your {SERVICE_NAME} API key:
(or press Enter to skip and add it manually later)
```

**Handle responses:**
- **User provides a key** → Store it for updating config.toml
- **Empty/Enter** → Keep "YOUR_API_KEY_HERE" placeholder

**Store keys temporarily** in memory:
```
ASR_API_KEY = "{user_input or 'YOUR_API_KEY_HERE'}"
TTS_API_KEY = "{user_input or 'YOUR_API_KEY_HERE'}"
LLM_API_KEY = "{user_input or 'YOUR_API_KEY_HERE'}"
```

### Step 3: Update config.toml with API keys

Read the existing config.toml using the Read tool, then replace the API key placeholders:

```
[asr]
api_key = "{ASR_API_KEY}"

[tts]
{CORRECT_TTS_FIELD} = "{TTS_API_KEY}"

[llm]
api_key = "{LLM_API_KEY}"
```

Use the Edit tool to update each API key field line in `{OUTPUT_DIR}/config.toml`.

**IMPORTANT - Platform-specific field names:**
- **ElevenLabs TTS:** uses `token` (not `api_key`) and `model_id` (not `model`)
- **OpenAI/Groq TTS:** uses `api_key` and `model`
- **Whisper ASR:** uses `api_key`
- **OpenAI Chat LLM:** uses `api_key`

Make sure to preserve the correct field names for each platform when editing!

### Step 4: Verify server installation

Check if the EchoKit server is already built:

1. Check if `{OUTPUT_DIR}/target/release/echokit_server` exists
2. If it exists, skip to Step 6

If not built, ask: *"The EchoKit server needs to be built. Do you want me to build it now? (y/n)"*

### Step 5: Build the server (if needed)

If user said **yes** in Step 4:

1. Change to the output directory: `cd {OUTPUT_DIR}`
2. Run: `cargo build --release`
3. Monitor build output for errors

**If build succeeds:**
```
✓ Server built successfully at {OUTPUT_DIR}/target/release/echokit_server
```

**If build fails:**
```
✗ Build failed. Please check the error messages above.

You can manually build the server later with:
  cd {OUTPUT_DIR} && cargo build --release

For now, your config.toml has been saved with your API keys.
```
- **End here** - don't proceed to server launch

### Step 6: Launch the server

If the server is available (either pre-built or just built):

Ask: *"Ready to launch the EchoKit server? (y/n)"*

If **no**:
```
Your config.toml has been saved with your API keys.

To run the server manually:
  cd {OUTPUT_DIR} && ./target/release/echokit_server
```
- **End here**

If **yes**:

1. Change to output directory: `cd {OUTPUT_DIR}`
2. Enable debug logging and launch server in background:
   ```bash
   export RUST_LOG=debug
   ./target/release/echokit_server &
   ```
3. Store the process ID for potential later management
4. **Get the actual local IP address** by running:
   ```bash
   # Try multiple methods to get IP, use first successful result
   ipconfig getifaddr en0 2>/dev/null || \
   ipconfig getifaddr en1 2>/dev/null || \
   hostname -I 2>/dev/null | awk '{print $1}' || \
   ifconfig | grep "inet " | grep -v 127.0.0.1 | awk '{print $2}' | head -1
   ```
5. Store the result as `LOCAL_IP`
6. Display success message with WebSocket URLs:

```
✓ EchoKit server is now running!

Server Details:
  Location: {OUTPUT_DIR}
  Config: {OUTPUT_DIR}/config.toml
  Bind Address: 0.0.0.0:8080

WebSocket URL: ws://{ACTUAL_LOCAL_IP}:8080/ws

You can connect to the server using any WebSocket client at this URL.

The server is running in the background.

To stop the server later:
  - Find the process: ps aux | grep echokit_server
  - Kill it: kill {PID}

Or if you have the PID: kill {SERVER_PID}
```

### Step 7: Verify server is running

After launching, verify the server started successfully:

1. Wait 2-3 seconds for startup
2. Check if the process is still running: `ps aux | grep echokit_server | grep -v grep`
3. Optionally test the endpoint: `curl -s http://localhost:8080/health || echo "Health check not available"`

**If server is running:**
```
✓ Server is running and responding!
```

**If server crashed:**
```
⚠ The server may have crashed. Check the logs above for errors.

You can try running it manually to see error messages:
  cd {OUTPUT_DIR} && ./target/release/echokit_server
```

### Error Handling

#### API key entry issues

If user enters an obviously invalid key (too short, contains spaces, etc.):
```
Warning: That doesn't look like a valid API key.
API keys are typically long strings without spaces.

Use this key anyway? (y/retry)
```

- **y** → Accept the key as entered
- **retry** → Ask again for that service's key

#### Build fails

If the build fails:
1. Show the error output
2. Don't proceed to server launch
3. Provide manual build instructions
4. Remind user that config.toml is saved with API keys

#### Server won't start

If the server process exits immediately:
1. Check for error messages in the output
2. Common issues:
   - Port 8080 already in use
   - Invalid config.toml syntax
   - Missing dependencies
   - Incorrect API keys

Provide troubleshooting suggestions based on error messages.

## File Locations

All files are relative to SKILL root:
- Platform data: `platforms/asr.yml`, `platforms/tts.yml`, `platforms/llm.yml`
- Templates: `templates/SETUP_GUIDE.md`
- Examples: `examples/voice-companion.toml`, `examples/coding-assistant.toml`

**No external dependencies** - this SKILL is completely self-contained.

## Error Handling

### Platform files not found

If a platform file is missing:
```
Error: Could not find platforms/{category}.yml

Please ensure the SKILL is properly installed with all platform data files.
```

### Invalid user input

If user enters invalid choice:
```
Invalid choice. Please enter a number between 1 and {N}.
```

### Cannot create output directory

If directory creation fails:
```
Error: Cannot create directory {OUTPUT_DIR}

Please choose a different location or create the directory manually.
```

### Cannot write files

If file write fails:
```
Error: Cannot write to {OUTPUT_DIR}/config.toml

Please check permissions and try again.
```

## Template Variables for SETUP_GUIDE.md

When generating SETUP_GUIDE.md, replace these variables:

- `{{TIMESTAMP}}` - Current date and time
- `{{ASSISTANT_TYPE}}` - User's description from Phase 1
- `{{ASR_NAME}}` - Selected ASR platform name
- `{{ASR_MODEL}}` - Selected ASR model
- `{{ASR_API_URL}}` - Selected ASR API key URL
- `{{TTS_NAME}}` - Selected TTS platform name
- `{{TTS_VOICE}}` - Selected TTS voice
- `{{TTS_API_URL}}` - Selected TTS API key URL
- `{{LLM_NAME}}` - Selected LLM platform name
- `{{LLM_MODEL}}` - Selected LLM model
- `{{LLM_API_URL}}` - Selected LLM API key URL
- `{{MCP_ENABLED}}` - "Yes" or "No" based on Phase 3

Read the template from `templates/SETUP_GUIDE.md`, replace variables, then write to output.

## Example Conversation

```
User: Generate an EchoKit config for a friendly voice assistant

AI: I'm using the EchoKit Config Generator to create your config.toml.

Phase 1: Assistant Definition

What is your AI assistant's primary purpose?
User: Daily conversational assistance and smart home control

What tone should it have?
User: Friendly

Any specific behaviors or requirements?
User: Keep responses short, show some personality

Phase 2: Platform Selection

Available ASR Services:

1. OpenAI Whisper
   URL: https://api.openai.com/v1/audio/transcriptions
   Model: gpt-4o-mini-transcribe
   Get API key: https://platform.openai.com/api-keys
   Notes: Best accuracy, requires OpenAI account with API access

2. Local Whisper
   URL: http://localhost:8080/transcribe
   Model: base
   Get API key: Not required (local deployment)
   Notes: Free and private, requires local whisper server running

Your choice (1-2):
User: 1

[Continue for TTS and LLM...]

Phase 3: MCP Server

Do you need an MCP server? (y/n)
User: n

Phase 4: Generate Files

[Preview config...]

Does this look correct? (y/edit/regenerate)
User: y

Where should I save? (default: echokit_server/)
User: [Enter]

✓ Configuration generated successfully!
[...]
```

## Version History

- 1.3.1 - Added `export RUST_LOG=debug` before server launch for better troubleshooting
- 1.3.0 - Fixed config.toml format with correct section order ([tts] → [asr] → [llm]), platform-specific field names (ElevenLabs uses `token`/`model_id`), removed comments from top, added `prompt` and `vad_url` fields for ASR
- 1.2.0 - Added Phase 5: API Key Entry and Server Launch with interactive key collection, automatic config updates, server build, and launch
- 1.1.0 - Enhanced system prompts, custom platform support with auto-fetch, corrected MCP format
- 1.0.0 - Initial release with four-phase flow

## Testing

To test this SKILL:

1. Install to `~/.claude/skills/`
2. In Claude Code: "Generate an EchoKit config for testing"
3. Verify files are created correctly
4. Check config.toml has valid TOML syntax
5. Test Phase 5: Enter API keys and verify config.toml is updated
6. Test server build (optional, requires Rust/Cargo)
7. Test server launch (optional, requires built server)

---

**This SKILL is 100% standalone with no external dependencies.**
