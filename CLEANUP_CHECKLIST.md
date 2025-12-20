# VTCode Configuration Cleanup - Checklist

## Phase 1: Configuration Changes

- [ ] **Remove hooks section from vtcode.toml (lines 746-774)**
  - [ ] Delete `[hooks.lifecycle]` empty section
  - [ ] Delete all commented hook examples (lines 758-774)
  - [ ] Keep `[model]` section (lines 753-756)
  - Estimated: 5 minutes

- [ ] **Remove dead semantic compression config (lines 481-488)**
  - [ ] Delete `semantic_compression = false`
  - [ ] Delete `tool_aware_retention = false`  
  - [ ] Delete `max_structural_depth = 3`
  - [ ] Delete `preserve_recent_tools = 5`
  - Estimated: 2 minutes

- [ ] **Disable vibe_coding by default (line 172)**
  - [ ] Change `enabled = true` to `enabled = false`
  - Estimated: 1 minute

- [ ] **Verify telemetry settings (lines 525-532)**
  - [ ] Confirm `trajectory_enabled = true` (REQUIRED)
  - [ ] Confirm `dashboards_enabled = false`
  - [ ] Confirm `bottleneck_tracing = false`
  - Estimated: 2 minutes

**Phase 1 Total: 10 minutes**

---

## Phase 2: Create Experimental Documentation

- [ ] **Create `docs/experimental/` directory**
  ```bash
  mkdir -p docs/experimental
  ```

- [ ] **Create `docs/experimental/HOOKS.md`**
  - [ ] Document hook configuration syntax
  - [ ] Provide 2-3 practical examples
  - [ ] Note: Feature is disabled by default
  - Estimated: 10 minutes

- [ ] **Create `docs/experimental/VIBE_CODING.md`**
  - [ ] Explain entity resolution capabilities
  - [ ] Document all configuration parameters
  - [ ] Note performance implications
  - Estimated: 8 minutes

- [ ] **Create `docs/experimental/CONTEXT_OPTIMIZATION.md`**
  - [ ] Explain semantic compression (planned)
  - [ ] Explain tool-aware retention (planned)
  - [ ] Clarify current status: "not implemented"
  - [ ] Show reserved config structure
  - Estimated: 8 minutes

**Phase 2 Total: 26 minutes**

---

## Phase 3: Documentation Cleanup

- [ ] **Check `docs/config.md` for removed sections**
  ```bash
  grep -n "semantic_compression\|tool_aware_retention\|hooks" docs/config.md
  ```
  - [ ] If found, remove references
  - Estimated: 5 minutes

- [ ] **Update README if it mentions experimental features**
  - [ ] Search for "vibe_coding", "semantic", "trajectory"
  - [ ] Remove if listed as default
  - Estimated: 3 minutes

**Phase 3 Total: 8 minutes**

---

## Phase 4: Verification

- [ ] **Parse config file**
  ```bash
  # Create a simple test to verify TOML is valid
  ```
  - Estimated: 2 minutes

- [ ] **Build and test**
  ```bash
  cargo check 2>&1 | grep -i "error"
  cargo nextest run config 2>&1 | tail -5
  ```
  - Estimated: 30-60 minutes (depends on system)

- [ ] **Verify agent starts**
  ```bash
  timeout 5 cargo run -- ask "test" 2>&1 | head -20
  ```
  - Estimated: 30-60 minutes (depends on system)

**Phase 4 Total: 2 minutes + build time**

---

## Phase 5: Git & Cleanup

- [ ] **Review changes**
  ```bash
  git diff vtcode.toml
  git status docs/
  ```

- [ ] **Commit changes**
  ```bash
  git add vtcode.toml docs/experimental/
  git commit -m "chore: reduce config complexity, move experimental features to docs"
  ```

- [ ] **Update CHANGELOG.md** (if applicable)
  ```markdown
  - Removed dead semantic compression config
  - Disabled vibe_coding by default (experimental)
  - Removed commented hooks configuration
  - Added docs/experimental/ directory for experimental features
  ```

**Phase 5 Total: 5 minutes**

---

## Quick Commands

### View changes before committing
```bash
# Show lines being removed
git diff vtcode.toml | grep "^-"

# Show new experimental docs
ls -la docs/experimental/

# Verify config syntax
cargo build --message-format=short 2>&1 | head -20
```

### Rollback if needed
```bash
git checkout vtcode.toml
rm -rf docs/experimental/
```

---

## Timeline Summary

| Phase | Task | Duration | Risk |
|-------|------|----------|------|
| 1 | Config changes | 10 min | Very Low |
| 2 | Write docs | 26 min | None |
| 3 | Update existing docs | 8 min | Low |
| 4 | Verification | 2 min + build | None |
| 5 | Git & commit | 5 min | None |
| | **TOTAL** | **51 min + build** | **Very Low** |

---

## Notes

- ✅ **No code changes required** - Only config and documentation
- ✅ **No breaking changes** - Disabled features are not currently used
- ✅ **Core agent unaffected** - All essential functionality preserved
- ✅ **Easy rollback** - Just revert vtcode.toml and delete docs/experimental/

---

## Success Criteria

After completion, verify:

1. [ ] `cargo check` passes without errors
2. [ ] `cargo nextest run config` passes
3. [ ] Agent starts with `cargo run`
4. [ ] vtcode.toml is ~38 lines shorter (775 → ~737)
5. [ ] Experimental features documented in docs/experimental/
6. [ ] No new clippy warnings
7. [ ] Git commit created with descriptive message
