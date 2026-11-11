//! Copy utilities for the CLI config editor dialog: quiet path/value buttons
//! for the focused setting. They reuse the row menu's methods (and
//! saved-state semantics) so Tab reaches them without a row selection model;
//! a dirty draft stays natively selectable in the input above.
use super::*;

/// Leading footer group for the config editor: copy buttons for the focused
/// setting's path and saved value. Labels match the row menu wording for the
/// same actions (English literals, like the menu).
pub(super) fn config_editor_copy_group(path: &str, cx: &mut Context<DesktopView>) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .child(
            desktop_button("copy-config-path")
                .label("Copy setting path".to_string())
                .ghost()
                .small()
                .on_click(cx.listener({
                    let path = path.to_string();
                    move |view, _, _, cx| view.copy_config_path(&path, cx)
                })),
        )
        .child(
            desktop_button("copy-config-value")
                .label("Copy value".to_string())
                .ghost()
                .small()
                .on_click(cx.listener({
                    let path = path.to_string();
                    move |view, _, _, cx| view.copy_config_value(&path, cx)
                })),
        )
        .into_any_element()
}
