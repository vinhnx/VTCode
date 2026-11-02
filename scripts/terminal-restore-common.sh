#!/usr/bin/env bash

# Shared terminal-state restoration for the TUI launch scripts
# (run.sh, rrf.sh, run-debug.sh). Sourced, not executed: it only defines
# restore_terminal_state, leaving each launcher to install its own
# `trap restore_terminal_state EXIT INT TERM`.
#
# Deliberately not part of scripts/common.sh: sourcing common.sh exports
# VTCODE_DISABLE_KEYRING=1, which would silently switch interactive dev runs
# off the macOS Keychain.

# Best-effort restore for raw/alternate-screen/mouse modes in case vtcode aborts.
# Order mirrors vtcode_ui::tui::panic_hook::restore_tui: LeaveAlternateScreen first,
# then bracketed paste/focus/mouse, pop keyboard flags, reset OSC 22 pointer,
# default cursor shape, show cursor.
restore_terminal_state() {
	if [[ -t 1 || -t 2 ]]; then
		printf '\r\033[K\033[?1049l\033[?2004l\033[?1004l\033[?1006l\033[?1015l\033[?1003l\033[?1002l\033[?1000l\033[<1u\033]22;default\007\033[0 q\033[?25h' >/dev/tty 2>/dev/null || true
		stty sane </dev/tty >/dev/tty 2>/dev/null || true
	fi
}
