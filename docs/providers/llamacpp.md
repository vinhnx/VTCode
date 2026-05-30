# llama.cpp Provider Guide

VT Code includes a built-in `llamacpp` provider for local inference through [`llama-server`](https://llama.app/).

## What VT Code manages

- OpenAI-compatible transport to `http://localhost:8080/v1` by default
- Optional managed `llama-server` startup when a local GGUF path is configured
- Readiness polling through `/health`
- Dynamic model discovery through `/v1/models`
- A starter catalog for:
  - `gpt-oss-20b`
  - `qwen3.6-27b`
  - `qwen3.6-35b-a3b`
  - `gemma-4-26b-a4b`
  - `gemma-4-e4b`
  - `step-3.5-flash`

## Quick start

1. Install llama.cpp from [llama.app](https://llama.app/).
2. Download or build a GGUF model file locally.
3. Point VT Code at that file.

```bash
export VTCODE_PROVIDER=llamacpp
export LLAMACPP_MODEL_PATH=/absolute/path/to/model.gguf
```

If `llama-server` is on `PATH`, VT Code can start it automatically on the first request.

## Manual server mode

You can also run the server yourself:

```bash
llama-server -m /absolute/path/to/model.gguf --port 8080
```

Then point VT Code at it if you changed the default port or host:

```bash
export LLAMACPP_BASE_URL=http://localhost:8080/v1
```

## Configuration

### Environment variables

| Variable | Purpose | Default |
| --- | --- | --- |
| `LLAMACPP_BASE_URL` | OpenAI-compatible llama.cpp endpoint | `http://localhost:8080/v1` |
| `LLAMACPP_MODEL_PATH` | Local GGUF path for VT Code-managed startup | unset |
| `LLAMACPP_BINARY_PATH` | Override path to `llama-server` | PATH lookup |
| `LLAMACPP_EXTRA_ARGS` | Extra CLI flags appended to `llama-server` | unset |
| `LLAMACPP_STARTUP_TIMEOUT_SECONDS` | Startup/readiness timeout | `60` |

### TOML

```toml
[agent]
provider = "llamacpp"

[providers.llamacpp]
base_url = "http://localhost:8080/v1"
model = "gpt-oss-20b"
```

If `model` points at a local `.gguf` path, VT Code also treats it as a managed-start hint.

## Notes

- Managed startup is intentionally limited to localhost endpoints.
- In single-model mode, VT Code resolves the exact request model ID from `/v1/models` so the request matches what `llama-server` exposes.
- Tool calling depends on the loaded model and chat template support in llama.cpp.
