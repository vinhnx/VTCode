# Ollama Provider Guide

Ollama is a local AI model runner that allows you to run LLMs directly on your machine without requiring internet access or API keys. VT Code integrates with Ollama to provide local AI capabilities for coding tasks.

## Prerequisites

- Ollama installed and running locally ([download](https://ollama.com/download))
- At least one model pulled locally (e.g., `ollama pull llama3:8b`)

## Installation and Setup

1. **Install Ollama**: Download from [ollama.com](https://ollama.com/download) and follow platform-specific instructions
2. **Start Ollama server**: Run `ollama serve` in a terminal
3. **Pull a model**: Choose and download a model to use:
   ```bash
   # Popular coding models
   ollama pull llama3:8b
   ollama pull codellama:7b
   ollama pull mistral:7b
   ollama pull qwen3:1.7b
   ollama pull deepseek-coder:6.7b
   ollama pull phind-coder:34b
   
   # List available local models
   ollama list
   ```

## Configuration

### Environment Variables

- `OLLAMA_BASE_URL` (optional): Custom Ollama endpoint (default: `http://localhost:11434/v1`)

### VT Code Configuration

Set up `vtcode.toml` in your project root:

```toml
[agent]
provider = "ollama"                    # Ollama provider
default_model = "llama3:8b"           # Any locally available model
# Note: No API key required for local Ollama

[tools]
default_policy = "prompt"             # Safety: "allow", "prompt", or "deny"

[tools.policies]
read_file = "allow"                   # Always allow file reading
write_file = "prompt"                 # Prompt before modifications
run_terminal_cmd = "prompt"           # Prompt before commands
```

## Using Custom Ollama Models

VT Code supports custom Ollama models through the interactive model picker or directly via CLI:

```bash
# Using the interactive model picker (select "custom-ollama")
vtcode

# Direct CLI usage with custom model
vtcode --provider ollama --model mistral:7b ask "Review this code"
vtcode --provider ollama --model codellama:7b ask "Explain this function"
vtcode --provider ollama --model gpt-oss-20b ask "Help with this implementation"
```

## OpenAI OSS Models Support

VT Code includes support for OpenAI's open-source models that can be run via Ollama:

- `gpt-oss-20b`: Open-source 20B parameter model from OpenAI

To use these models:

```bash
# Pull the model first
ollama pull gpt-oss-20b

# Use in VT Code
vtcode --provider ollama --model gpt-oss-20b ask "Code review this function"
```

## Troubleshooting

### Common Issues

1. **"Connection refused" errors**: Ensure Ollama server is running (`ollama serve`)
2. **Model not found**: Ensure the requested model has been pulled (`ollama pull MODEL_NAME`)
3. **Performance issues**: Consider model size - larger models require more RAM
4. **Memory errors**: For large models like gpt-oss-120b, ensure sufficient RAM (64GB+ recommended)

### Testing Ollama Connection

Verify Ollama is working correctly:

```bash
# Test basic Ollama functionality
ollama run llama3:8b

# Test via API call
curl http://localhost:11434/api/tags
```

## Performance Notes

- Local models don't require internet connection
- Performance varies significantly based on model size and local hardware
- Larger models (30B+) require substantial RAM (32GB+) for reasonable performance
- Smaller models (7B-13B) work well on consumer hardware with 16GB+ RAM