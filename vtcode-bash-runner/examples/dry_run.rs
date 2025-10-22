use std::sync::{Arc, Mutex};

use anyhow::Result;
use vtcode_bash_runner::{
    AllowAllPolicy, BashRunner, CommandCategory, CommandExecutor, CommandInvocation, CommandOutput,
    CommandStatus,
};

#[derive(Clone, Default)]
struct DryRunExecutor {
    log: Arc<Mutex<Vec<CommandInvocation>>>,
}

impl CommandExecutor for DryRunExecutor {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
        self.log.lock().unwrap().push(invocation.clone());
        if invocation.category == CommandCategory::ListDirectory {
            Ok(CommandOutput::success("(simulated listing)"))
        } else {
            Ok(CommandOutput {
                status: CommandStatus::new(true, Some(0)),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

fn main() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let executor = DryRunExecutor::default();
    let policy = AllowAllPolicy;

    let mut runner = BashRunner::new(workspace.path().to_path_buf(), executor.clone(), policy)?;

    runner.mkdir("logs", true)?;
    runner.cd("logs")?;
    let listing = runner.ls(None, false)?;

    println!("dry-run listing: {}", listing);

    for invocation in executor.log.lock().unwrap().iter() {
        println!("{}", invocation.command);
    }

    Ok(())
}
