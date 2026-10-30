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
export PATH="${STUB_BIN}:${PATH}"

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

WORK_DIR="${TEST_ROOT}/work"
mkdir -p "${WORK_DIR}"

# Default mode degrades to a warning when the toolchain is missing, attempting
# no clone...
if ! default_skip_output="$(PATH="${STUB_BIN}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)"; then
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
explicit_skip_output="$(PATH="${STUB_BIN}" maybe_install_desktop_app "v9.9.9-test" "${WORK_DIR}" 2>&1)" && {
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

install_desktop_app "v9.9.9-test" "${WORK_DIR}" >/dev/null 2>&1

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
