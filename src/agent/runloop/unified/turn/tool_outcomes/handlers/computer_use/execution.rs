//! Explicit computer-use turn dispatcher: generate, review, approve, execute.
//!
//! The dispatcher runs one self-contained computer turn through the bound
//! route and always tears the launched target down: cancellation and
//! timeouts funnel through the same teardown as completion and failure.
//! Approval arrives as injected logic so tests script the human decision;
//! production model-initiated turns approve through the permission flow
//! between proposal and execution.

#[cfg(test)]
use std::future::Future;
use std::time::Duration;

use anyhow::{Result, anyhow, ensure};
use serde_json::Value;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vtcode_computer_use::browser::BrowserDriver;
use vtcode_computer_use::browser::container::{ContainerHandle, ContainerSpec, TerminateOutcome, docker_available};
use vtcode_computer_use::{Driver, TurnControl};
use vtcode_config::ComputerUseConfig;
use vtcode_core::core::agent::events::tool_output_updated_event;
use vtcode_core::llm::provider::{
    LLMProvider, LLMRequest, NativeCall, NativeCallBody, NativeProvider, ToolCall, Usage,
};
use vtcode_core::llm::usage_cost;
use vtcode_llm::providers::anthropic::computer_use as anthropic_use;
use vtcode_llm::providers::openai::computer_use as openai_use;

use super::review::{BatchReviewVerdict, ComputerBatchSummary, REVIEW_TIMEOUT, review_computer_batch};
use super::{ApprovalRequest, ComputerUseTarget, action_kind};
use crate::agent::runloop::unified::auto_permission::wait_for_stop;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};

/// Stop signal and accounting for one computer turn. Progress events ride
/// the separate [`ComputerTurnProgress`] sink, which also carries the
/// outer row identity the updates address.
pub(crate) struct ComputerTurnRuntime<'a> {
    pub stop: &'a CtrlCState,
    pub notify: &'a Notify,
    pub stats: &'a mut SessionStats,
}

/// Redacted progress sink for one computer turn: stage updates ride the
/// existing output-item channel against the outer tool row, so transcripts
/// stream without new event types. The `computer_use` wrapper owns
/// invocation start/completion; the dispatcher only ever emits progress.
#[derive(Clone, Copy)]
pub(crate) struct ComputerTurnProgress<'a> {
    pub emitter: Option<&'a HarnessEventEmitter>,
    pub item_id: &'a str,
    pub call_id: &'a str,
}

impl ComputerTurnProgress<'_> {
    fn emit(&self, message: String) {
        let Some(emitter) = self.emitter else {
            return;
        };
        let _ = emitter.emit(tool_output_updated_event(self.item_id.to_string(), Some(self.call_id), message));
    }
}

/// Where a computer turn failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComputerFailureStage {
    Generate,
    Launch,
    Execute,
    Teardown,
}

/// Wire results keyed for history routing.
#[derive(Debug)]
pub(crate) enum NativeTurnResults {
    /// Anthropic results at their original content positions.
    Anthropic(Vec<(usize, Value)>),
    /// OpenAI outputs in call order.
    OpenAi(Vec<Value>),
}

/// Redacted turn summary: routing metadata only, never payloads.
#[derive(Debug, Clone)]
pub(crate) struct RedactedTurnSummary {
    pub target_kind: &'static str,
    pub target: String,
    pub recipient: String,
    pub action_kinds: Vec<String>,
    pub action_count: usize,
    /// Usable units: zero unless the turn completed, because failed and
    /// cancelled turns discard partial results instead of returning them.
    pub executed_units: usize,
    /// Review verdict when the review ran; `None` when it was not armed.
    pub review: Option<BatchReviewVerdict>,
    pub teardown_trigger: &'static str,
    pub teardown: TeardownOutcome,
}

/// How a computer turn ended. Usage rides alongside for turn-level accounting.
#[derive(Debug)]
pub(crate) struct ComputerTurnOutcome {
    pub kind: ComputerTurnOutcomeKind,
    pub model: String,
    pub usage: Option<Usage>,
}

