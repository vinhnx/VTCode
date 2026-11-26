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
| [Installation](https://gpui-kit.com/docs/installation/) | Re-checked 2026-11-25 against the site v0.11.0 page ([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted); pinned `gpui-kit = "=0.10.0"` / `gpui-pre 0.3.12` and toolchain 1.100.0 win. Keep the single `gpui-kit` dependency (`vtcode-desktop/Cargo.toml:25,55`), default features, and the checked-in lockfile; macOS 15+/Xcode CLT prerequisites are already in `desktop-workspace.md`. The v0.11.0 Rust 1.92 minimum is tied to `oo7 0.6.0` and does not apply to this pin; Wayland-only Linux builds are out of scope for the macOS-only app, and the `gpui-fast` feature stays a measured-only decision (FAQ row). Applied the one real delta: the dev profile lifts the framework rendering/layout/text crates (`gpui-pre`, `gpui-pre-platform`, `gpui-pre-apple`, `gpui-pre-macos`, `gpui-component`, `gpui-base`, `gpui-kit`, `gpui-kit-assets`, `rustybuzz`, `taffy`, `ttf-parser`) from the wildcard `opt-level = 1` to `opt-level = 3` for a responsive preview, while app code stays debuggable and proc macros stay in `build-override`. `gpui-base` and the macOS platform backends join the upstream list because the pinned graph still splits them out; a `cargo build -v` check confirmed they were the remaining render-path crates at level 1. `gpui-fast` is in the lockfile but not enabled. `scripts/check_dev_profile.py` (wired into the shared governance gate) and `scripts/tests/test_dev_profile.py` catch a dropped or stale override after an upgrade. Per-claim evidence: `.vtcode/memory/library.md` (2026-11-25 Installation entry). |
| [FAQ](https://gpui-kit.com/docs/faq/) | Keep upstream GPUI through the pinned `gpui-pre` snapshot. Re-checked 2026-11-25 against the pinned v0.10.0 FAQ (identical to the current prose; [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted). The FAQ's `gpui-fast` feature is a drop-in alternative backend — `patches/gpui-component/Cargo.toml:40-44` exposes it and `patches/gpui-component/README.md:141-145` confirms no application-code change — but `cargo tree --locked -p vtcode-desktop -e features` has no `gpui-fast`, so the default backend stays active. Do not enable it for the virtualized transcript, terminal, or file lists without a measured need (View cache row and Frame-rate monitoring below); the FAQ itself says to measure first. Upstream Kit does not modify GPUI, but VT Code carries one documented local fix to the vendored macOS snapshot (`patches/gpui-pre-macos/PATCH.md`, cursor refresh) — keep that distinction when reading upstream claims. Because `gpui-pre-*` are versioned Zed snapshots published with notices intact, THIRD-PARTY-NOTICES must name the vendored version and Zed revision: corrected to 0.3.12 / `d2a4fb6…` and guarded by `scripts/check_third_party_notices.py` in the shared governance gate. |
| [Getting started](https://gpui-kit.com/docs/getting-started/) | Re-checked 2026-11-25 against the site v0.11.0 page ([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted); pinned `gpui-kit = "=0.10.0"` / `gpui-pre 0.3.12` wins. The startup contract is already correct: `application().with_assets(DesktopAssets)` → one `gpui_kit::init(cx)` → `open_window` returning the content view, never a second `Root` (`view/bootstrap.rs:38,54-56,123-129`). `cargo tree --locked -p vtcode-desktop -e features -i gpui-pre-macos` confirms `gpui-kit` enables `gpui-pre-platform/font-kit → gpui-pre-macos/font-kit`, so macOS text renders without a manifest change; `cx.subscribe_in` is used at 23 sites and state entities are built once in `new_with_preferences`. Keep the default backend (`gpui-fast` stays a measured-only decision: Installation/FAQ rows) and no speculative feature-crate split (Coding guides row). The load-bearing new point is accessibility: pinned `label.rs:236-245` confirms plain text is absent from the accessibility tree, but Kit's `Label` also hardcodes `theme().foreground` + `line_height` (`label.rs:247-248`) and a `CodeLocation` id, so wrapping the desktop's token-styled, repeated text in it would break WCAG typography and duplicate ids. The desktop's explicit `.role()` + `.aria_label()` contract is the correct equivalent; applied it to the two screenshot-card notes that were bare text (`view/computer_preview.rs` expired/not-loaded) and to `view/inspector_panels.rs::output-page-loading`, whose `Status` node named only nested shimmer text. Both are pinned by role+label assertions in `view/tests/{computer_preview,review_output_a11y}.rs`, and a source scan of every production `Status`/`Alert`/`Label` node now finds no other unnamed instance. Per-claim evidence: `.vtcode/memory/library.md` (2026-11-25 Getting Started entry). |
| [Entity](https://gpui-kit.com/docs/entity/) | Retain component state and weak callback owners. Share immutable palette entries at the view boundary. Re-checked 2026-11-25 against the full Entity guide (site v0.11.0 prose, pinned `gpui-kit = "=0.10.0"` / `gpui-pre 0.3.12` wins). All substantive claims are already present: window-lifetime input/tree/select `Entity` state is built in `view/bootstrap.rs::new_with_preferences`, and per-interaction state (palette `CommandState`, effort slider, rename/review/diff viewers) is built in its `show_*`/`open_*` opener — never in `render`; the only four child→root back-references (`ThemeGrid`, handoff, deeplinks, `MainWindow` global) are `WeakEntity`, so no parent/child cycle; all `observe`/`subscribe` results are retained in `_subscription` fields and every spawned `Task` is either held in a `Task` slot or explicitly detached; no same-entity re-entrant access or `&T` across `await` remains (deferred callbacks and `detach_task_slot` guard the risky sites). `notify` and `emit` stay separate on the effort slider (`set_value` notifies, `settle_effort` re-emits `SliderEvent::Release`), and value-like drag previews use Kit's `Render`-requiring drag API, so their per-drag `cx.new` is API-mandated rather than a render-time state reset. No new cache: `Entity::cached` needs measured work and complete invalidation (row 83). Applied the guide's `read_with` boundary rule as a clarity change in `view/composer_controls.rs` (`step_effort`/`jump_effort` now take their `f32` inside one borrowed read; the previous statement-level extraction was already NLL-safe — no bug fixed). Per-claim evidence and re-verification: `.vtcode/memory/library.md` (2026-11-25 Entity entry). |
| [Context](https://gpui-kit.com/docs/context/) | Re-enter the owner after asynchronous work; guard popup callbacks by their originating entity. |
| [Window](https://gpui-kit.com/docs/window/) | Reject a deferred confirmation after its originating popup has been replaced. |
| [Multiple windows](https://gpui-kit.com/docs/multi-window/) | Retain the existing reopen/activation policy and per-window draft ownership; no new window registry. |
| [Element identity](https://gpui-kit.com/docs/element_id/) | Keep domain-derived row/control IDs and the palette entity ID as the callback generation. |
| [Focus](https://gpui-kit.com/docs/focus/) | Keep retained handles, next-frame popup focus, focus traps, and dismissal restoration. |
| [Shared strings](https://gpui-kit.com/docs/shared-string/) | Retained transcript text already uses shared storage. Share command payloads without adding UI types to the wire protocol. |
| [Render](https://gpui-kit.com/docs/render/) | Compose from retained state; no requests, parsing, or new subscriptions from render. |
| [RenderOnce](https://gpui-kit.com/docs/render-once/) | Re-checked 2026-11-26 against the site v0.11.0 page ([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted); pinned `gpui-kit = "=0.10.0"` / `gpui-pre 0.3.12` wins (`gpui-pre-0.3.12/src/element.rs:164-185` defines `Render::render(&mut self, …, &mut Context<Self>)` vs `RenderOnce::render(self, …, &mut App)`). The ownership split is already the app's default: shared UI in `view/components.rs` are free functions returning Kit `Button`/`Div`/`AnyElement` values (`desktop_button` builds a Kit `Button`, itself `RenderOnce`), while only five production views implement `Render` and own state/lifecycle; every `cx.new` site is a constructor, a `show_*`/`open_*` opener, a drag handler, or a test probe — none is inside `render` (Entity/Context rows). Handlers are created in the owning `Render` view (`cx.listener`/`cx.observe`/`cx.subscribe_in`) and passed into value elements; capturing handlers use `WeakEntity` where they must not extend a target's lifetime (`view/appearance_theme_picker.rs:166`, `view/composer_render.rs:218`, `view/tabs.rs:255`). SKIPPED: a custom `#[derive(IntoElement)]` component type — the free-function builders already give value semantics and adding a public type would be churn, not behavior (KISS); no allocation/FPS claim without a profiled build (FPS row). Applied the identity half of the guide: `SearchableListItem::render` option rows formerly concatenated namespace + domain key with `format!` at seven call sites, leaving no owner for the "label is display text, not identity" rule. `desktop/mod.rs` now owns `accessible_select_option(namespace, key, title)` + `option_element_id`, a typed `NamedChild(ElementId, SharedString)` from the namespace and the option's own id; the locale-dependent `title()` stays presentation only. New `view/tests/select_option_identity.rs` renders two identical labels under different keys (distinct rows) and swaps the label without remounting (both ids keep resolving); fail-with-revert proven (`left: missing … "alpha"` when the id is derived from `title`). Two existing selectors now use the same helper (`config_sessions.rs`, `files_destination.rs`). Per-claim evidence: `.vtcode/memory/library.md` (2026-11-26 RenderOnce entry). |
| [Style](https://gpui-kit.com/docs/style/) | Keep semantic Ciapre/theme colors and current font-aware layout; avoid unrelated pixel-to-rem rewrites. |
| [Action](https://gpui-kit.com/docs/action/) | Reuse typed actions and existing command owners across keys, menus, and buttons. |
| [Key bindings](https://gpui-kit.com/docs/keybinding/) | Preserve contextual input routing, fallible `KeyBinding::load` for user input, and native-menu rebuilds after shortcut/locale changes. Reviewed 2026-11-24 against the site v0.11.0 page (claims re-verified in pinned `gpui-pre 0.3.12`: `App::set_menus`/`set_dock_menu` snapshot the keymap and labels at build time); the rebuild was the one gap. `set_native_menus` is now the single rebuild point for the main, Dock, and status menus; `reinstall_shortcuts` and the UI-language switch both route through it with the caller's full-screen state. Pinned by `saving_shortcuts_rebuilds_the_native_menus` and `switching_language_rebuilds_the_native_menus`. Re-checked 2026-11-25 against the precedence section: `shortcuts.rs::EDITOR_CONTEXT_CHORDS` keeps the three `Input mode=editor` fixed chords (`cmd-.`, `cmd-[`, `cmd-]`) tied to their configurable owners — they cannot join `FIXED_RESERVED_CHORDS` without rejecting their own defaults, and a rebind to another command would be silently shadowed while the composer has focus. Pinned by `composer_context_chords_stay_with_their_commands`, `editor_context_owner_table_pins_defaults_and_fixed_bindings`, and `every_modified_fixed_chord_is_reserved` (derives fixed chords from the live keymap so the manual reservation list cannot drift; `.vtcode/memory/library.md` 2026-11-25 entry). |
| [Events](https://gpui-kit.com/docs/event/) | Confirm through the actual component action path; stale callbacks cannot dismiss a replacement popup. |
| [Tasks](https://gpui-kit.com/docs/task/) | Keep scans, previews, pickers, and event pumps owned by the view; existing preview close cancels its task. |
| [Global](https://gpui-kit.com/docs/global/) | Keep theme/application services global and workspace/session data under the view owner. |
| [View cache](https://gpui-kit.com/docs/view-cache/) | Do not add a scene cache without measured work and complete invalidation. Fix payload ownership first. |
| [Elements](https://gpui-kit.com/docs/element/) | Continue using Kit components and measured list infrastructure instead of custom control implementations. |
| [Painting](https://gpui-kit.com/docs/paint/) | Deep-reviewed 2026-11-26 (site v0.11.0, pinned 0.10.0 source): ALREADY PRESENT — `view/transcript_activity.rs::filename_dotted_underline` uses `canvas(prepaint, paint)` to build one owned dashed `Path` from resolved window bounds, `build().ok()` degrades to no paint, and the paint-free `TerminalOriginProbe` records bounds in prepaint without notify. Tessellation was already cut from 1024 `paint_quad`s to one path by the 2026-11-25 Elements review (commit `03d1d4694`); keyed-window-state caching stays SKIPPED (KISS — absolute coordinates change on scroll). No content mask or hitbox needed: the decoration stays inside its accessible button. Degenerate (zero/sub-pixel) bounds are pinned by `dotted_rule_degenerate_bounds_never_tessellate_a_wide_rule`. Full review in `desktop-component-review.md`. |
| [Text system](https://gpui-kit.com/docs/text-system/) | Keep native shaped Markdown and measured wrapping; do not estimate rows by character count. |
| [Geometry](https://gpui-kit.com/docs/geometry/) | Re-verified 2026-11-25 against pinned 0.10.0 (`gpui-pre 0.3.12`): prepaint bounds and pointer events share the window origin, so terminal cell mapping now converts once through `Point::relative_to` in one helper (`view/terminal_panel/mouse.rs`) instead of three component subtractions, and the canvas dotted rule uses `bounds.right()` with a non-zero-origin geometry assertion (`view/transcript_activity.rs`). Hitboxes/`ContentMask` stay Kit-owned (the only custom element is paint-free); popups keep `Anchor` + `snap_to_window_with_margin` rather than `Positioner::side` flipping, which would break the tested popup-above-CTA contract; theme ramps own all color math. |
| [Animation](https://gpui-kit.com/docs/animation/) | Preserve reduced motion, finite transitions, and existing theme motion tokens. |
| [Testing](https://gpui-kit.com/docs/test/) | Add a rendered stale-confirmation regression; retain native checks for executable identity and visible mode contents. |
| [Accessibility](https://gpui-kit.com/docs/accessibility/) | Keep semantic controls, accessible names, and current choice states; use a specific agent-mode search prompt. |
| [WebAssembly](https://gpui-kit.com/docs/webassembly/) | Showcase deployment is outside this native desktop increment; do not conflate it with VT Code WebMCP. |
| [Images](https://gpui-kit.com/docs/image/) | Keep packaged brand artwork and the transcript's existing remote-image restriction. |
| [Assets](https://gpui-kit.com/docs/assets/) | Retain selected embedded icons and asset-coverage checks; do not embed the full icon catalog. |
| [Fonts](https://gpui-kit.com/docs/fonts/) | Re-verified 2026-11-25 against pinned 0.10.0: vendored `theme/{system,mono}_font.rs` substitute missing defaults once on `Theme::update`/`change`; desktop applies all four font fields in `view/appearance.rs::apply_typography`, validates saved families against `all_font_names()` in `view/typography.rs`, and shapes terminal cells with `TextSystem::advance`. Keep system SF defaults, no bundled fonts/CJK stack, and the recorded invalidation/rem path. |
| [Internationalization](https://gpui-kit.com/docs/i18n/) | Re-checked 2026-11-25 against the pinned v0.10.0 page ([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted; site v0.11.0 text is equivalent, pinned crate wins). The extension contract is already correct: `rust_i18n::i18n!("locales", fallback = "en")` at `src/main.rs:4`, `extend!(gpui_component)` once before `gpui_kit::init` (`localization.rs:170-175`, `bootstrap.rs:34,56`), app overrides under the `gpui_component` namespace in `locales/gpui_component.yml` (source verified in rust-i18n 4.2.4 `NamespacedBackend`: app lookup first, built-in second, app `t!` sees only its own catalog), 20 locales with completeness gates in `localization/test_support.rs` and `tests_locales.rs`, and locale switching that rebuilds native menus, the cached composer placeholder, and palette labels before `cx.notify()` (`view/settings.rs:626-641`). `gpui_component::set_locale` merely re-exports `rust_i18n::set_locale`, so `localization.rs::set_language` now calls it once instead of twice. The load-bearing new point is whole translated messages: `view/chrome.rs` tooltips appended a hardcoded English `" (two-finger swipe)"` to the translated Back/Forward label, so every non-English locale showed half-English copy. They now use `app.navigation.back_swipe`/`forward_swipe` keys with all 20 translations, mirroring the existing `back_navigating`/`forward_navigating` pattern, pinned by `localization/tests_navigation.rs::swipe_navigation_tooltips_are_whole_translated_messages`. Other `format!("{}: {}", t!(label), data)` sites interpolate data, not translated fragments, and stay as-is; pure suffix tokens (`app.appearance.selected_suffix`) already own their locale-specific spacing. CJK font fallback and the absence of an RTL switch stay deliberate skips (Fonts row, 2026-11-24 Comparison entry). Per-claim evidence: `.vtcode/memory/library.md` (2026-11-25 I18n entry). |
| [System notifications](https://gpui-kit.com/docs/system-notification/) | Keep the existing originating-window notification route and durable in-app state. |
| [Native extensions](https://gpui-kit.com/docs/native-extension/) | Keep asynchronous native path prompts; do not introduce a synchronous native modal loop. |
| [WebView](https://gpui-kit.com/docs/webview/) | Experimental embedding is unnecessary for conversations, agent tools, or the observed mode picker. |
| [Mobile](https://gpui-kit.com/docs/mobile/) | Experimental mobile integration is outside the macOS desktop product boundary. |
| [Packaging](https://gpui-kit.com/docs/packaging/) | Verify the exact running bundle and matching bundled helper, not only the on-disk Cargo outputs. |
| [Auto update](https://gpui-kit.com/docs/auto-update/) | Do not bolt on an updater without the matching installer/release contract; reuse existing CLI release handling. |
| [Design guides](https://gpui-kit.com/docs/design-guides/) | Keep the existing restrained shell and explicit recovery states; missing mode metadata gets actionable guidance. |
| [Coding guides](https://gpui-kit.com/docs/coding-guides/) | Share immutable palette commands, keep lifecycle guards local, and avoid a speculative feature-crate split. |
| [Framework comparison](https://gpui-kit.com/docs/comparison/) | No framework, rendering backend, or dependency migration is justified by these findings. Re-checked 2026-11-24 against the site v0.11.0 page ([CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted); the pinned `gpui-kit = "=0.10.0"` wins over the prose. The page is a capability matrix (availability, not performance), so every applicable claim is already present or deliberately skipped here and in `desktop-component-review.md`: retained state/memoized projections, bounded drain cadence instead of continuous redraw, virtualized measured rows, one TextView for selectable Markdown/HTML (HTML only for MCP App static previews, no JS engine), app-owned WCAG-AA themes and 20-locale i18n with Kit strings extended through `rust_i18n`, and rendered/a11y tests. Skipped with reason: WebView/Wry and mobile/Wasm (macOS-only product; upstream features still experimental), DataTable/column resizing and the Rope-backed Editor (no consumers; the editor was removed from the view). The 120 Hz "frame budget, not a refresh promise" nuance stays under Frame-rate monitoring below — no FPS claims or HUD. Per-claim evidence and re-verification commands: `.vtcode/memory/library.md` (2026-11-24 Comparison entry). |
| [Frame-rate monitoring](https://gpui-kit.com/docs/fps/) | Make no FPS, CPU, or allocation-percentage claims without a representative profiled build. Re-checked 2026-11-25 against the full FPS Monitor page (site v0.11.0, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted; pinned `gpui-kit = "=0.10.0"` wins over the prose). The page's load-bearing distinctions are already honored: 120 Hz is a frame budget (~8.33 ms), not a refresh promise, and GPUI draws on invalidation — an idle window does not rebuild its element tree 120 times a second; `FRAME` is only `draw_end - draw_start` while `FPS`/`INTERVAL` come from `present_end`, so one cannot stand in for the other; `INV` counts invalidations coalesced into one draw, which is why a state change must not notify when the value is unchanged. Animations request frames only while moving and stop when settled (`view/motion.rs::fade`), the backend and PTY pumps use bounded drain cadence (`view/backend_events.rs:32`, `view/terminal_panel.rs:15`), and there is no FPS HUD dependency, so the page's "exclude the monitor's own clock frames" concern does not arise. The one gap was duplicate-value notifications in the backend event projection: `view/backend_events/thread_events.rs::handle_ui_progress` notified unconditionally on an unchanged boolean, `GoalUpdated` re-stamped the goal's live-elapsed clock on every per-turn refresh, and `UpdateInstallProgress` reassigned and notified on every duplicate download tick. All three are now equality-guarded at the handler boundary, matching the standing notify-guard policy (maintainer notes), pinned by `view/tests/turns_transcript_notify.rs::unchanged_ui_progress_does_not_notify`, `view/tests/goal_bar.rs::repeated_goal_snapshot_keeps_the_original_live_elapsed_stamp`, and `view/tests/notifications_install.rs::duplicate_install_progress_ticks_do_not_notify`. Root cause fixed: `drain_events` no longer notifies on event receipt — `process_drained_event` reports whether a handler mutated view state and the drain notifies only on a real change, with a conservative `true` default for every unproven arm so a new event type cannot render a stale UI. The terminal monitor's tool-output feed stays protected (`ThreadEvent` remains conservative). Pinned by `view/tests/backend_drain_notify.rs`. No FPS improvement is claimed without a profiled build. Skipped with reason: adding `gpui-fps`/a debug HUD (no measured problem), and adopting GPUI's `FrameTimingCollector`/trace tooling speculatively (use only to diagnose a real slowdown; disabling the trace clears its buffers). No FPS, CPU, or allocation claim is made from this review. |

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

Theme swatches and light/dark appearance-profile selectors also expose their current choice with
Kit Button's toggle state. Their rendered tests verify that selecting a new theme or profile clears
the previous pressed state.

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
