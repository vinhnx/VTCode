#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
INSTALL_SH="${ROOT_DIR}/scripts/install.sh"

# Usage text is platform-independent.
# NOTE: plain grep (no -q) consumes all input, avoiding SIGPIPE failures under pipefail.
if ! bash "${INSTALL_SH}" --help | grep -F -- "--with-desktop" >/dev/null; then
    printf 'FAIL: install.sh --help does not document --with-desktop.\n' >&2
    exit 1
fi
printf 'PASS: install.sh --help documents --with-desktop.\n'
if ! bash "${INSTALL_SH}" --help | grep -F -- "--without-desktop" >/dev/null; then
    printf 'FAIL: install.sh --help does not document --without-desktop.\n' >&2
    exit 1
fi
printf 'PASS: install.sh --help documents --without-desktop.\n'
if missing_dir_output="$(bash "${INSTALL_SH}" -d 2>&1)"; then
    printf 'FAIL: -d without a directory should fail.\n' >&2
    exit 1
fi
if ! printf '%s' "${missing_dir_output}" | grep -F -- '-d/--dir requires a directory' >/dev/null; then
    printf 'FAIL: bare -d should name the missing directory argument.\n' >&2
    exit 1
fi
printf 'PASS: bare -d fails with a usage error.\n'

if [[ "$(uname -s)" != "Darwin" ]]; then
    printf 'SKIP: desktop install flow requires Darwin.\n'
    exit 0
fi

TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-install-desktop-test.XXXXXX")"
trap 'rm -rf -- "${TEST_ROOT}"' EXIT

# Sandbox HOME and INSTALL_DIR before sourcing (install.sh creates INSTALL_DIR).
export HOME="${TEST_ROOT}/home"
export INSTALL_DIR="${TEST_ROOT}/bin"
mkdir -p "${HOME}"

# Source helpers without running main (guarded by the BASH_SOURCE check).
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
# Sourcing overwrites the cleanup trap above; reinstall it.
trap 'rm -rf -- "${TEST_ROOT}"; show_cursor' EXIT

STUB_BIN="${TEST_ROOT}/stub-bin"
mkdir -p "${STUB_BIN}"
export VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG="${TEST_ROOT}/git.log"

# Stubs use /bin/bash (this test is Darwin-gated): the restricted-PATH subtest
# below hides every PATH-resolved interpreter from `env`.
cat >"${STUB_BIN}/sw_vers" <<'STUB'
#!/bin/bash
printf '15.1\n'
STUB

cat >"${STUB_BIN}/uname" <<'STUB'
#!/bin/bash
printf 'Darwin\n'
STUB

cat >"${STUB_BIN}/cargo" <<'STUB'
#!/bin/bash
exit 0
STUB

# Stub git: record the clone invocation, then fake a source checkout whose
# packaging script builds a skeleton app bundle at its output argument.
cat >"${STUB_BIN}/git" <<'STUB'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >>"${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}"
dest="${!#}"
mkdir -p "${dest}/scripts"
cat >"${dest}/scripts/package-desktop-macos.sh" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$1/Contents/MacOS"
printf 'stub desktop\n' >"$1/Contents/MacOS/vtcode-desktop"
: >"$1/Contents/Info.plist"
printf 'Packaged locally signed app: %s\n' "$1"
SCRIPT
chmod +x "${dest}/scripts/package-desktop-macos.sh"
STUB
chmod +x "${STUB_BIN}/sw_vers" "${STUB_BIN}/uname" "${STUB_BIN}/cargo" "${STUB_BIN}/git"

# curl test double: serves release-asset fixtures from
# $VTCODE_INSTALL_DESKTOP_TEST_FIXTURES keyed by URL basename. HEAD (-I)
# checks succeed iff the fixture exists; downloads (-o <file>) copy the
# fixture; anything else fails. Unset fixtures dir => everything fails, which
# keeps the no-asset tests hermetic (no real network).
STUB_CURL="${TEST_ROOT}/stub-curl"
mkdir -p "${STUB_CURL}"
cat >"${STUB_CURL}/curl" <<'STUB'
#!/bin/bash
set -u
log="${VTCODE_INSTALL_DESKTOP_TEST_CURL_LOG:-}"
if [[ -n "$log" ]]; then
    printf '%s\n' "$*" >>"$log"
