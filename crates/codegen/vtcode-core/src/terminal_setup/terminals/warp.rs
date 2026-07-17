//! Warp terminal configuration generator.
//!
//! Warp has built-in multiline support and limited customization.

use anyhow::Result;

/// Generate Warp configuration (mostly informational since Warp has built-in features)
pub fn generate_config(
    features: &[crate::terminal_setup::detector::TerminalFeature],
) -> Result<String> {
    let mut info = r#"# Warp Terminal Configuration

Warp has built-in support for most features:

✓ Multiline Input: Already supported (Shift+Enter works by default)
✓ Copy/Paste: Built-in with smart selection
✓ Shell Integration: Built-in with command history and navigation
✓ Themes: Use Warp's built-in theme system

"#
    .to_string();

    // Check if notifications feature is requested
    if features.contains(&crate::terminal_setup::detector::TerminalFeature::Notifications) {
        info.push_str("\n## System Notifications\n");
        info.push_str("✓ Notifications: Warp supports system notifications through:\n");
        info.push_str("  - Built-in notification system\n");
        info.push_str("  - Terminal bell (\\a) for task completion alerts\n");
        info.push_str("  - No additional configuration needed\n");
    }

    info.push_str("\nNo additional configuration needed for VT Code!\n");

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_config() {
        let features = vec![TerminalFeature::Multiline];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("Warp"));
        assert!(config.contains("built-in"));
    }
}
