use vtcode_core::VTCodeConfig;
use vtcode_core::llm::rl::{PolicyContext, RlEngine};
use vtcode_core::orchestrator::{DistributedOrchestrator, ExecutionTarget, ScheduledWork};
use vtcode_core::utils::migration::apply_migration_defaults;

#[tokio::test]
async fn orchestration_pipeline_smoke() {
    let mut config = VTCodeConfig::default();
    apply_migration_defaults(&mut config);

    let rl = RlEngine::from_config(&config.optimization);
    let decision = rl
        .select(
            &[String::from("cloud"), String::from("edge")],
            PolicyContext::default(),
        )
        .await
        .expect("rl decision");

    let orchestrator = DistributedOrchestrator::new();
    orchestrator
        .submit(ScheduledWork::new(
            "stress-1",
            ExecutionTarget::Custom(decision.action),
            serde_json::json!({"task": "smoke"}),
            serde_json::json!({"priority": decision.priority}),
        ))
        .await
        .expect("submit work");

    assert_eq!(orchestrator.queue_depth().await, 1);
    let result = orchestrator.tick().await.expect("tick ok");
    assert!(result.is_some());
}
