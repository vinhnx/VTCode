//! TUI utilities for OpenRouter OAuth integration.
//!
//! This module provides helper functions for displaying OAuth status
//! in the TUI and handling OAuth-related user interactions.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::auth::{get_auth_status, load_oauth_token};

/// OAuth status for TUI display.
#[derive(Debug, Clone, PartialEq)]
pub enum OAuthTuiStatus {
    /// User is authenticated via OAuth
    Authenticated,
    /// User is using API key (not OAuth)
    UsingApiKey,
    /// Authentication status unknown/error
    Unknown,
}

impl OAuthTuiStatus {
    /// Get the current OAuth status for OpenRouter.
    pub fn current() -> Self {
        match load_oauth_token() {
            Ok(Some(_)) => Self::Authenticated,
            Ok(None) => Self::UsingApiKey,
            Err(_) => Self::Unknown,
        }
    }

    /// Get a short badge string for the status.
    pub fn badge(&self) -> Option<&'static str> {
        match self {
            Self::Authenticated => Some("[OAuth]"),
            Self::UsingApiKey | Self::Unknown => None,
        }
    }

    /// Get the style for the OAuth badge.
    pub fn badge_style(&self) -> Style {
        match self {
            Self::Authenticated => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            Self::UsingApiKey | Self::Unknown => Style::default(),
        }
    }

    /// Create a Span for the OAuth badge if applicable.
    pub fn badge_span(&self) -> Option<Span<'static>> {
        self.badge()
            .map(|b| Span::styled(b.to_string(), self.badge_style()))
    }
}

/// Get detailed OAuth authentication status for display.
pub fn get_oauth_display_status() -> String {
    match get_auth_status() {
        Ok(status) => status.display_string(),
        Err(e) => format!("Error checking OAuth status: {}", e),
    }
}

/// Check if OpenRouter OAuth is currently active.
pub fn is_oauth_active() -> bool {
    matches!(OAuthTuiStatus::current(), OAuthTuiStatus::Authenticated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_status_badge() {
        let status = OAuthTuiStatus::Authenticated;
        assert_eq!(status.badge(), Some("[OAuth]"));

        let status = OAuthTuiStatus::UsingApiKey;
        assert_eq!(status.badge(), None);
    }

    #[test]
    fn test_badge_span() {
        let status = OAuthTuiStatus::Authenticated;
        let span = status.badge_span();
        assert!(span.is_some());

        let status = OAuthTuiStatus::UsingApiKey;
        let span = status.badge_span();
        assert!(span.is_none());
    }
}
