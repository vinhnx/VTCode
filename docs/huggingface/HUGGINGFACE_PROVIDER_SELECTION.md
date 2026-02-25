# HuggingFace Model Support & Provider Selection

## Overview

The HuggingFace Inference Providers router supports automatic and explicit provider selection through model ID suffixes. When no suffix is provided, the router automatically selects the first available provider, making it simple to use while still allowing fine-grained control when needed.

## HuggingFace Suffix Syntax

### Supported Provider Selection Methods

1. **Automatic Selection** (Recommended): No suffix
   - `zai-org/GLM-4.7` - Auto-selects first available provider

2. **Explicit Provider**: `:provider-name`
   - `deepseek-ai/DeepSeek-V3.2:novita` - Force Novita provider
   - `openai/gpt-oss-20b:together` - Force Together provider

3. **Price Selection**: `:cheapest`
   - `deepseek-ai/DeepSeek-R1:cheapest` - Use cheapest available provider

4. **Performance Selection**: `:fastest`
   - `openai/gpt-oss-120b:fastest` - Use fastest available provider

## Supported Models by Provider

### Default HuggingFace Router
These models are available on the default HuggingFace router without suffix:

| Model | Reasoning | Tools | Context |
|-------|-----------|-------|---------|
| `google/gemma-2-2b-it` | No | Yes | 8K |
| `zai-org/GLM-4.5` | Yes | Yes | 131K |
| `zai-org/GLM-4.6` | Yes | Yes | 131K |
| `zai-org/GLM-4.7` | Yes | Yes | 131K |
| `deepseek-ai/DeepSeek-R1` | Yes | Yes | 131K |
| `openai/gpt-oss-120b` | Yes | Yes | 131K |

### Via Novita Provider (`:novita` suffix required)
These models are **only** available via Novita on HuggingFace:

| Model | Reasoning | Tools | Context | Cost |
|-------|-----------|-------|---------|------|
| `MiniMaxAI/MiniMax-M2:novita` | No | No | 200K | $0.30/$1.20 |
| `MiniMaxAI/MiniMax-M2.5:novita` | Yes | Yes | 200K | TBD |
| `deepseek-ai/DeepSeek-V3.2:novita` | Yes | Yes | 160K | $0.27/$0.40 (cheapest) |
| `XiaomiMiMo/MiMo-V2-Flash:novita` | Yes | Yes | 256K | $0.10/$0.29 |

## Code Implementation

Updated `vtcode-core/src/llm/providers/openai.rs` to:

1. **Preserve valid HuggingFace suffixes** (`:novita`, `:fastest`, `:cheapest`, `:provider-name`)
2. **Reject invalid model formats** with clear error messages
3. **Support Novita models** that require explicit provider selection

### Key Logic
```rust
if is_huggingface {
    // The HuggingFace router supports provider selection via suffixes.
    // These are valid and should be preserved.
    
    let lower_model = request.model.to_ascii_lowercase();
    // Only reject MiniMax-M2 if no provider suffix (it requires :novita)
    if lower_model.contains("minimax-m2") && !request.model.contains(':') {
        return Err(provider::LLMError::Provider {
            message: "MiniMax models require the ':novita' suffix. \
                     Use 'MiniMaxAI/MiniMax-M2:novita' or 'MiniMaxAI/MiniMax-M2.5:novita' to access via Novita provider.",
            // ...
        });
    }
}
```

## Usage Examples

```bash
# Default provider (auto-selected)
vtcode ask "Hello" --provider huggingface --model "zai-org/GLM-4.7:zai-org"

# Force Novita provider for cost-optimized reasoning
vtcode ask "Solve this" --provider huggingface --model "deepseek-ai/DeepSeek-V3.2:novita"

# Use fastest provider
vtcode ask "Generate code" --provider huggingface --model "openai/gpt-oss-120b:fastest"

# Use cheapest provider
vtcode ask "Summarize" --provider huggingface --model "deepseek-ai/DeepSeek-R1:cheapest"
```

## Testing
- `cargo check` passes
- All unit tests pass (28/28)
- Models properly configured in `docs/models.json`
- Documentation updated with examples

## References
- [HuggingFace Inference Providers](https://huggingface.co/docs/inference-providers)
- [Provider Selection Guide](https://huggingface.co/docs/inference-providers/en/guide)
- [Chat Completion API](https://huggingface.co/docs/inference-providers/tasks/chat-completion)
