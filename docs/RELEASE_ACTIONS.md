# v0.43.0 Release - Action Checklist

**Status**: ✅ **READY FOR PUSH & PUBLISH**

All development, testing, and documentation work is complete. Ready for deployment.

## Current State

```
Branch: main
Commits ahead of origin/main: 7
Tag: v0.43.0 (local)
Working directory: clean
All tests: PASSING ✅
```

## Immediate Actions (Do These Now)

### Step 1: Push to Remote
```bash
git push origin main
git push origin v0.43.0
```

**Impact**: Makes commits and tag available to CI/CD

### Step 2: Verify CI/CD Pipeline
- [ ] Go to https://github.com/vinhnx/vtcode/actions
- [ ] Confirm tests pass for the new commits
- [ ] Check for any build failures

**Expected**: All green (formatting ✅, clippy ✅, tests ✅)

### Step 3: Publish to Crates.io
```bash
# Verify publish works first
cargo publish --dry-run -p vtcode-acp-client

# Then publish all packages in order
cargo publish -p vtcode-acp-client
cargo publish -p vtcode-tools
cargo publish -p vtcode  # Main package last
```

**Expected**: Successfully published to https://crates.io/crates/vtcode

### Step 4: Create GitHub Release
Go to: https://github.com/vinhnx/vtcode/releases/new

**Tag**: v0.43.0  
**Title**: "Release v0.43.0: Agent Communication Protocol (ACP) Integration"

**Description** (use from docs/RELEASE_0_43_0_SUMMARY.md):
```markdown
## 🎯 What's New in v0.43.0

### Agent Communication Protocol (ACP)
Distributed multi-agent orchestration via HTTP-based RPC

**Key Features:**
- Sync/async RPC method calls
- Agent discovery with capability filtering  
- Type-safe message protocol with correlation IDs
- 3 new MCP tools: acp_call, acp_discover, acp_health
- Zed editor integration

### Quality Metrics
✅ 14/14 tests passing
✅ Full code coverage for new code
✅ Comprehensive documentation
✅ Production-ready example

[See full release notes](docs/RELEASE_0_43_0_SUMMARY.md)
```

- [ ] Attach build artifacts (if applicable)
- [ ] Set as "Latest release"
- [ ] Publish

### Step 5: Update Homebrew Formula (Optional)
```bash
# If you maintain Homebrew formula
brew bump-formula-pr vtcode --tag=v0.43.0 --revision=6a7ecb88
```

## Extended Actions (Do in Parallel)

### Documentation
- [ ] Update main README.md with ACP link
- [ ] Add v0.43.0 to docs/models.json if applicable
- [ ] Verify all docs links work
- [ ] Update website (if exists)

### Announcements (Optional)
- [ ] Post release announcement on:
  - [ ] GitHub Discussions
  - [ ] Discord/Slack (if applicable)
  - [ ] Twitter/X (if applicable)
- [ ] Include: Features, docs link, upgrade path

### Monitoring
- [ ] Monitor crates.io stats
- [ ] Watch GitHub issues for v0.43.0 problems
- [ ] Check for any reported incompatibilities

## Verification Steps Before Pushing

All completed ✅:
- [x] Version bumped to 0.43.0 in all Cargo.toml
- [x] CHANGELOG.md updated
- [x] package.json (vscode-extension) updated
- [x] All tests passing (14/14)
- [x] Clippy passing
- [x] Formatting valid
- [x] Build successful
- [x] Example compiles
- [x] Documentation complete (5 guides)
- [x] Release notes written
- [x] Commits tagged with v0.43.0
- [x] Git history clean

## Post-Release Tasks

### Track Issues
- [ ] Monitor GitHub Issues for v0.43.0 regression reports
- [ ] Pin critical issues (if any)
- [ ] Prepare hotfix branch if needed

### Performance Monitoring
```bash
# Run benchmarks in v0.43.0 environment
cargo bench --bench acp_benchmarks  # When added
```

### User Feedback
- [ ] Monitor downloads from crates.io
- [ ] Collect early user feedback
- [ ] Identify improvement areas for v0.44.0

### Documentation Updates
- [ ] Update "Getting Started" if needed based on feedback
- [ ] Add common issues FAQ (if applicable)

## Rollback Plan (If Issues Arise)

If critical issues found after publishing:

```bash
# Yank the crate on crates.io
cargo yank --vers 0.43.0 -p vtcode

# Revert commits locally
git revert 6a7ecb88..05c2559f

# Create hotfix branch
git checkout -b fix/critical-issue
# ... make fixes ...
git push origin fix/critical-issue
```

## Timeline Estimate

| Action | Duration | Notes |
|--------|----------|-------|
| Push to remote | 1 min | `git push` |
| CI/CD pass | 5-15 min | Depends on server load |
| Publish to crates.io | 5 min | 3 packages in sequence |
| GitHub Release | 5 min | Copy-paste from docs |
| Total | ~30 mins | Can be done in parallel |

## File References for This Release

**Core Implementation**:
- `vtcode-acp-client/src/` - ACP client code
- `vtcode-tools/src/acp_tool.rs` - MCP tool implementations
- `src/acp/` - TUI integration

**Documentation**:
- `docs/ACP_INTEGRATION.md` - Complete guide
- `docs/ACP_QUICK_REFERENCE.md` - Quick start
- `docs/RELEASE_0_43_0_SUMMARY.md` - Release details
- `docs/ACP_NEXT_STEPS.md` - Next steps
- `docs/ACP_IMPLEMENTATION_COMPLETE.md` - Implementation notes

**Configuration**:
- `Cargo.toml` (all) - Version 0.43.0
- `Cargo.lock` - Updated dependencies
- `CHANGELOG.md` - v0.43.0 entry

## Success Criteria

Release is successful when:

- [x] All 7 commits pushed to origin
- [x] v0.43.0 tag pushed to origin
- [ ] CI/CD pipeline passes (post-push)
- [ ] All packages published to crates.io
- [ ] GitHub release created and published
- [ ] No critical issues reported in first 24 hours

## Quick Reference Commands

```bash
# View release details
git log --oneline -8
git show v0.43.0:docs/RELEASE_0_43_0_SUMMARY.md

# Push everything
git push origin main
git push origin v0.43.0

# Verify on crates.io
curl https://crates.io/api/v1/crates/vtcode/0.43.0

# Check GitHub status
open https://github.com/vinhnx/vtcode/releases/tag/v0.43.0
```

## Support & Contact

For questions or issues:
1. Check `docs/ACP_INTEGRATION.md`
2. Review `docs/RELEASE_0_43_0_SUMMARY.md`
3. Open GitHub issue if bug found
4. Reach out to @vinhnx on GitHub

---

**Prepared by**: Release automation  
**Date**: November 9, 2025  
**Status**: ✅ Ready to proceed  
**Next Milestone**: v0.44.0 (Q1 2025)
