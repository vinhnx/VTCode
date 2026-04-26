use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::{HookCommandConfig, HookGroupConfig, LifecycleHooksConfig};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection};

const HOOKS_TITLE: &str = "Lifecycle Hooks";
const HOOKS_HINT: &str = "Enter open • Esc back • Double Esc close";
const HOOKS_SEARCH_LABEL: &str = "Search hooks";
const HOOKS_SEARCH_PLACEHOLDER: &str = "event, matcher, or command";
const ACTION_BACK: &str = "hooks:back";
const ACTION_EVENTS: &str = "hooks:events";
const ACTION_EVENT_PREFIX: &str = "hooks:event:";
const ACTION_GROUP_PREFIX: &str = "hooks:group:";
const ACTION_HANDLER_PREFIX: &str = "hooks:handler:";

#[derive(Clone)]
pub(crate) struct HooksPaletteState {
    pub(crate) workspace: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) source_label: String,
    pub(crate) lifecycle: LifecycleHooksConfig,
    pub(crate) view: HooksPaletteView,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HooksPaletteView {
    Events,
    Groups {
        event_key: String,
    },
    Handlers {
        event_key: String,
        group_index: usize,
    },
    Detail {
        event_key: String,
        group_index: usize,
        handler_index: usize,
    },
}

pub(crate) fn create_hooks_palette_state(
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
) -> Result<HooksPaletteState> {
    let manager = crate::main_helpers::load_workspace_config(workspace)?;
    let has_config_file = manager.config_path().is_some();
    let source_path = manager
        .config_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace.join("vtcode.toml"));
    let config = if has_config_file {
        manager.config().clone()
    } else {
        vt_snapshot
            .clone()
            .unwrap_or_else(|| manager.config().clone())
    };
    let source_label = if has_config_file {
        format!("Configuration source: {}", source_path.display())
    } else {
        format!(
            "No workspace vtcode.toml yet. Showing the resolved session hook config for {}.",
            workspace.display()
        )
    };

    Ok(HooksPaletteState {
        workspace: workspace.to_path_buf(),
        source_path,
        source_label,
        lifecycle: config.hooks.lifecycle.normalized(),
        view: HooksPaletteView::Events,
    })
}

pub(crate) fn show_hooks_palette(
    renderer: &mut AnsiRenderer,
    state: &HooksPaletteState,
    selected: Option<InlineListSelection>,
) -> Result<bool> {
    let mut lines = vec![state.source_label.clone()];
    lines.extend(view_lines(state));
    lines.push(HOOKS_HINT.to_string());

    let items = build_items(state)?;
    if items.is_empty() {
        return Ok(false);
    }

    renderer.show_list_modal(
        HOOKS_TITLE,
        lines,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: HOOKS_SEARCH_LABEL.to_string(),
            placeholder: Some(HOOKS_SEARCH_PLACEHOLDER.to_string()),
        }),
    );

    Ok(true)
}

pub(crate) fn apply_hooks_action(state: &mut HooksPaletteState, action: &str) -> Result<()> {
    match action {
        ACTION_EVENTS => {
            state.view = HooksPaletteView::Events;
            return Ok(());
        }
        ACTION_BACK => {
            state.view = parent_view(&state.view);
            return Ok(());
        }
        _ => {}
    }

    if let Some(event_key) = action.strip_prefix(ACTION_EVENT_PREFIX) {
        state.view = HooksPaletteView::Groups {
            event_key: event_key.to_string(),
        };
        return Ok(());
    }

    if let Some(rest) = action.strip_prefix(ACTION_GROUP_PREFIX) {
        let (event_key, group_index) = rest
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid hooks group action: {action}"))?;
        state.view = HooksPaletteView::Handlers {
            event_key: event_key.to_string(),
            group_index: group_index
                .parse()
                .with_context(|| format!("Invalid group index in hooks action: {action}"))?,
        };
        return Ok(());
    }

    if let Some(rest) = action.strip_prefix(ACTION_HANDLER_PREFIX) {
        let mut parts = rest.rsplitn(3, ':');
        let handler_index = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid hooks handler action: {action}"))?
            .parse()
            .with_context(|| format!("Invalid handler index in hooks action: {action}"))?;
        let group_index = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid hooks handler action: {action}"))?
            .parse()
            .with_context(|| format!("Invalid group index in hooks action: {action}"))?;
        let event_key = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid hooks handler action: {action}"))?;
        state.view = HooksPaletteView::Detail {
            event_key: event_key.to_string(),
            group_index,
            handler_index,
        };
        return Ok(());
    }

    bail!("Unknown hooks action: {}", action)
}

