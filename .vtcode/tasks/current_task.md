# Run vtcode-clippy and fix lint issues

- [x] Run cargo clippy workspace
  outcome: cargo clippy --workspace --all-targets --all-features -- -D warnings (fails in vtcode-llm)
- [ ] Fix reported issues
- [ ] Re-run clippy to confirm clean
