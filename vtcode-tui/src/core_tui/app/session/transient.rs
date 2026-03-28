#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransientPlacement {
    FloatingModal,
    BottomDocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransientFocusPolicy {
    Modal,
    CapturedInput,
    SharedInput,
    Passive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransientSurface {
    FloatingOverlay,
    DiffPreview,
    SlashPalette,
    HistoryPicker,
    AgentPalette,
    FilePalette,
    TaskPanel,
    LocalAgents,
}

impl TransientSurface {
    pub(crate) fn placement(self) -> TransientPlacement {
        match self {
            Self::FloatingOverlay | Self::DiffPreview => TransientPlacement::FloatingModal,
            Self::SlashPalette
            | Self::HistoryPicker
            | Self::AgentPalette
            | Self::FilePalette
            | Self::TaskPanel
            | Self::LocalAgents => TransientPlacement::BottomDocked,
        }
    }

    pub(crate) fn focus_policy(self) -> TransientFocusPolicy {
        match self {
            Self::FloatingOverlay | Self::DiffPreview => TransientFocusPolicy::Modal,
            Self::HistoryPicker | Self::LocalAgents => TransientFocusPolicy::CapturedInput,
            Self::SlashPalette | Self::AgentPalette | Self::FilePalette => {
                TransientFocusPolicy::SharedInput
            }
            Self::TaskPanel => TransientFocusPolicy::Passive,
        }
    }

    pub(crate) fn is_navigation_surface(self) -> bool {
        !matches!(self, Self::TaskPanel)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransientStatus {
    Active,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransientEntry {
    surface: TransientSurface,
    status: TransientStatus,
}

impl TransientEntry {
    fn new(surface: TransientSurface, status: TransientStatus) -> Self {
        Self { surface, status }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TransientVisibilityChange {
    pub(crate) previous_visible: Option<TransientSurface>,
    pub(crate) current_visible: Option<TransientSurface>,
}

impl TransientVisibilityChange {
    pub(crate) fn changed(&self) -> bool {
        self.previous_visible != self.current_visible
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TransientHost {
    stack: Vec<TransientEntry>,
}

impl TransientHost {
    pub(crate) fn top(&self) -> Option<TransientSurface> {
        self.visible_entry().map(|entry| entry.surface)
    }

    #[cfg(test)]
    pub(crate) fn status(&self, surface: TransientSurface) -> Option<TransientStatus> {
        self.stack
            .iter()
            .find(|entry| entry.surface == surface)
            .map(|entry| entry.status)
    }

    pub(crate) fn is_visible(&self, surface: TransientSurface) -> bool {
        self.top() == Some(surface)
    }

    pub(crate) fn show(&mut self, surface: TransientSurface) -> TransientVisibilityChange {
        let previous_visible = self.top();
        if previous_visible == Some(surface) {
            return TransientVisibilityChange {
                previous_visible,
                current_visible: previous_visible,
            };
        }

        self.stack.retain(|entry| entry.surface != surface);

        if let Some(entry) = self.stack.last_mut() {
            entry.status = TransientStatus::Suspended;
        }

        self.stack
            .push(TransientEntry::new(surface, TransientStatus::Active));

        TransientVisibilityChange {
            previous_visible,
            current_visible: Some(surface),
        }
    }

    pub(crate) fn hide(&mut self, surface: TransientSurface) -> TransientVisibilityChange {
        let previous_visible = self.top();
        let Some(index) = self.stack.iter().position(|entry| entry.surface == surface) else {
            return TransientVisibilityChange {
                previous_visible,
                current_visible: previous_visible,
            };
        };

        let was_visible = previous_visible == Some(surface);
        self.stack.remove(index);

        if was_visible && let Some(entry) = self.stack.last_mut() {
            entry.status = TransientStatus::Active;
        }

        TransientVisibilityChange {
            previous_visible,
            current_visible: self.top(),
        }
    }

    pub(crate) fn visible_bottom_docked(&self) -> Option<TransientSurface> {
        let surface = self.top()?;
        (surface.placement() == TransientPlacement::BottomDocked).then_some(surface)
    }

    pub(crate) fn has_active_navigation_surface(&self) -> bool {
        self.top()
            .is_some_and(TransientSurface::is_navigation_surface)
    }

    fn visible_entry(&self) -> Option<&TransientEntry> {
        self.stack
            .iter()
            .rev()
            .find(|entry| entry.status == TransientStatus::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TransientFocusPolicy, TransientHost, TransientPlacement, TransientStatus, TransientSurface,
        TransientVisibilityChange,
    };

    fn assert_change(
        change: TransientVisibilityChange,
        previous_visible: Option<TransientSurface>,
        current_visible: Option<TransientSurface>,
    ) {
        assert_eq!(
            change,
            TransientVisibilityChange {
                previous_visible,
                current_visible,
            }
        );
    }

    #[test]
    fn pushing_second_surface_suspends_previous_top() {
        let mut host = TransientHost::default();
        let first = host.show(TransientSurface::TaskPanel);
        let second = host.show(TransientSurface::DiffPreview);

        assert!(first.changed());
        assert_change(
            second,
            Some(TransientSurface::TaskPanel),
            Some(TransientSurface::DiffPreview),
        );
        assert_eq!(host.top(), Some(TransientSurface::DiffPreview));
        assert_eq!(
            host.status(TransientSurface::TaskPanel),
            Some(TransientStatus::Suspended)
        );
        assert_eq!(
            host.status(TransientSurface::DiffPreview),
            Some(TransientStatus::Active)
        );
        assert_eq!(host.visible_bottom_docked(), None);
        assert_eq!(
            host.top().map(TransientSurface::focus_policy),
            Some(TransientFocusPolicy::Modal)
        );
    }

    #[test]
    fn hiding_top_surface_resumes_previous_entry() {
        let mut host = TransientHost::default();
        host.show(TransientSurface::TaskPanel);
        host.show(TransientSurface::DiffPreview);

        let change = host.hide(TransientSurface::DiffPreview);

        assert_change(
            change,
            Some(TransientSurface::DiffPreview),
            Some(TransientSurface::TaskPanel),
        );
        assert_eq!(host.top(), Some(TransientSurface::TaskPanel));
        assert_eq!(
            host.status(TransientSurface::TaskPanel),
            Some(TransientStatus::Active)
        );
        assert_eq!(
            host.visible_bottom_docked(),
            Some(TransientSurface::TaskPanel)
        );
        assert_eq!(
            host.top().map(TransientSurface::focus_policy),
            Some(TransientFocusPolicy::Passive)
        );
    }

    #[test]
    fn re_showing_lower_surface_moves_it_to_top() {
        let mut host = TransientHost::default();
        host.show(TransientSurface::TaskPanel);
        host.show(TransientSurface::SlashPalette);

        let change = host.show(TransientSurface::TaskPanel);

        assert_change(
            change,
            Some(TransientSurface::SlashPalette),
            Some(TransientSurface::TaskPanel),
        );
        assert_eq!(host.top(), Some(TransientSurface::TaskPanel));
        assert_eq!(
            host.status(TransientSurface::SlashPalette),
            Some(TransientStatus::Suspended)
        );
        assert_eq!(
            host.status(TransientSurface::TaskPanel),
            Some(TransientStatus::Active)
        );
        assert_eq!(
            host.visible_bottom_docked(),
            Some(TransientSurface::TaskPanel)
        );
        assert_eq!(
            TransientSurface::TaskPanel.placement(),
            TransientPlacement::BottomDocked
        );
    }
}
