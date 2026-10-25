//! Native controls over the service's authoritative session options.
use super::*;

impl PaletteKind {
    pub(super) fn is_composer_popup(self) -> bool {
        matches!(self, Self::Models | Self::ModelOptions | Self::Permissions | Self::Modes | Self::AddContext)
    }
}

impl DesktopView {
    pub(super) fn render_workspace_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.workspace.as_ref().map(|path| path.display().to_string());
        let current_label = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Choose workspace".into());
        let recent = self.recent_workspaces();
        let disabled =
            !self.workspace_switch_idle() || self.retained_submission.is_some() || self.workspace_picker_open;
        let view = cx.entity().downgrade();
        let trigger = desktop_button("composer-workspace")
            .icon(IconName::Folder)
            .label(current_label)
            .ghost()
            .small()
            .max_w(px(240.))
            .overflow_hidden()
            .accessibility_label("Choose workspace")
            .tooltip("Choose a workspace or recent project")
            .desktop_disabled(disabled);

        Popover::new("composer-workspace-popover")
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .w(px(300.))
            .max_h(px(360.))
            .trigger(trigger)
            .content(move |_, _, cx| {
                let colors = cx.theme().colors;
                let mut content = div()
                    .id("workspace-picker-list")
                    .w_full()
                    .min_h_0()
                    .max_h(px(340.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(colors.muted_foreground)
                            .child("WORKSPACES"),
                    );
                let open_view = view.clone();
                content = content.child(
                    desktop_button("open-workspace-folder")
                        .icon(IconName::Folder)
                        .label("Choose folder…")
                        .ghost()
                        .w_full()
                        .desktop_disabled(disabled)
                        .on_click(cx.listener(move |popover, _, window, cx| {
                            popover.dismiss(window, cx);
                            let _updated = open_view.update(cx, |view, cx| view.choose_workspace(window, cx));
                        })),
                );
                if recent.is_empty() {
                    content = content.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child("No recent workspaces"),
                    );
                } else {
                    content = content.child(div().mx_2().my_1().h(px(1.)).bg(colors.border.opacity(0.7)));
                    for path in &recent {
                        let label = Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(path)
                            .to_string();
                        let selected = current.as_deref() == Some(path.as_str());
                        let entry_view = view.clone();
                        let entry_path = path.clone();
                        content = content.child(
                            desktop_button(opaque_path_element_id("recent-workspace-popover", path))
                                .accessibility_label(format!(
                                    "{label}{}",
                                    if selected { " (current workspace)" } else { "" }
                                ))
                                .child(navigation_label(label, IconName::Folder, selected, colors.primary))
                                .ghost()
                                .w_full()
                                .desktop_disabled(disabled)
                                .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                                .on_click(cx.listener(move |popover, _, window, cx| {
                                    popover.dismiss(window, cx);
                                    let path = PathBuf::from(entry_path.clone());
                                    let _updated = entry_view.update(cx, |view, cx| {
                                        view.select_workspace_from_ui(path, window, cx);
                                    });
                                })),
                        );
                    }
                }
                content.into_any_element()
            })
            .into_any_element()
    }

    pub(super) fn open_add_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() || self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        self.show_palette(PaletteKind::AddContext, Vec::new(), window, cx);
        if !self.resources_loaded && !self.resources_loading && self.session_change_idle() {
            self.resources_loading = self.request(protocol::method::COMPOSER_RESOURCES, json!({}));
            if !self.resources_loading {
                self.error = Some("The local CLI resource request queue is unavailable.".into());
                cx.notify();
            }
        }
    }

    pub(super) fn composer_resource_groups(&self) -> Vec<(String, Vec<PaletteCommand>)> {
        let mut groups = vec![("Add".into(), vec![command("Files…", PaletteAction::AttachFiles)])];
        let files = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| command(entry.rel_path.clone(), PaletteAction::AttachFile(entry.rel_path.clone())))
            .collect::<Vec<_>>();
        // Preserve section identities while asynchronous discovery fills empty groups.
        groups.push(("Workspace files".into(), files));
        for (kind, title) in [
            (protocol::ComposerResourceKind::Skill, "Skills"),
            (protocol::ComposerResourceKind::Plugin, "Plugins"),
        ] {
            let entries = self
                .composer_resources
                .iter()
                .filter(|resource| resource.kind == kind)
                .map(|resource| {
                    let text = match kind {
                        protocol::ComposerResourceKind::Skill => format!("${}", resource.name),
                        protocol::ComposerResourceKind::Plugin => format!("Use the {} plugin.", resource.name),
                    };
                    let mut entry = command(resource.name.clone(), PaletteAction::InsertDraft(text));
                    entry.description = Some(resource.description.clone());
                    entry
                })
                .collect::<Vec<_>>();
            groups.push((title.into(), entries));
        }
        groups.push((
            "Commands".into(),
            vec![
                command("Choose a model…", PaletteAction::ChooseModel),
                command("Switch workspace…", PaletteAction::ChooseWorkspace),
                command("Toggle inspector panel", PaletteAction::ToggleInspector),
                command("Refresh workspace files", PaletteAction::RefreshFiles),
            ],
        ));
        groups
    }

    pub(super) fn attach_context_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        if self.attachments.len() >= 5 {
            self.error = Some("At most 5 attachments per prompt.".into());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let session = self.selected_session_id.clone();
        self.attachments_picker_open = true;
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let attachment = cx
                .background_spawn(async move { files::read_attachment(&root, &root.join(path)) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                view.attachments_picker_open = false;
                if view.workspace.as_ref() != Some(&workspace) || view.selected_session_id != session {
                    return;
                }
                match attachment {
                    Ok(attachment) if !view.attachments.iter().any(|existing| existing.path == attachment.path) => {
                        view.attachments.push(attachment)
                    }
                    Ok(_) => {}
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn open_model_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        let groups = self.quick_model_groups();
        self.show_palette(PaletteKind::ModelOptions, groups, window, cx);
        let Some(config) = self.configuration.as_ref() else {
            return;
        };
        let Some(options) = config.session_options.as_ref() else {
            return;
        };
        let levels = options.supported_efforts.clone();
        if !(1..=8).contains(&levels.len()) {
            return;
        }
        let Some(selected) = levels.iter().position(|effort| effort == &options.reasoning_effort) else {
            // An inherited CLI value can be unsupported after a model switch. Offer
            // explicit valid choices without pretending that the knob is already set.
            let entries = levels
                .iter()
                .map(|effort| {
                    command(
                        format!("Use {effort} effort"),
                        PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                            reasoning_effort: Some(effort.clone()),
                            ..Default::default()
                        }),
                    )
                })
                .collect();
            self.palette_groups.insert(0, ("Supported reasoning effort".into(), entries));
            cx.notify();
            return;
        };
        if levels.len() < 2 {
            return;
        }
        let provider = config.provider.clone();
        let model = config.model.clone();
        let slider = cx.new(|_| {
            gpui_kit::component::slider::SliderState::new()
                .min(0.)
                .max((levels.len() - 1) as f32)
                .step(1.)
                .default_value(selected as f32)
        });
        let palette_id = self.palette.as_ref().map(|state| state.entity_id());
        cx.observe(&slider, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&slider, window, move |_view, _, event, window, cx| {
            if let gpui_kit::component::slider::SliderEvent::Release(value) = event {
                let Some(effort) = effort_at(&levels, value.start()).cloned() else {
                    return;
                };
                let provider = provider.clone();
                let model = model.clone();
                let view = cx.entity().downgrade();
                window.defer(cx, move |window, cx| {
                    let _updated = view.update(cx, |view, cx| {
                        if view.palette_kind != PaletteKind::ModelOptions
                            || view.palette.as_ref().map(|state| state.entity_id()) != palette_id
                            || !view.model_is_current(&provider, &model)
                        {
                            return;
                        }
                        view.close_palette(window, cx);
                        view.select_session_options(
                            protocol::SessionOptionsSelectParams {
                                reasoning_effort: Some(effort),
                                ..Default::default()
                            },
                            cx,
                        );
                    });
                });
            }
            cx.notify();
        })
        .detach();
        self.effort_slider = Some(slider);
        cx.notify();
    }

    fn quick_model_groups(&self) -> Vec<(String, Vec<PaletteCommand>)> {
        let active_model = self.active_model_fallback();
        let mut recommended = self
            .models
            .iter()
            .chain(active_model.iter())
            .filter_map(|model| model.recommended_rank.map(|rank| (rank, model)))
            .collect::<Vec<_>>();
        recommended.sort_by_key(|(rank, _)| *rank);

        let mut groups = Vec::new();
        if !recommended.is_empty() {
            groups.push((
                "Recommended".into(),
                recommended
                    .into_iter()
                    .map(|(_, model)| model_palette_command(model, self.model_is_current(&model.provider, &model.id)))
                    .collect(),
            ));
        }
        if let Some(current) = self
            .models
            .iter()
            .chain(active_model.iter())
            .find(|model| self.model_is_current(&model.provider, &model.id) && model.recommended_rank.is_none())
        {
            groups.push(("Current".into(), vec![model_palette_command(current, true)]));
        }
        groups.push(("Browse".into(), vec![command("Search all models…", PaletteAction::ChooseModel)]));
        groups
    }

    pub(super) fn can_select_session_options(&self) -> bool {
        self.session_change_allowed(None)
            && !self.attachments_picker_open
            && self
                .configuration
                .as_ref()
                .is_some_and(|config| config.session_options.is_some())
    }

    pub(super) fn select_session_options(
        &mut self,
        options: protocol::SessionOptionsSelectParams,
        cx: &mut Context<Self>,
    ) {
        if !self.can_select_session_options() || self.session_option_is_current(&options) {
            return;
        }
        if self.request_session_change(protocol::method::CONFIG_OPTIONS, json!(options), cx) {
            self.status = "Updating runtime options…".into();
        }
    }

    pub(super) fn session_option_is_current(&self, options: &protocol::SessionOptionsSelectParams) -> bool {
        self.configuration
            .as_ref()
            .and_then(|config| config.session_options.as_ref())
            .is_some_and(|current| {
                options.permission_mode.is_none_or(|mode| current.permission_mode == mode)
                    && options.mode.as_ref().is_none_or(|mode| current.mode == *mode)
                    && options
                        .reasoning_effort
                        .as_ref()
                        .is_none_or(|effort| current.reasoning_effort == *effort)
                    && options.fast_mode.is_none_or(|enabled| current.fast_mode_enabled == enabled)
            })
    }

    pub(super) fn open_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_select_session_options() {
            return;
        }
        let entries = [
            (
                protocol::PermissionMode::Ask,
                "Ask before protected actions",
                "Confirm protected commands using the CLI's approval flow.",
            ),
            (
                protocol::PermissionMode::AutoApproveCommands,
                "Auto-approve commands",
                "Skip command confirmations. Sandbox and tool authorization still apply.",
            ),
        ]
        .into_iter()
        .map(|(permission_mode, label, description)| {
            let mut entry = command(
                label,
                PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                    permission_mode: Some(permission_mode),
                    ..Default::default()
                }),
            );
            entry.description = Some(description.into());
            entry
        })
        .collect();
        self.show_palette(PaletteKind::Permissions, vec![("For the next session".into(), entries)], window, cx);
    }

    pub(super) fn open_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_select_session_options() {
            return;
        }
        let entries = [
            ("build", "Build", "Use the CLI's build agent."),
            ("plan", "Plan", "Use the CLI's planning workflow before implementation."),
        ]
        .into_iter()
        .map(|(mode, label, description)| {
            let mut entry = command(
                label,
                PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                    mode: Some(mode.into()),
                    ..Default::default()
                }),
            );
            entry.description = Some(description.into());
            entry
        })
        .collect();
        self.show_palette(PaletteKind::Modes, vec![("For the next session".into(), entries)], window, cx);
    }

    pub(super) fn render_mode_button(&self, _cx: &Context<Self>) -> AnyElement {
        let mode = self
            .active_primary_agent_display
            .as_ref()
            .filter(|active| self.selected_session_id.as_deref() == Some(active.session_id.as_str()))
            .map(|active| active.display_name.as_str())
            .or_else(|| {
                self.configuration
                    .as_ref()
                    .and_then(|config| config.session_options.as_ref())
                    .map(|options| match options.mode.as_str() {
                        "build" => "Build",
                        "plan" => "Plan",
                        mode => mode,
                    })
            })
            .unwrap_or("Unavailable");
        desktop_button("composer-mode")
            .child(div().truncate().child(format!("Mode: {mode}")))
            .max_w(px(180.))
            .overflow_hidden()
            .accessibility_label(format!("Agent mode: {mode}"))
            .tooltip_with_action("Choose agent mode", &OpenModePicker, Some("VTCodeDesktop"))
            .dropdown_caret(true)
            .ghost()
            .small()
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenModePicker, self.focus_handle.clone())
            .into_any_element()
    }
}

