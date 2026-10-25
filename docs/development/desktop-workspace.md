# VT Code Desktop Workspace

VT Code Desktop is a macOS 15+ GPUI Kit application backed by the matching local `vtcode desktop-server` executable. The
GUI sends typed JSON-RPC commands; the existing unified runloop remains responsible for provider calls, tool policy,
verification, cancellation, and durable session events. The desktop service does not initialize a terminal or emit
terminal presentation output to its protocol stream.

## Build and launch

For local development, build the service and launch the GUI from the workspace root:

```sh
cargo build --locked -p vtcode
cargo run --locked -p vtcode-desktop
```

Choose a workspace folder in the app. The desktop client starts `vtcode desktop-server` with an absolute workspace path
and uses the existing VT Code configuration, provider credentials, workspace trust, and sandbox policy. If startup
reports missing credentials, configure the provider from Terminal with the command shown by the app, then restart it.
Credential entry and OAuth onboarding are not part of this version.

To make a local application bundle on macOS 15 or newer:

```sh
scripts/package-desktop-macos.sh
```

The optional first argument selects the output `.app` path. The script builds the release GUI and matching service,
places the service in `Contents/Helpers`, signs both executables before signing the app, and verifies the bundle. The
default output is `dist/VT Code.app`; the script will not overwrite an existing output. Ad hoc signatures support local
development only. They do not establish Developer ID trust or notarize a downloaded app; see [Apple Developer ID](https://developer.apple.com/developer-id/).

The GUI crate is non-publishable and outside Cargo's default members. It uses the pinned [`gpui-kit` 0.8.0 setup](https://gpui-kit.com/docs/installation/).

## Runtime and session behavior

The app owns one selected workspace, one local service process, and at most one active turn. Workspace or session changes
wait for active work to cancel and its runloop to clean up. Closing the app or disconnecting the protocol also cancels
owned work and pending interactions.

Session history is discovered from VT Code's existing durable stores. Selecting a previous session explicitly loads its
bounded text projection and resume state. A restart does not replay a submitted prompt or an approval automatically.
Until the archive persists its first-prompt preview, the sidebar names a new session from the first submitted prompt and
keeps that name across session-list refreshes. The session list reloads whenever a turn ends. A Close button deselects
the current session through `session/close` without starting a replacement.
The transcript follows new output while it is at the latest position and preserves the reader's position after they
scroll upward.

Session creation, loading, closing, and model changes run one at a time. While a transition is pending, submission,
attachments, and further session/model/workspace changes wait for its response. A first message waits for confirmed
session creation before any turn is sent. Failed creation or submission leaves the original content recoverable;
an empty prompt does not create a session.

## UI quick reference

The window opens with a compact navigation rail and the conversation. The project/thread sidebar and
context inspector start collapsed on each launch; stored pane widths are retained.

| Action | Behavior |
| --- | --- |
| Reopen the app | Dock or Finder relaunch activates the existing window. If the last window was closed, it opens a new window and restores the most recent existing workspace. |
| Connect to a workspace | Initialization validates the negotiated protocol before configuration and session discovery. Session commands and Send wait for this handshake; incompatible helpers disconnect with the initialization error preserved. |
| Use native menus | File, Edit, View, and Window menus dispatch the same typed actions as keyboard controls. Session commands disable while unavailable; Edit targets the focused text field. ⌘W closes the window, ⌘M minimizes, ⌘Q quits, and ⌘⇧N shows or reopens the main window. |
| Open Utilities | The Utilities inspector tab summarizes session changes, recent tool activity, and draft source attachments, with actions into Changes, Activity, Files, and previews. |
| Inspect tool activity | Expand a consecutive tool-action summary, then open an action for output. Running and failed counts remain visible while collapsed. |
| Read streamed output | GPUI measures variable-height rows and follows the bottom of the newest message, including a message taller than the viewport. |
| Scroll upward | Tail following pauses; returning to the bottom resumes it. Older rows evicted by the history bound preserve the remaining reader anchor. |
| Send a prompt | Enter submits the composer draft; Shift+Enter inserts a newline. Send creates a session automatically when none is selected, then submits the exact captured prompt and attachments. A newer draft typed while creation is pending stays in the composer. |
| Recover a submission | The app retains the submitted prompt and attachments until acknowledgement. A rejected request offers Restore draft, Copy prompt, and Dismiss; restoration requires an empty composer so newer work is preserved. A disconnect before acknowledgement warns that delivery is uncertain. Recovery never resends automatically. |
| Attach files | "+ Attach" opens a native multi-select picker limited to workspace UTF-8 text files (at most 5 per prompt, 64 KiB each). Oversized files are rejected rather than silently truncated. Send waits for attachment loading. Chips above the composer remove items; the transcript echoes attached paths. |
| Switch models | The composer's Model button opens a searchable kit Command picker, grouped by provider. Search names, providers, or exact IDs; rows show context token counts and preselect the current model, including configured models outside the built-in catalog. Arrow keys and Enter choose a model through `config/select`. Choosing the current model preserves the session. Escape clears a nonempty query, then dismisses without changes. |
| Expand tool details | The changed row is remeasured while the reading position is preserved. |
| Review file changes | File-change events render as a diff card: per-file kind, +/− counts, apply status, and a collapsible line-colored unified diff (mono font, bounded to 2,000 rendered lines) with copy-patch. |
| Inspect changes | The inspector's Changes tab aggregates every change-set: per-file kind and +/− counts, and selecting a file opens it in the kit's syntax-highlighted Diff viewer with Copy patch and Open file. |
| Browse files | The inspector's Files tab shows a read-only, collapsed tree (at most 400 entries, directory traversal through depth 8, dependency/VCS/hidden directories skipped). Tab to Workspace files and press Enter to focus the tree; Up/Down moves selection and Left/Right collapses or expands folders. Clicking a folder also toggles it. Enter or a click opens a file's UTF-8 preview (64 KiB cap). Expansion and selection survive refresh. Scans and reads run off the UI thread; paths and symlinks resolving outside the workspace are rejected. |
| Review current Git changes | Changes → Review working tree opens a read-only snapshot viewer. Choose an optional folder and untracked-text inclusion, then Load changes; Refresh updates the capture. |
| Follow activity | The inspector's Activity tab projects tool and command statuses for the session, retaining the newest 300 rows in chronological order. |
| Watch command output | The inspector's Output tab shows the newest live or restored command/tool output in mono, tail-capped to the last 1,000 lines. Copy preserves the complete capture when available; restored spool reads are capped at 32 KiB and show an omission notice when partial. If archived activity history is incomplete, Changes, Activity, and Output show an accessible warning even when no rows were recovered. Historical unfinished work is marked interrupted. |
| Toggle the inspector | The toolbar button shows or hides the panel; the palette has the same command. |
| Navigate inspector tabs | The kit underline TabBar exposes selected tabs. Tab focuses the inspector navigation; Left/Right cycle tabs, Home selects Changes, and End selects Output. |
| Start from the welcome view | The kit Empty surface offers Browse files and Choose model actions. Changes, activity, and output use matching empty-state layouts until session data arrives. |
| Open the command palette | ⌘K searches currently available session commands, view toggles, recent sessions (10), and file previews (50). Named accessible rows retain selection semantics; Tab stays inside. Enter confirms. Escape clears a nonempty query first, then dismisses and returns focus; a backdrop click also dismisses. |
| Expand navigation | The toolbar toggle or ⌘B pins/unpins the project/thread sidebar. Hover the rail’s project icon for a transient list; leaving navigation or pressing Escape dismisses the peek. Clicking the icon also pins it for keyboard use. |
| Configure shortcuts | Settings → Keyboard shortcuts edits seventeen main desktop commands. Invalid, duplicate, typing-only, and reserved macOS/editing combinations are rejected. Save/reset applies on restart; native menus and action-aware tooltips use active bindings. Tab/Shift+Tab and Enter/Space remain available on Kit controls. |
| Use keyboard commands | ⌘N creates a session, ⌘⇧W closes the selected session, ⌘O chooses a workspace, ⌘L focuses the composer, ⌘⇧I toggles the inspector, and ⌘. cancels the active run. The same active-turn and cleanup guards apply as for buttons. |
| Dismiss an overlay | Escape closes the palette or file preview and restores prior keyboard focus. ⌘L dismisses these overlays and focuses the composer. Escape alone does not cancel agent work. |
| Read the status bar | Workspace name, the workspace's git branch (walked up to the repository root; detached HEAD shortened; worktree pointers followed), the sandbox policy label, Context % (last turn's usage against the model's context window, hidden while unknown), the configured provider · model, and the agent running/idle state. When the bar is enabled, assistive technology receives the agent state as a status announcement. |
| Reduce motion | Settings → Accessibility offers System, On, and Off. System uses the current macOS accessibility setting at launch or when selected; explicit choices persist with Desktop preferences and apply to GPUI Kit animations. |
| Switch workspaces | The sidebar's Workspaces section lists recent projects (at most 8) for one-click switching; the most recent workspace reopens on launch when it still exists. |
| Resize panes | Drag the dividers between panes; sidebar and inspector widths clamp to 180–560px and persist. |
| Read with assistive technology | Conversation messages expose readable text without Markdown markers, tool activity groups expose their expanded state, expanded output has named accessibility nodes, composer errors are announced as alerts, and visible navigation status changes are announced. Tool actions identify their tool; active workspaces/threads and current typography values are included in accessible labels. |
| Copy a message or code block | Message copy retains the original Markdown; code copy contains only the code. Markdown formatting, links, lists, and tables render through GPUI Kit. |
| Follow a link | Explicit activation opens HTTP/HTTPS links. Other URL schemes are blocked, images are not fetched, and raw HTML is removed from rendered Markdown. |
| Choose a workspace | One asynchronous native picker opens at a time. Selection is rejected if a turn started or service cleanup is pending before the picker returns. |
| Add context | ⌘⇧A opens the composer-anchored file/skill/plugin/command search. Selection adds context or invokes an existing native action; it never submits. |
| Change runtime options | Composer permission and mode controls select validated CLI options for the next session. The model knob changes supported reasoning effort only. Active work blocks changes. |
| Toggle status bar | Hidden by default. Settings, the header menu, and ⌘⇧S share one saved visibility preference. |
| Sync theme to CLI | Settings → CLI preferences or ⌘⇧T saves the selected built-in theme with the CLI's layered config writer. New CLI sessions use the saved theme; Desktop switches to following the CLI theme after the save succeeds. |
| Reload CLI settings | Settings → CLI preferences or configurable ⌘⇧R restarts the idle service, reads CLI configuration layers, and preserves the draft and attached sources. Previous sessions remain in history. |
| Edit CLI settings | Settings → CLI configuration searches effective CLI values. Booleans toggle directly; strings/numbers use typed editors and arrays/maps use JSON. Choose User, Workspace, or Project; VT Code validates and version-checks the write, then reloads the service. Credential-bearing fields are excluded. |
| Open settings | The toolbar button, application menu, or ⌘, opens the same Settings dialog: a live theme picker (swatches with mini previews, see below), configured provider and model, a copy button for the credential setup command, and service and protocol versions. |

### Utilities pane

Choose **Utilities** in the context inspector for a compact workspace summary. Changes shows the current
session's addition/deletion totals and opens the existing Changes view. Recent activity shows running,
completed, failed, and other counts from the same bounded live/restored activity projection used by Activity.
These are tool-event counts, not a subagent roster; subagent status is not exposed by the desktop protocol.
Draft Sources lists files attached to the next message. Activate a source to preview its current workspace
file, use the plus button to attach files through the native picker, or browse the Files view. Sending or
clearing a draft updates this list with the existing composer attachment lifecycle. Utilities uses the same
keyboard-selectable inspector tabs and accessible named controls as the other views.

### Compact composer

The composer is centered with a 760-pixel width cap and a one-to-six-line growing text field. A small context strip
shows workspace selection, the CLI/WebMCP documentation link, configured agent mode, and Settings. Its
theme-derived surface contains attachment, actual execution-policy text, model selection, and send/cancel controls.
Footer controls wrap when space is limited; long model names are bounded. Empty or whitespace-only drafts cannot be
sent. Enter/Shift+Enter behavior, attachment delivery, approval requests, cancellation, and uncertain-delivery
draft recovery retain their existing ownership and safeguards.

### Composer context and runtime controls

**Add files and more @** (⌘⇧A) opens a filtered Kit Command popup above the composer. It includes the bounded
workspace file list, the existing native file picker, CLI skills, installed Agent Plugins, and native commands.
Skills insert their explicit `$skill-name` mention. Plugins insert a plain-language request to use the installed
plugin; this does not grant new access or install anything. Commands reuse the existing model, workspace,
inspector, and file-refresh actions. No prompt is submitted automatically. File selection reuses workspace
containment, UTF-8, 64 KiB, and five-file limits. Discovery uses shared CLI loaders on a blocking worker while
idle, a three-second request deadline, bounded wire metadata, and the retained query state.

The permission control (⌘⇧P) chooses **Ask before protected actions** or **Auto-approve commands**. It controls the
CLI's command confirmation flag and preserves sandbox/tool authorization. **Mode** (⌘⇧B) selects Build or Plan
through the CLI's primary-agent configuration and planning entry point. Mode, effort, and permission changes
require an idle service, validate before shutting down the old session, and apply to the next session. The current
draft and attached files survive the change, including model switches. Model changes wait for file loading. These
are helper-lifetime overrides; they do not write `vtcode.toml`. The mode label reports the configured desktop
session mode; changes made through in-chat CLI slash commands are not currently projected back into this control.

The status bar starts hidden. Settings → **Show status bar**, the header menu, or ⌘⇧S toggles it and saves the
choice. The header keeps one overflow control for thread/window actions. The former duplicate thread ellipsis is
removed. CLI/WebMCP opens the integration documentation; it does not claim a live WebMCP connection.

### Cursor and contrast reliability

The patched, pinned macOS backend immediately applies hover cursors for the key content view, without holding its
window-state lock across AppKit cursor-rectangle callbacks. This covers buttons, text fields, resize handles, and
returns from native menus. Disabled controls keep the arrow. See [the backend patch
notes](../../patches/gpui-pre-macos/PATCH.md).

The composer uses VT Code's recessed surface token. Muted text is tested at WCAG AA 4.5:1 on the page, composer,
popover, and chrome surfaces for every built-in palette, rather than only against the page background. Conversation
and thread-heading nodes have stable accessible IDs and labels. Knob step buttons retain Kit focus, keyboard
activation, disabled boundaries, and accessible labels.

### Native window chrome

The header uses Kit’s `TitleBar` over a transparent macOS title bar, with the real traffic-light controls
reserved at the left. Kit owns window dragging and delegates double clicks to the system title-bar behavior.
The header and navigation rail share a subtle theme-derived surface. Matching overflow buttons expose
existing thread, navigation, inspector, search, and Settings actions; unavailable session actions remain disabled.
Menus preserve the draft and retain Kit keyboard navigation and dismissal.

### Startup and desktop shortcuts

Ciapre remains the CLI and desktop default. New desktop profiles follow the CLI-resolved theme delivered
by `config/discover`. Existing explicit desktop theme choices are preserved during preference migration.
Choosing a swatch creates a desktop override; **CLI preferences → Use CLI theme** restores inheritance.
**Sync current theme to CLI** persists a built-in swatch to the active CLI config layer using the shared
`ConfigManager` writer, then starts the next session with that theme. System appearance swatches cannot
be written as CLI themes. A failed write leaves Desktop's current appearance unchanged.
Font families and sizes remain independent of theme inheritance, and theme updates preserve the draft.
Unsupported CLI themes produce an explicit error rather than silently substituting a palette.

**CLI configuration** reads a bounded, searchable projection of effective values from the shared `ConfigService`.
The value type is preserved: booleans toggle, strings and numbers use typed fields, and arrays or maps use a JSON
editor. Writes select the existing User, Workspace, or Project layer, carry its observed version for conflict
detection, and pass through the CLI's normal config validation and safe writer. Credential-bearing branches are
never sent to the GUI or accepted as write values; use `vtcode auth` and configured environment variables for
credentials. Large values and catalogs beyond the UI bound are omitted and called out in Settings. A successful
write restarts the local service to reload all CLI layers while retaining the draft, attached files, and history.
If a higher-precedence layer still overrides the saved value, Settings reports that after reloading.

**CLI preferences → Reload CLI settings** remains available to refresh configuration changed outside Desktop.
It starts a fresh conversation and resets helper-lifetime model, effort, and permission overrides. The editable
draft and attached files remain; previous sessions are available in history. Reload waits for service shutdown
and is unavailable during active turns, pending submissions, interactions, session changes, or file pickers/reads.
It also retries a disconnected service after configuration or authentication has been repaired outside the app.

Protocol 1.9 adds searchable settings read/write methods alongside typed built-in theme synchronization. The service
uses the CLI startup loader for provider, model, runtime policy, and agent execution.
Its non-secret `sharedUi` projection carries the resolved theme and equivalent shortcut overrides.
**Import CLI shortcuts** uses the CLI key parser, normalizes modifier aliases, and imports the first
configured `interrupt` and `open_model_picker` binding as native Cancel Run and Choose Model commands.
An explicit empty list unbinds that command. Invalid bindings are reported rather than converted into
an unbind; reserved/native editing shortcuts and conflicts reject the whole import. Unsaved edits to
other shortcut fields are retained. Credentials and full configuration documents are not transferred.

The shortcut editor covers command search, new/close thread, workspace/model choice, composer focus,
sidebar/inspector toggles, cancellation, overlay dismissal, and Settings. Leave a field empty to unbind it.
Choose Model defaults to ⌘⇧M. Saving uses atomic private-file persistence and displays write errors;
changes apply after restart. Kit editing, inspector arrow navigation, and native window/application
shortcuts retain their own bindings. Per-item actions remain keyboard-accessible through focus navigation
and palettes without each receiving a global chord. CLI-only terminal actions have no inferred native mapping.
The preference projection reflects the helper's startup snapshot: restart the service to reload on-disk CLI
changes. The searchable CLI configuration catalog reads effective values through the shared CLI config service;
edits target User, Workspace, or Project layers, preserve the existing value type, and reload the helper after the
validated write. Credential-bearing branches are excluded, and large catalogs or values are bounded.

Settings is constrained to the available screen and scrolls rather than compressing its theme/typography
sections. The code-font preview is a fenced Rust block rendered by the same syntax-aware Markdown path
as conversations. Kit’s `syntect` feature supplies the highlighter (fences alone do not enable highlighting).
Native buttons use a pointing hand when enabled and an arrow when disabled; text and
resize handles retain their own cursors.

### Model picker

The composer opens a compact model/effort panel above the input; **Search all models…** opens the full search.
The effort knob appears only for advertised provider/model capabilities. Native decrease/increase buttons offer
the same change through keyboard and assistive activation. Changes commit on release, and restart the idle
session so the runtime receives the new effort. The header reflects the CLI's effective fast/ultrafast badge.
The searchable model panel is anchored above the growing composer and snaps inside the window.
 Model names appear above quieter provider,
canonical ID, and context-window metadata; the current model has a checkmark and is selected initially.
Search accepts display names, providers, and canonical IDs. The composer uses the catalog display name
when available and preserves the canonical ID for service requests. Escape dismisses the panel and restores
focus. The service does not currently expose reasoning-effort options, so the picker has no effort slider
or synthetic recommended/default model list.

### Conversation activity

Consecutive tool calls appear as one expandable activity summary between assistant messages.
The summary includes action counts and running/failed counts; assistant commentary always breaks a group.
Expand the summary to see compact action rows, then activate a row to show its output. Long action titles
truncate visually and remain available in tooltips and accessible names. Command/tool output uses the
configured code font, with structured event details as a fallback when plain output is unavailable.
Streaming updates retain group and detail expansion. Collapsing activity pauses automatic tail following,
so inspecting earlier work does not pull the conversation away. Original events and inspector history remain
unchanged; this is a presentation grouping over the measured transcript rows.

## Accessibility

The desktop follows the [VT Code accessibility conventions](tui-design-system.md#accessibility):
status includes text, active navigation does not rely on color, and decorative chrome does not
add keyboard stops. Conversation bodies expose the author and original message text to assistive
technology; expanded tool output is readable separately from its named show/hide action.
Font selectors announce the current family, and font-size values expose their current pixel size.
Existing keyboard controls, modal focus containment/restoration, and theme contrast tokens remain shared.
The conversation stays virtualized: only mounted rows appear in the accessibility tree. Navigation rows keep IDs tied
to thread or workspace identity so sorting does not transfer an accessibility node to a different item.

## Themes

The desktop ships a curated port of the VT Code TUI theme registry — **Ciapre** (the default), Ciapre Dark and
Ciapre Blue, Solarized Dark/Light/HC, Gruvbox Dark/Light, Dracula, GitHub Dark/Light, Atom One Dark/Light,
Tomorrow Night, Catppuccin Latte/Mocha, plus the kit's System Light/Dark bases. A theme sets background,
foreground, accent, danger, and warning; border, muted text, and panel colors are derived from the palette.
Muted text is binary-searched to the WCAG AA 4.5:1 threshold and filled controls pick white/black text at the
equal-contrast crossover; both are enforced by tests. The choice persists in preferences.
Palette changes use the kit's theme update path so Component and Base tokens stay synchronized. Tabs, focus rings,
and primary controls inherit VT Code's accent; primary hover and pressed states retain readable text.

Toolbar and command-palette mode switches share the theme-picker application
path and persist the selected System Light/Dark theme. They preserve typography
and drafts and remeasure the transcript. Retained Markdown derives its text,
inline-code, table, link, and fenced-code styles from the active theme each frame;
it must not install Base's fixed light default over Kit's theme tokens.

## Preferences

Comfort settings live in `~/.vtcode/desktop/preferences.json`: the selected theme, recent workspaces (at most 8),
inspector visibility, and pane widths. A missing or corrupt file falls back to defaults; write failures are ignored.
The most recent workspace reopens on launch when it still exists.

The UI drains at most 64 service events per foreground tick so a busy stream yields to input. Row data and GPUI's
measured list state are maintained together; streaming text, expansion, theme changes, and viewport resizing invalidate
the affected geometry. No estimated character count is used for transcript row heights.

File tasks retain cancellation handles: a newer scan or preview supersedes the previous request, and results from an
old workspace cannot replace the current view. Switching, creating, or closing sessions clears prior changes, diff
selection, context usage, pending interaction answers, and attachments together. Model/runtime-option switches
clear runtime projections while preserving the editable draft and its attached sources.
A retained unsent message survives reloading its selected session. Restore or Dismiss must resolve it before changing
sessions, models, or workspaces. Restoration waits for pending session requests to finish. Before enqueueing a turn,
the client checks its encoded JSON request, including escaping and envelope overhead, against the protocol's
1 MiB frame bound; oversized requests leave the composer and attachments intact. Once the runtime starts or finishes
a turn, a later acknowledgement cannot restart it or offer its accepted input for recovery.

Approval, plan approval, and question panels return the exact opaque option identities supplied by the service. Wizard
steps retain their order and collect free-form answers separately from display labels. The GUI does not create its own
agent loop or bypass VT Code's approval and execution policy.

## Local service protocol

`vtcode desktop-server` uses one UTF-8 JSON-RPC object per line on stdin/stdout. Diagnostics go to stderr. The client and
server negotiate protocol version `1.9` (additive over 1.0–1.8; older fields are serde-defaulted); incompatible ranges fail
explicitly. The protocol crate caps a line at 1 MiB.
The runtime-event, service-notification, host-command, and desktop-request queues each hold at most 128 entries; the
GUI event bridge holds at most 256 events. The runtime-event queue is a bounded coalescing channel: `item.updated`
snapshots replace the pending snapshot for the same item, consecutive `plan.delta` events for one item merge, and
ordered events displace stale snapshots before ever being dropped. A slow GUI therefore degrades the stream instead of
failing a healthy turn; only a disconnected consumer is fatal, and displaced or dropped events are counted and logged
by the service pump.

| Method | Purpose |
| --- | --- |
| `desktop/initialize` | Negotiate protocol compatibility and confirm the selected workspace. |
| `config/discover` | Read active provider, model, sandbox label, and context window. |
| `config/settings/read`, `config/settings/write` | Browse bounded, credential-filtered effective settings and save type-checked edits through the CLI config service. |
| `config/models`, `config/select` | List the model catalog, or switch provider/model for the next session (requires no active turn). |
| `workspace/review` | Capture bounded Git changes against HEAD or an unborn branch’s empty tree, with optional untracked text previews. |
| `session/list`, `session/create`, `session/load`, `session/close` | Select, create, and restore durable messages, canonical change reviews, activity, and captured output. |
| `turn/submit`, `turn/cancel` | Submit one user turn or cancel its exact session/turn identity. |
| `interaction/respond` | Resolve one pending approval or question using its session, turn, and interaction IDs. |
| `desktop/shutdown` | Cancel and clean up owned work before exit. |

The service consumes an interaction response only after the active runtime accepts its typed event. Failed delivery leaves
the interaction pending so the client can retry or let session cleanup dismiss it.

Notifications stream the canonical `VersionedThreadEvent` from `vtcode-exec-events` along with interaction requests and
turn status. A client must reject stale or duplicate interaction responses. EOF triggers service cleanup. The full wire
types and framing helpers live in [`vtcode-desktop-protocol`](../../crates/common/vtcode-desktop-protocol/README.md).

Terminal cleanup uses the accepted active-turn identity and typed runtime outcomes. It does not depend on receipt of
`turn.started`: cancellation during preparation or a saturated live queue can omit that event. The UI rejects
notifications for another selected session or active turn, including known old terminal events received while the next
submission awaits acknowledgement. Session-start questions remain valid for their selected session. Full attribution
of late events across adjacent accepted submissions remains limited because canonical terminal events do not carry
an input-submission identity.

The separation between client commands, streamed events, and interaction responses follows the public shape of the
[pinned Codex app-server protocol](https://github.com/openai/codex/blob/89461ea6833832b0551ce24ee32205824d5c108d/codex-rs/app-server-protocol/src/protocol/common.rs).
This reference informs the protocol boundary only; the desktop UI and runtime integration are VT Code-specific.

## Scope

The first desktop release is local and macOS-only. It has no embedded editor, terminal, remote service, automatic
updater, concurrent turns, GUI credential setup, or full parity with every TUI command.

### Concept workspace layout

The desktop uses a shared top toolbar above a recessed navigation pane, central
conversation, and contextual inspector. Navigation selections use quiet surfaces;
the accent emphasizes primary actions. The gpui-kit textarea and model/attachment
actions share one composer surface. Existing VT Code theme tokens drive all colors.

| Action | Control |
| --- | --- |
| Collapse or restore navigation | Toolbar navigation button or `Cmd+B` |
| Search commands | Toolbar search button or `Cmd+K` |

Collapsing navigation retains the active session, draft, and pane width for the
current window. The toolbar remains available when navigation is collapsed.

The toolbar and welcome logo embed `resources/icons/vtcode-profile-120.png`.
macOS packaging generates `Contents/Resources/VTCode.icns` from the shared 512px
artwork and declares it in `Info.plist`. The 1024px Retina slot is upscaled from
that source; no checkout paths or external image requests are needed at runtime.

### Appearance and typography

Open Settings (`Cmd+,`) for theme previews and a Typography card. UI font size
ranges from 12–24 logical pixels (default 16); code size ranges from 10–24
(default 14). The accessible decrease/increase buttons apply immediately and
save to desktop preferences. Code size controls Markdown code blocks, previews,
terminal output, and gpui-kit diff typography. Kit diff sizing also follows the
UI scale. Transcript measurements refresh when typography changes.

| Setting | Behavior |
| --- | --- |
| UI font | Native system font by default (SF Pro on macOS); searchable installed-font selector |
| Code font | SF Mono when installed, otherwise the platform's resolved monospace default; independent selector |
| UI font size | Base size for controls and conversation text |
| Code font size | Monospace code surfaces; independent saved preference |
| Reset appearance to defaults | Restores the default theme, font families, and sizes; preserves workspaces, sessions, and pane widths |

Existing preference files retain defaults for missing typography fields. Saved
sizes survive theme selection and are clamped to the supported bounds when loaded.
Font selections also survive theme switches and restarts. Missing saved fonts
fall back to the native UI or code default instead of being passed to the font
renderer. Font discovery is cached once per process; restart after installing
new fonts. The font picker reuses Kit's searchable command palette and returns
to Appearance on confirmation or cancellation. Default sizes are a base scale:
headings and secondary labels retain their relative hierarchy.
The Appearance dialog scrolls to keep controls reachable on smaller windows.
Custom colors, separate light/dark profiles, motion overrides, font-family pickers,
translucency, cursor overrides, alternate Dock icons, and font-smoothing overrides
are not implemented in this increment.

### Component composition and visual hierarchy

See the [complete component review](desktop-component-review.md) for the 79-page
catalog audit and adoption decisions. Conversation rows compose `Message`/`Bubble`,
composer files use `Attachment`, and `InputGroup` provides the shared composer frame.
Appearance uses dialog heading parts and `GroupBox`; theme previews are keyboard-accessible kit buttons.
Neutral kit hover colors are
distinct from VT Code's brand primary. Component surfaces preserve readable text.

| Interaction | Behavior |
| --- | --- |
| Thread overflow | New/close actions reuse existing guarded session commands |
| Appearance | Centered, scrollable overlay with trapped focus and explicit close |
| Escape from Appearance | Returns to previous focus without altering the draft |
| Welcome review | Opens session changes without creating a thread or sending a prompt |

Navigation retains real kit buttons with left-aligned icon/title content and
keyboard/disabled behavior. Transcript virtualization and raw-copy behavior retain
the existing measured list and shared runtime projection.

### Expanded session diff review

The Changes sidebar reserves most of its height for one syntax-highlighted file. Its compact file list,
addition/deletion totals, previous/next file controls, and named copy/open/expand icon buttons keep the
review usable at narrow widths. File navigation wraps at either end. Sidebar diffs retain unwrapped code
lines; the expanded review retains soft wrapping and unified/split mode controls. Session edits and
explicit Git working-tree snapshots remain separate review scopes.

Select a changed file in the Changes inspector, then choose **Expand diff review**.
The larger read-only surface reuses the selected gpui-kit `DiffState` and parsed
session patches. Unified/split mode persists while switching reviewed files.
Only one visible diff component renders that state at a time.

| Control | Behavior |
| --- | --- |
| Unified / Split | Changes the retained viewer layout |
| Next change / `F7` | Navigates to the next changed group in the selected file |
| Previous change / `Shift+F7` | Navigates to the previous changed group |
| Copy patch | Copies the exact change-set owning the parsed file preview |
| Open file | Leaves review and opens the bounded read-only workspace preview |
| Close / `Escape` | Returns to the inspector and restores previous focus |

Runtime updates refresh an already-open diff when its accepted patch changes;
unrelated file updates preserve its line selection. New text invalidates an old
selection. Invalidated previews or session transitions dismiss expanded review
and recover composer focus. Appearance, command palettes, file previews, and
expanded review are mutually exclusive overlays.

The Changes projection keeps the newest successfully parsed patch per exact file
path and its owning transcript item. Older patches cannot overwrite newer ones,
and a path merely mentioned in another patch's text cannot affect copied output.
The viewer reviews session-event patches; it does not stage, revert, or write files.

### Restoring saved inspector history

Reopening a session restores recent file-change and tool/activity snapshots from the shared canonical
`events.jsonl` through the same projection and GPUI Kit Diff viewer used during a
live turn. Patch copying and expanded unified/split review remain available after
reopening. Long lines wrap within each review column so both sides of a split
remain visible. Historical lifecycle and approval events are not dispatched as live
runtime events. Archives without a canonical log still restore their messages.

The read-only query uses handle-bound filesystem access and rejects symlinked
paths. It reads the final 4 MiB, excludes incomplete edge records, keeps the latest
snapshot per item, and returns at most 80 review items within a 200 KiB encoded budget
and 80 activity items within a separate 64 KiB budget. Large output cannot displace
saved patches. These limits include encoded JSON escaping.
Each category retains its newest contiguous suffix; an oversized item stops that
category rather than allowing an older patch or tool status to appear current. The status reports partial
inspector history when these limits omit evidence. Malformed complete records or
unsafe paths fail loading explicitly. Platforms without handle-bound reads restore
messages and report inspector history unavailable. No desktop-specific history store is written.

Historical tool-output rows also recover their spool contents when the reference
resolves beneath the workspace's `.vtcode/context/tool_outputs` directory. Each
read is bounded to 32 KiB and the existing activity budget; symlinks, outside
paths, missing files, and oversized output never expose a local path and report
partial inspector history. The captured inline preview remains available when a
spool cannot be read.

Restored activity uses the same projection and GPUI Kit cards as live tool events,
but stays in the inspector so historical tools do not crowd out the conversation.
New live activity takes precedence in Output. Session changes clear both projections.
Unfinished historical commands and tools display as interrupted, while completed
and failed outcomes retain their original status. Restoring never starts a command,
reopens an approval, or marks a turn running. Unstructured archive tool text remains
in its collapsed transcript card but is not classified as structured activity. The Output tab shows
the newest retained command/tool output and copies its exact captured text; spool
files are not followed or fetched automatically.

### Git working-tree review

In the Changes inspector, choose **Review working tree**. Leave **Folder** empty
for the selected workspace or enter a relative directory, then choose **Load changes**.
No Git capture starts merely by opening the dialog. Folder resolution reuses the
shared workspace path resolver; missing folders, files, and paths escaping the
workspace (including symlink escapes) are rejected.

The snapshot combines tracked staged and unstaged changes against HEAD within
that directory. Untracked files are excluded by default; enable **Include untracked
text files**, then Load changes or Refresh to capture them. Git ignore rules still
apply. The snapshot remains separate from historical
agent-attributed session patches. It never stages, discards, applies, or commits
changes. The shared core Git reader disables external diff/textconv, clean/process
filters, and filesystem-monitor hooks, disables optional index refresh writes,
redacts secrets, and enforces a two-second
timeout. Explanation snapshots retain their existing 8 KiB limit; desktop review
uses a separate 200 KiB bound.

Untracked previews use the shared no-follow file reader and diff formatter, with
at most 80 files and 64 KiB per file. Symlinks, hard links, non-regular files,
binary/non-UTF-8 content, unsupported filenames, and files exceeding the remaining
preview budget are omitted and counted in the capture note. Any omission marks
the preview incomplete. An oversized or failed Git listing makes the capture
unavailable. Empty new files retain their added-file header. Untracked review
never stages files or changes the index. Explanation snapshots stay tracked-only.

Before the first commit, a valid unborn branch is compared with Git's empty tree
in the repository's native object format (SHA-1 or SHA-256). The capture note
identifies that baseline. The reader computes the empty-tree ID without writing
objects, creating an index, or making a commit. A corrupt branch reference,
unresolvable detached HEAD, or invalid repository remains unavailable rather than
being treated as an empty history. Existing committed and detached branches use
their resolved HEAD tree.

GPUI Kit renders complete supported patches in unified or split mode with soft
wrapping. **Previous file** and **Next file** jump to file headers and wrap at the
ends; the navigation row shows the selected path and its position. Refresh keeps
that path when it remains in the patch, even if files reorder, and otherwise
selects the first remaining file. Navigation is disabled for a single-file patch
and unavailable for raw or truncated previews.
**Copy preview** copies the captured redacted text. A truncated or
unsupported patch is shown as raw text with an explicit notice rather than as a
complete structured diff. Clean tracked state is distinct from invalid or
unavailable Git state. Use Refresh after edits; captures are snapshots, not a filesystem watcher.

The viewer reuses the session review dialog's focus trap and Escape restoration.
Refresh retains the display mode. Request and workspace identities reject late
captures after refresh, dismissal, or workspace changes. Service failures unblock
retry. Captured Git changes never enter the agent transcript or session history.

### GPUI reference review (2026-10-25)

Reviewed Zed commit `347f171a3fd00a3b672382d78348b37bd969d17c`:
[accessibility](https://github.com/zed-industries/zed/blob/347f171a3fd00a3b672382d78348b37bd969d17c/crates/gpui/src/_accessibility.rs),
[ownership/data flow](https://github.com/zed-industries/zed/blob/347f171a3fd00a3b672382d78348b37bd969d17c/crates/gpui/src/_ownership_and_data_flow.rs),
and [examples](https://github.com/zed-industries/zed/tree/347f171a3fd00a3b672382d78348b37bd969d17c/crates/gpui/examples),
particularly `a11y.rs` and `popover.rs`. Applied stable roles/labels, retained entity/subscription ownership,
and anchored/deferred popovers using the APIs actually available through pinned GPUI Kit 0.8.0.
These are original VT Code integrations; no main-branch APIs or example implementation were copied.
