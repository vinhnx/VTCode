use anyhow::{Result, bail};

fn is_disallowed_control_char(ch: char) -> bool {
    if matches!(ch, '\n' | '\r' | '\t') {
        return false;
    }
    ch.is_control()
}

fn first_disallowed_control_char(value: &str) -> Option<char> {
    value.chars().find(|ch| is_disallowed_control_char(*ch))
}

pub fn validate_agent_safe_text(field_name: &str, value: &str) -> Result<()> {
    if let Some(ch) = first_disallowed_control_char(value) {
        bail!(
            "Invalid {}: contains unsupported control character U+{:04X}",
            field_name,
            ch as u32
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_agent_safe_text;

    #[test]
    fn allows_printable_text() {
        assert!(validate_agent_safe_text("prompt", "hello world").is_ok());
    }

    #[test]
    fn allows_newline_tab_and_carriage_return() {
        assert!(validate_agent_safe_text("prompt", "line1\nline2\r\n\tindent").is_ok());
    }

    #[test]
    fn rejects_nul() {
        let err =
            validate_agent_safe_text("prompt", "hello\0world").expect_err("nul should be rejected");
        assert!(err.to_string().contains("U+0000"));
    }

    #[test]
    fn rejects_other_control_characters() {
        let err = validate_agent_safe_text("prompt", "hello\u{0007}world")
            .expect_err("bell should be rejected");
        assert!(err.to_string().contains("U+0007"));
    }
}
