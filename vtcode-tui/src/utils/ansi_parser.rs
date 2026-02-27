use once_cell::sync::Lazy;
use regex::Regex;

static ANSI_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1B\\)|[@-Z\\-_])")
        .expect("valid ansi regex")
});

pub fn strip_ansi(input: &str) -> String {
    ANSI_RE.replace_all(input, "").to_string()
}