fi
fixtures="${VTCODE_INSTALL_DESKTOP_TEST_FIXTURES:-}"
url="${!#}"
is_head=0
args=("$@")
for ((i = 0; i < $# - 1; i++)); do
    case "${args[$i]}" in
    -*I*) is_head=1 ;;
    esac
done
outfile=""
prev=""
for arg in "$@"; do
    if [[ "$prev" == "-o" ]]; then
        outfile="$arg"
    fi
    prev="$arg"
done
base="$(basename "$url")"
if ((is_head)); then
    [[ -n "$fixtures" && -f "$fixtures/$base" ]]
    exit $?
fi
if [[ -n "$outfile" && -n "$fixtures" && -f "$fixtures/$base" ]]; then
    cp "$fixtures/$base" "$outfile"
    exit 0
fi
exit 1
STUB
chmod +x "${STUB_CURL}/curl"
export PATH="${STUB_CURL}:${STUB_BIN}:${PATH}"

# uname stubs that answer -m truthfully (the main stub always says Darwin).
STUB_ARM="${TEST_ROOT}/stub-arm"
STUB_X64="${TEST_ROOT}/stub-x64"
mkdir -p "$STUB_ARM" "$STUB_X64"
cat >"${STUB_ARM}/uname" <<'STUB'
#!/bin/bash
if [[ "${1:-}" == "-m" ]]; then printf 'arm64\n'; else printf 'Darwin\n'; fi
STUB
cat >"${STUB_X64}/uname" <<'STUB'
#!/bin/bash
if [[ "${1:-}" == "-m" ]]; then printf 'x86_64\n'; else printf 'Darwin\n'; fi
STUB
chmod +x "${STUB_ARM}/uname" "${STUB_X64}/uname"

# Minimal tool set for the no-toolchain test: uname + sw_vers only. Combined
# with /usr/bin:/bin this resolves ditto/shasum but NOT cargo.
STUB_MIN="${TEST_ROOT}/stub-min"
mkdir -p "$STUB_MIN"
cp "${STUB_X64}/uname" "${STUB_MIN}/uname"
printf '#!/bin/bash\nprintf "15.1\\n"\n' >"${STUB_MIN}/sw_vers"
chmod +x "${STUB_MIN}/uname" "${STUB_MIN}/sw_vers"

# Build a fixture desktop zip with ditto (mirroring the release layouts):
# $1 dest dir, $2 asset name, $3 bundle marker, $4 layout (adhoc|notarized|junk).
# Appends the zip's real checksum to <dest>/checksums.txt.
build_desktop_zip() {
    local dest="$1" asset="$2" marker="$3" layout="$4"
    local stage
    stage="$(mktemp -d "${TEST_ROOT}/stage.XXXXXX")"
    if [[ "$layout" == "junk" ]]; then
        printf 'junk\n' >"$stage/junk.txt"
        (cd "$stage" && ditto -c -k junk.txt "$dest/$asset")
    else
        mkdir -p "$stage/VT Code.app/Contents/MacOS"
        printf '%s\n' "$marker" >"$stage/VT Code.app/Contents/MacOS/vtcode-desktop"
        : >"$stage/VT Code.app/Contents/Info.plist"
        if [[ "$layout" == "adhoc" ]]; then
            printf 'first launch note\n' >"$stage/FIRST-LAUNCH.txt"
            ditto -c -k "$stage" "$dest/$asset"
        else
            (cd "$stage" && ditto -c -k --keepParent "VT Code.app" "$dest/$asset")
        fi
    fi
    rm -rf "$stage"
    (cd "$dest" && shasum -a 256 "$asset" >>checksums.txt)
}

FIX_VERSION="9.9.9-test"
FIX_X64_ASSET="vtcode-desktop-${FIX_VERSION}-x86_64-apple-darwin.zip"
FIX_ARM_ASSET="vtcode-desktop-${FIX_VERSION}-aarch64-apple-darwin.zip"

FIX_MAIN="${TEST_ROOT}/fix-main"
mkdir -p "$FIX_MAIN"
: >"$FIX_MAIN/checksums.txt"
build_desktop_zip "$FIX_MAIN" "$FIX_X64_ASSET" "asset desktop x64" "adhoc"
build_desktop_zip "$FIX_MAIN" "$FIX_ARM_ASSET" "asset desktop arm" "notarized"

FIX_X64ONLY="${TEST_ROOT}/fix-x64only"
mkdir -p "$FIX_X64ONLY"
: >"$FIX_X64ONLY/checksums.txt"
build_desktop_zip "$FIX_X64ONLY" "$FIX_X64_ASSET" "asset desktop x64" "adhoc"

# Sidecar-only checksums (no aggregate manifest).
FIX_SIDECAR="${TEST_ROOT}/fix-sidecar"
mkdir -p "$FIX_SIDECAR"
: >"$FIX_SIDECAR/checksums.txt"
build_desktop_zip "$FIX_SIDECAR" "$FIX_X64_ASSET" "asset desktop x64" "adhoc"
rm "$FIX_SIDECAR/checksums.txt"
(cd "$FIX_SIDECAR" && shasum -a 256 "$FIX_X64_ASSET" >"${FIX_X64_ASSET}.sha256")

# Correct zip, wrong checksum.
FIX_TAMPERED="${TEST_ROOT}/fix-tampered"
mkdir -p "$FIX_TAMPERED"
: >"$FIX_TAMPERED/checksums.txt"
build_desktop_zip "$FIX_TAMPERED" "$FIX_X64_ASSET" "asset desktop x64" "adhoc"
printf '%s  %s\n' '0000000000000000000000000000000000000000000000000000000000000000' \
    "$FIX_X64_ASSET" >"$FIX_TAMPERED/checksums.txt"

# Correct zip, no checksum metadata anywhere: strict mode must refuse it.
FIX_NOCHECKSUM="${TEST_ROOT}/fix-nochecksum"
mkdir -p "$FIX_NOCHECKSUM"
: >"$FIX_NOCHECKSUM/checksums.txt"
build_desktop_zip "$FIX_NOCHECKSUM" "$FIX_X64_ASSET" "asset desktop x64" "adhoc"
rm "$FIX_NOCHECKSUM/checksums.txt"

# Valid checksum, but the zip holds no app bundle.
FIX_JUNK="${TEST_ROOT}/fix-junk"
mkdir -p "$FIX_JUNK"
: >"$FIX_JUNK/checksums.txt"
build_desktop_zip "$FIX_JUNK" "$FIX_X64_ASSET" "junk" "junk"

FIX_EMPTY="${TEST_ROOT}/fix-empty"
mkdir -p "$FIX_EMPTY"
export VTCODE_INSTALL_DESKTOP_TEST_CURL_LOG="${TEST_ROOT}/curl.log"

STUB_LINUX="${TEST_ROOT}/stub-linux"
mkdir -p "${STUB_LINUX}"
printf '#!/bin/bash\nprintf "Linux\\n"\n' >"${STUB_LINUX}/uname"
chmod +x "${STUB_LINUX}/uname"

if ! check_desktop_prerequisites >/dev/null 2>&1; then
    printf 'FAIL: prerequisites should pass on macOS 15+ with stubs on PATH.\n' >&2
    exit 1
fi
printf 'PASS: desktop prerequisites pass when all tools are present.\n'

# Stub-only PATH hides the real macOS packaging tools, so the check must fail
# and name a missing tool.
missing_output="$(PATH="${STUB_BIN}" check_desktop_prerequisites 2>&1)" && {
    printf 'FAIL: prerequisites should fail when packaging tools are missing.\n' >&2
    exit 1
}
if ! printf '%s' "${missing_output}" | grep -F 'codesign' >/dev/null; then
    printf 'FAIL: prerequisite error should name the missing tool.\n' >&2
    exit 1
fi
printf 'PASS: desktop prerequisites fail closed and name missing tools.\n'

# Other side of the OS-version boundary: macOS 14 must be rejected.
STUB_OLD_OS="${TEST_ROOT}/stub-old-os"
mkdir -p "${STUB_OLD_OS}"
printf '#!/bin/bash\nprintf "14.7\\n"\n' >"${STUB_OLD_OS}/sw_vers"
chmod +x "${STUB_OLD_OS}/sw_vers"
old_os_output="$(PATH="${STUB_OLD_OS}:${PATH}" check_desktop_prerequisites 2>&1)" && {
    printf 'FAIL: prerequisites should fail on macOS 14.\n' >&2
    exit 1
}
if ! printf '%s' "${old_os_output}" | grep -F 'macOS 15' >/dev/null; then
    printf 'FAIL: version error should name the macOS 15 requirement.\n' >&2
    exit 1
fi
printf 'PASS: desktop prerequisites reject macOS older than 15.\n'

# Non-macOS hosts fail fast before any network access: a uname stub reporting
# Linux drives main's guard with no download attempted.
linux_output="$(PATH="${STUB_LINUX}:${PATH}" INSTALL_DIR="${TEST_ROOT}/bin" \
    bash "${INSTALL_SH}" --with-desktop 2>&1)" && {
    printf 'FAIL: --with-desktop should fail on non-macOS.\n' >&2
    exit 1
}
if ! printf '%s' "${linux_output}" | grep -F -- '--with-desktop requires macOS' >/dev/null; then
    printf 'FAIL: non-macOS error should name the flag and the requirement.\n' >&2
    exit 1
fi
printf 'PASS: --with-desktop fails fast on non-macOS.\n'

# The sourcing guard survives stdin execution (curl | bash): with a Linux
# uname the run reaches main's platform guard instead of crashing on BASH_SOURCE.
stdin_guard_output="$(PATH="${STUB_LINUX}:${PATH}" INSTALL_DIR="${TEST_ROOT}/bin" \
    bash -s -- --with-desktop < "${INSTALL_SH}" 2>&1)" && {
    printf 'FAIL: piped --with-desktop should fail on non-macOS.\n' >&2
    exit 1
}
if ! printf '%s' "${stdin_guard_output}" | grep -F -- '--with-desktop requires macOS' >/dev/null; then
    printf 'FAIL: piped run should reach main instead of crashing on BASH_SOURCE.\n' >&2
    exit 1
fi
printf 'PASS: sourcing guard survives stdin execution.\n'

# Desktop install is on by default, with no explicit request recorded.
[[ "${WITH_DESKTOP}" -eq 1 ]]
[[ "${WITH_DESKTOP_EXPLICIT}" -eq 0 ]]
printf 'PASS: desktop install defaults on without an explicit flag.\n'

# Last flag wins for the desktop option pair (re-sourcing runs the arg loop
# over the new positional params without running main).
set -- --without-desktop
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
[[ "${WITH_DESKTOP}" -eq 0 && "${WITH_DESKTOP_EXPLICIT}" -eq 0 ]]
set -- --with-desktop --without-desktop
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
[[ "${WITH_DESKTOP}" -eq 0 && "${WITH_DESKTOP_EXPLICIT}" -eq 0 ]]
set -- --without-desktop --with-desktop
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
[[ "${WITH_DESKTOP}" -eq 1 && "${WITH_DESKTOP_EXPLICIT}" -eq 1 ]]
# Restore pristine defaults for the tests below (re-source resets both flags).
set --
# shellcheck disable=SC1090,SC1091
source "${INSTALL_SH}"
trap 'rm -rf -- "${TEST_ROOT}"; show_cursor' EXIT
printf 'PASS: last desktop flag wins.\n'

# ask_yes_no: asymmetric answers on both defaults, including case folding,
# surrounding whitespace, garbage, empty input, and EOF (no trailing newline).
ask_case() {
    local input="$1" default="$2" expect="$3" label="$4"
    local result
    if printf '%b' "$input" | ask_yes_no "Install?" "$default" >/dev/null 2>&1; then
        result=0
    else
        result=1
    fi
    if [[ "$result" != "$expect" ]]; then
        printf 'FAIL: ask_yes_no %s (input %q, default %s).\n' "$label" "$input" "$default" >&2
        exit 1
    fi
}
ask_case 'y\n' y 0 "accepts y"
ask_case 'y' y 0 "accepts y without trailing newline"
ask_case 'Y\n' y 0 "folds case"
ask_case 'yes\n' y 0 "accepts yes"
ask_case '  YeS  \n' y 0 "trims whitespace"
ask_case 'n\n' y 1 "accepts n against a yes default"
ask_case 'No\n' y 1 "accepts No"
ask_case 'maybe\n' y 1 "treats garbage as no"
ask_case '\n' y 0 "empty input selects the yes default"
ask_case '\n' n 1 "empty input selects the no default"
ask_case '' y 0 "EOF selects the yes default"
ask_case '' n 1 "EOF selects the no default"
printf 'PASS: ask_yes_no parses answers on both defaults.\n'

# The prompt hint reflects the default. (Capture stderr separately: the answer's
# exit status would fail a direct pipeline under pipefail.)
hint_yes="$(printf 'n\n' | ask_yes_no "Install?" "y" 2>&1 >/dev/null || true)"
if ! printf '%s' "$hint_yes" | grep -F '[Y/n]' >/dev/null; then
    printf 'FAIL: yes-default prompt should show [Y/n].\n' >&2
    exit 1
fi
hint_no="$(printf 'y\n' | ask_yes_no "Install?" "n" 2>&1 >/dev/null || true)"
if ! printf '%s' "$hint_no" | grep -F '[y/N]' >/dev/null; then
    printf 'FAIL: no-default prompt should show [y/N].\n' >&2
    exit 1
fi
printf 'PASS: ask_yes_no hint reflects the default.\n'

# resolve_desktop_choice through a PTY (stdin is a TTY there): "n" declines,
# empty input takes the install default. The inner shell prints choice=$?
# because script(1) does not reliably propagate the child exit status.
export INSTALL_SH
pty_choice() {
    # The sourced script enables `set -e`, so capture the choice with `||`
    # instead of letting a declined answer exit the inner shell early. The
    # sleep holds stdin open: without it script(1) delivers EOF before the
    # child reads and the answer is lost. Single quotes are intentional:
    # INSTALL_SH must expand in the inner shell, not here. The finder override
    # keeps the gate hermetic (a real /Applications install must not matter).
    # shellcheck disable=SC2016
    (printf '%b' "$1"; sleep 2) | script -q /dev/null bash -c \
        'source "${INSTALL_SH}" >/dev/null 2>&1; find_installed_desktop_app() { return 1; }; status=0; resolve_desktop_choice >/dev/null 2>&1 || status=$?; echo "choice=$status"' 2>/dev/null
}
if ! pty_choice 'n\n' | grep -F 'choice=1' >/dev/null; then
    printf 'FAIL: PTY answer n should decline the desktop app.\n' >&2
    exit 1
fi
if ! pty_choice '\n' | grep -F 'choice=0' >/dev/null; then
    printf 'FAIL: PTY empty answer should take the install default.\n' >&2
    exit 1
fi
printf 'PASS: resolve_desktop_choice prompts on a TTY stdin.\n'

# Silent gate cases return 2 without printing: explicit flags (either one)
# and existing installs never prompt.
gate_rc=0
gate_out="$(WITH_DESKTOP=0 resolve_desktop_choice 2>&1)" || gate_rc=$?
[[ "$gate_rc" -eq 2 && -z "$gate_out" ]] || {
    printf 'FAIL: --without-desktop must skip the prompt silently.\n' >&2
    exit 1
}
gate_rc=0
gate_out="$(WITH_DESKTOP_EXPLICIT=1 resolve_desktop_choice 2>&1)" || gate_rc=$?
[[ "$gate_rc" -eq 2 && -z "$gate_out" ]] || {
    printf 'FAIL: --with-desktop must skip the install prompt silently.\n' >&2
    exit 1
}
gate_rc=0
# The override IS invoked (after the semicolon); shellcheck cannot see it.
# shellcheck disable=SC2329
gate_out="$(find_installed_desktop_app() { return 0; }; resolve_desktop_choice 2>&1)" || gate_rc=$?
[[ "$gate_rc" -eq 2 && -z "$gate_out" ]] || {
    printf 'FAIL: existing install must skip the prompt silently.\n' >&2
    exit 1
}
printf 'PASS: prompt gate stays silent on flags and existing installs.\n'

# Asset discovery: tagged and untagged release tags resolve the same asset,
# each host takes its native asset, and an Intel-only release still serves an
# Apple Silicon host via the Rosetta fallback.
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_MAIN"
[[ "$(PATH="${STUB_ARM}:${PATH}" find_desktop_release_asset "v${FIX_VERSION}")" == "$FIX_ARM_ASSET" ]]
[[ "$(PATH="${STUB_ARM}:${PATH}" find_desktop_release_asset "${FIX_VERSION}")" == "$FIX_ARM_ASSET" ]]
# The tag travels verbatim in the URL path while the asset name uses the
# stripped version (proves the curl stub is not masking a path bug).
: >"$TEST_ROOT/curl.log"
PATH="${STUB_ARM}:${PATH}" find_desktop_release_asset "v${FIX_VERSION}" >/dev/null
grep -F "/v${FIX_VERSION}/${FIX_ARM_ASSET}" "$TEST_ROOT/curl.log" >/dev/null
: >"$TEST_ROOT/curl.log"
PATH="${STUB_ARM}:${PATH}" find_desktop_release_asset "${FIX_VERSION}" >/dev/null
grep -F "/${FIX_VERSION}/${FIX_ARM_ASSET}" "$TEST_ROOT/curl.log" >/dev/null
[[ "$(PATH="${STUB_X64}:${PATH}" find_desktop_release_asset "v${FIX_VERSION}")" == "$FIX_X64_ASSET" ]]
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_X64ONLY"
[[ "$(PATH="${STUB_ARM}:${PATH}" find_desktop_release_asset "v${FIX_VERSION}")" == "$FIX_X64_ASSET" ]]
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_EMPTY"
found_asset=""
if found_asset="$(find_desktop_release_asset "v${FIX_VERSION}" 2>/dev/null)"; then
    printf 'FAIL: empty fixtures should yield no desktop asset.\n' >&2
    exit 1
fi
[[ -z "$found_asset" ]]
unset VTCODE_INSTALL_DESKTOP_TEST_FIXTURES
printf 'PASS: desktop asset discovery prefers native, falls back, then gives up.\n'

# The real finder, before the hermetic double below: with an empty user dir it
# succeeds only via the real system location (or fails when absent). Placed
# after the last re-source: sourcing install.sh would restore the original.
if real_found="$(find_installed_desktop_app 2>/dev/null)"; then
    [[ "$real_found" == "/Applications/VT Code.app" ]]
else
    [[ -z "$real_found" ]]
fi
printf 'PASS: real finder reports only genuine locations.\n'

# Hermetic double: the real /Applications may hold the app on dev machines.
# Keep in sync with find_installed_desktop_app in install.sh (same contract:
# echo path + 0 when found, silent + 1 otherwise).
SYSTEM_APPLICATIONS="${TEST_ROOT}/system-applications"
mkdir -p "$SYSTEM_APPLICATIONS"
find_installed_desktop_app() {
    local candidate
    for candidate in "$HOME/Applications/$DESKTOP_APP_NAME" "$SYSTEM_APPLICATIONS/$DESKTOP_APP_NAME"; do
        if [[ -d "$candidate" && -f "$candidate/Contents/Info.plist" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

# An app in the (fake) system location is detected like a user install.
SYS_APP="$SYSTEM_APPLICATIONS/VT Code.app"
mkdir -p "$SYS_APP/Contents/MacOS"
: >"$SYS_APP/Contents/Info.plist"
[[ "$(find_installed_desktop_app)" == "$SYS_APP" ]]
rm -rf "$SYS_APP"
printf 'PASS: system-location install is detected.\n'

WORK_DIR="${TEST_ROOT}/work"
mkdir -p "${WORK_DIR}"

# Default mode degrades to a warning when the toolchain is missing, attempting
# no clone... (STUB_CURL first: the asset probe must fail hermetically, with
# no real network, before the toolchain check runs.)
if ! default_skip_output="$(PATH="${STUB_CURL}:${STUB_BIN}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"; then
    printf 'FAIL: best-effort desktop install must succeed without a toolchain.\n' >&2
    exit 1
fi
if ! printf '%s' "${default_skip_output}" | grep -F -- '--without-desktop' >/dev/null; then
    printf 'FAIL: best-effort skip should point at --without-desktop.\n' >&2
    exit 1
fi
[[ ! -f "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}" ]]
printf 'PASS: default desktop install skips cleanly without a toolchain.\n'

# ...while an explicit request fails hard on the same host.
export WITH_DESKTOP_EXPLICIT=1
explicit_skip_output="$(PATH="${STUB_CURL}:${STUB_BIN}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)" && {
    printf 'FAIL: explicit --with-desktop must fail without a toolchain.\n' >&2
    exit 1
}
WITH_DESKTOP_EXPLICIT=0
if printf '%s' "${explicit_skip_output}" | grep -F 'Continuing without the desktop app' >/dev/null; then
    printf 'FAIL: explicit failure must not print the best-effort skip line.\n' >&2
    exit 1
fi
printf 'PASS: explicit --with-desktop fails hard without a toolchain.\n'

# Off macOS the default stays silent...
if ! linux_default_output="$(PATH="${STUB_LINUX}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"; then
    printf 'FAIL: default desktop install must be a silent no-op off macOS.\n' >&2
    exit 1
fi
[[ -z "${linux_default_output}" ]]
printf 'PASS: default desktop install is a silent no-op off macOS.\n'

# ...while an explicit request fails with the platform error.
WITH_DESKTOP_EXPLICIT=1
linux_explicit_output="$(PATH="${STUB_LINUX}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)" && {
    printf 'FAIL: explicit --with-desktop must fail off macOS.\n' >&2
    exit 1
}
WITH_DESKTOP_EXPLICIT=0
if ! printf '%s' "${linux_explicit_output}" | grep -F -- '--with-desktop requires macOS' >/dev/null; then
    printf 'FAIL: explicit off-macOS error should name the flag.\n' >&2
    exit 1
