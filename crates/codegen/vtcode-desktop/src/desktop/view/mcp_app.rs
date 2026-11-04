//! Static MCP Apps previews: tier gating, body fetching, and preview cards.
//!
//! Bodies arrive on demand via `mcp/app/read` when a tool row with app
//! previews expands under the `static` tier. Rendering stays disclosure-first:
//! cards always show the linkage facts, and the inert HTML preview only when a
//! body has loaded.

use super::super::markdown::render_html;
use super::*;
use vtcode_commons::formatting::format_size;

/// Maximum preview bodies fetched per tool row; matches the enrichment bound.
const MAX_ROW_PREVIEWS: usize = 4;

/// Maximum HTML source characters rendered per preview; keeps rows measurable.
const MAX_PREVIEW_CHARS: usize = 6_000;

/// Setting path selecting the MCP Apps presentation tier.
const MCP_APPS_TIER_PATH: &str = "mcp.ui.apps";

/// Tier value enabling static HTML previews.
const MCP_APPS_TIER_STATIC: &str = "static";

/// Identity of one in-flight app-body read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingMcpAppRead {
    pub(super) request_id: String,
    pub(super) item_id: String,
    pub(super) uri: String,
}

/// Cache key for a fetched app body: the owning row plus the resource URI.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct McpAppBodyKey {
    pub(super) item_id: String,
    pub(super) uri: String,
}

pub(super) fn valid_mcp_app_read(result: &McpAppReadResult, pending: &PendingMcpAppRead) -> bool {
    result.request_id == pending.request_id && result.uri == pending.uri
}

/// Head-truncate `text` to `max_chars`, reporting whether anything was cut.
///
/// Returns a borrowed slice so previews never allocate a second copy of the
/// fetched body; the cut lands on a UTF-8 boundary.
pub(super) fn head_with_truncation(text: &str, max_chars: usize) -> (&str, bool) {
    if text.chars().count() <= max_chars {
        return (text, false);
    }
    let end = text
        .char_indices()
        .take(max_chars)
        .last()
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    (text.get(..end).unwrap_or_default(), true)
}

impl DesktopView {
    /// Whether static app previews render: the `static` tier only.
    ///
    /// `off` (and anything unrecognized) hides app UI; `interactive` has no
    /// engine yet and the service rejects it at MCP startup, so previews never
    /// arrive under it either.
    pub(super) fn mcp_apps_preview_enabled(&self) -> bool {
        self.cli_config_settings.as_ref().is_some_and(|snapshot| {
            snapshot.settings.iter().any(|setting| {
                setting.path == MCP_APPS_TIER_PATH && setting.value.as_str() == Some(MCP_APPS_TIER_STATIC)
            })
        })
    }

    /// Fetch missing preview bodies for an expanded tool row.
    pub(super) fn request_mcp_app_previews(&mut self, item_id: &str, cx: &mut Context<Self>) {
        if !self.mcp_apps_preview_enabled() {
            return;
        }
        let uris: Vec<String> = self
            .transcript
            .position_by_id(item_id)
            .and_then(|index| self.transcript.get(index))
            .map(|item| &item.app_previews)
            .or_else(|| {
                self.restored_activity
                    .iter()
                    .find(|item| item.id == item_id)
                    .map(|item| &item.app_previews)
            })
            .map(|previews| {
                previews
                    .iter()
                    .take(MAX_ROW_PREVIEWS)
                    .map(|preview| preview.uri.clone())
                    .collect()
            })
            .unwrap_or_default();
        for uri in uris {
            let key = McpAppBodyKey { item_id: item_id.to_string(), uri: uri.clone() };
            if self.mcp_app_bodies.contains_key(&key)
                || self.mcp_app_errors.contains_key(&key)
                || self
                    .mcp_app_pending
                    .values()
                    .any(|pending| pending.item_id == item_id && pending.uri == uri)
            {
                continue;
            }
            let pending = PendingMcpAppRead {
                request_id: uuid_like_id(),
                item_id: item_id.to_string(),
                uri: uri.clone(),
            };
            if self.request(
                protocol::method::MCP_APP_READ,
                json!({
                    keys::URI: pending.uri,
                    keys::REQUEST_ID: pending.request_id,
                }),
            ) {
                let _previous = self.mcp_app_pending.insert(pending.request_id.clone(), pending);
            } else {
                let _previous = self
                    .mcp_app_errors
                    .insert(key, rust_i18n::t!("app.preview.queue_full").to_string().into());
            }
        }
        cx.notify();
    }

    /// Retry a failed preview body after the user asks.
    pub(super) fn retry_mcp_app_preview(&mut self, item_id: &str, uri: &str, cx: &mut Context<Self>) {
        let key = McpAppBodyKey { item_id: item_id.to_string(), uri: uri.to_string() };
        let _removed = self.mcp_app_errors.remove(&key);
        self.request_mcp_app_previews(item_id, cx);
    }

    pub(super) fn fail_mcp_app_preview(&mut self, request_id: &str, cx: &mut Context<Self>) {
        let Some(pending) = self.mcp_app_pending.remove(request_id) else {
            return;
        };
        self.mcp_app_task = None;
        let key = McpAppBodyKey { item_id: pending.item_id, uri: pending.uri };
        let _previous = self
            .mcp_app_errors
            .insert(key, rust_i18n::t!("app.preview.load_failed").to_string().into());
        cx.notify();
    }

