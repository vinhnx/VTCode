//! Messages wire codec for the versioned computer client toolset.

use std::{collections::HashSet, time::Duration};

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use compact_str::CompactString;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use vtcode_computer_use::{
    Action, Driver, MAX_BATCH_ACTIONS, MAX_SCREENSHOT_BYTES, MouseButton, Observation, Point, Screenshot,
    ScrollDirection, TurnControl,
};

use crate::provider::{NativeCall, NativeCallBody};

pub(super) const TOOLSET: &str = "computer";
/// Native protocol version declared on the wire and checked by the gate.
/// Re-exported from the owning gate table so declaration and admission cannot drift.
pub(crate) use vtcode_computer_use::gate::ANTHROPIC_COMPUTER_PROTOCOL as PROTOCOL;
const NOT_EXECUTED: &str = "Not executed: an earlier computer action in this turn failed.";

pub(super) fn unbound_route_error() -> crate::provider::LLMError {
    crate::provider::LLMError::Provider {
        message: "Anthropic computer toolset call requires an active computer-use route.".into(),
        metadata: None,
    }
}

pub fn declaration() -> Value {
    json!({"type": PROTOCOL})
}

/// An ephemeral native call. A malformed input retains its correlation ID so
/// the runtime can return an error for that block and halt the batch.
pub struct ComputerCall<'a> {
    id: &'a str,
    pub action: Result<Action>,
}

impl ComputerCall<'_> {
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn result(&self, outcome: ComputerOutcome<'_>) -> Value {
        computer_result(self.id, outcome)
    }
}

fn computer_result(id: &str, outcome: ComputerOutcome<'_>) -> Value {
    let (is_error, content) = match outcome {
        ComputerOutcome::Executed(observation) => match observation {
            Observation::Done => (false, json!("OK")),
            Observation::CursorPosition(Point { x, y }) => (false, json!(format!("X={x}, Y={y}"))),
            Observation::Screenshot(image) => (
                false,
                json!([{"type": "image", "source": {
                    "type": "base64", "media_type": "image/png", "data": STANDARD.encode(image.as_png())
                }}]),
            ),
        },
        ComputerOutcome::Failed => (true, json!("Computer action failed.")),
        ComputerOutcome::Skipped => (true, json!(NOT_EXECUTED)),
    };
    json!({
        "type": "tool_result",
        "tool_use_id": id,
        "toolset_name": TOOLSET,
        "is_error": is_error,
        "content": content,
    })
}

/// Recover the validated screenshot from a wire result this module built.
///
/// Returns `None` for non-screenshot outcomes and for any malformed payload
/// (wrong block shape, bad base64, non-PNG bytes, over the byte cap): the
/// transcript preview is best-effort and fails closed, never trusted input.
/// Lives beside the builder so a wire-shape change breaks this in review.
pub fn screenshot_in_output(output: &Value) -> Option<Screenshot> {
    let blocks = output.get("content")?.as_array()?;
    let data = blocks.iter().find_map(|block| {
        if block.get("type")?.as_str()? != "image" {
            return None;
        }
        let source = block.get("source")?;
        if source.get("type")?.as_str()? != "base64" {
            return None;
        }
        if source.get("media_type")?.as_str()? != "image/png" {
            return None;
        }
        source.get("data")?.as_str()
    })?;
    let bytes = STANDARD.decode(data).ok()?;
    Screenshot::png(bytes).ok()
}

pub enum ComputerOutcome<'a> {
    Executed(&'a Observation),
    Failed,
    Skipped,
}

/// Recognizes only toolset member calls. Ordinary tools can have the same name.
pub fn decode_call(block: &Value) -> Result<Option<ComputerCall<'_>>> {
    if block["type"].as_str() != Some("tool_use") || block["toolset_name"].as_str() != Some(TOOLSET) {
        return Ok(None);
    }
    let Some(id) = block["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 256) else {
        bail!("invalid Anthropic computer call identity");
    };
    Ok(Some(ComputerCall {
        id,
        action: decode_action(block["name"].as_str().unwrap_or_default(), &block["input"]),
    }))
}

