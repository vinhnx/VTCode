context engineering: managing the context the model sees at each inference step, so that multi-turn tool use is not drowned by history, noise, tool definitions, and intermediate results.

===

as tasks evolved into long-horizon tasks, agent capability became a system capability composed of the model, harness, context, tools, evals, sandbox, and state management together: within a finite context, external tools, and a changing environment, the agent must keep pushing toward a goal and get closer to completion over time.

===

A concrete context engineering example is: when there are too many MCP tools and too much data, we should not let every tool definition and intermediate result flow through context. Instead, the agent should write code to call tools on demand, filter the data, and bring only high-signal results back to the model. Context engineering also includes compaction, structured note-taking, and even multi-agent patterns, but these mostly serve the question of “how should context be managed?” The long-running agent harness discussed later goes one step further: it does not only manage context, but separates planning, implementation, evaluation, revision, and final acceptance into different processes.

---

Context engineering

People used to focus on prompt engineering, namely how to write the system prompt. But as agents become more multi-turn and longer-running, the focus becomes: at each inference step, what did the model see? For example, an agent’s context may contain the system prompt, developer instructions, tool definitions, MCP server descriptions, user messages, conversation history, file contents, command output, browser observations, screenshots, errors, progress notes, retrieved documents, evaluator feedback. These things cannot all be blindly stuffed in. Even if the context window becomes larger, we still need to choose selectively, because noise in a long context distracts the model and increases cost and latency. So a good context is the minimal high-signal context that lets the model complete the next step at the current step. In general, we use just-in-time context: instead of stuffing all documents, tools, and history into the model, we keep references, such as paths, URLs, commit hashes, log files, database table names, and so on. When the model needs them, it loads them through tools. In this way, the agent’s working memory keeps indexes and currently relevant fragments, rather than all of the context.

Common context techniques in long tasks include:

    Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

    Structured note-taking: Let the agent actively write task state into persistent external artifacts, such as progress.md, feature_list.json, NOTES.md, or git commits. It is not necessarily only for the same task. A project-level NOTES.md can be read by later sessions, by planner/generator/evaluator, or reused in neighboring tasks.

    Context reset: Use external artifacts, such as files produced by note-taking, git logs, test results, task lists, and so on, as startup material to open a clean new context/session. It does not preserve the full conversation history, and can clear noise and bad assumptions so that a new agent can reorient itself.

    Multi-agent architecture: Do role division and context isolation. Different agents receive different context, do different subtasks, and finally pass only high-density results back to the main flow.

===

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

https://jinyansu1.github.io/blog/2026/07/agent-context-engineering-long-running-harness/

===

check current existing .logs/.checkpoint/.sessions infra, and see if they're redudant or can be unified. The goal is to have a single source of truth for the agent's state, context, and history, which can be efficiently queried and updated as the agent progresses through its tasks. This may involve consolidating logs, checkpoints, and session data into a more streamlined format that supports both real-time inference and long-term learning. It's also prevent overhead from accumulating in the context, ensuring that the runtime environment remains responsive and efficient.

===

IMPORTANT for vtcode's compaction engine. Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

===

IMPORTANT CRITICAL: review vtcode's tool systems. the number of tools cannot expand without limit, otherwise the model wastes attention on choosing.

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

optimize CI build

INFO: CI status: in_progress (1995s elapsed)
INFO: CI status: completed (2027s elapsed)

it's taking too long to build.

https://github.com/vinhnx/VTCode/actions

The goal: reduce the CI build time. This may involve optimizing the build process, caching dependencies, parallelizing tasks, and removing unnecessary steps. We should also review the current CI configuration to identify any bottlenecks or inefficiencies that can be addressed. Optimize for speed without sacrificing reliability or correctness. Consider using incremental builds, leveraging build artifacts, and ensuring that tests are run in a way that maximizes efficiency. check connect with release.sh script for existing release infra, and see if it can be optimized or streamlined. The goal is to have a CI/CD pipeline that is fast, reliable, and easy to maintain, allowing for rapid iteration and deployment of changes.

===

• Read file /Users/vinhnguyenxuan…/vtcode/src/startup/mod.rs
└ Offset: 70
└ Limit: 400
└ Read 400 lines

IMPORTANT: some times read file tool offset and lines are too large, which can cause the model to be overwhelmed with too much information. We should implement a mechanism to limit the amount of data read from files, ensuring that only relevant sections are loaded into context. This may involve reading files in chunks, using pagination, or applying filters to extract only the necessary information. The goal is to provide the model with a manageable amount of data that is directly relevant to the current task, improving efficiency and reducing cognitive load.

