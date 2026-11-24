<!-- markdownlint-disable-next-line MD041 -->
<div align="center">

<img src="./resources/logo/vt_code_adaptive.svg" alt="VT Code" width="600" />

<p>
  <sub>VT Code is an open-source coding agent for everyone.</sub>
</p>

[![Rust](https://img.shields.io/badge/rust-1.100.0%2B-D99A4E?style=flat-square&logo=rust&logoColor=white)](https://github.com/vinhnx/vtcode)
[![License](https://img.shields.io/badge/License-MIT_OR_Apache--2.0-BFB38F?style=flat-square)](#license)
[![GitHub release](https://img.shields.io/github/release/vinhnx/vtcode.svg)](https://github.com/vinhnx/vtcode/releases)
[![Ask DeepWiki](https://img.shields.io/badge/Ask_DeepWiki-007ec6?style=flat-square)](https://deepwiki.com/vinhnx/vtcode)
[![macOS 15+](https://img.shields.io/badge/macOS-15%2B-383B73?style=flat-square&logo=apple)](#desktop-app)
[![arm64 and x64](https://img.shields.io/badge/arch-arm64_%7C_x64-30363D?style=flat-square)](#desktop-app)

<p>
  <img src="./resources/gif/demo-desktop.gif" width="85%"
    alt="VT Code Desktop app running an agent coding task" loading="lazy" />
  <br />
  <sub>Desktop app demo — see also the <a href="./resources/screenshots/vtcode-cli-tui.gif">terminal TUI demo</a></sub>
</p>

</div>

## Overview

VT Code is an open-source coding agent: one shared runloop powers the terminal TUI and the
native macOS app. It is open source under **MIT OR Apache-2.0** (see [License](#license)).
Start with [Quick start](#quick-start); the full map is in [Documentation](#documentation).

<!-- markdownlint-disable MD060 -- compact table; keep pipes unpadded -->

| At a glance   | What you get                                                                                                                                                               |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Plan**      | Read-only [planning](./docs/guides/planning-workflow.md); review [turn diffs](./docs/development/diff-preview.md).                                                         |
| **Safety**    | Sandboxed, [auditable command policy](./docs/security/security_model.md).                                                                                                  |
| **Long runs** | Resumable [headless exec](./docs/user-guide/exec-mode.md) with [snapshots](./docs/user-guide/commands.md) and [subagents](./docs/user-guide/subagents.md).                 |
| **Extend**    | [MCP](./docs/guides/mcp-integration.md) ([interop](./docs/guides/mcp-interop.md)), [skills](./docs/skills/skills_guide.md), and [plugins](./docs/guides/agent-plugins.md). |
| **Models**    | Per-task [hosted or local providers](./docs/providers/provider_guides.md).                                                                                                 |

<!-- markdownlint-enable MD060 -->

## Quick start

Install on macOS, Linux, or Windows/WSL, configure once, and run. Source builds need Rust 1.100.0+
(edition 2024); the [installation guide](./docs/installation/README.md) covers every method.

### 1. Install

```bash
# Review the installer script before running it: https://github.com/vinhnx/vtcode/blob/main/scripts/install.sh
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
vtcode --version
```

Alternatives in the [installation guide](./docs/installation/README.md): `brew install vinhnx/tap/vtcode`,
`cargo install vtcode`, [Desktop app](#desktop-app), or the Windows `install.ps1` script.

### 2. Configure your project

From your project directory:

```bash
cd path/to/your/project
# Scaffolds vtcode.toml, .vtcode/, and AGENTS.md; review before committing.
vtcode init
```

Choose an authentication method for your provider:

- **API key:** `vtcode secret add openai` (replace `openai` with your provider). Keys stay in secure storage
  (OS keyring or encrypted file); environment variables or a workspace `.env` also work where supported.
- **ChatGPT subscription:** `vtcode login openai` — no API key needed.
- **Reuse an existing Codex login:** `vtcode login openai --from-codex` (requires an authenticated Codex CLI).

See [getting started](./docs/user-guide/getting-started.md) and
[provider guides](./docs/providers/provider_guides.md) for details. Copilot uses the official `copilot` CLI — see
[OAuth authentication](./docs/guides/oauth-authentication.md).

> **Caution:** Keep API keys out of Git and `vtcode.toml`, and never commit a workspace `.env` file.
> Run `vtcode secret add` or `vtcode login` before your first task.

### 3. Run your first task

```bash
vtcode # opens the interactive TUI
```

Ask one focused question, review the proposed changes and test results, then commit.
If `vtcode` is not found, reopen your shell so `PATH` picks up the install, then rerun `vtcode --version`.
See [Usage](#usage) for automation commands.

## Usage

Common commands (full list in [command reference](./docs/user-guide/commands.md)):

| Goal                    | Command                                            |
| ----------------------- | -------------------------------------------------- |
| Start the TUI           | `vtcode`                                           |
| Ask without tools       | `vtcode ask "explain Rc vs Arc"`                   |
| Run a task headlessly   | `vtcode exec "refactor main.rs"`                   |
| Review pending changes  | `vtcode review`                                    |
| Schedule recurring work | `vtcode schedule create --prompt "..." --every 1h` |
| Resume the last session | `vtcode continue`                                  |
| Resume a headless run   | `vtcode exec resume --last "prompt"`               |
| Roll back a snapshot    | `vtcode snapshots` / `vtcode revert`               |
| Inspect a run log       | `vtcode trajectory`                                |

Trust levels: `ask` never runs tools, `review` inspects uncommitted changes without the agent loop, and only
`exec` runs the full tool loop. `exec` needs workspace trust: approve the TTY prompt, or set
`VTCODE_TRUST_WORKSPACE=full-auto` for non-TTY runs (`deny` refuses instead). For larger tasks, start read-only,
review the turn diff, and use `/explain` without another model call. See [interactive mode](./docs/user-guide/interactive-mode.md),
[exec mode](./docs/user-guide/exec-mode.md), and [full automation](./docs/guides/full-automation.md).

## Desktop app

Native macOS app (macOS 15+) sharing the CLI runloop, providers, tool policy, sandboxing, sessions,
config, and credentials. Configure a provider first ([Quick start](#quick-start)).

### Install (macOS 15+)

```bash
brew install --cask vinhnx/tap/vtcode-desktop
```

Update cask installs with `brew upgrade --cask vinhnx/tap/vtcode-desktop`. Without Homebrew, download the
[latest release](https://github.com/vinhnx/vtcode/releases/latest) or run `vtcode desktop --install`,
then launch with `vtcode desktop` or from Finder. The cask ships Apple Silicon builds; Intel Macs install
from source with `vtcode desktop --install`. Full options are in the [installation guide](./docs/installation/README.md).

> [!WARNING]
> **First launch needs one-time approval** for ad hoc-signed builds (Homebrew cask included): open the app
> once from Applications, click **Open Anyway** in **System Settings › Privacy & Security**, then reopen
> it. The button expires about an hour after a blocked launch; reopen the app to bring it back.
> Notarized releases skip this. See [desktop workspace](./docs/development/desktop-workspace.md).

## Documentation

Start from the canonical [documentation index](./docs/index.md) or [Wiki](https://github.com/vinhnx/vtcode/wiki).
This section keeps one link per task.

### Start

- [Installation](./docs/installation/README.md) and [getting started](./docs/user-guide/getting-started.md)
- [Providers](./docs/providers/provider_guides.md) and [OAuth login](./docs/guides/oauth-authentication.md)
- [FAQ](./docs/faq.md) and [compatibility](./docs/compatibility.md)

### Use

- [Interactive mode](./docs/user-guide/interactive-mode.md) and [command reference](./docs/user-guide/commands.md)
- [Configuration](./docs/config/config_field_reference.md) and [security model](./docs/security/security_model.md)
- [Computer Use](./docs/user-guide/computer-use.md): isolated offline browser preview with per-turn approval

### Automate

- [Full automation](./docs/guides/full-automation.md) and [scheduled tasks](./docs/user-guide/scheduled-tasks.md)
- [Hooks](./docs/guides/hooks-guide.md) and [decisions probe](./docs/guides/decisions-probe.md)

### Extend

- [Skills](./docs/skills/skills_guide.md), [plugins](./docs/guides/agent-plugins.md), and
  [MCP](./docs/guides/mcp-integration.md)
- [Editors (ACP)](./docs/guides/zed-acp.md), [WebMCP](./docs/user-guide/webmcp.md),
  [A2A](./docs/a2a/README.md), and [Companion](./docs/user-guide/vtcode-companion.md)

### Develop

- [Development](./docs/development/README.md), [testing](./docs/development/testing.md), and
  [evals](./docs/guides/eval.md)
- [Architecture](./docs/architecture.md), [desktop workspace](./docs/development/desktop-workspace.md),
  [loop engineering](./docs/loop-engineering.md), and [protocols](./docs/protocols/open_responses.md)

## Development

27-crate workspace at version 0.185.0 (full list in the [architecture guide](./docs/architecture.md)).
The default build covers the root binary plus `vtcode-core`, `vtcode-skills`, `vtcode-tui-runtime`,
and `vtcode-ui`. The sketch below shows the core harness path, not every crate:

```mermaid
graph LR
    BIN["vtcode binary"] --> CORE["vtcode-core harness"]
    CORE --> LLM["vtcode-llm"]
    CORE --> SAFETY["vtcode-safety"]
    CORE --> EVENTS["vtcode-exec-events (crates/common)"]
    CORE --> MEMORY["vtcode-memory"]
    CORE --> SKILLS["vtcode-skills"]
    CORE --> TUI["vtcode-tui-runtime"]
    CORE --> UI["vtcode-ui"]
```

Build with Rust 1.100.0+ (edition 2024; workspace version 0.185.0):

```bash
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/run-debug.sh     # build and launch a debug binary
./scripts/check-dev.sh     # fast gate: clippy, fmt, check (add --test for tests)
cargo nextest run          # tests via nextest (see testing guide)
```

CI uses `--locked` and `RUSTFLAGS="-D warnings"`; match locally with
`RUSTFLAGS="-D warnings" cargo check --locked`. See [development](./docs/development/README.md) and [testing](./docs/development/testing.md).

Release binaries and notes live on [GitHub releases](https://github.com/vinhnx/vtcode/releases).

## Contributing

VT Code is open source and community-driven. Code, docs, [evals](./docs/guides/eval.md), and fixes are
welcome — include `vtcode trajectory` output in bug reports where possible. Before a PR, read
[contributing](./docs/CONTRIBUTING.md) and pick a
[good first issue](https://github.com/vinhnx/vtcode/labels/good%20first%20issue) or
[help wanted](https://github.com/vinhnx/vtcode/labels/help%20wanted) item. Forks: keep license and
third-party notices intact (see [License](#license)).

## Security

Report vulnerabilities privately via
[GitHub private vulnerability reporting](https://github.com/vinhnx/vtcode/security/advisories/new).
See the [security policy](./docs/SECURITY.md).

## Community

- [Report a bug](https://github.com/vinhnx/vtcode/issues/new?template=bug_report.md) ·
  [Request a feature](https://github.com/vinhnx/vtcode/issues/new?template=feature_request.md) ·
  [Discussions](https://github.com/vinhnx/vtcode/discussions)
- Elsewhere: [building VT Code, a year in](https://huggingface.co/blog/vinhnx90/building-vtcode-a-year-in) ·
  [podcast](https://www.youtube.com/watch?v=XLoswcd5rH0) ·
  [video](https://www.youtube.com/watch?v=PvL_kPjgU6o)

The [Code of Conduct](./docs/CODE_OF_CONDUCT.md) keeps review constructive.
Contact: `vinhnguyen2308 [at] gmail [dot] com`.

## Thanks to our contributors

Thank you to everyone below. You are the reason this project keeps moving.

<details open> <!-- NOTE: keep open by default. DO NOT CLOSE this -->
<!-- CONTRIBUTORS-COUNT:START --><summary>View all contributors (31)</summary><!-- CONTRIBUTORS-COUNT:END -->

<!-- CONTRIBUTORS:START -->

<!-- markdownlint-disable MD013 -->

<div align="center">

### Security Advisors

<a href="https://github.com/glmgbj233"><img src="https://avatars.githubusercontent.com/u/115564047?v=4&s=60" width="40" height="40" alt="@glmgbj233" title="@glmgbj233 GHSA-wqgw-crr5-cr2p (security advisory)" /></a>&nbsp;
<a href="https://github.com/nnfrog"><img src="https://avatars.githubusercontent.com/u/142202920?v=4&s=60" width="40" height="40" alt="@nnfrog" title="@nnfrog GHSA-r249-hpfx-x2w7 (security advisory)" /></a>&nbsp;

### Main Contributor

<a href="https://github.com/kernitus"><img src="https://avatars.githubusercontent.com/u/2789734?v=4&s=60" width="40" height="40" alt="@kernitus" title="@kernitus Main Contributor (52 commits)" /></a>&nbsp;

### Core Contributors

<a href="https://github.com/ct-jaryn"><img src="https://avatars.githubusercontent.com/u/151006958?v=4&s=60" width="40" height="40" alt="@ct-jaryn" title="@ct-jaryn Core contributor (7 commits) - lint/model preset test gates (#769), swarm diff fix (#768), CLI test harness (#767), MCP docs (#765)" /></a>&nbsp;
<a href="https://github.com/7jrxt42BxFZo4iAnN4CX"><img src="https://avatars.githubusercontent.com/u/72938937?v=4&s=60" width="40" height="40" alt="@7jrxt42BxFZo4iAnN4CX" title="@7jrxt42BxFZo4iAnN4CX Core contributor (44 commits) - subagents, hooks, config & TUI fixes (#737, #738, #740-#742+)" /></a>&nbsp;
<a href="https://github.com/xcrong"><img src="https://avatars.githubusercontent.com/u/46434477?v=4&s=60" width="40" height="40" alt="@xcrong" title="@xcrong Core contributor (2 commits)" /></a>&nbsp;
<a href="https://github.com/netbrah"><img src="https://avatars.githubusercontent.com/u/162479981?v=4&s=60" width="40" height="40" alt="@netbrah" title="@netbrah Core contributor (2 commits)" /></a>&nbsp;
<a href="https://github.com/Sachin-Bhat"><img src="https://avatars.githubusercontent.com/u/25080916?v=4&s=60" width="40" height="40" alt="@Sachin-Bhat" title="@Sachin-Bhat Core contributor (3 commits)" /></a>&nbsp;
<a href="https://github.com/gzsombor"><img src="https://avatars.githubusercontent.com/u/66230?v=4&s=60" width="40" height="40" alt="@gzsombor" title="@gzsombor Core contributor (2 commits)" /></a>&nbsp;
<a href="https://github.com/leonj1"><img src="https://avatars.githubusercontent.com/u/5171829?v=4&s=60" width="40" height="40" alt="@leonj1" title="@leonj1 Core contributor (2 commits)" /></a>&nbsp;
<a href="https://github.com/chenrui333"><img src="https://avatars.githubusercontent.com/u/1580956?v=4&s=60" width="40" height="40" alt="@chenrui333" title="@chenrui333 Core contributor (3 commits)" /></a>&nbsp;
<a href="https://github.com/oiwn"><img src="https://avatars.githubusercontent.com/u/398035?v=4&s=60" width="40" height="40" alt="@oiwn" title="@oiwn Core contributor (6 commits)" /></a>&nbsp;

### Contributors

<a href="https://github.com/apewithllm"><img src="https://avatars.githubusercontent.com/u/101594868?v=4&s=60" width="40" height="40" alt="@apewithllm" title="@apewithllm Contributor (1 commit) - OpenLLM provider guide (#804)" /></a>&nbsp;
<a href="https://github.com/pawurb"><img src="https://avatars.githubusercontent.com/u/1131944?v=4&s=60" width="40" height="40" alt="@pawurb" title="@pawurb Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/f-o-x11"><img src="https://avatars.githubusercontent.com/u/239233855?v=4&s=60" width="40" height="40" alt="@f-o-x11" title="@f-o-x11 Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/shouryamaanjain"><img src="https://avatars.githubusercontent.com/u/72159602?v=4&s=60" width="40" height="40" alt="@shouryamaanjain" title="@shouryamaanjain Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/rs545837"><img src="https://avatars.githubusercontent.com/u/114828377?v=4&s=60" width="40" height="40" alt="@rs545837" title="@rs545837 Contributor (1 commit) - optional Sem code discovery docs (#783)" /></a>&nbsp;
<a href="https://github.com/georgeatparallel"><img src="https://avatars.githubusercontent.com/u/297992784?v=4&s=60" width="40" height="40" alt="@georgeatparallel" title="@georgeatparallel Contributor (1 commit) - opt-in Parallel Search MCP config (#779)" /></a>&nbsp;
<a href="https://github.com/S2thend"><img src="https://avatars.githubusercontent.com/u/81468081?v=4&s=60" width="40" height="40" alt="@S2thend" title="@S2thend Contributor (2 commits) - checkpoint rewind/redo (#771)" /></a>&nbsp;
<a href="https://github.com/vivekgupta-memcode"><img src="https://avatars.githubusercontent.com/u/330296621?v=4&s=60" width="40" height="40" alt="@vivekgupta-memcode" title="@vivekgupta-memcode Contributor (4 commits) - Memcode OAuth setup docs (#763)" /></a>&nbsp;
<a href="https://github.com/ericcurtin"><img src="https://avatars.githubusercontent.com/u/1694275?v=4&s=60" width="40" height="40" alt="@ericcurtin" title="@ericcurtin Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/Sanjays2402"><img src="https://avatars.githubusercontent.com/u/51058514?v=4&s=60" width="40" height="40" alt="@Sanjays2402" title="@Sanjays2402 Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/diegosouzapw"><img src="https://avatars.githubusercontent.com/u/8016841?v=4&s=60" width="40" height="40" alt="@diegosouzapw" title="@diegosouzapw Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/raphamorim"><img src="https://avatars.githubusercontent.com/u/3630346?v=4&s=60" width="40" height="40" alt="@raphamorim" title="@raphamorim Contributor (1 commit) - PR #708, rio-vt migration" /></a>&nbsp;
<a href="https://github.com/TuanLe-bk18"><img src="https://avatars.githubusercontent.com/u/222461688?v=4&s=60" width="40" height="40" alt="@TuanLe-bk18" title="@TuanLe-bk18 Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/ForrestThump"><img src="https://avatars.githubusercontent.com/u/44280834?v=4&s=60" width="40" height="40" alt="@ForrestThump" title="@ForrestThump Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/RobertBorg"><img src="https://avatars.githubusercontent.com/u/1288566?v=4&s=60" width="40" height="40" alt="@RobertBorg" title="@RobertBorg Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/EvoLinkAI"><img src="https://avatars.githubusercontent.com/u/253253881?v=4&s=60" width="40" height="40" alt="@EvoLinkAI" title="@EvoLinkAI Contributor (1 commit) - Evolink provider (#664)" /></a>&nbsp;
<a href="https://github.com/uiYzzi"><img src="https://avatars.githubusercontent.com/u/40852301?v=4&s=60" width="40" height="40" alt="@uiYzzi" title="@uiYzzi Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/poelzi"><img src="https://avatars.githubusercontent.com/u/66107?v=4&s=60" width="40" height="40" alt="@poelzi" title="@poelzi Contributor (1 commit)" /></a>&nbsp;
<a href="https://github.com/morler"><img src="https://avatars.githubusercontent.com/u/478444?v=4&s=60" width="40" height="40" alt="@morler" title="@morler Contributor (1 commit)" /></a>&nbsp;

</div>

<!-- markdownlint-enable MD013 -->

<!-- CONTRIBUTORS:END -->

</details>

## Support VT Code

<div align="center">

**Current sponsors: [Codemod](https://codemod.com/) and [CodeRabbit](https://www.coderabbit.ai/).**

[![@codemod](https://avatars.githubusercontent.com/u/78830094?s=80)](https://github.com/codemod)&nbsp;
[![@coderabbitai](https://avatars.githubusercontent.com/u/132028505?s=80)](https://github.com/coderabbitai)&nbsp;

[![GitHub Sponsors](https://img.shields.io/github/sponsors/vinhnx?style=for-the-badge&label=Click%20to%20sponsor&labelColor=BFB38F&color=000000&link=https%3A%2F%2Fgithub.com%2Fsponsors%2Fvinhnx)](https://github.com/sponsors/vinhnx)

If you like VT Code, please star the repo and share it — starring helps discovery and brings in new users,
contributors, and sponsors. Thank you for growing the community.

## Star History

<a href="https://star-history.com/#vinhnx/vtcode&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)"
     srcset="https://api.star-history.com/svg?repos=vinhnx/vtcode&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)"
     srcset="https://api.star-history.com/svg?repos=vinhnx/vtcode&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=vinhnx/vtcode&type=Date" />
 </picture>
</a>

</div>

## License

First-party code is **MIT OR Apache-2.0**, available under either [LICENSE-MIT](LICENSE-MIT) or
[LICENSE](LICENSE) (Apache-2.0). You are welcome to fork, use, copy, modify, merge, publish, distribute,
sublicense, and sell copies of the code, for personal or commercial use, under either license. Keep the
license and copyright notices when you distribute copies. Third-party code keeps its original licenses,
listed in [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES).