pub(super) fn model_options_header(
    config: Option<&ConfigurationSnapshot>,
    slider: Option<&Entity<gpui_kit::component::slider::SliderState>>,
    model_label: Option<&str>,
    speed_control: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let options = config.and_then(|config| config.session_options.as_ref());
    let effort = options.map(|options| {
        slider
            .and_then(|slider| effort_at(&options.supported_efforts, slider.read(cx).value().start()))
            .unwrap_or(&options.reasoning_effort)
            .as_str()
    });
    let model = model_label
        .or_else(|| config.map(|config| config.model.as_str()))
        .unwrap_or("Choose a model");
    let label = format!("{model}{}{}", effort.map(|_| " · ").unwrap_or(""), effort.unwrap_or(""));
    let mut panel = div().p_3().flex().flex_col().gap_3().min_w_0().child(
        div()
            .id("model-options-summary")
            .test_support()
            .role(gpui_kit::Role::Label)
            .aria_label(label.clone())
            .font_medium()
            .truncate()
            .child(label),
    );
    if let Some(slider) = slider {
        let value = slider.read(cx).value().start();
        let minimum = slider.read(cx).min_value();
        let maximum = slider.read(cx).max_value();
        let lower = slider.clone();
        let higher = slider.clone();
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    desktop_button("decrease-effort")
                        .icon(IconName::Minus)
                        .ghost()
                        .small()
                        .accessibility_label("Decrease reasoning effort")
                        .desktop_disabled(value <= minimum)
                        .on_click(move |_, window, cx| step_effort(&lower, -1., window, cx)),
                )
                .child(
                    gpui_kit::component::slider::Slider::new(slider)
                        .accessibility_label(
                            effort
                                .map(|effort| format!("Reasoning effort: {effort}"))
                                .unwrap_or_else(|| "Reasoning effort".to_string()),
                        )
                        .flex_1(),
                )
                .child(
                    desktop_button("increase-effort")
                        .icon(IconName::Plus)
                        .ghost()
                        .small()
                        .accessibility_label("Increase reasoning effort")
                        .desktop_disabled(value >= maximum)
                        .on_click(move |_, window, cx| step_effort(&higher, 1., window, cx)),
                ),
        );
    }
    if let Some(speed_control) = speed_control {
        panel = panel.child(
            div()
                .id("model-fast-mode-row")
                .test_support()
                .flex()
                .justify_end()
                .child(speed_control),
        );
    }
    panel.into_any_element()
}

