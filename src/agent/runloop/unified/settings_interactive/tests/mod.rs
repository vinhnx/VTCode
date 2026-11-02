pub(super) use super::*;
pub(super) use crate::agent::runloop::unified::config_section_headings::normalize_config_path;
pub(super) use serial_test::serial;
pub(super) use std::sync::Arc;
pub(super) use vtcode_commons::canonicalize;
pub(super) use vtcode_commons::reference::StaticWorkspacePaths;
pub(super) use vtcode_config::defaults::WorkspacePathsDefaults;
pub(super) use vtcode_config::defaults::provider::with_config_defaults_provider_for_test;
pub(super) use vtcode_core::config::ConfigManager;

/// Drive one `CycleNext` on `provider.openai.service_tier` from `preset` and
/// return the resulting tier. One helper covers the whole ring
/// (missing→flex→priority→ultrafast→flex) so each test only names its
/// fixture and oracle.
fn cycle_service_tier_from(
    preset: Option<vtcode_config::OpenAIServiceTier>,
) -> Option<vtcode_config::OpenAIServiceTier> {
    let mut state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("provider.openai".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    state.draft.provider.openai.service_tier = preset;

    mutate_draft(&mut state, |draft| {
        apply_scalar_operation(draft, "provider.openai.service_tier", ScalarOperation::CycleNext)
    })
    .expect("service tier should advance");

    state.draft.provider.openai.service_tier
}

fn header_test_state(view_path: Option<&str>) -> SettingsPaletteState {
    SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: Some("test".to_string()),
        draft: VTCodeConfig::default(),
        view_path: view_path.map(ToString::to_string),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    }
}

mod advanced;
mod docs_tier;
mod edit_workspace;
mod header_catalog;
mod model_editor;
mod palette_basics;
mod root_curated;
mod toggles_persist;
