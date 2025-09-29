use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task;
use tokio::time::sleep;

use serde_json::Value;
use toml::Value as TomlValue;
use tracing::warn;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::constants::{defaults, ui};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent};
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, ToolPermissionDecision};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineEvent, InlineHandle, InlineTextStyle, convert_style as convert_ui_style, spawn_session,
    theme_from_styles,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata, SessionMessage};
use vtcode_core::utils::transcript;

use crate::agent::runloop::context::{
    apply_aggressive_trim_unified, enforce_unified_context_window, prune_unified_tool_responses,
};
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::is_context_overflow_error;
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::{SlashCommandOutcome, handle_slash_command};
use crate::agent::runloop::text_tools::detect_textual_tool_call;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::ui::render_session_banner;

use super::display::{display_user_message, ensure_turn_bottom_gap, persist_theme_preference};
use super::session_setup::{SessionState, initialize_session};
use super::shell::{derive_recent_tool_output, should_short_circuit_shell};
use crate::agent::runloop::mcp_events;

#[derive(Default)]
struct SessionStats {
    tools: BTreeSet<String>,
}

impl SessionStats {
    fn record_tool(&mut self, name: &str) {
        self.tools.insert(name.to_string());
    }

    fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitlDecision {
    Approved,
    Denied,
    Exit,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPermissionFlow {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

const HITL_MODAL_TITLE: &str = "Tool approval required";
const HITL_MODAL_INSTRUCTION: &str = "Respond with 'y' to approve or 'n' to deny.";
const HITL_MODAL_CANCEL_HINT: &str = "Press Esc to cancel.";
const HITL_MODAL_TOOL_LABEL: &str = "Tool:";
const HITL_MODAL_KEY_LABEL: &str = "Key:";

#[derive(Default)]
struct CtrlCState {
    cancel_requested: AtomicBool,
    exit_requested: AtomicBool,
    exit_armed: AtomicBool,
}

enum CtrlCSignal {
    Cancel,
    Exit,
}

impl CtrlCState {
    fn new() -> Self {
        Self::default()
    }

    fn register_signal(&self) -> CtrlCSignal {
        if self.cancel_requested.swap(true, Ordering::SeqCst)
            || self.exit_armed.swap(false, Ordering::SeqCst)
        {
            self.exit_requested.store(true, Ordering::SeqCst);
            CtrlCSignal::Exit
        } else {
            CtrlCSignal::Cancel
        }
    }

    fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.exit_armed.store(true, Ordering::SeqCst);
    }

    fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }

    fn disarm_exit(&self) {
        self.exit_armed.store(false, Ordering::SeqCst);
    }
}

struct PlaceholderGuard {
    handle: InlineHandle,
    restore: Option<String>,
}

impl PlaceholderGuard {
    fn new(handle: &InlineHandle, restore: Option<String>) -> Self {
        Self {
            handle: handle.clone(),
            restore,
        }
    }
}

impl Drop for PlaceholderGuard {
    fn drop(&mut self) {
        self.handle.set_placeholder(self.restore.clone());
    }
}

struct ModalGuard {
    handle: InlineHandle,
    active: bool,
}

impl ModalGuard {
    fn show(handle: &InlineHandle, title: String, lines: Vec<String>) -> Self {
        handle.close_modal();
        handle.show_modal(title, lines);
        Self {
            handle: handle.clone(),
            active: true,
        }
    }

    fn close(&mut self) {
        if self.active {
            self.handle.close_modal();
            self.active = false;
        }
    }
}

impl Drop for ModalGuard {
    fn drop(&mut self) {
        self.close();
    }
}

fn render_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
) -> Result<()> {
    let (headline, used_keys) = describe_tool_action(tool_name, args);
    renderer.line(MessageStyle::Info, &format!("→ {}", headline))?;

    let bullets = derive_tool_argument_bullets(args, &used_keys);
    for bullet in bullets {
        renderer.line(MessageStyle::Output, &format!("    • {bullet}"))?;
    }

    Ok(())
}

fn derive_tool_argument_bullets(args: &Value, skip_keys: &HashSet<String>) -> Vec<String> {
    match args {
        Value::Object(map) => {
            if map.is_empty() {
                return Vec::new();
            }
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.into_iter()
                .filter(|key| !skip_keys.contains(key.as_str()))
                .filter_map(|key| {
                    map.get(key).map(|value| {
                        let label = humanize_key(key);
                        let summary = summarize_json_value(value);
                        format!("{label}: {summary}")
                    })
                })
                .collect()
        }
        Value::Array(items) => {
            if items.is_empty() {
                Vec::new()
            } else {
                vec![format!("Items: {}", summarize_json_value(args))]
            }
        }
        Value::Null => Vec::new(),
        primitive => vec![summarize_json_value(primitive)],
    }
}

fn describe_tool_action(tool_name: &str, args: &Value) -> (String, HashSet<String>) {
    match tool_name {
        tool_names::RUN_TERMINAL_CMD | tool_names::BASH => describe_shell_command(args)
            .unwrap_or_else(|| ("Run shell command".to_string(), HashSet::new())),
        tool_names::LIST_FILES => {
            describe_list_files(args).unwrap_or_else(|| ("List files".to_string(), HashSet::new()))
        }
        tool_names::GREP_SEARCH => describe_grep_search(args)
            .unwrap_or_else(|| ("Search with grep".to_string(), HashSet::new())),
        tool_names::READ_FILE => describe_path_action(args, "Read file", &["path"])
            .unwrap_or_else(|| ("Read file".to_string(), HashSet::new())),
        tool_names::WRITE_FILE => describe_path_action(args, "Write file", &["path"])
            .unwrap_or_else(|| ("Write file".to_string(), HashSet::new())),
        tool_names::EDIT_FILE => describe_path_action(args, "Edit file", &["path"])
            .unwrap_or_else(|| ("Edit file".to_string(), HashSet::new())),
        tool_names::CREATE_FILE => describe_path_action(args, "Create file", &["path"])
            .unwrap_or_else(|| ("Create file".to_string(), HashSet::new())),
        tool_names::DELETE_FILE => describe_path_action(args, "Delete file", &["path"])
            .unwrap_or_else(|| ("Delete file".to_string(), HashSet::new())),
        tool_names::CURL => {
            describe_curl(args).unwrap_or_else(|| ("Fetch URL".to_string(), HashSet::new()))
        }
        tool_names::SIMPLE_SEARCH => describe_simple_search(args)
            .unwrap_or_else(|| ("Search workspace".to_string(), HashSet::new())),
        tool_names::SRGN => describe_srgn(args)
            .unwrap_or_else(|| ("Search and replace".to_string(), HashSet::new())),
        tool_names::APPLY_PATCH => ("Apply workspace patch".to_string(), HashSet::new()),
        tool_names::UPDATE_PLAN => ("Update task plan".to_string(), HashSet::new()),
        _ => (
            format!("Use {}", humanize_tool_name(tool_name)),
            HashSet::new(),
        ),
    }
}

fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let mut used = HashSet::new();
    if let Some(parts) = args
        .get("command")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|parts: &Vec<String>| !parts.is_empty())
    {
        used.insert("command".to_string());
        let joined = parts.join(" ");
        let summary = truncate_middle(&joined, 60);
        return Some((format!("Run command {}", summary), used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 60);
        return Some((format!("Run bash {}", summary), used));
    }

    None
}

fn describe_list_files(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            truncate_middle(&path, 60)
        };
        return Some((format!("List files in {}", location), used));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        let mut used = HashSet::new();
        used.insert("name_pattern".to_string());
        return Some((
            format!("Find files named {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        let mut used = HashSet::new();
        used.insert("content_pattern".to_string());
        return Some((
            format!("Search files for {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    None
}

fn describe_grep_search(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("path".to_string());
            Some((
                format!(
                    "Grep {} in {}",
                    truncate_middle(&pat, 40),
                    truncate_middle(&path, 40)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Grep {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_simple_search(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(query) = lookup_string(args, "query") {
        let mut used = HashSet::new();
        used.insert("query".to_string());
        return Some((format!("Search for {}", truncate_middle(&query, 50)), used));
    }
    None
}

fn describe_srgn(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let replacement = lookup_string(args, "replacement");
    match (pattern, replacement) {
        (Some(pat), Some(rep)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("replacement".to_string());
            Some((
                format!(
                    "Replace {} → {}",
                    truncate_middle(&pat, 30),
                    truncate_middle(&rep, 30)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Search for {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_path_action(
    args: &Value,
    verb: &str,
    keys: &[&str],
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let summary = truncate_middle(&value, 60);
            return Some((format!("{} {}", verb, summary), used));
        }
    }
    None
}

fn describe_curl(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(url) = lookup_string(args, "url") {
        let mut used = HashSet::new();
        used.insert("url".to_string());
        return Some((format!("Fetch {}", truncate_middle(&url, 60)), used));
    }
    None
}

fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}
fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

fn summarize_json_value(value: &Value) -> String {
    const MAX_LEN: usize = 80;
    const ARRAY_PREVIEW: usize = 3;
    match value {
        Value::String(text) => {
            format!("`{}`", truncate_middle(&condense_whitespace(text), MAX_LEN))
        }
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }
            if items.iter().all(|item| matches!(item, Value::String(_))) {
                let preview: Vec<String> = items
                    .iter()
                    .take(ARRAY_PREVIEW)
                    .map(|item| {
                        item.as_str()
                            .map(condense_whitespace)
                            .map(|s| truncate_middle(&s, MAX_LEN / ARRAY_PREVIEW.max(1)))
                            .unwrap_or_else(|| "…".to_string())
                    })
                    .collect();
                let joined = preview.join(" ");
                let suffix = if items.len() > ARRAY_PREVIEW {
                    format!(" … ({} items)", items.len())
                } else {
                    String::new()
                };
                format!("`{}`{}", truncate_middle(&joined, MAX_LEN), suffix)
            } else {
                format!("[{} items]", items.len())
            }
        }
        Value::Object(map) => format!("{{{} keys}}", map.len()),
    }
}

fn condense_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return text.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);
    let mut result: String = chars.iter().take(head_len).collect();
    result.push('…');
    if tail_len > 0 {
        let tail: String = chars
            .iter()
            .rev()
            .take(tail_len)
            .cloned()
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        result.push_str(&tail);
    }
    result
}

async fn prompt_tool_permission(
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
) -> Result<HitlDecision> {
    // Clear any existing content
    renderer.line_if_not_empty(MessageStyle::Info)?;

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Approve '{}' tool? Respond with 'y' to approve or 'n' to deny. (Esc to cancel)",
            tool_name
        ),
    )?;

    let tool_label = humanize_tool_name(tool_name);
    let _modal_guard = if renderer.prefers_untruncated_output() {
        let mut lines = Vec::new();
        lines.push(format!("{HITL_MODAL_TOOL_LABEL} {tool_label}"));
        lines.push(format!("{HITL_MODAL_KEY_LABEL} {tool_name}"));
        lines.push(String::new());
        lines.push(HITL_MODAL_INSTRUCTION.to_string());
        lines.push(HITL_MODAL_CANCEL_HINT.to_string());
        Some(ModalGuard::show(
            handle,
            HITL_MODAL_TITLE.to_string(),
            lines,
        ))
    } else {
        None
    };

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    handle.set_placeholder(Some("y/n (Esc to cancel)".to_string()));

    // Yield once so the UI processes the prompt lines and placeholder update
    // before we start listening for user input. Without this the question would
    // only appear after a subsequent event (like cancel) fired.
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified(), if !ctrl_c_state.is_cancel_requested() => None,
            event = events.recv() => event,
        };

        let Some(event) = maybe_event else {
            // Clear input before exiting
            handle.clear_input();
            if ctrl_c_state.is_cancel_requested() {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        ctrl_c_state.disarm_exit();

        match event {
            InlineEvent::Submit(input) => {
                let normalized = input.trim().to_lowercase();
                if normalized.is_empty() {
                    renderer.line(MessageStyle::Info, "Please respond with 'yes' or 'no'.")?;
                    continue;
                }

                if matches!(normalized.as_str(), "y" | "yes" | "approve" | "allow") {
                    // Clear the input before returning
                    handle.clear_input();
                    return Ok(HitlDecision::Approved);
                }

                if matches!(normalized.as_str(), "n" | "no" | "deny" | "cancel" | "stop") {
                    // Clear the input before returning
                    handle.clear_input();
                    return Ok(HitlDecision::Denied);
                }

                renderer.line(
                    MessageStyle::Info,
                    "Respond with 'yes' to approve or 'no' to deny.",
                )?;
            }
            InlineEvent::Cancel => {
                handle.clear_input();
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Exit => {
                handle.clear_input();
                return Ok(HitlDecision::Exit);
            }
            InlineEvent::Interrupt => {
                handle.clear_input();
                return Ok(HitlDecision::Interrupt);
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => {
                // Scrolling is handled by the TUI event loop, just continue
            }
        }
    }
}

async fn ensure_tool_permission(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    default_placeholder: Option<String>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<ToolPermissionFlow> {
    match tool_registry.evaluate_tool_policy(tool_name)? {
        ToolPermissionDecision::Allow => Ok(ToolPermissionFlow::Approved),
        ToolPermissionDecision::Deny => Ok(ToolPermissionFlow::Denied),
        ToolPermissionDecision::Prompt => {
            let decision = prompt_tool_permission(
                tool_name,
                renderer,
                handle,
                events,
                ctrl_c_state,
                ctrl_c_notify,
                default_placeholder,
            )
            .await?;
            match decision {
                HitlDecision::Approved => {
                    tool_registry.mark_tool_preapproved(tool_name);
                    if let Err(err) =
                        tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Allow)
                    {
                        warn!(
                            "Failed to persist MCP approval for tool '{}': {}",
                            tool_name, err
                        );
                    }
                    Ok(ToolPermissionFlow::Approved)
                }
                HitlDecision::Denied => {
                    if let Err(err) =
                        tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Deny)
                    {
                        warn!(
                            "Failed to persist MCP denial for tool '{}': {}",
                            tool_name, err
                        );
                    }
                    Ok(ToolPermissionFlow::Denied)
                }
                HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
                HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
            }
        }
    }
}

fn apply_prompt_style(handle: &InlineHandle) {
    let styles = theme::active_styles();
    let style = convert_ui_style(styles.primary);
    handle.set_prompt("❯ ".to_string(), style);
}

const SPINNER_UPDATE_INTERVAL_MS: u64 = 120;

struct PlaceholderSpinner {
    handle: InlineHandle,
    restore_hint: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
}

fn spinner_placeholder_style() -> InlineTextStyle {
    let styles = theme::active_styles();
    let mut style = convert_ui_style(styles.secondary);
    if style.color.is_none() {
        let fallback = convert_ui_style(styles.primary);
        style.color = fallback.color;
    }
    style.bold = true;
    style
}

impl PlaceholderSpinner {
    fn new(
        handle: &InlineHandle,
        restore_hint: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let active = Arc::new(AtomicBool::new(true));
        let spinner_active = active.clone();
        let spinner_handle = handle.clone();
        let restore_on_stop = restore_hint.clone();
        let spinner_style = spinner_placeholder_style();

        spinner_handle.set_input_enabled(false);
        spinner_handle.set_cursor_visible(false);
        let task = task::spawn(async move {
            while spinner_active.load(Ordering::SeqCst) {
                let display = message.clone();
                spinner_handle
                    .set_placeholder_with_style(Some(display), Some(spinner_style.clone()));
                sleep(Duration::from_millis(SPINNER_UPDATE_INTERVAL_MS)).await;
            }

            spinner_handle.set_cursor_visible(true);
            spinner_handle.set_input_enabled(true);
            spinner_handle.set_placeholder_with_style(restore_on_stop, None);
        });

        Self {
            handle: handle.clone(),
            restore_hint,
            active,
            task,
        }
    }

    fn finish(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.handle
                .set_placeholder_with_style(self.restore_hint.clone(), None);
            self.handle.set_input_enabled(true);
            self.handle.set_cursor_visible(true);
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

fn map_render_error(provider_name: &str, err: anyhow::Error) -> uni::LLMError {
    let formatted_error = error_display::format_llm_error(
        provider_name,
        &format!("Failed to render streaming output: {}", err),
    );
    uni::LLMError::Provider(formatted_error)
}

fn stream_plain_response_delta(
    renderer: &mut AnsiRenderer,
    style: MessageStyle,
    indent: &str,
    pending_indent: &mut bool,
    delta: &str,
) -> Result<()> {
    for chunk in delta.split_inclusive('\n') {
        if chunk.is_empty() {
            continue;
        }

        if chunk.ends_with('\n') {
            let text = &chunk[..chunk.len() - 1];
            if !text.is_empty() {
                if *pending_indent && !indent.is_empty() {
                    renderer.inline_with_style(style, indent)?;
                }
                renderer.inline_with_style(style, text)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, "\n")?;
            *pending_indent = true;
        } else {
            if *pending_indent && !indent.is_empty() {
                renderer.inline_with_style(style, indent)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, chunk)?;
        }
    }

    Ok(())
}

async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let mut stream = provider.stream(request).await?;
    let provider_name = provider.name();
    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
    let supports_streaming_markdown = renderer.supports_streaming_markdown();
    let mut rendered_line_count = 0usize;
    let response_style = MessageStyle::Response;
    let response_indent = response_style.indent();
    let mut needs_indent = true;
    let finish_spinner = |active: &mut bool| {
        if *active {
            spinner.finish();
            *active = false;
        }
    };
    let mut emitted_tokens = false;

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                finish_spinner(&mut spinner_active);
                aggregated.push_str(&delta);
                if supports_streaming_markdown {
                    rendered_line_count = renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                } else {
                    stream_plain_response_delta(
                        renderer,
                        response_style,
                        response_indent,
                        &mut needs_indent,
                        &delta,
                    )
                    .map_err(|err| map_render_error(provider_name, err))?;
                }
                emitted_tokens = true;
            }
            Ok(LLMStreamEvent::Reasoning { .. }) => {}
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(response);
            }
            Err(err) => {
                finish_spinner(&mut spinner_active);
                return Err(err);
            }
        }
    }

    finish_spinner(&mut spinner_active);

    let response = final_response.ok_or_else(|| {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            "Stream ended without a completion event",
        );
        uni::LLMError::Provider(formatted_error)
    })?;

    if aggregated.is_empty() {
        if let Some(content) = response.content.clone() {
            if !content.is_empty() {
                aggregated.push_str(&content);
            }
        }
    }

    if !aggregated.is_empty() {
        if !emitted_tokens {
            if supports_streaming_markdown {
                let _ = renderer
                    .stream_markdown_response(&aggregated, rendered_line_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                renderer
                    .line(MessageStyle::Response, &aggregated)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
        } else if !supports_streaming_markdown && !aggregated.ends_with('\n') {
            renderer
                .line_if_not_empty(MessageStyle::Response)
                .map_err(|err| map_render_error(provider_name, err))?;
        }
    }

    Ok((response, emitted_tokens))
}

enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
}

const CONFIG_MODAL_TITLE: &str = "VTCode Configuration";
const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];

struct ConfigModalContent {
    title: String,
    source_label: String,
    config_lines: Vec<String>,
}

async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || VTCodeConfig::bootstrap_project(&workspace, force))
        .await
        .map_err(|err| anyhow!("failed to join configuration bootstrap task: {}", err))?;
    result.with_context(|| format!("failed to initialize configuration in {}", label))
}

async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace indexing task: {}", err))?;
    result.with_context(|| format!("failed to build workspace index in {}", label))
}

async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg))
        .await
        .map_err(|err| anyhow!("failed to join configuration load task: {}", err))?
}

