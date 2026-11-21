#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "Test assertions."
)]
//! Container-backed proof for the isolated browser target.
//!
//! These tests launch the real hardened container, drive it through
//! [`BrowserDriver`], and assert page pixels, daemon-applied isolation, and
//! exact teardown. They are opt-in via `VTCODE_CUA_CONTAINER=1` (the image must
//! be built first), skip when Docker is unavailable, and fail with a build
//! hint when the image is missing.

use std::path::PathBuf;
use std::time::Duration;

use tokio::time::sleep;
use vtcode_computer_use::browser::BrowserDriver;
use vtcode_computer_use::browser::container::{ContainerHandle, ContainerSpec, TerminateOutcome, docker_available};
use vtcode_computer_use::{Action, Driver, MouseButton, Observation, Point, TurnControl};

const IMAGE: &str = "vtcode-computer-use-browser:1.65.1";
const FLOW_TIMEOUT: Duration = Duration::from_secs(240);
const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];

fn fixture_app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/browser-app")
}

async fn require_docker() -> bool {
    // These are container-backed proof tests: Docker alone is not enough, the
    // pinned image must be built locally. CI has Docker but not the image, so
    // the tests are opt-in (see the crate README).
    if std::env::var("VTCODE_CUA_CONTAINER").as_deref() != Ok("1") {
        eprintln!(
            "skipping browser container test: set VTCODE_CUA_CONTAINER=1 to run (requires the built browser image)"
        );
        return false;
    }
    if docker_available().await {
        return true;
    }
    eprintln!("skipping browser container test: docker is unavailable");
    false
}

async fn launch_for_test() -> (ContainerHandle, BrowserDriver<tokio::process::ChildStdout, tokio::process::ChildStdin>)
{
    let spec = ContainerSpec::approved(IMAGE, &fixture_app()).await.unwrap();
    match ContainerHandle::launch(&spec).await {
        Ok((handle, replies, commands)) => {
            let driver = BrowserDriver::connect(replies, commands).await.unwrap();
            (handle, driver)
        }
        Err(error) => {
            if !image_present().await {
                panic!("browser image is missing; build it first: docker build --tag {IMAGE} browser ({error:#})");
            }
            panic!("could not launch the browser container: {error:#}");
        }
    }
}

