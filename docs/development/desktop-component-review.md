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
| [chart](https://gpui-kit.com/component/chart) | Applied: storage usage breakdown as a labeled 4-category bar chart, replacing the hand-rolled proportional bar; axes, grid, tooltips, and interaction stay off, and the full breakdown remains in the container aria label. |
| [checkbox](https://gpui-kit.com/component/checkbox) | Applied: controlled opt-in for untracked Git previews; preserve runtime-owned approval identities. |
| [clipboard](https://gpui-kit.com/component/clipboard) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [collapsible](https://gpui-kit.com/component/collapsible) | Retain app-owned pane visibility; no extra disclosure or animated layer needed. |
| [color-picker](https://gpui-kit.com/component/color-picker) | Applied to provider-supported reasoning effort with native step buttons and typed runtime updates. |
| [combobox](https://gpui-kit.com/component/combobox) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. |
| [command](https://gpui-kit.com/component/command) | Applied: retained filtered context/model palettes and conversation search; stable message/activity IDs jump through the measured MessageScroller, reveal tool details, open captured output in the inspector, and load bounded older-history pages without replacing the archive pager. |
| [data-table](https://gpui-kit.com/component/data-table) | Not needed for the current coding-workspace workflows or available data. |
| [date-picker](https://gpui-kit.com/component/date-picker) | Not needed for the current coding-workspace workflows or available data. |
| [description-list](https://gpui-kit.com/component/description-list) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [dialog](https://gpui-kit.com/component/dialog) | Applied: larger Appearance overlay, heading parts, contained focus, and focus restoration. |
| [diff](https://gpui-kit.com/component/diff) | Applied: retained session and scoped Git snapshot review, unified/split modes, soft wrapping, bounded raw fallback, and explicit unavailable/partial states from runtime file metadata. |
| [dock](https://gpui-kit.com/component/dock) | Retain tested three-pane resizing and persistence; avoid unnecessary docking migration. |
| [dropdown_button](https://gpui-kit.com/component/dropdown_button) | A standard thread menu is sufficient; no split-button behavior needed. |
| [editor](https://gpui-kit.com/component/editor) | Deferred: richer navigation/editing is a separate functional increment. |
| [empty](https://gpui-kit.com/component/empty) | Applied: branded welcome with real browse/review actions. |
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
| [menu](https://gpui-kit.com/component/menu) | Applied: thread overflow uses canonical guarded new/close commands. |
| [message](https://gpui-kit.com/component/message) | Applied: sender headers, outlined user prompts, unframed assistant responses, and copy footers. |
| [message-scroller](https://gpui-kit.com/component/message-scroller) | Retain measured transcript ListState and existing anchoring/following behavior. |
| [notification](https://gpui-kit.com/component/notification) | Applied system-only notices for inactive-window run completion, failure/blocking, and pending decisions; preserve transcript/interaction state as the source of truth and route clicks through the originating Kit window. |
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
| [settings](https://gpui-kit.com/component/settings) | Converged, container not adopted (2026-11-03): settings groups use the Kit gap_4 air rhythm with no inter-row dividers (pinned by `settings_group_separates_rows_by_gap_4_air`), and each tab owns a per-pane module under `view/settings_*.rs`. The container stays out: pinned 0.8.1 keeps page/group rendering crate-private, selection state internal (no controlled selection for deep links), sidebar rows click-only, and typed fields bind App globals — 0.9.0 changes none of this. No parallel global settings store. Codex pass: nav icons via `SettingsTab::icon` plus the shared `nav_entry_content` rail builder; scrolling content H1 (`content_page_title` tier, Heading role, inside the scroll region to preserve geometry); intra-card dividers deliberately not reintroduced (small groups scan with air; revisit per-group if density grows). |
| [sheet](https://gpui-kit.com/component/sheet) | Retired for Settings: the unified full-screen Settings route (left navigation, content, Back/Escape/⌘[ dismissal to the workspace with composer focus) replaces the Root-managed right Sheet, so body-level actions stay reachable and shortcut capture suppresses bindings by omitting key contexts while armed; the inspector remains an ordinary workspace pane. |
| [shimmer](https://gpui-kit.com/component/shimmer) | Applied: `ShimmerText` status labels through the shared `shimmer_label` builder, each tied to an owned runtime state (turn/history/navigation/storage/preview/output loading) with Kit's reduced-motion static path; keep status roles and aria labels on the owning row. |
| [sidebar](https://gpui-kit.com/component/sidebar) | Applied its labeled navigation structure to the rail and workspace/thread panel. Keep VT Code's keyboard-capable Kit Button rows and virtualized session list; Kit SidebarMenu rows are click-only tree items without equivalent keyboard handling. |
| [skeleton](https://gpui-kit.com/component/skeleton) | Applied to owned pending states (Git review, config loading, file scan, history paging, turn pending) through the shared `skeleton_line`; preserve its status label and render static theme-colored placeholders when reduced motion is enabled because Kit's Skeleton pulse is unconditional. |
| [slider](https://gpui-kit.com/component/slider) | Applied to persisted status-bar visibility; disabled/default state remains native. |
| [speech](https://gpui-kit.com/component/speech) | Deferred: microphone/recognition integration is outside this UI pass. |
| [spinner](https://gpui-kit.com/component/spinner) | Do not add decorative loading/motion without an owned runtime state and reduced-motion behavior. |
| [status-bar](https://gpui-kit.com/component/status-bar) | Applied Kit's left/right regions to the optional workspace and runtime status row; retain VT Code palette colors, the labeled status group, and the live agent-status announcement. |
| [stepper](https://gpui-kit.com/component/stepper) | Applied only to ordered multi-step runtime questions; keep the step order and current index runtime-owned, and disable direct navigation when the runtime requires sequential answers. Never invent execution phases or percentages. |
| [switch](https://gpui-kit.com/component/switch) | Applied: separate light/dark appearance profiles have persisted theme and typography effects with accessible state. |
| [table](https://gpui-kit.com/component/table) | Not needed for the current coding-workspace workflows or available data. |
| [tabs](https://gpui-kit.com/component/tabs) | Applied Kit TabBar with tracked reveal for keyboard/programmatic selection; retain scoped keyboard actions and VT Code-owned content. |
| [tag](https://gpui-kit.com/component/tag) | Applied: neutral provider metadata and explicit tool-status tags in recessed activity cards. |
| [text-view](https://gpui-kit.com/component/text-view) | Applied Base TextView with reduced-motion-aware assistant stream fades; preserve syntax highlighting, selection/copy, web-only links, and blocked transcript images. HTML mode (`TextView::html`, verified in pinned 0.8.0 `text/format/html.rs`: `style`/`script` elements are dropped, no JS engine) is approved for MCP Apps static previews only: same hardening as Markdown (blocked images, web-only links, themed code blocks), bodies arrive through the bounded `mcp/app/read` service call, rows stay measurable via a 6,000-char head cap with a truncation notice. No interactive app engine; app-originated `tools/call` enforcement and round-trips stay deferred. Model-side `visibility: ["app"]` enforcement is live host-wide (hidden from listings, rejected on direct call). |
| [textarea](https://gpui-kit.com/component/textarea) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [theme](https://gpui-kit.com/component/theme) | Applied: neutral hover/selection distinct from brand primary; component surfaces follow VT Code palettes. Syntax *token* colors stay on the Kit mode default (`ThemeStyle` is unconstructible in pinned 0.8.0, so per-palette `highlight_theme` is not possible); only code-block surfaces follow the palette. |
| [time-field](https://gpui-kit.com/component/time-field) | Not needed for the current coding-workspace workflows or available data. |
| [title-bar](https://gpui-kit.com/component/title-bar) | Retain one kit Root and tested native menu/window/reopen lifecycle. |
| [toggle](https://gpui-kit.com/component/toggle) | Applied to model speed selection as a controlled, accessible toolbar toggle; preference controls continue to use Switch. |
| [toolbar](https://gpui-kit.com/component/toolbar) | Applied to the header workspace actions with named grouping, compact Kit controls, and arrow-key focus navigation. |
| [tooltip](https://gpui-kit.com/component/tooltip) | Applied Kit-managed tooltips to action buttons; shortcut hints resolve from the active keymap, while icon controls retain explicit accessible names. Keep fixed Enter guidance only where the action is not configurable. |
| [tree](https://gpui-kit.com/component/tree) | Applied: workspace browsing uses the Kit Tree with stable relative-path IDs, keyboard expand/select, bounded discovery, and read-only previews. |
| [virtual-list](https://gpui-kit.com/component/virtual-list) | Applied to changed-file, recent-thread, and activity rows with stable IDs and accessible list position/selection semantics; Activity discloses its 300-row window and routes older-history discovery to bounded conversation search. Transcript retains measured MessageScroller geometry and existing anchoring/following behavior. |
