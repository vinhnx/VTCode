# Hugging Face Models Quick Reference

## Conversational LLMs

| Model ID | Name | Context | Reasoning | Tools | Structured Output | Best For |
|----------|------|---------|-----------|-------|-------------------|----------|
| `google/gemma-2-2b-it` | Gemma 2 2B IT | 8K | ❌ | ✅ | ✅ | Fast, lightweight tasks |
| `Qwen/Qwen3-Coder-480B-A35B-Instruct` | Qwen3 Coder 480B | 131K | ✅ | ✅ | ✅ | **Advanced coding** |
| `openai/gpt-oss-120b` | GPT-OSS 120B | 131K | ✅ | ✅ | ✅ | **Top tool calling** |
| `zai-org/GLM-4.5` | GLM-4.5 | 131K | ✅ | ✅ | ✅ | General reasoning |
| `Qwen/Qwen3-4B-Thinking-2507` | Qwen3 4B Thinking | 32K | ✅ | ✅ | ✅ | Small reasoning model |
| `Qwen/Qwen2.5-7B-Instruct-1M` | Qwen2.5 7B Instruct | **1M** | ✅ | ✅ | ✅ | **Very long context** |
| `Qwen/Qwen2.5-Coder-32B-Instruct` | Qwen2.5 Coder 32B | 131K | ✅ | ✅ | ✅ | Code generation |
| `deepseek-ai/DeepSeek-R1` | DeepSeek R1 | 131K | ✅ | ✅ | ✅ | **Advanced reasoning** |
| `deepseek-ai/DeepSeek-V3.2` | DeepSeek V3.2 | 131K | ✅ | ✅ | ✅ | Chat & reasoning |
| `openai/gpt-oss-20b` | GPT-OSS 20B | 131K | ✅ | ✅ | ✅ | Balanced performance |
| `zai-org/GLM-4.6` | GLM-4.6 | 131K | ✅ | ✅ | ✅ | General reasoning |
| `zai-org/GLM-4.7` | GLM-4.7 | 131K | ✅ | ✅ | ✅ | Latest GLM version |
| `moonshotai/Kimi-K2-Thinking` | Kimi K2 Thinking | 131K | ✅ | ✅ | ✅ | Reasoning tasks |

## Vision-Language Models (VLMs)

| Model ID | Name | Context | Modalities | Reasoning | Tools | Best For |
|----------|------|---------|------------|-----------|-------|----------|
| `zai-org/GLM-4.5V` | GLM-4.5V | 131K | Text + Image | ✅ | ✅ | **Vision reasoning** |

## Usage Examples

### Basic Chat Completion

```bash
# Using the default model (GPT-OSS 120B)
vtcode ask "Explain async/await in Rust" --provider huggingface

# Using a specific model
vtcode ask "Write a sorting algorithm" --provider huggingface --model "Qwen/Qwen3-Coder-480B-A35B-Instruct"
```

### Long Context Tasks

```bash
# Use Qwen2.5-7B-Instruct-1M for very long documents
vtcode ask "Analyze this entire codebase" --provider huggingface --model "Qwen/Qwen2.5-7B-Instruct-1M"
```

### Vision Tasks

```bash
# Use GLM-4.5V for image understanding
vtcode ask "What's in this image?" --provider huggingface --model "zai-org/GLM-4.5V" --image screenshot.png
```

### Reasoning Tasks

```bash
# Use DeepSeek R1 for complex reasoning
vtcode ask "Solve this math problem step by step" --provider huggingface --model "deepseek-ai/DeepSeek-R1"
```

### Coding Tasks

```bash
# Use Qwen3 Coder for advanced coding
vtcode ask "Refactor this function" --provider huggingface --model "Qwen/Qwen3-Coder-480B-A35B-Instruct"

# Use Qwen2.5 Coder for code generation
vtcode ask "Generate a REST API" --provider huggingface --model "Qwen/Qwen2.5-Coder-32B-Instruct"
```

## Model Selection Guide

### Choose by Use Case:

- **🚀 Best Overall**: `openai/gpt-oss-120b` - Great tool calling, balanced performance
- **💻 Best for Coding**: `Qwen/Qwen3-Coder-480B-A35B-Instruct` - Specialized for code
- **🧠 Best for Reasoning**: `deepseek-ai/DeepSeek-R1` - Advanced reasoning capabilities
- **📚 Best for Long Context**: `Qwen/Qwen2.5-7B-Instruct-1M` - 1M token context
- **👁️ Best for Vision**: `zai-org/GLM-4.5V` - Vision-language model
- **⚡ Fastest**: `google/gemma-2-2b-it` - Small, fast, lightweight
- **🎯 Best Small Reasoning**: `Qwen/Qwen3-4B-Thinking-2507` - Compact with reasoning

### Choose by Context Window:

- **1M tokens**: Qwen2.5-7B-Instruct-1M
- **131K tokens**: Most models (recommended for general use)
- **32K tokens**: Qwen3-4B-Thinking-2507
- **8K tokens**: Gemma 2 2B IT

## Configuration

### Environment Setup

```bash
# Set your Hugging Face token
export HF_TOKEN="hf_..."

# Optional: Override base URL
export HUGGINGFACE_BASE_URL="https://router.huggingface.co/v1"
```

### vtcode.toml

```toml
[llm]
provider = "huggingface"
model = "openai/gpt-oss-120b"  # or any model from the table above

[llm.huggingface]
api_key = "${HF_TOKEN}"
# base_url = "https://router.huggingface.co/v1"  # optional override
```

## API Features

All models support:
- ✅ **Streaming** - Real-time token streaming via SSE
- ✅ **Tool Calling** - Function calling and parallel execution
- ✅ **Structured Output** - JSON mode via `response_format`

Most models support:
- ✅ **Reasoning** - Advanced reasoning capabilities (11/13 models)

Some models support:
- ✅ **Vision** - Image inputs (GLM-4.5V only)
- ✅ **1M Context** - Ultra-long context (Qwen2.5-7B-Instruct-1M only)

## Links

- [Chat Completion API Docs](https://huggingface.co/docs/inference-providers/tasks/chat-completion)
- [API Playground](https://huggingface.co/playground)
- [Model Hub](https://huggingface.co/models?inference=warm&pipeline_tag=text-generation&sort=trending)
