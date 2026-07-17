reference compaction summarization template for auto compact -> continuation

```
Objective
Publish a battery-pack crate for vtcode following the battery-pack-rs spec, then track it in git and push to GitHub/crates.io.
Important Details
User wants standalone battery pack, not inside vtcode workspace (avoids duplication/bloat)
Must follow cargo-bp / battery-pack-rs format exactly
vtcode-core 0.128.4 on crates.io has rmcp type-mismatch compilation error in src/mcp/rmcp_client.rs:278; vtcode-core 0.135.11 depends on internal unpublished crates (vtcode-a2a, vtcode-mcp, vtcode-llm, vtcode-skills, vtcode-safety)
vtcode-ui 0.135.11 was successfully published to crates.io
GitHub auth uses gh CLI with account vinhnx; GITHUB_TOKEN env var interferes with auth switching
Battery pack published as vtcode-battery-pack 0.135.13 on crates.io
Repo created at github.com/vinhnx/vtcode-battery-pack
Work State
Completed
Fixed clippy/compilation issues in vtcode workspace (src/agent/runloop/unified/prompts.rs:84, vtcode-ui/.../thinking.rs:127, vtcode-core/.../builtins.rs:1085-1088)
Published vtcode-ui 0.135.11 to crates.io
Created standalone vtcode-battery-pack using cargo bp new battery-pack template
Added categories (infrastructure, protocols, execution, state, tools, eval, auth), features, dependency metadata, examples, minimal template
Published vtcode-battery-pack 0.135.11, 0.135.12, 0.135.13 to crates.io
Initialized git repo and pushed to github.com/vinhnx/vtcode-battery-pack
Validation passes: cargo bp validate outputs "vtcode-battery-pack is valid"
Active
Battery pack is live on crates.io and GitHub; no active work
Blocked
vtcode-core and vtcode-ui cannot be included in the battery pack because published versions are broken and fixed versions depend on internal crates not on crates.io
Next Move
Monitor crates.io for future vtcode-core/vtcode-ui fixes and publish updated battery pack when possible
(none)
Relevant Files
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/Cargo.toml: battery pack manifest with categories, features, dependencies, template registration
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/README.md: public-facing docs on crates.io
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/docs.handlebars.md: docs.rs template
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/examples/: minimal-agent.rs, session-state.rs, tool-protocol.rs
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/templates/minimal/: project scaffold template (bp-template.toml, _Cargo.toml, src/main.rs)
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/src/lib.rs: spec-minimum doc include
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode-battery-pack/build.rs: doc generation build script
https://github.com/vinhnx/vtcode-battery-pack: GitHub repository
https://crates.io/crates/vtcode-battery-pack: crates.io listing
```

===

