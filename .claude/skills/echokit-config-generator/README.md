# EchoKit Config Generator

> 🎯 A Claude Code SKILL for generating EchoKit server configurations through an interactive setup

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![SKILL Version](https://img.shields.io/badge/Version-1.1.0-blue.svg)](https://github.com/second-state/echokit_server)
[![Claude Code](https://img.shields.io/badge/Claude_Code-SKILL-teal.svg)](https://code.claude.com)

---

## ✨ What It Does

Generate `config.toml` files for EchoKit servers through an **interactive 4-phase process**:

1. 📝 **Assistant Definition** - Define your AI assistant's purpose, tone, capabilities, and behaviors
2. 🔧 **Platform Selection** - Choose ASR, TTS, and LLM services from supported platforms **or use any custom platform**
3. 🔌 **MCP Configuration** (Optional) - Add MCP server support
4. 📦 **Generate Files** - Create production-ready config.toml with setup guide

---

## 🚀 Quick Start

### Installation

This SKILL can be installed in the following ways:

```bash
# Clone echokit_server
git clone https://github.com/second-state/echokit_server.git
cd echokit_server

**That's it!** The SKILL is now available in Claude Code.

### Usage

In Claude Code, simply say:

```
"Generate an EchoKit config for a voice companion assistant"
```

Or be more specific:

```
"Create an EchoKit server configuration for a coding helper using Groq"
```


## 🎯 Key Features

### ✅ Rich System Prompt Generation

The SKILL asks **7 detailed questions** to create sophisticated, customized system prompts:

1. **Purpose** - What does your assistant do?
2. **Tone** - Professional, casual, friendly, expert, or custom
3. **Capabilities** - Specific skills and abilities
4. **Response Format** - Short answers, detailed, step-by-step, etc.
5. **Domain Knowledge** - Programming, medicine, finance, etc.
6. **Constraints** - Formatting rules, citation requirements, etc.
7. **Additional Instructions** - Any custom preferences

### ✅ Flexible Platform Support

**Pre-configured Platforms:**
- **ASR:** OpenAI Whisper, Local Whisper
- **TTS:** OpenAI, ElevenLabs (streaming), GPT-SoVITS
- **LLM:** OpenAI Chat, OpenAI Responses API

**Custom Platforms** (via WebSearch auto-discovery):
- Groq, DeepSeek, Mistral, Together, or any other platform
- Automatically fetches API endpoints
- Suggests default models
- Confirms with you before using

### ✅ MCP Server Support

Add MCP servers to your EchoKit configuration:
- Embedded in LLM section: `[[llm.mcp_server]]`
- Supports HTTP streaming types
- Customizable server URLs

### ✅ Zero Dependencies

- No scripts to install
- No Python packages required
- No build process
- Just pure Claude Code SKILL magic

---

## 📦 What You Get

After running the SKILL, you'll have:

```
echokit_server/
├── config.toml          # Production-ready configuration
└── SETUP_GUIDE.md       # Step-by-step setup instructions
```

The `config.toml` includes:
- Server configuration
- ASR, TTS, LLM platform settings
- Custom system prompt
- MCP server (if enabled)

The `SETUP_GUIDE.md` includes:
- API key acquisition links
- Build instructions
- Connection troubleshooting
- Testing procedures

---

## 💡 Usage Examples

### Example 1: Voice Companion

```
You: Generate an EchoKit config for a friendly voice assistant

SKILL: I'm using the EchoKit Config Generator...

Phase 1: Assistant Definition
What is your AI assistant's primary purpose?
You: Daily conversational assistance and casual chat

What tone should it have?
You: Friendly

What specific capabilities should it have?
You: General knowledge, quick responses

[...continues through all phases...]

✓ Configuration generated!
```

### Example 2: Custom Platform (Groq)

```
You: Create config for coding assistant using Groq LLM

SKILL: [...phase 1 questions...]

Phase 2: Platform Selection

Available LLM Services:
1. OpenAI Chat
   ...
2. OpenAI Responses API
   ...

C. Custom - Specify your own platform/model

Your choice (1-2 or C):
You: C

What's the platform name?
You: groq

[SKILL searches for Groq API documentation...]

I found the following for groq LLM:
API Endpoint: https://api.groq.com/openai/v1/chat/completions
Documentation: https://console.groq.com/docs/quickstart
Authentication: Bearer token
Default Models: llama-3.3-70b-versatile, mixtral-8x7b-32768

Is this correct? (y/edit)
You: y

What model should I use?
You: llama-3.3-70b-versatile

[...continues...]
```

### Example 3: With MCP Server

```
You: Generate EchoKit config with MCP server

[...phases 1-2...]

Phase 3: MCP Server

Do you need an MCP server? (y/n)
You: y

What's your MCP server URL? (press Enter for default: http://localhost:8000/mcp)
You: [Enter]

[...generates config with MCP embedded...]
```

---

## 🏗️ Supported Platforms

### ASR (Speech Recognition): Any OpenAI-compatible

| Platform | Model | Notes |
|----------|-------|-------|
| OpenAI Whisper | gpt-4o-mini-transcribe | Best accuracy |
| Local Whisper | base | Free, private |
| **Custom** | Any | Auto-discovered via WebSearch |

### TTS (Text-to-Speech)

| Platform | Voice | Notes |
|----------|-------|-------|
| OpenAI TTS | ash, alloy, echo, fable, onyx, nova | Multiple voices |
| ElevenLabs | Custom | Streaming via WebSocket |
| GPT-SoVITS | Custom | Local streaming |
| **Custom** | Any | Auto-discovered via WebSearch |

### LLM (Chat): Any OpenAI-chat and OpenAI-responses compatible

| Platform | Models | Notes |
|----------|--------|-------|
| OpenAI Chat | gpt-4o-mini, gpt-4o, etc. | Most compatible |
| OpenAI Responses | gpt-4o-mini, etc. | For streaming interactions |
| **Custom** | Any | Groq, DeepSeek, Mistral, etc. |

---

## 📁 Repository Structure

```
echokit-config-skill/
├── SKILL.md              # Main SKILL file (all logic)
├── platforms/            # Platform configuration data
│   ├── asr.yml
│   ├── tts.yml
│   └── llm.yml
├── templates/            # Output file templates
│   └── SETUP_GUIDE.md
├── examples/             # Example configurations
│   ├── voice-companion.toml
│   └── coding-assistant.toml
├── README.md             # This file
├── CONTRIBUTING.md       # Contribution guidelines
├── CHANGELOG.md          # Version history
└── LICENSE               # MIT License
```

---

## 🔄 Updating

### If installed standalone:

```bash
cd ~/.claude/skills/echokit-config-generator
git pull origin main
```

### If installed from echokit_server:

```bash
cd echokit_server
git pull origin main
# The SKILL will be updated automatically
```

---

## 🛠️ Development

### Adding New Platforms

Edit the appropriate file in `platforms/`:

```yaml
your_platform:
  name: "Display Name"
  platform: "platform_id"
  url: "https://api.example.com/endpoint"
  model: "default_model"
  api_key_url: "https://example.com/get-keys"
  notes: "Additional information"
```

### Testing

```bash
# In Claude Code
"Generate an EchoKit config for testing"

# Verify output
cat echokit_server/config.toml
cat echokit_server/SETUP_GUIDE.md
```

---

## 🤝 Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

- Report bugs
- Suggest new features
- Add platform support
- Improve documentation
- Submit pull requests

---

## 📝 Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history.

### Recent Changes (v1.1.0)

- ✨ Enhanced system prompt generation (7 detailed questions)
- ✨ Added custom platform support with automatic API discovery via WebSearch
- 🐛 Fixed Enter key handling for default values
- 🐛 Corrected MCP server format to use `[[llm.mcp_server]]`

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🔗 Links

- [EchoKit Server](https://github.com/second-state/echokit_server)
- [EchoKit Documentation](https://echokit.dev)
- [Claude Code](https://code.claude.com)
- [Report Issues](https://github.com/YOUR_USERNAME/echokit-config-skill/issues)

---

## 🌟 Star History

If you find this SKILL helpful, consider giving it a star! ⭐

---

**Made with ❤️ for the EchoKit community**

*Generated with [Claude Code](https://code.claude.com)*