fn step_effort(
    slider: &Entity<gpui_kit::component::slider::SliderState>,
    direction: f32,
    window: &mut Window,
    cx: &mut App,
) {
    slider.update(cx, |state, cx| {
        let value = (state.value().start() + direction).clamp(state.min_value(), state.max_value());
        state.set_value(value, window, cx);
        cx.emit(gpui_kit::component::slider::SliderEvent::Release(state.value()));
    });
}

// Match the small capability list directly: no float-to-index cast, saturation, or panic.
fn effort_at(levels: &[String], value: f32) -> Option<&String> {
    if !value.is_finite() || value < 0. {
        return None;
    }
    let selected = value.round();
    levels
        .iter()
        .enumerate()
        .find_map(|(index, level)| ((index as f32 - selected).abs() < f32::EPSILON).then_some(level))
}

#[cfg(test)]
mod tests {
    use super::effort_at;
    #[test]
    fn effort_knob_respects_discrete_capability_boundaries() {
        let levels = ["low".into(), "medium".into(), "high".into()];
        assert_eq!(effort_at(&levels, 0.49).map(String::as_str), Some("low"));
        assert_eq!(effort_at(&levels, 0.51).map(String::as_str), Some("medium"));
        assert_eq!(effort_at(&levels, 2.).map(String::as_str), Some("high"));
        for value in [-1., 3., f32::NAN, f32::INFINITY] {
            assert!(effort_at(&levels, value).is_none());
        }
    }
}
