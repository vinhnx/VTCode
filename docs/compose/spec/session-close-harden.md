---
feature: session-close-harden
status: delivered
updated: 2026-09-25
branch: fix/session-close-harden
commits: 29e3da632..ff5ecda75
---

# Session Close Harden (Unresponsive UI)

## Report

**What was built** — Session stop/close can no longer freeze the TUI. PTY terminate/close uses only bounded waits:
`reap_child_bounded` polls `try_wait` (2s) instead of `Child::wait`, and `join_reader_thread_bounded` (5s) is shared by
`Drop` and `PtyManager::close_session`. `ExecSessionManager::close_session` detaches the record first, aborts watchers
under a 1s timeout, acquires the output-read lock under a 1s timeout, runs PTY close on `spawn_blocking` under a 12s
outer bound, and always releases the foreground PTY counter and background slot even when backend close times out.
`ForceCancelPtySession` now force-terminates and closes every foreground exec session (background sessions stay
user-owned) and reports stopped/closed/failed counts instead of a no-op status line.

**Verification** — commands run and observed results:

- `cargo nextest run -p vtcode-core -E 'test(/exec_session/) or test(/pty/)'` — PASS (218)
- `cargo nextest run -p vtcode -E 'test(/inline_events/) or test(/local_agent/)'` — PASS (65)
- `./scripts/check-dev.sh` — PASS (fmt, clippy `-D warnings`, compile, shell lint)
- New regressions: `force_terminate_or_close_exited_pty_releases_foreground_count`,
  `force_cancel_foreground_sessions_skips_background` — PASS
- Independent review (general-2): AC1–5 largely met; critical finding that `output_read_lock` acquire sat outside the
  timeout — fixed in `ff5ecda75` (lock acquire time-bounded, close budget 12s, PTY terminate on `spawn_blocking`). First
  review agent failed with APIError; second completed.

**Journey log** —

1. Symptom cluster (unstoppable background task + sticky `Running PTY command...` + locked composer) traced to unbounded
   `reader_thread.join()` / `child.wait()` on the async close path; `Drop` already had a timed join that `close_session`
   lacked.
2. User chose hardened close path (not concurrent mid-turn session actions).
3. `force_terminate_or_close` on a live session still only kills and keeps the row visible (existing UX); ForceCancel is
   the escape hatch that also detaches so counters drop.
4. Review taught: any `Mutex::lock().await` on a teardown path must sit inside the timeout wrapper or the bound is
   illusory; `spawn_blocking` cannot be cancelled on timeout so the outer budget must cover inner sums.
5. Simulated stuck-reader unit test was not practical with portable-pty; coverage is exited-PTY close + ForceCancel
   counter release instead.

## [S1] Problem

When a background/foreground exec session exits (or is retained after exit), attempting to stop or close it can hang the
main runloop. Observed symptoms:

1. Background task cannot be stopped or closed (`ForceTerminateOrClose` / Ctrl+X appears to do nothing).
2. Main runloop stays in a loading state (`Running PTY command...` shimmer).
3. User cannot send new messages or commands (composer locked).

Root cause: the session-close path performs unbounded blocking waits on the async runloop:

- `PtyManager::close_session` calls `reader_thread.join()` with **no timeout**.
- `PtySessionHandle::graceful_terminate` / `force_terminate` call `child.wait()` while holding the child lock.
- `ExecSessionManager::close_session` awaits aborted watcher tasks and then runs the blocking PTY close **inline on the
  async worker**.

When any of those waits stall (reader blocked on a PTY `read`, child wait stuck, watcher in a sync section),
`close_session` never returns, the runloop cannot process further UI events, and the composer stays locked. `Drop`
already uses a timed reader join; `close_session` does not.

## [S2] Design

### Contracts

1. **Bounded waits only.** No unbounded `join()`/`wait()` on the PTY close/terminate path. Reuse
   `READER_THREAD_TIMEOUT_MS` (5s) for reader join; child wait polls `try_wait` with a 2s budget instead of
   `child.wait()`. Output-read lock acquire during close is bounded (1s).
2. **Close never stalls the async runtime.** `ExecSessionManager::close_session` runs the blocking backend close on
   `tokio::task::spawn_blocking`, wrapped in `tokio::time::timeout` (12s outer). PTY terminate also runs on
   `spawn_blocking`.
3. **Counters always release.** Foreground PTY count and background slot are released even when backend close times out
   or errors. After `close_session` returns (success or timeout), the UI must not keep `Running PTY command...` for that
   session.
4. **ForceCancel actually kills.** `ForceCancelPtySession` force-terminates and closes foreground exec sessions and
   reports stopped/closed/failed counts; background sessions are left alone.
5. **Timeout is a soft failure.** On timeout, close returns an error to the caller, but the session record is already
   detached and counters are released so the UI unblocks. Residual OS process cleanup may finish later; it must not pin
   the runloop.

### Behavior

- `force_terminate_or_close` on an already-exited session closes it and returns `true` without hanging.
- `force_terminate_or_close` on a live session force-terminates and leaves the row visible until a later close (existing
  drawer UX); ForceCancel force-terminates **and** detaches.
- Graceful terminate of an already-exited session still prints the retained-output message; ForceTerminateOrClose
  remains the close path and now completes.
- Composer accepts input again once close returns (or times out).

### Testing boundaries

- Unit: timed reader join / child wait do not block past the budget; close releases counters on backend failure.
- Regression: `force_terminate_or_close` on an exited PTY completes under the bound with `active_pty_sessions == 0`;
  ForceCancel detaches foreground and leaves background intact.
- No change to wait/yield streaming, background completion capture, or drawer entry retention.

## [S3] Out of Scope

- Processing `ExecSessionAction` concurrently with in-flight tools (drawer mid-turn).
- Changing exec-session retention / local-agents window layout.
- Pipe-backend close paths beyond shared timeout wiring.
- User-visible timeout constants/config.

## Tasks

- [x] T1: Bound PTY terminate/close waits — acceptance: `graceful_terminate`/`force_terminate` poll `try_wait` under a
      timeout; `close_session` reader join uses the Drop timeout helper; no unbounded join/wait remains on this path
      (covers: S2.1)
- [x] T2: Harden `ExecSessionManager::close_session` — acceptance: backend close runs on `spawn_blocking` with timeout;
      foreground/background counters release on error/timeout; `force_terminate_or_close` returns without hanging
      (covers: S2.2, S2.3, S2.5)
- [x] T3: Implement real ForceCancel — acceptance: `ForceCancelPtySession` force-terminates foreground exec sessions and
      reports count/outcome (covers: S2.4)
- [x] T4: Regression tests + verify — acceptance: tests cover exited-session close counter release and ForceCancel
      isolation; `cargo nextest run -p vtcode-core -E 'test(/exec_session/) or test(/pty/)'` and
      `./scripts/check-dev.sh` pass (covers: S2.1–S2.5)
