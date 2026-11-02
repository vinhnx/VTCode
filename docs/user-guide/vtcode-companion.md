# VT Code Companion (experimental)

VT Code Companion lets a local Codex session delegate inspection and planning to
VT Code in one selected workspace. It packages an MCP connection and a workflow
skill; VT Code owns execution, trust, permissions, task history, and evidence.
It is free and open source. Your configured model provider may charge for usage.
Repository context and prompts can be sent to that provider even though execution
runs locally.

## Current support

| Workflow | Status |
| --- | --- |
| Local Codex discovery, workspace fetch, read-only planning | Demonstrated in the pilot; large reviews can still be partial or exceed output limits. |
| Implementation after an explicit request and completed plan | Experimental; the live implementation workflow previously failed evaluator parsing. |
| ChatGPT Desktop / Local Work | One user-reported read-only planning task completed. Its review evidence was partial; implementation and full workflow remain unverified. |
| ChatGPT web/mobile or public registry installation | Not provided by this local pilot. |

The [WebMCP browser editor](webmcp.md) is a separate integration. Opening its
website does not install Companion or connect an agent to your filesystem.

## Install VT Code and prepare Companion

Companion uses VT Code's `webmcp stdio --companion` command; there is no separate
Companion command in the VT Code installer. The standard
[`scripts/install.sh`](../../scripts/install.sh) downloads the latest published
release binary. Use it only after that release includes the Companion CLI. For
this experimental feature in a source checkout, build the binary from that same
checkout as shown below so it is not paired with an older installed binary.

[`scripts/setup.sh`](../../scripts/setup.sh) prepares a contributor's Rust
environment and checks a source checkout. It updates Rust and installs
development tools; plugin users do not need to run it, and it does not install
Companion. Contributors should follow the canonical
[Development Setup](../development/DEVELOPMENT_SETUP.md) guide.

## Prepare your local plugin

You need Python 3, a plugin-capable local Codex host, and a VT Code binary built
from a checkout containing `webmcp stdio --companion`. Use a workspace you own.
Configure the model/provider in VT Code first. For coding tasks, explicitly
persist full-auto workspace trust and configure `automation.full_auto.enabled`
and the intended tool allowlist through VT Code's normal setup. Companion cannot
grant trust or approve permissions for you. Inspect these settings before use;
see [execution policy](../development/EXECUTION_POLICY.md). Choose the exact
binary and one workspace for the plugin before installing or connecting it:

```sh
vtcode --workspace /absolute/path/to/repository companion setup \
  --binary /absolute/path/to/vtcode
```

The command stores these paths in VT Code's private per-user config. It does not
grant workspace trust or change execution permissions. Re-run it to switch the
Companion workspace, then restart the plugin connection.

From the VT Code repository root:

```sh
cargo build --locked --bin vtcode
python3 scripts/package-vtcode-companion.py --local \
  --binary "$PWD/target/debug/vtcode" --workspace "$PWD" \
  --marketplace "$PWD/target/vtcode-companion-marketplace-0.1.1"
```

Replace `--workspace` with your chosen absolute repository path. The generator
creates a new directory containing a local marketplace catalog and the plugin,
including its skill, launcher, icons, and machine-specific MCP configuration.
It refuses to overwrite an existing directory. It does not install anything,
change host settings, set credentials, grant trust, or start a service.

### Install from the GitHub marketplace

The GitHub repository marketplace is the portable package to share. For a
private test of the latest merged changes, use `main`; install or build a
compatible VT Code binary and bind it to the repository you want Companion to
use:

```sh
vtcode --workspace /absolute/path/to/repository companion setup \
  --binary /absolute/path/to/vtcode
```

Then add the repository marketplace:

```sh
codex plugin marketplace add https://github.com/vinhnx/VTCode.git \
  --ref main \
  --sparse .agents/plugins \
  --sparse plugins/vtcode-companion
codex plugin add vtcode-companion@vtcode-companion
```

#### Replace an existing marketplace source

