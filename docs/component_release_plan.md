# Extracted Crates Release Plan

This document aligns the release process for the extracted VTCode crates so the first
publishes to crates.io land smoothly and downstream consumers receive consistent
versioning, changelog, and documentation updates.

## Release scope
- `vtcode-commons`
- `vtcode-markdown-store`
- `vtcode-indexer`
- `vtcode-bash-runner`
- `vtcode-exec-events`

Each crate already re-exports back through `vtcode-core`, so the release cadence must
keep the workspace dependency graph coherent.

## Versioning strategy
1. Cut an initial **`0.1.0`** release for every extracted crate.
   - Align on the same minor version to signal that the crates are being published
     together as part of the extraction effort.
   - Use semantic versioning: breaking changes increment the minor version while
     additive changes bump the patch version during the 0.x series.
2. Tag the workspace with `vtcode-<crate>-v0.1.0` Git tags for each publish so
   downstream consumers can trace source history per crate.
3. Update `vtcode-core` dependency constraints to the released versions immediately
   after each publish to keep the workspace build reproducible.

## Changelog updates
1. Add crate-specific sections to `CHANGELOG.md` highlighting:
   - The initial release summary (new crate, primary capabilities, feature flags).
   - Notable differences versus the in-tree implementations (e.g., configurable
     storage, policy hooks, examples).
2. Include links back to the relevant documentation in `docs/` for quick reference
   (e.g., `docs/vtcode_indexer.md`, `docs/vtcode_bash_runner.md`).
3. Cross-link the changelog entries from crate-level README files once they exist
   to provide a consistent upgrade path for users discovering the crate directly on
   crates.io or docs.rs.

## Documentation refresh
1. Regenerate API docs with `cargo doc --no-deps --all-features -p <crate>` prior to
   publishing. Manually inspect rendered docs for broken intra-doc links or missing
   examples.
2. Ensure each crate README references the component extraction roadmap and the new
   release plan so contributors can follow future milestones.
3. Update `docs/component_extraction_plan.md` and `docs/component_extraction_todo.md`
   to reflect the completed milestones and include pointers to the release plan.

## Publication checklist
1. Run the full validation suite in CI and locally:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo nextest run --workspace`
2. Bump crate versions in their respective `Cargo.toml` files and run `cargo check`
   to confirm lockfile updates compile.
3. Use `cargo publish --dry-run -p <crate>` for each crate to catch manifest or
   packaging issues (missing files, license metadata).
   - `vtcode-bash-runner` depends on `vtcode-commons`, so its dry run must be
     re-executed after the shared traits crate is published to crates.io.
4. Publish crates sequentially, starting with shared dependencies (`vtcode-commons`,
   `vtcode-markdown-store`) followed by dependents (`vtcode-indexer`,
   `vtcode-bash-runner`, `vtcode-exec-events`).
5. After each publish, push the git tags and open a PR updating the workspace to the
   released versions (including regenerated lockfiles and changelog entries).

## Sequential publish schedule

The release window will follow a tightly ordered sequence so dependency updates and
documentation refreshes land in predictable batches. Each step assumes the previous
publish has fully propagated on crates.io (typically a few minutes) before moving on.

### Automation helper

The `scripts/publish_extracted_crates.sh` helper mirrors the sequence below. It
provides optional dry-run coverage (`--dry-run` flag or `VT_RELEASE_DRY_RUN=1`)
and can resume from any crate via `--start-from <crate>`. Invoke the script to
run the fmt/clippy/test validation suite, execute the publish command for each
crate, tag the release, and prompt for the dependency bump follow-up. Use it
during rehearsals and the live release window to keep the process consistent.

1. **`vtcode-commons`**
   - Commands: `cargo publish -p vtcode-commons`, then `git tag vtcode-commons-v0.1.0`.
   - Follow-up: regenerate `Cargo.lock`, update workspace manifests to the published
     version, and open a tracking PR with the changelog excerpt already prepared.
2. **`vtcode-markdown-store`**
   - Commands: `cargo publish -p vtcode-markdown-store`, tag `vtcode-markdown-store-v0.1.0`.
   - Follow-up: bump the dependency in `vtcode-core` and rerun `cargo doc --no-deps` for
     the crate before pushing the tracking PR updates.
3. **`vtcode-indexer`**
   - Commands: `cargo publish -p vtcode-indexer`, tag `vtcode-indexer-v0.1.0`.
   - Follow-up: refresh the lockfile, update docs.rs links in README snippets if needed,
     and merge the dependency bump PR.
4. **`vtcode-bash-runner`**
   - Prerequisite: rerun `cargo publish --dry-run -p vtcode-bash-runner` now that
     `vtcode-commons` is live to confirm the published dependency graph matches crates.io.
   - Commands: `cargo publish -p vtcode-bash-runner`, tag `vtcode-bash-runner-v0.1.0`.
   - Follow-up: regenerate the lockfile and ensure the `dry_run` example stays in sync
     with the newly published dependency versions before merging the tracking PR.
5. **`vtcode-exec-events`**
   - Commands: `cargo publish -p vtcode-exec-events`, tag `vtcode-exec-events-v0.1.0`.
   - Follow-up: update the workspace dependency, confirm the example binaries still run,
     and close out the changelog section by linking to the published crate.

After the final publish, push all tags, merge the accumulated dependency bump PRs, and
announce the release plan completion in the project README and communication channels.

## Post-release follow-up
- Monitor crates.io download metrics and GitHub issues for early adopter feedback.
- Schedule a follow-up milestone to evaluate extracting additional crates (`vtcode-llm`,
  `vtcode-tools`, `vtcode-config`) once the initial batch stabilizes.
- Keep the release plan updated as workflows evolve (e.g., adding automation via
  `cargo release` or GitHub Actions publishing steps).