pub(crate) fn render_hooks_summary(
    renderer: &mut AnsiRenderer,
    lifecycle: &LifecycleHooksConfig,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Interactive /hooks browser requires inline UI. Effective hooks.lifecycle summary:",
    )?;
    for event in event_specs() {
        let groups = groups_for(lifecycle, event.key);
        let handler_count: usize = groups.iter().map(|group| group.hooks.len()).sum();
        renderer.line(
            MessageStyle::Info,
            &format!(
                "- {}: {} group(s), {} handler(s)",
                event.label,
                groups.len(),
                handler_count
            ),
        )?;
    }
    renderer.line(
        MessageStyle::Info,
        "Use /config hooks.lifecycle for raw configuration inspection.",
    )?;
    Ok(())
}

pub(crate) fn parent_view(view: &HooksPaletteView) -> HooksPaletteView {
    match view {
        HooksPaletteView::Events => HooksPaletteView::Events,
        HooksPaletteView::Groups { .. } => HooksPaletteView::Events,
        HooksPaletteView::Handlers { event_key, .. } => HooksPaletteView::Groups {
            event_key: event_key.clone(),
        },
        HooksPaletteView::Detail {
            event_key,
            group_index,
            ..
        } => HooksPaletteView::Handlers {
            event_key: event_key.clone(),
            group_index: *group_index,
        },
    }
}

fn build_items(state: &HooksPaletteState) -> Result<Vec<InlineListItem>> {
    match &state.view {
        HooksPaletteView::Events => Ok(event_specs()
            .iter()
            .map(|event| {
                let groups = groups_for(&state.lifecycle, event.key);
                let handler_count: usize = groups.iter().map(|group| group.hooks.len()).sum();
                InlineListItem {
                    title: event.label.to_string(),
                    subtitle: Some(format!(
                        "{} matcher group(s) • {} handler(s)",
                        groups.len(),
                        handler_count
                    )),
                    badge: (!groups.is_empty()).then(|| groups.len().to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::ConfigAction(format!(
                        "{ACTION_EVENT_PREFIX}{}",
                        event.key
                    ))),
                    search_value: Some(format!(
                        "{} {} {} {}",
                        event.label,
                        event.key,
                        groups.len(),
                        handler_count
                    )),
                }
            })
            .collect()),
        HooksPaletteView::Groups { event_key } => {
            let groups = groups_for(&state.lifecycle, event_key);
            Ok(groups
                .iter()
                .enumerate()
                .map(|(index, group)| {
                    let matcher = group.matcher.as_deref().unwrap_or("*");
                    InlineListItem {
                        title: format!("Matcher {}", index + 1),
                        subtitle: Some(format!(
                            "matcher: {} • {} handler(s)",
                            matcher,
                            group.hooks.len()
                        )),
                        badge: Some(group.hooks.len().to_string()),
                        indent: 0,
                        selection: Some(InlineListSelection::ConfigAction(format!(
                            "{ACTION_GROUP_PREFIX}{event_key}:{index}"
                        ))),
                        search_value: Some(format!("{matcher} {}", group.hooks.len())),
                    }
                })
                .collect())
        }
        HooksPaletteView::Handlers {
            event_key,
            group_index,
        } => {
            let Some(group) = groups_for(&state.lifecycle, event_key).get(*group_index) else {
                bail!("Invalid hooks group index");
            };
            Ok(group
                .hooks
                .iter()
                .enumerate()
                .map(|(index, hook)| InlineListItem {
                    title: hook.command.clone(),
                    subtitle: Some(render_handler_summary(hook)),
                    badge: hook.timeout_seconds.map(|timeout| format!("{timeout}s")),
                    indent: 0,
                    selection: Some(InlineListSelection::ConfigAction(format!(
                        "{ACTION_HANDLER_PREFIX}{event_key}:{group_index}:{index}"
                    ))),
                    search_value: Some(format!(
                        "{} {}",
                        hook.command,
                        render_handler_summary(hook)
                    )),
                })
                .collect())
        }
        HooksPaletteView::Detail {
            event_key,
            group_index,
            handler_index,
        } => {
            let Some(group) = groups_for(&state.lifecycle, event_key).get(*group_index) else {
                bail!("Invalid hooks group index");
            };
            let Some(handler) = group.hooks.get(*handler_index) else {
                bail!("Invalid hooks handler index");
            };

            Ok(vec![InlineListItem {
                title: "Back".to_string(),
                subtitle: Some(format!(
                    "event: {} • matcher: {} • timeout: {}",
                    event_label(event_key),
                    group.matcher.as_deref().unwrap_or("*"),
                    handler
                        .timeout_seconds
                        .map(|timeout| format!("{timeout}s"))
                        .unwrap_or_else(|| "default".to_string())
                )),
                badge: Some("Detail".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(ACTION_BACK.to_string())),
                search_value: Some(format!(
                    "{} {} {} {}",
                    event_key,
                    group.matcher.as_deref().unwrap_or("*"),
                    handler.command,
                    state.source_path.display()
                )),
            }])
        }
    }
}