Unsafe Code Audit Report — vtcode Codebase
Summary
Found 4 production files containing unsafe code, with 1 unsafe impl, 2 #[unsafe(export_name = ...)] attributes, 1 libloading module, no std::mem::transmute, no extern { } blocks (only extern "C" function pointer types), and raw pointer usage confined to FFI bridge code.
Finding 1: src/allocator.rs — unsafe impl Sync without safety comment
File: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/allocator.rs
Lines 19–32 (the entire jemalloc module): #[repr(transparent)]
struct MallocConfPtr(*const u8); #[allow(clippy::undocumented_unsafe_blocks)]
unsafe impl Sync for MallocConfPtr {}
static CONF: [u8; 63] = *b"prof:true,prof_active:false,lg_prof_sample:19,prof_final:false\0"; #[cfg(not(any(target_os = "macos", target_os = "ios")))] #[used] #[allow(unsafe_code)] #[unsafe(export_name = "malloc_conf")]
static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr()); #[cfg(any(target_os = "macos", target_os = "ios"))] #[used] #[allow(unsafe_code)] #[unsafe(export_name = "_rjem_malloc_conf")]
static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr());
Attribute Details
Safety comment on unsafe impl NONE — line 21 has no // SAFETY: comment. The #[allow(clippy::undocumented_unsafe_blocks)] on line 20 actively suppresses the lint that would enforce one. #[unsafe(export_name = ...)] Present on lines 26 and 31. These export the symbol name jemalloc looks up at startup.
Risk assessment Medium. The Sync impl is for a #[repr(transparent)] wrapper around *const u8 pointing to static read-only data (CONF). The safety argument is that the pointer is to immutable static data shared across threads, so concurrent & access is valid. However, the absence of a documented // SAFETY: comment means this reasoning is not encoded in the code — if CONF ever changes to mutable or the wrapper gains interior mutability, the Sync impl silently becomes unsound.
Finding 2: crates/codegen/vtcode-skills/src/native_plugin.rs — Extensive FFI via libloading
File: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/codegen/vtcode-skills/src/native_plugin.rs
This is the highest-risk file in the codebase. It contains the entire native plugin loading and execution pipeline.
2a. Module-level safety annotation (line 22–30)
// SAFETY: This module contains FFI calls to load and execute native plugins.
// All unsafe blocks are necessary for:
// - Loading dynamic libraries (Library::new)
// - Calling FFI functions (version_fn, metadata_fn, execute_fn, free_string_fn)
// - Converting C strings (CStr::from_ptr)
// Symbol resolution is centralized in the safe `get_plugin_symbol` wrapper; the
// remaining raw calls cannot be made safe without losing the ability to load
// arbitrary plugin code. #![allow(unsafe_code)]
Attribute Details
Safety comment YES — module-level doc comment explains the rationale.
Risk assessment Low (documentation) — the #![allow(unsafe_code)] is scoped to the module and justified.
2b. extern "C" function pointer type aliases (lines 66–69)
type PluginVersionFn = unsafe extern "C" fn() -> u32;
type PluginMetadataFn = unsafe extern "C" fn() -> *const c_char;
type PluginExecuteFn = unsafe extern "C" fn(*const c_char) -> *const c_char;
type PluginFreeStringFn = unsafe extern "C" fn(*const c_char);
Attribute Details
Safety comment No per-line comment, but these are type declarations (not call sites).
Risk assessment Low — these are type aliases, not executable code. The unsafe qualifier on the function pointer type correctly signals that calling them is unsafe.
2c. decode_plugin_c_string — unsafe { CStr::from_ptr } and unsafe { free_fn } (lines 165–189)
fn decode_plugin_c_string(
ptr: NonNull<c_char>,
free_string_fn: Option<PluginFreeStringFn>,
utf8_error_context: &'static str,
) -> Result<String> {
let raw_ptr = ptr.as_ptr() as *const c_char;
// SAFETY:
// 1. `raw_ptr` is guaranteed to be non-null (validated by `ensure_non_null_c_string_ptr`).
// 2. We assume the plugin-returned pointer is a valid nul-terminated C string per the plugin ABI.
// 3. The reference created by `CStr::from_ptr` is only used to copy the data into a Rust `String`.
let decoded = unsafe { CStr::from_ptr(raw_ptr) }
.to_str()
.context(utf8_error_context)
.map(str::to_owned);

    if let Some(free_fn) = free_string_fn {
        // SAFETY: The pointer originated from the same plugin instance that provided `free_fn`.
        unsafe { free_fn(raw_ptr) };
    }

    decoded

}
Attribute Details
Safety comment YES — detailed // SAFETY: comments on both unsafe blocks (lines 171–176 and 183–184).
Risk assessment Medium — correctness depends on the plugin obeying the ABI contract (valid nul-terminated UTF-8, free*fn matches the allocator). The NonNull pre-check and ABI version gate reduce but do not eliminate the risk of a buggy/malicious plugin causing UB.
2d. get_plugin_symbol — unsafe { library.get(name) } (lines 210–220)
fn get_plugin_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
// SAFETY: symbol name and signature are defined by the plugin ABI; we trust
// the library (canonicalized/trusted location). `T: Copy` makes the deref safe.
let symbol: Symbol<T> = unsafe { library.get(name) }.with_context(|| { ... })?;
Ok(*symbol)
}
Attribute Details
Safety comment YES — // SAFETY: comment on line 211–212.
Risk assessment Low (isolated) — this is the single chokepoint for all symbol resolution. The T: Copy bound ensures the deref is valid. Risk is bounded by the trusted-path validation upstream.
2e. NativePlugin::new — multiple FFI calls (lines 236–269)
let abi*version = unsafe { version_fn() };
let metadata_ptr = ensure_non_null_c_string_ptr(unsafe { metadata_fn() }, ...)?;
let execute_fn = get_plugin_symbol::<PluginExecuteFn>(&library, b"vtcode_plugin_execute\0")?;
Attribute Details
Safety comment Partial — version_fn() call has // SAFETY: on line 239. metadata_fn() call has // SAFETY: on line 255. execute_fn goes through get_plugin_symbol which has its own safety comment.
Risk assessment Medium — if a plugin returns an invalid ABI version, the function is rejected early. But if the plugin lies about its ABI (returns correct version but has incompatible signature), calling through the wrong signature is UB.
2f. execute_ffi — calling the plugin execute function (lines 311–330)
fn execute_ffi(&self, input_cstr: CString) -> Result<PluginResult> {
// SAFETY:
// 1. The `input_cstr` pointer is valid for the duration of this call.
// 2. The `execute_fn` obeys the plugin ABI and expects a nul-terminated string.
// 3. VT Code either holds `execution_lock` or the plugin is explicitly reentrant.
let result_ptr = ensure_non_null_c_string_ptr(
unsafe { (self.execute_fn)(input_cstr.as_ptr()) },
"Plugin execute function",
)?;
...
}
Attribute Details
Safety comment YES — three-point // SAFETY: comment on lines 312–315.
Risk assessment Medium — the execution_lock guards non-reentrant plugins. The input CString is guaranteed nul-terminated. The main residual risk is a buggy or malicious plugin violating the ABI.
2g. PluginLoader::load_plugin — unsafe { Library::new(&lib_path) } (lines 383–394)
// SAFETY: Loading a dynamic library is inherently unsafe because:
// 1. The library code executes with full privileges.
// 2. `lib_path` is an existing canonical path under a trusted root, so
// path traversal and symlink escapes were rejected before this point.
// 3. The library could have bugs or malicious intent.
//
// Risk Mitigation:
// - Only load from canonicalized trusted directories.
// - Verify ABI version compatibility in `NativePlugin::new`.
// - Validate metadata format.
let library = unsafe { Library::new(&lib_path) }
.with_context(|| format!("Failed to load dynamic library at {lib_path:?}"))?;
Attribute Details
Safety comment YES — extensive // SAFETY: comment on lines 383–392.
Risk assessment Medium (trusted-path dependent) — the safety relies on ensure_trusted_path canonicalizing and verifying the path is under a trusted root. The module has tests for .. escape and symlink escape (lines 801–873). The trusted_dirs mechanism is the primary guardrail. A misconfigured trusted_dirs (e.g., pointing to /) would negate the protection.
2h. Test-only unsafe extern "C" functions (lines 629, 687) — EXCLUDED
These are inside #[cfg(test)] mod tests and are excluded from this production-only audit.
Finding 3: crates/common/vtcode-commons/src/memory.rs — macOS libc FFI
File: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/common/vtcode-commons/src/memory.rs
Lines 12–37 (macOS resident_set_size_mb): #[cfg(target_os = "macos")] #[allow(deprecated, unsafe_code, unused_qualifications)]
pub fn resident_set_size_mb() -> Option<f64> {
// SAFETY: `mach_task_basic_info` is a plain old data struct; zeroing it
// produces a valid (all-zero) starting value before `task_info` fills it.
let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
// SAFETY: `mach_task_self()` returns a send-right to the current task with
// no preconditions; it cannot fail to produce a valid port name.
let task = unsafe { libc::mach_task_self() };
// SAFETY: `task` is our own task port; `info` and `count` are valid
// out-pointers of the expected size, and `task_info` only writes them on
// success.
let ret = unsafe {
libc::task_info(
task,
libc::MACH_TASK_BASIC_INFO,
&mut info as \_mut * as *mut libc::integer_t,
&mut count,
)
};
...
}
Attribute Details
Safety comment YES — each unsafe block has a // SAFETY: comment (lines 14, 20, 23).
Risk assessment Low — standard macOS Mach API usage. mach_task_self() always returns a valid port. The #[allow(unsafe_code)] is scoped to this macOS-specific function. libc::mach_task_self is deprecated but still functional. The cast &mut info as *mut \_ as \*mut libc::integer_t is a well-known pattern for task_info's integer_t out-parameter.
Line 47 (Linux path):
let page_size = unsafe { libc::sysconf(libc::\_SC_PAGESIZE) } as f64;
Attribute Details
Safety comment NO — no // SAFETY: comment on this unsafe block.
Risk assessment Low — sysconf is a pure syscall wrapper with no safety invariants beyond valid arguments, which are compile-time constants. The missing safety comment is a minor style gap.
Finding 4: crates/common/vtcode-commons/src/env_lock.rs — Thread-unsafe POSIX env vars
File: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/common/vtcode-commons/src/env_lock.rs
Lines 58–65 and 74–79:
pub fn set_var(&self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
// SAFETY: `self` is the unique holder of the process-wide env mutex
// for the duration of this call; all set/remove calls in this module
// route through the same mutex, so no concurrent env access occurs.
unsafe {
std::env::set_var(key, value);
}
}

