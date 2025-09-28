# OpenAI Function Calling Guide

## Official contract essentials
- Expose tools using the canonical OpenAI structure:
  `{ "type": "function", "function": { ... } }`.
- Provide JSON Schema parameter definitions so the model can validate argument payloads before invocation.
- Keep tool descriptions concise and action-oriented to help the model select the correct function.

## VT Code implementation details
- `OpenAIProvider::serialize_tools` normalizes internal tool definitions
  into the OpenAI function wrapper before every request.
- Both chat-completions and responses API payloads reuse the shared
  serialization path, guaranteeing identical behavior.
- Regression tests exercise the serialization logic to ensure tools are
  emitted with the required `function` object and never leak legacy
  fields.

## Testing checklist
- `cargo test openai` – validates the request builders for both chat and responses flows.
