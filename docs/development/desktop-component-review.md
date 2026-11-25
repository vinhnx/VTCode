# Desktop component review

Reviewed 2026-10-24 against the [GPUI Kit component catalog](https://gpui-kit.com/component/).
All 79 linked component pages and their code-example blocks were retrieved;
repeated code lines were deduplicated for review. Applicable APIs were also checked
against locally resolved `gpui-kit 0.8.0` source. This is an API/example review,
not a claim that every component was instantiated or visually tested.
Rows re-verified against the pinned `0.8.1` source say so inline with a date.
Upgraded to pinned `0.9.0` on 2026-11-09; rows whose 0.9.0 status changed say so inline.
Upgraded to pinned `0.9.1` on 2026-11-11 (backward-compatible patch; `gpui-pre 0.3.12`); rows whose 0.9.1 status changed
say so inline. 0.9.1 additions with no desktop consumer need no row: `GridLayout` (no draggable-grid workflow),
`ColumnFixed::Right` and `TimeField::separator` (no table/time-field consumers), ratio-sized PieChart hole (bar chart
only), `TransitionState`/`ReducedMotion` (no virtualized-row transition consumer; the `MotionTokens` policy stands),
and `Slider::track_focus` (no external focus-handle need at the desktop slider sites).
Upgraded to pinned `0.10.0` on 2026-11-22 (minor; `gpui-pre` stays `0.3.12`); rows whose 0.10.0 status changed say so
inline. 0.10.0 additions with no desktop consumer need no row: global RTL layout (no RTL locale), `TableRow::id`/`hover`
and `DataTable::scrollbar_mode` (no table consumers), `Checkbox::indeterminate` (all desktop checkboxes are binary),
`NumberInput` stepper (no number-input consumers), `SidebarGroup::suffix` (the rail uses `Button` rows),
`TextareaState::set_tokens`/decorations and `context_menu_offset` (the composer uses `EditorState`),
`Diff::split_ratio` (no designed ratio need), `TextView` raw-HTML-as-text (dropping `style`/`script` with no JS engine
is already adequate; flipping the default would re-render existing transcripts), and the Kit reduced-motion preference
(zero production delta over the app-level System/On/Off — the same one-shot macOS re-read — with no injectable
baseline for the deterministic restore tests). Every release-note API below was confirmed against the 0.10.0 sources
before adoption; the vendored patch carries seven deltas (see `patches/gpui-component/PATCH.md`).

Base motion reviewed 2026-11-24 against the
[GPUI Kit motion guide](https://gpui-kit.com/base/motion/) (site v0.11.0; claims re-verified against pinned
`gpui-base 0.10.0` and `gpui-fast 0.1.17`). Keyed reduced-motion-aware animations, absolute-time sampling and
frame scheduling are already supplied by `with_animation`, and the OS/app preference stays in
`desktop/preferences.rs`. Adopted from the interruption/reversal guidance: `view/motion.rs` is now the single
timing/animation builder for mount and navigation fades, and `NavigationMotion` resumes a retarget from the last
sampled opacity with a proportionally shortened duration instead of flashing to transparent — keyed on the sampled
value, not the clock, since a shortened reveal settles before its base duration and the Workspace route resets the
clock without painting a fade. `transition`/`spring`/`Keyframes`/`Presence`/`Sequence`/`Stagger`/`MotionReveal` remain
SKIPPED: no desktop consumer needs them and the recorded reduced-motion preference decision stands.

Animation reviewed 2026-11-24 against the
[GPUI Kit animation guide](https://gpui-kit.com/docs/animation/) (site v0.11.0,
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted; claims re-verified against pinned
`gpui-fast 0.1.17` `Animation`/`AnimationExt` and `gpui-base 0.10.0` `Easing`). The three-level choice (mounted
one-shot → GPUI `Animation`, retargetable target → Base motion, styled control → `motion_tokens()`), one-shot ID and
replay semantics, interruption/reversal, domain-derived channel ids, reduced-motion endpoints, and "a spinner needs a
static text cue" are ALREADY PRESENT in `view/motion.rs`, `NavigationMotion`, `swipe_navigation_overlay.rs`,
`components.rs::shimmer_label`, `preferences.rs`, and every spinner call site. `Presence`, `with_spring`,
`with_animations` and `repeat_synced` stay SKIPPED: no consumer and the recorded decisions stand. APPLIED: both
opacity wrappers (`FadeInExt`, `NavigationFadeExt`) now delegate to the single `motion::opacity_fade` builder, so the
animation and its animator cannot drift; `fade_animation` clamps the eased value to 0..=1 because opacity is narrower
than GPUI's easing output, and `shared_opacity_fade_is_a_finite_oneshot` pins that contract with an overshooting curve.

Icons & Assets reviewed 2026-11-24 against the
[GPUI Kit assets guide](https://gpui-kit.com/docs/assets/) (site v0.11.0,
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted; claims re-verified against pinned
`gpui-kit-assets 0.10.0` generated `IconName`/`icon_assets!` and the desktop asset source). "Depending on the crate
does not embed every icon" is ALREADY HONORED: `with_assets(DesktopAssets)` is registered before any window opens,
and the composite source checks `ExtraIcons` before falling back to the 106 default `KitAssets`, merging/sorting/
deduping `list` so every served key is listed. `Icon::data` for the one-off brand V monogram, `svg()` for the
monochrome mark and `img()` for full-color app-icon art are already correct — no changes. `AllAssets` (1,830 SVGs)
and a `rust-embed` asset folder stay SKIPPED: the app references 68 named glyphs and its app icons already ship via
`include_bytes!`. The 0.11.0 shared-vs-legacy `IconName` compatibility note is moot — the desktop imports
`assets::IconName` everywhere. APPLIED (root cause, DRY): the hand-maintained `DESKTOP_ICONS` catalog could not catch
a new `IconName::X` used in UI code without a matching registration — an unregistered path renders blank with no
headless failure. `desktop/assets.rs` now tokenizes every `src/**/*.rs` with `proc-macro2`, resolves the collected
`IconName::Variant` pairs through `IconName::ALL`, and asserts each `path()` loads through `DesktopAssets`.
Tokenizing (rather than matching source text) ignores comments and string literals, keeps multiline paths and macro
bodies in scope, and avoids false failures from fake variants mentioned in prose; the scan unit test pins all of
those cases plus the `ALL` exclusion.

Images reviewed 2026-11-24 and deep-passed 2026-11-25 against the
[GPUI Kit images guide](https://gpui-kit.com/docs/image/) (site v0.11.0,
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/), adapted; claims re-verified against pinned
`gpui-fast 0.1.17` `elements/img.rs`, `image_cache.rs`, `platform.rs` and `window.rs`). ALREADY PRESENT: both desktop image
holders (`files::Preview::image`, `computer_preview::Body::image`) are `Arc<gpui_kit::Image>` — encoded bytes already
held — so there is no URL, asset-key, filesystem-path or custom-loader path to fix, and no native `HttpClient` is
needed. `brand_mark` sets both `.text_color(...)` and `.size(...)` on the `svg()` element itself; full-color art uses
`img()` with explicit bounds; no `with_loading` consumer needs `.id(...)`. SKIPPED: HTTP-layer caching (no remote
images), scoped caches (the App cache already keys by source), and reserving a box for the screenshot preview (bytes
are already held and the card sets width; a fixed height would letterbox varying aspect ratios — a design decision,
not a code fix). APPLIED 2026-11-25 (reverses the earlier "no animated image" verdict): `files::is_image_path` accepts
gif/webp, so the viewer does render animated formats, but pinned `elements/img.rs` advances frames only when the
`Img` has retained element state, which requires an `ElementId`. The viewer's `img()` now carries
`.id("file-preview-image")`; decode failures happen after the file read (so `open_image_viewer` cannot catch them) and
now use `.with_fallback(...)` with an accessible `file-preview-unavailable` node reusing `app.files.preview_unavailable`.
Pinned by `view/tests/files_preview.rs::file_preview_image_keeps_a_stable_identity` and
`::undecodable_image_shows_the_preview_unavailable_fallback` (both fail with the old chain). `.test_support()` stays
last: it wraps the element in `Observed<E>`, which ends image-specific builder methods.

