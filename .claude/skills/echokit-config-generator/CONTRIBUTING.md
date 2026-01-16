# Contributing to EchoKit Config Generator

Thanks for your interest in contributing!

## How to Contribute

### Adding Platforms

Edit the appropriate file in `platforms/`:

**For ASR platforms** - `platforms/asr.yml`:
```yaml
your_platform:
  name: "Display Name"
  platform: "platform_id"
  url: "https://api.example.com/endpoint"
  model: "default_model"
  api_key_url: "https://example.com/get-keys"
  notes: "Additional information"
```

**For TTS platforms** - `platforms/tts.yml`:
```yaml
your_platform:
  name: "Display Name"
  platform: "platform_id"
  url: "https://api.example.com/endpoint"
  model: "default_model"
  voice: "default_voice"
  api_key_url: "https://example.com/get-keys"
  notes: "Additional information"
```

**For LLM platforms** - `platforms/llm.yml`:
```yaml
your_platform:
  name: "Display Name"
  platform: "platform_id"
  url: "https://api.example.com/endpoint"
  model: "default_model"
  history: 5
  api_key_url: "https://example.com/get-keys"
  notes: "Additional information"
```

### Adding Examples

Create new example configs in `examples/` directory:
- Follow existing naming convention: `example-name.toml`
- Include system prompt with behavior and constraints
- Use `YOUR_API_KEY_HERE` placeholders

### Reporting Issues

When reporting bugs, please include:
- Steps to reproduce
- Expected vs actual behavior
- Your environment (OS, Claude Code version)
- Error messages or logs

### Submitting Changes

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly
5. Submit a pull request

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
