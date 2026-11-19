use std::future::pending;

use super::*;

#[derive(Default)]
struct FakeDriver {
    delivered: Vec<String>,
    screenshot_size: Option<usize>,
}

impl Driver for FakeDriver {
    async fn execute(&mut self, action: &Action, revoked: &CancellationToken) -> Result<Observation> {
        match action {
            Action::Type(text) => self.delivered.push(text.clone()),
            Action::Wait(_) => pending::<()>().await,
            Action::Screenshot if let Some(size) = self.screenshot_size => {
                let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
                bytes.resize(size, 42);
                return Ok(Observation::Screenshot(Screenshot::png(bytes)?));
            }
            Action::CursorPosition => {
                revoked.cancel();
                return Ok(Observation::CursorPosition(Point { x: 13, y: 7 }));
            }
            _ => bail!("fake driver rejected private input"),
        }
        Ok(Observation::Done)
    }
}

const DEADLINE: Duration = Duration::from_secs(2);

#[tokio::test]
async fn computer_control_orders_actions_and_never_replays_a_call() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [Action::Type("first".into()), Action::Type("second".into())];
    assert_eq!(control.execute_batch("call_a", &batch, DEADLINE).await.unwrap().len(), 2);
    assert!(control.execute_batch("call_a", &batch, DEADLINE).await.is_err());
    assert_eq!(control.driver.delivered, ["first", "second"]);
}

#[tokio::test]
async fn computer_control_stops_after_partial_failure() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [
        Action::Type("delivered".into()),
        Action::Screenshot,
        Action::Type("skipped".into()),
    ];
    let Err(error) = control.execute_batch("call_a", &batch, DEADLINE).await else {
        panic!("unexpected success");
    };
    assert_eq!(error.to_string(), "computer driver action failed");
    assert!(control.execute_batch("call_b", &batch, DEADLINE).await.is_err());
    assert_eq!(control.driver.delivered, ["delivered"]);
}

#[tokio::test]
async fn computer_control_rejects_further_input_after_cancellation() {
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(FakeDriver::default(), &turn);
    let batch = [
        Action::Type("delivered".into()),
        Action::Wait(DEADLINE),
        Action::Type("skipped".into()),
    ];
    let mut execution = std::pin::pin!(control.execute_batch("call_a", &batch, DEADLINE));
    assert!(futures::poll!(&mut execution).is_pending());
    turn.cancel();
    assert!(execution.await.is_err());

    let replacement_turn = CancellationToken::new();
    let mut replacement = TurnControl::after_approval(FakeDriver::default(), &replacement_turn);
    let new_batch = [Action::Type("replacement".into())];
    assert!(replacement.execute_batch("call_a", &new_batch, DEADLINE).await.is_ok());
    assert_eq!(replacement.driver.delivered, ["replacement"]);
}

#[tokio::test]
async fn computer_control_cannot_reuse_a_lease_after_dispatch_is_dropped() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [Action::Type("once".into()), Action::Wait(DEADLINE)];
    {
        let mut execution = std::pin::pin!(control.execute_batch("call_a", &batch, DEADLINE));
        assert!(futures::poll!(&mut execution).is_pending());
    }
    assert!(control.execute_batch("call_a", &batch, DEADLINE).await.is_err());
    assert_eq!(control.driver.delivered, ["once"]);
}

#[tokio::test(start_paused = true)]
async fn computer_control_cannot_reuse_a_lease_after_timeout() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [Action::Type("once".into()), Action::Wait(DEADLINE)];
    assert!(control.execute_batch("call_a", &batch, DEADLINE).await.is_err());
    assert!(control.execute_batch("call_b", &batch, DEADLINE).await.is_err());
    assert_eq!(control.driver.delivered, ["once"]);
}

#[tokio::test]
async fn computer_control_discards_a_reply_returned_after_revocation() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [Action::CursorPosition, Action::Type("skipped".into())];
    assert!(control.execute_batch("call_a", &batch, DEADLINE).await.is_err());
    assert!(control.driver.delivered.is_empty());
}

#[tokio::test]
async fn computer_control_rejects_malformed_batches_before_dispatch() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let actions: Vec<_> = (0..=MAX_BATCH_ACTIONS).map(|_| Action::Type("skipped".into())).collect();
    assert!(control.execute_batch("call_a", &actions, DEADLINE).await.is_err());
    assert!(control.execute_batch("", &[Action::Screenshot], DEADLINE).await.is_err());
    assert!(control.driver.delivered.is_empty());

    let actions = [Action::Type("valid".into())];
    assert!(control.execute_batch("call_a", &actions, DEADLINE).await.is_ok());
    assert_eq!(control.driver.delivered, ["valid"]);
}

#[tokio::test]
async fn computer_control_does_not_dispatch_after_deadline() {
    let mut control = TurnControl::after_approval(FakeDriver::default(), &CancellationToken::new());
    let batch = [Action::Type("must not be delivered".into())];
    assert!(control.execute_batch("call_a", &batch, Duration::ZERO).await.is_err());
    assert!(control.driver.delivered.is_empty());
}

#[tokio::test]
async fn computer_control_bounds_screenshots_across_the_batch() {
    let driver = FakeDriver {
        screenshot_size: Some(MAX_SCREENSHOT_BYTES / 2 + 1),
        ..FakeDriver::default()
    };
    let mut control = TurnControl::after_approval(driver, &CancellationToken::new());
    let observations = control.execute_batch("call_a", &[Action::Screenshot], DEADLINE).await.unwrap();
    let [Observation::Screenshot(image)] = observations.as_slice() else {
        panic!("expected one screenshot");
    };
    assert_eq!(image.as_png().len(), MAX_SCREENSHOT_BYTES / 2 + 1);
    drop(observations);

    let batch = [Action::Screenshot, Action::Screenshot, Action::Type("skipped".into())];
    let Err(error) = control.execute_batch("call_b", &batch, DEADLINE).await else {
        panic!("unexpected success");
    };
    assert_eq!(error.to_string(), "computer action batch exceeded its screenshot budget");
    assert!(control.driver.delivered.is_empty());
}
