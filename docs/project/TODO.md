benchmark VT Code https://chatgpt.com/c/6ae5d291-8374-83ec-a83d-e075e1804da5

---

support computer use

https://developers.openai.com/api/docs/guides/tools-computer-use

https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool

---

add visual sync for the utils pane '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 10.47.11 AM.png' also add a button to close it.

1. remove the left line border
2. make the utils pane resizable and collapsible to hide when not needed.
3. round the corners for better look, sync with overall style.
4. add close button functionality.

---

for the activity log, reverse the list with newest on top. and maybe add a filter option. also auto scroll to new entries. '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 10.46.12 AM.png'

---

settings: '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 10.45.22 AM.png'

1. the edges/padding/margins between the left sidebar and the section containers are too tight, add some spacing.
2. maybe increase spacing inside each section a bit.
3. check alignment of labels and inputs.
4. maybe add some spacing between sections.
5. check if the section headers are visually distinct enough.

---

modernize the confirmation human in the loop UI '/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 10.44.53 AM.png'.

1. make it premium designs.
2. make the buttons more stylish.
3. maybe add some subtle shadows.
4. check spacing around the buttons.
5. maybe add some hover effects for buttons.
6. check overall spacing and alignment.

---

also refine the overall design system for more modern macos 27 look and feels. check apple guidelines.

---

desktop app: check file viewer settings doesn't save on selected and not applied on subsequent launches.

---

desktop app: check and make sure all settings are applied and saved correctly.

---

desktop app: implement keyboard navigation for threads panel. also consider adding keyboard navigation for other panels for a non-mouse user.

---

check and fix desktop themes doesn't grouped by modes (light/dark) in the settings

'/Users/vinhnguyenxuan/Documents/vtcode-resources/Screenshot 2026-11-02 at 11.01.13 AM.png'

---

For both cli and desktop app, show human-readable provider names in the model picker list instead of codes. also regroup and sort and move the frontier company and models to top. Move OpenAI, Anthropic to top. Then open source ones (HuggingFace, Ollama, LM Studio, etc).

---

click on a notification should deeplink and open to the related information.

---

revamp and refine model effort picker design, https://gpui-kit.com/component/slider/

maybe implement a vertical slider for effort selection. make it special and intuitive for users. premium feel.

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
