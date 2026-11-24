//! `vtcode debug` handlers: inspect and exercise the configured OS sandbox.

use anyhow::{Context, Result, bail};
use vtcode_core::cli::args::DebugCommand;
use vtcode_core::sandboxing::{
    DomainPolicy, LinuxSandboxLauncher, SandboxType, debug_sandbox, proxy_enforcement_available,
    sandbox_capabilities_summary, start_proxy,
};
use vtcode_core::tools::registry::sandbox_policy_from_runtime_config;

use crate::startup::StartupContext;

pub(crate) async fn handle_sandbox_debug_command(startup: &StartupContext, command: DebugCommand) -> Result<()> {
    match command {
        DebugCommand::Capabilities => {
            print!("{}", sandbox_capabilities_summary());
            Ok(())
        }
        DebugCommand::Seatbelt { command } => run_sandboxed(startup, SandboxType::MacosSeatbelt, &command).await,
        DebugCommand::Landlock { command } => run_sandboxed(startup, SandboxType::LinuxLandlock, &command).await,
    }
}

async fn run_sandboxed(startup: &StartupContext, sandbox_type: SandboxType, command: &[String]) -> Result<()> {
    if command.is_empty() {
        bail!("debug {sandbox_type:?} requires `--cmd <COMMAND...>`");
    }

    // Use the same policy the agent would apply in this workspace so the probe
    // measures the effective configuration, not a synthetic one.
    let policy = sandbox_policy_from_runtime_config(&startup.config.sandbox, &startup.workspace)?;
    let launcher = LinuxSandboxLauncher::resolve();
    let proxy = if policy.has_network_allowlist() && proxy_enforcement_available() {
        Some(
            start_proxy(DomainPolicy::from_allowlist(policy.network_allowlist()))
                .await
                .context("start hostname allowlist proxy for debug command")?,
        )
    } else {
        None
    };
    let endpoints = proxy.as_ref().map(|proxy| proxy.endpoints());
    let result =
        debug_sandbox(sandbox_type, &policy, command, &startup.workspace, launcher.as_ref(), endpoints.as_ref())
            .await?;

    if !result.sandbox_active() {
        let detail = result.stderr();
        if detail.trim().is_empty() {
            bail!("sandbox was not applied; the workspace policy may be danger-full-access");
        }
        bail!("sandbox was not applied: {detail}");
    }
    print!("{}", result.stdout());
    if !result.success() {
        bail!("sandboxed command failed: {}", result.stderr());
    }
    Ok(())
}
