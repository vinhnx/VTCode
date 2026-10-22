# Octen search integration

The built-in search implementation lives in `vtcode-core/src/tools/web_search/`. The fixed Octen endpoint returns
ranked highlights mapped to the existing title/URL/snippet result contract. `full_content.enable` is explicitly
false. Separate `web_fetch` approval and domain/network controls remain authoritative for page retrieval.

Credential resolution is shared with `/secret` through `vtcode_config::api_keys::resolve_octen_credential_with_mode`:
nonblank environment override, then scoped secure storage. Registry snapshots carry the configured storage mode;
keys are resolved on each uncached call, with no LLM authentication or paid key validation.

The state lock atomically admits requests against cooldown and session cap immediately before dispatch. Failed or
cancelled sends retain their reservation. Request preparation and credential errors consume none. Only nonempty
successes enter the TTL cache. Redirects are refused; streamed response bytes are bounded before JSON parsing.
External error bodies are never emitted. Local HTTP fixtures cover wire shape, statuses, redirect refusal, bounded
reads, cancellation accounting, and failure caching. See [usage, migration, pricing and pilot](../tools/web_search.md).

Low-level registry construction keeps web tools disabled when workspace configuration cannot be loaded, including
removed-provider migration errors. Explicit approval cannot replace missing network restrictions or storage-mode
configuration. Repair the configuration before constructing a new registry.

Octen writes are verified in the shared credential-storage boundary, including CLI and interactive migrations,
before workspace `.env` cleanup. This keeps all saving paths under the same read-back contract.
