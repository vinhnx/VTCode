use super::*;

async fn initialized_context(workspace: &Path) -> ServiceContext {
    let (context, _) = test_service_context(test_startup_context(workspace), workspace.to_path_buf());
    context.state.lock().await.initialized = true;
    context
}

async fn recent_prompts(context: &ServiceContext, params: Value) -> Result<Value> {
    let (result, _) = handle_request(
        context,
        &Request {
            jsonrpc: "2.0".into(),
            id: RequestId::String("recent-prompts".into()),
            method: protocol::method::SESSION_RECENT_PROMPTS.into(),
            params,
        },
    )
    .await?;
    Ok(result)
}

#[tokio::test]
async fn recent_prompts_session_scope_returns_empty_without_reading_archives() {
    let directory = tempfile::tempdir().expect("workspace");
    let context = initialized_context(directory.path()).await;

    let result = recent_prompts(&context, json!({ "scope": "session" }))
        .await
        .expect("session recents");
    assert_eq!(result, json!({ "scope": "session", "prompts": [] }));
}

#[tokio::test]
async fn recent_prompts_rejects_unknown_scope() {
    let directory = tempfile::tempdir().expect("workspace");
    let context = initialized_context(directory.path()).await;

    let error = recent_prompts(&context, json!({ "scope": "galaxy" }))
        .await
        .expect_err("unknown scope");
    assert!(error.to_string().contains("invalid request parameters"), "unexpected error: {error}");
}