fi
printf 'PASS: explicit --with-desktop fails off macOS.\n'

# With no fixtures configured the curl stub fails every probe, so the install
# falls back to a from-source build at the release tag.
fallback_output="$(install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"
printf '%s' "${fallback_output}" | grep -F 'falling back to a from-source build' >/dev/null

INSTALLED_APP="${HOME}/Applications/VT Code.app"
[[ -f "${INSTALLED_APP}/Contents/Info.plist" ]]
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "stub desktop" ]]
grep -Fq -- "clone --depth 1 --branch v9.9.9-test https://github.com/vinhnx/vtcode.git" \
    "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}"
printf 'PASS: desktop app clones at the release tag and installs to ~/Applications.\n'

[[ "$(find_installed_desktop_app)" == "${INSTALLED_APP}" ]]
printf 'PASS: installed desktop app is detected.\n'

# A second install must be a no-op, not a re-clone.
second_output="$(install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"
printf '%s' "${second_output}" | grep -F 'already installed' >/dev/null
[[ "$(grep -c . "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}")" -eq 1 ]]
printf 'PASS: reinstalling skips the build when the app exists.\n'

# The default path reports an existing install as success without rebuilding.
maybe_output="$(maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"
printf '%s' "${maybe_output}" | grep -F 'already installed' >/dev/null
[[ "$(grep -c . "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}")" -eq 1 ]]
printf 'PASS: default desktop install accepts an existing app.\n'