Focus reviewed 2026-11-24 against the
[GPUI Kit focus guide](https://gpui-kit.com/docs/focus/) (site v0.11.0; claims re-verified against the pinned
`gpui-fast`/`gpui-pre` sources). ALREADY PRESENT: `view/bootstrap.rs:217` registers one `cx.on_focus_lost` per window
on the root view and restores through `window.focus_lost_restore_target(cx).unwrap_or(root.focus_handle)` — the guide's
exact recipe for "focused element stops rendering". Retained handles get `tab_stop(true)` in bootstrap and are
re-attached with `.track_focus(...)` each render; overlay flows save `window.focused(cx)` and restore on dismissal;
`contains_focused` drives region activity. SKIPPED: no new trap or restore path — every custom surface already pairs
`.focus_trap(...)` with entry/dismissal focus ownership.

Re-verified 2026-11-25 against the supplied Focus Guide text: the root `on_focus_lost` recipe, `tab_stop(true)` on
retained handles, saved/restore overlay focus, `contains_focused`, and paired `.focus_trap(...)` entry/dismissal all
stand. APPLIED (real gap): the element carrying `key_context::COMPOSER` had no tracked restore handle, so a focused
child that unmounts inside it (the stop button when a turn ends) restored to the root DESKTOP node and lost composer
bindings. `composer_render.rs` now tracks a dedicated non-Tab-stop handle on that wrapper, retained across renders via
`window.use_keyed_state("composer-focus-region", …)` — the guide's stateless-component pattern. Reusing the composer
input's own Tab-stop handle instead adds a second Tab stop (the guide's explicit warning) and breaks dialog Tab traps,
so a separate anchor is required. Pinned by
`view/tests/exec_controls.rs::ending_a_focused_stop_button_restores_the_composer_context` (fails without the tracking
line: focus falls back to root; also asserts a COMPOSER-scoped `ctrl-m` binding fires after restore).

