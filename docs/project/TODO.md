routine: .vtcode/memory/issues.md

---

benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

https://developers.openai.com/api/docs/guides/agents-api/tools/computer-use

---

gpui apps for other plqtforms

---

gpui-fast-linux, gpui-fast-windows, gpui-fast-web, gpui-fast-macos

---

gpui-web ?

---

---
implement a simple markdown preview for desktop app.

---

implement standalone terminal for desktop app.

--
---

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---
---

note integrate pele to desktop app

https://pele.run/

---

---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/dropdown_button/


---

https://gpui-kit.com/component/toolbar/

---

https://gpui-kit.com/component/title-bar/

---

improve existing desktop shimmer https://gpui-kit.com/component/shimmer/

---

revamp existing manual build settings/config screen with https://gpui-kit.com/component/settings/ for more consistency


---
--

https://gpui-kit.com/component/editor/

---
---

https://gpui-kit.com/component/empty/

---

https://gpui-kit.com/component/focus-trap/

---

check and fix splitting the two 1000+ line files — is large surgery in view/

---

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

---
