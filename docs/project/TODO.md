routine: .vtcode/memory/issues.md

---

benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

https://developers.openai.com/api/docs/guides/agents-api/tools/computer-use

---

implement standalone terminal for desktop app. maybe reuse https://github.com/raphamorim/rio current existing riovt infra. reference codex design /Users/vinhnguyenxuan/Documents/vtcode-resources/terminal.

you can use deepwiki mcp for openai/codex for arch and system design but for desktop app

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

check and fix splitting the two 1000+ line files — is large surgery in view/

---

check and improve vtcode-acp compile speed and efficiency to clear the bottlneck
and improve effiency in build

---

scan and rename old docs files that is upper case and change to lower case, also audit and check for dead and wrong links and fix and improve, remove outdated docs and update if needed.

for somple special docs file that need upper case (eg: SECURITY, ACCESSIBILITY. README LICENSE) -> keep as is. check github markdown files convention

also chekc and audit the files and move to correct directory and group for a clean structure

---

check and improve Pele font, currently it showing as serif font, make sure it can follow user system font's
  settings if possible. also check pele.run docs and add a new configurable settings if pele has API for
  configuration and save and apply that for future diagram views

---

check and fix Translucent views doesn't apply translucent, views still show opaque and the options should follow macOS liquid glass human inteferace guideline and apply correctly.

---


only show force terminate stuck processs button if there actually are ones (default to hide the button) to keep the input composer area clean

---

No files changed — review only.

Short answer: keep 1000 as the universal hard ceiling, but it is too lax as a Rust quality gate. Enforce stricter for first-party Rust, tiered, not a blanket `500-enforce` tomorrow.

What you have now:

* `scripts/check_file_length.py:DEFAULT_MAX_LINES=1000`, `enforce` mode: no tracked text file >1000 except `scripts/file_length_allowlist.txt`. That is a safety net, not a design rule.
* `scripts/check_rust_file_length.py --max-lines 500 --mode warn`: soft Rust-only signal. Its own output says `Promote to --mode enforce after baseline cleanup`.
* `clippy.toml`: `cognitive-complexity-threshold=75`, `too-many-arguments-threshold=9`. No file-length lint — Clippy/Rust has no official LOC lint.

Why 1000 is unsuitable for Rust:

* Distribution from this checkout (`git ls-files *.rs`, 3560 files): `<=300: 2218, 300-500: 642, 500-800: 448, 800-1000: 246, >1000: 6`. `>500 = 700` files.
* Top first-party files cluster at `991-1000`, e.g. `crates/codegen/vtcode-desktop/src/desktop/view/components.rs:1000`, `shortcuts.rs:1000`, `crates/codegen/vtcode-webmcp/src/filesystem/mod.rs:1000`, `view.rs:999`, `plan_approval/mod.rs:999`, `turn_loop_helpers.rs:995`. That bunching just under the ceiling is classic gate-gaming / God-module smell.
* The 6 `>1000` Rust files are all vendored `patches/**` (`window.rs:4203`, `generated.rs:1928`, etc.), already exempted in the hard gate by `patches/**=75_000` but still counted in the Rust warn output. They pollute the signal.
* Rust best practice has no canonical file LOC number. Idiom is small cohesive modules, explicit `mod` tree, `foo.rs` + `foo/tests.rs`, single responsibility, fast incremental compile, reviewable diffs. Community norm is roughly: `<300 ideal, 500 warning, 800+ split`. Your existing `500-warn` already matches that; `1000-enforce` permits 2x that.

Recommendation — tiered, not one number:

1. Keep `1000-enforce` for all tracked text. It is suitable there.
2. Do not flip Rust `500-enforce` globally now — 700 files would fail. Instead:
   * Exclude `patches/**`, generated (`simdutf`, `crossterm` vendored, `*.generated.rs`) from the Rust gate.
   * `warn at 500` (keep, for new/refactored code target).
   * `enforce at 750-800` for first-party `src/**/*.rs` (would catch only the ~250 worst offenders, forces split plans).
   * Separate or higher budget for `tests/**` — long fixture/test files are less harmful, but still require `mod` splits over ~1000.
   * Ratchet rule you already use for allowlist: caps only ratchet down, new files >500 need justification in PR.
