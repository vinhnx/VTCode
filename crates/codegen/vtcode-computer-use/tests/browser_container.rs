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
//! exact teardown. They skip loudly when Docker is unavailable and fail with
//! a build hint when the image is missing.

use std::path::PathBuf;
use std::time::Duration;

use tokio::time::sleep;
use vtcode_computer_use::browser::BrowserDriver;
use vtcode_computer_use::browser::container::{ContainerHandle, ContainerSpec, docker_available};
use vtcode_computer_use::{Action, Driver, MouseButton, Observation, Point};

const IMAGE: &str = "vtcode-computer-use-browser:1.64.1";
const FLOW_TIMEOUT: Duration = Duration::from_secs(240);
const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];

fn fixture_app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/browser-app")
}

async fn require_docker() -> bool {
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
        handle.terminate().await.unwrap();
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

        handle.terminate().await.unwrap();
        assert_eq!(container_running(handle.id()).await, None);
        handle.terminate().await.unwrap();
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
        handle.terminate().await.unwrap();
        assert_eq!(container_running(handle.id()).await, None);
        assert!(held.await.unwrap().is_err());
    })
    .await
    .unwrap();
}