/// Terminal states of one computer turn.
#[derive(Debug)]
pub(crate) enum ComputerTurnOutcomeKind {
    /// Refused before any I/O: disabled, incapable, or unconfigured.
    Disabled { reason: &'static str },
    /// The model proposed no native calls; ordinaries are counted in the
    /// report and dropped, never dispatched — the outer loop only ever
    /// sees the redacted text and issues its own validated calls.
    NoNativeCalls {
        content: Option<String>,
        ordinaries: Vec<ToolCall>,
    },
    /// The human refused the target.
    ApprovalDenied,
    /// Proposed actions exceeded the configured budget.
    BudgetExceeded { proposed: usize, max: usize },
    /// Decisions review halted the batch: suspect, or unscreened because the
    /// review could not conclude. The verdict rides along so the report can
    /// name which without re-deriving it.
    ReviewHalted { verdict: BatchReviewVerdict },
    /// Executed to completion with wire results and a redacted summary.
    Completed {
        results: NativeTurnResults,
        summary: RedactedTurnSummary,
    },
    /// Failed mid-turn; teardown still ran.
    Failed {
        stage: ComputerFailureStage,
        message: String,
    },
    /// Stopped; input halted and the target removed.
    Cancelled,
}

/// Display data derived from decoded native calls.
struct NativeCallSummary {
    call_ids: Vec<String>,
    kinds: Vec<String>,
    valid_actions: usize,
    provider: NativeProvider,
}

fn summarize_native_calls(calls: &[NativeCall]) -> Result<NativeCallSummary> {
    let Some(first) = calls.first() else {
        return Err(anyhow!("computer turn has no native calls"));
    };
    if calls.iter().any(|call| call.provider != first.provider) {
        return Err(anyhow!("computer turn mixes providers"));
    }
    let mut kinds = Vec::new();
    let mut valid_actions = 0;
    for call in calls {
        match &call.body {
            NativeCallBody::Anthropic { name, action } => match action {
                Ok(action) => {
                    valid_actions += 1;
                    kinds.push(action_kind(action).to_string());
                }
                Err(_) => kinds.push(name.clone()),
            },
            NativeCallBody::OpenAi { actions, .. } => {
                valid_actions += actions.len();
                kinds.extend(actions.iter().map(action_kind).map(str::to_string));
            }
        }
    }
    kinds.sort();
    kinds.dedup();
    Ok(NativeCallSummary {
        call_ids: calls.iter().map(|call| call.id.clone()).collect(),
        kinds,
        valid_actions,
        provider: first.provider,
    })
}

/// What teardown found: explicit in every case so the audit never guesses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TeardownOutcome {
    NotLaunched,
    Removed,
    AlreadyGone,
    Failed(String),
}

/// A proposed turn awaiting the human decision: everything generation and
/// review produced, with nothing launched yet.
pub(crate) struct ProposedComputerTurn {
    /// The approval prompt payload; also identifies the turn in events.
    pub(crate) approval: ApprovalRequest,
    model: String,
    pub(crate) usage: Option<Usage>,
    calls: Vec<NativeCall>,
    summary: NativeCallSummary,
    target: ComputerUseTarget,
    recipient: String,
    review_armed: bool,
    review_verdict: BatchReviewVerdict,
    turn_token: CancellationToken,
}

/// Either a turn ready for approval or an already-terminal outcome.
/// Splitting proposal from execution lets model-initiated callers approve
/// through the permission flow (which needs the turn context) without
/// holding dispatcher borrows across the prompt.
pub(crate) enum ComputerTurnProposal {
    Proceed(ProposedComputerTurn),
    Done(ComputerTurnOutcome),
}