Marketplace names must be unique in your Codex configuration. Before adding this
GitHub source, check `codex plugin marketplace list`. If `vtcode-companion` is
already registered from a different source or ref, do not continue to
`codex plugin add` after the add command fails: that can install the plugin from
the old source. Remove the old installed plugin and marketplace entry, then
repeat the add commands above:

```sh
codex plugin remove vtcode-companion@vtcode-companion
codex plugin marketplace remove vtcode-companion
```

Run the plugin removal only if `codex plugin list` shows it installed from that
marketplace. Removing the marketplace unregisters that source; it does not
remove plugins installed from other marketplaces. Then add the GitHub source
and plugin again. If the marketplace already points to the intended GitHub ref,
run `codex plugin marketplace upgrade vtcode-companion` to refresh its snapshot.
To refresh an already installed plugin from that snapshot, remove that plugin
with `codex plugin remove vtcode-companion@vtcode-companion`, then add it again.

The `0.179.2` release includes the Companion source and setup command, but it
predates the later merged task-failure fix. After a release containing that fix
is published, replace `main` above with that release tag for a stable install.
Keep the marketplace source and VT Code binary on compatible revisions.

The package does not contain machine-specific paths. Its launcher reads the
binary and workspace selected by `vtcode companion setup`; the host's working
directory does not select or change the workspace. Tool arguments cannot change
either path. Python 3 is required for the launcher. Until a compatible release
is available, build VT Code from source and point setup at that binary.

Do not commit or publish the machine-specific marketplace directory or ZIP
generated with `--local --marketplace` / `--local --output`. Those packages
contain the creator's absolute binary and workspace paths and are only suitable
for that machine. Share the repository marketplace from GitHub; each recipient
selects their own binary and workspace with `vtcode companion setup`.

The launcher uses the saved binary and workspace as separate process arguments.
No shell is used. Keep the binary at its configured path and restart the MCP
connection after rebuilding. The GitHub source is reusable; generated local
marketplaces and ZIPs contain absolute local paths and belong only to your machine.

## Install in Codex

These commands install the explicit-path local package. For direct GitHub
installation, use the marketplace commands above. If `vtcode-local` is already in
`codex plugin marketplace list`, use that registered marketplace instead of
adding a second catalog with the same name:

```sh
codex plugin marketplace add "$PWD/target/vtcode-companion-marketplace-0.1.1"
codex plugin add vtcode-companion@vtcode-local
```

