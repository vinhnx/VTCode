# Tool-free planning provider audit

`LLMRequest.tool_choice = ToolChoice::None` disables new callable tools; it does
not delete recorded assistant calls or matching results. Runtime rejection of
unexpected calls remains a second boundary. A stable prompt prefix is useful
only when the transport actually enforces disabling calls.

This source audit covers every exported adapter in `vtcode-llm/src/providers`.
The classifications describe request serialization, not a live guarantee about
an external service. Both streaming and non-streaming use the same serialization
except where noted. Empty provider output remains a separate failure category;
these fixes do not prove why a model returned no content.

| Adapter / route | Tool-free behavior | Source / disposition |
| --- | --- | --- |
| OpenAI Chat Completions | Keep catalog with explicit `tool_choice: "none"` | `openai/request_builder/chat.rs`; enforced wire control |
| OpenAI Responses, HTTP and WebSocket | Keep catalog with explicit `tool_choice: "none"` | `openai/request_builder/responses.rs`; enforced wire control |
| Anthropic | Keep catalog with `tool_choice: {type: "none"}` | `anthropic/request_builder/tools.rs`; forced-choice downgrades do not alter none |
| MiniMax | Same as Anthropic | `minimax.rs` delegates to `AnthropicProvider` |
| Gemini GenerateContent | Keep function-only catalog with `functionCallingConfig.mode: "NONE"`; omit hosted/mixed catalogs | `gemini/helpers/request.rs`; hosted-tool omission fixed; constrained calls bypass incompatible explicit caches |
| Gemini Interactions | Keep function-only catalog with tool choice `none`; omit hosted/mixed catalogs | `gemini/helpers/request.rs`; hosted-tool omission fixed |
| Ollama | Omit callable catalog | `ollama.rs`; no transmitted tool-choice control; fixed |
| StepFun Responses | Omit callable catalog and choice | `stepfun.rs`; only auto is supported; fixed |
| Merge Gateway native Responses | Omit callable catalog and choice | `merge_gateway/provider_impl.rs`; Bedrock routes reject none; fixed |
| Merge Gateway legacy Chat | Omit callable catalog and choice | `merge_gateway/spec.rs`; same restriction; fixed |
| Meta AI | Omit callable catalog and choice; accept explicit none | `meta.rs`; only auto is supported with tools; fixed |
| Z.AI | Omit callable catalog, choice, and tool-stream flag | `zai.rs`; never coerce none to auto; fixed |
| OpenResponses native | Keep catalog with explicit `tool_choice: "none"` | `openresponses/provider/native.rs`; forced functions use the flat native shape, shared with OpenAI Responses |
| OpenResponses Chat fallback | Keep catalog with explicit `tool_choice: "none"` | `openresponses/provider/legacy.rs`; previously omitted choice; fixed |
| OpenRouter | Keep catalog with explicit `tool_choice: "none"` | `openrouter/request_builder.rs` |
| Hugging Face, generate and stream | Keep catalog with explicit `tool_choice: "none"` | `huggingface.rs`; both independent request builders inspected |
| DeepSeek | Keep catalog with explicit none | `deepseek.rs` uses shared `openai_compat.rs` |
| MiMo | Keep catalog with explicit none | `mimo.rs` uses shared compat builder |
| Mistral | Keep catalog with explicit none | `mistral.rs` choice override preserves explicit none |
| Moonshot | Keep catalog with explicit none | `moonshot.rs` uses shared compat builder |
| NVIDIA | Keep catalog with explicit none | `nvidia.rs` uses shared compat builder |
| Poolside | Keep catalog with explicit none | `poolside.rs` uses shared compat builder |
| Qwen | Keep catalog with explicit none | `qwen.rs` uses shared compat builder |
| Vercel AI Gateway | Keep catalog with explicit none | `vercel.rs` uses shared compat builder |
| xAI | Keep catalog with explicit none | `xai.rs` uses shared compat builder |
| OpenCode Go / Zen | Keep catalog with explicit none | `opencode_shared.rs` delegates to shared compat builder |
| Evolink Chat | Keep catalog with explicit none | `evolink.rs` uses shared compat builder |
| Evolink Anthropic route | Never advertises callable tools | Existing text-only serializer; broader tool support is outside this fix |
| LM Studio / llama.cpp | Same as OpenAI Chat | Wrappers delegate to `OpenAIProvider` |
| Custom profiles | Same as selected backend | `custom_provider.rs` preserves requested choice |
| Copilot prompt sessions | Omit custom tools and disable built-ins with an explicit empty allowlist in CLI and session payloads; distinguish tool-free cached clients | `copilot.rs`, `copilot/command.rs`, `copilot/acp_client.rs`; feature-gated. Ordinary requests reject inline VT Code tool definitions |

Regression coverage serializes both wire modes for changed HTTP adapters with
explicit auto controls. Ollama additionally verifies retained historical call
and result pairing. Merge tests cover every built-in model route and missing
vendor cache state. Copilot tests cover its separate tool slice and connection
signature; these do not establish control of tools independently owned by the
external Copilot agent.