/// Test-only combined entry: production flows propose, approve through the
/// permission flow, then execute, so only tests drive turns end to end.
/// Progress streams against the caller-supplied outer row identity, exactly
/// as the wrapper supplies it in production.
#[cfg(test)]
pub(crate) async fn run_computer_turn<Fut>(
    provider: &dyn LLMProvider,
    request: LLMRequest,
    config: &ComputerUseConfig,
    runtime: &mut ComputerTurnRuntime<'_>,
    emitter: Option<&HarnessEventEmitter>,
    harness_item_id: &str,
    tool_call_id: &str,
    approve: impl FnOnce(ApprovalRequest) -> Fut,
) -> Result<ComputerTurnOutcome>
where
    Fut: Future<Output = Result<bool>>,
{
    let progress = ComputerTurnProgress {
        emitter,
        item_id: harness_item_id,
        call_id: tool_call_id,
    };
    match propose_computer_turn(provider, request, config, runtime, progress).await? {
        ComputerTurnProposal::Done(outcome) => Ok(outcome),
        ComputerTurnProposal::Proceed(proposed) => {
            let granted = approve(proposed.approval.clone()).await?;
            execute_proposed_turn(proposed, config, runtime, granted, progress).await
        }
    }
}

/// The plain-words task the caller asked this turn to perform. The safety
/// reviewer must compare proposed actions against this request, not against
/// the acting model's own prose: a model describing benign intent is not
/// evidence that its actions serve the user's goal.
fn requested_task(request: &LLMRequest) -> String {
    request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == vtcode_core::llm::provider::MessageRole::User)
        .map(|message| message.content.as_text().into_owned())
        .unwrap_or_default()
}

/// Run the turn through batch proposal: gates, native generation, budget,
/// and Decisions review. Returns `Done` for every terminal-before-approval
/// outcome; `Proceed` carries a turn with nothing launched. Proposal streams
/// progress only; the wrapper completes the outer row for every outcome.
pub(crate) async fn propose_computer_turn(
    provider: &dyn LLMProvider,
    request: LLMRequest,
    config: &ComputerUseConfig,
    runtime: &mut ComputerTurnRuntime<'_>,
    progress: ComputerTurnProgress<'_>,
) -> Result<ComputerTurnProposal> {
    let model = request.model.clone();
    // Capture before `generate_native` consumes the request. This is the
    // caller's request, not the model's self-description of what it is doing.
    let requested_task = requested_task(&request);
    let disabled = |reason: &'static str| {
        Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::Disabled { reason },
            model: model.clone(),
            usage: None,
        }))
    };
    if !config.enabled {
        return disabled("disabled");
    }
    if !provider.supports_computer_use() {
        return disabled("unsupported provider");
    }
    let Some(build_dir) = config.browser_build.clone() else {
        return disabled("no target configured");
    };
    let target = ComputerUseTarget::Browser { build_dir };
    let recipient = format!("{} ({})", request.model, provider.name());

    progress.emit(format!("Proposing actions with {model}…"));
    let response = tokio::select! {
        biased;
        _ = wait_for_stop(runtime.stop, runtime.notify) => {
            return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
                kind: ComputerTurnOutcomeKind::Cancelled,
                model,
                usage: None,
            }));
        }
        response = provider.generate_native(request) => match response {
            Ok(response) => response,
            Err(error) => {
                return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
                    kind: ComputerTurnOutcomeKind::Failed {
                        stage: ComputerFailureStage::Generate,
                        message: error.to_string(),
                    },
                    model,
                    usage: None,
                }));
            }
        },
    };
    let model = response.model.clone();
    let usage = response.usage.clone();
    if response.native_calls.is_empty() {
        return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::NoNativeCalls {
                content: response.content,
                ordinaries: response.tool_calls,
            },
            model,
            usage,
        }));
    }

    let turn_token = CancellationToken::new();
    let summary = match summarize_native_calls(&response.native_calls) {
        Ok(summary) => summary,
        Err(error) => {
            return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
                kind: ComputerTurnOutcomeKind::Failed {
                    stage: ComputerFailureStage::Generate,
                    message: error.to_string(),
                },
                model,
                usage,
            }));
        }
    };
    if summary.valid_actions > config.max_actions_per_turn {
        return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::BudgetExceeded {
                proposed: summary.valid_actions,
                max: config.max_actions_per_turn,
            },
            model,
            usage,
        }));
    }

    let review_armed = config.decisions_review && provider.supports_decisions();
    let review = review_computer_batch(
        provider,
        &ComputerBatchSummary {
            task: requested_task,
            target: target.clone(),
            recipient: recipient.clone(),
            action_kinds: summary.kinds.clone(),
            action_count: summary.valid_actions,
        },
        config.decisions_review,
        REVIEW_TIMEOUT,
        &turn_token,
    )
    .await;
    record_review_usage(&mut *runtime.stats, provider.name(), &review);
    // An enabled review is the only semantic injection screen: the human
    // approves action kinds, not payloads. If it could not conclude, refuse
    // the batch instead of letting an unscreened one reach the target. The
    // refusal itself is audit material; the wrapper completes the outer row
    // with it, so proposal emits nothing terminal here.
    if !matches!(review.verdict, BatchReviewVerdict::Proceed) {
        let kind = match &review.verdict {
            BatchReviewVerdict::Unreviewed { reason: "cancelled" } => ComputerTurnOutcomeKind::Cancelled,
            _ => ComputerTurnOutcomeKind::ReviewHalted { verdict: review.verdict.clone() },
        };
        return Ok(ComputerTurnProposal::Done(ComputerTurnOutcome { kind, model, usage }));
    }

    let approval = ApprovalRequest {
        target: target.clone(),
        recipient: recipient.clone(),
        call_ids: summary.call_ids.clone(),
        action_kinds: summary.kinds.clone(),
        action_count: summary.valid_actions,
    };
    let batch_noun = if summary.valid_actions == 1 {
        "action"
    } else {
        "actions"
    };
    if review_armed {
        progress.emit(format!(
            "Safety review passed — proposed {} {batch_noun} ({}), awaiting approval…",
            summary.valid_actions,
            summary.kinds.join(", "),
        ));
    } else {
        progress.emit(format!(
            "Proposed {} {batch_noun} ({}), awaiting approval…",
            summary.valid_actions,
            summary.kinds.join(", "),
        ));
    }
    Ok(ComputerTurnProposal::Proceed(ProposedComputerTurn {
        approval,
        model,
        usage,
        calls: response.native_calls,
        summary,
        target,
        recipient,
        review_armed,
        review_verdict: review.verdict,
        turn_token,
    }))
}