pub fn remove_var(&self, key: impl AsRef<OsStr>) {
// SAFETY: see `set_var` — the guard serializes all mutators.
unsafe {
std::env::remove_var(key);
}
}
Attribute Details
Safety comment YES — both unsafe blocks have // SAFETY: comments (lines 59–61 and 75). #[expect(unsafe_code, reason = ...)] Present on lines 54–57 and 70–73, with explicit rationale.
Risk assessment Low (well-contained) — the EnvGuard struct holds a MutexGuard<'static, ()>, which proves at the type level that no other thread can enter the env mutation path. The unsafe is required because Rust 2024 marks std::env::set_var/remove_var as unsafe (POSIX setenv/getenv are not thread-safe). The safety invariant is: the guard's Mutex serializes all env mutations in-process. External C libraries calling setenv concurrently are out of scope.
Finding 5: crates/common/vtcode-commons/src/thread_safety.rs — PhantomData<*mut ()> (NO unsafe code)
File: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/common/vtcode-commons/src/thread_safety.rs
Line 322:
pub struct MainThreadToken(PhantomData<*mut ()>);
Attribute Details
Safety comment N/A — this is safe Rust. PhantomData<*mut ()> is a standard pattern to make a type !Send + !Sync without any unsafe code. The *mut () inside PhantomData is never dereferenced.
Risk assessment None — pure safe Rust. No unsafe blocks or impls.
Items explicitly searched for and NOT found
Pattern Result
std::mem::transmute Not found anywhere in the codebase
extern { ... } blocks (C-style extern blocks) Not found — only extern "C" fn function pointer type aliases
unsafe impl in crates/ (outside allocator.rs) Not found #[unsafe(export_name = ...)] Only in src/allocator.rs (lines 26, 31)
Consolidated Risk Summary
File Pattern Safety Comment Risk Level
src/allocator.rs:21 unsafe impl Sync for MallocConfPtr MISSING — suppressed by #[allow(clippy::undocumented_unsafe_blocks)] Medium
src/allocator.rs:26,31 #[unsafe(export_name = ...)] Contextual (module-level doc) Low
vtcode-skills/.../native_plugin.rs:22-30 #![allow(unsafe_code)] Module-level // SAFETY: doc Low
vtcode-skills/.../native_plugin.rs:211-212 unsafe { library.get(name) } // SAFETY: present Low
vtcode-skills/.../native_plugin.rs:171-176 unsafe { CStr::from_ptr(raw_ptr) } // SAFETY: present Medium
vtcode-skills/.../native_plugin.rs:185 unsafe { free_fn(raw_ptr) } // SAFETY: present Medium
vtcode-skills/.../native_plugin.rs:383-392 unsafe { Library::new(&lib_path) } Extensive // SAFETY: Medium (trusted-path dependent)
vtcode-commons/.../memory.rs:16 unsafe { std::mem::zeroed() } // SAFETY: present Low
vtcode-commons/.../memory.rs:22,26-32 unsafe { libc::mach_task_self() } / task_info // SAFETY: present Low
vtcode-commons/.../memory.rs:47 unsafe { libc::sysconf(...) } MISSING Low
vtcode-commons/.../env_lock.rs:62 unsafe { std::env::set_var(...) } // SAFETY: present + #[expect] Low
vtcode-commons/.../env_lock.rs:76 unsafe { std::env::remove_var(...) } // SAFETY: present + #[expect] Low
Key Recommendations

