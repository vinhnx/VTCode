//! Bounded private protocol for the isolated Chromium runner.

pub mod container;

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::{
    codec::{FramedRead, LinesCodec},
    sync::CancellationToken,
};

use crate::{Action, Driver, MAX_SCREENSHOT_BYTES, MouseButton, Observation, Point, Screenshot, ScrollDirection};

const MAX_ACTION_BYTES: usize = 1024 * 1024;
const MAX_REPLY_BYTES: usize = MAX_SCREENSHOT_BYTES.div_ceil(3) * 4 + 256;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// A browser driver over one runner's private pipes. The caller starts and
/// owns the approved isolated container, including forced teardown when a
/// cancelled action may still be in flight.
pub struct BrowserDriver<R, W> {
    replies: FramedRead<R, LinesCodec>,
    commands: W,
    usable: bool,
}

impl<R, W> BrowserDriver<R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    /// Attach to the pipes of a fresh, approved runner and await readiness.
    ///
    /// # Errors
    ///
    /// Returns an error if the runner exits, times out, or fails to open the target.
    pub async fn connect(replies: R, commands: W) -> Result<Self> {
        let mut driver = Self {
            replies: FramedRead::new(replies, LinesCodec::new_with_max_length(MAX_REPLY_BYTES)),
            commands,
            usable: true,
        };
        let reply = tokio::time::timeout(STARTUP_TIMEOUT, driver.read_reply())
            .await
            .context("browser runner startup timed out")??;
        ensure!(reply.get("kind").and_then(Value::as_str) == Some("ready"), "browser runner did not open the target");
        Ok(driver)
    }

    async fn read_reply(&mut self) -> Result<Value> {
        let line = self
            .replies
            .next()
            .await
            .context("browser runner closed its reply stream")?
            .map_err(|_private_error| anyhow::anyhow!("invalid browser runner reply frame"))?;
        serde_json::from_str(&line).map_err(|_private_error| anyhow::anyhow!("invalid browser runner reply"))
    }
}

impl<R, W> Driver for BrowserDriver<R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    async fn execute(&mut self, action: &Action, revoked: &CancellationToken) -> Result<Observation> {
        ensure!(self.usable, "browser runner exchange has ended");
        ensure!(!revoked.is_cancelled(), "browser control was revoked");
        let mut frame = serde_json::to_vec(&encode_action(action)?)?;
        ensure!(frame.len() <= MAX_ACTION_BYTES, "browser action exceeded its request budget");
        frame.push(b'\n');

        // A stopped exchange may leave an action or reply in flight. Keep this
        // pipe unusable even when the caller drops the future before it settles.
        self.usable = false;
        let observation = tokio::select! {
            biased;
            _ = revoked.cancelled() => bail!("browser control was revoked"),
            result = async {
                self.commands.write_all(&frame).await.context("could not write browser action")?;
                self.commands.flush().await.context("could not flush browser action")?;
                decode_observation(action, &self.read_reply().await?)
            } => result?,
        };
        ensure!(!revoked.is_cancelled(), "browser control was revoked");
        self.usable = true;
        Ok(observation)
    }
}

fn decode_observation(action: &Action, reply: &Value) -> Result<Observation> {
    match (action, reply.get("kind").and_then(Value::as_str)) {
        (Action::Screenshot | Action::Zoom { .. }, Some("screenshot")) => {
            let data = reply
                .get("data")
                .and_then(Value::as_str)
                .context("browser runner omitted screenshot data")?;
            let bytes = STANDARD.decode(data).context("invalid browser screenshot encoding")?;
            Ok(Observation::Screenshot(Screenshot::png(bytes)?))
        }
        (Action::CursorPosition, Some("cursor_position")) => {
            let coordinate = |field| {
                reply
                    .get(field)
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            };
            let x = coordinate("x").context("invalid browser cursor x")?;
            let y = coordinate("y").context("invalid browser cursor y")?;
            Ok(Observation::CursorPosition(Point { x, y }))
        }
        (_, Some("failed")) => bail!("browser runner refused the action"),
        (Action::Screenshot | Action::Zoom { .. } | Action::CursorPosition, _) => {
            bail!("browser runner returned an unexpected observation")
        }
        (_, Some("done")) => Ok(Observation::Done),
        _ => bail!("browser runner returned an unexpected observation"),
    }
}

fn point(Point { x, y }: Point) -> Value {
    json!({ "x": x, "y": y })
}

fn encode_action(action: &Action) -> Result<Value> {
    Ok(match action {
        Action::Screenshot => json!({"kind":"screenshot"}),
        Action::Zoom { top_left, bottom_right } => {
            json!({"kind":"zoom", "top_left":point(*top_left), "bottom_right":point(*bottom_right)})
        }
        Action::Click { at, button, count, modifiers } => {
            let button = match button {
                MouseButton::Left => "left",
                MouseButton::Middle => "middle",
                MouseButton::Right => "right",
                MouseButton::Back | MouseButton::Forward => bail!("browser does not deliver navigation mouse buttons"),
            };
            json!({"kind":"click", "at": at.map(point), "button":button, "count":count, "modifiers":modifiers})
        }
        Action::Drag { path, modifiers } => {
            json!({"kind":"drag", "path":path.iter().copied().map(point).collect::<Vec<_>>(), "modifiers":modifiers})
        }
        Action::Move { at, modifiers } => json!({"kind":"move", "at":point(*at), "modifiers":modifiers}),
        Action::ScrollPixels { at, x, y, modifiers } => {
            json!({"kind":"scroll_pixels", "at":point(*at), "x":x, "y":y, "modifiers":modifiers})
        }
        Action::ScrollTicks { at, direction, amount, modifiers } => {
            let direction = match direction {
                ScrollDirection::Up => "up",
                ScrollDirection::Down => "down",
                ScrollDirection::Left => "left",
                ScrollDirection::Right => "right",
            };
            json!({"kind":"scroll_ticks", "at":at.map(point), "direction":direction, "amount":amount, "modifiers":modifiers})
        }
        Action::Key { keys, repeat } => json!({"kind":"key", "keys":keys, "repeat":repeat}),
        Action::HoldKey { keys, duration } => {
            json!({"kind":"hold_key", "keys":keys, "milliseconds":bounded_milliseconds(*duration)?})
        }
        Action::LeftMouseDown => json!({"kind":"left_mouse_down"}),
        Action::LeftMouseUp => json!({"kind":"left_mouse_up"}),
        Action::CursorPosition => json!({"kind":"cursor_position"}),
        Action::Type(text) => json!({"kind":"type", "text":text}),
        Action::Wait(duration) => json!({"kind":"wait", "milliseconds":bounded_milliseconds(*duration)?}),
    })
}

fn bounded_milliseconds(duration: Duration) -> Result<u64> {
    ensure!(duration <= Duration::from_secs(300), "browser action duration exceeded its limit");
    u64::try_from(duration.as_millis()).context("invalid browser action duration")
}

#[cfg(test)]
mod tests;