If `vtcode-local` is already registered, inspect `codex plugin marketplace list`
and update its source through your host's supported marketplace flow rather than
editing the installed cache. Check `codex plugin --help` if your host version
uses a different installation flow. Start a fresh local session with Companion
enabled. Availability varies by host; see OpenAI's
[local marketplace guidance](https://developers.openai.com/plugins/build/plugins).

The VT Code release installer and plugin marketplace have separate jobs: the
installer places the VT Code binary on the machine, while the marketplace
installs the plugin configuration and skill in the host. Building from source
is recommended until a published release includes `webmcp stdio --companion`.

## First read-only workflow

1. Ask: **Use VT Code Companion to fetch the connected workspace's root README.md.
   Report which Companion tool you used. Do not use GitHub, web retrieval, or shell.**
2. Confirm the server advertises `search`, `fetch`, `start_task`, `get_task`, and
   `cancel_task`. Fetching through GitHub does not verify local workspace access.
3. Ask: **Use VT Code Companion to create a read-only plan for reviewing one
   selected module. Do not implement. Start in plan mode and poll get_task until
   it completes. Report the task ID, coverage, truncation, and verification status.**
4. Review the returned plan and evidence. Missing coverage or truncated evidence
   means an incomplete review; ask for a smaller follow-up task.

For an experimental implementation trial, explicitly request implementation and
reference the successfully completed, untruncated planning task. Review the diff
and verification evidence yourself. A completed task or exit zero does not prove
checks passed. The skill must not infer implementation approval from a plan
request. There are no automatic commits, pushes, or deployments.

Only one task may run per instance. Use `get_task` for progress and `cancel_task`
for cancellation; closing the MCP input also cancels and reaps owned execution.

## ChatGPT Desktop Local Work and troubleshooting

For direct GitHub installation, add the marketplace using the command above,
restart ChatGPT Desktop, then open **Plugins → Personal** and install **VT Code
Companion** from its marketplace. For the machine-specific fallback, add the
generated catalog with `codex plugin marketplace add`, restart the app, and
install **VT Code Companion** from **VT Code Local**. Start a new **Local Work** chat
and explicitly choose `@VT Code Companion` before asking it to fetch the
workspace README. The skill and plugin tools become available to new chats after
installation. A Codex CLI installation or an MCP server's connected status alone
does not prove that ChatGPT has installed and selected this plugin.

OpenAI's [local plugin guidance](https://developers.openai.com/plugins/build/plugins)
describes local marketplaces in Work mode and Codex in the desktop app, and says
to restart the desktop app before browsing and installing from the marketplace.
Companion tool attachment and read-only planning have now been demonstrated in
one user-reported ChatGPT Desktop Local Work task. This does not establish full
review coverage or implementation reliability. Earlier conversations reported
missing tools; if that recurs, use the checks below to capture the exact host
state.

- **No tools:** confirm the plugin is installed from the local marketplace and
  explicitly selected with `@VT Code Companion` in a new Local Work chat. In
  Settings → MCP servers or `/mcp`, inspect the server and its tools. A connected
  badge or loaded skill alone is insufficient. Restart the desktop app after
  adding/updating the marketplace. If tools still never appear, record whether
  `vtcode` and all five tools are listed, plus the exact launch error.
- **Only search/fetch:** the launch command must include `--companion`.
- **Trust or permission failure:** configure the selected workspace explicitly in
  VT Code. Do not bypass trust or broaden the allowlist to hide an error.
- **Output limit, loop detection, or partial review:** update/rebuild VT Code,
  restart the MCP process, and scope the task to a smaller module. Preserve the
  failed task ID and coverage limitations in the review report.
- **Implementation evaluation failure:** treat the task as failed and inspect any
  attributed changes and evidence before continuing. The task's `progress` retains
  a sanitized canonical failure reason and reports process, event-stream, or
  cleanup failures when available. Preserve this reason along with the task and
  session IDs. A successful intermediate command does not establish task success.

For a file-content smoke test, use a byte-equality assertion such as `cmp` against
the expected text. Printing a hash or byte dump with exit zero only establishes
that the inspection ran; it does not assert that the contents match. After
removing a temporary smoke-test file, confirm its absence separately. Shell-based
create/delete operations may leave no attributed file-change event or final diff.

Inspect `command_results` for the actual smoke assertion command and its exit
code, then inspect the final absence assertion separately. The legacy
`verification_command` may be a configured build check such as
`cargo check --locked`; that result supports the build check only. An atomic
script may contain several assertions in one command record. Null commands,
null exit codes, failed results, or truncated commands leave claims unsupported.
These records are historical observations, so later mutations can change the
state they inspected. Output streams are intentionally excluded from the public
list; canonical event references support local diagnostics.

Preserve existing workspace edits during smoke tests; do not require a clean Git
tree unless that is the intended task. Use a before/after baseline comparison to
check preservation. Configured checks can create build artifacts, and the
runtime can write `.vtcode/` metadata or `memories/progress.md`. Report these
separately from requested edits. If your restriction forbids those runtime
outputs too, report the conflict before implementing; do not skip required
verification to force a successful result.

Report problems through [GitHub Issues](https://github.com/vinhnx/VTCode/issues)
with host/binary versions, task IDs, sanitized errors, and discovery results.
Remove credentials, private source, and personal paths before sharing logs.

## ZIP export and further details

To export the same local instance as a ZIP, use `--output` instead of `--marketplace`:

```sh
python3 scripts/package-vtcode-companion.py --local \
  --binary "$PWD/target/debug/vtcode" --workspace "$PWD" \
  --output "$PWD/target/vtcode-companion-local-0.1.1.zip"
```

Choose a new output path if it exists. This is a machine-specific artifact, not a
public installer. Share the repository and let each developer generate their own
marketplace instead. Public submission is deferred. See the
[technical contract and pilot evidence](../development/vtcode-companion.md) and
[future submission requirements](../development/vtcode-companion-submission.md).