# A published asset installs without cloning: the ad hoc fixture also proves
# the FIRST-LAUNCH warning only prints for unsigned builds.
rm -rf "${HOME}/Applications"
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_MAIN"
: >"$TEST_ROOT/curl.log"
asset_output="$(PATH="${STUB_X64}:${PATH}" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" 2>&1)"
printf '%s' "${asset_output}" | grep -F "Downloading prebuilt desktop app: ${FIX_X64_ASSET}" >/dev/null
printf '%s' "${asset_output}" | grep -F 'First launch needs one manual approval' >/dev/null
printf '%s' "${asset_output}" | grep -F 'System Settings > Privacy & Security > Open Anyway' >/dev/null
[[ -f "${INSTALLED_APP}/Contents/Info.plist" ]]
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
[[ "$(find_installed_desktop_app)" == "${INSTALLED_APP}" ]]
[[ "$(grep -c . "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}")" -eq 1 ]]
printf 'PASS: prebuilt asset installs without cloning.\n'

# A notarized-layout asset (no FIRST-LAUNCH.txt) installs without the
# Gatekeeper warning.
rm -rf "${HOME}/Applications"
notarized_output="$(PATH="${STUB_ARM}:${PATH}" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" 2>&1)"
printf '%s' "${notarized_output}" | grep -F "Downloading prebuilt desktop app: ${FIX_ARM_ASSET}" >/dev/null
if printf '%s' "${notarized_output}" | grep -F 'First launch needs' >/dev/null; then
    printf 'FAIL: notarized asset should not print first-launch guidance.\n' >&2
    exit 1