/// Admitted call with owned identity, ready for the fail-open loop.
struct PreparedCall {
    index: usize,
    id: String,
    action: Result<Action>,
}

/// Return private result blocks with their original output positions. The
/// caller can merge ordinary client-tool results at the intervening positions.
pub async fn execute_batch<D: Driver>(
    control: &mut TurnControl<D>,
    blocks: &[Value],
    timeout: Duration,
) -> Result<Vec<(usize, Value)>> {
    let mut prepared = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if let Some(call) = decode_call(block)? {
            prepared.push(PreparedCall {
                index,
                id: call.id().to_string(),
                action: call.action,
            });
        }
    }
    let result = execute_calls(control, prepared, timeout).await;
    if result.is_err() {
        control.revoke_target();
    }
    result
}

/// Execute bound-route calls decoded by `generate_native`. Positions ride
/// along from [`NativeCall::index`] so results merge back with ordinaries.
pub async fn execute_decoded<D: Driver>(
    control: &mut TurnControl<D>,
    calls: Vec<NativeCall>,
    timeout: Duration,
) -> Result<Vec<(usize, Value)>> {
    let mut prepared = Vec::with_capacity(calls.len());
    for call in calls {
        let NativeCallBody::Anthropic { action, .. } = call.body else {
            bail!("mixed-provider computer batch");
        };
        prepared.push(PreparedCall {
            index: call.index,
            id: call.id,
            action: action.map_err(|message| anyhow::anyhow!(message)),
        });
    }
    let result = execute_calls(control, prepared, timeout).await;
    if result.is_err() {
        control.revoke_target();
    }
    result
}

async fn execute_calls<D: Driver>(
    control: &mut TurnControl<D>,
    prepared: Vec<PreparedCall>,
    timeout: Duration,
) -> Result<Vec<(usize, Value)>> {
    control.ensure_turn_active()?;
    let mut identities = HashSet::new();
    for call in &prepared {
        ensure!(identities.insert(call.id.as_str()), "duplicate Anthropic computer call identity");
    }
    ensure!(prepared.len() <= MAX_BATCH_ACTIONS, "too many Anthropic computer actions");

    let start = tokio::time::Instant::now();
    let mut failed = false;
    let mut screenshot_bytes = 0;
    let mut results = Vec::with_capacity(prepared.len());
    for call in &prepared {
        let observations = match &call.action {
            Ok(action) if !failed => control
                .execute_batch(&call.id, std::slice::from_ref(action), timeout.saturating_sub(start.elapsed()))
                .await
                .ok(),
            _ => None,
        };
        control.ensure_turn_active()?;
        let outcome = match observations.as_deref() {
            Some([observation]) if observation_matches(&call.action, observation, &mut screenshot_bytes) => {
                ComputerOutcome::Executed(observation)
            }
            _ if failed => ComputerOutcome::Skipped,
            _ => {
                control.revoke_target();
                failed = true;
                ComputerOutcome::Failed
            }
        };
        results.push((call.index, computer_result(&call.id, outcome)));
    }
    control.ensure_turn_active()?;
    Ok(results)
}

fn observation_matches(action: &Result<Action>, observation: &Observation, screenshot_bytes: &mut usize) -> bool {
    match (action, observation) {
        (Ok(Action::Screenshot | Action::Zoom { .. }), Observation::Screenshot(image)) => {
            *screenshot_bytes += image.as_png().len();
            *screenshot_bytes <= MAX_SCREENSHOT_BYTES
        }
        (Ok(Action::CursorPosition), Observation::CursorPosition(_)) => true,
        (Ok(Action::Screenshot | Action::Zoom { .. } | Action::CursorPosition), _) => false,
        (Ok(_), Observation::Done) => true,
        _ => false,
    }
}

