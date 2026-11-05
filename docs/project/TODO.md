routine: .vtcode/memory/issues.md

---

benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

https://developers.openai.com/api/docs/guides/agents-api/tools/computer-use

---

layout preference (zen vs non-zen) modes should be persisted between apps launches. not override

---

gpui apps for other plqtforms


---

fix the alignments '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 12.31.15 PM.png'


----

CRITICAL: check apply_patch tool. it seems the tool having a lot of error and trouble for vtcode to work fast/reliable/and smooth. a normal patch seems having a lot of errors and round trip before the patch can be apply. Help me reason through this problem step by step. Review the context, challenge assumptions, explore solutions and trade-offs, and think bigger about the overall design. Recommend the simplest practical approach before implementation.

session: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/sessions/session-vtcode-20261105T052554Z_128122-45584

---

implement lazy loading for threads data and heavy listing for all places on desktop app.

---

fix and match the force stop run and background processs to match the idle run button on the chat input design. also update all the primary/secondary/iterary/ghost design to follow macos 27 modern design guideline and make it more premium. current design are too bland. build and update the app componnets again

--

adjust the main container width for comformtable read.

---

click on a notification should deeplink and open to the related information.

---

remove the icons in tool summarization '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 10.35.25 AM.png'

---
---


implement /doctor command. https://code.claude.com/docs/en/skills#check-your-setup-with-/doctor

---

cli: improve line height markdown tui and spacing, currently the text and paragraphs seems too cramp and dense. can we improve to make it comfortable to read on the TUI ternimal? '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 11.17.05 AM.png'

---

gpui-fast-linux, gpui-fast-windows, gpui-fast-web, gpui-fast-macos
    
---

gpui-web ?

---

check and fix reverse history search (control+r) for desktop app should use same logic from the cli.

---

implement a simple markdown preview for desktop app.

---

implement terminal for desktop app.

--

improve desktop shimmer text by lowering the text opacity, keep on the shimmer bright

---

for search, it should able to deeplink to specific item when click, and scroll to and then highlight it for a brief moment.

---

desktop and cli, implement enter to jump to latest message, but only when chat input is not-empty.

---

---

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

https://github.com/vinhnx/VTCode/issues/803

---
---

note integrate pele to desktop app

---
---
---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/dropdown_button/

---

https://gpui-kit.com/component/toolbar/

---

https://gpui-kit.com/component/title-bar/

---
---

improve existing desktop shimmer https://gpui-kit.com/component/shimmer/

---

revamp existing manual build settings/config screen with https://gpui-kit.com/component/settings/ for more consistency

---
---

check the mac icons, it seems missing the macos 27 subtle icon borders? '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 3.25.21 PM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 3.25.06 PM.png'


---

update the overall desktop icon size baseline, make sure the icon is not too small not too big, matching the font size UI settings baseline

----

fix app icon corner rounded is not smooth. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 2.27.39 PM.png'

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

add icons to quick command (command+k) https://gpui-kit.com/component/command

----

https://gpui-kit.com/component/editor/

----

allow customization diff view with settings configuration https://gpui-kit.com/component/diff/. check example `options` config

---

https://gpui-kit.com/component/empty/

---

https://gpui-kit.com/component/focus-trap/

---

check and fix splitting the two 1000+ line
  files — is large surgery in view/
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

gpui-web
