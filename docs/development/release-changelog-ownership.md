# Release changelog ownership

| Owner | Responsibility | Contract |
| --- | --- | --- |
| `scripts/release.sh` | Release arguments, versioning, changelog file updates, packaging, tagging, and publication | Sources the canonical formatter; publication stays entrypoint-owned |
| `scripts/release-changelog.sh` | Canonical username mapping, author tags, grouped release notes, and contributors | Sourceable without release work; reads the supplied Git range and formats output |
| `scripts/release-changelog-common.sh` | Conventional commit type parsing, exclusion predicate, changelog entry insertion, and version-section removal | Shared by canonical and legacy adapters; sourcing only defines functions, while explicit insertion updates the caller's `CHANGELOG.md` |
| `scripts/release-lib.sh` | Existing legacy formatting and release helpers | Shares classification while retaining its different titles, author mapping, and CI-marker cleanup |

Canonical notes retain Highlights/Other Changes grouping, newest-first order
within each category, contributor deduplication, bot handling, and existing
subject exclusions. Legacy notes keep their established output contract.
The two adapters intentionally retain separate formatting implementations.

The canonical Git readers process the final record even when
`git log --pretty=format` omits its trailing newline. This fixes single-commit
ranges appearing empty and oldest commit/contributor/author-mapping omissions.

The formatter library performs no work when sourced beyond loading function
definitions. Formatting reads Git history; the author-tag helper uses a temporary
mapping file and cleans it up. Both release owners call the same insertion helper
for newest-first artifact updates. Empty, header-only, and headerless files are
supported; a version on line 1 skips the empty prefix instead of calling macOS
`head` with a zero line count. Entries are written as literal text. Commits, tags,
uploads, package publishing, and Homebrew operations stay with the release owners.

```sh
bash scripts/tests/test_release_changelog.sh
bash -n scripts/release.sh scripts/release-lib.sh scripts/release-changelog.sh scripts/release-changelog-common.sh
shellcheck scripts/release-changelog.sh scripts/release-changelog-common.sh scripts/tests/test_release_changelog.sh
```

The fixture suite creates its own temporary Git repository. It checks complete
canonical output, category/history ordering, single-commit EOF, author aliases,
contributors, excluded subjects, empty ranges, distinct legacy contracts, and
entrypoint help wiring. Publication commands are instrumented to fail. The suite
also confirms that formatting leaves HEAD, worktree, and tags unchanged.
Insertion fixtures verify repeated version order, preservation of old bodies
and nested headings, literal shell-like text, and both callers' shared helper.
Removal fixtures verify middle/first/last extraction, exact-match safety on
version prefixes (9.9 vs 9.9.9), and no-op behavior for absent versions and
missing files, so re-runs regenerate stale entries instead of skipping.
Upload fixtures stub the per-file retry wrapper to assert parallel fan-out
(including throttled and spaced filenames), failure propagation, and
`UPLOAD_PARALLEL_JOBS` fallback. A metadata guard asserts no workspace
manifest sets docs.rs `rustc-args`.
It does not run the release entrypoint's dry-run orchestration or publication.
