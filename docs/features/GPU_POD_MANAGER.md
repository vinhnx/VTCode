# GPU Pod Manager

VT Code includes a `pods` command family for managing remote GPU-backed model pods over SSH.
It is a backend + CLI feature in v1, with no TUI integration and no `vtcode.toml` section.

## Overview

The pod manager keeps a small amount of state under `~/.vtcode/pods/`:

- `state.json` stores the active pod, running model names, ports, PIDs, and GPU assignments.
- `catalog.json` stores the model/profile catalog used by `pods known-models` and `pods start`.

The current implementation is SSH-only. VT Code uploads a run script and a wrapper script to the
remote host, starts the model in a detached session, and tracks the process locally.

## Commands

```bash
vtcode pods start --name llama --model meta-llama/Llama-3.1-8B-Instruct \
  --ssh "ssh root@gpu.example.com" \
  --gpu 0:A100 --gpu 1:A100 \
  --gpus 2 \
  --memory 90 \
  --context 32k

vtcode pods list
vtcode pods logs --name llama
vtcode pods stop --name llama
vtcode pods stop-all
vtcode pods known-models
```

### `start`

Launches a model on the active pod.

- `--name` is the local label VT Code stores in `state.json`.
- `--model` is the remote model identifier passed to the launch script.
- `--ssh` provides the SSH target used for all remote operations.
- `--gpu` entries define the visible GPU inventory as `ID:NAME`.
- `--gpus` requests a specific GPU count when matching a profile.
- `--profile` forces a catalog profile when you do not want automatic selection.
- `--memory` and `--context` override the profile's default vLLM arguments.

### `list`

Shows the running models for the active pod and classifies each one as:

- `running`
- `starting`
- `crashed`
- `dead`

### `logs`

Streams the remote log file for the selected model.

### `known-models`

Splits the catalog into compatible and incompatible profiles for the active pod.
Compatibility is based on the pod's GPU inventory and the profile's GPU requirements.

### `stop` and `stop-all`

Stops a single model or every tracked model on the active pod, then updates the persisted state.

## Behavior Notes

- The command family is isolated from the existing `models` commands.
- The first version does not add config-file support or TUI controls.
- The bundled catalog is intentionally editable at runtime by replacing `catalog.json`.
- The default launch flow assumes `vllm serve`, but the command template is stored per profile.

## Testing

Run the pod-focused tests with:

```bash
cargo test -p vtcode-core pods -- --nocapture
```