    /// Preview cards for an expanded tool row's app references.
    ///
    /// Renders nothing unless the `static` tier is on and the row carries
    /// previews. Each card shows the stable linkage facts plus one body state:
    /// the inert preview, a loading row, an error with retry, or an empty note.
    pub(super) fn render_mcp_app_previews(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        if !self.mcp_apps_preview_enabled() || item.app_previews.is_empty() {
            return div().into_any_element();
        }
        let colors = cx.theme().colors;
        let caption = themes::aa_muted(colors.background, colors.muted_foreground);
        let mut cards = div().flex().flex_col().gap_2().ml_4();
        for (index, preview) in item.app_previews.iter().take(MAX_ROW_PREVIEWS).enumerate() {
            let key = McpAppBodyKey { item_id: item.id.clone(), uri: preview.uri.clone() };
            let mut meta = match preview.mime_type.as_deref() {
                Some(mime) => format!("{mime} · {}", format_size(preview.bytes)),
                None => format_size(preview.bytes),
            };
            if self.mcp_app_bodies.get(&key).is_some_and(|body| body.truncated) {
                meta.push_str(" · truncated");
            }
            let mut card = div()
                .id(format!("mcp-app-preview-{}-{index}", item.id))
                .test_support()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(themes::label_foreground(&colors))
                                .child(rust_i18n::t!("app.preview.title").to_string()),
                        )
                        .child(div().text_xs().text_color(caption).child(meta)),
                )
                .child(div().text_xs().text_color(caption).truncate().child(preview.uri.clone()));
            if let Some(body) = self.mcp_app_bodies.get(&key) {
                card = card.child(self.render_mcp_app_body(item, index, preview, body, cx));
            } else if let Some(error) = self.mcp_app_errors.get(&key) {
                let retry_id = item.id.clone();
                let retry_uri = preview.uri.clone();
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_sm().text_color(colors.danger).child(error.clone()))
                        .child(
                            desktop_button(format!("retry-mcp-app-{}-{index}", item.id))
                                .label(rust_i18n::t!("app.preview.retry").to_string())
                                .accessibility_label(
                                    rust_i18n::t!("app.preview.retry_for", uri = preview.uri.as_str()).to_string(),
                                )
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.retry_mcp_app_preview(&retry_id, &retry_uri, cx);
                                })),
                        ),
                );
            } else if self
                .mcp_app_pending
                .values()
                .any(|pending| pending.item_id == item.id && pending.uri == preview.uri)
            {
                card = card.child(loading_marker_row_with_spinner(
                    format!("mcp-app-loading-label-{}-{index}", item.id),
                    rust_i18n::t!("app.preview.loading").to_string(),
                    Spinner::new().small().color(caption),
                    themes::muted_text(&colors),
                ));
            } else {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(themes::muted_text(&colors))
                        .child(rust_i18n::t!("app.preview.not_loaded").to_string()),
                );
            }
            cards = cards.child(card);
        }
        cards = cards.child(
            div()
                .text_xs()
                .text_color(caption)
                .child(rust_i18n::t!("app.preview.static_notice").to_string()),
        );
        cards.into_any_element()
    }

    /// The loaded body region: inert HTML, or an empty-body note.
    fn render_mcp_app_body(
        &self,
        item: &TranscriptItem,
        index: usize,
        preview: &vtcode_exec_events::AppPreviewItem,
        body: &McpAppReadResult,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if body.body.is_empty() {
            return div()
                .text_sm()
                .text_color(themes::muted_text(&cx.theme().colors))
                .child(rust_i18n::t!("app.preview.no_text").to_string())
                .into_any_element();
        }
        let (source, cut) = head_with_truncation(&body.body, MAX_PREVIEW_CHARS);
        let mut panel = div()
            .id(format!("mcp-app-body-{}-{index}", item.id))
            .role(gpui_kit::Role::Label)
            .aria_label(rust_i18n::t!("app.preview.static_aria", uri = preview.uri.as_str()).to_string())
            .test_support()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().colors.secondary)
            .child(render_html(format!("mcp-app-html-{}-{index}", item.id), source.to_string().into(), cx));
        if cut || body.truncated {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(themes::aa_muted(cx.theme().colors.secondary, cx.theme().colors.muted_foreground))
                    .child(rust_i18n::t!("app.preview.truncated").to_string()),
            );
        }
        panel.into_any_element()
    }

    pub(super) fn begin_mcp_app_preview(&mut self, result: Value, cx: &mut Context<Self>) {
        let request_id = result
            .get(keys::REQUEST_ID)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(pending) = self.mcp_app_pending.get(&request_id).cloned() else {
            return;
        };
        self.mcp_app_task = Some(cx.spawn(async move |view, cx| {
            let parsed = cx
                .background_spawn(async move { serde_json::from_value::<McpAppReadResult>(result) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.mcp_app_pending.get(&request_id) != Some(&pending) {
                    return;
                }
                if let Some(task) = view.mcp_app_task.take() {
                    task.detach();
                }
                let Ok(page) = parsed else {
                    view.fail_mcp_app_preview(&request_id, cx);
                    return;
                };
                if !valid_mcp_app_read(&page, &pending) {
                    view.fail_mcp_app_preview(&request_id, cx);
                    return;
                }
                let _removed = view.mcp_app_pending.remove(&request_id);
                let key = McpAppBodyKey { item_id: pending.item_id, uri: pending.uri };
                let _previous = view.mcp_app_bodies.insert(key, page);
                cx.notify();
            });
        }));
    }
}
