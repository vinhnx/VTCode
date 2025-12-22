# Codex Cloud Environment Setup for VT Code

This guide explains how to provision a Codex Cloud environment that can build and test
VT Code reliably. It summarizes the task lifecycle, recommends setup and maintenance
scripts, and highlights the environment variables that VT Code expects.

## 1. Understand the Codex task lifecycle

Codex executes tasks in three phases:

1. **Setup** – the platform runs your setup script inside a fresh container. Install toolchains
   and dependencies here.
2. **Agent run** – the Codex agent works on your repository. Only commands and tooling that
   are present after the setup phase are available.
3. **Maintenance** – optional script that runs when a cached container is resumed. Use it to
   refresh dependencies that might have changed since the cache was created.

Scripts run in separate non-interactive shells. To persist environment variables across phases,
append them to `~/.bashrc` inside the script instead of relying on `export`.

## 2. Configure the environment in Codex settings

1. Open **Codex  Settings  Environments** and create (or edit) an environment for VT Code.
2. Select the `universal` base image. It already includes Rust and common build tooling, which
   aligns with VT Code’s Rust workspace defined in `Cargo.toml`.
3. Set the default branch or commit that Codex should check out when starting tasks.
4. Decide whether the agent should have network access during the task phase. The default is
   disabled; enable limited or full access only if the task requires it.

## 3. Provide environment variables and secrets

Add the following environment variables in the environment settings. Use Secrets for sensitive
values so they are only available during setup.

| Name                                  | Type                 | Purpose                                                                |
| ------------------------------------- | -------------------- | ---------------------------------------------------------------------- |
| `GEMINI_API_KEY`, `GOOGLE_API_KEY`    | Secret               | Primary Gemini provider credentials used by the default configuration. |
| `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` | Secret               | Optional provider keys when switching models.                          |
| `VTCODE_CONTEXT_TOKEN_LIMIT`          | Environment variable | Limits the context window when tasks need smaller token budgets.       |

Secrets are stripped from the environment before the agent phase, so persist anything the agent
must read as a regular environment variable.

## 4. Recommended setup script

Create a script named `codex-setup.sh` and paste it into the **Setup script** field.

```bash
#!/usr/bin/env bash
set -euxo pipefail

apt-get update
apt-get install -y build-essential pkg-config libssl-dev

rustup update stable
rustup default stable
rustup component add clippy rustfmt

cargo install cargo-audit --locked || true

cargo fetch --locked
```

This script makes sure:

-   System build dependencies are present.
-   The stable Rust toolchain and required components (`rustfmt`, `clippy`) are installed.
-   `cargo-audit` is available for security checks.
-   Cargo downloads dependencies up front so cached containers can reuse them.

If you need additional tooling (for example `node`, `python`, or project-specific binaries), add
those commands to the setup script.

## 5. Recommended maintenance script

Paste the following into the **Maintenance script** field to keep cached containers aligned with
recent changes:

```bash
#!/usr/bin/env bash
set -euxo pipefail

rustup update stable
cargo fetch --locked
```

Use the **Reset cache** button in the environment if dependency changes require a clean rebuild.

## 6. Repository configuration reminders

-   Copy `vtcode.toml.example` to `vtcode.toml` in your repository and adjust provider settings
    (model IDs, API key environment variables, tool policies) before launching a Codex task.
-   Keep project-level setup scripts such as `scripts/setup.sh` aligned with the Codex setup script
    so local and cloud environments behave consistently.

## 7. Validate the environment

After a container finishes its setup, run a smoke test task in Codex (or locally via the Codex
CLI) that executes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
```

All three commands should succeed without additional manual steps. If they fail, update the setup
script to install missing tooling or dependencies.

## 8. Troubleshooting tips

-   If the agent reports missing environment variables, double-check whether they were configured as
    Secrets (available only during setup) or environment variables (available during the agent run).
-   For build failures caused by stale caches, reset the environment cache or bump a no-op command in
    the setup script to force cache invalidation.
-   When adding new system packages, prefer `apt-get` in the setup script and keep the list minimal to
    reduce setup time.
-   If you need to debug interactively, reproduce the setup locally with the
    [`codex-universal`](https://github.com/openai/codex-universal) image.