fn decode_action(name: &str, input: &Value) -> Result<Action> {
    ensure!(input.is_object(), "invalid Anthropic computer action input");
    Ok(match name {
        "screenshot" => Action::Screenshot,
        "cursor_position" => Action::CursorPosition,
        "left_mouse_down" => Action::LeftMouseDown,
        "left_mouse_up" => Action::LeftMouseUp,
        "zoom" => {
            let [x0, y0, x1, y1] = field(input, "region")?;
            ensure!(x0 < x1 && y0 < y1, "invalid computer zoom region");
            Action::Zoom {
                top_left: Point { x: x0, y: y0 },
                bottom_right: Point { x: x1, y: y1 },
            }
        }
        "left_click" | "middle_click" | "right_click" | "double_click" | "triple_click" => {
            let (button, count) = match name {
                "middle_click" => (MouseButton::Middle, 1),
                "right_click" => (MouseButton::Right, 1),
                "double_click" => (MouseButton::Left, 2),
                "triple_click" => (MouseButton::Left, 3),
                _ => (MouseButton::Left, 1),
            };
            Action::Click {
                at: optional_point(input)?,
                button,
                count,
                modifiers: modifiers(input)?,
            }
        }
        "left_click_drag" => Action::Drag {
            path: vec![
                point(field(input, "start_coordinate")?),
                point(field(input, "coordinate")?),
            ],
            modifiers: modifiers(input)?,
        },
        "mouse_move" => Action::Move {
            at: point(field(input, "coordinate")?),
            modifiers: Vec::new(),
        },
        "scroll" => {
            let amount: u32 = field(input, "scroll_amount")?;
            ensure!(amount <= 100_000, "invalid computer scroll amount");
            Action::ScrollTicks {
                at: optional_point(input)?,
                direction: match input["scroll_direction"].as_str() {
                    Some("up") => ScrollDirection::Up,
                    Some("down") => ScrollDirection::Down,
                    Some("left") => ScrollDirection::Left,
                    Some("right") => ScrollDirection::Right,
                    _ => bail!("invalid computer scroll direction"),
                },
                amount,
                modifiers: modifiers(input)?,
            }
        }
        "type" => Action::Type(field(input, "text")?),
        "key" => {
            let repeat = optional_field(input, "repeat")?.unwrap_or(1);
            ensure!((1..=100).contains(&repeat), "invalid computer key repeat count");
            Action::Key {
                keys: chord(&field::<String>(input, "text")?)?,
                repeat,
            }
        }
        "hold_key" => Action::HoldKey {
            keys: chord(&field::<String>(input, "text")?)?,
            duration: duration(input)?,
        },
        "wait" => Action::Wait(duration(input)?),
        _ => bail!("unsupported Anthropic computer action"),
    })
}

fn point([x, y]: [u32; 2]) -> Point {
    Point { x, y }
}

fn optional_point(input: &Value) -> Result<Option<Point>> {
    Ok(optional_field(input, "coordinate")?.map(point))
}

fn chord(text: &str) -> Result<Vec<CompactString>> {
    ensure!(text.len() <= 128, "computer key chord is too long");
    text.split('+')
        .map(|key| {
            ensure!(!key.is_empty(), "invalid computer key chord");
            Ok(key.into())
        })
        .collect()
}

fn modifiers(input: &Value) -> Result<Vec<CompactString>> {
    let Some(text) = optional_field::<String>(input, "text")? else {
        return Ok(Vec::new());
    };
    let keys = chord(&text)?;
    ensure!(
        keys.iter()
            .all(|key| matches!(key.as_str(), "shift" | "ctrl" | "alt" | "super")),
        "invalid computer modifier"
    );
    Ok(keys)
}

fn duration(input: &Value) -> Result<Duration> {
    let seconds: f64 = field(input, "duration")?;
    ensure!((0.0..=300.0).contains(&seconds), "invalid computer action duration");
    Ok(Duration::from_secs_f64(seconds))
}

fn field<T: DeserializeOwned>(input: &Value, key: &str) -> Result<T> {
    T::deserialize(&input[key])
        .map_err(|_private_error| anyhow::anyhow!("invalid Anthropic computer action field {key}"))
}

fn optional_field<T: DeserializeOwned>(input: &Value, key: &str) -> Result<Option<T>> {
    input.get(key).map(|_| field(input, key)).transpose()
}

#[cfg(test)]
mod tests;