fn prepare_config_modal_content(
    workspace: &Path,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "failed to resolve configuration for workspace {}",
            workspace.display()
        )
    })?;

    let config_path = manager.config_path().map(Path::to_path_buf);
    let config_data = if config_path.is_some() {
        manager.config().clone()
    } else if let Some(snapshot) = vt_cfg {
        snapshot
    } else {
        manager.config().clone()
    };

    let mut value = TomlValue::try_from(config_data)
        .context("failed to serialize configuration for display")?;
    mask_sensitive_config(&mut value);

    let formatted =
        toml::to_string_pretty(&value).context("failed to render configuration to TOML")?;
    let config_lines = formatted.lines().map(|line| line.to_string()).collect();

    let source_label = if let Some(path) = config_path {
        format!("Configuration source: {}", path.display())
    } else {
        "No vtcode.toml file found; showing runtime defaults.".to_string()
    };

    Ok(ConfigModalContent {
        title: CONFIG_MODAL_TITLE.to_string(),
        source_label,
        config_lines,
    })
}

fn mask_sensitive_config(value: &mut TomlValue) {
    match value {
        TomlValue::Table(table) => {
            for (key, entry) in table.iter_mut() {
                if is_sensitive_key(key) {
                    *entry = TomlValue::String("********".to_string());
                } else {
                    mask_sensitive_config(entry);
                }
            }
        }
        TomlValue::Array(items) => {
            for item in items {
                mask_sensitive_config(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    // Set up panic handler to ensure MCP cleanup on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Application panic occurred: {:?}", panic_info);
        // Note: We can't easily access the MCP client here due to move semantics
        // The cleanup will happen in the Drop implementations
        original_hook(panic_info);
    }));
    let SessionState {
        session_bootstrap,
        provider_client,
        mut tool_registry,
        tools,
        trim_config,
        mut conversation_history,
        mut ledger,
        trajectory: traj,
        base_system_prompt,
        full_auto_allowlist,
        #[allow(unused_variables)]
        mcp_client,
        mut mcp_panel_state,
    } = initialize_session(config, vt_cfg, full_auto).await?;

    let active_styles = theme::active_styles();
    let theme_spec = theme_from_styles(&active_styles);
    let default_placeholder = session_bootstrap.placeholder.clone();
    let inline_rows = vt_cfg
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);
    let session = spawn_session(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
    )
    .context("failed to launch inline session")?;
    let handle = session.handle.clone();
    let highlight_config = vt_cfg
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

    transcript::clear();

    let workspace_label = config
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = config.workspace.to_string_lossy().into_owned();
    let provider_label = if config.provider.trim().is_empty() {
        provider_client.name().to_string()
    } else {
        config.provider.clone()
    };
    let archive_metadata = SessionArchiveMetadata::new(
        workspace_label,
        workspace_path,
        config.model.clone(),
        provider_label,
        config.theme.clone(),
        config.reasoning_effort.as_str().to_string(),
    );
    let mut session_archive_error: Option<String> = None;
    let mut session_archive = match SessionArchive::new(archive_metadata) {
        Ok(archive) => Some(archive),
        Err(err) => {
            session_archive_error = Some(err.to_string());
            None
        }
    };

    handle.set_theme(theme_spec);
    apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());
    let center_status = format!("{} · {}", config.model, reasoning_label);
    renderer.line(MessageStyle::Info, &center_status)?;
    renderer.line_if_not_empty(MessageStyle::Output)?;

    render_session_banner(&mut renderer, config, &session_bootstrap)?;
    if let Some(text) = session_bootstrap.welcome_text.as_ref() {
        renderer.line(MessageStyle::Response, text)?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    // MCP events are now rendered as message blocks in the conversation history

    if let Some(message) = session_archive_error.take() {
        renderer.line(
            MessageStyle::Info,
            &format!("Session archiving disabled: {}", message),
        )?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    if full_auto {
        if let Some(allowlist) = full_auto_allowlist.as_ref() {
            if allowlist.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Full-auto mode enabled. Permitted tools: {}",
                        allowlist.join(", ")
                    ),
                )?;
            }
        }
    }

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mcp_client_for_signal = mcp_client.clone();
    {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }

                let signal = state.register_signal();
                notify.notify_waiters();

                // Shutdown MCP client on interrupt
                if let Some(mcp_client) = &mcp_client_for_signal {
                    if let Err(e) = mcp_client.shutdown().await {
                        let error_msg = e.to_string();
                        if error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            eprintln!(
                                "Info: MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                e
                            );
                        } else {
                            eprintln!("Warning: Failed to shutdown MCP client on interrupt: {}", e);
                        }
                    }
                }

                if matches!(signal, CtrlCSignal::Exit) {
                    break;
                }
            }
        });
    }

    let mut session_stats = SessionStats::default();
    let mut events = session.events;
    let mut last_forced_redraw = Instant::now();
    loop {
        if ctrl_c_state.is_exit_requested() {
            break;
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified(), if ctrl_c_state.is_cancel_requested() => None,
            event = events.recv() => event,
        };

        if ctrl_c_state.is_cancel_requested() {
            if ctrl_c_state.is_exit_requested() {
                break;
            }

            renderer.line_if_not_empty(MessageStyle::Output)?;
            renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            handle.clear_input();
            handle.set_placeholder(default_placeholder.clone());
            ctrl_c_state.clear_cancel();
            continue;
        }

        let Some(event) = maybe_event else {
            break;
        };

        ctrl_c_state.disarm_exit();

        let submitted = match event {
            InlineEvent::Submit(text) => text,
            InlineEvent::Cancel => {
                renderer.line(
                    MessageStyle::Info,
                    "Cancellation request noted. No active run to stop.",
                )?;
                continue;
            }
            InlineEvent::Exit => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            InlineEvent::Interrupt => {
                break;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => continue,
        };

        let input_owned = submitted.trim().to_string();

        if input_owned.is_empty() {
            continue;
        }

        match input_owned.as_str() {
            "" => continue,
            "exit" | "quit" => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            "help" => {
                renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            input if input.starts_with('/') => {
                // Handle slash commands
                if let Some(command_input) = input.strip_prefix('/') {
                    match handle_slash_command(command_input, &mut renderer)? {
                        SlashCommandOutcome::Handled => {
                            continue;
                        }
                        SlashCommandOutcome::ThemeChanged(theme_id) => {
                            persist_theme_preference(&mut renderer, &theme_id)?;
                            let styles = theme::active_styles();
                            handle.set_theme(theme_from_styles(&styles));
                            apply_prompt_style(&handle);
                            continue;
                        }
                        SlashCommandOutcome::InitializeWorkspace { force } => {
                            let workspace_path = config.workspace.clone();
                            let workspace_label = workspace_path.display().to_string();
                            renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "Initializing vtcode configuration in {}...",
                                    workspace_label
                                ),
                            )?;

                            let created_files =
                                match bootstrap_config_files(workspace_path.clone(), force).await {
                                    Ok(files) => files,
                                    Err(err) => {
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("Failed to initialize configuration: {}", err),
                                        )?;
                                        continue;
                                    }
                                };

                            if created_files.is_empty() {
                                renderer.line(
                                    MessageStyle::Info,
                                    "Existing configuration detected; no files were changed.",
                                )?;
                            } else {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Created {}: {}",
                                        if created_files.len() == 1 {
                                            "file"
                                        } else {
                                            "files"
                                        },
                                        created_files.join(", "),
                                    ),
                                )?;
                            }

                            renderer.line(
                                MessageStyle::Info,
                                "Indexing workspace context (this may take a moment)...",
                            )?;

                            match build_workspace_index(workspace_path.clone()).await {
                                Ok(()) => {
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Workspace indexing complete. Stored under .vtcode/index.",
                                    )?;
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index workspace: {}", err),
                                    )?;
                                }
                            }

                            continue;
                        }
                        SlashCommandOutcome::ShowConfig => {
                            let workspace_path = config.workspace.clone();
                            let vt_snapshot = vt_cfg.cloned();
                            match load_config_modal_content(workspace_path, vt_snapshot).await {
                                Ok(content) => {
                                    if renderer.prefers_untruncated_output() {
                                        let mut modal_lines = Vec::new();
                                        modal_lines.push(content.source_label.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.extend(content.config_lines.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.push(MODAL_CLOSE_HINT.to_string());
                                        handle.close_modal();
                                        handle.show_modal(content.title.clone(), modal_lines);
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "Opened {} modal ({}).",
                                                content.title, content.source_label
                                            ),
                                        )?;
                                        renderer.line(MessageStyle::Info, MODAL_CLOSE_HINT)?;
                                    } else {
                                        renderer.line(MessageStyle::Info, &content.source_label)?;
                                        for line in content.config_lines {
                                            renderer.line(MessageStyle::Info, &line)?;
                                        }
                                    }
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to load configuration for display: {}",
                                            err
                                        ),
                                    )?;
                                }
                            }
                            continue;
                        }
                        #[allow(unused_variables)]
                        SlashCommandOutcome::ExecuteTool { name, args: _ } => {
                            // Handle tool execution from slash command
                            match ensure_tool_permission(
                                &mut tool_registry,
                                &name,
                                &mut renderer,
                                &handle,
                                &mut events,
                                default_placeholder.clone(),
                                &ctrl_c_state,
                                &ctrl_c_notify,
                            )
                            .await
                            {
                                Ok(ToolPermissionFlow::Approved) => {
                                    // Tool execution logic
                                    continue;
                                }
                                Ok(ToolPermissionFlow::Denied) => continue,
                                Ok(ToolPermissionFlow::Exit) => break,
                                Ok(ToolPermissionFlow::Interrupted) => break,
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to evaluate policy for tool '{}': {}",
                                            name, err
                                        ),
                                    )?;
                                    continue;
                                }
                            }
                        }
                        SlashCommandOutcome::Exit => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break;
                        }
                    }
                }
                continue;
            }
            _ => {}
        }

        let input = input_owned.as_str();

        let refined_user = refine_user_prompt_if_enabled(input, config, vt_cfg).await;
        // Display the user message with inline border decoration
        display_user_message(&mut renderer, &refined_user)?;
        conversation_history.push(uni::Message::user(refined_user));
        let _pruned_tools = prune_unified_tool_responses(
            &mut conversation_history,
            trim_config.preserve_recent_turns,
        );
        // Removed: Tool response pruning message
        let trim_result = enforce_unified_context_window(&mut conversation_history, trim_config);
        if trim_result.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    trim_result.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        let mut working_history = conversation_history.clone();
        let max_tool_loops = vt_cfg
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

        let mut loop_guard = 0usize;
        let mut any_write_effect = false;
        let mut last_tool_stdout: Option<String> = None;
        let mut bottom_gap_applied = false;

        let turn_result = 'outer: loop {
            if ctrl_c_state.is_cancel_requested() {
                break TurnLoopResult::Cancelled;
            }
            if loop_guard == 0 {
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            loop_guard += 1;
            if loop_guard >= max_tool_loops {
                if !bottom_gap_applied {
                    renderer.line(MessageStyle::Output, "")?;
                }
                let notice = format!(
                    "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                    max_tool_loops
                );
                renderer.line(MessageStyle::Error, &notice)?;
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(notice));
                break TurnLoopResult::Completed;
            }

            let _ = enforce_unified_context_window(&mut working_history, trim_config);

            let decision = if let Some(cfg) = vt_cfg.filter(|cfg| cfg.router.enabled) {
                Router::route_async(cfg, config, &config.api_key, input).await
            } else {
                Router::route(&VTCodeConfig::default(), config, input)
            };
            traj.log_route(
                working_history.len(),
                &decision.selected_model,
                match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                },
                &input.chars().take(120).collect::<String>(),
            );

            let active_model = decision.selected_model;
            let (max_tokens_opt, parallel_cfg_opt) = if let Some(vt) = vt_cfg {
                let key = match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                };
                let budget = vt.router.budgets.get(key);
                let max_tokens = budget.and_then(|b| b.max_tokens).map(|value| value as u32);
                let parallel = budget.and_then(|b| b.max_parallel_tools).map(|value| {
                    vtcode_core::llm::provider::ParallelToolConfig {
                        disable_parallel_tool_use: value <= 1,
                        max_parallel_tools: Some(value),
                        encourage_parallel: value > 1,
                    }
                });
                (max_tokens, parallel)
            } else {
                (None, None)
            };

            let (lg_enabled, lg_max, lg_include) = vt_cfg
                .map(|cfg| {
                    (
                        cfg.context.ledger.enabled,
                        cfg.context.ledger.max_entries,
                        cfg.context.ledger.include_in_prompt,
                    )
                })
                .unwrap_or((true, 12, true));

            ledger.start_turn(
                working_history.len(),
                working_history
                    .last()
                    .map(|message| message.content.clone()),
            );
            let tool_names: Vec<String> = tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect();
            ledger.update_available_tools(tool_names);

            let system_prompt = if lg_enabled && lg_include {
                format!(
                    "{}\n\n[Decision Ledger]\n{}",
                    base_system_prompt,
                    ledger.render_ledger_brief(lg_max)
                )
            } else {
                base_system_prompt.clone()
            };

            let mut attempt_history = working_history.clone();
            let mut retry_attempts = 0usize;
            let (response, response_streamed) = loop {
                retry_attempts += 1;
                let _ = enforce_unified_context_window(&mut attempt_history, trim_config);

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort.as_str().to_string())
                    } else {
                        None
                    }
                });
                let request = uni::LLMRequest {
                    messages: attempt_history.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    tools: Some(tools.clone()),
                    model: active_model.clone(),
                    max_tokens: max_tokens_opt.or(Some(2000)),
                    temperature: Some(0.7),
                    stream: use_streaming,
                    tool_choice: Some(uni::ToolChoice::auto()),
                    parallel_tool_calls: None,
                    parallel_tool_config: parallel_cfg_opt.clone(),
                    reasoning_effort,
                };

                let thinking_spinner =
                    PlaceholderSpinner::new(&handle, default_placeholder.clone(), "Thinking...");
                let mut spinner_active = true;
                task::yield_now().await;
                let result = if use_streaming {
                    let outcome = stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                    )
                    .await;
                    spinner_active = false;
                    outcome
                } else {
                    provider_client
                        .generate(request)
                        .await
                        .map(|resp| (resp, false))
                };

                if spinner_active {
                    thinking_spinner.finish();
                }

                match result {
                    Ok((result, streamed_tokens)) => {
                        working_history = attempt_history.clone();
                        break (result, streamed_tokens);
                    }
                    Err(error) => {
                        if ctrl_c_state.is_cancel_requested() {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        let error_text = error.to_string();
                        if is_context_overflow_error(&error_text)
                            && retry_attempts <= vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT
                        {
                            let removed_tool_messages = prune_unified_tool_responses(
                                &mut attempt_history,
                                trim_config.preserve_recent_turns,
                            );
                            let removed_turns =
                                apply_aggressive_trim_unified(&mut attempt_history, trim_config);
                            let total_removed = removed_tool_messages + removed_turns;
                            if total_removed > 0 {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Context overflow detected; removed {} older messages (retry {}/{}).",
                                        total_removed,
                                        retry_attempts,
                                        vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT,
                                    ),
                                )?;
                                conversation_history.clone_from(&attempt_history);
                                continue;
                            }
                        }

                        let has_tool = working_history
                            .iter()
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_tool {
                            eprintln!("Provider error (suppressed): {error_text}");
                            let reply = derive_recent_tool_output(&working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        } else {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Provider error: {error_text}"),
                            )?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            break 'outer TurnLoopResult::Aborted;
                        }
                    }
                }
            };

            let mut final_text = response.content.clone();
            let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
            let mut interpreted_textual_call = false;

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
                && let Some((name, args)) = detect_textual_tool_call(&text)
            {
                let args_display =
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interpreting textual tool request as {} {}",
                        &name, &args_display
                    ),
                )?;
                let call_id = format!("call_textual_{}", working_history.len());
                tool_calls.push(uni::ToolCall::function(
                    call_id.clone(),
                    name.clone(),
                    args_display.clone(),
                ));
                interpreted_textual_call = true;
                final_text = None;
            }

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
            {
                working_history.push(uni::Message::assistant(text));
            } else {
                let assistant_text = if interpreted_textual_call {
                    String::new()
                } else {
                    final_text.clone().unwrap_or_default()
                };
                working_history.push(uni::Message::assistant_with_tools(
                    assistant_text,
                    tool_calls.clone(),
                ));
                for call in &tool_calls {
                    let name = call.function.name.as_str();
                    let args_val = call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));

                    // Render MCP tool calls as assistant messages instead of user input
                    if name.starts_with("mcp_") {
                        let tool_name = &name[4..]; // Remove "mcp_" prefix
                        let (headline, _) = describe_tool_action(tool_name, &args_val);

                        // Render MCP tool call as a single message block
                        renderer.line(MessageStyle::Info, &format!("→ {}", headline))?;
                        renderer.line(
                            MessageStyle::Info,
                            &format!("MCP: {} → {}", "mcp", tool_name),
                        )?;

                        // Force immediate TUI refresh to ensure proper layout
                        handle.force_redraw();
                        tokio::time::sleep(Duration::from_millis(10)).await;

                        // Also capture for logging
                        {
                            let mut mcp_event = mcp_events::McpEvent::new(
                                "mcp".to_string(),
                                tool_name.to_string(),
                                Some(args_val.to_string()),
                            );
                            mcp_event.success(None);
                            mcp_panel_state.add_event(mcp_event);
                        }
                    } else {
                        render_tool_call_summary(&mut renderer, name, &args_val)?;
                    }
                    let dec_id = ledger.record_decision(
                        format!("Execute tool '{}' to progress task", name),
                        DTAction::ToolCall {
                            name: name.to_string(),
                            args: args_val.clone(),
                            expected_outcome: "Use tool output to decide next step".to_string(),
                        },
                        None,
                    );

                    match ensure_tool_permission(
                        &mut tool_registry,
                        name,
                        &mut renderer,
                        &handle,
                        &mut events,
                        default_placeholder.clone(),
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await
                    {
                        Ok(ToolPermissionFlow::Approved) => {
                            let tool_spinner = PlaceholderSpinner::new(
                                &handle,
                                default_placeholder.clone(),
                                format!("Running tool: {}", name),
                            );

                            // Force TUI refresh to ensure display stability
                            safe_force_redraw(&handle, &mut last_forced_redraw);

                            match tokio::time::timeout(
                                tokio::time::Duration::from_secs(300), // 5 minute timeout for long-running tools
                                tool_registry.execute_tool(name, args_val.clone()),
                            )
                            .await
                            {
                                Ok(Ok(tool_output)) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        true,
                                    );

                                    // Add MCP success message and capture event for logging (only for MCP tools)
                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        // Ensure clean message block for completion
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!("✓ MCP: {} completed", tool_name),
                                        )?;

                                        // Force immediate TUI refresh to ensure proper layout
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        {
                                            let mut mcp_event = mcp_events::McpEvent::new(
                                                "mcp".to_string(),
                                                tool_name.to_string(),
                                                Some(args_val.to_string()),
                                            );
                                            mcp_event.success(None);
                                            mcp_panel_state.add_event(mcp_event);
                                        }
                                    }

                                    render_tool_output(
                                        &mut renderer,
                                        Some(name),
                                        &tool_output,
                                        vt_cfg,
                                    )?;
                                    last_tool_stdout = tool_output
                                        .get("stdout")
                                        .and_then(|value| value.as_str())
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty());
                                    let modified_files: Vec<String> = if let Some(files) =
                                        tool_output
                                            .get("modified_files")
                                            .and_then(|value| value.as_array())
                                    {
                                        files
                                            .iter()
                                            .filter_map(|file| {
                                                file.as_str().map(|value| value.to_string())
                                            })
                                            .collect()
                                    } else {
                                        vec![]
                                    };

                                    if matches!(
                                        name,
                                        "write_file"
                                            | "edit_file"
                                            | "create_file"
                                            | "delete_file"
                                            | "srgn"
                                    ) {
                                        any_write_effect = true;
                                    }

                                    if !modified_files.is_empty()
                                        && confirm_changes_with_git_diff(
                                            &modified_files,
                                            skip_confirmations,
                                        )
                                        .await?
                                    {
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Changes applied successfully.",
                                        )?;
                                    } else if !modified_files.is_empty() {
                                        renderer.line(MessageStyle::Info, "Changes discarded.")?;
                                    }

                                    let content = serde_json::to_string(&tool_output)
                                        .unwrap_or("{}".to_string());
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Success {
                                            result: "tool_ok".to_string(),
                                            metrics: Default::default(),
                                        },
                                    );

                                    if should_short_circuit_shell(input, name, &args_val) {
                                        let reply = last_tool_stdout.clone().unwrap_or_else(|| {
                                            "Command completed successfully.".to_string()
                                        });
                                        renderer.line(MessageStyle::Response, &reply)?;
                                        ensure_turn_bottom_gap(
                                            &mut renderer,
                                            &mut bottom_gap_applied,
                                        )?;
                                        working_history.push(uni::Message::assistant(reply));
                                        let _ = last_tool_stdout.take();
                                        break 'outer TurnLoopResult::Completed;
                                    }
                                }
                                Ok(Err(error)) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    renderer.line(
                                        MessageStyle::Tool,
                                        &format!("Tool {} failed.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    // Add MCP failure as assistant message and capture for logging
                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        // Ensure clean message block for error
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("❌ MCP: {} failed - {}", tool_name, error),
                                        )?;

                                        // Force immediate TUI refresh to ensure proper layout
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        {
                                            let mut mcp_event = mcp_events::McpEvent::new(
                                                "mcp".to_string(),
                                                tool_name.to_string(),
                                                Some(args_val.to_string()),
                                            );
                                            mcp_event.failure(Some(error.to_string()));
                                            mcp_panel_state.add_event(mcp_event);
                                        }
                                    }

                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool error: {error}"),
                                    )?;
                                    let err = serde_json::json!({ "error": error.to_string() });
                                    let content = err.to_string();
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    let _ = last_tool_stdout.take();
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Failure {
                                            error: error.to_string(),
                                            recovery_attempts: 0,
                                            context_preserved: true,
                                        },
                                    );
                                }
                                Err(_timeout) => {
                                    tool_spinner.finish();

                                    // Ensure TUI layout is clean after spinner finishes
                                    handle.force_redraw();
                                    tokio::time::sleep(Duration::from_millis(10)).await;

                                    session_stats.record_tool(name);
                                    // Ensure clean message block for timeout error
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool {} timed out after 5 minutes.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    let timeout_error = ToolExecutionError::new(
                                        name.to_string(),
                                        ToolErrorType::ExecutionError,
                                        "Tool execution timed out after 5 minutes".to_string(),
                                    );
                                    let err_json = serde_json::json!({
                                        "error": timeout_error.message
                                    });
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        err_json.to_string(),
                                    ));
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Failure {
                                            error: "Tool execution timed out after 5 minutes"
                                                .to_string(),
                                            recovery_attempts: 0,
                                            context_preserved: true,
                                        },
                                    );

                                    // Force final TUI refresh after timeout
                                    handle.force_redraw();
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                            }
                        }
                        Ok(ToolPermissionFlow::Denied) => {
                            session_stats.record_tool(name);
                            let denial = ToolExecutionError::new(
                                name.to_string(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            render_tool_output(&mut renderer, Some(name), &denial, vt_cfg)?;
                            let content =
                                serde_json::to_string(&denial).unwrap_or("{}".to_string());
                            working_history
                                .push(uni::Message::tool_response(call.id.clone(), content));
                            ledger.record_outcome(
                                &dec_id,
                                DecisionOutcome::Failure {
                                    error: format!("Tool '{}' execution denied by policy", name),
                                    recovery_attempts: 0,
                                    context_preserved: true,
                                },
                            );
                            continue;
                        }
                        Ok(ToolPermissionFlow::Exit) => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Ok(ToolPermissionFlow::Interrupted) => {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Err(err) => {
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                            let err_json = serde_json::json!({
                                "error": format!(
                                    "Policy evaluation error for '{}' : {}",
                                    name, err
                                )
                            });
                            working_history.push(uni::Message::tool_response(
                                call.id.clone(),
                                err_json.to_string(),
                            ));
                            let _ = last_tool_stdout.take();
                            ledger.record_outcome(
                                &dec_id,
                                DecisionOutcome::Failure {
                                    error: format!(
                                        "Failed to evaluate policy for tool '{}': {}",
                                        name, err
                                    ),
                                    recovery_attempts: 0,
                                    context_preserved: true,
                                },
                            );
                            continue;
                        }
                    }
                }
                continue;
            }

            if let Some(mut text) = final_text.clone() {
                let do_review = vt_cfg
                    .map(|cfg| cfg.agent.enable_self_review)
                    .unwrap_or(false);
                let review_passes = vt_cfg
                    .map(|cfg| cfg.agent.max_review_passes)
                    .unwrap_or(1)
                    .max(1);
                if do_review {
                    let review_system = "You are the agent's critical code reviewer. Improve clarity, correctness, and add missing test or validation guidance. Return only the improved final answer (no meta commentary).".to_string();
                    for _ in 0..review_passes {
                        let review_req = uni::LLMRequest {
                            messages: vec![uni::Message::user(format!(
                                "Please review and refine the following response. Return only the improved response.\n\n{}",
                                text
                            ))],
                            system_prompt: Some(review_system.clone()),
                            tools: None,
                            model: config.model.clone(),
                            max_tokens: Some(2000),
                            temperature: Some(0.5),
                            stream: false,
                            tool_choice: Some(uni::ToolChoice::none()),
                            parallel_tool_calls: None,
                            parallel_tool_config: None,
                            reasoning_effort: vt_cfg.and_then(|cfg| {
                                if provider_client.supports_reasoning_effort(&active_model) {
                                    Some(cfg.agent.reasoning_effort.as_str().to_string())
                                } else {
                                    None
                                }
                            }),
                        };
                        let rr = provider_client.generate(review_req).await.ok();
                        if let Some(r) = rr.and_then(|result| result.content)
                            && !r.trim().is_empty()
                        {
                            text = r;
                        }
                    }
                }
                let trimmed = text.trim();
                let suppress_response = trimmed.is_empty()
                    || last_tool_stdout
                        .as_ref()
                        .map(|stdout| stdout == trimmed)
                        .unwrap_or(false);

                let streamed_matches_output = response_streamed
                    && response
                        .content
                        .as_ref()
                        .map(|original| original == &text)
                        .unwrap_or(false);

                if !suppress_response && !streamed_matches_output {
                    renderer.line(MessageStyle::Response, &text)?;
                }
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(text));
                let _ = last_tool_stdout.take();
            } else {
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
            }
            break TurnLoopResult::Completed;
        };

        match turn_result {
            TurnLoopResult::Cancelled => {
                if ctrl_c_state.is_exit_requested() {
                    break;
                }

                renderer.line_if_not_empty(MessageStyle::Output)?;
                renderer.line(
                    MessageStyle::Info,
                    "Interrupted current task. Press Ctrl+C again to exit.",
                )?;
                handle.clear_input();
                handle.set_placeholder(default_placeholder.clone());
                ctrl_c_state.clear_cancel();
                continue;
            }
            TurnLoopResult::Aborted => {
                let _ = conversation_history.pop();
                continue;
            }
            TurnLoopResult::Completed => {
                conversation_history = working_history;

                let _pruned_after_turn = prune_unified_tool_responses(
                    &mut conversation_history,
                    trim_config.preserve_recent_turns,
                );
                // Removed: Tool response pruning message after completion
                let post_trim =
                    enforce_unified_context_window(&mut conversation_history, trim_config);
                if post_trim.is_trimmed() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                            post_trim.removed_messages, trim_config.max_tokens,
                        ),
                    )?;
                }

                if let Some(last) = conversation_history.last()
                    && last.role == uni::MessageRole::Assistant
                {
                    let text = &last.content;
                    let claims_write = text.contains("I've updated")
                        || text.contains("I have updated")
                        || text.contains("updated the `");
                    if claims_write && !any_write_effect {
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                        renderer.line(
                            MessageStyle::Info,
                            "Note: The assistant mentioned edits but no write tool ran.",
                        )?;
                    }
                }
            }
        }
    }

    let transcript_lines = transcript::snapshot();
    if let Some(archive) = session_archive.take() {
        let distinct_tools = session_stats.sorted_tools();
        let total_messages = conversation_history.len();
        let session_messages: Vec<SessionMessage> = conversation_history
            .iter()
            .map(SessionMessage::from)
            .collect();
        match archive.finalize(
            transcript_lines,
            total_messages,
            distinct_tools,
            session_messages,
        ) {
            Ok(path) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Session saved to {}", path.display()),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to save session: {}", err),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
        }
    }

    // Shutdown MCP client properly before TUI shutdown
    if let Some(mcp_client) = &mcp_client {
        if let Err(e) = mcp_client.shutdown().await {
            let error_msg = e.to_string();
            if error_msg.contains("EPIPE")
                || error_msg.contains("Broken pipe")
                || error_msg.contains("write EPIPE")
            {
                eprintln!(
                    "Info: MCP client shutdown encountered pipe errors (normal): {}",
                    e
                );
            } else {
                eprintln!("Warning: Failed to shutdown MCP client cleanly: {}", e);
            }
        }
    }

    handle.shutdown();
    Ok(())
}

fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    // Rate limit force_redraw calls to prevent TUI corruption
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}
