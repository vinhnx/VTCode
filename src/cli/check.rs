use anyhow::{Context, Result, anyhow, bail};
use crossterm::style::Stylize;
use std::path::Path;
use tokio::process::Command;
use vtcode_core::cli::args::CheckSubcommand;
use vtcode_core::tools::ast_grep_binary::{
    missing_ast_grep_message, resolve_ast_grep_binary_from_env_and_fs,
};

const AST_GREP_CONFIG_PATH: &str = "sgconfig.yml";
const AST_GREP_INIT_COMMAND: &str = "vtcode init";

pub async fn handle_check_command(workspace: &Path, command: CheckSubcommand) -> Result<()> {
    match command {
        CheckSubcommand::AstGrep => handle_ast_grep_check(workspace).await,
    }
}

async fn handle_ast_grep_check(workspace: &Path) -> Result<()> {
    let ast_grep = resolve_ast_grep_binary_from_env_and_fs().ok_or_else(|| {
        anyhow!(missing_ast_grep_message(
            "After installation, run `vtcode init` to materialize the local ast-grep scaffold."
        ))
    })?;

    let config_path = workspace.join(AST_GREP_CONFIG_PATH);
    if !config_path.is_file() {
        bail!(
            "ast-grep scaffold is missing in {}. Run `{AST_GREP_INIT_COMMAND}` to materialize `sgconfig.yml`, `rules/`, and `rule-tests/` for this workspace.",
            workspace.display()
        );
    }

    println!("{}", "→ Running ast-grep rule tests...".cyan());
    run_ast_grep_subcommand(workspace, &ast_grep, "test")
        .await
        .with_context(|| "ast-grep rule tests failed")?;

    println!("{}", "→ Running ast-grep repository scan...".cyan());
    run_ast_grep_subcommand(workspace, &ast_grep, "scan")
        .await
        .with_context(|| "ast-grep scan found issues")?;

    println!("{}", "✓ ast-grep rules passed!".green());
    Ok(())
}

async fn run_ast_grep_subcommand(
    workspace: &Path,
    ast_grep: &Path,
    subcommand: &str,
) -> Result<()> {
    let status = Command::new(ast_grep)
        .current_dir(workspace)
        .arg(subcommand)
        .arg("--config")
        .arg(AST_GREP_CONFIG_PATH)
        .status()
        .await
        .with_context(|| format!("failed to run ast-grep {subcommand}"))?;

    if status.success() {
        return Ok(());
    }

    if subcommand == "test" {
        bail!("ast-grep exited with status {status} while running tests");
    }

    bail!("ast-grep exited with status {status} while running scan");
}

#[cfg(test)]
mod tests {
    use super::{AST_GREP_CONFIG_PATH, CheckSubcommand, handle_check_command};
    use serial_test::serial;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use vtcode_core::tools::ast_grep_binary::{
        AST_GREP_INSTALL_COMMAND, set_ast_grep_binary_override_for_tests,
    };

    fn create_workspace_with_scaffold() -> TempDir {
        let temp_dir = TempDir::new().expect("workspace");
        fs::write(temp_dir.path().join(AST_GREP_CONFIG_PATH), "ruleDirs: []\n").expect("config");
        temp_dir
    }

    fn create_ast_grep_stub(
        temp_dir: &TempDir,
        test_exit_code: i32,
        scan_exit_code: i32,
    ) -> PathBuf {
        #[cfg(windows)]
        let script_path = temp_dir.path().join("ast-grep-stub.cmd");
        #[cfg(not(windows))]
        let script_path = temp_dir.path().join("ast-grep-stub.sh");

        let log_dir = temp_dir.path().display().to_string();

        #[cfg(windows)]
        let body = format!(
            "@echo off\r\n\
set subcommand=%~1\r\n\
set log_dir={log_dir}\r\n\
> \"%log_dir%\\%subcommand%-args.log\" echo %subcommand%\r\n\
shift\r\n\
:args\r\n\
if \"%~1\"==\"\" goto done_args\r\n\
>> \"%log_dir%\\%subcommand%-args.log\" echo %~1\r\n\
shift\r\n\
goto args\r\n\
:done_args\r\n\
> \"%log_dir%\\%subcommand%-cwd.log\" echo %CD%\r\n\
if /I \"%subcommand%\"==\"test\" exit /b {test_exit_code}\r\n\
if /I \"%subcommand%\"==\"scan\" exit /b {scan_exit_code}\r\n\
exit /b 0\r\n"
        );

        #[cfg(not(windows))]
        let body = format!(
            "#!/bin/sh\n\
subcommand=\"$1\"\n\
shift\n\
log_dir='{log_dir}'\n\
{{\n\
  printf '%s\\n' \"$subcommand\"\n\
  for arg in \"$@\"; do\n\
    printf '%s\\n' \"$arg\"\n\
  done\n\
}} > \"$log_dir/$subcommand-args.log\"\n\
pwd > \"$log_dir/$subcommand-cwd.log\"\n\
case \"$subcommand\" in\n\
  test) exit {test_exit_code} ;;\n\
  scan) exit {scan_exit_code} ;;\n\
esac\n\
exit 0\n"
        );

        fs::write(&script_path, body).expect("write stub");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions).expect("chmod");
        }

        script_path
    }

    fn read_lines(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .expect("read log")
            .lines()
            .map(ToString::to_string)
            .collect()
    }

    #[tokio::test]
    #[serial]
    async fn ast_grep_check_runs_test_then_scan_from_workspace_root() {
        let workspace = create_workspace_with_scaffold();
        let stub = create_ast_grep_stub(&workspace, 0, 0);
        let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

        handle_check_command(workspace.path(), CheckSubcommand::AstGrep)
            .await
            .expect("ast-grep check should pass");

        let test_args = read_lines(&workspace.path().join("test-args.log"));
        assert_eq!(test_args, ["test", "--config", AST_GREP_CONFIG_PATH]);

        let scan_args = read_lines(&workspace.path().join("scan-args.log"));
        assert_eq!(scan_args, ["scan", "--config", AST_GREP_CONFIG_PATH]);

        let expected_workspace = fs::canonicalize(workspace.path()).expect("canonical workspace");

        let test_cwd = fs::read_to_string(workspace.path().join("test-cwd.log")).expect("test cwd");
        let test_cwd = fs::canonicalize(test_cwd.trim()).expect("canonical test cwd");
        assert_eq!(test_cwd, expected_workspace);

        let scan_cwd = fs::read_to_string(workspace.path().join("scan-cwd.log")).expect("scan cwd");
        let scan_cwd = fs::canonicalize(scan_cwd.trim()).expect("canonical scan cwd");
        assert_eq!(scan_cwd, expected_workspace);
    }

    #[tokio::test]
    #[serial]
    async fn ast_grep_check_reports_missing_binary() {
        let workspace = create_workspace_with_scaffold();
        let _guard = set_ast_grep_binary_override_for_tests(None);

        let result = handle_check_command(workspace.path(), CheckSubcommand::AstGrep).await;

        let error = result.expect_err("missing binary should fail").to_string();
        assert!(error.contains(AST_GREP_INSTALL_COMMAND), "{error}");
    }

    #[tokio::test]
    #[serial]
    async fn ast_grep_check_reports_missing_scaffold() {
        let workspace = TempDir::new().expect("workspace");
        let stub = create_ast_grep_stub(&workspace, 0, 0);
        let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

        let error = handle_check_command(workspace.path(), CheckSubcommand::AstGrep)
            .await
            .expect_err("missing scaffold should fail")
            .to_string();

        assert!(error.contains("vtcode init"), "{error}");
    }
}
