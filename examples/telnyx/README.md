# Telnyx Integration for EchoKit Server

This guide explains how to configure EchoKit Server to use Telnyx AI services.

## Why Telnyx?

Telnyx provides a comprehensive AI platform that integrates seamlessly with EchoKit:

- **OpenAI-Compatible API**: All endpoints follow OpenAI specifications, enabling drop-in compatibility with EchoKit's architecture
- **53+ AI Models**: Access to GPT-4, Claude, Llama, Mistral, and many open-source models through a single API
- **Global Edge Network**: Low-latency inference from data centers worldwide
- **Unified Billing**: Single API key for ASR, TTS, and LLM services
- **Competitive Pricing**: Pay-per-use with transparent, per-token pricing

## Prerequisites

1. A Telnyx account ([sign up here](https://telnyx.com/sign-up))
2. An API key from the [Telnyx Portal](https://portal.telnyx.com)

## Quick Start

### 1. Set Your API Key

```bash
export TELNYX_API_KEY="your-api-key-here"
```

### 2. Use the Example Configuration

```bash
cp examples/telnyx/config.toml config.toml
```

### 3. Build and Run

```bash
cargo build --release
./target/release/echokit_server
```

## Configuration Reference

### ASR (Speech Recognition)

```toml
[asr]
platform = "openai"
url = "https://api.telnyx.com/v2/ai/transcriptions"
api_key = "${TELNYX_API_KEY}"
model = "whisper-1"
lang = "en"
```

Available models:
- `whisper-1` - OpenAI Whisper (recommended)

### TTS (Text-to-Speech)

```toml
[tts]
platform = "openai"
url = "https://api.telnyx.com/v2/ai/speech"
model = "tts-1"
api_key = "${TELNYX_API_KEY}"
voice = "alloy"
```

Available voices:
- `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`

Available models:
- `tts-1` - Optimized for speed
- `tts-1-hd` - Higher quality audio

### LLM (Language Model)

```toml
[llm]
platform = "openai_chat"
url = "https://api.telnyx.com/v2/ai/chat/completions"
api_key = "${TELNYX_API_KEY}"
model = "gpt-4o-mini"
history = 5
```

Popular model options:
- `gpt-4o` - Latest GPT-4 optimized
- `gpt-4o-mini` - Fast, cost-effective
- `claude-3-5-sonnet` - Anthropic Claude
- `llama-3.1-70b-instruct` - Open-source alternative
- `llama-3.1-8b-instruct` - Lightweight, fast inference

See the [Telnyx AI documentation](https://developers.telnyx.com/docs/ai/introduction) for the complete model list.

## Using Telnyx LiteLLM Proxy

For advanced use cases, Telnyx offers a LiteLLM proxy that provides:

- Automatic fallback between models
- Load balancing across providers
- Unified rate limiting
- Custom model aliases

Configure the proxy URL in your `config.toml`:

```toml
[llm]
platform = "openai_chat"
url = "https://api.telnyx.com/v2/ai/chat/completions"
# Use any supported model
model = "claude-3-5-sonnet"
```

## Troubleshooting

### Authentication Errors

Verify your API key is set correctly:

```bash
echo $TELNYX_API_KEY
```

### Model Not Found

Check available models in your Telnyx Portal or consult the [API documentation](https://developers.telnyx.com/docs/ai/introduction).

### High Latency

Telnyx routes requests to the nearest edge location automatically. If you experience latency issues, verify your network connection or contact Telnyx support.

## Additional Resources

- [Telnyx AI Documentation](https://developers.telnyx.com/docs/ai/introduction)
- [Telnyx API Reference](https://developers.telnyx.com/docs/api/v2/overview)
- [EchoKit Documentation](https://echokit.dev/docs/quick-start/)
- [Telnyx Discord Community](https://discord.gg/telnyx)

## License

This integration example is provided under the same license as EchoKit Server.
