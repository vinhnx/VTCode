# Provider Guides

This index collects provider-specific guides for configuring VT Code with different LLM backends.

## Google Gemini

-   Configuration details are covered in the main [Getting Started guide](../user-guide/getting-started.md#api-requirements).
-   Models and constants are defined in [`crates/codegen/vtcode-core/src/config/constants.rs`](../../crates/codegen/vtcode-core/src/config/constants.rs).

## OpenAI

-   **Official docs:**
    -   [API reference index](https://developers.openai.com/api/reference/llms.txt)
    -   [Models catalog](https://developers.openai.com/api/docs/models)
- Follow the [Getting Started guide](../user-guide/getting-started.md#api-requirements) for API key setup.
-   See [`crates/codegen/vtcode-core/src/config/constants.rs`](../../crates/codegen/vtcode-core/src/config/constants.rs) for the latest supported models.
-   GPT-5.2 reference: See OpenAI documentation for latest models
-   VT Code's default OpenAI profile is `gpt-5.4` with `reasoning_effort = "none"` and `verbosity = "medium"`; raise reasoning only when the task shape justifies the extra latency.
-   VT Code applies a compact GPT-5.4 prompt contract rather than a verbatim cookbook prompt: compact outputs, low-risk follow-through, dependency-aware tool use, completeness checks, verification, and conditional grounding/citation rules.
-   File inputs are supported for native OpenAI Responses API requests through `input_file` parts.
-   Supported file input fields in VT Code message parts: `file_id`, `file_data`, `file_url`, `filename`.
-   `file_url` is Responses API only; VT Code rejects `file_url` when a request uses Chat Completions.
-   VT Code only upgrades local non-image file refs such as `@report.pdf` and `@"Quarterly Deck.pptx"` into structured file attachments for native OpenAI Responses sessions on `api.openai.com`.
-   Remote external document URLs such as `@https://example.com/letter.pdf` are also only elevated to structured `file_url` inputs for native OpenAI Responses sessions.
-   ChatGPT subscription sessions, OpenAI-compatible endpoints, and other providers keep non-image `@file` refs as plain text plus file-reference metadata so the agent can resolve the path and read the file with tools.
-   Raw image paths still use the existing multimodal image path flow. Non-image files require explicit `@...` references.
-   Official OpenAI Responses replays now preserve assistant phase metadata for replayed assistant history (`commentary` for preambles/progress updates, `final_answer` for completed answers) when the target GPT model supports it. VT Code does not send this field to Chat Completions, tool/user items, or non-native OpenAI-compatible endpoints.
-   OpenAI Responses hosted tools currently map through `ToolDefinition` for `web_search`, `file_search`, hosted `tool_search`, and remote `mcp`, with hosted config passed through directly on each tool entry.
-   OpenAI hosted shell mounts are configured through `provider.openai.hosted_shell` in `vtcode.toml`.
-   Hosted shell skill mounts support both `skill_reference` and `inline` bundle entries; VT Code forwards them to OpenAI but does not upload/create hosted skills in this path.
-   This hosted-shell workflow is separate from VT Code's local `SKILL.md` filesystem skills.
-   For large corpora, prefer File Search/Retrieval instead of sending full files inline.
-   For spreadsheet-heavy analysis, use Hosted Shell workflows instead of large inline sheet prompts.

## Anthropic

-   Key management and defaults mirror the Gemini/OpenAI flow in [Getting Started](../user-guide/getting-started.md#api-requirements).
-   Supported model IDs live in [`crates/codegen/vtcode-core/src/config/constants.rs`](../../crates/codegen/vtcode-core/src/config/constants.rs).

## GitHub Copilot

-   **Guide:** [GitHub Copilot Managed Auth](./copilot.md)
-   **Runtime dependency:** `copilot` must be installed and runnable for login/logout
-   **Optional fallback:** `gh` is only used when VT Code probes an existing GitHub CLI auth session
-   **Commands:** `vtcode login copilot`, `vtcode logout copilot`, `/login copilot`, `/logout copilot`

## OpenRouter Marketplace

-   **Guide:** [OpenRouter Integration](./openrouter.md)
-   **Official docs:**
    -   [API overview](https://openrouter.ai/docs/api-reference/overview/llms)
    -   [Streaming](https://openrouter.ai/docs/api-reference/streaming/llms)
    -   [Model catalog](https://openrouter.ai/docs/llms)
-   Default model: `xiaomi/mimo-v2.5-pro` (VT Code's default). Xiaomi MiMo V2.5 and V2.5 Pro are also available.
-   **Xiaomi MiMo models:**
    -   `xiaomi/mimo-v2.5-pro` — flagship agentic model, 1M context, reasoning + tool calls
    -   `xiaomi/mimo-v2.5` — omnimodal model, 1M context, reasoning + tool calls

## Atlas Cloud

-   **Guide:** [Atlas Cloud Integration](./atlascloud.md)
-   **Official docs:**
    -   [LLM / Chat](https://www.atlascloud.ai/docs/models/llm)
    -   [FAQ](https://www.atlascloud.ai/docs/en/faq)
-   **Integration mode:** configure Atlas Cloud through VT Code's `[[custom_providers]]` support because the LLM endpoint is OpenAI-compatible.
-   **Base URL:** `https://api.atlascloud.ai/v1`
-   **Recommended model:** start with `deepseek-ai/deepseek-v4-flash` (DeepSeek's latest flash model, 1M context, $0.14/M input tokens).

## Xiaomi MiMo

-   **Provider key:** `mimo`
-   **Docs:** [Xiaomi MiMo Platform](https://platform.xiaomimimo.com/docs/en-US/welcome)
-   **Pricing:** [Pay-as-you-go](https://platform.xiaomimimo.com/docs/en-US/price/pay-as-you-go) · [Subscription](https://platform.xiaomimimo.com/docs/en-US/price/tokenplan/subscription) · [Quick Access](https://platform.xiaomimimo.com/docs/en-US/price/tokenplan/quick-access)
-   **Setup:** Set `MIMO_API_KEY` or use the MiMo provider in VT Code's configuration
-   **Models:**
    -   `mimo-v2.5-pro` — flagship agentic model, 1M context, deep thinking
    -   `mimo-v2.5` — omnimodal model (text, image, audio, video), 1M context

## Ollama Local & Cloud Models

-   **Guide:** [Local Inference Servers](./local-servers.md) (unified `/local` command)
-   **Setup:** Install and run Ollama locally ([official install](https://ollama.com/download))
-   **Configuration:** Local usage needs no key; set `OLLAMA_API_KEY` to access Ollama Cloud
-   **Default model:** Any locally available model (e.g., `llama3:8b`, `mistral:7b`, `qwen3:1.7b`)
-   **Cloud models:** Use IDs like `gpt-oss:120b-cloud` with `OLLAMA_BASE_URL=https://ollama.com`
-   **Custom Models:** Use the `custom-ollama` option in the model picker to enter any locally or cloud-available model ID
-   **Base URL:** Configurable via `OLLAMA_BASE_URL` environment variable (defaults to `http://localhost:11434`)
-   **Features:** Streaming, structured tool calling (including Ollama's web search tools), and thinking traces when `reasoning_effort` is enabled

## LM Studio Local Server

-   **Guide:** [LM Studio Provider Guide](./lmstudio.md)
-   **Server:** Enable the OpenAI-compatible Developer server in LM Studio (defaults to `http://localhost:1234/v1`)
-   **Environment:** Optional `LMSTUDIO_API_KEY` when auth is enabled; override host/port via `LMSTUDIO_BASE_URL`
-   **Default model:** `lmstudio-community/meta-llama-3.1-8b-instruct` (local inference)
-   **Catalog:** Also ships with `lmstudio-community/meta-llama-3-8b-instruct`, `lmstudio-community/qwen2.5-7b-instruct`, `lmstudio-community/gemma-2-2b-it`, `lmstudio-community/gemma-2-9b-it`, and `lmstudio-community/phi-3.1-mini-4k-instruct`, plus any custom GGUF models you expose
-   **Features:** Streaming, tool calling, structured output, and reasoning effort passthrough via the shared OpenAI surface

## llama.cpp Local Server

-   **Guide:** [llama.cpp Provider Guide](./llamacpp.md)
-   **Server:** VT Code targets `llama-server` and defaults to `http://localhost:8080/v1`
-   **Environment:** `LLAMACPP_BASE_URL` overrides the endpoint; `LLAMACPP_MODEL_PATH` enables VT Code-managed startup
-   **Managed startup:** VT Code can launch `llama-server -m /path/to/model.gguf --port ...` when the endpoint is localhost and a GGUF path is configured
-   **Starter catalog:** `gpt-oss-20b`, `qwen3.6-27b`, `qwen3.6-35b-a3b`, `gemma-4-26b-a4b`, `gemma-4-e4b`, and `step-3.5-flash`
-   **Features:** Streaming, dynamic `/v1/models` discovery, local no-auth defaults, and OpenAI-compatible request handling

## Evolink Multi-Model Gateway

-   **Provider key:** `evolink`
-   **Official docs:** [Evolink Docs](https://docs.evolink.ai/llms.txt)
-   **Base URL:** `https://direct.evolink.ai/v1`
-   **Auth:** `EVOLINK_API_KEY` environment variable
-   **Setup:** Set `EVOLINK_API_KEY` from [Evolink dashboard](https://evolink.ai/dashboard/keys), then configure `provider = "evolink"` in `vtcode.toml`
-   **Models:**
    -   `evolink/gpt-5.2` (default)
    -   `evolink/gpt-5.5`
    -   `evolink/deepseek-v4-pro`
    -   `evolink/deepseek-v4-flash`
    -   `evolink/doubao-seed-2.0-pro`
    -   `evolink/gemini-3.1-pro-preview`
    -   `evolink/gemini-3.5-flash`
    -   `evolink/MiniMax-M3`
    -   `evolink/claude-sonnet-4-6`
    -   `evolink/claude-opus-4-8`
    -   `evolink/claude-haiku-4-5-20251001`
-   **Features:** OpenAI-compatible gateway exposing many upstream models behind one endpoint. Evolink serves models under bare upstream names (e.g. `gpt-5.2`) that collide with VT Code's first-class providers, so curated model IDs are namespaced as `evolink/<model>`. The provider strips the prefix before sending requests upstream.

## Anthropic API Compatibility Server

VT Code provides compatibility with the Anthropic Messages API to help connect existing applications to VT Code, including tools like Claude Code.

- **Feature:** Anthropic API compatibility server
- **Command:** `vtcode anthropic-api --port 11434`
- **Endpoint:** `/v1/messages` (mirrors Anthropic Messages API)
- **Environment variables:**
  - `ANTHROPIC_AUTH_TOKEN=ollama` (required but ignored)
  - `ANTHROPIC_BASE_URL=http://localhost:11434`
  - `ANTHROPIC_API_KEY=ollama` (required but ignored)
- **Features:** Streaming, tool calling, vision support, multi-turn conversations

## Z.AI (ZAI)

-   **Provider key:** `zai`
-   **Official docs:** [Z.AI Platform](https://z.ai/docs)
-   **Auth:** `ZAI_API_KEY` environment variable
-   **Setup:** Set `ZAI_API_KEY` from Z.AI platform, then configure `provider = "zai"` in `vtcode.toml`
-   **Models:**
    -   `glm-5.2` — flagship model for long-horizon tasks, 1M context, reasoning + tool calls
    -   `glm-5.1` — next-gen foundation model, reasoning + tool calls
    -   `glm-4.7` — efficient model for general tasks
-   **Default:** `glm-5.1`
-   **Features:** Streaming, tool calling, reasoning effort support

## Moonshot (Kimi)

-   **Provider key:** `moonshot`
-   **Official docs:** [Moonshot Platform](https://platform.moonshot.ai/docs)
-   **Auth:** `MOONSHOT_API_KEY` environment variable
-   **Setup:** Set `MOONSHOT_API_KEY` from Moonshot platform, then configure `provider = "moonshot"` in `vtcode.toml`
-   **Models:**
    -   `kimi-k2.7-code` — most capable coding model with long-horizon coding breakthrough, 256K context
    -   `kimi-k2.6` — multimodal model for coding and UI/UX generation, 1M context
    -   `kimi-k2.5` — enhanced reasoning model
-   **Default:** `kimi-k2.7-code`
-   **Features:** Streaming, tool calling, reasoning support (K2.7 Code), multimodal input (K2.6)

## StepFun

-   **Provider key:** `stepfun`
-   **Official docs:** [StepFun Platform](https://platform.stepfun.ai/docs)
-   **Auth:** `STEPFUN_API_KEY` environment variable
-   **Setup:** Set `STEPFUN_API_KEY` from StepFun platform, then configure `provider = "stepfun"` in `vtcode.toml`
-   **Models:**
    -   `step-3.7-flash` — efficient reasoning model based on sparse MoE architecture
-   **Default:** `step-3.7-flash`
-   **Features:** Streaming, tool calling, reasoning support

## MiniMax

-   **Provider key:** `minimax`
-   **Official docs:** [MiniMax Platform](https://platform.minimax.io/docs)
-   **Auth:** `MINIMAX_API_KEY` environment variable
-   **Setup:** Set `MINIMAX_API_KEY` from MiniMax platform, then configure `provider = "minimax"` in `vtcode.toml`
-   **Models:**
    -   `MiniMax-M3` — frontier multimodal coding model, 1M context
    -   `MiniMax-M2.7` — recursive self-improvement with enhanced reasoning
    -   `MiniMax-M2.5` — efficient model for general tasks
-   **Default:** `MiniMax-M3`
-   **Features:** Streaming, tool calling

## HuggingFace

-   **Provider key:** `huggingface`
-   **Official docs:** [HuggingFace Inference API](https://huggingface.co/docs/api-inference)
-   **Auth:** `HUGGINGFACE_API_KEY` environment variable
-   **Setup:** Set `HUGGINGFACE_API_KEY` from HuggingFace settings, then configure `provider = "huggingface"` in `vtcode.toml`
-   **Features:** Access to various models through HuggingFace's inference API, including models from Z.AI, DeepSeek, and other providers

## Poolside

-   **Provider key:** `poolside`
-   **Auth:** `POOLSIDE_API_KEY` environment variable
-   **Setup:** Set `POOLSIDE_API_KEY` from Poolside platform, then configure `provider = "poolside"` in `vtcode.toml`

> ℹ Additional provider-specific guides will be added as new integrations land in VT Code.