fn view_lines(state: &HooksPaletteState) -> Vec<String> {
    match &state.view {
        HooksPaletteView::Events => vec!["Choose a lifecycle event.".to_string()],
        HooksPaletteView::Groups { event_key } => {
            vec![format!("{} ({})", event_label(event_key), event_key)]
        }
        HooksPaletteView::Handlers {
            event_key,
            group_index,
        } => {
            let matcher = groups_for(&state.lifecycle, event_key)
                .get(*group_index)
                .and_then(|group| group.matcher.as_deref())
                .unwrap_or("*");
            vec![
                format!("{} ({})", event_label(event_key), event_key),
                format!("Matcher: {}", matcher),
            ]
        }
        HooksPaletteView::Detail {
            event_key,
            group_index,
            handler_index,
        } => {
            let group = groups_for(&state.lifecycle, event_key).get(*group_index);
            let handler = group.and_then(|group| group.hooks.get(*handler_index));
            vec![
                format!("{} ({})", event_label(event_key), event_key),
                format!(
                    "Matcher: {}",
                    group
                        .and_then(|group| group.matcher.as_deref())
                        .unwrap_or("*")
                ),
                format!(
                    "Command: {}",
                    handler
                        .map(|handler| handler.command.as_str())
                        .unwrap_or("<missing>")
                ),
                format!(
                    "Timeout: {}",
                    handler
                        .and_then(|handler| handler.timeout_seconds)
                        .map(|timeout| format!("{timeout}s"))
                        .unwrap_or_else(|| "default".to_string())
                ),
                format!("Source path: {}", state.source_path.display()),
                format!("Workspace: {}", state.workspace.display()),
            ]
        }
    }
}

fn render_handler_summary(hook: &HookCommandConfig) -> String {
    match hook.timeout_seconds {
        Some(timeout) => format!("command hook • timeout {}s", timeout),
        None => "command hook • default timeout".to_string(),
    }
}

