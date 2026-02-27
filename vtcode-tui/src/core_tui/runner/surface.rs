use std::io;

use anyhow::Result;
use ratatui::crossterm::terminal;
use terminal_size::{Height, Width, terminal_size};

use crate::config::{constants::ui, types::UiSurfacePreference};

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;

pub(super) struct TerminalSurface {
    rows: u16,
    alternate: bool,
}

impl TerminalSurface {
    pub(super) fn detect(preference: UiSurfacePreference, inline_rows: u16) -> Result<Self> {
        use crate::utils::tty::TtyExt;

        let fallback_rows = inline_rows.max(1);
        let stderr_is_terminal = io::stderr().is_tty_ext();

        // Detect terminal capabilities before proceeding
        let capabilities = if stderr_is_terminal {
            crate::utils::tty::TtyCapabilities::detect()
        } else {
            None
        };

        let resolved_rows = if stderr_is_terminal {
            match measure_terminal_dimensions() {
                Some((_, rows)) if rows > 0 => rows,
                _ => match terminal::size() {
                    Ok((_, 0)) => fallback_rows.max(INLINE_FALLBACK_ROWS),
                    Ok((_, rows)) => rows,
                    Err(error) => {
                        tracing::debug!(%error, "failed to determine terminal size");
                        fallback_rows.max(INLINE_FALLBACK_ROWS)
                    }
                },
            }
        } else {
            fallback_rows.max(INLINE_FALLBACK_ROWS)
        };

        let resolved_rows = resolved_rows.max(1);

        // Check if terminal supports the features we need
        if stderr_is_terminal
            && let Some(caps) = capabilities
            && !caps.is_basic_tui()
        {
            tracing::warn!("Terminal has limited capabilities, some features may be disabled");
        }

        let use_alternate = match preference {
            UiSurfacePreference::Alternate => stderr_is_terminal,
            UiSurfacePreference::Inline => false,
            UiSurfacePreference::Auto => stderr_is_terminal,
        };

        if use_alternate && !stderr_is_terminal {
            tracing::debug!("alternate surface requested but stderr is not a tty");
        }

        Ok(Self {
            rows: resolved_rows,
            alternate: use_alternate && stderr_is_terminal,
        })
    }

    pub(super) fn rows(&self) -> u16 {
        self.rows
    }

    pub(super) fn use_alternate(&self) -> bool {
        self.alternate
    }
}

fn measure_terminal_dimensions() -> Option<(u16, u16)> {
    let (Width(columns), Height(rows)) = terminal_size()?;
    if rows == 0 {
        return None;
    }
    Some((columns, rows))
}
