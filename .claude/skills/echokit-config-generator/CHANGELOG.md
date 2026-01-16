# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.1] - 2025-01-16

### Changed
- Added `export RUST_LOG=debug` before launching server in Phase 5, Step 6
- Server now runs with debug logging enabled for better troubleshooting

## [1.3.0] - 2025-01-16

### Fixed
- **CRITICAL**: Corrected config.toml section order to `[tts]` → `[asr]` → `[llm]` (required by EchoKit server parser)
- **CRITICAL**: Fixed platform-specific field names:
  - ElevenLabs TTS now uses `token` instead of `api_key`
  - ElevenLabs TTS now uses `model_id` instead of `model`
- Removed header comments from config.toml (parser rejects comments at file start)
- Added required ASR fields: `prompt` and `vad_url` for Whisper compatibility
- Updated platform YAML metadata with `api_key_field` and `model_field` properties

### Changed
- Enhanced Phase 4 documentation with platform-specific field mapping guide
- Enhanced Phase 5 with warnings about correct field names when updating API keys
- Improved error prevention by documenting exact field name requirements per platform

## [1.2.0] - 2025-01-15

### Added
- Phase 5: API Key Entry and Server Launch
  - Interactive API key collection for all services
  - Automatic config.toml update with provided keys
  - Server build verification and automatic building
  - Server launch with background process management
  - Local IP address detection for WebSocket URL display
  - Process ID tracking for server management
- Enhanced success messages with actual WebSocket URLs
- Server verification steps after launch
- Comprehensive error handling for build failures and server crashes

### Changed
- Updated workflow from file generation only to complete server setup
- Five-phase process instead of four-phase

## [1.1.0] - 2025-01-14

### Added
- Enhanced system prompt generation with tone-based behavioral defaults
- Custom platform support with auto-configuration via WebSearch
- Corrected MCP server configuration format
- Platform auto-detection for common providers (Groq, DeepSeek, etc.)

### Changed
- Improved system prompt sophistication with domain knowledge integration
- Better handling of user constraints and formatting requirements

## [1.0.0] - 2025-01-13

### Added
- Initial release of EchoKit Config Generator SKILL
- Four-phase interactive configuration flow
- Platform knowledge base with 9+ platforms:
  - ASR: OpenAI Whisper, Local Whisper, Deepgram
  - TTS: OpenAI, ElevenLabs, Azure Speech
  - LLM: OpenAI GPT, Anthropic Claude, Ollama
- System prompt generation from user requirements
- MCP server templates (filesystem, git, custom)
- Example configurations for common use cases
- Comprehensive documentation
- Zero external dependencies (completely standalone)

### Features
- Interactive platform selection with API key locations
- Automatic config.toml generation
- SETUP_GUIDE.md with step-by-step instructions
- MCP server support (optional)
- Pre-built examples (voice companion, coding assistant)

[1.0.0]: https://github.com/YOUR_USERNAME/echokit-config-skill/releases/tag/v1.0.0
