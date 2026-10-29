use super::*;

#[test]
fn test_suggests_limit_for_cat() {
    assert_eq!(suggest_max_tokens_for_command("cat file.txt"), Some(250));
    assert_eq!(suggest_max_tokens_for_command("cat /path/to/file.rs"), Some(250));
    assert_eq!(suggest_max_tokens_for_command("CAT file.txt"), Some(250)); // case insensitive
}

#[test]
fn test_suggests_limit_for_bat() {
    assert_eq!(suggest_max_tokens_for_command("bat file.rs"), Some(250));
}

#[test]
fn test_no_limit_when_already_limited() {
    assert_eq!(suggest_max_tokens_for_command("cat file.txt | head"), None);
    assert_eq!(suggest_max_tokens_for_command("head -n 50 file.txt"), None);
    assert_eq!(suggest_max_tokens_for_command("tail -n 20 file.txt"), None);
}

#[test]
fn test_no_limit_for_other_commands() {
    assert_eq!(suggest_max_tokens_for_command("ls -la"), None);
    assert_eq!(suggest_max_tokens_for_command("grep pattern file"), None);
    assert_eq!(suggest_max_tokens_for_command("echo hello"), None);
}
