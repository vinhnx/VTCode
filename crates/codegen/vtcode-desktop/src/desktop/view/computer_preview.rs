//! Ephemeral computer-use screenshot previews: fetch-on-expand and cards.
//!
//! Transcript rows carry a `computer-preview://` reference (row id plus byte
//! metadata, no pixels); bodies resolve on demand via `computer/preview/read`
//! while the service still retains the bytes. Retention is memory-only and
//! bounded, so reads answer either the PNG body or an explicit expiry — the
//! card renders an expired note with no retry, never a spinner that cannot
//! finish.
//!
//! New user-visible strings here are English literals: full native-quality
//! translations cannot ship for them in this change (same rule as the shared
//! transcript headlines). Only the generic `app.preview.retry` label is
//! reused.

use super::*;
use vtcode_commons::formatting::format_size;

/// Identity of one in-flight screenshot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingComputerPreviewRead {
    pub(super) request_id: String,
    pub(super) item_id: String,
    pub(super) uri: String,
}

/// Cache key for a fetched screenshot: the owning row plus the preview URI.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ComputerPreviewBodyKey {
    pub(super) item_id: String,
    pub(super) uri: String,
}

/// A resolved screenshot: decoded pixels plus the served byte count and
/// whether the service downscaled it to fit the frame budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComputerPreviewBody {
    pub(super) image: Arc<gpui_kit::Image>,
    pub(super) bytes: u64,
    pub(super) downscaled: bool,
}

/// Whether a preview reference names a computer screenshot rather than an
/// MCP app body. The two card pipelines partition `app_previews` on this so
/// neither ever fetches nor renders the other's references.
pub(super) fn is_computer_preview_uri(uri: &str) -> bool {
    uri.starts_with(protocol::preview::COMPUTER_PREVIEW_URI_SCHEME)
}

pub(super) fn valid_computer_preview_read(
    result: &ComputerPreviewReadResult,
    pending: &PendingComputerPreviewRead,
) -> bool {
    result.request_id == pending.request_id && result.uri == pending.uri
}

