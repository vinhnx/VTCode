# Hugging Face Inference Providers Integrations

A curated collection of tools and integrations built to work with Hugging Face Inference Providers.

## Featured Integrations

| Tool | Description | Provider Support |
|------|-------------|------------------|
| [VT Code](./vtcode.md) | AI coding agent with semantic code analysis and 53+ specialized tools | Inference API, Dedicated Endpoints |

## About This Directory

This directory contains documentation for tools that integrate with Hugging Face Inference Providers to provide enhanced functionality for AI-powered workflows.

### Requirements to Be Listed

To be featured in this directory, integrations should:

- ✅ Work with Hugging Face Inference Providers (Inference API or Dedicated Endpoints)
- ✅ Be actively maintained with recent commits or releases
- ✅ Have clear documentation showing how to connect to Inference Providers

### How to Submit Your Integration

We'd love to feature your tool! Here's how:

1. **Test your integration** with Hugging Face Inference Providers
2. **Fork the repository** at [github.com/huggingface/hub-docs](https://github.com/huggingface/hub-docs)
3. **Update this index** (`docs/inference-providers/integrations/index.md`) to add your tool with a link to your integration docs
4. **Create an integration page** (optional but recommended) following the [Integration Page Template](#integration-page-template)
5. **Submit a Pull Request** with your changes

### Integration Page Template

If you create a dedicated integration page, use this structure:

```markdown
# Your Tool Name

Brief description of what your tool does.

## Overview

How your tool integrates with Hugging Face Inference Providers.

## Prerequisites

- Your tool installed
- HF account with [API token](https://huggingface.co/settings/tokens)

## Configuration

Step-by-step setup instructions with code examples.

## Resources

- [Your Tool Documentation](https://yourtool.com/docs)
- [HF Integration Guide](link-to-your-guide)
```

## Getting Started

### Setup

1. Create a [Hugging Face account](https://huggingface.co/join) if you don't have one
2. Get your [API token](https://huggingface.co/settings/tokens)
3. Choose an integration from the list above
4. Follow the integration's setup instructions

### Choosing Between Inference API and Dedicated Endpoints

**Hugging Face Inference API**
- Best for: Getting started, testing, lower traffic volumes
- No setup required beyond API token
- Built-in rate limiting
- Model selection from Hugging Face model hub

**Dedicated Endpoints**
- Best for: Production workloads, high traffic, custom models
- Dedicated GPU resources
- Custom fine-tuned models support
- Higher throughput and lower latency

## Common Patterns

### Environment Variables

Most integrations use environment variables for configuration:

```bash
export HF_TOKEN="hf_your_token_here"
export HF_ENDPOINT="https://api-inference.huggingface.co/v1"
```

### Configuration Files

Tools typically support configuration files (YAML, TOML, JSON):

```toml
[llm]
provider = "huggingface"
api_key = "hf_your_token_here"
base_url = "https://api-inference.huggingface.co/v1"
```

### Streaming Responses

Most integrations support streaming for real-time feedback:

```python
# Pseudo-code example
for chunk in client.chat_stream(messages):
    print(chunk.content, end="", flush=True)
```

## Resources

### Hugging Face Documentation

- [Inference API Documentation](https://huggingface.co/docs/api-inference/index)
- [Inference Endpoints Guide](https://huggingface.co/docs/inference-endpoints/index)
- [API Reference](https://huggingface.co/docs/api-inference/detailed_parameters)
- [Pricing](https://huggingface.co/pricing)

### Best Practices

- Always use environment variables for API keys, not hardcoded values
- Implement error handling and retry logic for API calls
- Monitor your API usage on the [Hugging Face dashboard](https://huggingface.co/account)
- Use token budgeting for cost control
- Cache responses when appropriate

## Community

- **GitHub Discussions**: [HF Hub-Docs](https://github.com/huggingface/hub-docs/discussions)
- **Hugging Face Forums**: [forums.huggingface.co](https://forums.huggingface.co)
- **Discord**: [Hugging Face Community](https://discord.gg/JfAtqhgAVd)

## Contributing

Found a bug or want to improve an integration? 

- Open an issue on the [integration's repository](./vtcode.md#support)
- Submit a PR with improvements
- Share your feedback on [Hugging Face discussions](https://github.com/huggingface/hub-docs/discussions)

## License

Documentation is available under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/).
