# Threat model for VTCode (vinhnx/VTCode)

## What this project does and where untrusted input enters

VTCode is an open-source terminal coding agent written in Rust. It plans, runs, and reviews
coding work via an interactive TUI (`vtcode`) and headless `vtcode exec`, calling LLMs
(hosted or local) and executing tools on the user's behalf.

Assume ALL of the following are attacker-controlled unless authenticated otherwise:

- Repository content being worked on (source files, `AGENTS.md`, `CLAUDE.md`, `.vtcode/rules/`,
  skills, plugins, task files) — these are context, NOT policy, and must not grant permissions
  or bypass the sandbox. See `docs/guides/security.md`.
- LLM output / tool-call arguments (prompts, patches, shell commands).
- MCP server responses, WebMCP bridge messages, ACP (Zed) messages, A2A messages.
- Web fetch / browser content, pasted terminal output, image/PDF attachments.
- User config files (`vtcode.toml`), session logs, checkpoint files on resume.
- Environment variables and files inside the workspace (symlinks, `..`, FIFOs, sockets).

Trusted (but still validate): CLI flags from the invoking user, OS-level sandbox primitives.

## Components that matter most

In priority order:

1. **Exec / sandbox boundary** — `crates/codegen/vtcode-bash-runner`, `crates/codegen/vtcode-safety`,
   `crates/codegen/vtcode-core/src/execpolicy/`, sandbox-aware launch paths.
   Bugs here = arbitrary command execution, sandbox escape, network exfil.
2. **Command safety + execution policy** — allowlist, per-command arg validation,
   path normalization, symlink escape, env leakage, fail-closed behavior.
   See `docs/development/COMMAND_SECURITY_MODEL.md`, `docs/development/EXECUTION_POLICY.md`.
3. **Tool dispatch** — `exec_command` / `write_stdin` / `apply_patch` / `code_search`,
   bounded diffs (`vtcode-diff`), patch application, HITL approval persistence
   (Approve Once vs Session vs Always).
4. **Credential handling** — `vtcode-auth` (OAuth, keyring/token storage), API keys in logs,
   `printenv` exposure, ATIF export / `ThreadEvent` contract (`vtcode-exec-events`).
5. **Protocol parsers** — MCP client, ACP server, WebMCP pairing/event replay,
   agent-plugins manifest parsing, config/schema parsing (`vtcode-config`,
   `vtcode-utility-tool-specs`), indexer / code search.
6. **Supply chain** — `build.rs`, `xtask`, release scripts, GitHub Actions, npm/homebrew wrappers.

Lower priority / out of scope:

- `contrib/`, `examples/`, `benches/`, demo GIFs, docs typos.
- UI theming / contrast (WCAG), layout glitches in `vtcode-ui` unless they enable spoofing
  of HITL prompts (e.g., fake approval dialog) — those ARE in scope.
- DoS via huge inputs unless it is unauthenticated remote and trivially triggerable.
- `target/`, `dist/`, generated `THIRD-PARTY-NOTICES` (do not report license nits).

## How to exercise it

```sh
cargo build --locked --workspace
cargo nextest run -p vtcode-commons
cargo nextest run -p vtcode-core -E 'binary(/pty_tests/)'
cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'
./scripts/check-dev.sh --test
```

Key binaries: `target/debug/vtcode`. Key harnesses: `scripts/tests/`, crate `tests/`,
`fuzz/` targets. Fuzz targets that accept untrusted bytes are good entry points.

Offline note: the scan container has NO network. Use vendored crates and local files only.
Do not `cargo install`, `npm install`, or `curl` during audit.

## How you rate severity

- **Critical**: RCE, sandbox escape, command injection bypassing allowlist/validators,
  path/symlink escape writing outside workspace, auth token / keychain exfiltration,
  MCP/ACP/WebMCP Remote attack achieving code exec without user approval.
- **High**: Persistent approval bypass (e.g., Approve Once persisting), env leakage
  enabling secret theft, patch-apply writing outside workspace, SSRF in webfetch/MCP
  reaching cloud metadata, credential logging.
- **Medium**: Limited DoS (CPU/disk exhaustion via crafted repo), prompt-injection
  causing unwanted but sandboxed actions that still require HITL, info disclosure
  of non-secret paths, missing `// SAFETY:` / `unsafe` misuse without demonstrated impact.
- **Low**: Best-practice hardenings, noisy clippy lints, theoretical issues without PoC,
  UI spoofing requiring local config write + user misclick.

Please do NOT report:

- `unwrap`/`expect`/`panic` in tests only (allowed per `clippy.toml`).
- `cargo audit` dependency nits without a reachable PoC in VTCode.
- Missing rate-limits on local-only TUI loops.

## Report / patch / PoC preferences

- One report per root cause. Deduplicate by sink + vulnerable function, not by each call site.
  If the same validator bug affects 5 commands, file ONE report listing all 5.
- Minimal Rust PoC preferred: `cargo test` snippet, small `#[test]`, or shell transcript
  showing bypass (e.g., `rg --pre ...` escaping, `../` or symlink escaping `/src`).
- Include: file:line (`crate/path/file.rs:123`), attacker-controlled source → sink,
  why allowlist/validator/sandbox failed, and a minimal patch (not a full refactor).
- If a patch touches `ThreadEvent`, harness config (`agent.harness` /
  `automation.full_auto` / `context.dynamic`), or compiled prompts
  (`vtcode-core/src/prompts/`), note the contract test that must be updated.
- Severity should map to the rubric above, not CVSS defaults.