fi
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop arm" ]]
printf 'PASS: notarized asset skips the Gatekeeper warning.\n'

# Rosetta fallback: an Apple Silicon host with an Intel-only release installs
# the Intel asset, probing native first and Intel second.
rm -rf "${HOME}/Applications"
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_X64ONLY"
: >"$TEST_ROOT/curl.log"
PATH="${STUB_ARM}:${PATH}" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" >/dev/null 2>&1
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
first_two_probes="$(grep -F 'vtcode-desktop-' "$TEST_ROOT/curl.log" | head -n 2)"
[[ "$(printf '%s' "$first_two_probes" | head -n 1)" == *"${FIX_ARM_ASSET}"* ]]
[[ "$(printf '%s' "$first_two_probes" | tail -n 1)" == *"${FIX_X64_ASSET}"* ]]
printf 'PASS: Apple Silicon falls back to the Intel asset in probe order.\n'

# Sidecar-only checksums verify the same as the aggregate manifest.
rm -rf "${HOME}/Applications"
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_SIDECAR"
PATH="${STUB_X64}:${PATH}" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" >/dev/null 2>&1
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
printf 'PASS: sidecar checksum verifies the desktop asset.\n'

# Missing checksum metadata fails closed (strict): a mangled release must
# never install unverified with only a warning.
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_NOCHECKSUM"
strict_output="$(install_desktop_release_asset "v${FIX_VERSION}" "$FIX_X64_ASSET" "${WORK_DIR}" 2>&1)" && {
    printf 'FAIL: missing checksum must fail the desktop install.\n' >&2
    exit 1
}
printf '%s' "${strict_output}" | grep -F 'refusing to install it unverified' >/dev/null
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
printf 'PASS: missing checksum fails closed instead of warning.\n'