1. src/allocator.rs:21 — Add a // SAFETY: comment to the unsafe impl Sync for MallocConfPtr explaining that the wrapper is #[repr(transparent)] over \*const u8 pointing to immutable static data, so shared & references across threads are valid. Remove the #[allow(clippy::undocumented_unsafe_blocks)] suppression once the comment is present.
2. vtcode-commons/.../memory.rs:47 — Add a // SAFETY: comment to the unsafe { libc::sysconf(...) } block, noting that \_SC_PAGESIZE is a compile-time constant and the syscall has no mutable aliasing concerns.
3. vtcode-skills/.../native_plugin.rs — The overall structure is sound: symbol resolution is centralized in get_plugin_symbol, all unsafe blocks have safety comments, and the execution_lock guards non-reentrant plugins. The main architectural risk is the trusted_dirs configuration — if an operator adds a writable directory like /tmp to trusted_dirs, the symlink and .. escape tests are the only remaining protection. Consider adding a startup warning when trusted dirs are writable by non-owners.

===

Here is a comprehensive summary of the vtcode codebase structure and architectural observations.

1. Overall Crate Layout
   The workspace is organized under crates/ with two top-level directories:
   Directory Crates Purpose
   crates/common/ vtcode-commons, vtcode-exec-events, vtcode-macros, vtcode-utility-tool-specs Shared, reusable infrastructure with minimal business logic
   crates/codegen/ 14 crates (see below) Feature crates containing actual runtime logic
   xtask/ Release automation Build/release scripting
   Common crates (4):

