# GPUI Kit desktop guide review

Reviewed on 2026-10-26 against GPUI Kit 0.8.0, the version locked by VT Code Desktop.

## Scope and method

The [documentation index](https://gpui-kit.com/docs/) and its official
[full documentation export](https://gpui-kit.com/llms-full.txt) supplied 41 guide topics.
Every guide was surveyed for applicability; the detailed implementation review focused on
ownership, deferred callbacks, rendering, focus, testing, and packaging. Component, Base,
and Shell API catalogs are separate from this 41-guide count.

API behavior was checked against the resolved `gpui-component-0.8.0`,
`gpui-base-0.8.0`, and `gpui-pre-0.3.9` sources. Current website examples are
guidance, not permission to change the locked dependency graph.

## Confirmed findings and changes

1. A Command confirmation runs through `window.defer`. Desktop checked the
   originating palette identity for query changes but not confirmations.
   Confirming a command and replacing the popup before delivery could execute
   the old command and dismiss the replacement. The rendered regression failed
   before the fix. Confirmation, cancellation, and outside-click handlers now
   reject events from a superseded palette.
2. Rendering cloned owned `PaletteCommand` values twice, including full prompt
   text in history actions. Entries now become immutable `Rc<PaletteCommand>`
   values at the UI ownership boundary. Rendered snapshots share the payload;
   only the chosen action is cloned when confirmed. Group vectors and normal
   component descriptions are still rebuilt. No quantified speedup is claimed.
3. A helper without the additive mode catalog yielded a generic empty result.
   The mode picker now names the missing catalog and tells the user to rebuild
   the matching app/helper and reload settings. Its search prompt also identifies
   agent modes.
4. Native inspection found the running temporary app bundled an older helper.
   Its live `config/discover` response had no `availableModes`; the rebuilt
   helper returned auto, build, coordinator, duck, and plan. A matching local
   development bundle was installed at `~/Applications/VT Code.app`, and those
   five entries were verified in its native picker.
5. The mode catalog was complete, but CLI mode names arrived lowercase and
   looked like missing choices. Desktop now capitalizes the first character in
   the mode list and composer control while preserving canonical lowercase mode
   IDs and custom labels. Rendered tests cover Auto, Build, Plan, Duck, and
   Coordinator.
6. The compact model picker placed its selected model and effort controls on a
   single flat surface. Its selected-model summary now has a rounded, subtly
   accent-tinted card, a model icon, stronger title hierarchy, and a clearer
   effort track while retaining wrapping for long model names and the existing
   compact width.
7. Review found two composer edge cases: a package-manager word earlier in
   ordinary prose suppressed a later `@file` completion, and an unavailable
   native slash action could fall through as an agent prompt. Package-manager
   detection is now scoped to the current shell command segment; unavailable
   native actions stay in the composer and explain what needs to be available.
   Focused regressions cover package scopes, prose, command boundaries, and
   unavailable mode/fast actions.

These changes retain the shared CLI/runtime and protocol. No second agent loop,
new production dependency, generic view cache, updater, WebView, or rendering
backend was introduced.

## Guide coverage and application decisions

| Guide | Application decision |
| --- | --- |
| [Framework overview](https://gpui-kit.com/docs/) | Keep the existing Kit application, Base root, native controls, and CLI service boundary. |
| [Installation](https://gpui-kit.com/docs/installation/) | Keep the locked GPUI Kit 0.8.0 dependency family and macOS 15 minimum. |
| [Getting started](https://gpui-kit.com/docs/getting-started/) | Initialize Kit once and retain view-owned inputs; no second GPUI dependency. |
| [Entity](https://gpui-kit.com/docs/entity/) | Retain component state and weak callback owners. Share immutable palette entries at the view boundary. |
| [Context](https://gpui-kit.com/docs/context/) | Re-enter the owner after asynchronous work; guard popup callbacks by their originating entity. |
| [Window](https://gpui-kit.com/docs/window/) | Reject a deferred confirmation after its originating popup has been replaced. |
| [Multiple windows](https://gpui-kit.com/docs/multi-window/) | Retain the existing reopen/activation policy and per-window draft ownership; no new window registry. |
| [Element identity](https://gpui-kit.com/docs/element_id/) | Keep domain-derived row/control IDs and the palette entity ID as the callback generation. |
| [Focus](https://gpui-kit.com/docs/focus/) | Keep retained handles, next-frame popup focus, focus traps, and dismissal restoration. |
| [Shared strings](https://gpui-kit.com/docs/shared-string/) | Retained transcript text already uses shared storage. Share command payloads without adding UI types to the wire protocol. |
| [Render](https://gpui-kit.com/docs/render/) | Compose from retained state; no requests, parsing, or new subscriptions from render. |
| [RenderOnce](https://gpui-kit.com/docs/render-once/) | Keep lightweight Kit controls as values; do not turn each button or row into an entity. |
| [Style](https://gpui-kit.com/docs/style/) | Keep semantic Ciapre/theme colors and current font-aware layout; avoid unrelated pixel-to-rem rewrites. |
| [Action](https://gpui-kit.com/docs/action/) | Reuse typed actions and existing command owners across keys, menus, and buttons. |
| [Key bindings](https://gpui-kit.com/docs/keybinding/) | Preserve contextual input routing and native-menu rebuilds after shortcut changes. |
| [Events](https://gpui-kit.com/docs/event/) | Confirm through the actual component action path; stale callbacks cannot dismiss a replacement popup. |
| [Tasks](https://gpui-kit.com/docs/task/) | Keep scans, previews, pickers, and event pumps owned by the view; existing preview close cancels its task. |
| [Global](https://gpui-kit.com/docs/global/) | Keep theme/application services global and workspace/session data under the view owner. |
| [View cache](https://gpui-kit.com/docs/view-cache/) | Do not add a scene cache without measured work and complete invalidation. Fix payload ownership first. |
| [Elements](https://gpui-kit.com/docs/element/) | Continue using Kit components and measured list infrastructure instead of custom control implementations. |
| [Painting](https://gpui-kit.com/docs/paint/) | No new canvas or tessellation path is needed for the confirmed palette issues. |
| [Text system](https://gpui-kit.com/docs/text-system/) | Keep native shaped Markdown and measured wrapping; do not estimate rows by character count. |
| [Geometry](https://gpui-kit.com/docs/geometry/) | Preserve the shared scroller inset and viewport-constrained popups. |
| [Animation](https://gpui-kit.com/docs/animation/) | Preserve reduced motion, finite transitions, and existing theme motion tokens. |
| [Testing](https://gpui-kit.com/docs/test/) | Add a rendered stale-confirmation regression; retain native checks for executable identity and visible mode contents. |
| [Accessibility](https://gpui-kit.com/docs/accessibility/) | Keep semantic controls, accessible names, and current choice states; use a specific agent-mode search prompt. |
| [WebAssembly](https://gpui-kit.com/docs/webassembly/) | Showcase deployment is outside this native desktop increment; do not conflate it with VT Code WebMCP. |
| [Images](https://gpui-kit.com/docs/image/) | Keep packaged brand artwork and the transcript's existing remote-image restriction. |
| [Assets](https://gpui-kit.com/docs/assets/) | Retain selected embedded icons and asset-coverage checks; do not embed the full icon catalog. |
| [Fonts](https://gpui-kit.com/docs/fonts/) | Keep system SF defaults, independent code fonts, and current typography invalidation. |
| [Internationalization](https://gpui-kit.com/docs/i18n/) | Retain app-owned locale resources and explicit view refresh; no new translation subsystem. |
| [System notifications](https://gpui-kit.com/docs/system-notification/) | Keep the existing originating-window notification route and durable in-app state. |
| [Native extensions](https://gpui-kit.com/docs/native-extension/) | Keep asynchronous native path prompts; do not introduce a synchronous native modal loop. |
| [WebView](https://gpui-kit.com/docs/webview/) | Experimental embedding is unnecessary for conversations, agent tools, or the observed mode picker. |
| [Mobile](https://gpui-kit.com/docs/mobile/) | Experimental mobile integration is outside the macOS desktop product boundary. |
| [Packaging](https://gpui-kit.com/docs/packaging/) | Verify the exact running bundle and matching bundled helper, not only the on-disk Cargo outputs. |
| [Auto update](https://gpui-kit.com/docs/auto-update/) | Do not bolt on an updater without the matching installer/release contract; reuse existing CLI release handling. |
| [Design guides](https://gpui-kit.com/docs/design-guides/) | Keep the existing restrained shell and explicit recovery states; missing mode metadata gets actionable guidance. |
| [Coding guides](https://gpui-kit.com/docs/coding-guides/) | Share immutable palette commands, keep lifecycle guards local, and avoid a speculative feature-crate split. |
| [Framework comparison](https://gpui-kit.com/docs/comparison/) | No framework, rendering backend, or dependency migration is justified by these findings. |
| [Frame-rate monitoring](https://gpui-kit.com/docs/fps/) | Make no FPS, CPU, or allocation-percentage claims without a representative profiled build. |

## Component catalog review

The [v0.8.0 component catalog](https://gpui-kit.com/component/) lists 79 components.
VT Code already uses the catalog components that match its workflows: `Message`/`Bubble` and
`MessageScroller` for conversations, `Command` for searchable actions and changed-file selection, `InputGroup` and
`Attachment` for the composer, `Tree` for workspace files, `Diff` for code review, and
`GroupBox`, `DescriptionList`, `TabBar`, `Popover`, `Toggle`, `Checkbox`, `Marker`, `Tag`, and
`StatusBar` in their respective settings, review, and navigation surfaces. No replacement of these
established compositions was justified by the catalog examples.

The Files inspector was the confirmed gap: its first asynchronous workspace scan rendered an
empty tree, which looked like a completed empty result. It now shows a bounded Kit `Skeleton`
with a semantic status label, then a Kit `Empty` state if the completed scan finds no files or
no workspace is selected. The skeleton helper is shared with Git review and falls back to a
static theme-colored shape when reduced motion is enabled. See [Skeleton](https://gpui-kit.com/component/skeleton/),
[Empty](https://gpui-kit.com/component/empty/), and [Tree](https://gpui-kit.com/component/tree/).

Running tool rows now use Kit's reduced-motion-aware `Spinner` in the transcript, collapsed
activity-group summaries, and activity inspector; completed and failed rows keep static status
icons. A shared semantic status-color mapping keeps running, completed, failed, interrupted, and
other states consistent across the transcript and inspector. Spinners appear only while work is
running, so archived or idle rows do not animate. See
[Spinner](https://gpui-kit.com/component/spinner/).

Follow-up (2026-10-27): Multi-step runtime questions now render their single-choice options with
Kit `Radio` controls inside a labeled radio group. The checked index still derives from the
view's protocol-backed interaction state, and selection uses the existing validated response
path; the Kit `QuestionnaireState` would duplicate that runtime-owned answer state. The locked
Kit 0.8.0 `RadioGroup` has no group-label builder, so the view supplies the semantic group and
accessible name while Kit owns each radio's selection semantics. See
[Radio](https://gpui-kit.com/component/radio/) and
[Questionnaire](https://gpui-kit.com/component/questionnaire/).

Follow-up (2026-10-27): Rechecked the [current component catalog](https://gpui-kit.com/component/),
which now documents Kit 0.8.1, against VT Code's exact 0.8.0 lock. Keep API decisions grounded in
the locked 0.8.0 source; a newer docs page does not authorize a dependency upgrade. The app already
uses the catalog components that fit its workflows, including virtualized transcript and activity
lists, protocol-backed `Radio` choices, and shared `Button`/`Tooltip` actions. Do not add catalog
controls such as calendars, ratings, or avatars without a matching desktop workflow and data source.

Follow-up (2026-10-27): The toolbar navigation control and collapsed Home preview now expose their
pressed state as Kit toggle buttons, in addition to their accessible names. The rendered tests check
both states while navigation opens and closes, including the transient hover preview. Kit 0.8.0
Button exposes toggle state but not `aria-expanded`; using the supported toggle state keeps the
existing Button semantics and avoids wrapping it in a conflicting nested control. See
[Button](https://gpui-kit.com/component/button/) and
[Accessibility](https://gpui-kit.com/docs/accessibility/).

The app-owned alert wrapper remains intentional. Kit 0.8.0 `Alert` provides the semantic alert
role, but its `RenderOnce` interface does not expose the test-support registration used to verify
the app's stable alert ID and accessible message. Keep that narrow wrapper for its tested identity
and app-specific warning/error contrast; replacing it with the visual component alone would weaken
the current regression coverage.

## Verification boundaries

Focused review checks passed. The full
`cargo nextest run --locked -p vtcode-desktop -p vtcode-desktop-protocol --test-threads 1 --no-fail-fast`
suite passed all 279 tests. After the last package-scope cases were added, the focused
composer-trigger test passed again.
Desktop all-target Clippy with warnings denied, workspace formatting, changed Markdown,
and `git diff --check` passed. `./scripts/check-dev.sh` passed formatting, default-member
Clippy, compilation, and shell-script lint; it does not run tests. Its shellcheck warnings
are in existing scripts. The earlier `let_underscore_future` blocker in
`vtcode-core/src/utils/session_archive/retention.rs` has been fixed.

The lifecycle regression uses a real rendered Kit Command, dispatches Confirm
through its focused control, replaces the popup before deferred delivery, and
checks that no backend command is sent and the new popup remains open.
Existing tests cover ordinary confirmations, filtering, file/slash completion,
prompt-history insertion, mode choices, and reasoning controls.

Native mode inspection proves the selected bundle displayed the five built-in
modes. It does not prove a live provider/tool run, OS notification delivery,
VoiceOver announcements, other platforms, or production performance.
The development bundle is locally signed, not a notarized release.

This is an original VT Code applicability review. Upstream prose is credited to
[GPUI Kit](https://gpui-kit.com/docs/) under its documented
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) terms; no upstream code
was copied into this increment.
