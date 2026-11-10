use gpui_kit::{Context, Window};

/// Shared destructive-confirm helper for Warning-level `window.prompt` flows.
///
/// Captures the `PromptLevel::Warning` + `&[confirm, cancel]` + `Ok(0)`
/// shape shared by the destructive confirms (file delete, session delete,
/// worktree remove, custom-theme delete, unsaved-changes discard) so the
/// next confirm cannot re-implement it with a different level. The prompt
/// is window-bound, so the task is spawned in the window and the callback
/// receives the window. The caller owns title/detail localization and the
/// post-confirm mutation.
pub(super) fn spawn_destructive_confirm<V, F>(
    title: String,
    detail: String,
    confirm: String,
    cancel: String,
    window: &mut Window,
    cx: &mut Context<V>,
    on_confirm: F,
) where
    V: 'static,
    F: FnOnce(&mut V, &mut Window, &mut Context<V>) + Send + 'static,
{
    let answer = window.prompt(
        gpui_kit::PromptLevel::Warning,
        title.as_str(),
        Some(detail.as_str()),
        &[confirm.as_str(), cancel.as_str()],
        cx,
    );
    cx.spawn_in(window, async move |view, cx| {
        if matches!(answer.await, Ok(0)) {
            let _updated = view.update_in(cx, |view, window, cx| {
                on_confirm(view, window, cx);
            });
        }
    })
    .detach();
}