impl DesktopView {
    /// Fetch the missing screenshot for an expanded tool row.
    pub(super) fn request_computer_previews(&mut self, item_id: &str, cx: &mut Context<Self>) {
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
                    .filter(|preview| is_computer_preview_uri(&preview.uri))
                    .take(1)
                    .map(|preview| preview.uri.clone())
                    .collect()
            })
            .unwrap_or_default();
        for uri in uris {
            let key = ComputerPreviewBodyKey { item_id: item_id.to_string(), uri: uri.clone() };
            if self.computer_preview_bodies.contains_key(&key)
                || self.computer_preview_errors.contains_key(&key)
                || self.computer_preview_expired.contains(&key)
                || self
                    .computer_preview_pending
                    .values()
                    .any(|pending| pending.item_id == item_id && pending.uri == uri)
            {
                continue;
            }
            let pending = PendingComputerPreviewRead {
                request_id: uuid_like_id(),
                item_id: item_id.to_string(),
                uri: uri.clone(),
            };
            if self.request(
                protocol::method::COMPUTER_PREVIEW_READ,
                json!({
                    keys::URI: pending.uri,
                    keys::REQUEST_ID: pending.request_id,
                }),
            ) {
                let _previous = self.computer_preview_pending.insert(pending.request_id.clone(), pending);
            } else {
                let _previous = self
                    .computer_preview_errors
                    .insert(key, "Could not load the screenshot: the service request queue is full.".into());
            }
        }
        cx.notify();
    }

    /// Retry a failed screenshot read after the user asks. Expired previews
    /// have no retry control, so this only ever refires transport failures.
    pub(super) fn retry_computer_preview(&mut self, item_id: &str, uri: &str, cx: &mut Context<Self>) {
        let key = ComputerPreviewBodyKey { item_id: item_id.to_string(), uri: uri.to_string() };
        let _removed = self.computer_preview_errors.remove(&key);
        self.request_computer_previews(item_id, cx);
    }

    pub(super) fn fail_computer_preview(&mut self, request_id: &str, cx: &mut Context<Self>) {
        let Some(pending) = self.computer_preview_pending.remove(request_id) else {
            return;
        };
        self.computer_preview_task = None;
        let key = ComputerPreviewBodyKey { item_id: pending.item_id, uri: pending.uri };
        let _previous = self
            .computer_preview_errors
            .insert(key, "Could not load the screenshot. Try again.".into());
        cx.notify();
    }

    /// Screenshot card for an expanded tool row carrying a computer preview.
    ///
    /// Renders nothing without a computer reference. The card always shows
    /// the linkage facts (caption plus URI) and one body state: the image,
    /// a loading row, an error with retry, an expired note, or a not-loaded
    /// note when no fetch is in flight.
    pub(super) fn render_computer_previews(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let Some(preview) = item.app_previews.iter().find(|preview| is_computer_preview_uri(&preview.uri)) else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let caption = themes::aa_muted(colors.background, colors.muted_foreground);
        let key = ComputerPreviewBodyKey { item_id: item.id.clone(), uri: preview.uri.clone() };
        let body = self.computer_preview_bodies.get(&key);
        // Served bytes once loaded (the service may have downscaled to fit
        // the frame); the retained size from the reference until then.
        let (bytes, scaled) = match body {
            Some(body) => (body.bytes, body.downscaled),
            None => (preview.bytes, false),
        };
        let mut meta = match preview.mime_type.as_deref() {
            Some(mime) => format!("{mime} · {}", format_size(bytes)),
            None => format_size(bytes),
        };
        if scaled {
            meta.push_str(" · scaled");
        }
        let mut card = div()
            .id(format!("computer-preview-{}", item.id))
            .test_support()
            .flex()
            .flex_col()
            .gap_1()
            .ml_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(themes::label_foreground(&colors))
                            .child("Screenshot".to_string()),
                    )
                    .child(div().text_xs().text_color(caption).child(meta.clone())),
            )
            .child(div().text_xs().text_color(caption).truncate().child(preview.uri.clone()));
        if let Some(body) = body {
            card = card.child(
                div()
                    .id(format!("computer-preview-body-{}", item.id))
                    .role(gpui_kit::Role::Label)
                    .aria_label(format!("Screenshot preview ({meta})"))
                    .test_support()
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(themes::soft_edge_color(&colors))
                    .child(img(body.image.clone()).w_full()),
            );
        } else if self.computer_preview_expired.contains(&key) {
            card = card.child(
                div()
                    .text_sm()
                    .text_color(themes::muted_text(&colors))
                    .child("Screenshot expired — previews are kept in memory only while recent.".to_string()),
            );
        } else if let Some(error) = self.computer_preview_errors.get(&key) {
            let retry_id = item.id.clone();
            let retry_uri = preview.uri.clone();
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(accessible_error(format!("computer-preview-error-{}", item.id), error.clone(), &colors))
                    .child(
                        desktop_ghost_button(format!("retry-computer-preview-{}", item.id))
                            .label(rust_i18n::t!("app.preview.retry").to_string())
                            .accessibility_label(format!("Retry loading the screenshot for {}", preview.uri))
                            .xsmall()
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.retry_computer_preview(&retry_id, &retry_uri, cx);
                            })),
                    ),
            );
        } else if self
            .computer_preview_pending
            .values()
            .any(|pending| pending.item_id == item.id && pending.uri == preview.uri)
        {
            card = card.child(loading_marker_row_with_spinner(
                format!("computer-preview-loading-{}", item.id),
                "Loading screenshot…".to_string(),
                Spinner::new().small().color(caption),
                themes::muted_text(&colors),
                cx,
            ));
        } else {
            card = card.child(
                div()
                    .text_sm()
                    .text_color(themes::muted_text(&colors))
                    .child("Screenshot not loaded.".to_string()),
            );
        }
        card.into_any_element()
    }

    pub(super) fn begin_computer_preview(&mut self, result: Value, cx: &mut Context<Self>) {
        let request_id = result
            .get(keys::REQUEST_ID)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(pending) = self.computer_preview_pending.get(&request_id).cloned() else {
            return;
        };
        self.computer_preview_task = Some(cx.spawn(async move |view, cx| {
            // Base64 and PNG handling stay off the UI thread; `Image` is
            // plain decoded bytes plus an id, so the result crosses back.
            let decoded = cx
                .background_spawn(async move {
                    let page = serde_json::from_value::<ComputerPreviewReadResult>(result).ok()?;
                    if page.expired {
                        return Some((page, None));
                    }
                    if page.mime_type.as_deref() != Some("image/png") {
                        return None;
                    }
                    let bytes =
                        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, page.data.as_deref()?)
                            .ok()?;
                    let image = Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes));
                    Some((page, Some(image)))
                })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.computer_preview_pending.get(&request_id) != Some(&pending) {
                    return;
                }
                detach_task_slot(&mut view.computer_preview_task);
                let Some((page, image)) = decoded else {
                    view.fail_computer_preview(&request_id, cx);
                    return;
                };
                if !valid_computer_preview_read(&page, &pending) {
                    view.fail_computer_preview(&request_id, cx);
                    return;
                }
                let _removed = view.computer_preview_pending.remove(&request_id);
                let key = ComputerPreviewBodyKey { item_id: pending.item_id, uri: pending.uri };
                match image {
                    Some(image) => {
                        let body = ComputerPreviewBody {
                            image,
                            bytes: page.total_bytes,
                            downscaled: page.downscaled,
                        };
                        let _previous = view.computer_preview_bodies.insert(key, body);
                    }
                    None => {
                        let _inserted = view.computer_preview_expired.insert(key);
                    }
                }
                cx.notify();
            });
        }));
    }
}
