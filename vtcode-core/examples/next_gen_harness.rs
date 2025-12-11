use std::path::PathBuf;

use vtcode_core::orchestrator::{DistributedOrchestrator, ExecutionTarget, ScheduledWork};
use vtcode_core::telemetry::{TelemetryEvent, TelemetryPipeline};
use vtcode_core::tools::plugins::PluginRuntime;
use vtcode_core::utils::migration::apply_migration_defaults;
use vtcode_core::{PluginRuntimeConfig, PolicyContext, RlEngine, VTCodeConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = VTCodeConfig::default();
    apply_migration_defaults(&mut config);

    // Initialize plugin runtime (hot-swappable).
    let plugin_runtime = PluginRuntime::new(PathBuf::from("."), PluginRuntimeConfig::default());
    let _ = plugin_runtime
        .register_manifest("examples/plugin_example.toml")
        .await
        .ok();

    // RL engine prefers low-latency actions.
    let rl_engine = RlEngine::from_config(&config.optimization);
    let decision = rl_engine
        .select(
            &[String::from("cloud"), String::from("edge")],
            PolicyContext::default(),
        )
        .await?;

    // Orchestrate a simple job.
    let orchestrator = DistributedOrchestrator::new();
    orchestrator
        .submit(ScheduledWork::new(
            "demo",
            ExecutionTarget::Custom(decision.action),
            serde_json::json!({"task": "telemetry-refresh"}),
            serde_json::json!({"priority": decision.priority}),
        ))
        .await?;

    // Emit startup telemetry.
    let telemetry = TelemetryPipeline::new(config.telemetry.clone());
    telemetry.record(TelemetryEvent::new("next_gen_boot", 1.0)).await?;

    Ok(())
}
