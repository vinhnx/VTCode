<!-- markdownlint-disable-next-line MD041 -->
<div align="center">

<img src="./resources/logo/vt_code_adaptive.svg" alt="VT Code" width="600" />

<p>
  <sub>Terminal-first. Desktop-ready.</sub><br />
  <sub>Explore, plan, edit, and verify with your model of choice.</sub>
</p>

[![Rust](https://img.shields.io/badge/rust-1.99.1%2B-000000?style=flat-square&logo=rust&logoColor=white)](https://github.com/vinhnx/VTCode)
[![License](https://img.shields.io/badge/License-MIT_OR_Apache--2.0-30363D?style=flat-square)](#license)
[![GitHub release](https://img.shields.io/github/release/vinhnx/VTCode.svg)](https://github.com/vinhnx/VTCode/releases)

<!-- NB: not https://deepwiki.com/badge.svg — that route is behind a Vercel bot
  challenge, so GitHub's camo proxy gets 429 and renders the alt text instead.
  This shields.io URL reproduces the same 109x20 badge, logo, and colors. -->
<!-- markdownlint-disable-next-line MD013 -->

[![Ask DeepWiki](https://img.shields.io/badge/-Ask_DeepWiki-007ec6?logo=data%3Aimage%2Fpng%3Bbase64%2CiVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAACXBIWXMAAAPoAAAD6AG1e1JrAAACIUlEQVRYw%2B2XP2gUQRTGv7d3JhYWQawkIBaCVWzETos0gmAnVoKNTSoLK0E7EQQvC20sBVFBtLPQRgTBtBaKjYgIQUSxiJq7fT%2BLvCGPJZdNLt4dQj5Y3u7sznzfzLw%2Fs9IO%2FicANiniCujEfWdsQgBLxAbsbraPZcmBfcAt4DNwKQsZNfEUsAB8YRV12LfA6ZHuedg7AO7ed%2Fc%2BUBcbQhbiu%2B6wXIM6EnZWkszMJe2Ke0n6I2la0v51hFeSMLN6OwIK6jJwEBdUYb2MAxRSz6sY4geiahFgLe1Hwq6YWe3us8AN4FQQU4QM64SPY69Xyr43fADgNjAPXHb3peSsD4FDQ0VLEjAHvIhBPS6AHvA1bBO9JPAXcHFbIRtJ5yzwIQZ9CZwAusDJENIkLqsG8DpNyIZJwSUk9wLHgakcesC1JCCjiHm1kYDNOEjptCxpSVK%2F8f73OFLxPLAYM3oCzEX7MXf%2FlJbc%2F8kWpA4HgQdpYI9IWAbeh83whi84cHXL%2B58EPBoQhnmm94EzwE3gZ2p%2FDhzdbhg%2BTaRr01x7ftb4%2FjBwFzjXrCvDpmLW9crVLNeR9CaapoGemb2TdCERW1tNaBNQ8jktwmozqwtpqRNtdWAjARYk39IS12ZWRX7vmJmA71nQZgi36gN7gCvAj5xc3P0jcH7kx7Ik5IC73wsh14GZsZySow5002l4Jr0b6%2Bm4eSyvpAn8lEzsx2QHo8RfUrlN%2BuPq4ksAAAAASUVORK5CYII%3D&labelColor=000000)](https://deepwiki.com/vinhnx/vtcode)
[![macOS 15+](https://img.shields.io/badge/macOS-15%2B-000000?style=flat-square&logo=apple)](#desktop-app)
[![arm64 and x64](https://img.shields.io/badge/arch-arm64_%7C_x64-30363D?style=flat-square)](#desktop-app)

<p>
  <img src="./resources/gif/demo-desktop.gif" alt="VT Code Desktop" />
  <sub>Desktop app</sub>
</p>

<p>
  <img src="./resources/screenshots/vtcode-cli-tui.gif" width="100%"
    alt="Terminal TUI demo" /><br />
  <sub>Terminal TUI</sub>
</p>

</div>

## Overview

VT Code is a terminal-first coding agent for your repository, in the CLI and its native macOS app.
It owns context, tool calls, and execution policy so long tasks stay auditable and resumable.

| At a glance      | What you get                                                                                                        |
| ---------------- | ------------------------------------------------------------------------------------------------------------------- |
| **Planning**     | [Plan read-only](./docs/guides/planning-workflow.md), then review [turn diffs](./docs/development/diff-preview.md). |
| **Safety**       | [Auditable command policy](./docs/security/SECURITY_MODEL.md) with sandboxing.                                      |
| **Self-correct** | [HOVER loop](./docs/development/hypothesis-revision-validation.md) on mismatch.                                     |
| **Long runs**    | [Headless exec](./docs/user-guide/exec-mode.md) with resume and compaction.                                         |
| **Integrations** | [MCP](./docs/guides/mcp-integration.md), [skills](./docs/skills/SKILLS_GUIDE.md), plugins, and editor bridges.      |
| **Models**       | [Hosted or local providers](./docs/providers/PROVIDER_GUIDES.md) per task.                                          |

## Quick start

Runs on macOS, Linux, or Windows/WSL. Building from source needs Rust 1.99.1+ (edition 2024); the
[installation guide](./docs/installation/README.md) covers prerequisites and optional search tools.

### 1. Install

```bash
# Review the installer script before running it: https://github.com/vinhnx/VTCode/blob/main/scripts/install.sh
curl -fsSL https://raw.githubusercontent.com/vinhnx/VTCode/main/scripts/install.sh | bash
```

Or install the CLI with Homebrew or Cargo (the desktop cask lives in [Desktop app](#desktop-app)):

```bash
brew trust vinhnx/tap && brew install vinhnx/tap/vtcode

# Or install from crates.io
cargo install vtcode
```

On Windows (best-effort artifacts; may lag behind macOS/Linux):

```powershell
irm https://raw.githubusercontent.com/vinhnx/VTCode/main/scripts/install.ps1 | iex
```

Verify with `vtcode --version`. The [installation guide](./docs/installation/README.md) covers other install
methods, desktop flags, and optional search tools.

### 2. Configure your project

In your project directory, scaffold configuration and add credentials:

```bash
cd path/to/your/project
vtcode init                # scaffolds config + AGENTS.md; review before committing
vtcode secret add openai   # stores your OpenAI API key in secure storage
```

Replace `openai` with your provider. Keys use secure storage (OS keyring or encrypted file); alternatives are
environment variables, a workspace `.env` file, or `vtcode login` where supported. See
[getting started](./docs/user-guide/getting-started.md) and the
[provider guides](./docs/providers/PROVIDER_GUIDES.md) for provider-specific setup.

> **Note:** ChatGPT OAuth reuses the Codex CLI's public client identity via an unofficial compatibility flow;
> prefer your own OpenAI API key. GitHub Copilot uses the official `copilot` CLI. See
> [OAuth authentication](./docs/guides/oauth-authentication.md).
>
> **Caution:** Keep API keys out of Git and `vtcode.toml`; never commit a workspace `.env` file.

### 3. Run your first task

```bash
vtcode   # open the interactive TUI in your project
```

Ask a focused question, then review proposed changes and test results before committing. See [Usage](#usage)
for automation and session commands.

## Usage

Common commands (see the [command reference](./docs/user-guide/commands.md)):

| Goal                    | Command                                            |
| ----------------------- | -------------------------------------------------- |
| Start the TUI           | `vtcode`                                           |
| Ask without tools       | `vtcode ask "explain Rc vs Arc"`                   |
| Run a task headlessly   | `vtcode exec "refactor main.rs"`                   |
| Review pending changes  | `vtcode review`                                    |
| Schedule recurring work | `vtcode schedule create --prompt "..." --every 1h` |
| Resume the last session | `vtcode continue`                                  |
| Inspect a run log       | `vtcode trajectory`                                |

For larger tasks, start read-only, review the turn diff before committing, and use `/explain` for a summary
without another model call. See the [interactive guide](./docs/user-guide/interactive-mode.md).

### Modes

- **`ask`** is tool-free: it prints one reply and exits.
- **`exec`** runs the full tool loop under explicit workspace trust (`full_auto` plus `[automation.full_auto]`).
  Terminals prompt for trust; non-TTY runs need `VTCODE_TRUST_WORKSPACE=full-auto`. Allow-lists, denies, and
  execution policy still apply.
- **`review`** inspects uncommitted changes.

Schedule recurring work on the same exec runtime. Resume or fork with `vtcode continue` (or `--session-id <id>`),
resume headless runs with `vtcode exec resume --last`, and inspect logs with `vtcode trajectory`. See
[exec mode](./docs/user-guide/exec-mode.md) and [full automation](./docs/guides/full-automation.md).

## Desktop app

The native macOS app shares the CLI runloop, providers, tool policy, sandboxing, sessions, configuration, and
credentials. Configure a provider first (see [Quick start](#quick-start)).

### Install (macOS 15+)

On Apple Silicon, install it with Homebrew (update with `brew upgrade --cask vinhnx/tap/vtcode-desktop`):

```bash
brew install --cask vinhnx/tap/vtcode-desktop
```

Without Homebrew, download it from the [latest release](https://github.com/vinhnx/VTCode/releases/latest) or run
`vtcode desktop --install`. Launch it with `vtcode desktop` or from Finder. Intel Macs build from source.

> [!WARNING]
> **First launch needs a one-time macOS security approval.** Builds are ad hoc signed, not notarized, so macOS
> blocks the first launch (Homebrew cask installs included). Open the app once, click **Open Anyway** in
> **System Settings › Privacy & Security**, then open it again. The button shows for about an hour after a block.

See the [desktop workspace guide](./docs/development/desktop-workspace.md) for requirements, source builds, and
troubleshooting.

## Documentation

Start from the [documentation index](./docs/INDEX.md) or [Wiki](https://github.com/vinhnx/VTCode/wiki);
integrations below are optional.

### Get started

- [Installation](./docs/installation/README.md) and [getting started](./docs/user-guide/getting-started.md)
- [Desktop app](./docs/development/desktop-workspace.md) for the native macOS GUI
- [Providers](./docs/providers/PROVIDER_GUIDES.md) and [OAuth login](./docs/guides/oauth-authentication.md)
- [FAQ](./docs/FAQ.md) and [compatibility](./docs/COMPATIBILITY.md)

### Work in the TUI

- [Interactive mode](./docs/user-guide/interactive-mode.md) and [command reference](./docs/user-guide/commands.md)
- [Read-only planning](./docs/guides/planning-workflow.md) and [turn diffs](./docs/development/diff-preview.md)
- [Configuration](./docs/config/CONFIG_FIELD_REFERENCE.md) and [security model](./docs/security/SECURITY_MODEL.md)

### Automate

- [Full automation](./docs/guides/full-automation.md) and [decisions probe](./docs/guides/decisions-probe.md)
- [Scheduled tasks](./docs/user-guide/scheduled-tasks.md) and [hooks](./docs/guides/hooks-guide.md)

### Extend and integrate

- [Skills](./docs/skills/SKILLS_GUIDE.md), [plugins](./docs/guides/agent-plugins.md), [MCP](./docs/guides/mcp-integration.md)
- [MCP interop](./docs/guides/mcp-interop.md) for client and server maintainers
- [Editors (ACP)](./docs/guides/zed-acp.md), [WebMCP](./docs/user-guide/webmcp.md) ([deployment](./docs/reference/webmcp.md)),
  [Companion](./docs/user-guide/vtcode-companion.md), and [Memcode](./docs/guides/memcode-mcp.md)

### Develop and evaluate

- [Development](./docs/development/README.md), [testing](./docs/development/testing.md), and [evals](./docs/guides/eval.md)
- [Architecture](./docs/ARCHITECTURE.md), [HOVER](./docs/development/hypothesis-revision-validation.md), and [protocols](./docs/protocols/OPEN_RESPONSES.md)
- [Loop engineering](./docs/project/PLAN-loop-engineering.md)

## Development

```mermaid
graph LR
    BIN[vtcode binary] --> CORE[vtcode-core harness]
    CORE --> LLM[vtcode-llm]
    CORE --> SAFETY[vtcode-safety]
    CORE --> EVENTS[vtcode-exec-events]
    CORE --> MEMORY[vtcode-memory]
    CORE --> UI[vtcode-ui]
```

Build with Rust 1.99.1+ (edition 2024) and install `cargo-nextest` to run tests. The [architecture
guide](./docs/ARCHITECTURE.md) maps all 25 crates, including the context and compaction design.

```bash
git clone https://github.com/vinhnx/VTCode.git
cd VTCode
./scripts/run-debug.sh     # build and launch a debug binary
./scripts/check-dev.sh     # fast gate: clippy, fmt, check
cargo nextest run          # tests (requires cargo-nextest)
```

CI builds with `--locked` and `RUSTFLAGS="-D warnings"`; match locally with
`RUSTFLAGS="-D warnings" cargo check --locked`. See the [development overview](./docs/development/README.md) and
[testing guide](./docs/development/testing.md).

Release binaries and notes live on [GitHub releases](https://github.com/vinhnx/VTCode/releases).

## Contributing

Contributions are welcome: code, docs, [evals](./docs/guides/eval.md), and bug reports (include `vtcode trajectory`
output). Read the [contribution guide](./docs/CONTRIBUTING.md) before opening a PR; MCP maintainers should also see
the [interop guide](./docs/guides/mcp-interop.md).

## Security

Report vulnerabilities privately with [GitHub private vulnerability
reporting](https://github.com/vinhnx/VTCode/security/advisories/new). See the [security
policy](./docs/SECURITY.md).

## Community

- [Report a bug](https://github.com/vinhnx/VTCode/issues/new?template=bug_report.md) ·
  [Request a feature](https://github.com/vinhnx/VTCode/issues/new?template=feature_request.md) ·
  [Discussions](https://github.com/vinhnx/VTCode/discussions)
- [Good first issues](https://github.com/vinhnx/VTCode/labels/good%20first%20issue) · [Help wanted](https://github.com/vinhnx/VTCode/labels/help%20wanted)

The [Code of Conduct](./docs/CODE_OF_CONDUCT.md) keeps review constructive. Contact:
`vinhnguyen2308 [at] gmail [dot] com`.

## Thanks to our contributors

Thank you to everyone below. You are the reason this project keeps moving.

<details>
<!-- CONTRIBUTORS-COUNT:START --><summary>View all contributors (30)</summary><!-- CONTRIBUTORS-COUNT:END -->

<!-- CONTRIBUTORS:START -->

<!-- markdownlint-disable MD013 -->

### Security Advisors

<a href="https://github.com/glmgbj233"><img src="https://avatars.githubusercontent.com/u/115564047?v=4&s=60" width="40" height="40" alt="@glmgbj233" title="@glmgbj233 GHSA-wqgw-crr5-cr2p (security advisory)" style="border-radius: 50%; border: 2px solid #FF6B6B;" /></a>&nbsp;
<a href="https://github.com/nnfrog"><img src="https://avatars.githubusercontent.com/u/142202920?v=4&s=60" width="40" height="40" alt="@nnfrog" title="@nnfrog GHSA-r249-hpfx-x2w7 (security advisory)" style="border-radius: 50%; border: 2px solid #FF6B6B;" /></a>&nbsp;

### Main Contributor

<a href="https://github.com/kernitus"><img src="https://avatars.githubusercontent.com/u/2789734?v=4&s=60" width="40" height="40" alt="@kernitus" title="@kernitus Main Contributor (52 commits)" style="border-radius: 50%; border: 2px solid #FFD700;" /></a>&nbsp;

### Core Contributors

<a href="https://github.com/7jrxt42BxFZo4iAnN4CX"><img src="https://avatars.githubusercontent.com/u/72938937?v=4&s=60" width="40" height="40" alt="@7jrxt42BxFZo4iAnN4CX" title="@7jrxt42BxFZo4iAnN4CX Core contributor (44 commits) - subagents, hooks, config & TUI fixes (#737, #738, #740-#742+)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/ct-jaryn"><img src="https://avatars.githubusercontent.com/u/151006958?v=4&s=60" width="40" height="40" alt="@ct-jaryn" title="@ct-jaryn Core contributor (7 commits) - lint/model preset test gates (#769), swarm diff fix (#768), CLI test harness (#767), MCP docs (#765)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/oiwn"><img src="https://avatars.githubusercontent.com/u/398035?v=4&s=60" width="40" height="40" alt="@oiwn" title="@oiwn Core contributor (6 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/Sachin-Bhat"><img src="https://avatars.githubusercontent.com/u/25080916?v=4&s=60" width="40" height="40" alt="@Sachin-Bhat" title="@Sachin-Bhat Core contributor (3 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/chenrui333"><img src="https://avatars.githubusercontent.com/u/1580956?v=4&s=60" width="40" height="40" alt="@chenrui333" title="@chenrui333 Core contributor (3 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/xcrong"><img src="https://avatars.githubusercontent.com/u/46434477?v=4&s=60" width="40" height="40" alt="@xcrong" title="@xcrong Core contributor (2 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/netbrah"><img src="https://avatars.githubusercontent.com/u/162479981?v=4&s=60" width="40" height="40" alt="@netbrah" title="@netbrah Core contributor (2 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/leonj1"><img src="https://avatars.githubusercontent.com/u/5171829?v=4&s=60" width="40" height="40" alt="@leonj1" title="@leonj1 Core contributor (2 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;
<a href="https://github.com/gzsombor"><img src="https://avatars.githubusercontent.com/u/66230?v=4&s=60" width="40" height="40" alt="@gzsombor" title="@gzsombor Core contributor (2 commits)" style="border-radius: 50%; border: 2px solid #50C878;" /></a>&nbsp;

### Contributors

<a href="https://github.com/vivekgupta-memcode"><img src="https://avatars.githubusercontent.com/u/330296621?v=4&s=60" width="40" height="40" alt="@vivekgupta-memcode" title="@vivekgupta-memcode Contributor (4 commits) - Memcode OAuth setup docs (#763)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/S2thend"><img src="https://avatars.githubusercontent.com/u/81468081?v=4&s=60" width="40" height="40" alt="@S2thend" title="@S2thend Contributor (2 commits) - checkpoint rewind/redo (#771)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/uiYzzi"><img src="https://avatars.githubusercontent.com/u/40852301?v=4&s=60" width="40" height="40" alt="@uiYzzi" title="@uiYzzi Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/TuanLe-bk18"><img src="https://avatars.githubusercontent.com/u/222461688?v=4&s=60" width="40" height="40" alt="@TuanLe-bk18" title="@TuanLe-bk18 Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/shouryamaanjain"><img src="https://avatars.githubusercontent.com/u/72159602?v=4&s=60" width="40" height="40" alt="@shouryamaanjain" title="@shouryamaanjain Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/Sanjays2402"><img src="https://avatars.githubusercontent.com/u/51058514?v=4&s=60" width="40" height="40" alt="@Sanjays2402" title="@Sanjays2402 Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/rs545837"><img src="https://avatars.githubusercontent.com/u/114828377?v=4&s=60" width="40" height="40" alt="@rs545837" title="@rs545837 Contributor (1 commit) - optional Sem code discovery docs (#783)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/RobertBorg"><img src="https://avatars.githubusercontent.com/u/1288566?v=4&s=60" width="40" height="40" alt="@RobertBorg" title="@RobertBorg Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/raphamorim"><img src="https://avatars.githubusercontent.com/u/3630346?v=4&s=60" width="40" height="40" alt="@raphamorim" title="@raphamorim Contributor (1 commit) - PR #708, rio-vt migration" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/poelzi"><img src="https://avatars.githubusercontent.com/u/66107?v=4&s=60" width="40" height="40" alt="@poelzi" title="@poelzi Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/pawurb"><img src="https://avatars.githubusercontent.com/u/1131944?v=4&s=60" width="40" height="40" alt="@pawurb" title="@pawurb Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/morler"><img src="https://avatars.githubusercontent.com/u/478444?v=4&s=60" width="40" height="40" alt="@morler" title="@morler Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/georgeatparallel"><img src="https://avatars.githubusercontent.com/u/297992784?v=4&s=60" width="40" height="40" alt="@georgeatparallel" title="@georgeatparallel Contributor (1 commit) - opt-in Parallel Search MCP config (#779)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/ForrestThump"><img src="https://avatars.githubusercontent.com/u/44280834?v=4&s=60" width="40" height="40" alt="@ForrestThump" title="@ForrestThump Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/f-o-x11"><img src="https://avatars.githubusercontent.com/u/239233855?v=4&s=60" width="40" height="40" alt="@f-o-x11" title="@f-o-x11 Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/EvoLinkAI"><img src="https://avatars.githubusercontent.com/u/253253881?v=4&s=60" width="40" height="40" alt="@EvoLinkAI" title="@EvoLinkAI Contributor (1 commit) - Evolink provider (#664)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/ericcurtin"><img src="https://avatars.githubusercontent.com/u/1694275?v=4&s=60" width="40" height="40" alt="@ericcurtin" title="@ericcurtin Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;
<a href="https://github.com/diegosouzapw"><img src="https://avatars.githubusercontent.com/u/8016841?v=4&s=60" width="40" height="40" alt="@diegosouzapw" title="@diegosouzapw Contributor (1 commit)" style="border-radius: 50%; border: 2px solid #B19CD9;" /></a>&nbsp;

<!-- markdownlint-enable MD013 -->

<!-- CONTRIBUTORS:END -->

</details>

Want to see your avatar here? Every bit counts: a one-line fix, a bug report, or a thoughtful comment. Not sure where
to begin? Say hello on a [good first issue](https://github.com/vinhnx/VTCode/labels/good%20first%20issue) and we will
figure it out together.

## Resources

- [Building VT Code, a year in](https://huggingface.co/blog/vinhnx90/building-vtcode-a-year-in) ·
  [Podcast](https://www.youtube.com/watch?v=XLoswcd5rH0) · [Video](https://www.youtube.com/watch?v=PvL_kPjgU6o)

## Support VT Code

<div align="center">

**Thanks to current sponsors: [Codemod](https://codemod.com/) and [CodeRabbit](https://www.coderabbit.ai/).**

[![@codemod](https://avatars.githubusercontent.com/u/78830094?s=80)](https://github.com/codemod)&nbsp;
[![@coderabbitai](https://avatars.githubusercontent.com/u/132028505?s=80)](https://github.com/coderabbitai)&nbsp;

[![GitHub Sponsors](https://img.shields.io/github/sponsors/vinhnz?style=for-the-badge&label=Click%20to%20sponsor&labelColor=BFB38F&color=000000&link=https%3A%2F%2Fgithub.com%2Fsponsors%2Fvinhnx)](https://github.com/sponsors/vinhnx)

## Thank you

<a
  href="https://www.star-history.com/?repos=vinhnx%2FVTCode&type=timeline&legend=top-left">
<picture>

   <source media="(prefers-color-scheme: dark)"
     srcset="https://api.star-history.com/chart?repos=vinhnx/VTCode&type=timeline&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)"
     srcset="https://api.star-history.com/chart?repos=vinhnx/VTCode&type=timeline&legend=top-left" />
   <img alt="Star History Chart"
     src="https://api.star-history.com/chart?repos=vinhnx/VTCode&type=timeline&legend=top-left" />
 </picture>
</a>

</div>

## License

First-party code is **MIT OR Apache-2.0**, available under either [LICENSE-MIT](LICENSE-MIT) or
[LICENSE](LICENSE) (Apache-2.0). Third-party code keeps its original licenses, listed in
[THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES).
