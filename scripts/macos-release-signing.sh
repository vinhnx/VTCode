#!/usr/bin/env bash

# Optional Developer ID signing and notarization for macOS release binaries.
# Without credentials, release scripts keep packaging unsigned binaries.
# Source this file from release scripts; it intentionally does not change the
# caller's shell options.

VTCODE_MACOS_RELEASE_IDENTIFIER="com.vinhnx.vtcode"
_vtcode_macos_release_signing_mode="uninitialized"

macos_release_signing_preflight() {
    if [[ "$_vtcode_macos_release_signing_mode" != "uninitialized" ]]; then
        return 0
    fi

    if [[ "$(uname -s)" != "Darwin" ]]; then
        printf 'Error: macOS release binaries must be signed and notarized on macOS.\n' >&2
        return 1
    fi

    if [[ -z "${VTCODE_MACOS_SIGNING_IDENTITY:-}" && -z "${VTCODE_MACOS_NOTARY_PROFILE:-}" ]]; then
        _vtcode_macos_release_signing_mode="unsigned"
        printf 'Warning: packaging unsigned, unnotarized macOS binaries. Gatekeeper may warn, block, or request user approval.\n' >&2
        printf 'Set VTCODE_MACOS_SIGNING_IDENTITY and VTCODE_MACOS_NOTARY_PROFILE to enable Developer ID signing and notarization.\n' >&2
        return 0
    fi
    if [[ -z "${VTCODE_MACOS_SIGNING_IDENTITY:-}" || -z "${VTCODE_MACOS_NOTARY_PROFILE:-}" ]]; then
        printf 'Error: set both VTCODE_MACOS_SIGNING_IDENTITY and VTCODE_MACOS_NOTARY_PROFILE, or leave both unset for unsigned packaging.\n' >&2
        return 1
    fi
    case "$VTCODE_MACOS_SIGNING_IDENTITY" in
        'Developer ID Application:'*) ;;
        *)
            printf 'Error: VTCODE_MACOS_SIGNING_IDENTITY must be a Developer ID Application identity.\n' >&2
            return 1
            ;;
    esac
    if [[ -z "${VTCODE_MACOS_NOTARY_PROFILE:-}" ]]; then
        printf 'Error: set VTCODE_MACOS_NOTARY_PROFILE to a configured notarytool keychain profile.\n' >&2
        return 1
    fi

    local tool
    for tool in codesign security xcrun ditto spctl; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            printf 'Error: required macOS release tool is missing: %s\n' "$tool" >&2
            return 1
        fi
    done

    local identities
    if ! identities=$(security find-identity -v -p codesigning 2>&1); then
        printf 'Error: could not inspect the macOS code-signing identities.\n%s\n' "$identities" >&2
        return 1
    fi
    if ! printf '%s\n' "$identities" | grep -Fq -- "$VTCODE_MACOS_SIGNING_IDENTITY"; then
        printf 'Error: Developer ID Application identity is not available in the keychain: %s\n' \
            "$VTCODE_MACOS_SIGNING_IDENTITY" >&2
        return 1
    fi

    if ! xcrun notarytool history --keychain-profile "$VTCODE_MACOS_NOTARY_PROFILE" >/dev/null 2>&1; then
        printf 'Error: notarytool could not use the configured keychain profile: %s\n' \
            "$VTCODE_MACOS_NOTARY_PROFILE" >&2
        return 1
    fi

    _vtcode_macos_release_signing_mode="notarized"
}

verify_macos_release_binary() {
    local binary=$1
    if ! macos_release_signing_preflight; then
        return 1
    fi

    if [[ ! -f "$binary" || ! -x "$binary" ]]; then
        printf 'Error: macOS release binary is missing or not executable: %s\n' "$binary" >&2
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi

    if ! codesign --verify --strict --verbose=2 "$binary"; then
        printf 'Error: macOS release binary has an invalid code signature: %s\n' "$binary" >&2
        return 1
    fi

    local assessment
    if ! assessment=$(spctl --assess --type execute --verbose=4 "$binary" 2>&1); then
        printf 'Error: Gatekeeper rejected the macOS release binary: %s\n%s\n' "$binary" "$assessment" >&2
        return 1
    fi
    if ! printf '%s\n' "$assessment" | grep -Fq 'source=Notarized Developer ID'; then
        printf 'Error: Gatekeeper did not report a notarized Developer ID signature for %s:\n%s\n' \
            "$binary" "$assessment" >&2
        return 1
    fi
}

