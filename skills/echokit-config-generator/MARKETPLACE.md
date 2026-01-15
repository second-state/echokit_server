# Claude Code Skills Marketplace Submission

## SKILL Information

**Name:** EchoKit Config Generator
**Category:** Development Tools / Configuration
**Tags:** echokit, config, toml, rust, iot, server
**Version:** 1.0.0
**License:** MIT
**Standalone:** Yes (no dependencies)

## Short Description

Generate `config.toml` files for EchoKit servers through an interactive 4-step process. Choose ASR, TTS, and LLM platforms, define your AI assistant's personality, and get production-ready configurations with setup guides.

## Long Description

The EchoKit Config Generator SKILL simplifies setting up EchoKit servers by guiding you through:

1. **Assistant Definition** - Define your AI assistant's purpose, tone, and behaviors
2. **Platform Selection** - Choose from 9+ supported platforms (ASR, TTS, LLM)
3. **MCP Configuration** - Optional MCP server templates (filesystem, git, custom)
4. **File Generation** - Get config.toml, setup guide, and MCP templates

### Key Features

- ✅ **Interactive flow** - Step-by-step guidance with clear prompts
- ✅ **Platform knowledge** - Curated platform data with API key locations
- ✅ **System prompts** - Auto-generate prompts based on your requirements
- ✅ **MCP support** - Templates for common MCP servers
- ✅ **Examples** - Pre-built configs for voice companion and coding assistant
- ✅ **Zero dependencies** - Completely standalone SKILL

### Supported Platforms

**ASR:** OpenAI Whisper, Local Whisper, Deepgram
**TTS:** OpenAI, ElevenLabs, Azure Speech
**LLM:** OpenAI GPT, Anthropic Claude, Ollama

### What You Get

- `config.toml` - Production-ready EchoKit server configuration
- `SETUP_GUIDE.md` - Setup instructions with API key links
- `mcp_server.toml` - MCP server template (if enabled)

## Repository

https://github.com/YOUR_USERNAME/echokit-config-skill

## Installation

```bash
git clone https://github.com/YOUR_USERNAME/echokit-config-skill.git ~/.claude/skills/echokit-config-generator
```

## Usage Example

```
User: Generate an EchoKit config for a voice companion assistant

AI: I'm using the EchoKit Config Generator to create your config.toml.

[Interactive flow with questions about assistant type, platforms, etc.]

✓ Configuration generated!
  Files created:
  - echokit_server/config.toml
  - echokit_server/SETUP_GUIDE.md

  Next steps:
  1. Replace YOUR_API_KEY_HERE placeholders
  2. Build: cargo build --release
  3. Run: ./target/release/echokit_server
```

## Dependencies

None. This is a completely standalone SKILL with no external dependencies.

## Testing

Tested on:
- macOS 14+
- Ubuntu 22.04+
- Claude Code v1.x

To test:
```bash
# Install SKILL
git clone https://github.com/YOUR_USERNAME/echokit-config-skill.git ~/.claude/skills/echokit-config-generator

# In Claude Code
"Generate an EchoKit config for testing"

# Verify output
cat echokit_server/config.toml
```

## Documentation

- README: Comprehensive installation and usage guide
- Examples: Pre-built configurations for common use cases
- Inline: SKILL.md contains detailed instructions

## Maintainer

Your Name <your.email@example.com>

## License

MIT License - freely usable, modifiable, and distributable

## Changelog

See CHANGELOG.md for version history.

---

**Why this SKILL?**

Setting up EchoKit servers requires editing TOML configs with correct API endpoints, models, and formats. This SKILL eliminates manual config editing, reduces errors, and gets you from zero to running server in minutes.