/// Execute a proposed turn after the human decision. Denial ends the turn
/// before anything launches; approval launches, executes, and tears down
/// through the same guarded path as before. Execution streams progress
/// only; the wrapper completes the outer row for every outcome.
pub(crate) async fn execute_proposed_turn(
    proposed: ProposedComputerTurn,
    config: &ComputerUseConfig,
    runtime: &mut ComputerTurnRuntime<'_>,
    granted: bool,
    progress: ComputerTurnProgress<'_>,
) -> Result<ComputerTurnOutcome> {
    let ProposedComputerTurn {
        approval: _,
        model,
        usage,
        calls,
        summary,
        target,
        recipient,
        review_armed,
        review_verdict,
        turn_token,
    } = proposed;
    if !granted {
        return Ok(ComputerTurnOutcome {
            kind: if stop_requested(runtime.stop) {
                ComputerTurnOutcomeKind::Cancelled
            } else {
                ComputerTurnOutcomeKind::ApprovalDenied
            },
            model,
            usage,
        });
    }

    let batch_noun = if summary.valid_actions == 1 {
        "action"
    } else {
        "actions"
    };
    progress.emit(format!(
        "Approved {} {batch_noun} ({}) — launching preview…",
        summary.valid_actions,
        summary.kinds.join(", "),
    ));
    let execution =
        run_guarded_execution(&summary, calls, &config.image, &target, config, runtime, &turn_token, progress).await;
    let executed_units = match &execution.outcome {
        GuardedOutcome::Completed { units, .. } => *units,
        _ => 0,
    };
    let redacted = RedactedTurnSummary {
        target_kind: target.kind(),
        target: target.display(),
        recipient,
        action_kinds: summary.kinds,
        action_count: summary.valid_actions,
        executed_units,
        review: review_armed.then_some(review_verdict),
        teardown_trigger: execution.teardown_trigger,
        teardown: execution.teardown.clone(),
    };
    let kind = match execution.outcome {
        GuardedOutcome::Completed { results, .. } => {
            ComputerTurnOutcomeKind::Completed { results, summary: redacted.clone() }
        }
        GuardedOutcome::Failed { stage, message } => ComputerTurnOutcomeKind::Failed { stage, message },
        GuardedOutcome::Cancelled => ComputerTurnOutcomeKind::Cancelled,
    };
    Ok(ComputerTurnOutcome { kind, model, usage })
}