===

optimize and reduce line height for tool call group. '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-07-06 at 11.18.05.png'

===

add nord themes dark/light.

===

check the agent loop again:

" 3. Goals (measurable)
• \*\*P50 launch for vtcode --version / vtcode --help... [plan]
[reasoning] Tool wall-clock budget exhausted. I have enough context to synthesize a solid plan. Let me compile what I know:"

==> here the plan mode summarization is interrupted, and then the agent changes to reasoning thinking, this is not ideal. The agent should be able to complete the plan mode summarization without being interrupted by reasoning thinking. We need to ensure that the agent can maintain focus on the current mode of operation, whether it is planning, reasoning, or executing tasks. This may involve implementing better context management, ensuring that the agent has enough information to complete its current task before switching modes, and providing clear boundaries between different modes of operation.

=> then later on, the agent completely failed and stop

"
------------------------------------------------------ Info -------------------------------------------------------
Tool execution completed, but the model follow-up failed (transient — may resolve on retry). Output above is
valid.
"

---

• I've already gathered enough context. Let me also quickly check the dispatch/mod.rs files and one more critical
area—the validate_startup_configuration and init_global_guardian/dotfolder—to confirm my findings.<tool_call>
<invoke name="unified_search"><action>list</action><items>structure</items><max_depth>1</max_depth><path>
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/config/loader</path></invoke>
</tool_call>"

==> check logs: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_621.json

===

Here's a comprehensive set of techniques, roughly ordered by impact:

## 1. Faster linker (biggest win for iteration speed)

The default linker is often the bottleneck on `cargo check`/`clippy` incremental builds. Switch to `lld` or `mold`.

**macOS:**

```toml
# .cargo/config.toml
[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

**Linux (mold is fastest):**

```bash
# Install mold, then:
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

Note: `cargo check` doesn't actually link, so this mainly helps `clippy` and full builds. Still worth setting since you'll run `cargo build`/`test` too.

## 2. sccache — cache compilation artifacts

```bash
cargo install sccache
```

```toml
# .cargo/config.toml
[build]
rustc-wrapper = "sccache"
```

Huge win for CI and for switching branches with overlapping dependency trees. Less impactful for pure incremental local iteration (incremental compilation already handles that), but great after `cargo clean` or on fresh checkouts.

## 3. Tune the dev profile

```toml
# Cargo.toml
[profile.dev]
opt-level = 0
debug = 0          # or "line-tables-only" if you need minimal debug info
incremental = true
codegen-units = 256  # more parallelism, less optimization — fine for check/clippy

[profile.dev.package."*"]
opt-level = 0
```

`debug = 0` in particular can meaningfully cut down debuginfo generation time, which `cargo check` still partially does.

## 4. Split debuginfo (macOS/Linux)

```toml
[profile.dev]
split-debuginfo = "unpacked"  # macOS: avoids slow dsymutil-style bundling
```

## 5. Reduce what gets checked

- `cargo check -p <crate>` instead of `--workspace` when iterating on one crate.
- `cargo clippy --workspace --all-targets` is expensive — during iteration, scope it: `cargo clippy -p vtcode-core --lib`.
- Use `cargo-hakari` if you have a workspace with many crates sharing dependencies — it unifies feature resolution so you're not recompiling the same dep with different feature sets across crates.

## 6. Watch mode instead of manual reruns

```bash
cargo install cargo-watch
cargo watch -x check -x clippy
```

Avoids the overhead of you manually retyping commands, and `cargo-watch` won't do anything if nothing changed.

## 7. rust-analyzer specific (if this is about editor lag, not CLI)

If the actual pain point is rust-analyzer running `cargo check`/`clippy` in the background:

```json
// VS Code settings.json (or equivalent for your editor)
"rust-analyzer.check.command": "clippy",
"rust-analyzer.cargo.buildScripts.enable": true,
"rust-analyzer.check.workspace": false
```

Setting `check.workspace: false` limits checks to the current package, which is often the single biggest editor-responsiveness fix in a large workspace.

## 8. Reduce proc-macro / heavy dependency overhead

Proc-macro-heavy crates (`serde` with derive, `tokio` full features, `clap` derive) dominate check time in many projects. Audit with:

```bash
cargo build --timings
```

This generates an HTML report (`target/cargo-timings/`) showing which crates are slowest to compile — often the actual bottleneck isn't your code but a handful of dependencies. Trim unused features (`default-features = false` + explicit feature list) where you can.

