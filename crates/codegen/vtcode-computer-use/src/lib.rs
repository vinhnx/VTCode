//! Turn-local computer input and observation. Payloads are deliberately not serializable.

use std::{collections::HashSet, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use compact_str::CompactString;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

pub mod browser;

/// Maximum number of actions admitted per batch.
pub const MAX_BATCH_ACTIONS: usize = 128;
/// Maximum screenshot bytes accepted per action batch.
pub const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A position in the target screenshot’s pixel coordinates.
pub struct Point {
    /// Horizontal distance from the left edge.
    pub x: u32,
    /// Vertical distance from the top edge.
    pub y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A mouse button identified independently of the driver.
pub enum MouseButton {
    /// Left mouse button.
    Left,
    /// Middle mouse button.
    Middle,
    /// Right mouse button.
    Right,
    /// Back mouse button.
    Back,
    /// Forward mouse button.
    Forward,
}

/// A provider-neutral request to observe or interact with the target.
pub enum Action {
    /// Capture the target viewport.
    Screenshot,
    /// Capture a rectangle within the viewport.
    Zoom {
        /// Top-left corner of the rectangle.
        top_left: Point,
        /// Bottom-right corner of the rectangle.
        bottom_right: Point,
    },
    /// Deliver one or more clicks.
    Click {
        /// Position for the action, or the current pointer position.
        at: Option<Point>,
        /// Button to click.
        button: MouseButton,
        /// Number of clicks.
        count: u8,
        /// Modifier keys held during the action.
        modifiers: Vec<CompactString>,
    },
    /// Deliver a drag along an ordered path.
    Drag {
        /// Path from the start point to the release point.
        path: Vec<Point>,
        /// Modifier keys held during the action.
        modifiers: Vec<CompactString>,
    },
    /// Move the pointer to the given position.
    Move {
        /// Destination in screenshot pixels.
        at: Point,
        /// Modifier keys held during pointer movement.
        modifiers: Vec<CompactString>,
    },
    /// Scroll by a signed pixel offset.
    ScrollPixels {
        /// Position at which to scroll.
        at: Point,
        /// Horizontal scroll offset.
        x: i32,
        /// Vertical scroll offset.
        y: i32,
        /// Modifier keys held during the action.
        modifiers: Vec<CompactString>,
    },
    /// Scroll by wheel ticks.
    ScrollTicks {
        /// Position for the action, or the current pointer position.
        at: Option<Point>,
        /// Direction of movement.
        direction: ScrollDirection,
        /// Number of wheel ticks.
        amount: u32,
        /// Modifier keys held during the action.
        modifiers: Vec<CompactString>,
    },
    /// Press a key or chord.
    Key {
        /// Key names in press order.
        keys: Vec<CompactString>,
        /// Number of presses.
        repeat: u8,
    },
    /// Hold a key or chord before releasing it.
    HoldKey {
        /// Key names in press order.
        keys: Vec<CompactString>,
        /// Time to hold the keys.
        duration: Duration,
    },
    /// Press the left mouse button at the current position.
    LeftMouseDown,
    /// Release the left mouse button.
    LeftMouseUp,
    /// Query the current pointer position.
    CursorPosition,
    /// Insert literal text.
    Type(String),
    /// Wait for a duration.
    Wait(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Direction of a wheel scroll.
pub enum ScrollDirection {
    /// Scroll upward.
    Up,
    /// Scroll downward.
    Down,
    /// Scroll left.
    Left,
    /// Scroll right.
    Right,
}

/// An in-memory screenshot bounded by the controller’s payload limit.
pub struct Screenshot(Vec<u8>);

impl Screenshot {
    /// Accepts a bounded PNG payload from the driver.
    ///
    /// # Errors
    ///
    /// Returns an error if the signature is absent or the byte limit is exceeded.
    pub fn png(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_SCREENSHOT_BYTES && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "invalid computer screenshot payload"
        );
        Ok(Self(bytes))
    }

    /// Borrow the bytes for provider encoding within the current turn.
    pub fn as_png(&self) -> &[u8] {
        &self.0
    }
}

/// The result of a single driver action.
pub enum Observation {
    /// An action completed without an informational result.
    Done,
    /// A screenshot returned by the driver.
    Screenshot(Screenshot),
    /// The current pointer position.
    CursorPosition(Point),
}

/// An instance is bound to one target. The driver owns delivery checks and cleanup
/// even when an action future is dropped while input is in flight.
pub trait Driver {
    /// Dispatch one action, observing revocation before and during target delivery.
    fn execute(
        &mut self,
        action: &Action,
        revoked: &CancellationToken,
    ) -> impl Future<Output = Result<Observation>> + Send;
}

/// A unique driver lease for an approved target in one turn. The caller supplies
/// that turn's cancellation token after approval and keeps the lease off durable history.
pub struct TurnControl<D> {
    driver: D,
    turn: CancellationToken,
    revoked: CancellationToken,
    seen_calls: HashSet<CompactString>,
}

impl<D: Driver> TurnControl<D> {
    /// Create the controller after the run loop grants per-turn target approval.
    pub fn after_approval(driver: D, turn: &CancellationToken) -> Self {
        Self {
            driver,
            turn: turn.clone(),
            revoked: turn.child_token(),
            seen_calls: HashSet::new(),
        }
    }

    /// Check the parent turn before returning private results to the provider.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent turn has ended.
    pub fn ensure_turn_active(&self) -> Result<()> {
        ensure!(!self.turn.is_cancelled(), "computer turn has ended");
        Ok(())
    }

    /// End this target lease without cancelling unrelated work in the turn.
    pub fn revoke_target(&self) {
        self.revoked.cancel();
    }

    /// Execute an ordered batch once and return observations in the same order.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid or repeated calls, an expired deadline,
    /// revocation, driver failure, or an exceeded screenshot budget.
    pub async fn execute_batch(
        &mut self,
        call_id: &str,
        actions: &[Action],
        timeout: Duration,
    ) -> Result<Vec<Observation>> {
        ensure!(!self.revoked.is_cancelled(), "computer control has ended");
        ensure!(!call_id.is_empty() && call_id.len() <= 256, "invalid computer call identity");
        ensure!((1..=MAX_BATCH_ACTIONS).contains(&actions.len()), "invalid computer action batch size");
        ensure!(self.seen_calls.insert(call_id.into()), "duplicate computer call");

        // Dropping this future also revokes the lease because delivery may have
        // already happened.
        let revoke_on_failure = self.revoked.clone().drop_guard();
        let deadline = Instant::now()
            .checked_add(timeout)
            .context("invalid computer action batch timeout")?;
        let mut timer = std::pin::pin!(sleep_until(deadline));
        let mut observations = Vec::with_capacity(actions.len());
        let mut screenshot_bytes = 0;
        for action in actions {
            ensure!(Instant::now() < deadline, "computer action batch timed out");
            let observation = tokio::select! {
                biased;
                _ = self.revoked.cancelled() => bail!("computer control was revoked"),
                _ = &mut timer => bail!("computer action batch timed out"),
                result = self.driver.execute(action, &self.revoked) => {
                    // Driver diagnostics can contain action payloads.
                    result.map_err(|_private_error| anyhow::anyhow!("computer driver action failed"))?
                }
            };
            ensure!(!self.revoked.is_cancelled(), "computer control was revoked");
            ensure!(Instant::now() < deadline, "computer action batch timed out");
            if let Observation::Screenshot(screenshot) = &observation {
                screenshot_bytes += screenshot.as_png().len();
                ensure!(
                    screenshot_bytes <= MAX_SCREENSHOT_BYTES,
                    "computer action batch exceeded its screenshot budget"
                );
            }
            observations.push(observation);
        }
        drop(revoke_on_failure.disarm());
        Ok(observations)
    }
}

impl<D> Drop for TurnControl<D> {
    fn drop(&mut self) {
        self.revoked.cancel();
    }
}

#[cfg(test)]
mod tests;
