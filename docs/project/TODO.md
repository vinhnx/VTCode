https://developers.openai.com/codex/memories

# Memories

Memories are off by default and aren't available in the European Economic
Area, the United Kingdom, or Switzerland at launch. Enable them in Codex
settings, or set `memories = true` in the `[features]` table in
`~/.codex/config.toml`.

Memories let Codex carry useful context from earlier threads into future work.
After you enable memories, Codex can remember stable preferences, recurring
workflows, tech stacks, project conventions, and known pitfalls so you don't
need to repeat the same context in every thread.

Keep required team guidance in `AGENTS.md` or checked-in documentation. Treat
memories as a helpful local recall layer, not as the only source for rules that
must always apply.

## Enable memories

In the Codex app, enable Memories in settings.

For config-based setup, add the feature flag to `config.toml`:

```toml
[features]
memories = true
```

See [Config basics](https://developers.openai.com/codex/config-basic) for where Codex stores user-level
configuration and how Codex loads `~/.codex/config.toml`.

## How memories work

After you enable memories, Codex can turn useful context from eligible prior
threads into local memory files. Codex skips active or short-lived sessions,
redacts secrets from generated memory fields, and updates memories in the
background instead of immediately at the end of every thread.

Memories may not update right away when a thread ends. Codex waits until a
thread has been idle long enough to avoid summarizing work that's still in
progress.

## Memory storage

Codex stores memories under your Codex home directory. By default, that's
`~/.codex`. See [Config and state locations](https://developers.openai.com/codex/config-advanced#config-and-state-locations)
for how Codex uses `CODEX_HOME`.

The main memory files live under `~/.codex/memories/` and include summaries,
durable entries, recent inputs, and supporting evidence from prior threads.

Treat these files as generated state. You can inspect them when troubleshooting
or before sharing your Codex home directory, but don't rely on editing them by
hand as your primary control surface.

## Control memories per thread

In the Codex app and Codex TUI, use `/memories` to control memory behavior for
the current thread. Thread-level choices let you decide whether the current
thread can use existing memories and whether Codex can use the thread to
generate future memories.

Thread-level choices don't change your global memory settings.

## Configuration

Enable memories in the Codex app settings, or set `memories = true` in the
`[features]` section of `config.toml`.

For config file locations and the full list of memory-related settings, see the
[configuration reference](https://developers.openai.com/codex/config-reference).

Common memory-specific settings include:

- `memories.generate_memories`: controls whether newly created threads can be
  stored as memory-generation inputs.
- `memories.use_memories`: controls whether Codex injects existing memories into
  future sessions.
- `memories.extract_model`: overrides the model used for per-thread memory
  extraction.
- `memories.consolidation_model`: overrides the model used for global memory
  consolidation.

## Review memories

Don't store secrets in memories. Codex redacts secrets from generated memory
fields, but you should still review memory files before sharing your Codex home
directory or generated memory artifacts.

---

---

https://github.com/vinhnx/VTCode/issues/629

[Feature Request] Please add OpenCode Zen/ OpenCode Go as providers
Hi,

While I am aware that these providers can be somewhat configured by tweaking vtcode.toml it may be worth looking into supporting these providers by default as the popularity of OpenCode rises with their affordable plans.

Context:
https://opencode.ai/zen
https://opencode.ai/go

the docs for using their endpoints are as here:
https://opencode.ai/docs/go/#endpoints
https://opencode.ai/docs/zen/#endpoints
