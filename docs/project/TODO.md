benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

---

also refine the overall design system for more modern macos 27 look and feels. check apple guidelines.


---

click on a notification should deeplink and open to the related information.

---

for zen mode, when activated, hide distractions and focus on the main content. move and group the chat input accessory buttons like models/effort, build modes, approval modes together. keep it minimal and simple with clean layout, and must focused on the main content. and easy to toggle on/off.

---

add current time at each agent's message response. reference: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 3.12.46 PM.png'. also implement for the CLI replacing current status clock.

---

implement context menu on files for quick actions like open, delete, rename. maybe also add copy path. available in desktop app where the file is shown.

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

check and fix TUI

```

Tool 'task_tracker' failed. Expand Details for recovery
steps.
Details (click or F8 to expand)
```

when the thread message runloop is running, clicking on this doesn't do anything. may need to handle it differently in the UI for both clid and desktop. for the context for the agent, it should still show relevant info.

---

check auto mode seems can't use web search tool? /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/sessions/session-vtcode-20261102T044638Z_528994-20422

---

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

desktop app: check chat input labels seems not read and respect the font settings? the label size is fixed and not dynamic -> fix it

---

https://github.com/vinhnx/VTCode/issues/803

---

implement desktop app file brower with core fast logic from CLI. and implement proper and fast files brower for macos. current the brower is basic. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 7.40.37 AM.png'. check https://gpui-kit.com/component component kit. maybe use Tree component https://gpui-kit.com/component/tree/.

---

check for all of the panels behavior and layout. when clicking away, they should close or hide properly. and make sure no overlap occurs.

fix: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 7.40.37 AM.png'

---

check and fix desktop to cli handoff doesn't work, it seems, it just open Terminal and doesn't do anything. it just show as CTA, for security can we launch it differently?

error:

```
Error: session session-vtcode-20261103T010100Z_820673-41390 is already open in another process; continue there or fork a new session with --session-id <suffix>
```

1. for security can we directly launch the terminal differently? '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 8.04.43 AM.png'
2. check for properly passing context to CLI when handoff occurs.
3. make sure CLI can handle it correctly.
4. test the handoff flow end-to-end.
5. check error handling if CLI fails.
6. also check cli -> desktop handoff works.
7. test with different contexts.
8. also for desktop CLI handoff, also add option to choose which terminal to use.

---

try to fix Ask Deep Wiki badge keep falling on github remote markdown, though it can render fine locally. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 7.41.56 AM.png'

---

note integrate pele to desktop app

---

update readme and docs show badges and bullet list all vtcode platforms

---

use marker to refine and build all current components in the desktop app https://gpui-kit.com/component/marker/. idea

1. port the TUI info/warning/error message to use "Status" marker component for labels info
2. check other marker components example for idea and replace and build where suitable. '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.43 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.40 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.21 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.14 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.11 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.08 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.52.04 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.51.48 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.51.46 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.51.20 AM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/markers/Screenshot 2026-11-03 at 9.51.07 AM.png'

---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/dropdown_button/

---

https://gpui-kit.com/component/toolbar/

---

https://gpui-kit.com/component/title-bar/

---

build and replace/revamp current desktop app components where suitable https://gpui-kit.com/component/tabs/

1. revamp all current tabs layout for more unified and consistent look accross the app. Prefer using the "Folder tabs" design.
2. size medium for better readability.
3. max width: unlimited
4. implement including more menu options if needed.

---

improve existing desktop shimmer https://gpui-kit.com/component/shimmer/

---

revamp existing manual build settings/config screen with https://gpui-kit.com/component/settings/ for more consistency

---

revampe desktop app HITL confirmation popup shared component with https://gpui-kit.com/component/questionnaire/

1. remove the main hitl container border.
2. keep the button and layout consistent with the new design.
3. update the text and labels to match the new style.
4. check alignment and spacing for consistency.
5. test the popup in different scenarios.
6. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 11.19.09 AM.png' refernce this style from example

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

update and change the current button design system again, currently it doesn't look as premium and good. https://gpui-kit.com/component/button/.

design a modern button with macos 27 design guideline

---

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
  files — is large surgery in view/, where the peer
