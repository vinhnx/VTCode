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

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

https://github.com/vinhnx/VTCode/issues/803

---

improve the transcript UI

1. remove the marker deviders for the messages '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 3.46.19 PM.png'
2. Implement auto grouped summarization when turn end and finialized. Idea: when the agent complete the turn with a a final summarization message. show it only as one agent message per user's message in the transcript. then group all the work log, tool call thinking and trace under a collapsible group UI. reference: Codex OpenAI.
a. collapsed  '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 3.46.42 PM.png'. b. expanded: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 3.49.06 PM.png'.
3. add a files changes summirzation component and show in the bottom of agent's message. idea: move the existing component for Utils pane and move here, CTA should just deep link to the Utils pane/tab. with files changes, views changes, and an Undo changes mechanism if possible -> think and design this logic with a confirm.
4. Under that show a current clock like cli.

the goal is a clean focused and simplified minified transcript log only.

KISS and DRY.

Help me reason through this problem step by step. Review the context, challenge assumptions, explore solutions and trade-offs, and think bigger about the overall design. Recommend the simplest practical approach before implementation. 

---

note integrate pele to desktop app

https://pele.run/

---

advanced: implement live PTY Shell readonly viewer component for live stdio/error events with short and condensed view in the desktop app. check VT Code's component and try to port to desktop app.

reference this design component from Codex. 

'/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 3.40.47 PM.png'

replace the current JSON tool transcript display in the app with new component

---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/dropdown_button/

---

improve the design of the current HITL container component. maybe remove the yello warning background tone and refine and revamp each of the element and design. reference Codex OpenAI design: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-05 at 4.07.32 PM.png'

---

implement message queue/steer behavior and configuable in settings. this behavior shapes how message enter the queue when user press enter when a transcript loop is running. default to queue.


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
