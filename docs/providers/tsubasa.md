# Tsubasa

Use Tsubasa with VT Code's custom provider and its `ask` command for text requests.
Save this configuration outside the project, then select its absolute path with
`--config`. VT Code rejects custom provider endpoints supplied by ordinary
repository-controlled configuration layers.

```toml
[agent]
provider = "tsubasa"
default_model = "tsubasa-fast"
api_key_env = "TSUBASA_API_KEY"

[[custom_providers]]
name = "tsubasa"
display_name = "Tsubasa"
base_url = "https://api.tsubasa.sh/v1"
api_format = "openai-chat"
api_key_env = "TSUBASA_API_KEY"
model = "tsubasa-fast"
models = ["tsubasa-fast", "tsubasa-pro"]
context_window = 32768
supports_tools = false
supports_reasoning = false
supports_reasoning_effort = false
supports_vision = false
supports_structured_output = false
supports_parallel_tool_calls = false
supports_context_caching = false
supports_responses_compaction = false
supports_context_edits = false
```

Set `TSUBASA_API_KEY`, then run:

```sh
: "${TSUBASA_API_KEY:?Set TSUBASA_API_KEY}"
vtcode ask "Say Hello." --config /absolute/path/to/tsubasa.toml \
  --provider tsubasa --model tsubasa-fast --api-key-env TSUBASA_API_KEY
```

Use `--model tsubasa-pro` for Pro. `ask` sends a single prompt without tools or a
persistent agent session. This command omits an explicit output limit, so
Tsubasa applies its 512-token default. Keep the prompt and output within the
shared 32,768-token context window.