fn groups_for<'a>(lifecycle: &'a LifecycleHooksConfig, event_key: &str) -> &'a [HookGroupConfig] {
    match event_key {
        "session_start" => &lifecycle.session_start,
        "session_end" => &lifecycle.session_end,
        "subagent_start" => &lifecycle.subagent_start,
        "subagent_stop" => &lifecycle.subagent_stop,
        "user_prompt_submit" => &lifecycle.user_prompt_submit,
        "pre_tool_use" => &lifecycle.pre_tool_use,
        "post_tool_use" => &lifecycle.post_tool_use,
        "permission_request" => &lifecycle.permission_request,
        "pre_compact" => &lifecycle.pre_compact,
        "stop" => &lifecycle.stop,
        "notification" => &lifecycle.notification,
        _ => &[],
    }
}

fn event_label(event_key: &str) -> &'static str {
    event_specs()
        .iter()
        .find(|event| event.key == event_key)
        .map(|event| event.label)
        .unwrap_or("Unknown")
}

struct HookEventSpec {
    key: &'static str,
    label: &'static str,
}

impl HookEventSpec {
    const fn new(key: &'static str, label: &'static str) -> Self {
        Self { key, label }
    }
}

const EVENT_SPECS: [HookEventSpec; 11] = [
    HookEventSpec::new("session_start", "SessionStart"),
    HookEventSpec::new("session_end", "SessionEnd"),
    HookEventSpec::new("subagent_start", "SubagentStart"),
    HookEventSpec::new("subagent_stop", "SubagentStop"),
    HookEventSpec::new("user_prompt_submit", "UserPromptSubmit"),
    HookEventSpec::new("pre_tool_use", "PreToolUse"),
    HookEventSpec::new("post_tool_use", "PostToolUse"),
    HookEventSpec::new("permission_request", "PermissionRequest"),
    HookEventSpec::new("pre_compact", "PreCompact"),
    HookEventSpec::new("stop", "Stop"),
    HookEventSpec::new("notification", "Notification"),
];

fn event_specs() -> &'static [HookEventSpec] {
    &EVENT_SPECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::{HookCommandConfig, HookGroupConfig};

    fn sample_state() -> HooksPaletteState {
        HooksPaletteState {
            workspace: PathBuf::from("/tmp/workspace"),
            source_path: PathBuf::from("/tmp/workspace/vtcode.toml"),
            source_label: "source".to_string(),
            lifecycle: LifecycleHooksConfig {
                pre_tool_use: vec![HookGroupConfig {
                    matcher: Some("bash".to_string()),
                    hooks: vec![HookCommandConfig {
                        kind: Default::default(),
                        command: "echo test".to_string(),
                        timeout_seconds: Some(5),
                    }],
                }],
                ..Default::default()
            },
            view: HooksPaletteView::Events,
        }
    }

    #[test]
    fn hooks_palette_navigates_to_handler_detail() {
        let mut state = sample_state();
        apply_hooks_action(&mut state, "hooks:event:pre_tool_use").expect("event action");
        assert_eq!(
            state.view,
            HooksPaletteView::Groups {
                event_key: "pre_tool_use".to_string()
            }
        );

        apply_hooks_action(&mut state, "hooks:group:pre_tool_use:0").expect("group action");
        assert_eq!(
            state.view,
            HooksPaletteView::Handlers {
                event_key: "pre_tool_use".to_string(),
                group_index: 0
            }
        );

        apply_hooks_action(&mut state, "hooks:handler:pre_tool_use:0:0").expect("detail action");
        assert_eq!(
            state.view,
            HooksPaletteView::Detail {
                event_key: "pre_tool_use".to_string(),
                group_index: 0,
                handler_index: 0
            }
        );
    }

    #[test]
    fn hooks_summary_counts_handlers() {
        let state = sample_state();
        let items = build_items(&state).expect("build event items");
        let pre_tool = items
            .iter()
            .find(|item| item.title == "PreToolUse")
            .expect("pre_tool item");
        assert_eq!(pre_tool.badge.as_deref(), Some("1"));
    }
}
