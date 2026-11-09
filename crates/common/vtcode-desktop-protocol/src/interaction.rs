//! Runtime interaction request/response DTOs.

use serde::{Deserialize, Serialize};

/// Host hotkey signalled back to the service mid-interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionHotkey {
    /// Open the pending plan draft in the service-configured external editor.
    LaunchEditor,
}

/// User response to a pending runtime interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity provided by the service.
    pub interaction_id: String,
    /// Opaque choice identity provided by the service, if selecting an option.
    pub choice_id: Option<String>,
    /// Opaque identities selected for each step in a wizard interaction.
    #[serde(default)]
    pub choice_ids: Vec<String>,
    /// Free-form answer for text questions.
    pub text: Option<String>,
    /// Free-form answers for each ordered wizard step.
    #[serde(default)]
    pub answer_texts: Vec<Option<String>>,
    /// Whether the interaction was explicitly cancelled by the user.
    #[serde(default)]
    pub cancelled: bool,
    /// Host hotkey signal for a mid-interaction action. `None` for ordinary
    /// choice/cancel responses; `Some` asks the service to act mid-interaction
    /// (e.g. open the plan editor) and follow up with a new request.
    #[serde(default)]
    pub hotkey: Option<InteractionHotkey>,
}

/// Interaction request sent from the service to the desktop client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity.
    pub interaction_id: String,
    /// Kind of interaction required.
    pub kind: InteractionKind,
    /// Whether the runtime expects an ordered wizard response.
    #[serde(default)]
    pub wizard: bool,
    /// Dialog title or question prompt.
    pub title: String,
    /// Supporting text shown by the client.
    pub details: Vec<String>,
    /// Available choices, preserving each runtime's opaque identity.
    pub choices: Vec<InteractionChoice>,
    /// Ordered multi-step questions when the runtime request is a wizard.
    #[serde(default)]
    pub steps: Vec<InteractionStep>,
    /// Optional selected choice identity.
    pub selected_choice_id: Option<String>,
    /// Optional selected choice identities, one per wizard step.
    #[serde(default)]
    pub selected_choice_ids: Vec<String>,
    /// Full plan markdown for `PlanApproval` prompts, normalized the same
    /// way as the transcript copy. `None` for other kinds and for payloads
    /// from older services; clients hide plan-expansion UI rather than
    /// guessing.
    #[serde(default)]
    pub plan_markdown: Option<String>,
}

/// One ordered step in a question or approval wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionStep {
    /// Step label.
    pub title: String,
    /// Question or instruction for this step.
    pub question: String,
    /// Choices preserving exact service-issued identities.
    pub choices: Vec<InteractionChoice>,
    /// Optional selected identity.
    pub selected_choice_id: Option<String>,
    /// Whether the client may submit a free-form answer.
    pub allow_freeform: bool,
}

/// Interaction category understood by native clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    /// A tool or policy approval decision.
    Approval,
    /// Plan approval decision.
    PlanApproval,
    /// User-facing question requiring a response.
    Question,
    /// Non-security choice picker.
    Choice,
    /// Unsupported or informational transient request.
    Other,
}

/// One display label paired with a stable opaque identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionChoice {
    /// Opaque identity returned unchanged by the client.
    pub id: String,
    /// User-facing label.
    pub label: String,
    /// Optional supporting description.
    pub description: Option<String>,
    /// Whether the service recommends this option.
    pub recommended: bool,
}
