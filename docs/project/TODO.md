benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

---

also refine the overall design system for more modern macos 27 look and feels. check apple guidelines.

---

implement lazy loading for threads data and heavy listing for all places on desktop app.

---

fix tool call display being raw on the HITL popup on desktop. can we syntax highlight it? '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 9.50.38 AM.png'

---

check and improve the model reasoning effort UI. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 10.23.13 AM.png' it doesn't look good. find another UI component for more modern and premium. also use theme's accent. make it easy to use

--

check stop and cancel active run on desktop doesn't work and doesn't end the session. the element and loading global state still showing as active running. find and fix and improve the force stop active loop

---

check content overflow on HITL popup if content is too wide '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 9.46.09 AM.png'

---

adjust the main container width for comformtable read.

---

click on a notification should deeplink and open to the related information.

---
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

check auto mode seems can't use web search tool? /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/sessions/session-vtcode-20261102T044638Z_528994-20422

---

https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0

---

contribute back to OSS with learning from this project. check gotchas and decicions.

---

https://github.com/vinhnx/VTCode/issues/803

---

implement desktop app file brower with core fast logic from CLI. and implement proper and fast files brower for macos. current the brower is basic. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 7.40.37 AM.png'. check https://gpui-kit.com/component component kit. maybe use Tree component https://gpui-kit.com/component/tree/.

---

check for all of the panels behavior and layout. when clicking away, they should close or hide properly. and make sure no overlap occurs.

fix: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-03 at 7.40.37 AM.png'


---

note integrate pele to desktop app

---

update readme and docs show badges and bullet list all vtcode platforms

---
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
---

use this https://gpui-kit.com/component/plot/ and https://gpui-kit.com/component/chart/ components for all data visualization

1. check current charts and replace with new plot components where needed.
2. ensure consistent styling and spacing.
3. test with different data sets.
4. check responsiveness for different screen sizes.
5. update any outdated chart references.

---

check and fix the alignment of the loading "Running" it seems misaligned. or it is because the chat input container itself is misaligned? '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 1.33.54 PM.png' '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-04 at 1.33.21 PM.png'
check the input container padding and margin to aligned with the whole transcript layout fit

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
  files — is large surgery in view/
