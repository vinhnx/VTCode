use anyhow::Result;

#[test]
fn theme_state_is_shared_across_core_and_tui_wrappers() -> Result<()> {
    let original = vtcode_core::ui::theme::active_theme_id();

    vtcode_core::ui::theme::set_active_theme("mono")?;
    assert_eq!(vtcode_tui::ui::theme::active_theme_id(), "mono");

    vtcode_tui::ui::theme::set_active_theme("ansi-classic")?;
    assert_eq!(vtcode_core::ui::theme::active_theme_id(), "ansi-classic");

    vtcode_core::ui::theme::set_active_theme(&original)?;
    Ok(())
}
