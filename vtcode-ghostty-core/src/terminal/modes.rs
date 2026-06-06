use super::Terminal;

impl Terminal {
    /// Set or reset DEC private modes.
    pub(crate) fn set_private_modes(&mut self, params: &[Option<usize>], enabled: bool) {
        for param in params {
            if let Some(mode) = *param {
                if mode == 1049 {
                    // Alternate screen buffer (with cursor save/restore)
                    if enabled {
                        self.save_cursor();
                        self.switch_alternate_screen(true);
                        let style = self.current_style;
                        let cols = self.cols;
                        let rows = self.rows;
                        let screen = self.screen_mut();
                        screen.reset(cols, rows, style);
                    } else {
                        self.switch_alternate_screen(false);
                        self.restore_cursor();
                    }
                } else {
                    self.modes.set_private_mode(mode, enabled);
                }
            }
        }
    }

    /// Set cursor shape from DECSCUSR parameter.
    pub(crate) fn set_cursor_shape(&mut self, param: usize) {
        self.modes.set_cursor_shape(param);
    }
}