# A checksum mismatch fails the install and leaves the previous app untouched.
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_TAMPERED"
tamper_output="$(install_desktop_release_asset "v${FIX_VERSION}" "$FIX_X64_ASSET" "${WORK_DIR}" 2>&1)" && {
    printf 'FAIL: checksum mismatch must fail the desktop install.\n' >&2
    exit 1
}
printf '%s' "${tamper_output}" | grep -F 'Checksum mismatch' >/dev/null
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
printf 'PASS: checksum mismatch fails closed without clobbering the app.\n'

# A zip without an app bundle fails instead of installing junk.
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_JUNK"
junk_output="$(install_desktop_release_asset "v${FIX_VERSION}" "$FIX_X64_ASSET" "${WORK_DIR}" 2>&1)" && {
    printf 'FAIL: bundle-less zip must fail the desktop install.\n' >&2
    exit 1
}
printf '%s' "${junk_output}" | grep -F 'Desktop archive is not a valid app bundle' >/dev/null
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
printf 'PASS: bundle-less zip fails closed without clobbering the app.\n'

# The asset path needs no build toolchain: cargo is unresolvable here and no
# clone is attempted (git ships in /usr/bin, so absence is proven by the
# untouched clone log instead of PATH exclusion).
rm -rf "${HOME}/Applications"
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_MAIN"
if PATH="${STUB_CURL}:${STUB_MIN}:/usr/bin:/bin" command -v cargo >/dev/null 2>&1; then
    printf 'FAIL: restricted PATH must not resolve cargo.\n' >&2
    exit 1
