//! Responses wire codec for the batched `computer` tool.

use std::time::Duration;

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use compact_str::CompactString;
use serde::Deserialize;
use serde_json::{Value, json};
use vtcode_computer_use::{
    Action, Driver, MAX_BATCH_ACTIONS, MouseButton, Observation, Point, Screenshot, TurnControl,
};

use crate::provider::{NativeCall, NativeCallBody};

/// Native protocol version declared on the wire and checked by the gate.
/// Re-exported from the owning gate table so declaration and admission cannot drift.
pub(crate) use vtcode_computer_use::gate::OPENAI_COMPUTER_PROTOCOL as PROTOCOL;

pub fn declaration() -> Value {
    json!({"type": PROTOCOL})
}

/// Carries the original item for turn-local stateless replay. This type
/// deliberately has no serialization or debug implementation.
pub struct ComputerCall<'a> {
    id: &'a str,
    item: &'a Value,
    pub actions: Vec<Action>,
}

impl ComputerCall<'_> {
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn replay_item(&self) -> &Value {
        self.item
    }

    pub fn result(&self, screenshot: &Screenshot) -> Value {
        json!({
            "type": "computer_call_output",
            "call_id": self.id,
            "output": {
                "type": "computer_screenshot",
                "image_url": format!("data:image/png;base64,{}", STANDARD.encode(screenshot.as_png())),
                "detail": "original",
            }
        })
    }
}

/// Execute one bound-route call decoded by `generate_native`. The replay
/// item and actions ride along from the decoded body; anything else bails
/// before the driver runs.
pub async fn execute_decoded_call<D: Driver>(
    control: &mut TurnControl<D>,
    call: NativeCall,
    timeout: Duration,
) -> Result<Value> {
    let NativeCallBody::OpenAi { replay_item, actions } = call.body else {
        bail!("mixed-provider computer batch");
    };
    let decoded = ComputerCall { id: &call.id, item: &replay_item, actions };
    execute_call(control, &decoded, timeout).await
}

/// Execute an admitted Responses call and prepare its private wire result.
/// An ambiguous or incomplete batch ends the target lease.
pub async fn execute_call<D: Driver>(
    control: &mut TurnControl<D>,
    call: &ComputerCall<'_>,
    timeout: Duration,
) -> Result<Value> {
    let observations = match control.execute_batch(call.id(), &call.actions, timeout).await {
        Ok(observations) => observations,
        Err(error) => {
            control.revoke_target();
            return Err(error);
        }
    };
    control.ensure_turn_active()?;
    let Some(Observation::Screenshot(image)) = observations.last() else {
        control.revoke_target();
        bail!("OpenAI computer batch did not return its final screenshot");
    };
    let result = call.result(image);
    control.ensure_turn_active()?;
    Ok(result)
}

/// Decode a completed output item and ensure the batch ends with a screenshot.
/// Calls requiring a safety acknowledgement remain blocked until a dedicated
/// approval flow can verify and acknowledge the individual check IDs.
pub fn decode_call(item: &Value) -> Result<Option<ComputerCall<'_>>> {
    if item["type"].as_str() != Some("computer_call") {
        return Ok(None);
    }
    ensure!(item["status"].as_str() == Some("completed"), "incomplete OpenAI computer call");
    let id = item["call_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(|| anyhow::anyhow!("invalid OpenAI computer call identity"))?;
    ensure!(
        item.get("pending_safety_checks")
            .is_none_or(|checks| checks.as_array().is_some_and(Vec::is_empty)),
        "OpenAI computer call requires a safety acknowledgement"
    );
    let wire_actions = item["actions"]
        .as_array()
        .filter(|actions| (1..=MAX_BATCH_ACTIONS).contains(&actions.len()))
        .ok_or_else(|| anyhow::anyhow!("invalid OpenAI computer action batch"))?;
    let mut actions = wire_actions
        .iter()
        .map(|action| {
            WireAction::deserialize(action)
                .map_err(|_private_error| anyhow::anyhow!("invalid OpenAI computer action"))?
                .into_action()
        })
        .collect::<Result<Vec<_>>>()?;
    if !matches!(actions.last(), Some(Action::Screenshot)) {
        ensure!(actions.len() < MAX_BATCH_ACTIONS, "no room for the final computer screenshot");
        actions.push(Action::Screenshot);
    }
    Ok(Some(ComputerCall { id, item, actions }))
}

#[derive(Deserialize)]
struct WirePoint {
    x: u32,
    y: u32,
}

impl From<WirePoint> for Point {
    fn from(WirePoint { x, y }: WirePoint) -> Self {
        Self { x, y }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireButton {
    Left,
    Right,
    Wheel,
    Back,
    Forward,
}

impl From<WireButton> for MouseButton {
    fn from(button: WireButton) -> Self {
        match button {
            WireButton::Left => Self::Left,
            WireButton::Right => Self::Right,
            WireButton::Wheel => Self::Middle,
            WireButton::Back => Self::Back,
            WireButton::Forward => Self::Forward,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireAction {
    Click {
        x: u32,
        y: u32,
        button: WireButton,
        keys: Option<Vec<CompactString>>,
    },
    DoubleClick {
        x: u32,
        y: u32,
        keys: Option<Vec<CompactString>>,
    },
    Drag {
        path: Vec<WirePoint>,
        keys: Option<Vec<CompactString>>,
    },
    Move {
        x: u32,
        y: u32,
        keys: Option<Vec<CompactString>>,
    },
    Scroll {
        x: u32,
        y: u32,
        scroll_x: i32,
        scroll_y: i32,
        keys: Option<Vec<CompactString>>,
    },
    Keypress {
        keys: Vec<CompactString>,
    },
    Type {
        text: String,
    },
    Wait,
    Screenshot,
}

impl WireAction {
    fn into_action(self) -> Result<Action> {
        Ok(match self {
            Self::Click { x, y, button, keys } => Action::Click {
                at: Some(Point { x, y }),
                button: button.into(),
                count: 1,
                modifiers: keys.unwrap_or_default(),
            },
            Self::DoubleClick { x, y, keys } => Action::Click {
                at: Some(Point { x, y }),
                button: MouseButton::Left,
                count: 2,
                modifiers: keys.unwrap_or_default(),
            },
            Self::Drag { path, keys } => {
                ensure!((2..=1024).contains(&path.len()), "invalid OpenAI computer drag path");
                Action::Drag {
                    path: path.into_iter().map(Point::from).collect(),
                    modifiers: keys.unwrap_or_default(),
                }
            }
            Self::Move { x, y, keys } => Action::Move {
                at: Point { x, y },
                modifiers: keys.unwrap_or_default(),
            },
            Self::Scroll { x, y, scroll_x, scroll_y, keys } => Action::ScrollPixels {
                at: Point { x, y },
                x: scroll_x,
                y: scroll_y,
                modifiers: keys.unwrap_or_default(),
            },
            Self::Keypress { keys } => {
                ensure!(!keys.is_empty(), "empty OpenAI computer keypress");
                Action::Key { keys, repeat: 1 }
            }
            Self::Type { text } => Action::Type(text),
            Self::Wait => Action::Wait(Duration::from_secs(1)),
            Self::Screenshot => Action::Screenshot,
        })
    }
}

#[cfg(test)]
mod tests;
