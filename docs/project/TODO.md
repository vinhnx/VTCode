reference compaction summarization template for auto compact -> continuation for vtcode harnessing agent, m

```
--- compaction ---

+ Thought: 9.5s
Objective
Restructure vscode-extension/, zed-extension/, and xtask/ into better locations and update all codebase references. Then continue with build improvements, starting with fixing failing tests caused by broken docs/models.json path in vtcode-config build script after the restructure.
Important Details
User chose: both extensions → extensions/; xtask → crates/codegen/xtask/
cargo check --locked passes after all moves
cargo nextest run --profile quick revealed 3 test failures in model picker/capabilities:
static_model_search_terms_include_modalities_and_tool_state expects "modalities" but gets "input text image"
test_generated_model_capability_lookup panics at model_catalog_entry("openai", "gpt-5.4") (returns None)
test_enum_variants_match_all_models_collection enum mismatch
Root cause: vtcode-config/build.rs generates model_capabilities.rs from docs/models.json. After moving crates under crates/codegen/, the hardcoded ../docs/models.json path is wrong. Generated file shows empty ENTRIES/PROVIDERS, meaning load_model_capability_entries is returning Ok(Vec::new()) because the file isn't found.
Path math from crates/codegen/vtcode-config/: ../../../docs/models.json reaches repo-root docs/models.json. The load_provider_metadata function was already updated to this path; load_model_capability_entries still has the wrong relative path (../../docs/models.json).
sccache was masking build script execution during earlier rebuilds; bypass with RUSTC_WRAPPER="" cargo clean -p vtcode-config && RUSTC_WRAPPER="" cargo build -p vtcode-config
Work State
Completed
Moved vscode-extension/ → extensions/vscode-extension/
Moved zed-extension/ → extensions/zed-extension/
Moved xtask/ → crates/codegen/xtask/
Updated all references in Cargo.toml, docs, scripts, continuation.rs, root AGENTS.md, crate AGENTS.md files
Verified cargo check --locked passes
Identified failing tests and root cause in vtcode-config/build.rs
Active
Fixing crates/codegen/vtcode-config/build.rs path so model capabilities are actually generated from docs/models.json
Need to verify generated model_capabilities.rs contains real data
Need to re-run failing tests to confirm fixes
Blocked
None currently, but path fix in load_model_capability_entries is incomplete and debug logging may still be present in build.rs
Next Move
Fix crates/codegen/vtcode-config/build.rs: change load_model_capability_entries path from ../../docs/models.json to ../../../docs/models.json and remove any debug eprintln! statements
Rebuild with RUSTC_WRAPPER="" cargo clean -p vtcode-config && RUSTC_WRAPPER="" cargo build -p vtcode-config and inspect generated target/debug/build/vtcode-config-*/out/model_capabilities.rs to confirm non-empty ENTRIES/PROVIDERS
Run the failing tests: cargo nextest run -p vtcode agent::runloop::model_picker::tests::static_model_search_terms_include_modalities_and_tool_state and cargo nextest run -p vtcode-config models::tests::test_generated_model_capability_lookup
Continue with broader performance/IO deep-dive using the vtcode-dev-helper skill (quality gate, clippy, nextest) after model capability tests pass
Relevant Files
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/codegen/vtcode-config/build.rs — build script with broken relative path to docs/models.json causing empty model catalog generation
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/docs/models.json — source of truth for model capabilities
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/model_picker/tests.rs — failing test expecting "modalities" term
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/crates/codegen/vtcode-config/src/models/tests.rs — failing capability lookup test
/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-dev-helper — skill for quality gate, testing, linting workflows
▣  Compaction · Step 3.7 Flash · 26.6s

// then agent continue and resume works based on the above context, and the agent will continue to work on the next steps of the project, including fixing the build script path and verifying that the model capabilities are generated correctly. The agent will also run the failing tests to ensure that the fixes have resolved the issues.
```

===

reference and explore research /Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main and apply learning to improve vtcode codebase.
