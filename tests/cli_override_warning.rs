use assert_fs::TempDir;
use clap::Parser;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::Cli;

#[tokio::test]
async fn cli_override_with_non_responses_model_warns() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().to_path_buf();

    unsafe {
        std::env::set_var("OPENAI_API_KEY", "test");
    }
    let args = Cli::try_parse_from([
        "vtcode",
        "--workspace",
        workspace.to_str().unwrap(),
        "--model",
        "gpt-oss-20b",
        "--config",
        "prompt_cache.providers.openai.prompt_cache_retention=24h",
    ])
    .unwrap();

    let ctx = StartupContext::from_cli_args(&args)
        .await
        .expect("startup success");
    let maybe_warning = vtcode::startup::check_prompt_cache_retention_compat(
        &ctx.config,
        &ctx.agent_config.model,
        &ctx.agent_config.provider,
    );

    assert!(maybe_warning.is_some());
}

#[tokio::test]
async fn cli_override_with_responses_model_no_warn() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().to_path_buf();

    unsafe {
        std::env::set_var("OPENAI_API_KEY", "test");
    }
    let args = Cli::try_parse_from([
        "vtcode",
        "--workspace",
        workspace.to_str().unwrap(),
        "--model",
        "gpt-oss-20b",
        "--config",
        "prompt_cache.providers.openai.prompt_cache_retention=24h",
    ])
    .unwrap();

    let ctx = StartupContext::from_cli_args(&args)
        .await
        .expect("startup success");
    let maybe_warning = vtcode::startup::check_prompt_cache_retention_compat(
        &ctx.config,
        &ctx.agent_config.model,
        &ctx.agent_config.provider,
    );

    assert!(maybe_warning.is_none());
}