struct GuardedExecution {
    outcome: GuardedOutcome,
    teardown_trigger: &'static str,
    teardown: TeardownOutcome,
}

enum GuardedOutcome {
    Completed {
        results: NativeTurnResults,
        units: usize,
    },
    Failed {
        stage: ComputerFailureStage,
        message: String,
    },
    Cancelled,
}

/// Actionable guidance when the container engine is unreachable. Spawning
/// `docker run` against a stopped daemon surfaces only an opaque ENOENT from
/// the missing socket, so refuse before the launch with a message the model
/// and user can act on. Kept as a constant so the durable failure line is
/// stable and independently testable.
const DOCKER_UNAVAILABLE_GUIDANCE: &str =
    "Docker daemon is not available; start your container engine (e.g. Docker Desktop) and retry";

/// Fail closed on an unreachable container engine. The availability bit is
/// injected so the actionable failure text stays testable without a daemon.
fn ensure_container_engine(available: bool) -> Result<()> {
    ensure!(available, "{DOCKER_UNAVAILABLE_GUIDANCE}");
    Ok(())
}

/// Launch, execute, and tear down with stop awareness. Stopping abandons
/// in-flight work (revocation guards fire on drop) and still removes an
/// already-launched target; a stop that lands mid-launch is honored by the
/// post-launch check, which terminates the freshly launched target before
/// any input runs.
#[allow(
    clippy::too_many_arguments,
    reason = "Dispatcher funnel takes one bundle per concern."
)]
async fn run_guarded_execution(
    summary: &NativeCallSummary,
    native_calls: Vec<NativeCall>,
    image: &str,
    target: &ComputerUseTarget,
    config: &ComputerUseConfig,
    runtime: &mut ComputerTurnRuntime<'_>,
    turn: &CancellationToken,
    progress: ComputerTurnProgress<'_>,
) -> GuardedExecution {
    let turn_timeout = Duration::from_secs(config.turn_timeout_secs);
    let launched = async {
        // Direct destructure: Browser is the only target today, and adding a
        // macOS variant must force this launch site to handle it at compile time.
        let ComputerUseTarget::Browser { build_dir } = target;
        let spec = ContainerSpec::approved(image, build_dir).await?;
        // Check the daemon after validating the build dir so configuration
        // errors keep their precise diagnostics (see the canonicalize
        // regression test). A stopped daemon otherwise fails later with a
        // bare "No such file or directory" from the missing socket.
        ensure_container_engine(docker_available().await)?;
        let (handle, replies, commands) = ContainerHandle::launch(&spec).await?;
        let driver = BrowserDriver::connect(replies, commands).await?;
        Ok((handle, driver))
    }
    .await
    .map_err(|error: anyhow::Error| (ComputerFailureStage::Launch, format!("{error:#}")));
    let (handle, driver) = match launched {
        Ok(launched) => launched,
        Err((stage, message)) => {
            return GuardedExecution {
                outcome: GuardedOutcome::Failed { stage, message },
                teardown_trigger: "failed",
                teardown: TeardownOutcome::NotLaunched,
            };
        }
    };
    if stop_requested(runtime.stop) {
        turn.cancel();
        let teardown = terminate_for_audit(&handle).await;
        return GuardedExecution {
            outcome: GuardedOutcome::Cancelled,
            teardown_trigger: "stopped",
            teardown,
        };
    }
    let batch_noun = if summary.valid_actions == 1 {
        "action"
    } else {
        "actions"
    };
    progress.emit(format!(
        "Preview ready — running {} {batch_noun} ({})…",
        summary.valid_actions,
        summary.kinds.join(", "),
    ));
    let mut control = TurnControl::after_approval(driver, turn);
    let executed = tokio::time::timeout(turn_timeout, async {
        tokio::select! {
            biased;
            _ = wait_for_stop(runtime.stop, runtime.notify) => {
                turn.cancel();
                None
            }
            result = execute_native_calls(summary.provider, &mut control, native_calls, turn_timeout, progress)
            => Some(result),
        }
    })
    .await;
    let outcome = match executed {
        Err(_) => {
            turn.cancel();
            GuardedOutcome::Failed {
                stage: ComputerFailureStage::Execute,
                message: "computer turn timed out".to_string(),
            }
        }
        Ok(None) => GuardedOutcome::Cancelled,
        Ok(Some(Ok((results, units)))) => GuardedOutcome::Completed { results, units },
        Ok(Some(Err(error))) => {
            if turn.is_cancelled() {
                GuardedOutcome::Cancelled
            } else {
                GuardedOutcome::Failed {
                    stage: ComputerFailureStage::Execute,
                    message: format!("{error:#}"),
                }
            }
        }
    };
    progress.emit("Removing preview target…".to_string());
    let teardown = terminate_for_audit(&handle).await;
    // A teardown failure after completion changes the verdict: the target
    // state is unknown, so success must not be reported.
    let outcome = match (&outcome, &teardown) {
        (GuardedOutcome::Completed { .. }, TeardownOutcome::Failed(error)) => GuardedOutcome::Failed {
            stage: ComputerFailureStage::Teardown,
            message: format!("computer target teardown failed: {error}"),
        },
        _ => outcome,
    };
    if !matches!(teardown, TeardownOutcome::Removed | TeardownOutcome::AlreadyGone) {
        tracing::warn!("computer-use teardown did not confirm removal");
    }
    GuardedExecution { outcome, teardown_trigger: "finished", teardown }
}