3. Pair LOC with complexity, not LOC alone: keep `cognitive-complexity 75` and consider `clippy::pedantic`/`too-many-lines` style lints per-crate, plus module-cohesion review (view/runloop/tools are the repeat offenders).

If you want it implemented, the increment is: fix Rust script excludes, wire `check_rust_file_length.py --mode enforce --max-lines 800` into `scripts/check.sh` (warn in `check-dev.sh`), update `AGENTS.md` Rules line and `docs/development/rust-style-guide.md`.

Validation run: `check_rust_file_length.py --mode warn --max-lines 500` and `check_file_length.py --mode warn` both exit 0; numbers above are from those runs.


Harness verification: `cargo check --locked` exited 0.

--

plan: gpui cross-platform (mac-only today -> linux-first, defer windows/web)

context: vtcode-desktop is macOS-gated (Cargo.toml macos-only deps, main.rs exit(1) elsewhere, objc2 NSStatusBar/NSApplication/NSFont/trash, package-desktop-macos.sh + install.sh check_desktop_os + desktop-macos.yml macos-15). upstream GPUI already ships gpui-fast-linux/windows/web/macos in Cargo.lock; desktop-protocol (serde/json/tokio/url) is portable. mac-only seams are ~6 sites: view/menu_bar.rs (NSStatusBar), app_icon.rs (dock), view/about.rs (NSApplication panel), preferences.rs (NSFont), files/mutate.rs (trashItemAtURL), view/navigation_history.rs (NSUserDefaults scroll). typography.rs + navigation_history.rs already have non-mac fallbacks.

pros of porting: reach linux/windows majority, reuse portable protocol+runloop (thin client), linux packaging easy (tarball, no signing), CI cheap on ubuntu.
cons/costs: per-OS tray/menu/dock/trash/open/fonts/IME/notifications/deeplink/updater/signing; 2-3x CI + manual QA (focus/keyboard/IME/HiDPI/Wayland-X11/GPU-less); degraded port hurts more than no port; opportunity cost vs agent loop.
tradeoff: do not port now; buy optionality cheap. options considered: (A) stay mac-only, (B) linux-first preview, (C) linux+windows, (D) web client over same protocol, (E) protocol-as-product. pick B-gated: seam + check-green, ship preview only on real demand.

phase 0 - spike (hours, no product promise):

1. cargo check -p vtcode-desktop on ubuntu + gpui-kit hello-world on linux/windows; record text/IME/GPU breakage.
2. list exact cfg breakage (objc2 sites above + files/launch.rs fixed-argv + fonts/scroll).
3. stop if text/IME/GPU fails.
   verify: spike notes with breakage list.

phase 1 - platform seam, zero mac behavior change:

1. add desktop/platform/{trash,reveal,open_path,dock_tray,fonts,about,scroll_direction}.rs behind PlatformHost trait; mac impl keeps current objc2 calls; other OS compile-time stubs (still exit 1 at runtime).
2. swap Foundation trash for `trash` crate (cross-platform) or gate behind trait.
3. keep views calling only trait; no #[cfg(macos)] outside platform/.
   verify: cargo check --locked -p vtcode-desktop; cargo nextest run -p vtcode-desktop (mac green, no behavior diff).

phase 2 - linux check-green only (no runtime enable):

1. ungate [dev-dependencies] test-support; add ubuntu-latest CI job: cargo check + headless nextest for desktop; runtime still exits(1) off-mac.
2. prevent re-entrenching mac-isms.
   verify: CI green on ubuntu; mac packaging unchanged.

phase 3 - only on demand signal: linux preview tarball, no auto-update, documented gaps (tray/fonts/IME). defer windows MSIX/Authenticode/SmartScreen + installer/updater until paying/high-volume ask. revisit web client (gpui-fast-web or thin web over desktop-protocol) if need is view/control-from-anywhere.
verify: manual QA checklist (WCAG AA 4.5:1, 20 locales, keyboard/focus/Escape, trash fail-closed, launch argv boundary).

non-goals: no windows packaging/signing now; no auto-update for linux preview; no behavior change on mac.