Base plot reviewed 2026-11-24 against the
[GPUI Kit plot guide](https://gpui-kit.com/base/plot/) (site v0.11.0; the pinned
[`v0.10.0` page](https://gpui-kit.com/versions/v0.10.0/base/plot) carries identical prose, re-verified against
`gpui-base 0.10.0` `src/plot/`). The base owns geometry and takes every color from the caller; `component::plot`
is only a compatibility re-export, so the sole plot consumer (`view/storage.rs`) now imports `BarAlignment` from
`base::plot::shape`. `PlotValue` confirms the existing `as f64` byte cast (integers are excluded because linear
scales divide). `PathCaches`, `Curve`, axes, grids and hover overlays stay SKIPPED: the storage chart is the only
plot, its axes/grid/tooltip/interaction are deliberately off, and the full breakdown lives in the aria label.
`PlotAppearScope` and `PlotMotion` add nothing over the 2026-11-24 motion review and the chart's `.appear(false)`.
Applied the reference's datum model as a DRY fix: the four categories are one named `StorageUsageDatum` array
feeding the chart bands, the aria breakdown and the legend rows, with colors still handed in by the styled layer.

Base text selection reviewed 2026-11-24 against the
[GPUI Kit text selection guide](https://gpui-kit.com/base/text-selection/) (site v0.11.0; the pinned
[`v0.10.0` page](https://gpui-kit.com/versions/v0.10.0/base/text-selection) carries identical prose,
re-verified against `gpui-base 0.10.0` `src/text_selection.rs`, `src/selectable_text.rs`, `src/root.rs` and the
vendored `patches/gpui-component/src/root.rs`). The window layer, scopes, snapshots, multi-click boundaries,
touch handles and advanced participant adapters are already Kit-owned: `gpui_kit::open_window` mounts `base::Root`,
whose render installs one `TextSelectionLayer` per window (`gpui-base/root.rs:261`), `RootPlugin::prepare`
activates the scope stack (vendored `root.rs:722-727`) while `present_dialog`/`present_sheet` clear selection on
open (`root.rs:415-417`, `:548-552`), and
`TextView` retains a stable handle and projects runs into UTF-8-safe byte ranges. Kept as-is: terminal drag
selection stays rio-vt-owned (VT grid cells and semantic word/line expansion have no `TextSelectionRun`
equivalent); app-owned scrim modals keep their own focus/copy flow because Kit scopes cover Kit dialogs/sheets
only and an ordinary pointer press clears the window selection in `prepare_for_mouse_down` (shift-click and
handle presses are the deliberate exceptions); `highlights`/`reveal_range`
stay unused because the inspector filters lines and conversation search jumps messages rather than byte ranges;
touch selection stays off because the bundle must never declare `NSIsTouchNative`. Applied the guide's plain-text
participant pattern to the one real gap: the inspector output body now renders through `SelectableText`
(`markdown.rs::render_selectable_output_text`), so a drag selects and copies exactly the displayed capture while
the compact transcript excerpt stays plain (its row owns click-to-expand and a full-output copy button). Pinned
by `markdown/tests.rs::captured_output_text_selects_and_copies_from_a_drag`.

Base text view reviewed 2026-11-24 against the
[GPUI Kit TextView guide](https://gpui-kit.com/base/text-view/) (site v0.11.0; the pinned
[`v0.10.0` page](https://gpui-kit.com/versions/v0.10.0/base/text-view) carries identical prose, re-verified
against `gpui-base 0.10.0` `src/text/{text_view,state,style,node}.rs`). Window setup (one selection layer from
`base::Root`), Markdown/HTML constructors, default styling through `TextViewStyle::from_theme` with the desktop's
code/link/muted/paragraph/inline-code/table overrides, opt-in syntax highlighting (`syntect` feature, pinned by
`markdown/tests.rs::retained_markdown_repaints_inline_code_and_tables_in_both_theme_directions`), per-window link
filtering, blocked images, and reduced-motion-aware stream fades are already in place. The
`TextViewState` recommendation does not apply: VT Code re-renders a keyed `TextView::markdown` each frame, and
pinned `state.rs::set_element_text`/`set_text` retain the state entity, skip unchanged sources, and append extending
text through `push_str` (which is what drives the stream fade). Selection across presentation changes stays
Kit-owned. SKIPPED (no consumer or recorded decision): `MarkdownExtensions`/frontmatter/inline and block plugins,
`block_surface`, `editor_host`, `table_actions` (no designed whole-table copy affordance), `max_lines` (transcript
grows to fit while the inspector viewer uses `scrollable(true)`), `with_table_row_hover`, and the newer
`with_inline_code_style`/`list_style`/`blockquote_style` refinements over the already contrast-tested style.
APPLIED (real gap): fenced code blocks now opt into Kit's scroll mode with a 24rem cap in `markdown.rs::markdown_style`.
Caused by an agent emitting a whole file in one fence: the block grew without bound inside the virtualized
MessageScroller, inflating measured row geometry and burying the conversation. Pinned `style.rs::with_code_block`
documents that `overflow.y = Scroll` plus a max height gives the block its own scrollbar and consumes the wheel until
its edge; `node.rs:2728-2758` implements it. Pinned by `markdown/tests.rs::long_fenced_code_is_height_capped_and_scrollable`.

Styling reviewed 2026-11-24 against the
[GPUI Kit style guide](https://gpui-kit.com/docs/style/) (site v0.11.0; claims re-verified against pinned
`gpui-fast 0.1.17` `styled.rs`, vendored `gpui-component 0.10.0` `status_bar.rs`, and the desktop checkout).
Viewport-width breakpoints (`view.rs`, `view/settings.rs`), rem-based spacing tokens (`themes/layout.rs`),
theme-token colors, gap-on-parent/padding-on-surface sizing, fixed rail plus `flex_1().min_w_0()` content,
header-beside-bounded-scroll regions, `relative()`/`absolute()` overlays with no `z_index`, and one scroll
owner with its scrollbar and inner padding (`view/components.rs::scroll_column_inner`) are all already in
place. The guide's warning that `.truncate()` alone keeps a nowrap flex item's intrinsic width was checked,
not assumed: pinned `styled.rs::truncate` sets only overflow/nowrap/ellipsis, yet the desktop status bar's
`min_w_0` rows already release its labels — pinned by
`tests/settings_spacing.rs::status_bar_long_labels_shrink_instead_of_pushing_siblings_out` at 14px and 24px.
The fluent builder contract (`.when`/`.when_some`/`.map` are frame-local, not subscriptions; `Styled` vs
`ParentElement` vs `InteractiveElement` traits) is already the codebase's normal composition style. The
one cosmetic gap is `view/updates.rs::changelog-highlights`, which tracks a `ScrollHandle` and has a bounded
`max_h` but no visible scrollbar; adding `vertical_scrollbar` is deferred because there is no headless
scrollbar affordance to assert and no report that the scroll is undiscoverable. No production styling change
followed from this review.

Coding guides reviewed 2026-11-24 against the
[Coding Guides](https://gpui-kit.com/docs/coding-guides/) page (site v0.11.0 prose,
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); the supplied text is
newer than the vendored `gpui-kit` skill copy, which was updated with the three
new sections). Claims re-verified against pinned `gpui-kit 0.10.0` /
`gpui-pre-util 0.3.12` `SharedString`. "Own persistent UI text with `SharedString`"
and "make the common path inexpensive" found one real hot-path gap:
`view/palette.rs` retained each entry's label/description and each group label as
`String`, while `palette_render.rs::render_command_palette` clones the whole group
table on every frame to feed `'static` Kit row closures — for conversation search
that copies full message text per rendered row. APPLIED (single ingestion
boundary, DRY): `PaletteCommand::{label, description}` and `PaletteGroups`' group
labels are now `SharedString`, converted once in `share_palette_commands` and at
the `command()` builder, so per-frame clones are refcount bumps. `accessible_label`
became `SharedString` too, and the two per-row keyword `Vec` clones now pass
`.iter()` into Kit's `keywords()` builder (which copies into its own state),
removing two more allocations per row per frame. "Keep ownership links and subscriptions bounded" is
ALREADY PRESENT: child→parent references are `WeakEntity`, subscriptions live in
explicit owners, and all 12 `.detach()` calls are one-shot `Task`s (allowed) with
no `Subscription::detach()`. "Bind keys before `set_menus`", stable domain IDs,
rem-size-synced cached geometry, and controlled-value feedback rules are already
recorded as applied above. SKIPPED with reason: converting
`vtcode-desktop-protocol` response fields to `SharedString` would add a GPUI type
to the published, GUI-agnostic wire crate; the desktop-only interaction/curated
projections are individual short strings, not a measured hot path. Pinned by
`view/palette.rs::palette_group_clone_shares_display_text_without_copying_bytes`
(pointer identity across a group-table clone; fails with `String` fields) and the
existing `model_palette`/`prompt_history`/`search_config_conversation` suites.
Repairing the pre-existing 1003-line `search_config.rs` hard-ceiling violation
split its 7 cohesive `conversation_search_*` tests into
`view/tests/search_config_conversation.rs` (same `super::super::` depth because it
is a sibling, not a child). Re-verify: `cargo fmt --all -- --check`,
`RUSTFLAGS="-D warnings" cargo clippy --locked -p vtcode-desktop --bin vtcode-desktop --all-targets`,
`cargo nextest run --locked -p vtcode-desktop` (1916/1916 passed, 1 skipped),
`./scripts/check-dev.sh --changed` (all checks passed).

The supplied VT Code concept guides the design: quiet navigation,
conversation-first hierarchy, recessed inspector surfaces, compact native actions,
and a branded composer. Reuse behavior without replacing the existing runtime,
measured-list, approval, persistence, or accessibility owners.

| Component | Decision |
| --- | --- |
| [accordion](https://gpui-kit.com/component/accordion) | Retain app-owned pane visibility; no extra disclosure or animated layer needed. |
| [alert](https://gpui-kit.com/component/alert) | Retain inline error and draft-recovery state with existing ownership. |
| [alert-dialog](https://gpui-kit.com/component/alert-dialog) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. |
| [attachment](https://gpui-kit.com/component/attachment) | Applied: metadata cards and accessible file-removal actions. Echo cards share the composer chip surface through `attachment_cards::base_attachment_card`, including the in-app View action (echo-scoped ids, no remove). |
| [avatar](https://gpui-kit.com/component/avatar) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [badge](https://gpui-kit.com/component/badge) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [bubble](https://gpui-kit.com/component/bubble) | Applied: sender headers, outlined user prompts, unframed assistant responses, and copy footers. User echoes with attachments stack `Attachment` cards above the prompt (rich content; View action, no remove). |
| [button](https://gpui-kit.com/component/button) | Shared Primary, Secondary, Danger, and Ghost roles. Primary uses an opaque theme accent with subtle hover. Enabled Primary and Danger actions share soft neutral shadows and subtle face shading. Kit supplies disabled and focus paint. See [button roles](maintainer-notes/vtcode-desktop.md#button-roles). 2026-11-21: `Selectable::open` no longer paints the selected style (vendored-patch change) — the open popup is the state indicator, so palette trigger pills stay quiet while up; dropdown triggers use the `desktop_trigger_pill` quiet filled body. |
| [calendar](https://gpui-kit.com/component/calendar) | Not needed for the current coding-workspace workflows or available data. |
| [carousel](https://gpui-kit.com/component/carousel) | Not needed for the current coding-workspace workflows or available data. |
| [chart](https://gpui-kit.com/component/chart) | Applied: storage usage breakdown as a labeled 4-category bar chart, replacing the hand-rolled proportional bar; axes, grid, tooltips, and interaction stay off, and the full breakdown remains in the container aria label. The 0.9.1 ratio-sized PieChart hole is N/A (bar chart only). |
| [checkbox](https://gpui-kit.com/component/checkbox) | Applied: controlled opt-in for untracked Git previews; preserve runtime-owned approval identities. |
| [clipboard](https://gpui-kit.com/component/clipboard) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [collapsible](https://gpui-kit.com/component/collapsible) | Retain app-owned pane visibility; no extra disclosure or animated layer needed. |
| [color-picker](https://gpui-kit.com/component/color-picker) | Applied to provider-supported reasoning effort with native step buttons and typed runtime updates. |
| [combobox](https://gpui-kit.com/component/combobox) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. 0.10.0 `highlight_matches` skipped 2026-11-22 (no `Combobox` consumers in the desktop view; same custom-row reasoning as select). |
| [command](https://gpui-kit.com/component/command) | Applied: retained filtered context/model palettes and conversation search; stable message/activity IDs jump through the measured MessageScroller, reveal tool details, open captured output in the inspector, and load bounded older-history pages without replacing the archive pager. 0.10.0 `highlight_matches` skipped 2026-11-22: palette rows are custom `.child()` content, which bypasses the Kit highlight path entirely (`render_item_with_binding` renders custom content verbatim); matches are already highlighted via `Label::highlights`. |
| [data-table](https://gpui-kit.com/component/data-table) | Not needed for the current coding-workspace workflows or available data. |
| [date-picker](https://gpui-kit.com/component/date-picker) | Not needed for the current coding-workspace workflows or available data. |
| [description-list](https://gpui-kit.com/component/description-list) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [dialog](https://gpui-kit.com/component/dialog) | Applied: larger Appearance overlay, heading parts, contained focus, and focus restoration. 0.9.1 `Dialog::accessibility_label` verified 2026-11-11: there are no Kit Dialog consumers — all nine scrim modals are app-owned and already carry `Role::Dialog` + `aria_label` — so there is nothing to adopt. |
| [diff](https://gpui-kit.com/component/diff) | Applied: retained session and scoped Git snapshot review, unified/split modes, soft wrapping, bounded raw fallback, and explicit unavailable/partial states from runtime file metadata. Appearance and context options are user-configurable (Settings → Appearance → Diffs, persisted in the `preferences.json` `diff` struct; layout stays toolbar-owned per surface). 0.9.0 `Diff::decorations`/gutter markers deferred 2026-11-09 (no designed per-line consumer; future hook for review actions). 0.9.1 pins invalid-hunk-prefix rejection in Kit tests only (`parser.rs` byte-identical); the desktop fail-closed boundary is pinned by `changes_inspector_fails_closed_on_invalid_hunk_prefixes_but_keeps_valid_unicode_previews`. |
| [dock](https://gpui-kit.com/component/dock) | Retain tested three-pane resizing and persistence; avoid unnecessary docking migration. Re-confirmed 2026-11-12 for the terminal panel: a single configurable bottom/left/right dock needs no tab groups, splits, or serialized layouts; evaluated `DockArea`/`set_dock`/`dump`/`load` against the composed regions and kept them (full analysis in session notes). Applied the reference's persistence principle 2026-11-24: `LayoutChanged` fires per interaction step, so splitter arrow-key resizes now defer the fsync-backed write to key release instead of every key repeat (pointer drags already saved once on drop). The `DockArea` verdict is unchanged. |
| [dropdown_button](https://gpui-kit.com/component/dropdown_button) | Re-confirmed 2026-11-08: no split-button behavior needed. Every `.dropdown_menu` site is a menu-only trigger (thread chrome menu, thread-strip and inspector overflows) with no primary action to pair — joining the overflow with its neighbor button would conflate distinct tasks (switch vs create, navigate vs dismiss). Composer pickers open searchable palettes, not `PopupMenu`s, so a split cannot host them. The one apparent candidate fails the promise test: a Send split offering "Queue as one turn" would duplicate Send when idle (an idle batchable submit goes out immediately with no flag) while the split hides during a run, when batching is meaningful. |
| [editor](https://gpui-kit.com/component/editor) | Applied: the markdown preview Source arm renders a persistent `EditorState` (`.language("markdown")`, syntect highlighting follows the app theme, `appearance(false)` keeps the borderless body look). The kit editor has a fixed intrinsic height and no `auto_grow`, so the preview sizes it from the document line count clamped to 3–24 rows (2026-11-07). Editing shipped 2026-11-07 as an Edit mode toggle (md-only, hidden for truncated previews whose editor holds just the head): dirty-tracked Save over a background write mirroring the rename commit flow, errors through `preview_editor_error`, and Escape exits edit mode before dismissing since editable editors consume Tab for indent; Cmd+S saves from edit mode through a fixed `SavePreviewSource` binding (reserved against configurable overrides), mirroring the Save button. 0.9.0 gutter/overview-ruler decorations skipped 2026-11-09 (the preview editor carries no VCS line data). Still no `auto_grow` on `EditorState` in 0.9.1 (`auto_grow`/`set_rows` re-verified textarea-only in `input/base/state.rs`); the gutter skip stands. Removed 2026-11-16 with the text/markdown preview and its edit/save flow (files open externally now; the overlay is images-only): `Editor`/`EditorState` no longer referenced from the desktop view. |
| [empty](https://gpui-kit.com/component/empty) | Applied to panel empty states. Icon media uses the framed `EmptyMediaVariant::Icon` tile and brand media uses the default variant. The config empty state offers `app.workspace.choose_folder`. The conversation welcome shares the composer column so the greeting stays beside the input in normal and Zen modes. The thread-browser header owns New thread. |
| [focus-trap](https://gpui-kit.com/component/focus-trap) | Applied: larger Appearance overlay, heading parts, contained focus, and focus restoration. All nine scrim-backed modals trap Tab as of 2026-11-07 (running-confirm, full-access, file-rename, file-preview, and about joined the four already trapped; the attachment picker trap is non-scrim); route panels stay deliberately untrapped (`tab_out_and_back_into_scope`); every trapped surface pins the wrap contract (2026-11-07: endpoint-exact via `assert_tab_trapped_within_dialog` for about/rename/preview/full-access/running/changelog/license, containment loops for the palette, cli-config enum editor, and attachment picker whose interior stops carry kit-owned ids — plus a closed-Select id-less stop quirk, see gotchas). |
| [form](https://gpui-kit.com/component/form) | Use current grouped, bounded preference controls; do not introduce a parallel global settings store. |
| [group-box](https://gpui-kit.com/component/group-box) | Applied: settings cards and grouped typography controls. Reviewed 2026-11-24 against the [GroupBox guide](https://gpui-kit.com/component/group-box) (site v0.11.0; pinned `gpui-component 0.10.0` `src/group_box.rs` carries the same API, including `footer`, `title_style`, `content_style`, and the three variants). All other claims are already present: `view/components.rs` owns `settings_group_box`/`settings_group_box_untitled`/`card_border_style` and the pre-existing `.footer()` consumers (`shared_preferences.rs`, `configuration_render.rs`) pass plain text as the guide recommends; `accessible_group_box` wires the domain id and `Role::Group`. The one untested contract was the footer geometry the `card_border_style` comment relies on — footer outside the surface, one `gap_2` below it, on the title spine, not inset by `content_style` `p_6`. Now pinned by `view/tests/settings_spacing.rs::settings_group_footer_sits_below_the_surface_on_the_title_spine` at 14px and 24px. Also applied the guide's "supporting text outside the surface" rule to `view/shortcuts_render.rs`: the shortcuts usage hint was a hand-styled last child inside the filled card; it now uses Kit's `.footer()` seat, so it inherits the shared small muted caption treatment and sits on the title spine below the card. Pinned by `view/tests/shortcuts_editor.rs::shortcut_usage_hint_is_a_footer_outside_the_group_surface` (verified to fail with the old in-surface placement). `title_style` stays skipped: the desktop's `group_box_title` already owns heading typography as a styled child, so routing it through the Kit refinement would duplicate one ownership source without behavior change. |
| [hover-card](https://gpui-kit.com/component/hover-card) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [icon](https://gpui-kit.com/component/icon) | Retained and restyled: accessible buttons, packaged icons, and embedded brand artwork. 0.9.0 `set_icon_resolver` skipped 2026-11-09 (`DesktopAssets` already covers the extra-icon catalog; no runtime icon-theme feature exists). Still skipped in 0.9.1 (API present in `icon.rs`; still no icon-theme feature). |
| [image](https://gpui-kit.com/component/image) | Retained and restyled: accessible buttons, packaged icons, and embedded brand artwork. |
| [input](https://gpui-kit.com/component/input) | Retain rich Markdown, owned input state, raw copy, and image/security policy. 0.9.0 `InputEditsEvent` evaluated 2026-11-09 and skipped (the Change-driven flows have no per-edit consumer). Still skipped in 0.9.1 (event present; still no per-edit consumer); the caret/layout fixes are automatic. |
| [input-group](https://gpui-kit.com/component/input-group) | Applied: shared composer frame with block-end controls. |
| [kbd](https://gpui-kit.com/component/kbd) | Retain current command and runtime surfaces; icon controls keep accessible names and shortcut hints. |
| [label](https://gpui-kit.com/component/label) | Reviewed presentation options; keep essential metadata visible and avoid extra decoration/state. |
| [list](https://gpui-kit.com/component/list) | Retain keyboard-capable navigation buttons; resolved SidebarMenu rows do not offer equivalent keyboard handling. 0.9.1 accent rows (list and searchable option rows alike) pair `accent_foreground` text, covered by the pinned 4.5:1 `(accent, accent_foreground)` pair; the 0.7-opacity hover composite inherits the floor from its pinned endpoints. |
| [marker](https://gpui-kit.com/component/marker) | Applied (2026-11-04): severity status rows through the shared `severity_marker` builder (transcript notices keep a solid/outlined tone marker, roles, and the trailing Show/Hide toggle), shimmer loading rows (`turn_pending_row`, history paging (normalizes an inherited 16px foreground label to `text_sm` muted like its siblings), file scan, MCP preview with spinner) via `loading_marker_row`, and a `Separator` for the history-limit boundary. Callers keep test identity/roles/aria labels on their wrapper: `Marker` is `RenderOnce` and cannot carry `.test_support()`. Retained: bordered alert cards, tool-status `Tag`s, inline `ShimmerText` embeddings, one-off `text_xs`/centered loading rows (navigation, output page, new-thread), the unlabeled `turn_divider` hairline. Rejected: unread/delivery/presence rows (no such models), filled pills in the transcript, and execution percentages/phases beyond existing runtime events. Shimmer tuning is no longer rejected as of 2026-11-06: marker rows take the same resting/highlight pair through `with_shimmer_style`, documented on the shimmer row. 2026-11-20: transcript severity rows are one shared announcement row (`view/transcript_notice.rs`): no card surface/border/icon — a tone marker in the shared icon lane, theme-derived tone title, muted body, and ghost details CTA, all resolved from `themes/layout.rs`. Kit `Alert` stays Retained (its fixed role and close-only action cannot carry the details/stack CTAs); the `Marker` rejection itself still stands. |
| [menu](https://gpui-kit.com/component/menu) | Applied: thread overflow uses canonical guarded new/close commands. 0.9.1 rem-sized items, scroll-to-checked, and Kit-owned outside-press dismiss (`handle_dismiss`/`on_mouse_down_out` still present) are automatic. |
| [message](https://gpui-kit.com/component/message) | Applied: sender headers, outlined user prompts, unframed assistant responses, and copy footers. |
| [message-scroller](https://gpui-kit.com/component/message-scroller) | Applied: transcript mounts MessageScroller with `TranscriptLayoutChange` sync (reset/prepend/splice/remeasure, pause/follow tail). App-owned navigation composes from Kit readers: scroll-to-first overlay, shared jump-to-message helper (palette search, activity rows, deep links), click-to-anchor rows. 0.9.0 notes (2026-11-09): Kit moved list padding onto rows — scroll-away uses positive pixel delta (`scroll_transcript_up`), and overlay buttons stop propagation so clicks don't fall through to the row beneath. Unchanged in 0.9.1 — carries forward. 0.10.0 `scroll_to_offset` skipped 2026-11-22: it restores a saved offset, and there is no desktop save/restore flow and still no visible-range query (the only scroller API delta). |
| [notification](https://gpui-kit.com/component/notification) | Applied system-only notices for inactive-window run completion, failure/blocking, and pending decisions; preserve transcript/interaction state as the source of truth and route clicks through the originating Kit window. |
| [number-input](https://gpui-kit.com/component/number-input) | Use current grouped, bounded preference controls; do not introduce a parallel global settings store. |
| [otp-input](https://gpui-kit.com/component/otp-input) | Not needed for the current coding-workspace workflows or available data. |
| [pagination](https://gpui-kit.com/component/pagination) | Not needed for the current coding-workspace workflows or available data. |
| [plot](https://gpui-kit.com/component/plot) | Low-level scales/shapes powering Chart; `base::plot::shape::BarAlignment` orients the storage chart — no custom plot surface needed. Base doc reviewed 2026-11-24 against pinned 0.10.0 (paragraph above). |
| [popover](https://gpui-kit.com/component/popover) | Replace crowded Appearance popover with a larger modal surface. The 0.9.1 `popover.rs` diff is test-only; trigger open-state behavior carries forward. |
| [progress](https://gpui-kit.com/component/progress) | Do not invent execution percentages or phases beyond existing runtime events. |
| [questionnaire](https://gpui-kit.com/component/questionnaire) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. Visual language adapted (2026-11-03): borderless panel, counter eyebrow above title above choices, approval marker icon plus tint; digits stay (immediate-commit contract), auto-advance not adopted. Restyled (2026-11-06): framed popover card, borderless rows with checked wash, neutral Recommended Tag pill, header dismiss; approval tint removed, marker icon kept. |
| [radio](https://gpui-kit.com/component/radio) | Preserve runtime-owned approval/choice identity and existing guarded callbacks. |
| [rating](https://gpui-kit.com/component/rating) | Not needed for the current coding-workspace workflows or available data. |
| [resizable](https://gpui-kit.com/component/resizable) | Applied standalone Kit handles with splitter semantics and keyboard resizing; preserve custom width bounds/persistence instead of migrating independent panes into a panel group. |
| [root](https://gpui-kit.com/component/root) | Applied: `gpui_kit::open_window` owns the single Root; initialize Kit before opening, and retain tested native menu/window/reopen lifecycle. 0.9.1 adds Linux window-corner following only; `Root` rem sync (`set_rem_size` in `prepare`) re-verified. |
| [scrollable](https://gpui-kit.com/component/scrollable) | Applied themed Kit scrollbars to settings and file preview; recent sessions use a virtual list with a Kit scrollbar. |
| [select](https://gpui-kit.com/component/select) | Applied: retained filtered context/model palettes; stable sections survive asynchronous discovery. 0.10.0 `highlight_matches` skipped 2026-11-22: the sole searchable select (UI language) renders custom accessible rows, and the flag would swap them for default title-only rows unless `render_highlighted` is also overridden; the new `list_highlight_foreground` token only affects Kit-rendered rows. |
| [settings](https://gpui-kit.com/component/settings) | Adopted, coarse-per-page v1 (2026-11-08; reverses the 2026-11-03 verdict): Kit `setting::Settings` hosts all 7 tabs plus CLI configuration on wide viewports (`view/settings_kit.rs` remount id `settings-kit-{page_ix}` drives view-to-Kit selection; Kit search filters pages via tab keywords; fuzzy palette, deep links, tab-cycle, and dismissal preserved; narrow viewports keep the legacy stacked navigation and content). Pane renderers converted to props-and-handle content builders (`*Props::capture` + `(props, view, window, cx)` associated fns; Typed `SettingField`s bind App globals, so all pane content hosts as Element items). Pinned 0.9.0 gaps designed around, not fixed (re-verified 2026-11-09 and 2026-11-11 against pinned 0.9.1; every ask below is still open): no controlled selection or on-select (Kit-internal sidebar clicks navigate Kit only, so WebMCP selection side effects stay view-driven — pinned in the wrapping test; the storage summary fetches on display from its Kit item closure instead, since Kit clicks bypass view selection), sidebar rows click-only and unlabeled (keyboard path is tab-cycle actions plus the palette; the panel-level key context runs only from the panel root and re-propagates for focused controls), initial `group_ix` ignored on fresh mount (section links land on pages), substring-only search (palette kept), no narrow stacking (legacy fallback), English-only search placeholder. Per-group decomposition follows page by page; upstream asks tracked: controlled selection + on-select, sidebar keyboard nav and labels, `search_placeholder` override, narrow stacking, controlled query, scroll-to-item, custom page icon (VtCode page keeps a Lucide Bot meanwhile). 0.9.1 adopts nothing new: Escape-clears-search, the locale-following placeholder, and the unmatched-search empty state are automatic (Escape clears a non-empty query and propagates otherwise, matching panel dismissal order). |
| [sheet](https://gpui-kit.com/component/sheet) | Retired for Settings: the unified full-screen Settings route (left navigation, content, Back/Escape/⌘[ dismissal to the workspace with composer focus) replaces the Root-managed right Sheet, so body-level actions stay reachable and shortcut capture suppresses bindings by omitting key contexts while armed; the inspector remains an ordinary workspace pane. |
| [shimmer](https://gpui-kit.com/component/shimmer) | Applied: `ShimmerText` status labels through the shared `shimmer_label` builder, each tied to an owned runtime state (turn/history/navigation/storage/preview/output loading) with Kit's reduced-motion static path; keep status roles and aria labels on the owning row. Color contract (2026-11-06): the builder owns one `(resting, highlight)` pair — resting is the caller's tone dimmed to the 3:1 quiet floor (`themes::shimmer_resting`, anchored to `colors.popover` like `muted_text` because raised seats move toward the text color in both polarities, so holding the floor on the hardest seat holds it everywhere; anchoring to the canvas instead lands at ~2.7:1 on the panel/popover seats these labels use), and the sweep is pinned to the palette foreground so it always outshines the text it crosses. Kit's own band derivation was replaced because it mixes the text toward a pole and flips to the opposite one within 0.1 lightness (Ciapre: muted and foreground are 0.097 apart, so the band painted darker than the text), and on low-headroom palettes the "far pole" is the canvas itself. Reduced motion keeps the full semantic tone for both halves. Verified complete 2026-11-07: every owned load has a shimmer surface behind the `global_loading_active` gate, Kit motion defaults are kept, and the contract stays pinned by `contrast_tests.rs` plus the reduced-motion render test. |
| [sidebar](https://gpui-kit.com/component/sidebar) | Applied its labeled navigation structure to the rail and workspace/thread panel. Keep VT Code's keyboard-capable Kit Button rows and virtualized session list; Kit SidebarMenu rows are click-only tree items without equivalent keyboard handling. |
| [skeleton](https://gpui-kit.com/component/skeleton) | Applied to owned pending states (Git review, config loading, file scan, history paging, turn pending) through the shared `skeleton_line`; preserve its status label and render static theme-colored placeholders when reduced motion is enabled because Kit's Skeleton pulse is unconditional. |
| [slider](https://gpui-kit.com/component/slider) | Applied to persisted status-bar visibility; disabled/default state remains native. 0.9.1 min/max-build, step-snap, and assistive-technology thumb fixes are automatic (`track_focus` needs no row; see header). |
| [speech](https://gpui-kit.com/component/speech) | Deferred: microphone/recognition integration is outside this UI pass. |
| [spinner](https://gpui-kit.com/component/spinner) | Do not add decorative loading/motion without an owned runtime state and reduced-motion behavior. |
| [status-bar](https://gpui-kit.com/component/status-bar) | Applied Kit's left/right regions to the optional workspace and runtime status row; retain VT Code palette colors, the labeled status group, and the live agent-status announcement. |
| [stepper](https://gpui-kit.com/component/stepper) | Applied only to ordered multi-step runtime questions; keep the step order and current index runtime-owned, and disable direct navigation when the runtime requires sequential answers. Never invent execution phases or percentages. |
| [switch](https://gpui-kit.com/component/switch) | Applied: separate light/dark appearance profiles have persisted theme and typography effects with accessible state. |
| [table](https://gpui-kit.com/component/table) | Not needed for the current coding-workspace workflows or available data. |
| [tabs](https://gpui-kit.com/component/tabs) | Applied Kit TabBar with tracked reveal for keyboard/programmatic selection; retain scoped keyboard actions and VT Code-owned content. Two variants by semantics, both from one `desktop_tab_bar` core in `view/components.rs`. Folder via `desktop_folder_tab_bar` (medium default, `w_full`, no width cap, named TopRight-anchored overflow dropdown in the suffix instead of Kit's unlabeled menu button): thread strip (checked-item tab list through the guarded session loader, per-tab close suffix with stopPropagation, prefix focus rail) and inspector rail (tab list through the existing tab navigation, close in suffix). Segmented via `desktop_segmented_tab_bar` (small, intrinsic width) for mutually exclusive choices: attachment-picker source ("VT Code"/"This computer"), reduce-motion preference ("System"/"On"/"Off", was a row of hand-styled ghost buttons), and prompt-history scope (was `ToggleGroup::segmented`). Reversal 2026-11-20: the picker is a mode switch inside a popover, not a document tab — folder tabs drew a curve with no content to join, reading as a notch. Kit's `TabVariant::Segmented` supplies the one-pill track, sliding selection and concentric radius; no new color values (the `tab_bar_segmented`/`tab_foreground`/`tab_active_foreground` tokens in `themes/fills.rs` were already derived for it). Kit 0.10.0 tabs join a roving-focus list by default, so every desktop bar sets the vendored `keyboard_navigation(false)` (`desktop_tab_bar` core; inner tabs stay pointer-only) and `segmented_choice_wrapper` remains the one canonical wrapper: it tracks the host's focus handle, paints a visible ring, focuses on click, and maps Left/Right/Home/End through `segmented_choice_index` (clamped, stops propagation). Pinned-0.9.0 gaps (re-verified 2026-11-09, 2026-11-11 against pinned 0.9.1, and 2026-11-22 against pinned 0.10.0 — still no per-tab id setter or bar-level `context_menu`): no per-tab custom id (tests address tabs by strip scope + index), `.context_menu()` returns a wrapper the bar cannot take back (menu lives on a trigger child + the close button), bar ids scope paths but register nothing observable (wrappers own lookup ids). Folder tabs never shrink, so clipped tabs need explicit reveal — keyboard/programmatic selection scrolls, tests scroll-then-click (`click_thread_tab_close`). The 0.9.1 icon-only minimum width is N/A (all desktop tabs are labeled). 0.10.0 roving adopted as an opt-out: the single-stop strip, shared highlight memory, and direct-select segmented arrows are preserved; `tabs_strip_is_one_tab_stop_rows_are_skipped` pins it. |
| [tag](https://gpui-kit.com/component/tag) | Applied: neutral provider metadata and explicit tool-status tags in recessed activity cards. |
| [text-view](https://gpui-kit.com/component/text-view) | Applied Base TextView with reduced-motion-aware assistant stream fades; preserve syntax highlighting, selection/copy, web-only links, and blocked transcript images. Reference-style images (rendered since 0.9.0) pass through the preprocessor untouched and stay blocked at render time like every other image — verified in pinned 0.9.0 `node.rs` 2026-11-09, re-verified in 0.9.1, and pinned by `reference_style_images_pass_through_untouched`. HTML mode (`TextView::html`, re-verified in pinned 0.9.0 `gpui-base/src/text/format/html.rs` 2026-11-09 and in 0.9.1 (one additive `task_marker` line; hardening unchanged): `style`/`script` elements are dropped, no event-handler attributes, no JS engine) is approved for MCP Apps static previews only: same hardening as Markdown (blocked images, web-only links, themed code blocks), bodies arrive through the bounded `mcp/app/read` service call, rows stay measurable via a 6,000-char head cap with a truncation notice. No interactive app engine; app-originated `tools/call` enforcement and round-trips stay deferred. Model-side `visibility: ["app"]` enforcement is live host-wide (hidden from listings, rejected on direct call). 0.9.1 `on_task_toggle`/`link_tooltip` skipped 2026-11-11: caller-owns-source semantics fit neither the read-only transcript (no service mutation backs a toggle) nor any designed tooltip consumer; both default off without handlers. Adopted 2026-11-22: `TextView::link` drops non-web links to plain text at draw time (`transcript_link_target`, pinned by `transcript_link_filter_draws_only_web_links_as_links`); the click-time `external_web_url` opener stays. Base page reviewed 2026-11-24 (paragraph above); fenced code blocks are capped at 24rem with Kit's own scroll region. |
| [textarea](https://gpui-kit.com/component/textarea) | Retain rich Markdown, owned input state, raw copy, and image/security policy. |
| [theme](https://gpui-kit.com/component/theme) | Applied: neutral hover/selection distinct from brand primary; component surfaces follow VT Code palettes. Syntax *token* colors stay on the Kit mode default (`ThemeStyle` is unconstructible in pinned 0.9.0 — re-verified 2026-11-09 and 2026-11-11 against pinned 0.9.1 (`highlighter/registry.rs` byte-identical): private fields, no constructors, only serde plus an outbound `From`, and `HighlightTheme` is now also `#[non_exhaustive]`, so per-palette `highlight_theme` is not cleanly possible); only code-block surfaces follow the palette. Adopted 2026-11-09: the caret follows reduced motion via `Theme::cursor_blink` (component `Theme::update` projects it onto the base theme). Refined 2026-11-24 against the [GPUI Kit cursor-blink guide](https://gpui-kit.com/base/primitives/input/#cursor-blinking) and [style guide](https://gpui-kit.com/docs/style/#effects-and-cursor) (site v0.11.0; APIs re-verified in pinned `0.10.0`): the caret uses Kit's native blink (`CursorBlink::default()`) under normal motion and `steady()` under reduced motion, projected through component `Theme::update`; the application-owned composer fade was removed, so no custom opacity animation runs. `Theme::cursor_blink` is intact in 0.9.1; 0.9.1 `window.border`/`window_radius` are skipped as Linux-only per the source docs (the desktop is macOS-only). |
| [time-field](https://gpui-kit.com/component/time-field) | Not needed for the current coding-workspace workflows or available data. |
| [title-bar](https://gpui-kit.com/component/title-bar) | Retain one kit Root and tested native menu/window/reopen lifecycle. Main-relative collapse 2026-11-07: the sidebar yields to its rail below 900px on every route, and the workspace suffix plus speed tag collapse below 760px of main-column width (`MAIN_COLUMN_DETAILS_BREAKPOINT`, capped at `WORKSPACE_LABEL_MAX_WIDTH`). The bar fills main correctly (no flex bug); the defect was sidebar starvation hiding buttons underneath it at narrow widths. The 0.9.1 corner-radii following is Linux window-border pairing (zero radii on macOS); collapse math carries forward. |
| [toggle](https://gpui-kit.com/component/toggle) | Applied to model speed selection as a controlled, accessible toolbar toggle; preference controls continue to use Switch. The 0.9.1 `single` group mode is unneeded (the speed choice is already app-controlled); the hover-registration fix is automatic. The prompt-history scope `ToggleGroup::segmented()` was retired 2026-11-20 in favor of the shared segmented `TabBar` (see tabs row): its independent ghost toggles and accent fill could not produce the one-track, neutral-selection silhouette, while Kit's tabs already own selection and the wrapper owns keyboard focus. |
| [toolbar](https://gpui-kit.com/component/toolbar) | Applied to the header workspace actions with named grouping, compact Kit controls, and arrow-key focus navigation. Audited 2026-11-07: remaining multi-button rows are dialog footers/settings rows (correctly not toolbars) and the icon/tooltip audit is clean. Migrated 2026-11-07: the file-preview header is a `Toolbar` + `ToolbarGroup` (roving arrows, wrap at the ends; Tab stops unchanged, wrap preserved) — the locale blocker dissolved because group labels may ship as English literals per repo rule, and bar ids scope paths but register nothing observable (pin roving behavior, not structure). 0.9.1 `axis` is skipped (no vertical toolbar; the `Horizontal` default preserves roving behavior); bar-id scoping carries forward (the id lines sit outside the delta). |
| [tooltip](https://gpui-kit.com/component/tooltip) | Applied Kit-managed tooltips to action buttons; shortcut hints resolve from the active keymap, while icon controls retain explicit accessible names. Keep fixed Enter guidance only where the action is not configurable. |
| [tree](https://gpui-kit.com/component/tree) | Applied: workspace browsing uses the Kit Tree with stable relative-path IDs, keyboard expand/select, bounded discovery, and read-only previews. Both trees (inspector Files tab, attachment picker) share one gitignore-aware snapshot plus in-memory nucleo fuzzy search (`files/model.rs`); filters switch rows to flat ranked hits with a shown/total count, the picker's hits are file-only with attach-on-toggle rows, and Enter reuses the global tree-context `OpenSelectedFile` binding per surface. Base primitive doc reviewed 2026-11-24 against pinned 0.10.0: virtualization, entity-owned state, stable ids, and role/selection/expansion semantics are already present; accessible row names are now one shared builder (`files_panel::{file_name, file_tree_row_accessibility_label}`). `aria_level` is intentionally not added — the base row owns the `TreeItem` node and an inner level has no a11y node (gpui-pre only creates nodes for elements with a role); needs an upstream gpui-base change, not a vendored fork. Tree-scoped Home/End are desktop-owned (`SelectFirstFile`/`SelectLastFile` → `files_panel::select_tree_edge`) because the base primitive binds arrows only; both surface containers route the action to their own tree. |
| [virtual-list](https://gpui-kit.com/component/virtual-list) | Applied to changed-file, recent-thread, and activity rows with stable IDs and accessible list position/selection semantics; Activity discloses its 300-row window and routes older-history discovery to bounded conversation search. Transcript retains measured MessageScroller geometry and existing anchoring/following behavior. |
