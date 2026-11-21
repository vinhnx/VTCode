#!/bin/bash
# Exercise the tracked Git hooks against a disposable index.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
git -C "$fixture" init -q
cd "$fixture"

mkdir -p scripts/hooks 'src/with spaces'
cp "$repo_root/scripts/hooks/pre-commit" "$repo_root/scripts/hooks/pre-push" scripts/hooks/
cp "$repo_root/scripts/install-hooks.sh" scripts/
printf 'run_repo_governance_gates() { :; }\n' > scripts/common.sh

git config --local core.hooksPath outside-hooks
if bash scripts/install-hooks.sh > install.log 2>&1; then
	echo "installer unexpectedly replaced an existing hook path" >&2
	exit 1
fi
test "$(git config --get core.hooksPath)" = outside-hooks
git config --local --unset core.hooksPath
bash scripts/install-hooks.sh
bash scripts/install-hooks.sh
test "$(git config --get core.hooksPath)" = scripts/hooks

printf 'max_width = 35\n' > rustfmt.toml
printf 'max_width = 120\n' > 'src/with spaces/rustfmt.toml'
printf 'mod child;\n\nfn example() {\n    eprintln!("a deliberately long line that fits the staged configuration");\n}\n' > 'src/with spaces/mod.rs'
printf 'fn child() {}\n' > 'src/with spaces/child.rs'
git add rustfmt.toml 'src/with spaces/child.rs' 'src/with spaces/rustfmt.toml'
git -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath=/dev/null commit -qm baseline
# Only the parent is staged as a change; its child already exists in HEAD.
printf 'fn numbered() {}\n' > '0:root.rs'
git add 'src/with spaces/mod.rs' '0:root.rs'

printf 'max_width = 35\n' > 'src/with spaces/rustfmt.toml'
printf 'this is an unstaged edit, not Rust\n' > 'src/with spaces/mod.rs'
bash scripts/hooks/pre-commit

printf 'fn  unformatted(){}\n' > 'src/with spaces/mod.rs'
git add 'src/with spaces/mod.rs'
printf 'fn unformatted() {}\n' > 'src/with spaces/mod.rs'
if bash scripts/hooks/pre-commit > format.log 2>&1; then
	echo "hook accepted the unformatted staged blob" >&2
	exit 1
fi
grep -q 'staged formatting check failed' format.log
echo "Repository hook tests passed."
