/// Cursor shape variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

impl Default for CursorShape {
    fn default() -> Self {
        Self::Block
    }
}

/// Mouse tracking mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseTracking {
    Button,
    Drag,
    Any,
}

/// Central mode-state manager for DEC private modes.
#[derive(Clone, Debug)]
pub(crate) struct TerminalModes {
    pub(crate) wraparound: bool,
    pub(crate) cursor_visible: bool,
    pub(crate) cursor_shape: CursorShape,
    pub(crate) application_cursor_keys: bool,
    pub(crate) bracketed_paste: bool,
    pub(crate) focus_reporting: bool,
    pub(crate) mouse_tracking: Option<MouseTracking>,
    pub(crate) sgr_mouse: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            wraparound: true,
            cursor_visible: true,
            cursor_shape: CursorShape::Block,
            application_cursor_keys: false,
            bracketed_paste: false,
            focus_reporting: false,
            mouse_tracking: None,
            sgr_mouse: false,
        }
    }
}

impl TerminalModes {
    /// Set a DEC private mode. Returns `true` if the mode was recognized.
    pub(crate) fn set_private_mode(&mut self, mode: usize, enabled: bool) -> bool {
        match mode {
            1 => {
                self.application_cursor_keys = enabled;
                true
            }
            7 => {
                self.wraparound = enabled;
                true
            }
            25 => {
                self.cursor_visible = enabled;
                true
            }
            1000 => {
                self.mouse_tracking = if enabled {
                    Some(MouseTracking::Button)
                } else {
                    None
                };
                true
            }
            1002 => {
                self.mouse_tracking = if enabled {
                    Some(MouseTracking::Drag)
                } else {
                    None
                };
                true
            }
            1003 => {
                self.mouse_tracking = if enabled {
                    Some(MouseTracking::Any)
                } else {
                    None
                };
                true
            }
            1004 => {
                self.focus_reporting = enabled;
                true
            }
            1006 => {
                self.sgr_mouse = enabled;
                true
            }
            2004 => {
                self.bracketed_paste = enabled;
                true
            }
            _ => false,
        }
    }

    /// Set cursor shape from a DECSCUSR parameter.
    pub(crate) fn set_cursor_shape(&mut self, param: usize) {
        self.cursor_shape = match param {
            3 | 4 => CursorShape::Underline,
            5 | 6 => CursorShape::Bar,
            _ => CursorShape::Block,
        };
    }
}
