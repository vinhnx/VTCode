# vtcode-lmstudio

LM Studio OSS provider integration for VT Code. Interfaces with the local LM Studio server for model inference.

## Conventions

- This crate is internal (`publish = false`). It does not need crates.io-compatible dependency specs.
- Uses `reqwest` for HTTP calls to the LM Studio local server.
- Model discovery probes the LM Studio `/v1/models` endpoint. Cache results -- do not re-probe on every request.
- Test with `wiremock` for HTTP mocking. Do not require a running LM Studio instance for unit tests.

## Dependencies

- `reqwest` (HTTP client)
- `vtcode-commons` (shared utilities)
- `vtcode-config` (configuration)
