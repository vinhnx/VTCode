# Quick Reference

## Install

```bash
# macOS & Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex

# Homebrew
brew install vtcode

# Cargo
cargo install vtcode

```

## Quick Start

```bash
export OPENAI_API_KEY="sk-..."
vtcode
```

## Uninstall

```bash
# Shell installer
rm /usr/local/bin/vtcode

# Homebrew
brew uninstall vtcode

# Cargo
cargo uninstall vtcode

```

## Verify

```bash
vtcode --version
```

## Troubleshooting

```bash
# If install fails with "No such file or directory", use the GitHub API endpoint
curl -fsSL "https://api.github.com/repos/vinhnx/vtcode/contents/scripts/install.sh?ref=main" | jq -r '.content' | base64 -d | bash
```

## API Keys

```bash
export OPENAI_API_KEY="..."       # OpenAI
export ANTHROPIC_API_KEY="..."    # Anthropic
export GEMINI_API_KEY="..."       # Google Gemini
export MODEL_API_KEY="..."        # Meta AI (official, documented name)
# Or: export META_API_KEY="..."
export XAI_API_KEY="..."          # xAI
export DEEPSEEK_API_KEY="..."     # DeepSeek
export OPENROUTER_API_KEY="..."   # OpenRouter
export AI_GATEWAY_API_KEY="..."   # Vercel AI Gateway
export MERGE_GATEWAY_API_KEY="..." # Merge Gateway
```

## Authentication (OAuth / ChatGPT Subscription)

VT Code supports three ways to authenticate with OpenAI:

### 1. In-process ChatGPT OAuth (unofficial Codex compatibility flow)

No Codex CLI or app required — VT Code performs an in-process PKCE browser login:

```bash
vtcode login openai       # browser-based ChatGPT subscription login
vtcode logout openai      # clear the stored VT Code session
vtcode auth               # show auth status for all providers
```

Inside the TUI, use the equivalent slash commands:

```text
/login openai    /logout openai    /auth
```

> **⚠️ Unofficial compatibility mechanism:** By default, VT Code reuses the Codex CLI's public PKCE client ID as an
> unofficial, unguaranteed compatibility approach. OpenAI has not documented or guaranteed third-party reuse of this
> client identity, and a public client ID is not authorization to reuse another tool's OAuth registration. Organizations
> with their own OpenAI-issued client pair can override via `VTCODE_OPENAI_OAUTH_CLIENT_ID` /
> `VTCODE_OPENAI_OAUTH_ORIGINATOR` (both must be set together).

### 2. Codex CLI fallback (automatic)

If you already ran `codex login`, VT Code automatically detects and uses `~/.codex/auth.json` at runtime — no extra
setup needed. To validate those credentials explicitly without starting a new browser flow:

```bash
vtcode login openai --from-codex   # validate Codex credentials (does not persist)
```

> **Note:** `--from-codex` validates and reuses Codex's runtime credentials but does **not** persist a separate VT Code
> session. Codex-owned tokens are not rotated by VT Code — copying or redeeming them could race Codex's refresh cycle or
> invalidate Codex-maintained credentials. Instead, VT Code re-reads `auth.json` when tokens need refreshing. Run
> `codex login` to refresh expired Codex tokens.

### 3. API key

```bash
export OPENAI_API_KEY="sk-..."
```

### Credential precedence

When `auth.openai.preferred_method = "auto"` (the default):

1. VT Code ChatGPT session from the in-process PKCE flow (full auto-refresh)
2. Codex `auth.json` fallback (managed by Codex)
3. `OPENAI_API_KEY` environment variable

Logout only clears VT Code's own session. To remove Codex fallback credentials, run `codex logout` separately. See
[OAuth Authentication Guide](../guides/oauth-authentication.md) for details.

### Managing API-key secrets

```text
/secret         # interactive secret manager (list, add, delete)
/secret list    # show which provider keys are stored (no values printed)
/secret add openai OPENAI_API_KEY
/secret delete openai OPENAI_API_KEY
```

Secrets are stored in the system keyring or encrypted file storage (see `agent.credential_storage_mode` in config).
OAuth sessions and API-key secrets are separate — `/secret` manages API keys; `/login` manages OAuth.

## Resources

- Docs: <https://github.com/vinhnx/vtcode/docs>
- Issues: <https://github.com/vinhnx/vtcode/issues>
- [Full Installation Guide](./README.md)
- [Technical Details](./NATIVE_INSTALLERS.md)

## Conversation and file history

| In-chat command | Action                                                                             |
| --------------- | ---------------------------------------------------------------------------------- |
| `/rewind`       | Pick a saved prompt and confirm restoring files and conversation together.         |
| `/redo`         | Reverse the last rewind, including conversation.                                   |
| `/rewind`       | Rewind to a checkpoint; an interrupted rewind auto-recovers on the next `/rewind`. |

New prompts are checkpointed automatically. See [checkpoint details](../development/filesnap-checkpoints.md).
