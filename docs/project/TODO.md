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


implement /doctor command. https://code.claude.com/docs/en/skills#check-your-setup-with-/doctor

---

update the HITL confirmation layout again. reference current design '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-06 at 2.25.32 PM.png'

1. no close button at the right top.
2. allow to explan and show the full plan proposal view as markdown view -> extract as a share component for later use. can open and add the markdown view as a side accessory view -> design this. Help me reason through this problem step by step. Review the context, challenge assumptions, explore solutions and trade-offs, and think bigger about the overall design. Recommend the simplest practical approach before implementation. 

---

gpui-fast-linux, gpui-fast-windows, gpui-fast-web, gpui-fast-macos
    
---

gpui-web ?

---
---

desktop: change the main start/cancel button in the chat input with a shimmer effect button.

---

desktop: change the main start/cancel button in the chat input with a shimmer effect button.

---

implement a simple markdown preview for desktop app.

---

implement terminal for desktop app.

--

for search, it should able to deeplink to specific item when click, and scroll to and then highlight it for a brief moment.

---

desktop and cli, implement enter to jump to latest message, but only when chat input is not-empty.

---

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

https://github.com/vinhnx/VTCode/issues/803

---

note integrate pele to desktop app

https://pele.run/

---
---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/dropdown_button/

------

implement message queue/steer behavior and configuable in settings. this behavior shapes how message enter the queue when user press enter when a transcript loop is running. default to queue.


---

https://gpui-kit.com/component/toolbar/

---

https://gpui-kit.com/component/title-bar/

---

improve existing desktop shimmer https://gpui-kit.com/component/shimmer/

---

revamp existing manual build settings/config screen with https://gpui-kit.com/component/settings/ for more consistency

---
---
---
----
---

use this https://gpui-kit.com/component/plot/ and https://gpui-kit.com/component/chart/ components for all data visualization

1. check current charts and replace with new plot components where needed.
2. ensure consistent styling and spacing.
3. test with different data sets.
4. check responsiveness for different screen sizes.
5. update any outdated chart references.

---

use https://gpui-kit.com/component/pagination/

1. replace current pagination with new component.
2. check styling and spacing.
3. test navigation with new pagination.
4. check for any edge cases.
5. ensure it works with different data lengths.

---

https://gpui-kit.com/component/notification/ improve current in-app notification infra to implement deeplink to relevant info based on a notification

1. design a proper notification payload
2. implement deeplink handling in the app.
3. test notifications with different scenarios.
4. check deeplink navigation.
5. handle any errors gracefully.

--

implement user's message bubble message with rich content when user's message has attachement -> '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 2.38.16 PM.png' check https://gpui-kit.com/component/bubble/

---

implement custom theme https://gpui-kit.com/component/color-picker/ with color picker configuration for desktop app. allow user to create their own preferred theme. allow to save theme with name and add or delete it. have a new custom theme group from current theme selection in the settings

---

https://gpui-kit.com/component/editor/

----

allow customization diff view with settings configuration https://gpui-kit.com/component/diff/. check example `options` config

---

https://gpui-kit.com/component/empty/

---

https://gpui-kit.com/component/focus-trap/

---

check and fix splitting the two 1000+ line files — is large surgery in view/

---

improve the desktop app message queue UI. check and reference '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 3.34.09 PM.png'

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

## Plan (2026-11-06): centralize prompt budget caps / move goldens to disk files
- Context: one guidance sentence pins assertions in 4 test files
  (budget_cache_measure, content_and_dynamic, context_and_identity,
  prompt_shape); every prompt tweak pays this tax.
- Proposal: centralize caps or move goldens to disk files (DRYer).
- Status: deferred deliberately -- bigger refactor with its own review cost.
  Commit b8dd0e50d followed the existing raise-with-measured-value
  convention instead (simplest correct move).
