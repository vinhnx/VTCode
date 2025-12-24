# VT Code

VT Code is a powerful AI coding agent that integrates with Hugging Face Inference Providers to enable intelligent code analysis, generation, and automation within your development workflow.

## Overview

VT Code leverages Hugging Face Inference Providers to:

-   **Multi-provider LLM support** - Seamlessly switch between multiple inference providers
-   **Code understanding** - Uses Tree-Sitter integration for semantic code analysis
-   **Intelligent tool system** - 53+ specialized tools for code inspection, execution, and manipulation
-   **Real-time collaboration** - Interactive sessions with streaming output and feedback
-   **Provider failover** - Automatic fallback to alternate providers if one fails

## Prerequisites

-   VT Code installed ([installation guide](https://github.com/vinhnx/vtcode))
-   Hugging Face account with [API token](https://huggingface.co/settings/tokens)
-   Your preferred Hugging Face Inference Provider configured

## Configuration

### Step 1: Get Your HF API Token

1. Log in to [Hugging Face](https://huggingface.co)
2. Go to [Settings → Access Tokens](https://huggingface.co/settings/tokens)
3. Create a new token with `read` access
4. Copy your token

### Step 2: Configure VT Code

Set your Hugging Face API key as an environment variable:

```bash
export HF_TOKEN="hf_your_token_here"
```

Or add it to your `vtcode.toml` configuration file:

```toml
[llm]
provider = "huggingface"
api_key = "hf_your_token_here"
model = "zai-org/GLM-4.7"
```

### Step 3: Configure Inference Provider

VT Code supports multiple Hugging Face Inference Provider types:

#### Using Hugging Face Inference API

```toml
[llm]
provider = "huggingface"
base_url = "https://api-inference.huggingface.co/v1"
model = "zai-org/GLM-4.7"
```

#### Using Hugging Face Dedicated Endpoints

```toml
[llm]
provider = "huggingface"
base_url = "https://your-endpoint-name.endpoints.huggingface.cloud"
model = "your-model-name"
api_key = "hf_your_token_here"
```

### Step 4: Run VT Code with HF Provider

```bash
# Using CLI
cargo run -- ask "What does this code do?" < example.py

# Using interactive mode
cargo run -- chat
```

## Supported Models

VT Code works with any model available through Hugging Face Inference Providers:

-   **Code-specific models**: Codestral, Code Llama, StarCoder
-   **General models**: Llama 2, Mistral, Zephyr
-   **Multi-modal models**: LLaVA and other vision models
-   **Custom fine-tuned models**: Deploy your own endpoints

## Features with HF Integration

### Real-time Code Analysis

```rust
// VT Code analyzes code using Tree-Sitter + LLM
// Provides semantic understanding beyond simple text matching
let result = analyze_code_with_hf(code_snippet).await?;
```

### Streaming Responses

Get streaming output for better user experience:

```bash
cargo run -- ask "Generate a Rust function for..." --stream
```

### Multi-Provider Failover

Configure multiple providers for redundancy:

```toml
[[llm.providers]]
name = "huggingface-main"
base_url = "https://api-inference.huggingface.co/v1"

[[llm.providers]]
name = "huggingface-backup"
base_url = "https://backup-endpoint.huggingface.cloud"
```

## Common Use Cases

### Code Review

```bash
cargo run -- ask "Review this code for security issues:" < main.rs
```

### Documentation Generation

```bash
cargo run -- ask "Generate comprehensive documentation for this Python module" < module.py
```

### Test Generation

```bash
cargo run -- ask "Write unit tests for this function" < function.ts
```

### Code Refactoring

```bash
cargo run -- ask "Refactor this code for better performance" < slow_code.py
```

## Advanced Configuration

### Token Budget Management

Control token usage for cost optimization:

```toml
[llm]
max_tokens = 4096
token_budget = 100000  # Monthly budget
```

### Model Switching

Dynamically switch models based on task complexity:

```toml
[llm.task_models]
code_review = "zai-org/GLM-4.7"
documentation = "zai-org/GLM-4.7"
testing = "zai-org/GLM-4.7"
```

### Prompt Caching

Optimize costs with prompt caching (where supported):

```toml
[llm]
enable_prompt_caching = true
cache_ttl_seconds = 3600
```

## Troubleshooting

### Authentication Errors

Verify your HF token is correctly set:

```bash
# Check if token is exported
echo $HF_TOKEN

# Test API access
curl -H "Authorization: Bearer $HF_TOKEN" \
  https://api-inference.huggingface.co/v1/models
```

### Rate Limiting

If you encounter rate limits:

1. Check your Hugging Face API usage dashboard
2. Consider using Dedicated Endpoints for higher quotas
3. Adjust request concurrency in `vtcode.toml`

### Model Not Found

Ensure the model exists and you have access:

```bash
# List available models
curl -H "Authorization: Bearer $HF_TOKEN" \
  https://api-inference.huggingface.co/v1/models
```

## Resources

-   [VT Code Repository](https://github.com/vinhnx/vtcode)
-   [VT Code Architecture Documentation](https://github.com/vinhnx/vtcode/blob/main/docs/ARCHITECTURE.md)
-   [Hugging Face Inference API Documentation](https://huggingface.co/docs/api-inference/index)
-   [Hugging Face Inference Endpoints](https://huggingface.co/docs/inference-endpoints/index)
-   [Supported Models on Hugging Face](https://huggingface.co/models)

## Support

For issues or questions:

-   **GitHub Issues**: [VT Code Issues](https://github.com/vinhnx/vtcode/issues)
-   **Hugging Face Support**: [HF Support](https://huggingface.co/support)
-   **Documentation**: [VT Code Docs](https://github.com/vinhnx/vtcode/tree/main/docs)

## Contributing

VT Code is open source and welcomes contributions. See [CONTRIBUTING.md](https://github.com/vinhnx/vtcode/blob/main/CONTRIBUTING.md) for guidelines.