- vtcode-commons — Shared traits, utilities, error handling, path resolution, ANSI styling, env locking
- vtcode-exec-events — Authoritative ThreadEvent schema (the single runtime event contract)
- vtcode-macros — Proc-macro crate (derive/attribute macros); zero internal vtcode dependencies
- vtcode-utility-tool-specs — Passive JSON schemas for tool surfaces; leaf dependency
  Codegen crates (14):
- vtcode-core — The central runtime (agent loop, tools, LLM orchestration, config, safety)
- vtcode-ui — Unified TUI/design system (ratatui-based)
- vtcode-config — Config loading, schema, constants, model definitions
- vtcode-llm — LLM provider abstraction and implementations
- vtcode-skills — Skill types, discovery, loading, validation
- vtcode-mcp — Model Context Protocol client
- vtcode-bash-runner — Shell execution helpers
- vtcode-safety — Command safety detection and sandboxing
- vtcode-session-store — Append-only ThreadEvent log + derived views
- vtcode-auth — OAuth PKCE and credential storage
- vtcode-acp — Agent Client Protocol (Zed integration)
- vtcode-a2a — Agent2Agent protocol
- vtcode-indexer — Workspace file indexer
- vtcode-eval — Agent evaluation framework

2. Dependency Graph (Key Relationships)
   vtcode (binary)
   ├── vtcode-core [default-features = false]
   ├── vtcode-ui
   ├── vtcode-config
   ├── vtcode-auth
   ├── vtcode-session-store
   ├── vtcode-eval
   └── ...

vtcode-core (the hub)
├── vtcode-commons
├── vtcode-exec-events
├── vtcode-macros
├── vtcode-utility-tool-specs
├── vtcode-config
├── vtcode-auth
├── vtcode-ui
├── vtcode-indexer
├── vtcode-bash-runner
├── vtcode-safety
├── vtcode-session-store
├── vtcode-llm [features = ["copilot"]]
├── vtcode-mcp
├── vtcode-skills
└── vtcode-a2a

vtcode-acp
├── vtcode-core
├── vtcode-config
└── vtcode-commons

vtcode-llm
├── vtcode-commons
├── vtcode-config
├── vtcode-exec-events
├── vtcode-macros
└── vtcode-utility-tool-specs

vtcode-mcp
├── vtcode-commons
├── vtcode-config
├── vtcode-llm
└── vtcode-utility-tool-specs

vtcode-ui
├── vtcode-commons
└── vtcode-config

