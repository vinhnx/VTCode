# Local Inference Servers

VT Code manages local LLM inference servers through the `/local` command. Control Ollama, LM Studio, and llama.cpp directly from the TUI.

## Quick Start

```
/local                          Open interactive server manager
/local status                   Check all local servers
/local start ollama             Start a specific server
/local troubleshoot             Diagnose issues
```

## Supported Providers

| Provider | Default Endpoint | Binary | Install |
|----------|-----------------|--------|---------|
| **Ollama** | `http://localhost:11434` | `ollama` | `brew install ollama` |
| **LM Studio** | `http://localhost:1234/v1` | `lms` | https://lmstudio.ai/download |
| **llama.cpp** | `http://localhost:8080/v1` | `llama-server` | https://llama.app |

---

## Running Ollama

### Install

```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh

# Windows
# Download from https://ollama.com/download
```

### Start the server

```bash
ollama serve
```

Or use VT Code: `/local start ollama`

### Pull a model

```bash
ollama pull gemma3
ollama pull qwen3.5:7b
ollama pull llama3.1:8b
```

### Run a model interactively

```bash
ollama run gemma3
```

### List available models

```bash
ollama ls
```

### List running models

```bash
ollama ps
```

### Stop a running model

```bash
ollama stop gemma3
```

### Verify the server

```bash
curl http://localhost:11434/api/tags
```

### Logs

```bash
cat ~/.ollama/logs/server.log
```

### Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `OLLAMA_BASE_URL` | Server URL | `http://localhost:11434` |
| `OLLAMA_HOST` | Listen address | `127.0.0.1:11434` |

### VT Code integration

```
/local ollama              Open Ollama actions in TUI
/local status ollama       Check if Ollama is running
/local start ollama        Start the Ollama server
/local configure ollama    Show environment config
/local troubleshoot ollama Diagnose connection issues
```

---

## Running LM Studio

### Install

Download from https://lmstudio.ai/download (macOS, Windows, Linux).

The `lms` CLI ships with LM Studio. Verify:

```bash
lms --help
```

If `lms` is not on PATH, it may be at `~/.lmstudio/bin/lms`.

### Start the server

```bash
lms server start
```

Or use VT Code: `/local start lmstudio`

Custom port:

```bash
lms server start --port 3000
```

Bind to all interfaces (for network access):

```bash
lms server start --bind 0.0.0.0
```

### Stop the server

```bash
lms server stop
```

### Check server status

```bash
lms server status
lms server status --json --quiet
```

### Download models

```bash
lms get gemma3
lms get deepseek-r1
lms get qwen2.5-7b-instruct --mlx
```

### List models on disk

```bash
lms ls
```

### List models in memory

```bash
lms ps
```

### Load a model

```bash
lms load
lms load openai/gpt-oss-20b --identifier="my-model"
```

### Unload a model

```bash
lms unload
lms unload --all
```

### Run LM Studio headless (as a service)

1. Open LM Studio app
2. Go to Settings (Cmd+,)
3. Enable "Run LLM server on machine login"
4. Exit the app -- it runs in the background

### Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `LMSTUDIO_BASE_URL` | Server URL | `http://localhost:1234/v1` |

### VT Code integration

```
/local lmstudio              Open LM Studio actions in TUI
/local status lmstudio       Check if server is running
/local start lmstudio        Start the server via lms CLI
/local configure lmstudio    Show environment config
/local troubleshoot lmstudio Diagnose connection issues
```

---

## Running llama.cpp

### Install

```bash
# macOS (Homebrew)
brew install llama.cpp

# Or download from https://llama.app

# Build from source
git clone https://github.com/ggml-org/llama.cpp
cd llama.cpp
cmake -B build
cmake --build build --config Release
```

The server binary is `llama-server`.

### Start the server

```bash
llama-server -m /path/to/model.gguf --port 8080
```

Or use VT Code (auto-start): set `LLAMACPP_MODEL_PATH` and VT Code manages everything.

### Verify the server

```bash
curl http://localhost:8080/health
```

### List loaded models

```bash
curl http://localhost:8080/v1/models
```

### Download a model

Download `.gguf` files from https://huggingface.co or https://llama.app/models:

- https://llama.app/models/Qwen3.6-27B
- https://llama.app/models/gemma-4-26B-A4B
- https://llama.app/models/gpt-oss-20b
- https://llama.app/models/Step-3.5-Flash

### Auto-start with VT Code

Set `LLAMACPP_MODEL_PATH` to a `.gguf` file. VT Code will:

1. Detect the model path
2. Start `llama-server` automatically when needed
3. Wait for the server to be ready
4. Connect and serve requests

```bash
export LLAMACPP_MODEL_PATH=/path/to/model.gguf
```

### Extra arguments

Pass additional arguments to `llama-server`:

```bash
export LLAMACPP_EXTRA_ARGS="--ctx-size 4096 --n-gpu-layers 99"
```

### Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `LLAMACPP_BASE_URL` | Server URL | `http://localhost:8080/v1` |
| `LLAMACPP_MODEL_PATH` | Model file for auto-start | (none) |
| `LLAMACPP_BINARY_PATH` | Path to `llama-server` | search `PATH` |
| `LLAMACPP_EXTRA_ARGS` | Extra server arguments | (none) |
| `LLAMACPP_STARTUP_TIMEOUT_SECONDS` | Startup timeout | 60 |

### VT Code integration

```
/local llamacpp              Open llama.cpp actions in TUI
/local status llamacpp       Check if server is running
/local start llamacpp        Start server (requires LLAMACPP_MODEL_PATH)
/local configure llamacpp    Show environment config
/local troubleshoot llamacpp Diagnose connection issues
```

---

## Command Reference

### Interactive Mode

```
/local
```

Opens an inline modal showing all providers with status. Select a provider to see actions.

### Explicit Subcommands

```
/local status                          Check all servers
/local status ollama                   Check Ollama only
/local start ollama                    Start Ollama
/local start lmstudio                  Start LM Studio
/local start llamacpp                  Start llama.cpp
/local stop ollama                     Stop Ollama
/local configure                       Show all env vars
/local configure llamacpp              Show llama.cpp config
/local troubleshoot                    Diagnose all servers
/local troubleshoot ollama             Diagnose Ollama
```

### Provider Shortcuts

```
/local ollama                          Open Ollama actions
/local lmstudio                        Open LM Studio actions
/local llamacpp                        Open llama.cpp actions
/local ollama status                   Check Ollama status
```

### Aliases

- `lm-studio`, `lm_studio` -> `lmstudio`
- `llama.cpp`, `llama-cpp`, `llama_cpp` -> `llamacpp`

---

## Troubleshooting

### Server not detected

1. Check if binary is installed: `/local configure <provider>`
2. Try starting: `/local start <provider>`
3. Run diagnostics: `/local troubleshoot <provider>`

### Ollama not responding

```bash
# Check if running
curl http://localhost:11434/api/tags

# Start manually
ollama serve

# Check logs
cat ~/.ollama/logs/server.log
```

### LM Studio not responding

```bash
# Check status
lms server status --json

# Start server
lms server start

# Open the app
open -a "LM Studio"
```

### llama.cpp not responding

```bash
# Check health
curl http://localhost:8080/health

# Check model path
echo $LLAMACPP_MODEL_PATH

# Check binary
which llama-server
```

## See Also

- [Ollama Provider Guide](ollama.md)
- [LM Studio Provider Guide](lmstudio.md)
- [llama.cpp Provider Guide](llamacpp.md)
