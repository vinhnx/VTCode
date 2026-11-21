use super::*;

#[test]
fn settings_tab_icons_cover_every_tab_exactly_once() {
    assert_eq!(SettingsTab::VtCode.icon(), None);
    assert_eq!(SettingsTab::Appearance.icon(), Some(IconName::SunMoon));
    assert_eq!(SettingsTab::Accessibility.icon(), Some(IconName::PersonStanding));
    assert_eq!(SettingsTab::Shortcuts.icon(), Some(IconName::Keyboard));
    assert_eq!(SettingsTab::Connections.icon(), Some(IconName::Plug));
    assert_eq!(SettingsTab::Notifications.icon(), Some(IconName::Bell));
    assert_eq!(SettingsTab::Storage.icon(), Some(IconName::HardDrive));
    assert_eq!(SettingsTab::ComputerUse.icon(), Some(IconName::Computer));
    assert_eq!(SettingsTab::About.icon(), Some(IconName::Info));
    let icons = SettingsTab::ALL.map(SettingsTab::icon);
    for (index, icon) in icons.iter().enumerate() {
        assert!(icons.iter().skip(index + 1).all(|other| other != icon), "duplicate nav icon {icon:?}");
    }
}

#[test]
fn vtcode_leads_the_settings_rail_and_about_closes_it() {
    assert_eq!(SettingsPage::ALL.first(), Some(&SettingsPage::Tab(SettingsTab::VtCode)));
    assert_eq!(SettingsPage::ALL.last(), Some(&SettingsPage::Tab(SettingsTab::About)));
    // CLI configuration sits just before About, which is the final entry
    // in every settings navigation surface.
    assert_eq!(SettingsPage::ALL.get(SettingsTab::CONFIG_INDEX), Some(&SettingsPage::Configuration));
    assert_eq!(SettingsTab::CONFIG_INDEX + 1, SettingsTab::ABOUT_INDEX);
    assert_eq!(SettingsTab::PAGE_COUNT, SettingsPage::ALL.len());
}
