#!/usr/bin/env bash

set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    printf 'Usage: %s <app-path>\n' "${0##*/}" >&2
    exit 2
fi

APP_PATH="$1"
if [[ "${APP_PATH}" != /* ]]; then
    APP_PATH="${PWD}/${APP_PATH}"
fi

if [[ "${APP_PATH}" != *.app || ! -d "${APP_PATH}/Contents" || ! -f "${APP_PATH}/Contents/Info.plist" ]]; then
    printf 'Error: not a macOS app bundle: %s\n' "${APP_PATH}" >&2
    exit 1
fi

if ! command -v open >/dev/null 2>&1; then
    printf 'Error: macOS open command is unavailable.\n' >&2
    exit 1
fi

# Address the selected bundle path directly; display-name lookup is ambiguous
# when development builds with the same bundle identifier are registered.
open -n "${APP_PATH}"
