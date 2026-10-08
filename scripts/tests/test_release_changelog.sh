#!/usr/bin/env bash
# Fixture-only changelog checks; never invoke release publication.
set -euo pipefail

repo_scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_dir=$(mktemp -d)
trap 'rm -rf "$fixture_dir"' EXIT
checks=0

assert_equal() {
	local actual=$1 expected=$2 label=$3
	if [[ "$actual" != "$expected" ]]; then
		printf 'FAIL: %s\nExpected:\n%s\nActual:\n%s\n' "$label" "$expected" "$actual" >&2
		exit 1
	fi
	checks=$((checks + 1))
}

forbidden_publication() {
	printf 'unexpected publication command\n' >>"$fixture_dir/publication-calls"
	return 99
}
cargo() { forbidden_publication; }
gh() { forbidden_publication; }
curl() { forbidden_publication; }
export -f cargo gh curl forbidden_publication
export fixture_dir

original_trap=$(trap -p EXIT)
# shellcheck disable=SC1091
source "$repo_scripts/release-changelog.sh"
assert_equal "$(trap -p EXIT)" "$original_trap" "library keeps caller cleanup trap"
assert_equal "$(parse_commit_type 'feat(core): new behavior')" feat "scoped conventional commit"
assert_equal "$(parse_commit_type 'fix: repair')" fix "unscoped conventional commit"
assert_equal "$(parse_commit_type 'Unstructured subject')" other "unstructured commit"
assert_equal "$(clean_commit_message 'feat(core):  keep [skip ci]')" 'keep [skip ci]' "canonical CI marker contract"
assert_equal "$(get_github_username '123+alice@users.noreply.github.com')" alice "GitHub ID email"
assert_equal "$(get_github_username 'noreply@vtcode.com')" vtcode-release-bot "release bot identity"
assert_equal "$(get_github_username 'vinhnguyen.fixture@example.test')" vinhnx "maintainer alias"

for subject in 'chore(release): v1.2.3' 'UPDATE TODOs' 'docs(todo): owner notes' 'Build: update version number'; do
	if ! release_commit_is_excluded "$subject"; then
		printf 'FAIL: expected excluded subject: %s\n' "$subject" >&2
		exit 1
	fi
	checks=$((checks + 1))
done
for subject in 'feat: new behavior' 'fix: preserve version parsing' 'docs: user workflow'; do
	if release_commit_is_excluded "$subject"; then
		printf 'FAIL: unexpectedly excluded subject: %s\n' "$subject" >&2
		exit 1
	fi
	checks=$((checks + 1))
done

mkdir "$fixture_dir/repository"
cd "$fixture_dir/repository"
command git init --quiet
command git config core.abbrev 7
command git config core.hooksPath /dev/null
command git config commit.gpgsign false
command git config user.name 'Fixture Author'
command git config user.email 'fixture@example.test'

commit_fixture() {
	command git -c "user.email=$2" commit --quiet --allow-empty -m "$1"
}

commit_fixture 'feat(core): oldest feature' '11+oldest@users.noreply.github.com'
oldest_hash=$(command git rev-parse --short=7 HEAD)
expected=$(
	cat <<EOF
### Highlights

#### Features

- oldest feature ($oldest_hash) (@oldest)

### Contributors

@oldest
EOF
)
assert_equal "$(generate_structured_changelog HEAD)" "$expected" "single commit without trailing git-log newline"
assert_equal "$(add_username_tags "- oldest feature ($oldest_hash)" HEAD)" \
	"- oldest feature ($oldest_hash) (@oldest)" "oldest author mapping without trailing newline"

commit_fixture 'fix(io): second fix' 'vinhnguyen.fixture@example.test'
fix_hash=$(command git rev-parse --short=7 HEAD)
commit_fixture 'docs: third guide' 'noreply@vtcode.com'
docs_hash=$(command git rev-parse --short=7 HEAD)
commit_fixture 'perf: fourth optimization' '11+oldest@users.noreply.github.com'
perf_hash=$(command git rev-parse --short=7 HEAD)
commit_fixture 'docs(todo): owner bookkeeping' 'noreply@vtcode.com'
commit_fixture 'unstructured sixth change' '12+newest@users.noreply.github.com'
other_hash=$(command git rev-parse --short=7 HEAD)
commit_fixture 'feat(tui): newest feature' 'vinhnguyen.fixture@example.test'
newest_hash=$(command git rev-parse --short=7 HEAD)
original_head=$(command git rev-parse HEAD)
expected=$(
	cat <<EOF
### Highlights

#### Features

- newest feature ($newest_hash) (@vinhnx)
- oldest feature ($oldest_hash) (@oldest)

#### Bug Fixes

- second fix ($fix_hash) (@vinhnx)

#### Documentation

- third guide ($docs_hash)

### Other Changes

#### Performance

- fourth optimization ($perf_hash) (@oldest)

#### Other

- unstructured sixth change ($other_hash) (@newest)

### Contributors

@vinhnx, @newest, @oldest
EOF
)
assert_equal "$(generate_structured_changelog HEAD)" "$expected" "group order, history order, aliases and deduplication"
assert_equal "$(generate_structured_changelog HEAD..HEAD)" '*No significant changes*' "empty range"

