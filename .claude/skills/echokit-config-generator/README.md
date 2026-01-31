# EchoKit Config Generator

> 🎯 A Claude Code SKILL for generating EchoKit server configurations through an interactive setup

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![SKILL Version](https://img.shields.io/badge/Version-1.4.0-blue.svg)](https://github.com/second-state/echokit_server)
[![Claude Code](https://img.shields.io/badge/Claude_Code-SKILL-teal.svg)](https://code.claude.com)

---

## ✨ What It Does

Generate `config.toml` files for EchoKit servers through an **interactive 5-phase process**:

1. 📝 **Assistant Definition** - Choose from 8 role presets or create custom AI assistant with advanced system prompt generation
2. 🎯 **End-to-End Option** - Use integrated models like Gemini Live or separate ASR/TTS/LLM services
3. 🔧 **Platform Selection** - Choose from 15+ providers or use any custom platform with auto-discovery
4. 🔌 **MCP Configuration** (Optional) - Add MCP server support
5. 📦 **Generate & Launch** - Create config, enter API keys, build and launch server



## 🚀 Quick Start

### Installation

This SKILL can be installed in the following ways:

```bash
# Clone echokit_server
git clone https://github.com/second-state/echokit_server.git
cd echokit_server
```
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

### ✅ Role Presets & Advanced System Prompt Generation

**8 Pre-configured Role Presets:**
1. **General Assistant** - Versatile AI for everyday tasks
2. **Coding Assistant** - Programming and software development expert
3. **Creative Writer** - Creative writing and storytelling companion
4. **Business Analyst** - Business strategy and data analysis expert
5. **Language Tutor** - Language learning and practice companion
6. **Research Assistant** - Academic research and information synthesis
7. **Wellness Coach** - Health, fitness, and lifestyle guidance
8. **Data Scientist** - Data analysis, ML, and statistical modeling

**Custom Configuration** with detailed questions:
1. **Purpose** - What does your assistant do?
2. **Tone** - Professional, casual, friendly, expert, or custom
3. **Capabilities** - Specific skills and abilities
4. **Response Format** - Short answers, detailed, step-by-step, etc.
5. **Domain Knowledge** - Programming, medicine, finance, etc.
6. **Constraints** - Formatting rules, citation requirements, etc.
7. **Additional Instructions** - Any custom preferences
8. **Safety Constraints** - Medical disclaimers, content filtering, etc.
9. **Tool Access** - External APIs, web search, database access

### ✅ End-to-End Voice AI Models

**Integrated Solutions:**
- **Google Gemini Live** - Multimodal real-time API with native audio I/O, 1M context
- **OpenAI Realtime API** - Low-latency multimodal with function calling

Skip separate ASR/TTS/LLM configuration and use a single unified endpoint!

### ✅ Extensive Platform Support

**Pre-configured Platforms:**
- **ASR:** OpenAI Whisper, Deepgram Nova-2, AssemblyAI, Azure Speech, Groq Whisper, Local Whisper
- **TTS:** OpenAI, ElevenLabs, Azure TTS, Google Cloud TTS, Cartesia Sonic, PlayHT, GPT-SoVITS
- **LLM:** OpenAI Chat, Anthropic Claude, Google Gemini, Groq, Together AI, DeepSeek, Mistral

**Custom Platforms** (via WebSearch auto-discovery):
- Any OpenAI-compatible API
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

### End-to-End Models

| Platform | Features | Notes |
|----------|----------|-------|
| Google Gemini Live | Native audio I/O, multimodal, 1M context | Free tier available |
| OpenAI Realtime API | Low-latency, function calling, VAD | Preview access |

### ASR (Speech Recognition)

| Platform | Model | Notes |
|----------|-------|-------|
| OpenAI Whisper | gpt-4o-mini-transcribe | Best accuracy |
| Deepgram Nova-2 | nova-2 | Fast, 45+ languages |
| AssemblyAI | best | Speaker diarization, sentiment |
| Azure Speech | default | Enterprise-grade, 100+ languages |
| Groq Whisper | whisper-large-v3 | Ultra-fast (500+ tokens/s) |
| Local Whisper | base | Free, private |
| **Custom** | Any | Auto-discovered via WebSearch |

### TTS (Text-to-Speech)

| Platform | Voice | Notes |
|----------|-------|-------|
| OpenAI TTS | ash, alloy, echo, fable, onyx, nova | Multiple voices |
| ElevenLabs | Custom | Premium streaming via WebSocket |
| Azure TTS | 400+ neural voices | 140+ languages, SSML support |
| Google Cloud TTS | Neural2, WaveNet | Natural prosody, 40+ languages |
| Cartesia Sonic | sonic-english | Ultra-low latency (<300ms) |
| PlayHT 2.0 | Custom | Voice cloning, 142 languages |
| GPT-SoVITS | Custom | Local streaming |
| **Custom** | Any | Auto-discovered via WebSearch |

### LLM (Chat)

| Platform | Models | Notes |
|----------|--------|-------|
| OpenAI Chat | gpt-4o-mini, gpt-4o | Most compatible |
| Anthropic Claude | claude-3-5-sonnet | 200K context, advanced reasoning |
| Google Gemini | gemini-2.0-flash-exp | 1M context, multimodal |
| Groq | llama-3.3-70b-versatile | Fastest inference (500+ tokens/s) |
| Together AI | Meta-Llama-3.1-70B | 100+ open-source models |
| DeepSeek | deepseek-chat | Cost-effective, strong coding |
| Mistral | mistral-large-latest | European AI, multilingual |
| **Custom** | Any | OpenAI-compatible APIs |

---

## 📁 Repository Structure

```
echokit-config-generator/
├── skill.md              # Main SKILL file (all logic)
├── platforms/            # Platform configuration data
│   ├── asr.yml          # ASR providers (6 platforms)
│   ├── tts.yml          # TTS providers (7 platforms)
│   ├── llm.yml          # LLM providers (8 platforms)
│   └── end-to-end.yml   # Integrated models (2 platforms)
├── templates/            # Output file templates
│   ├── SETUP_GUIDE.md   # Setup instructions template
│   └── prompt-presets.yml # 8 role presets
├── examples/             # Example configurations
│   ├── voice-companion.toml
│   ├── coding-assistant.toml
│   ├── customer-service.toml
│   ├── education-tutor.toml
│   ├── technical-support.toml
│   └── healthcare-assistant.toml
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

### Recent Changes (v1.4.0)

- ✨ Added 8 role presets for quick assistant setup
- ✨ Added end-to-end model support (Gemini Live, OpenAI Realtime API)
- ✨ Expanded to 15+ platform providers across ASR/TTS/LLM
- ✨ Added safety constraints and tool access configuration
- ✨ Added system prompt validation
- ✨ Added 4 new example configs (customer service, education, technical support, healthcare)
- 🎯 Enhanced system prompt generation with advanced options

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🔗 Links

- [EchoKit Server](https://github.com/second-state/echokit_server)
- [EchoKit Documentation](https://echokit.dev)
- [Claude Code](https://code.claude.com)
- [Report Issues](https://github.com/second-state/echokit_server/issues)

---

## 🌟 Star History

If you find this SKILL helpful, consider giving it a star! ⭐

---

**Made with ❤️ for the EchoKit community**

*Generated with [Claude Code](https://code.claude.com)*
