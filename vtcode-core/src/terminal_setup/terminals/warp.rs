//! Warp terminal configuration generator.
//!
//! Warp has built-in multiline support and limited customization.

use anyhow::Result;

/// Generate Warp configuration (mostly informational since Warp has built-in features)
pub fn generate_config(_features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let info = r#"# Warp Terminal Configuration

Warp has built-in support for most features:

✓ Multiline Input: Already supported (Shift+Enter works by default)
✓ Copy/Paste: Built-in with smart selection
✓ Shell Integration: Built-in with command history and navigation
✓ Themes: Use Warp's built-in theme system

No additional configuration needed for VTCode!
"#;

    Ok(info.to_string())
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
