use super::is_git_diff_command;

#[test]
fn detects_git_diff() {
    let cmd = vec!["git".to_string(), "diff".to_string()];
    assert!(is_git_diff_command(&cmd));
}

#[test]
fn detects_git_diff_with_flags() {
    let cmd = vec![
        "git".to_string(),
        "-c".to_string(),
        "color.ui=always".to_string(),
        "diff".to_string(),
        "--stat".to_string(),
    ];
    assert!(is_git_diff_command(&cmd));
}

#[test]
fn detects_git_diff_with_path() {
    let cmd = vec!["/usr/bin/git".to_string(), "diff".to_string()];
    assert!(is_git_diff_command(&cmd));
}

#[test]
fn ignores_other_git_commands() {
    let cmd = vec!["git".to_string(), "status".to_string()];
    assert!(!is_git_diff_command(&cmd));
}
