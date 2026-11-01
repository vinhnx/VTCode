//! Attachment picker: a tabbed card-grid surface for attaching workspace
//! files or picking from this computer, anchored above the composer.
use super::*;

/// Which source tab the picker shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttachmentPickerTab {
    Workspace,
    Computer,
}

/// Open picker state: the active tab plus card thumbnails for image files,
/// loaded in bounded background batches while the picker is open.
pub(super) struct AttachmentPickerState {
    tab: AttachmentPickerTab,
    thumbnails: HashMap<String, Arc<gpui_kit::Image>>,
}

/// How many image cards get thumbnails per picker opening.
const PICKER_THUMBNAIL_LIMIT: usize = 24;

impl AttachmentPickerState {
    /// Thumbnail cached for one image card, if its bounded read finished.
    pub(super) fn thumbnail(&self, rel_path: &str) -> Option<&Arc<gpui_kit::Image>> {
        self.thumbnails.get(rel_path)
    }
}

impl DesktopView {
    pub(super) fn open_attachment_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() || self.session_request.is_some() || self.attachment_picker.is_some() {
            return;
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        self.disarm_double_escape();
        self.attachment_picker = Some(AttachmentPickerState {
            tab: AttachmentPickerTab::Workspace,
            thumbnails: HashMap::new(),
        });
        self.attachment_picker_focus.focus(window, cx);
        self.load_picker_thumbnails(cx);
        cx.notify();
    }

    pub(super) fn close_attachment_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachment_picker.take().is_none() {
            return;
        }
        self.disarm_double_escape();
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn set_attachment_picker_tab(&mut self, tab: AttachmentPickerTab, cx: &mut Context<Self>) {
        if let Some(state) = self.attachment_picker.as_mut() {
            if state.tab != tab {
                state.tab = tab;
                cx.notify();
            }
        }
    }

