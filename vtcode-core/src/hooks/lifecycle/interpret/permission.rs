use serde_json::Value;

use crate::config::HookCommandConfig;
use crate::config::PermissionMode;
use crate::hooks::lifecycle::types::{
    HookMessage, PermissionDecisionBehavior, PermissionDecisionScope, PermissionRequestHookDecision,
    PermissionRequestHookOutcome, PermissionUpdateDestination, PermissionUpdateKind,
    PermissionUpdateRequest,
};

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit,
    handle_timeout, looks_like_json, matches_hook_event, parse_json_output, trimmed_non_empty,
};

pub(crate) fn interpret_permission_request(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PermissionRequestHookOutcome,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code
        && code != 0
    {
        handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
    }

    if let Some(stderr) = trimmed_non_empty(&result.stderr) {
        outcome.messages.push(HookMessage::warning(format!(
            "PermissionRequest hook `{}` stderr: {}",
            command.command, stderr
        )));
    }

    let Some(json) = parse_json_output(&result.stdout) else {
        if !result.stdout.trim().is_empty() {
            if looks_like_json(&result.stdout) {
                outcome.messages.push(HookMessage::error(format!(
                    "PermissionRequest hook `{}` returned invalid JSON output",
                    command.command
                )));
            } else if allow_plain_success_stdout(result, quiet_success_output) {
                outcome
                    .messages
                    .push(HookMessage::info(result.stdout.trim().to_owned()));
            }
        }
        return;
    };

    let common = extract_common_fields(&json, &mut outcome.messages);
    let Some(Value::Object(spec)) = common.hook_specific else {
        return;
    };
    if !matches_hook_event(&spec, "PermissionRequest") {
        return;
    }

    let decision_value = spec.get("decision");
    let behavior = match decision_value
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("behavior"))
        .and_then(Value::as_str)
    {
        Some(value) if value.eq_ignore_ascii_case("allow") => Some(PermissionDecisionBehavior::Allow),
        Some(value) if value.eq_ignore_ascii_case("deny") => Some(PermissionDecisionBehavior::Deny),
        _ => None,
    };

    if let Some(message) = spec
        .get("message")
        .and_then(Value::as_str)
        .and_then(trimmed_non_empty)
    {
        outcome.messages.push(HookMessage::info(message));
    }

    let Some(behavior) = behavior else {
        return;
    };

    let updated_input = spec.get("updatedInput").cloned();
    let permission_updates = parse_permission_updates(spec.get("updatedPermissions"));
    let mut scope = PermissionDecisionScope::Once;
    for update in &permission_updates {
        match update.destination {
            PermissionUpdateDestination::Session => {}
            PermissionUpdateDestination::ProjectSettings => {
                scope = PermissionDecisionScope::Permanent;
                break;
            }
            PermissionUpdateDestination::Unsupported(_) => {}
        }
        if matches!(update.destination, PermissionUpdateDestination::Session)
            && scope != PermissionDecisionScope::Permanent
        {
            scope = PermissionDecisionScope::Session;
        }
    }

    let interrupt = decision_value
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("interrupt"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    outcome.decision = Some(PermissionRequestHookDecision {
        behavior,
        scope,
        updated_input,
        permission_updates,
        interrupt,
    });
}

fn parse_permission_updates(value: Option<&Value>) -> Vec<PermissionUpdateRequest> {
    let Some(value) = value else {
        return Vec::new();
    };

    let updates = match value {
        Value::Array(entries) => entries.iter().collect::<Vec<_>>(),
        other => vec![other],
    };

    updates
        .into_iter()
        .filter_map(parse_permission_update)
        .collect()
}

fn parse_permission_update(value: &Value) -> Option<PermissionUpdateRequest> {
    let object = value.as_object()?;
    let destination = match object.get("destination").and_then(Value::as_str) {
        Some("session") => PermissionUpdateDestination::Session,
        Some("projectSettings") => PermissionUpdateDestination::ProjectSettings,
        Some(other) => PermissionUpdateDestination::Unsupported(other.to_owned()),
        None => PermissionUpdateDestination::Session,
    };

    if let Some(rules) = parse_rules(object.get("addRules")) {
        return Some(PermissionUpdateRequest {
            destination,
            kind: PermissionUpdateKind::AddRules(rules),
        });
    }

    if let Some(rules) = parse_rules(object.get("replaceRules")) {
        return Some(PermissionUpdateRequest {
            destination,
            kind: PermissionUpdateKind::ReplaceRules(rules),
        });
    }

    if let Some(rules) = parse_rules(object.get("removeRules")) {
        return Some(PermissionUpdateRequest {
            destination,
            kind: PermissionUpdateKind::RemoveRules(rules),
        });
    }

    if let Some(mode) = object
        .get("setMode")
        .cloned()
        .and_then(|raw| serde_json::from_value::<PermissionMode>(raw).ok())
    {
        return Some(PermissionUpdateRequest {
            destination,
            kind: PermissionUpdateKind::SetMode(mode),
        });
    }

    let unsupported_key = ["localSettings", "userSettings", "addDirectories", "removeDirectories"]
        .into_iter()
        .find(|key| object.contains_key(*key))
        .map(str::to_string)
        .or_else(|| object.keys().next().cloned())?;

    Some(PermissionUpdateRequest {
        destination,
        kind: PermissionUpdateKind::Unsupported(unsupported_key),
    })
}

fn parse_rules(value: Option<&Value>) -> Option<Vec<String>> {
    let array = value?.as_array()?;
    let rules = array
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!rules.is_empty()).then_some(rules)
}