verify_macos_release_archive() {
    local archive=$1

    if [[ "$(uname -s)" != "Darwin" ]]; then
        printf 'Error: macOS release archives can only be verified on macOS: %s\n' "$archive" >&2
        return 1
    fi

    local archive_listing
    if ! archive_listing=$(tar -tzf "$archive"); then
        printf 'Error: could not list macOS release archive: %s\n' "$archive" >&2
        return 1
    fi

    local archive_entry
    archive_entry=$(printf '%s\n' "$archive_listing" | awk '$0 == "vtcode" || $0 == "./vtcode" { print; exit }')
    if [[ -z "$archive_entry" ]]; then
        printf 'Error: macOS release archive does not contain a root vtcode executable: %s\n' "$archive" >&2
        return 1
    fi

    local verify_dir
    if ! verify_dir=$(mktemp -d "${TMPDIR:-/tmp}/vtcode-release-verify.XXXXXX"); then
        printf 'Error: could not create a temporary directory to verify %s\n' "$archive" >&2
        return 1
    fi

    if ! tar -xOzf "$archive" "$archive_entry" >"$verify_dir/vtcode"; then
        rm -rf "$verify_dir"
        printf 'Error: could not extract vtcode from macOS release archive: %s\n' "$archive" >&2
        return 1
    fi
    if ! chmod +x "$verify_dir/vtcode"; then
        rm -rf "$verify_dir"
        printf 'Error: could not make the extracted release executable assessable: %s\n' "$archive" >&2
        return 1
    fi

    if ! verify_macos_release_binary "$verify_dir/vtcode"; then
        rm -rf "$verify_dir"
        return 1
    fi

    rm -rf "$verify_dir"
    return 0
}

sign_and_notarize_macos_binary() {
    local binary=$1

    if ! macos_release_signing_preflight; then
        return 1
    fi
    if [[ ! -f "$binary" ]]; then
        printf 'Error: macOS release binary is missing: %s\n' "$binary" >&2
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi

    if ! codesign --force --options runtime --timestamp \
        --identifier "$VTCODE_MACOS_RELEASE_IDENTIFIER" \
        --sign "$VTCODE_MACOS_SIGNING_IDENTITY" "$binary"; then
        printf 'Error: failed to Developer ID sign macOS release binary: %s\n' "$binary" >&2
        return 1
    fi
    if ! codesign --verify --strict --verbose=2 "$binary"; then
        printf 'Error: signed macOS release binary failed signature verification: %s\n' "$binary" >&2
        return 1
    fi

    local work_dir
    if ! work_dir=$(mktemp -d "${TMPDIR:-/tmp}/vtcode-notary.XXXXXX"); then
        printf 'Error: could not create a temporary directory for notarization.\n' >&2
        return 1
    fi

    local staged_binary="$work_dir/vtcode"
    local notarization_zip="$work_dir/vtcode-notarization.zip"
    if ! cp "$binary" "$staged_binary"; then
        rm -rf "$work_dir"
        printf 'Error: could not stage macOS release binary for notarization: %s\n' "$binary" >&2
        return 1
    fi
    if ! ditto -c -k --norsrc --noextattr --noqtn "$staged_binary" "$notarization_zip"; then
        rm -rf "$work_dir"
        printf 'Error: could not create the notarization archive for %s\n' "$binary" >&2
        return 1
    fi

    local submission
    if ! submission=$(xcrun notarytool submit "$notarization_zip" \
        --keychain-profile "$VTCODE_MACOS_NOTARY_PROFILE" --wait --output-format json 2>&1); then
        rm -rf "$work_dir"
        printf 'Error: Apple notarization failed for %s:\n%s\n' "$binary" "$submission" >&2
        return 1
    fi
    if ! printf '%s\n' "$submission" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"Accepted"'; then
        rm -rf "$work_dir"
        printf 'Error: Apple did not accept notarization for %s:\n%s\n' "$binary" "$submission" >&2
        return 1
    fi

    rm -rf "$work_dir"
    verify_macos_release_binary "$binary"
}