fi
PATH="${STUB_CURL}:${STUB_MIN}:/usr/bin:/bin" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" >/dev/null 2>&1
[[ -f "${INSTALLED_APP}/Contents/Info.plist" ]]
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "asset desktop x64" ]]
[[ "$(grep -c . "${VTCODE_INSTALL_DESKTOP_TEST_GIT_LOG}")" -eq 1 ]]
printf 'PASS: prebuilt asset installs without cargo or cloning.\n'

# Location choice through a PTY: "y" picks system-wide, "n" keeps the user dir.
pty_location() {
    # shellcheck disable=SC2016
    (printf '%b' "$1"; sleep 2) | script -q /dev/null bash -c \
        'source "${INSTALL_SH}" >/dev/null 2>&1; resolve_desktop_location >/dev/null 2>&1; echo "dir=$DESKTOP_INSTALL_DIR"' 2>/dev/null
}
if ! pty_location 'y\n' | grep -F 'dir=/Applications' >/dev/null; then
    printf 'FAIL: PTY answer y should pick /Applications.\n' >&2
    exit 1
fi
if ! pty_location 'n\n' | grep -F "dir=${HOME}/Applications" >/dev/null; then
    printf 'FAIL: PTY answer n should keep the user Applications dir.\n' >&2
    exit 1
fi
if ! pty_location '\n' | grep -F "dir=${HOME}/Applications" >/dev/null; then
    printf 'FAIL: PTY empty answer should default to the user Applications dir.\n' >&2
    exit 1
fi
printf 'PASS: location prompt picks user vs system Applications dir.\n'

# A preset DESKTOP_INSTALL_DIR is honored without prompting.
STAGED_SKELETON="${TEST_ROOT}/staged/VT Code.app"
mkdir -p "${STAGED_SKELETON}/Contents/MacOS"
printf 'staged marker\n' >"${STAGED_SKELETON}/Contents/MacOS/vtcode-desktop"
: >"${STAGED_SKELETON}/Contents/Info.plist"
rm -rf "${HOME}/Applications"
# Consumed by install_staged_desktop_app in the sourced install.sh.
# shellcheck disable=SC2034
DESKTOP_INSTALL_DIR="${TEST_ROOT}/alt-Apps"
install_staged_desktop_app "$STAGED_SKELETON" >/dev/null 2>&1
[[ -f "${TEST_ROOT}/alt-Apps/VT Code.app/Contents/Info.plist" ]]
[[ ! -e "${HOME}/Applications" ]]
printf 'PASS: staged install honors DESKTOP_INSTALL_DIR.\n'

# Unset location falls back to the user Applications dir.
unset DESKTOP_INSTALL_DIR
install_staged_desktop_app "$STAGED_SKELETON" >/dev/null 2>&1
[[ "$(cat "${INSTALLED_APP}/Contents/MacOS/vtcode-desktop")" == "staged marker" ]]
printf 'PASS: staged install defaults to the user Applications dir.\n'

