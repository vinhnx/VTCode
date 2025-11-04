use anyhow::Result;
use assert_fs::TempDir;
use vtcode_bash_runner::{AllowAllPolicy, BashRunner, DryRunCommandExecutor};

fn main() -> Result<()> {
    let workspace = TempDir::new()?;
    let executor = DryRunCommandExecutor::new();
    let policy = AllowAllPolicy;

    let mut runner = BashRunner::new(workspace.path().to_path_buf(), executor.clone(), policy)?;

    runner.mkdir("logs", true)?;
    runner.cd("logs")?;
    let listing = runner.ls(None, false)?;

    println!("dry-run listing: {}", listing);

    for invocation in executor.logged_invocations() {
        println!("{}", invocation.command);
    }

    Ok(())
}