export SCRIPT_DIR=$repo_scripts
legacy_contract=$(
	# shellcheck disable=SC1091
	source "$repo_scripts/release-lib.sh"
	printf '%s\n' "$(parse_commit_type 'fix(io): repair')" \
		"$(get_github_username '12+newest@users.noreply.github.com')" \
		"$(clean_commit_message 'fix: preserve legacy [skip ci]')" \
		"$(get_type_title feat)"
)
assert_equal "$legacy_contract" $'fix\n12+newest\npreserve legacy\nNew Features' "legacy adapter keeps distinct contracts"
legacy_notes=$(
	# shellcheck disable=SC1091
	source "$repo_scripts/release-lib.sh"
	generate_structured_changelog HEAD
)
if [[ "$legacy_notes" != *'### New Features'* || "$legacy_notes" != *"oldest feature ($oldest_hash)"* ||
	"$legacy_notes" == *'owner bookkeeping'* || "$legacy_notes" == *'### Highlights'* ]]; then
	printf 'FAIL: legacy formatter wiring or exclusion policy\n%s\n' "$legacy_notes" >&2
	exit 1
fi
checks=$((checks + 1))

mkdir "$fixture_dir/changelogs"
entry_one=$'## 2.0.0 - 2026-10-05\n\nSecond version: literal 100% and ${HOME} $(touch injection-marker).\n'
entry_two=$'## 3.0.0 - 2026-10-06\n\nThird version: distinct text.\n'
(
	cd "$fixture_dir/changelogs"
	: >CHANGELOG.md
	insert_changelog_entry "$entry_one"
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "${entry_one%$'\n'}" "empty changelog insertion"
printf '# Changelog\n\nIntro.\n' >"$fixture_dir/changelogs/CHANGELOG.md"
(
	cd "$fixture_dir/changelogs"
	insert_changelog_entry "$entry_one"
)
expected=$(
	cat <<EOF
# Changelog

Intro.

$entry_one
EOF
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "$expected" "header-only changelog insertion"
cat >"$fixture_dir/changelogs/CHANGELOG.md" <<'EOF'
# Changelog

Intro.

## 1.0.0 - 2026-10-04

Original version body.

### Details

Nested heading and original content remain together.
EOF
(
	cd "$fixture_dir/changelogs"
	insert_changelog_entry "$entry_one"
	insert_changelog_entry "$entry_two"
)
expected=$(
	cat <<EOF
# Changelog

Intro.

$entry_two
$entry_one
## 1.0.0 - 2026-10-04

Original version body.

### Details

Nested heading and original content remain together.
EOF
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "$expected" "repeated insertion preserves newest-first versions and original body"
if [[ -e "$fixture_dir/changelogs/injection-marker" ]]; then
	printf 'FAIL: inserted text was evaluated as shell code\n' >&2
	exit 1
fi
checks=$((checks + 1))
(
	cd "$fixture_dir/changelogs"
	# shellcheck disable=SC1091
	source "$repo_scripts/release-lib.sh"
	printf '## 1.0.0\n\nLegacy body.\n' >CHANGELOG.md
	insert_changelog_entry "$entry_one"
)
expected=$(
	cat <<EOF
$entry_one
## 1.0.0

Legacy body.
EOF
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "$expected" "legacy caller uses the shared insertion helper"

# remove_changelog_version_section: middle removal keeps neighbors byte-identical.
cat >"$fixture_dir/changelogs/CHANGELOG.md" <<'EOF'
# Changelog

Intro.

## 2.0.0 - 2026-10-05

Second body.

## 1.9.9 - 2026-10-04

Middle body.

## 1.9.0 - 2026-10-03

First body.
EOF
(
	cd "$fixture_dir/changelogs"
	remove_changelog_version_section "1.9.9"
)
expected=$(
	cat <<EOF
# Changelog

Intro.

## 2.0.0 - 2026-10-05

Second body.

## 1.9.0 - 2026-10-03

First body.
EOF
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "$expected" "middle version removal preserves neighbors"
# Independent oracle: no trace of the removed section, both neighbors present.
if grep -q '1\.9\.9' "$fixture_dir/changelogs/CHANGELOG.md"; then
	printf 'FAIL: removed version still present\n' >&2
	exit 1
fi
checks=$((checks + 1))
if [[ "$(grep -c '^## ' "$fixture_dir/changelogs/CHANGELOG.md")" != '2' ]]; then
	printf 'FAIL: expected 2 version headings after removal\n' >&2
	exit 1
fi
checks=$((checks + 1))

# Asymmetric prefix collision: removing 9.9 must not touch 9.9.9 (and vice versa).
cat >"$fixture_dir/changelogs/CHANGELOG.md" <<'EOF'
# Changelog

## 9.9.9 - 2026-10-05

Longer version body.

## 9.9 - 2026-10-04

Shorter version body.
EOF
(
	cd "$fixture_dir/changelogs"
	remove_changelog_version_section "9.9"
)
if grep -q 'Shorter version body' "$fixture_dir/changelogs/CHANGELOG.md"; then
	printf 'FAIL: short-version removal left its body behind\n' >&2
	exit 1
fi
checks=$((checks + 1))
if ! grep -q 'Longer version body' "$fixture_dir/changelogs/CHANGELOG.md"; then
	printf 'FAIL: prefix collision removed 9.9.9 when removing 9.9\n' >&2
	exit 1
fi
checks=$((checks + 1))
(
	cd "$fixture_dir/changelogs"
	remove_changelog_version_section "9.9.9"
)
if grep -q '^## ' "$fixture_dir/changelogs/CHANGELOG.md"; then
	printf 'FAIL: removing the last remaining version left a heading\n' >&2
	exit 1
fi
checks=$((checks + 1))

# No-op boundary: absent version leaves the file byte-identical.
printf '# Changelog\n\n## 1.0.0 - 2026-10-04\n\nBody.\n' >"$fixture_dir/changelogs/CHANGELOG.md"
before=$(cat "$fixture_dir/changelogs/CHANGELOG.md")
(
	cd "$fixture_dir/changelogs"
	remove_changelog_version_section "2.0.0"
)
assert_equal "$(cat "$fixture_dir/changelogs/CHANGELOG.md")" "$before" "absent version removal is a no-op"

# upload_release_assets_parallel: orchestration with stubbed per-file upload
# (stubs the retry wrapper so failure cases run without backoff sleeps).
# shellcheck disable=SC1091
source "$repo_scripts/release-assets.sh"
: >"$fixture_dir/upload-calls"
upload_release_asset_with_retry() {
	printf '%s\n' "$(basename "$2")" >>"$fixture_dir/upload-calls"
	case "$2" in
	*bad-*) return 1 ;;
	*) return 0 ;;
	esac
}
mkdir -p "$fixture_dir/uploads"
: >"$fixture_dir/uploads/ok-a.bin"
: >"$fixture_dir/uploads/ok-b.bin"
: >"$fixture_dir/uploads/bad-c.bin"
if ! upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/ok-a.bin" "$fixture_dir/uploads/ok-b.bin"; then
	printf 'FAIL: parallel upload failed on healthy assets\n' >&2
	exit 1
fi
checks=$((checks + 1))
# Independent oracle: every asset was attempted (order-insensitive).
if [[ "$(sort "$fixture_dir/upload-calls" | tr '\n' ' ')" != 'ok-a.bin ok-b.bin ' ]]; then
	printf 'FAIL: parallel upload did not attempt every asset\n' >&2
	exit 1
fi
checks=$((checks + 1))
: >"$fixture_dir/upload-calls"
if upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/ok-a.bin" "$fixture_dir/uploads/bad-c.bin"; then
	printf 'FAIL: parallel upload hid a failed asset\n' >&2
	exit 1
fi
checks=$((checks + 1))
if [[ "$(sort "$fixture_dir/upload-calls" | tr '\n' ' ')" != 'bad-c.bin ok-a.bin ' ]]; then
	printf 'FAIL: failed asset was not attempted alongside healthy ones\n' >&2
	exit 1
fi
checks=$((checks + 1))
# Invalid UPLOAD_PARALLEL_JOBS falls back to the default instead of stalling.
if ! UPLOAD_PARALLEL_JOBS=bogus upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/ok-a.bin"; then
	printf 'FAIL: invalid UPLOAD_PARALLEL_JOBS broke the upload\n' >&2
	exit 1
fi
checks=$((checks + 1))

# docs.rs metadata guard: --cfg docsrs must only reach rustdoc, never rustc
# (RUSTFLAGS --cfg docsrs breaks generic-array 0.14.7 on nightly: E0557
# `feature(doc_auto_cfg)` removed in 1.92; see vtcode 0.174.0 docs build).
repo_root="$(cd "$repo_scripts/.." && pwd)"
manifest_count=0
while IFS= read -r manifest; do
	manifest_count=$((manifest_count + 1))
	if grep -q 'rustc-args' "$manifest"; then
		printf 'FAIL: %s sets docs.rs rustc-args (breaks nightly dep builds)\n' "$manifest" >&2
		exit 1
	fi
	checks=$((checks + 1))
done < <(find "$repo_root" -type d \( -name target -o -name .git \) -prune -o -type f -name 'Cargo.toml' -exec grep -l 'metadata.docs.rs' {} +)
if [[ "$manifest_count" -lt 2 ]]; then
	printf 'FAIL: docs.rs guard scanned only %s manifests (vacuous pass)\n' "$manifest_count" >&2
	exit 1
fi
checks=$((checks + 1))

# Throttle path: UPLOAD_PARALLEL_JOBS=1 serializes but still attempts every asset.
: >"$fixture_dir/upload-calls"
if ! UPLOAD_PARALLEL_JOBS=1 upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/ok-a.bin" "$fixture_dir/uploads/ok-b.bin"; then
	printf 'FAIL: throttled parallel upload failed on healthy assets\n' >&2
	exit 1
fi
checks=$((checks + 1))
if [[ "$(sort "$fixture_dir/upload-calls" | tr '\n' ' ')" != 'ok-a.bin ok-b.bin ' ]]; then
	printf 'FAIL: throttled upload did not attempt every asset\n' >&2
	exit 1
fi
checks=$((checks + 1))

# Filenames with spaces are attempted intact.
: >"$fixture_dir/uploads/sp ace.bin"
: >"$fixture_dir/upload-calls"
if ! upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/sp ace.bin"; then
	printf 'FAIL: parallel upload failed on a spaced filename\n' >&2
	exit 1
fi
checks=$((checks + 1))
if [[ "$(cat "$fixture_dir/upload-calls")" != 'sp ace.bin' ]]; then
	printf 'FAIL: spaced filename was mangled in transit\n' >&2
	exit 1
fi
checks=$((checks + 1))

# A job killed before writing its status file must fail the release, never
# pass silently. Simulate by overriding the single-asset helper for one file.
_upload_single_asset() {
	case "$2" in
	*ghost-*) return 0 ;;
	*)
		if upload_release_asset_with_retry "$1" "$2"; then
			echo "0" >"$3"
		else
			echo "1:$2" >"$3"
		fi
		;;
	esac
}
: >"$fixture_dir/uploads/ghost-d.bin"
if upload_release_assets_parallel "9.9.9-test" "$fixture_dir/uploads/ok-a.bin" "$fixture_dir/uploads/ghost-d.bin"; then
	printf 'FAIL: missing status file reported success\n' >&2
	exit 1
fi
checks=$((checks + 1))
# Restore the real helpers (re-source is side-effect free: definitions only).
# shellcheck disable=SC1091
source "$repo_scripts/release-assets.sh"

# No-op boundary: missing CHANGELOG.md creates nothing.
mkdir -p "$fixture_dir/no-changelog"
(
	cd "$fixture_dir/no-changelog"
	remove_changelog_version_section "9.9.9"
)
if [[ -e "$fixture_dir/no-changelog/CHANGELOG.md" ]]; then
	printf 'FAIL: removal created a CHANGELOG.md that did not exist\n' >&2
	exit 1
fi
checks=$((checks + 1))

# --help exits in argument parsing, before build/version/tag/upload operations.
bash "$repo_scripts/release.sh" --help >"$fixture_dir/help.txt"
if ! grep -q '^Usage: ./scripts/release.sh' "$fixture_dir/help.txt"; then
	printf 'FAIL: entrypoint help wiring\n' >&2
	exit 1
fi
checks=$((checks + 1))
assert_equal "$(command git rev-parse HEAD)" "$original_head" "formatter leaves HEAD unchanged"
assert_equal "$(command git status --porcelain)" '' "formatter leaves worktree unchanged"
assert_equal "$(command git tag -l)" '' "formatter creates no tags"
if [[ -e "$fixture_dir/publication-calls" ]]; then
	printf 'FAIL: publication was attempted\n' >&2
	exit 1
fi
checks=$((checks + 1))
printf 'PASS: %s changelog checks\n' "$checks"
