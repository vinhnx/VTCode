//! Window-level swipe-navigation overlay: while a trackpad swipe is armed,
//! a transient pill names the pending direction and target thread. The overlay
//! takes no focus and handles no input — cancellation arrives as scroll
//! phases, guard loss, or Escape — and unmounts on commit or cancel.

use super::*;

impl DesktopView {
    /// Display title for a session id: the stored title when present and
    /// non-blank. Shared by the window title and the swipe overlay target.
    pub(super) fn session_title(&self, session_id: &str) -> Option<&str> {
        self.sessions
            .iter()
            .find(|session| session.session_id.as_str() == session_id)
            .and_then(|session| session.title.as_deref())
            .filter(|title| !title.trim().is_empty())
    }

    /// Target label for an armed swipe: the thread title when the target
    /// session is loaded, else the workspace name, else the bare direction
    /// label. The commit re-check drops arms whose target left history
    /// mid-arm; the label just never renders empty.
    pub(super) fn swipe_target_label(&self, direction: NavigationDirection) -> String {
        let index = match direction {
            NavigationDirection::Back => self.navigation_history.back_index(),
            NavigationDirection::Forward => self.navigation_history.forward_index(),
        };
        let location = index.and_then(|index| self.navigation_history.location(index));
        let thread = location
            .as_ref()
            .and_then(|location| location.session_id.as_deref())
            .and_then(|session_id| self.session_title(session_id))
            .map(str::to_owned)
            .or_else(|| {
                location
                    .as_ref()
                    .and_then(|location| location.workspace.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.trim().is_empty())
            });
        match thread {
            Some(thread) => match direction {
                NavigationDirection::Back => rust_i18n::t!("app.navigation.swipe_back_to", thread = thread).to_string(),
                NavigationDirection::Forward => {
                    rust_i18n::t!("app.navigation.swipe_forward_to", thread = thread).to_string()
                }
            },
            None => match direction {
                NavigationDirection::Back => rust_i18n::t!("app.navigation.back").to_string(),
                NavigationDirection::Forward => rust_i18n::t!("app.navigation.forward").to_string(),
            },
        }
    }

    /// Transient armed-swipe pill: top-center, pointer-transparent (no
    /// handlers, like the drop overlay), painted beneath the modals. The
    /// animation id is direction-keyed so a supersede replays the fade;
    /// same-direction updates never renotify, so the surface holds steady
    /// instead of flickering. `with_animation` renders the end state
    /// statically under reduced motion, so no manual gate is needed.
    pub(super) fn render_swipe_navigation_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(direction) = self.swipe_arming.pending_direction() else {
            return div().into_any_element();
        };
        let colors = cx.theme().colors;
        let label = self.swipe_target_label(direction);
        let (icon, animation_id): (IconName, &'static str) = match direction {
            NavigationDirection::Back => (IconName::ArrowLeft, "swipe-navigation-arm-back"),
            NavigationDirection::Forward => (IconName::ArrowRight, "swipe-navigation-arm-forward"),
        };
        div()
            .id("swipe-navigation-overlay")
            .test_support()
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(TOOLBAR_HEIGHT + 12.))
            .role(gpui_kit::Role::Status)
            .aria_label(label.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .shadow_md()
                    .px_4()
                    .py_2()
                    .child(Icon::new(icon).small().text_color(colors.primary))
                    .child(div().text_sm().text_color(colors.foreground).child(label))
                    .with_animation(animation_id, zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta)),
            )
            .into_any_element()
    }
}