    /// Load thumbnails for the first image files of the workspace scan, so
    /// image cards show pictures without stalling the open.
    fn load_picker_thumbnails(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let images: Vec<String> = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir && files::is_image_path(&entry.rel_path))
            .take(PICKER_THUMBNAIL_LIMIT)
            .map(|entry| entry.rel_path.clone())
            .collect();
        if images.is_empty() {
            return;
        }
        self.picker_thumbnail_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let thumbnails = cx
                .background_spawn(async move {
                    images
                        .into_iter()
                        .filter_map(|path| files::read_image(&root, &path).map(|image| (path, image)))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if let Some(state) = view.attachment_picker.as_mut() {
                    for (path, image) in thumbnails {
                        let _ = state.thumbnails.insert(path, image);
                    }
                    cx.notify();
                }
            });
        }));
    }

    /// The workspace files the picker cards show, newest scan order.
    fn picker_files(&self) -> impl Iterator<Item = &FileEntry> {
        self.files.iter().filter(|entry| !entry.is_dir)
    }

    /// Tabbed card-grid picker anchored above the composer: the workspace tab
    /// shows file cards (image thumbnails when loaded), the computer tab hands
    /// off to the native file picker.
    pub(super) fn render_attachment_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let ui_font_size = cx.theme().font_size;
        let popup_margin = ui_font_size * 0.75;
        let Some(state) = self.attachment_picker.as_ref() else {
            return div().into_any_element();
        };
        let attached: HashSet<&str> = self.attachments.iter().map(|attachment| attachment.path.as_str()).collect();

        let tabs = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                desktop_button("picker-tab-workspace")
                    .icon(IconName::FolderOpen)
                    .label(app::NAME.to_string())
                    .toggled(state.tab == AttachmentPickerTab::Workspace)
                    .when(state.tab == AttachmentPickerTab::Workspace, |button| {
                        button.bg(colors.secondary).text_color(colors.foreground)
                    })
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_attachment_picker_tab(AttachmentPickerTab::Workspace, cx);
                    })),
            )
            .child(
                desktop_button("picker-tab-computer")
                    .icon(IconName::LaptopMinimal)
                    .label(rust_i18n::t!("app.composer.picker_this_computer").to_string())
                    .toggled(state.tab == AttachmentPickerTab::Computer)
                    .when(state.tab == AttachmentPickerTab::Computer, |button| {
                        button.bg(colors.secondary).text_color(colors.foreground)
                    })
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_attachment_picker_tab(AttachmentPickerTab::Computer, cx);
                    })),
            );

        let body = match state.tab {
            AttachmentPickerTab::Workspace => {
                let cards: Vec<AnyElement> = self
                    .picker_files()
                    .map(|entry| {
                        let path = entry.rel_path.clone();
                        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                        let is_attached = attached.contains(path.as_str());
                        let thumbnail = state.thumbnails.get(&path).cloned();
                        let attach_path = path.clone();
                        let remove_path = path.clone();
                        div()
                            .relative()
                            .child(
                                desktop_button(format!("picker-file-{path}"))
                                    .w(px(116.))
                                    .h(px(96.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .min_w_0()
                                    .ghost()
                                    .when(is_attached, |card| {
                                        card.bg(colors.secondary)
                                            .border_1()
                                            .border_color(colors.primary)
                                            .text_color(colors.foreground)
                                    })
                                    .disabled(is_attached)
                                    .when(!is_attached, |card| card.cursor_pointer())
                                    .child(match thumbnail {
                                        Some(image) => {
                                            img(image).size(px(52.)).rounded_sm().overflow_hidden().into_any_element()
                                        }
                                        None => Icon::new(if files::is_image_path(&path) {
                                            IconName::FileImage
                                        } else {
                                            IconName::FileCode
                                        })
                                        .text_color(colors.muted_foreground)
                                        .into_any_element(),
                                    })
                                    .child(
                                        div()
                                            .max_w_full()
                                            .truncate()
                                            .text_xs()
                                            .text_color(colors.foreground)
                                            .child(name),
                                    )
                                    .when(is_attached, |card| {
                                        card.child(Icon::new(IconName::Check).small().text_color(colors.primary))
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.attach_context_file(attach_path.clone(), cx);
                                    })),
                            )
                            .when(is_attached, |card| {
                                card.child(
                                    div().absolute().top_1().right_1().child(
                                        desktop_button(opaque_path_element_id("picker-remove", &remove_path))
                                            .icon(IconName::X)
                                            .accessibility_label(
                                                rust_i18n::t!(
                                                    "app.composer.remove_attachment",
                                                    path = remove_path.as_str()
                                                )
                                                .to_string(),
                                            )
                                            .ghost()
                                            .xsmall()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.remove_attachment(remove_path.clone(), cx);
                                            })),
                                    ),
                                )
                            })
                            .into_any_element()
                    })
                    .collect();
                if cards.is_empty() {
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .child(empty_state(
                            rust_i18n::t!("app.files.empty.title").to_string(),
                            rust_i18n::t!("app.files.empty.description").to_string(),
                            IconName::Folder,
                        ))
                        .into_any_element()
                } else {
                    div()
                        .id("attachment-picker-grid")
                        .lock_scroll_axis()
                        .test_support()
                        .overflow_y_scrollbar()
                        .max_h(px(340.))
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(cards)
                        .into_any_element()
                }
            }
            AttachmentPickerTab::Computer => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_4()
                .child(
                    desktop_button("picker-choose-files")
                        .label(rust_i18n::t!("app.composer.attach_files").to_string())
                        .primary()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_attachment_picker(window, cx);
                            this.attach_files(window, cx);
                        })),
                )
                .into_any_element(),
        };

        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .offset(point(px(0.), ui_font_size * -0.5))
                .snap_to_window_with_margin(popup_margin)
                .child(
                    div()
                        .id("attachment-picker")
                        .test_support()
                        .track_focus(&self.attachment_picker_focus)
                        .focus_trap("attachment-picker-trap", &self.attachment_picker_focus)
                        .role(gpui_kit::Role::Dialog)
                        .accessibility_id("attachment-picker")
                        .aria_label(rust_i18n::t!("app.composer.attach_files").to_string())
                        .w(px(560.))
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_md()
                        .bg(colors.popover)
                        .border_1()
                        .border_color(colors.border)
                        .shadow_md()
                        .p_3()
                        .child(tabs)
                        .child(body),
                ),
        )
        .priority(2)
        .into_any_element()
    }
}