## 9. Nightly parallel frontend (experimental)

```toml
# .cargo/config.toml (nightly only)
[unstable]
build-std = false
```

```bash
RUSTFLAGS="-Z threads=8" cargo +nightly check
```

The parallel rustc frontend (`-Z threads=N`) can meaningfully speed up `check` on large crates, but requires nightly and is still maturing.

## 10. Disk space / tmpfs trick

If you're I/O-bound (common on CI or slower disks), point `target/` at a tmpfs or fast NVMe scratch disk:

```bash
export CARGO_TARGET_DIR=/tmp/target-vtcode
```

---

**Practical order to try, given a Rust CLI project like VT Code:** mold/lld first (fastest to set up, no code changes), then `cargo build --timings` to find your actual bottleneck crates, then hakari if the workspace has many interdependent crates, then sccache for CI. rust-analyzer's `check.workspace: false` is worth flipping immediately if editor lag is the real complaint rather than raw CLI time.

==> NOTE: optimize only for my local machine, not on CI/CD. for CI/CD check existing free github action infra and see if it can be optimized or streamlined.

===

make the bottom status line text foreground color to be dimmer. Like the top right status line.

=--

check the CI step check in release.sh script. (SUCCESS: CI builds completed successfully)) is shown even though the CI build is failed and cancelled (example: https://github.com/vinhnx/VTCode/actions/runs/28781549754/job/85338081603). The CI step check should accurately reflect the actual status of the CI build, and not show a success message if the build has failed or been cancelled. We need to review the logic in the release.sh script that determines the CI build status and ensure that it correctly interprets the results from the GitHub Actions API. This may involve checking for specific status codes or messages returned by the API and updating the script to handle these cases appropriately.

---

warning: unresolved link to `vtcode_safety::audit_log`
--> vtcode-config/src/safety.rs:3:29
|
3 | //! This module wires the [`vtcode_safety::audit_log`] sinks into the top-level
| ^^^^^^^^^^^^^^^^^^^^^^^^ no item named `vtcode_safety` in scope
|
= note: `#[warn(rustdoc::broken_intra_doc_links)]` on by default

warning: unresolved link to `vtcode_safety::audit_log::JsonlFileSink`
--> vtcode-config/src/safety.rs:29:66
|
29 | /// Subset of `[safety.audit]` controlling the [`JsonlFileSink`](vtcode_safety::audit_log::JsonlFileSink).
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ no item named `vtcode_safety` in scope

warning: `vtcode-config` (lib doc) generated 2 warnings

664
warning: usage of an `unsafe` block
665
--> vtcode-core/src/tools/ripgrep_installer/platform.rs:122:19
666
|
667
122 | let is_root = unsafe { libc::getuid() == 0 };
668
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
669
|
670
= note: requested on the command line with `-W unsafe-code`

681
warning: usage of an `unsafe` block
682
--> vtcode-core/src/tools/ripgrep_installer/platform.rs:122:19
683
|
684
122 | let is_root = unsafe { libc::getuid() == 0 };
685
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
686
|
687
= note: requested on the command line with `-W unsafe-code`
688
Compiling link-section v0.18.3
689
Compiling ctor v1.0.7
690
Compiling mimalloc v0.1.52
691
Compiling itertools v0.15.0
692
Compiling vtcode-acp v0.134.12 (/project/vtcode-acp)
693
warning: `vtcode-core` (lib) generated 1 warning

1046
warning: unreachable expression
1047
--> vtcode-config\src\root.rs:777:9
1048
|
1049
754 | bail!("pty.shell_zsh_fork is only supported on Unix platforms");
1050
| --------------------------------------------------------------- any code following this expression is unreachable
1051
...
1052
777 | Ok(Some(zsh_path))
1053
| ^^^^^^^^^^^^^^^^^^ unreachable expression
1054
|
1055
= note: `#[warn(unreachable_code)]` on by default


1230
warning: unused import: `ctor::ctor`
1231
  --> src\process_hardening.rs:12:5
1232
   |
1233
12 | use ctor::ctor;
1234
   |     ^^^^^^^^^^
1235
   |
1236
   = note: `#[warn(unused_imports)]` on by default
1237
warning: function `cap_stack_rlimit` is never used
1238
   --> src\process_hardening.rs:171:4
1239
    |
1240
171 | fn cap_stack_rlimit() {
1241
    |    ^^^^^^^^^^^^^^^^
1242
    |
1243
    = note: `#[warn(dead_code)]` on by default
