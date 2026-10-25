# Desktop component review

Reviewed 2026-10-24 against the [GPUI Kit component catalog](https://gpui-kit.com/component/).
All 79 linked component pages and their code-example blocks were retrieved;
repeated code lines were deduplicated for review. Applicable APIs were also checked
against locally resolved `gpui-kit 0.8.0` source. This is an API/example review,
not a claim that every component was instantiated or visually tested.

The supplied VT Code concept guides the design: quiet navigation,
conversation-first hierarchy, recessed inspector surfaces, compact native actions,
and a branded composer. Reuse behavior without replacing the existing runtime,
measured-list, approval, persistence, or accessibility owners.

| Component | Decision |
| --- | --- |
| [accordion](https://gpui-kit.com/component/accordion) | Retain app-owned pane visibility; no extra disclosure or animated layer needed. |
| [alert](https://gpui-kit.com/component/alert) | Retain inline error and draft-recovery state with existing ownership. |
| [alert-dialog](https://gpui-kit.com/component/alert-dialog) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. |
| [attachment](https://gpui-kit.com/component/attachment) | Applied: metadata cards and accessible file-removal actions. |
| [avatar](https://gpui-kit.com/component/avatar) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [badge](https://gpui-kit.com/component/badge) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [bubble](https://gpui-kit.com/component/bubble) | Applied: sender headers, outlined user prompts, unframed assistant responses, and copy footers. |
| [button](https://gpui-kit.com/component/button) | Retained and restyled: accessible buttons, packaged icons, and embedded brand artwork. |
| [calendar](https://gpui-kit.com/component/calendar) | Not needed for the current coding-workspace workflows or available data. |
| [carousel](https://gpui-kit.com/component/carousel) | Not needed for the current coding-workspace workflows or available data. |
| [chart](https://gpui-kit.com/component/chart) | Not needed for the current coding-workspace workflows or available data. |
| [checkbox](https://gpui-kit.com/component/checkbox) | Applied: controlled opt-in for untracked Git previews; preserve runtime-owned approval identities. |
| [clipboard](https://gpui-kit.com/component/clipboard) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [collapsible](https://gpui-kit.com/component/collapsible) | Retain app-owned pane visibility; no extra disclosure or animated layer needed. |
| [color-picker](https://gpui-kit.com/component/color-picker) | Applied to provider-supported reasoning effort with native step buttons and typed runtime updates. |
| [combobox](https://gpui-kit.com/component/combobox) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. |
| [command](https://gpui-kit.com/component/command) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. |
| [data-table](https://gpui-kit.com/component/data-table) | Not needed for the current coding-workspace workflows or available data. |
| [date-picker](https://gpui-kit.com/component/date-picker) | Not needed for the current coding-workspace workflows or available data. |
| [description-list](https://gpui-kit.com/component/description-list) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [dialog](https://gpui-kit.com/component/dialog) | Applied: larger Appearance overlay, heading parts, contained focus, and focus restoration. |
| [diff](https://gpui-kit.com/component/diff) | Applied: retained session and scoped Git snapshot review, unified/split modes, soft wrapping, and bounded raw fallback. |
| [dock](https://gpui-kit.com/component/dock) | Retain tested three-pane resizing and persistence; avoid unnecessary docking migration. |
| [dropdown_button](https://gpui-kit.com/component/dropdown_button) | A standard thread menu is sufficient; no split-button behavior needed. |
| [editor](https://gpui-kit.com/component/editor) | Deferred: richer navigation/editing is a separate functional increment. |
| [empty](https://gpui-kit.com/component/empty) | Applied: quiet branded welcome; real workspace/model actions remain in navigation and the composer. |
| [focus-trap](https://gpui-kit.com/component/focus-trap) | Applied: larger Appearance overlay, heading parts, contained focus, and focus restoration. |
| [form](https://gpui-kit.com/component/form) | Use current grouped, bounded preference controls; do not introduce a parallel global settings store. |
| [group-box](https://gpui-kit.com/component/group-box) | Applied: grouped typography controls. |
| [hover-card](https://gpui-kit.com/component/hover-card) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [icon](https://gpui-kit.com/component/icon) | Retained and restyled: accessible buttons, packaged icons, and embedded brand artwork. |
| [image](https://gpui-kit.com/component/image) | Retained and restyled: accessible buttons, packaged icons, and embedded brand artwork. |
| [input](https://gpui-kit.com/component/input) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [input-group](https://gpui-kit.com/component/input-group) | Applied: shared composer frame with block-end controls. |
| [kbd](https://gpui-kit.com/component/kbd) | Retain current command and runtime surfaces; icon controls keep accessible names and shortcut hints. |
| [label](https://gpui-kit.com/component/label) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [list](https://gpui-kit.com/component/list) | Retain keyboard-capable navigation buttons; resolved SidebarMenu rows do not offer equivalent keyboard handling. |
| [marker](https://gpui-kit.com/component/marker) | Do not invent execution percentages or phases beyond existing runtime events. |
| [menu](https://gpui-kit.com/component/menu) | Applied: one overflow trigger per chrome area uses canonical guarded new/close commands. |
| [message](https://gpui-kit.com/component/message) | Applied: sender headers, outlined user prompts, unframed assistant responses, and copy footers. |
| [message-scroller](https://gpui-kit.com/component/message-scroller) | Retain measured transcript ListState and existing anchoring/following behavior. |
| [notification](https://gpui-kit.com/component/notification) | Retain inline error and draft-recovery state with existing ownership. |
| [number-input](https://gpui-kit.com/component/number-input) | Use current grouped, bounded preference controls; do not introduce a parallel global settings store. |
| [otp-input](https://gpui-kit.com/component/otp-input) | Not needed for the current coding-workspace workflows or available data. |
| [pagination](https://gpui-kit.com/component/pagination) | Not needed for the current coding-workspace workflows or available data. |
| [plot](https://gpui-kit.com/component/plot) | Not needed for the current coding-workspace workflows or available data. |
| [popover](https://gpui-kit.com/component/popover) | Replace crowded Appearance popover with a larger modal surface. |
| [progress](https://gpui-kit.com/component/progress) | Do not invent execution percentages or phases beyond existing runtime events. |
| [questionnaire](https://gpui-kit.com/component/questionnaire) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. |
| [radio](https://gpui-kit.com/component/radio) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. |
| [rating](https://gpui-kit.com/component/rating) | Not needed for the current coding-workspace workflows or available data. |
| [resizable](https://gpui-kit.com/component/resizable) | Applied standalone Kit handles with splitter semantics and keyboard resizing; preserve custom width bounds/persistence instead of migrating independent panes into a panel group. |
| [root](https://gpui-kit.com/component/root) | Applied: `gpui_kit::open_window` owns the single Root; initialize Kit before opening, and retain tested native menu/window/reopen lifecycle. |
| [scrollable](https://gpui-kit.com/component/scrollable) | Applied themed Kit scrollbars to settings and file preview; recent sessions use a virtual list with a Kit scrollbar. |
| [select](https://gpui-kit.com/component/select) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. |
| [settings](https://gpui-kit.com/component/settings) | Use current grouped, bounded preference controls; do not introduce a parallel global settings store. |
| [sheet](https://gpui-kit.com/component/sheet) | Applied: Settings uses the Root-managed right Sheet for responsive sizing, built-in focus trapping, Escape/outside dismissal, and close behavior; the inspector remains an ordinary workspace pane. |
| [shimmer](https://gpui-kit.com/component/shimmer) | Do not add decorative loading/motion without an owned runtime state and reduced-motion behavior. |
| [sidebar](https://gpui-kit.com/component/sidebar) | Applied its labeled navigation structure to the rail and workspace/thread panel. Keep VT Code's keyboard-capable Kit Button rows and virtualized session list; Kit SidebarMenu rows are click-only tree items without equivalent keyboard handling. |
| [skeleton](https://gpui-kit.com/component/skeleton) | Applied to the owned Git review pending state; preserve its status label and render static theme-colored placeholders when reduced motion is enabled because Kit's Skeleton pulse is unconditional. |
| [slider](https://gpui-kit.com/component/slider) | Applied to persisted status-bar visibility; disabled/default state remains native. |
| [speech](https://gpui-kit.com/component/speech) | Deferred: microphone/recognition integration is outside this UI pass. |
| [spinner](https://gpui-kit.com/component/spinner) | Do not add decorative loading/motion without an owned runtime state and reduced-motion behavior. |
| [status-bar](https://gpui-kit.com/component/status-bar) | Applied Kit's left/right regions to the optional workspace and runtime status row; retain VT Code palette colors, the labeled status group, and the live agent-status announcement. |
| [stepper](https://gpui-kit.com/component/stepper) | Applied only to ordered multi-step runtime questions; keep the step order and current index runtime-owned, and disable direct navigation when the runtime requires sequential answers. Never invent execution phases or percentages. |
| [switch](https://gpui-kit.com/component/switch) | Applied: separate light/dark appearance profiles have persisted theme and typography effects with accessible state. |
| [table](https://gpui-kit.com/component/table) | Not needed for the current coding-workspace workflows or available data. |
| [tabs](https://gpui-kit.com/component/tabs) | Applied Kit TabBar with tracked reveal for keyboard/programmatic selection; retain scoped keyboard actions and VT Code-owned content. |
| [tag](https://gpui-kit.com/component/tag) | Applied: neutral provider metadata and explicit tool-status tags in recessed activity cards. |
| [text-view](https://gpui-kit.com/component/text-view) | Applied Base TextView with reduced-motion-aware assistant stream fades; preserve syntax highlighting, selection/copy, web-only links, and blocked transcript images. |
| [textarea](https://gpui-kit.com/component/textarea) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [theme](https://gpui-kit.com/component/theme) | Applied: neutral hover/selection distinct from brand primary; component surfaces follow VT Code palettes. |
| [time-field](https://gpui-kit.com/component/time-field) | Not needed for the current coding-workspace workflows or available data. |
| [title-bar](https://gpui-kit.com/component/title-bar) | Retain one kit Root and tested native menu/window/reopen lifecycle. |
| [toggle](https://gpui-kit.com/component/toggle) | Applied to model speed selection as a controlled, accessible toolbar toggle; preference controls continue to use Switch. |
| [toolbar](https://gpui-kit.com/component/toolbar) | Applied to the header workspace actions with named grouping, compact Kit controls, and arrow-key focus navigation. |
| [tooltip](https://gpui-kit.com/component/tooltip) | Retain current command and runtime surfaces; icon controls keep accessible names and shortcut hints. |
| [tree](https://gpui-kit.com/component/tree) | Deferred: richer navigation/editing is a separate functional increment. |
| [virtual-list](https://gpui-kit.com/component/virtual-list) | Retain measured transcript ListState and existing anchoring/following behavior. |