async fn image_present() -> bool {
    tokio::process::Command::new("docker")
        .args(["images", "--quiet", IMAGE])
        .kill_on_drop(true)
        .output()
        .await
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

async fn inspect_format(id: &str, format: &str) -> Option<String> {
    tokio::process::Command::new("docker")
        .args(["inspect", &format!("--format={format}"), id])
        .kill_on_drop(true)
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn container_running(id: &str) -> Option<bool> {
    inspect_format(id, "{{.State.Running}}").await.map(|state| state == "true")
}

async fn poll_screenshot_until(
    driver: &mut BrowserDriver<tokio::process::ChildStdout, tokio::process::ChildStdin>,
    description: &str,
    timeout: Duration,
    settled: impl Fn(&image::RgbImage) -> bool,
) {
    let start = tokio::time::Instant::now();
    loop {
        let png = screenshot_png(driver).await;
        let shot = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!((shot.width(), shot.height()), (1280, 800), "unexpected screenshot size");
        if settled(&shot) {
            return;
        }
        assert!(start.elapsed() < timeout, "{description} did not settle");
        sleep(Duration::from_millis(250)).await;
    }
}

async fn poll_until_gone(description: &str, timeout: Duration, id: &str) {
    let start = tokio::time::Instant::now();
    loop {
        if container_running(id).await != Some(true) {
            return;
        }
        assert!(start.elapsed() < timeout, "{description} did not settle");
        sleep(Duration::from_millis(250)).await;
    }
}

async fn screenshot_png(
    driver: &mut BrowserDriver<tokio::process::ChildStdout, tokio::process::ChildStdin>,
) -> Vec<u8> {
    let token = tokio_util::sync::CancellationToken::new();
    match driver.execute(&Action::Screenshot, &token).await.unwrap() {
        Observation::Screenshot(image) => image.as_png().to_vec(),
        other => panic!("expected a screenshot, got {}", observation_name(&other)),
    }
}

fn observation_name(observation: &Observation) -> &'static str {
    match observation {
        Observation::Done => "done",
        Observation::Screenshot(_) => "screenshot",
        Observation::CursorPosition(_) => "cursor position",
    }
}

fn pixel(shot: &image::RgbImage, x: u32, y: u32) -> [u8; 3] {
    shot.get_pixel(x, y).0
}

async fn click(driver: &mut BrowserDriver<tokio::process::ChildStdout, tokio::process::ChildStdin>, x: u32, y: u32) {
    let token = tokio_util::sync::CancellationToken::new();
    let action = Action::Click {
        at: Some(Point { x, y }),
        button: MouseButton::Left,
        count: 1,
        modifiers: Vec::new(),
    };
    assert!(matches!(driver.execute(&action, &token).await.unwrap(), Observation::Done));
}

#[tokio::test]
async fn browser_container_delivers_input_and_contains_the_target() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let symlink_intact = std::fs::symlink_metadata(fixture_app().join("evil.png"))
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        if !symlink_intact {
            eprintln!("browser-app/evil.png is not a symlink in this checkout; skipping the escape assertion");
        }
        let (handle, mut driver) = launch_for_test().await;

        poll_screenshot_until(&mut driver, "fixture markers", SETTLE_TIMEOUT, |shot| {
            pixel(shot, 70, 70) == GREEN
                && pixel(shot, 210, 70) == RED
                && (!symlink_intact || pixel(shot, 350, 70) == RED)
        })
        .await;

        click(&mut driver, 600, 340).await;
        poll_screenshot_until(&mut driver, "click marker", SETTLE_TIMEOUT, |shot| pixel(shot, 560, 460) == GREEN).await;

        let token = tokio_util::sync::CancellationToken::new();
        match driver.execute(&Action::CursorPosition, &token).await.unwrap() {
            Observation::CursorPosition(point) => assert_eq!(point, Point { x: 600, y: 340 }),
            other => panic!("expected a cursor position, got {}", observation_name(&other)),
        }

        let token = tokio_util::sync::CancellationToken::new();
        let out_of_bounds = Action::Click {
            at: Some(Point { x: 5000, y: 5000 }),
            button: MouseButton::Left,
            count: 1,
            modifiers: Vec::new(),
        };
        assert!(driver.execute(&out_of_bounds, &token).await.is_err());

        poll_until_gone("container exit", TEARDOWN_TIMEOUT, handle.id()).await;
        assert!(matches!(
            handle.terminate().await.unwrap(),
            TerminateOutcome::Removed | TerminateOutcome::AlreadyGone
        ));
        assert!(container_running(handle.id()).await != Some(true));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_container_terminate_is_exact_and_idempotent() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let spec = ContainerSpec::approved(IMAGE, &fixture_app()).await.unwrap();
        let (handle, replies, commands) = ContainerHandle::launch(&spec).await.unwrap();
        let mut driver = BrowserDriver::connect(replies, commands).await.unwrap();
        assert_eq!(container_running(handle.id()).await, Some(true));

        assert_eq!(inspect_format(handle.id(), "{{.HostConfig.NetworkMode}}").await.as_deref(), Some("none"));
        assert_eq!(inspect_format(handle.id(), "{{.HostConfig.ReadonlyRootfs}}").await.as_deref(), Some("true"));
        let mounts = inspect_format(handle.id(), "{{json .Mounts}}").await.unwrap();
        let mounts: serde_json::Value = serde_json::from_str(&mounts).unwrap();
        assert_eq!(mounts[0]["Destination"], "/build");
        assert_eq!(mounts[0]["RW"], false);

        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert_eq!(container_running(handle.id()).await, None);
        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::AlreadyGone);
        assert_eq!(container_running(handle.id()).await, None);

        let token = tokio_util::sync::CancellationToken::new();
        assert!(driver.execute(&Action::Screenshot, &token).await.is_err());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_container_dropped_pipes_end_the_target() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let spec = ContainerSpec::approved(IMAGE, &fixture_app()).await.unwrap();
        let id = {
            let (handle, replies, commands) = ContainerHandle::launch(&spec).await.unwrap();
            let id = handle.id().to_owned();
            let _driver = BrowserDriver::connect(replies, commands).await.unwrap();
            id
        };
        poll_until_gone("container self-removal", TEARDOWN_TIMEOUT, &id).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_control_executes_ordered_batch_over_real_target() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (handle, driver) = launch_for_test().await;
        let turn = tokio_util::sync::CancellationToken::new();
        let mut control = TurnControl::after_approval(driver, &turn);
        let batch = [
            Action::Click {
                at: Some(Point { x: 600, y: 580 }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            },
            Action::Type("hello".into()),
            Action::Click {
                at: Some(Point { x: 600, y: 340 }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            },
            Action::Screenshot,
        ];
        let observations = control
            .execute_batch("call_real", &batch, Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(observations.len(), 4);
        let Observation::Screenshot(image) = observations.last().unwrap() else {
            panic!("expected a final screenshot");
        };
        let shot = image::load_from_memory(image.as_png()).unwrap().to_rgb8();
        assert_eq!(pixel(&shot, 560, 460), GREEN);
        assert_eq!(pixel(&shot, 560, 680), GREEN);

        assert!(
            control
                .execute_batch("call_real", &batch, Duration::from_secs(30))
                .await
                .is_err()
        );
        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert_eq!(container_running(handle.id()).await, None);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_control_partial_failure_revokes_real_target() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (handle, driver) = launch_for_test().await;
        let turn = tokio_util::sync::CancellationToken::new();
        let mut control = TurnControl::after_approval(driver, &turn);
        let delivered = [
            Action::Click {
                at: Some(Point { x: 600, y: 340 }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            },
            Action::Screenshot,
        ];
        let observations = control
            .execute_batch("call_ok", &delivered, Duration::from_secs(30))
            .await
            .unwrap();
        let Observation::Screenshot(image) = observations.last().unwrap() else {
            panic!("expected a screenshot");
        };
        let shot = image::load_from_memory(image.as_png()).unwrap().to_rgb8();
        assert_eq!(pixel(&shot, 560, 460), GREEN);

        let failing = [
            Action::Type("before-failure".into()),
            Action::Click {
                at: Some(Point { x: 5000, y: 5000 }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            },
        ];
        assert!(
            control
                .execute_batch("call_fail", &failing, Duration::from_secs(30))
                .await
                .is_err()
        );
        assert!(
            control
                .execute_batch("call_after", &delivered, Duration::from_secs(30))
                .await
                .is_err()
        );

        poll_until_gone("container exit", TEARDOWN_TIMEOUT, handle.id()).await;
        assert!(matches!(
            handle.terminate().await.unwrap(),
            TerminateOutcome::Removed | TerminateOutcome::AlreadyGone
        ));
        assert_eq!(container_running(handle.id()).await, None);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_control_revocation_stops_real_dispatch_without_teardown() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (handle, driver) = launch_for_test().await;
        let turn = tokio_util::sync::CancellationToken::new();
        let mut control = TurnControl::after_approval(driver, &turn);
        let held_batch = [Action::Wait(Duration::from_secs(60))];
        let mut run = std::pin::pin!(control.execute_batch("call_held", &held_batch, SETTLE_TIMEOUT));
        assert!(futures::poll!(&mut run).is_pending());
        turn.cancel();
        assert!(run.await.is_err());

        assert_eq!(container_running(handle.id()).await, Some(true));
        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert_eq!(container_running(handle.id()).await, None);
    })
    .await
    .unwrap();
}

async fn raw_exchange(frames: Vec<serde_json::Value>) -> (Vec<String>, bool) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let (handle, replies, commands) =
        ContainerHandle::launch(&ContainerSpec::approved(IMAGE, &fixture_app()).await.unwrap())
            .await
            .unwrap();
    let mut commands = commands;
    let mut lines = tokio::io::BufReader::new(replies).lines();
    assert_eq!(lines.next_line().await.unwrap().unwrap(), "{\"kind\":\"ready\"}");
    let mut kinds = Vec::with_capacity(frames.len());
    for frame in frames {
        let mut frame = serde_json::to_vec(&frame).unwrap();
        frame.push(b'\n');
        commands.write_all(&frame).await.unwrap();
        commands.flush().await.unwrap();
        let reply = lines.next_line().await.unwrap().unwrap();
        kinds.push(reply_kind(&reply));
    }
    // A live runner holds the stream open; an exited one delivers EOF promptly.
    let closed = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .is_ok_and(|line| line.unwrap().is_none());
    assert!(matches!(
        handle.terminate().await.unwrap(),
        TerminateOutcome::Removed | TerminateOutcome::AlreadyGone
    ));
    (kinds, closed)
}

fn reply_kind(reply: &str) -> String {
    serde_json::from_str::<serde_json::Value>(reply)
        .unwrap()
        .get("kind")
        .and_then(|kind| kind.as_str())
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn browser_runner_rejects_malformed_actions() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        // Positive control proves the harness: a valid action succeeds and
        // the runner stays alive for the next one.
        let (kinds, closed) = raw_exchange(vec![serde_json::json!({
            "kind": "click", "at": {"x": 100, "y": 100}, "button": "left", "count": 1, "modifiers": []
        })])
        .await;
        assert_eq!(kinds, ["done"]);
        assert!(!closed);

        // Each probe isolates exactly one invalid field; the runner must
        // refuse and exit rather than act or hang.
        let mut malformed = vec![
            serde_json::json!({"kind": "click", "at": {"x": 100, "y": 100}, "button": "left",
                               "count": 999999999, "modifiers": []}),
            serde_json::json!({"kind": "click", "at": {"x": 100, "y": 100}, "button": "left",
                               "count": 1, "modifiers": "shift"}),
            serde_json::json!({"kind": "click", "at": 0, "button": "left",
                               "count": 1, "modifiers": []}),
            serde_json::json!({"kind": "scroll_ticks", "at": {"x": 100, "y": 100}, "direction": "diagonal",
                               "amount": 3, "modifiers": []}),
            serde_json::json!({"kind": "key", "keys": "a", "repeat": 1}),
            serde_json::json!({"kind": "key", "keys": ["a"], "repeat": 101}),
            serde_json::json!({"kind": "wait", "milliseconds": -5}),
            serde_json::json!({"kind": "type", "text": 123}),
            serde_json::json!({"kind": "hold_key", "keys": ["shift"], "milliseconds": 99999999}),
            serde_json::json!({"kind": "drag", "path": {"x": 1}, "modifiers": []}),
        ];
        let long_path: Vec<serde_json::Value> = (0..4097).map(|i| serde_json::json!({"x": i % 1280, "y": 0})).collect();
        malformed.push(serde_json::json!({"kind": "drag", "path": long_path, "modifiers": []}));
        let probes = malformed
            .into_iter()
            .map(|frame| tokio::spawn(async move { raw_exchange(vec![frame]).await }));
        for probe in probes {
            let (kinds, closed) = probe.await.unwrap();
            assert_eq!(kinds, ["failed"]);
            assert!(closed);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn browser_container_stop_during_dispatch_removes_the_target() {
    if !require_docker().await {
        return;
    }
    tokio::time::timeout(FLOW_TIMEOUT, async {
        let (handle, mut driver) = launch_for_test().await;
        let token = tokio_util::sync::CancellationToken::new();
        let held = tokio::spawn(async move {
            let action = Action::HoldKey {
                keys: vec!["shift".into()],
                duration: Duration::from_secs(120),
            };
            driver.execute(&action, &token).await
        });
        sleep(Duration::from_secs(2)).await;
        assert_eq!(handle.terminate().await.unwrap(), TerminateOutcome::Removed);
        assert_eq!(container_running(handle.id()).await, None);
        assert!(held.await.unwrap().is_err());
    })
    .await
    .unwrap();
}