# A non-writable destination routes through sudo and fails gracefully when
# elevation cannot complete (stub sudo never prompts).
STUB_SUDO="${TEST_ROOT}/stub-sudo"
mkdir -p "$STUB_SUDO"
cat >"${STUB_SUDO}/sudo" <<'STUB'
#!/bin/bash
printf '%s\n' "$*" >>"${VTCODE_INSTALL_DESKTOP_TEST_SUDO_LOG}"
exec "$@"
STUB
chmod +x "${STUB_SUDO}/sudo"
export VTCODE_INSTALL_DESKTOP_TEST_SUDO_LOG="${TEST_ROOT}/sudo.log"
NOWRITE_APPS="${TEST_ROOT}/nowrite"
mkdir -p "$NOWRITE_APPS"
chmod 555 "$NOWRITE_APPS"
rm -rf "${HOME}/Applications"
# Consumed by install_staged_desktop_app in the sourced install.sh.
# shellcheck disable=SC2034
DESKTOP_INSTALL_DIR="$NOWRITE_APPS"
: >"$TEST_ROOT/sudo.log"
sudo_output="$(PATH="${STUB_SUDO}:${PATH}" install_staged_desktop_app "$STAGED_SKELETON" 2>&1)" && {
    printf 'FAIL: unwritable destination must fail the staged install.\n' >&2
    exit 1
}
grep -F 'ditto' "$TEST_ROOT/sudo.log" >/dev/null
printf '%s' "$sudo_output" | grep -F "Failed to install the desktop app to $NOWRITE_APPS" >/dev/null
[[ ! -e "${HOME}/Applications" ]]
unset DESKTOP_INSTALL_DIR
printf 'PASS: unwritable destination elevates via sudo and fails gracefully.\n'

# Explicit flags and presets skip the location question: exact output match
# proves nothing was printed (any prompt would fail it, on any machine).
# Subshell setup (not VAR=x prefixes): bash discards in-function assignments
# to prefix-assigned variables, hiding the result from the trailing echo.
loc_result="$(WITH_DESKTOP_EXPLICIT=1; DESKTOP_INSTALL_DIR=""; resolve_desktop_location 2>&1; echo "dir=$DESKTOP_INSTALL_DIR")"
[[ "$loc_result" == "dir=${HOME}/Applications" ]]
loc_result="$(DESKTOP_INSTALL_DIR="${TEST_ROOT}/preset-Apps"; resolve_desktop_location 2>&1; echo "dir=$DESKTOP_INSTALL_DIR")"
[[ "$loc_result" == "dir=${TEST_ROOT}/preset-Apps" ]]
printf 'PASS: explicit flags and presets skip the location question.\n'

# Failures return instead of exiting (set -e): these run in the main shell,
# so an exit would kill the suite instead of reaching the PASS line.
rm -rf "${HOME}/Applications"
export VTCODE_INSTALL_DESKTOP_TEST_FIXTURES="$FIX_TAMPERED"
if PATH="${STUB_X64}:${PATH}" install_desktop_app "v${FIX_VERSION}" "${WORK_DIR}" >/dev/null 2>&1; then
    printf 'FAIL: tampered asset must fail the full install.\n' >&2
    exit 1
fi
NOPERM_PARENT="${TEST_ROOT}/noperm"
mkdir -p "$NOPERM_PARENT"
chmod 555 "$NOPERM_PARENT"
DESKTOP_INSTALL_DIR="$NOPERM_PARENT/Apps"
if PATH="${STUB_SUDO}:${PATH}" install_staged_desktop_app "$STAGED_SKELETON" >/dev/null 2>&1; then
    printf 'FAIL: uncreatable destination must fail the staged install.\n' >&2
    exit 1
fi
unset DESKTOP_INSTALL_DIR
# Reset for hygiene; consumed by run_desktop_install_step in install.sh.
# shellcheck disable=SC2034
DESKTOP_NEEDS_SUDO=0
printf 'PASS: install failures return instead of exiting.\n'

unset VTCODE_INSTALL_DESKTOP_TEST_FIXTURES VTCODE_INSTALL_DESKTOP_TEST_CURL_LOG VTCODE_INSTALL_DESKTOP_TEST_SUDO_LOG

# The EXIT-trap handler removes the recorded work dir (even with spaces in the
# path) and tolerates an empty one.
CLEANUP_DIR="${TEST_ROOT}/cleanup dir"
mkdir -p "${CLEANUP_DIR}/nested"
: >"${CLEANUP_DIR}/nested/file"
: >"${TEST_ROOT}/cleanup-neighbor"
export VTCODE_WORK_DIR="${CLEANUP_DIR}"
cleanup >/dev/null 2>&1
[[ ! -e "${CLEANUP_DIR}" ]]
[[ -f "${TEST_ROOT}/cleanup-neighbor" ]]
VTCODE_WORK_DIR=""
cleanup >/dev/null 2>&1
printf 'PASS: cleanup removes the quoted work dir and tolerates an empty one.\n'
