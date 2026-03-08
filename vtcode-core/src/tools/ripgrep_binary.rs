pub const RIPGREP_INSTALL_COMMAND: &str = "vtcode dependencies install ripgrep";

pub fn missing_ripgrep_message(suffix: &str) -> String {
    let extra = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    format!(
        "ripgrep (`rg`) is not available on PATH; run `{RIPGREP_INSTALL_COMMAND}` or install `ripgrep` manually.{extra}"
    )
}

#[cfg(test)]
mod tests {
    use super::{RIPGREP_INSTALL_COMMAND, missing_ripgrep_message};

    #[test]
    fn missing_message_includes_install_command() {
        let message = missing_ripgrep_message("VT Code can fall back to built-in grep.");
        assert!(message.contains(RIPGREP_INSTALL_COMMAND));
        assert!(message.contains("built-in grep"));
    }
}