Leaf/low-dependency crates:
├── vtcode-eval → commons, exec-events
├── vtcode-session-store → exec-events
├── vtcode-bash-runner → commons [optional: exec-events]
├── vtcode-indexer → commons
├── vtcode-safety → commons
├── vtcode-skills → commons, config
├── vtcode-auth → (no internal vtcode deps)
└── vtcode-a2a → (no internal vtcode deps)
Key observation: vtcode-core is the central hub, depending on 13 of the 14 other workspace crates (everything except vtcode-eval). vtcode-commons is the universal foundation, depended on by almost every crate. 3. Unsafe Code Blocks
Unsafe code is concentrated in a few specific areas:
File Purpose Risk Level
src/allocator.rs jemalloc malloc_conf export via #[unsafe(export_name = ...)]; unsafe impl Sync for transparent pointer Low (well-isolated allocator config)
crates/common/vtcode-commons/src/memory.rs libc::mach_task_basic_info, mach_task_self(), sysconf for RSS reporting on macOS Low (platform-specific, bounded)
crates/common/vtcode-commons/src/env_lock.rs POSIX setenv/getenv are not thread-safe; mutex-serialized env mutation Low (centralized, startup-only)
crates/codegen/vtcode-skills/src/native_plugin.rs FFI boundary for dynamic plugin loading (libloading, CStr::from_ptr, extern "C" calls) Medium-High — loads arbitrary native code; safety relies on trusted-path canonicalization and ABI version checks
crates/codegen/vtcode-core/src/tools/registry/builtins.rs #![allow(unsafe_code)] at crate level Low (likely test-related suppression)
The native_plugin.rs system is the most significant unsafe surface. It loads .dylib/.so/.dll files from "trusted" directories with ABI version verification, but the safety contract depends on path canonicalization holding. 4. Large Files / Monolith Concerns
vtcode-core is the dominant crate by volume. Files exceeding 1,500 lines:
File Lines Concern
src/prompts/system.rs 2,486 Massive prompt template definitions
src/models_manager/model_presets.rs 2,396 Model preset data (could be codegen'd or external JSON)
src/tools/registry/mod.rs 2,274 Tool registry (should be split)
src/tools/handlers/session_tool_catalog.rs 2,236 Tool catalog shaping
src/utils/session_archive.rs 2,097 Session archive utilities
src/skills/executor.rs 2,074 Skill execution engine
src/cli/args.rs 2,072 CLI argument definitions (clap)
src/core/loop_detector.rs 2,007 Loop detection logic
src/tools/registry/execution_history.rs 1,971
src/tools/registry/execution_facade.rs 1,963
src/subagents/tests.rs 1,882 Test file (justified)
src/tools/code_search.rs 1,869
src/tools/handlers/read_file.rs 1,784
src/commands/init.rs 1,748
src/tool_policy.rs 1,691
Binary crate duplication concern: The src/agent/runloop/unified/ directory alone contains 322 files totaling ~106,000 lines. This overlaps thematically with vtcode-core/src/core/agent/ (77 files). The AGENTS.md notes that agent/runloop/unified/turn/compaction/ is a "thin delegator" and that compaction logic lives in vtcode-core::compaction, but the sheer volume of code in the binary crate's agent/ tree suggests either:

- Significant duplication between src/agent/runloop/ and vtcode-core/src/core/agent/
- Or the binary crate contains the "real" runloop while vtcode-core provides supporting infrastructure

5. Architectural Red Flags
   A. vtcode-core is a "God Crate"
   vtcode-core depends on 13 of 14 other workspace crates and has 70+ public modules. Its lib.rs is 508 lines of re-exports. The AGENTS.md acknowledges: "lib.rs is 500+ lines — append re-exports, don't restructure." This is a known accumulation problem.
   B. Binary Crate Contains Massive Agent Logic
   src/agent/runloop/unified/ has 106k+ lines across 322 files. The root src/AGENTS.md says this is the "single-agent runloop" and that "multi-agent is in vtcode-core::subagents", but the volume suggests the binary crate is doing substantial runtime work that arguably belongs in vtcode-core.
   C. vtcode-acp Depends on vtcode-core
   vtcode-acp depends on vtcode-core, which is fine, but vtcode-core also re-exports ACP-related types. This is a minor coupling concern — if ACP needs to reference core types, this is expected, but it creates a bidirectional conceptual link.
   D. Feature Flag Complexity
   vtcode-core has features: default, tui, mock, policies, schema, a2a-server, anthropic-api, http-server, desktop-notifications, sqlite. The root binary disables vtcode-core's default features (default-features = false) and then selectively enables them. This creates a complex feature interaction surface.
   E. Tree-Sitter Version Pinning
   Multiple tree-sitter crates are pinned to specific minor versions (0.23–0.26 range) across vtcode-core and vtcode-safety. This creates a maintenance burden and potential for subtle AST-parsing differences.
   F. Native Plugin System (vtcode-skills)
   The native_plugin.rs module loads arbitrary native code via libloading. While it has path canonicalization and ABI version checks, this is the most security-sensitive unsafe surface in the codebase. A compromised plugin directory could lead to arbitrary code execution.
   G. The "Partial Extraction" Pattern
   vtcode-skills, vtcode-llm (copilot), and vtcode-core's open_responses/copilot modules use a "partial extraction" pattern where vtcode-core re-exports from sub-crates but keeps local sub-modules. This creates a split-brain architecture where the "canonical" implementation lives in the sub-crate but vtcode-core maintains parallel local modules.
6. Summary
   The vtcode codebase is a large, mature Rust workspace (~30 crates) with a clear separation between:

- Common crates (crates/common/) — pure infrastructure, zero business logic
- Codegen crates (crates/codegen/) — runtime feature implementations
  vtcode-core is the architectural center, depending on nearly every other crate. The main immediate concerns are:

1. Monolithic files in vtcode-core (several >2,000 lines)
2. Massive binary crate agent code (~106k lines in src/agent/runloop/) that may overlap with vtcode-core
3. Unsafe FFI surface in the native plugin system
4. God-crate syndrome in vtcode-core (70+ modules, 13 inbound deps within the workspace)

===

Based on my thorough scan of 50+ files across the vtcode codebase, here are the confirmed production-code uses of unwrap(), expect(), and panic! that could cause crashes, excluding all test modules and test files:
CRITICAL / HIGH SEVERITY

1. ModelId metadata invariant violations (3 files)
   Severity: Critical — Will panic on valid input if codegen table is out of sync with enum
   crates/codegen/vtcode-config/src/models/model_id/as_str.rs:30
   .expect("generated OpenRouter model should have metadata")
   Panics if any non-custom ModelId variant lacks OpenRouter metadata. A developer adding a new model variant without updating the codegen table will crash at runtime.
   crates/codegen/vtcode-config/src/models/model_id/description.rs:42
   .expect("generated OpenRouter model should have metadata")
   Same invariant violation.
   crates/codegen/vtcode-config/src/models/model_id/display.rs:33
   .expect("generated OpenRouter model should have metadata")
   Same invariant violation.
2. Default theme missing at startup
   Severity: Critical — App cannot start
   crates/codegen/vtcode-ui/src/theme/runtime.rs:27
   let default = theme_definition(DEFAULT_THEME_ID).expect("default theme must exist");
   Static initialization panics if the default theme isn't registered. The entire UI fails to launch.
3. Init command crashes on user input
   Severity: High — Will panic under normal user interaction
   src/cli/init.rs:184
   GuidedInitAnswer::from_input(question.key, Some(&option.value), None)
   .expect("recommended option produces an answer")
   src/cli/init.rs:193
   GuidedInitAnswer::from_input(question.key, Some(&option.value), None)
   .expect("selected option produces an answer")
   src/cli/init.rs:201
   GuidedInitAnswer::from_input(question.key, None, Some(&custom))
   .expect("custom prompt always yields an answer")
   src/cli/init.rs:210
   GuidedInitAnswer::from_input(question.key, None, Some(trimmed))
   .expect("typed custom input produces an answer")
   All four expect() calls are on user-provided input during the interactive vtcode init flow. If from_input() returns Err for any valid user input, the CLI crashes.
4. Mutex poisoning in llama.cpp provider
   Severity: High — Will panic if any thread panics while holding the lock
   crates/codegen/vtcode-llm/src/providers/llamacpp.rs:208
   MANAGED_LLAMACPP_SERVERS.lock().expect("llama.cpp managed server map poisoned");
   If any thread panics while holding this mutex, all subsequent llama.cpp operations will panic due to poisoning.
   MEDIUM SEVERITY
5. HTTP client build failure
   Severity: Medium — Can fail due to environmental issues
   crates/common/vtcode-commons/src/http.rs:64
   panic!(
   "failed to build HTTP client: {e}. \
    This usually indicates a TLS configuration issue or system resource exhaustion."
   )
   Crashes if the system TLS certificates are missing or resources are exhausted. There's a try_build_client() alternative but build_client() is used in production paths.
6. Provider builder documented panics
   Severity: Medium — Configuration bugs crash the process
   crates/codegen/vtcode-core/src/llm/provider_builder.rs:93
   Err(error) => panic!(
   "provider builder invariant violated for `{}`: {}",
   T::PROVIDER_KEY, error
   )
   crates/codegen/vtcode-core/src/llm/provider_builder.rs:143
   panic!("provider config invariant violated for `{}`: {}", Self::PROVIDER_KEY, error)
   Both are in the build() method (documented to panic). If try_build() fails due to a configuration bug that validation missed, the process crashes.
7. Config builder defensive check
   Severity: Medium — Should never fail, but could with logic bugs
   crates/codegen/vtcode-config/src/loader/builder.rs:107
   .expect("inserted table should remain a table")
   Inside insert_cli_config_keys. The code just inserted the key as a table on the previous iteration, but concurrent modification or a logic bug could cause this to fail.
8. Update command version inconsistency
   Severity: Medium — Logic bug could crash update
   src/cli/update.rs:127
   .expect("is_pinned() returned true, so pinned_version() should be Some")
   If is_pinned() and pinned_version() become inconsistent, the update unpin command crashes.
9. TUI thinking reflow empty spans
   Severity: Medium — Rendering edge case
   crates/codegen/vtcode-ui/src/tui/core_tui/session/reflow/thinking.rs:129
   .expect("combined_spans is not empty")
   Crashes if the thinking block rendering produces empty spans unexpectedly.
10. Config TOML table initialization
    Severity: Medium — Defensive check after initialization
    src/agent/runloop/unified/turn/session/slash_commands/config_toml.rs:21
    .expect("table entry should be a table after initialization")
    The function ensures the entry is a table just before this call, but a logic bug or corrupted TOML could trigger the panic.
    LOW SEVERITY (Hardcoded patterns / compile-time)
11. Regex compilation panics (LazyLock statics)
    Severity: Low — Only panics if hardcoded regex string is invalid
    crates/common/vtcode-commons/src/at*pattern.rs:12
    Err(error) => panic!("Failed to compile @ pattern regex: {error}")
    crates/common/vtcode-commons/src/sanitizer.rs:60
    Err(err) => panic!("invalid regex pattern `{pattern}`: {err}")
    crates/codegen/vtcode-ui/src/tui/ui/markdown/links.rs:7
    Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$").expect("invalid location suffix regex")
crates/codegen/vtcode-ui/src/tui/ui/markdown/links.rs:11
Regex::new(r"^L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$").expect("invalid hash location regex")
    crates/codegen/vtcode-ui/src/tui/ui/markdown/parsing.rs:17
    Regex::new(r"\S+").expect("valid transcript token regex")
    src/agent/runloop/unified/settings_interactive/docs.rs:32
    Regex::new(r#"\"([^\"]+)\""#).expect("valid regex")
    src/agent/runloop/unified/turn/session/slash_commands/share_log.rs:22
    Regex::new(r"\b[A-Z0-9.*%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b").expect("valid email regex")
    src/agent/runloop/unified/turn/session/slash_commands/share_log.rs:25
    Regex::new(r"(?P<prefix>/(?:Users|home)/)[^/\s]+").expect("valid user path regex")
    src/updater/release_notes.rs:72
    Regex::new(r"\s*\([0-9a-fA-F]+\)\s*(\(@[^)]+\))?\s\*$").expect("valid regex pattern")
12. Proc-macro panics on invalid derive input
    Severity: Low — Crashes compiler, not runtime
    crates/common/vtcode-macros/src/lib.rs:43
    let field = fields.unnamed.first().unwrap();
    The StringNewtype derive macro panics if applied to a tuple struct with zero fields. Should return syn::Error instead.
    Summary by Category
    Category
    ModelId metadata invariant (3 files)
    Theme/runtime startup
    User input processing (init CLI)
    Mutex poisoning (llama.cpp)
    HTTP client / provider builder
    Defensive checks (config, TUI, update)
    Hardcoded regex LazyLock
    Proc-macro derive
    Total: 27 production-code crash risks identified across 20+ files.