fn stop_requested(stop: &CtrlCState) -> bool {
    stop.is_cancel_requested() || stop.is_cancel_handled() || stop.is_exit_requested()
}

/// Remove the exact container for the audit trail, mapping the outcome.
async fn terminate_for_audit(handle: &ContainerHandle) -> TeardownOutcome {
    match handle.terminate().await {
        Ok(outcome) => match outcome {
            TerminateOutcome::Removed => TeardownOutcome::Removed,
            TerminateOutcome::AlreadyGone => TeardownOutcome::AlreadyGone,
        },
        Err(error) => TeardownOutcome::Failed(format!("{error:#}")),
    }
}

/// Record auxiliary review usage without touching cache health.
fn record_review_usage(stats: &mut SessionStats, provider: &str, review: &super::review::BatchReviewOutcome) {
    stats.record_usage(provider, review.usage.as_ref());
    stats.record_cost(review.usage.as_ref().map(usage_cost::estimate_decisions_cost));
}

/// Execute decoded natives against one live target. Anthropic calls run as
/// one batch with halt/skip semantics; OpenAI calls run in order, failing
/// the turn on the first malformed or failed call. Returns wire results and
/// the answered unit count. OpenAI progress names each call (ids correlate
/// with the completion verdicts); the Anthropic batch is one unit, so its
/// pre-batch line above is the only per-turn progress it needs.
async fn execute_native_calls<D: Driver>(
    provider_kind: NativeProvider,
    control: &mut TurnControl<D>,
    native_calls: Vec<NativeCall>,
    timeout: Duration,
    progress: ComputerTurnProgress<'_>,
) -> Result<(NativeTurnResults, usize)> {
    match provider_kind {
        NativeProvider::Anthropic => {
            let results = anthropic_use::execute_decoded(control, native_calls, timeout).await?;
            let units = results.len();
            Ok((NativeTurnResults::Anthropic(results), units))
        }
        NativeProvider::OpenAi => {
            let total = native_calls.len();
            let mut results = Vec::with_capacity(total);
            for (index, call) in native_calls.into_iter().enumerate() {
                let kinds = match &call.body {
                    NativeCallBody::OpenAi { actions, .. } => actions
                        .iter()
                        .map(action_kind)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    NativeCallBody::Anthropic { .. } => "batch".to_string(),
                };
                progress.emit(format!("Running call {}/{total} ({}, {kinds})…", index + 1, call.id));
                results.push(openai_use::execute_decoded_call(control, call, timeout).await?);
            }
            let units = results.len();
            Ok((NativeTurnResults::OpenAi(results), units))
        }
    }
}

#[cfg(test)]
mod tests;