# Developer ID sign every executable inside a macOS .app bundle, then the bundle.
#
#   sign_macos_app_bundle <app-bundle>
#
# Nested code takes the shared release identifier; the bundle itself keeps the
# identifier from its own Info.plist (CFBundleIdentifier), which is what
# Gatekeeper and the notarization ticket bind to. No-op when packaging
# unsigned (the ad hoc signature from package-desktop-macos.sh stands).
sign_macos_app_bundle() {
    if [[ $# -ne 1 ]]; then
        echo "usage: sign_macos_app_bundle <app-bundle>" >&2
        return 2
    fi
    local app=$1
    if ! macos_release_signing_preflight; then
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi
    if [[ ! -d "$app/Contents/MacOS" ]]; then
        printf 'Error: macOS app bundle is missing Contents/MacOS: %s\n' "$app" >&2
        return 1
    fi

    local nullglob_was_set=0
    shopt -q nullglob && nullglob_was_set=1
    shopt -s nullglob
    local -a nested_binaries=("$app"/Contents/MacOS/* "$app"/Contents/Helpers/*)
    [[ "$nullglob_was_set" -eq 1 ]] || shopt -u nullglob

    local nested
    for nested in "${nested_binaries[@]}"; do
        [[ -f "$nested" ]] || continue
        if ! codesign --force --options runtime --timestamp \
            --identifier "$VTCODE_MACOS_RELEASE_IDENTIFIER" \
            --sign "$VTCODE_MACOS_SIGNING_IDENTITY" "$nested"; then
            printf 'Error: failed to Developer ID sign bundled executable: %s\n' "$nested" >&2
            return 1
        fi
    done
    if ! codesign --force --options runtime --timestamp \
        --sign "$VTCODE_MACOS_SIGNING_IDENTITY" "$app"; then
        printf 'Error: failed to Developer ID sign macOS app bundle: %s\n' "$app" >&2
        return 1
    fi
    if ! codesign --verify --deep --strict --verbose=2 "$app"; then
        printf 'Error: signed macOS app bundle failed signature verification: %s\n' "$app" >&2
        return 1
    fi
}

# Submit a macOS distribution archive to Apple notarization and wait.
#
#   notarize_macos_archive <archive>
#
# No-op when packaging unsigned.
notarize_macos_archive() {
    if [[ $# -ne 1 ]]; then
        echo "usage: notarize_macos_archive <archive>" >&2
        return 2
    fi
    local archive=$1
    if ! macos_release_signing_preflight; then
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi
    if [[ ! -f "$archive" ]]; then
        printf 'Error: macOS distribution archive is missing: %s\n' "$archive" >&2
        return 1
    fi

    local submission
    if ! submission=$(xcrun notarytool submit "$archive" \
        --keychain-profile "$VTCODE_MACOS_NOTARY_PROFILE" --wait --output-format json 2>&1); then
        printf 'Error: Apple notarization failed for %s:\n%s\n' "$archive" "$submission" >&2
        return 1
    fi
    if ! printf '%s\n' "$submission" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"Accepted"'; then
        printf 'Error: Apple did not accept notarization for %s:\n%s\n' "$archive" "$submission" >&2
        return 1
    fi
}

# Staple the notarization ticket to a macOS app bundle after acceptance, so
# offline Gatekeeper checks pass. No-op when packaging unsigned.
#
#   staple_macos_app_bundle <app-bundle>
staple_macos_app_bundle() {
    if [[ $# -ne 1 ]]; then
        echo "usage: staple_macos_app_bundle <app-bundle>" >&2
        return 2
    fi
    local app=$1
    if ! macos_release_signing_preflight; then
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi
    if [[ ! -d "$app" ]]; then
        printf 'Error: macOS app bundle is missing: %s\n' "$app" >&2
        return 1
    fi
    local staple_output
    if ! staple_output=$(xcrun stapler staple "$app" 2>&1); then
        printf 'Error: failed to staple the notarization ticket to %s:\n%s\n' "$app" "$staple_output" >&2
        return 1
    fi
}

# Verify a macOS app bundle carries a notarized Developer ID signature.
#
#   verify_macos_app_bundle <app-bundle>
#
# No-op when packaging unsigned.
verify_macos_app_bundle() {
    if [[ $# -ne 1 ]]; then
        echo "usage: verify_macos_app_bundle <app-bundle>" >&2
        return 2
    fi
    local app=$1
    if ! macos_release_signing_preflight; then
        return 1
    fi
    if [[ ! -d "$app" ]]; then
        printf 'Error: macOS app bundle is missing: %s\n' "$app" >&2
        return 1
    fi
    if [[ "$_vtcode_macos_release_signing_mode" == "unsigned" ]]; then
        return 0
    fi

    if ! codesign --verify --deep --strict --verbose=2 "$app"; then
        printf 'Error: macOS app bundle has an invalid code signature: %s\n' "$app" >&2
        return 1
    fi

    # No --type flag: spctl picks the assessment context for a bundle path
    # itself (unlike --type execute, which is for bare executables).
    local assessment
    if ! assessment=$(spctl --assess --verbose=4 "$app" 2>&1); then
        printf 'Error: Gatekeeper rejected the macOS app bundle: %s\n%s\n' "$app" "$assessment" >&2
        return 1
    fi
    if ! printf '%s\n' "$assessment" | grep -Fq 'source=Notarized Developer ID'; then
        printf 'Error: Gatekeeper did not report a notarized Developer ID signature for %s:\n%s\n' \
            "$app" "$assessment" >&2
        return 1
    fi
}
