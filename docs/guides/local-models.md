# Local Models: Ollama, LM Studio & llama.cpp

VT Code can run models entirely on your machine through three local inference
backends. This guide explains when local makes sense, how it compares to remote
APIs, what hardware you need, and how to get reliable results.

> **Status: experimental.** Local inference is less reliable than remote APIs.
> Tool use, reasoning, and streaming fidelity depend on the model and the
> backend you choose. Remote providers (OpenAI, Anthropic, Gemini, etc.) are
> recommended for production work; use local models for privacy, offline use,
> or cost-free experimentation.

## Local vs Remote

| Dimension | Local (Ollama / LM Studio / llama.cpp) | Remote API |
|-----------|----------------------------------------|------------|
| **Privacy** | Data never leaves your machine | Sent to a third-party API |
| **Cost** | Free after hardware | Per-token billing |
| **Latency** | Limited by your CPU/GPU | Typically faster, datacenter GPUs |
| **Model quality** | Smaller open-weight models | Frontier models (GPT-5.x, etc.) |
| **Tool use / reasoning** | Works, but less consistent | Most reliable |
| **Setup** | You manage server + model | Zero setup |
| **Reliability** | Server/load state must be correct | Generally "just works" |

**Recommendation:** use remote APIs as the default. Switch to a local model
when you need offline access, maximum privacy, or want to avoid API costs for
lightweight tasks.

## Hardware & model sizing

Local performance is dominated by memory bandwidth and VRAM. Rough guidance:

| Tier | VRAM / RAM | Example models | Use case |
|------|-----------|----------------|----------|
| Tiny | 8–16 GB | `gemma-4-e4b` (llama.cpp), `llama-3.1-8b` (LM Studio) | Quick edits, autocomplete-style help |
| Mid | 16–32 GB | `gpt-oss:20b`, `gemma-3-12b` | General coding assistance |
| High | 32–64 GB | `gemma-4-26b-a4b`, `step-3.5-flash` | Heavier agentic workflows |
| Max | 64 GB+ | Multiple / larger MoEs | Long-horizon tasks |

Quantized GGUF models (llama.cpp) run with far less VRAM than full-precision
weights. Prefer Q4/Q5 quantizations for the best speed/quality trade-off.

## Getting reliable results

Local generation fails most often for two reasons: **the server is stopped**
or **the model isn't loaded**. VT Code now detects both *before* generating and
returns an exact recovery command (e.g. `ollama pull gpt-oss:20b` or
`/local start ollama`) instead of a cryptic error.

Checklist:

1. **Start the server.** Use `/local status` to check, or `/local start <provider>`.
   - Ollama: `ollama serve`
   - LM Studio: `lms server start` (or the app's "Run LLM server on login")
   - llama.cpp: `llama-server -m /path/to/model.gguf --port 8080`, or set
     `LLAMACPP_MODEL_PATH` and let VT Code manage it.
2. **Make sure the model is available.**
   - Ollama: `ollama pull <model>` (then it auto-loads on request).
   - LM Studio: load the model in the app or `lms load <model>`.
   - llama.cpp: the model passed to `llama-server` is the only loaded model.
3. **Pick a model that actually exists.** The model picker shows models that
   are currently loaded when the server is running. If you select a preset whose
   model isn't loaded, VT Code tells you the exact pull/load command.
4. **Use `/local troubleshoot`** for guided diagnostics.

## Known limitations

- **Ollama** — best-supported local backend. Parallel tool calls and some
  `tool_choice` modes are not supported; requests fall back gracefully.
  Reasoning models use the native `think` parameter.
- **LM Studio** — uses the OpenAI-compatible `/v1` endpoint. Only one model is
  active at a time; select the loaded model in the picker. Some models reject
  parameters that the OpenAI API accepts (e.g. `parallel_tool_calls`) — if you
  see a 400, switch to a model that supports tools or disable parallel tools.
- **llama.cpp** — most "managed" backend (auto-starts via `LLAMACPP_MODEL_PATH`).
  Feature support (reasoning, tools, structured output) varies by build and
  model; verify your server was built with the needed extensions.

## See also

- [Local Inference Servers (CLI & `/local` command)](../providers/local-servers.md)
- [Ollama Provider](ollama.md) · [LM Studio Provider](lmstudio.md) · [llama.cpp Provider](llamacpp.md)
