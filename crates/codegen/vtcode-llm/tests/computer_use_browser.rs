#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "Test assertions."
)]
//! Full-stack computer-use proof: the provider adapters drive the real turn
//! controller, browser driver, and container. Each test builds a hermetic
//! fixture app in a temp dir, so no cross-crate fixtures are shared. They
//! skip loudly when Docker is unavailable and fail with a build hint when
//! the image is missing.

use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use vtcode_computer_use::TurnControl;
use vtcode_computer_use::browser::BrowserDriver;
use vtcode_computer_use::browser::container::{ContainerHandle, ContainerSpec, TerminateOutcome, docker_available};
use vtcode_llm::providers::anthropic::computer_use as anthropic_use;
use vtcode_llm::providers::openai::computer_use as openai_use;

const IMAGE: &str = "vtcode-computer-use-browser:1.64.1";
const FLOW_TIMEOUT: Duration = Duration::from_secs(240);
const BATCH_TIMEOUT: Duration = Duration::from_secs(30);
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

const FIXTURE_APP: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>fixture</title></head>
<body style="margin:0">
<button id="btn" style="position:absolute;left:400px;top:300px;width:200px;height:80px">Go</button>
<input id="field" style="position:absolute;left:400px;top:420px;width:200px;height:40px">
</body></html>
"#;

async fn require_docker() -> bool {
    if docker_available().await {
        return true;
    }
    eprintln!("skipping computer-use browser test: docker is unavailable");
    false
}

async fn image_present() -> bool {
    tokio::process::Command::new("docker")
        .args(["images", "--quiet", IMAGE])
        .kill_on_drop(true)
        .output()
        .await
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

async fn container_gone(id: &str) -> bool {
    !tokio::process::Command::new("docker")
        .args(["inspect", id])
        .kill_on_drop(true)
        .output()
        .await
        .is_ok_and(|output| output.status.success())
}

async fn poll_until_gone(id: &str) {
    let start = tokio::time::Instant::now();
    loop {
        if container_gone(id).await {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "container did not exit");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn launch_for_test() -> (
    tempfile::TempDir,
    ContainerHandle,
    BrowserDriver<tokio::process::ChildStdout, tokio::process::ChildStdin>,
) {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("index.html"), FIXTURE_APP).unwrap();
    let spec = ContainerSpec::approved(IMAGE, fixture.path()).await.unwrap();
    match ContainerHandle::launch(&spec).await {
        Ok((handle, replies, commands)) => {
            let driver = BrowserDriver::connect(replies, commands).await.unwrap();
            (fixture, handle, driver)
        }
        Err(error) => {
            if !image_present().await {
                panic!("browser image is missing; build it first: docker build --tag {IMAGE} browser ({error:#})");
            }
            panic!("could not launch the browser container: {error:#}");
        }
    }
}

fn assert_png_image(data: &str) {
    let bytes = STANDARD.decode(data).unwrap();
    assert!(bytes.starts_with(PNG_MAGIC), "result is not a PNG screenshot");
}

#[tokio::test]
async fn computer_use_anthropic_end_to_end_over_real_browser() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (_fixture, handle, driver) = launch_for_test().await;
        let mut control = TurnControl::after_approval(driver, &CancellationToken::new());
        let blocks = [
            json!({"type": "tool_use", "id": "click", "toolset_name": "computer", "name": "left_click",
                   "input": {"coordinate": [500, 340]}}),
            json!({"type": "tool_use", "id": "typed", "toolset_name": "computer", "name": "type",
                   "input": {"text": "hello"}}),
            json!({"type": "tool_use", "id": "shot", "toolset_name": "computer", "name": "screenshot",
                   "input": {}}),
        ];
        let results = anthropic_use::execute_batch(&mut control, &blocks, BATCH_TIMEOUT)
            .await
            .unwrap();
        let [(0, clicked), (1, typed), (2, shot)] = results.as_slice() else {
            panic!("expected three positioned results");
        };
        assert_eq!(clicked["tool_use_id"], "click");
        assert_eq!(clicked["content"], "OK");
        assert_eq!(typed["tool_use_id"], "typed");
        assert_eq!(typed["content"], "OK");
        assert_eq!(shot["tool_use_id"], "shot");
        let data = shot["content"][0]["source"]["data"].as_str().unwrap();
        assert_eq!(shot["content"][0]["source"]["media_type"], "image/png");
        assert_png_image(data);

        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert!(container_gone(handle.id()).await);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn computer_use_anthropic_failure_halts_real_batch() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (_fixture, handle, driver) = launch_for_test().await;
        let mut control = TurnControl::after_approval(driver, &CancellationToken::new());
        let blocks = [
            json!({"type": "tool_use", "id": "typed", "toolset_name": "computer", "name": "type",
                   "input": {"text": "first"}}),
            json!({"type": "tool_use", "id": "broken", "toolset_name": "computer", "name": "left_click",
                   "input": {"coordinate": [5000, 5000]}}),
            json!({"type": "tool_use", "id": "skipped", "toolset_name": "computer", "name": "type",
                   "input": {"text": "must not execute"}}),
        ];
        let results = anthropic_use::execute_batch(&mut control, &blocks, BATCH_TIMEOUT)
            .await
            .unwrap();
        let [(0, typed), (1, broken), (2, skipped)] = results.as_slice() else {
            panic!("expected three positioned results");
        };
        assert_eq!(typed["content"], "OK");
        assert_eq!(broken["is_error"], true);
        assert_eq!(skipped["is_error"], true);
        assert_eq!(skipped["content"], "Not executed: an earlier computer action in this turn failed.");

        poll_until_gone(handle.id()).await;
        assert!(matches!(
            handle.terminate().await.unwrap(),
            TerminateOutcome::Removed | TerminateOutcome::AlreadyGone
        ));
        assert!(container_gone(handle.id()).await);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn computer_use_openai_end_to_end_over_real_browser() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (_fixture, handle, driver) = launch_for_test().await;
        let mut control = TurnControl::after_approval(driver, &CancellationToken::new());
        let item = json!({"type": "computer_call", "id": "cua_item", "call_id": "call_live",
        "status": "completed", "actions": [
            {"type": "move", "x": 500, "y": 440},
            {"type": "click", "x": 500, "y": 440, "button": "left"},
            {"type": "type", "text": "live"},
            {"type": "screenshot"},
        ]});
        let call = openai_use::decode_call(&item).unwrap().unwrap();
        let result = openai_use::execute_call(&mut control, &call, BATCH_TIMEOUT).await.unwrap();
        assert_eq!(result["type"], "computer_call_output");
        assert_eq!(result["call_id"], "call_live");
        assert_eq!(result["output"]["type"], "computer_screenshot");
        let data = result["output"]["image_url"]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        assert_png_image(data);

        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert!(container_gone(handle.id()).await);
    })
    .await
    .unwrap();
}
