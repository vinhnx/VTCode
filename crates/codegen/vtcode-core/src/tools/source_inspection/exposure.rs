use super::*;
use crate::config::constants::tools;
use crate::llm::provider::{Message, MessageRole};
use crate::tools::registry::ToolRegistry;

fn output_text(value: &Value) -> Option<&str> {
    ["content", "output", "stdout"]
        .iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
}

fn read_support(name: &str, args: &Value, output: &Value) -> Option<String> {
    // Fingerprint the actual returned source, not decorative output metadata.
    let digest = if name == tools::CODE_SEARCH {
        revision(&output.get("results")?.to_string())
    } else {
        revision(output_text(output)?)
    };
    Some(format!("{}:{digest}", call_key(name, args)))
}

struct RetainedSignatures {
    by_call_id: HashMap<String, Vec<String>>,
    call_ids: HashSet<String>,
}

struct SourceReadTarget {
    path: PathBuf,
    raw_start: Option<usize>,
}

fn retained_signatures(messages: &[Message]) -> RetainedSignatures {
    let calls: HashMap<_, _> = messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .flat_map(|message| message.tool_calls.iter().flatten())
        .map(|call| (call.id.as_str(), call))
        .collect();
    let mut signatures = HashMap::new();
    let mut current_calls = HashSet::new();
    for message in messages.iter().filter(|message| message.role == MessageRole::Tool) {
        let Some(call) = message.tool_call_id.as_deref().and_then(|id| calls.get(id)) else {
            continue;
        };
        current_calls.insert(call.id.clone());
        let Some(function) = call.function.as_ref() else {
            continue;
        };
        let text = message.content.as_text();
        let Ok(output) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if output.get("cleared").and_then(Value::as_str) == Some("tool_result")
            || output.get("error").is_some()
            || output.get("success").and_then(Value::as_bool) == Some(false)
            || output.get("conflict_detected").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let args = serde_json::from_str::<Value>(&function.arguments)
            .unwrap_or_else(|_| Value::String(function.arguments.clone()));
        let mut call_signatures = vec![call_key(&function.name, &args)];
        if let Some(support) = read_support(&function.name, &args, &output) {
            call_signatures.push(support);
        }
        signatures.insert(call.id.clone(), call_signatures);
    }
    RetainedSignatures { by_call_id: signatures, call_ids: current_calls }
}

impl SourceInspection {
    /// Revoke evidence whose exact source output or authored patch is no longer
    /// present. This must receive the normalized, locally cleared request view.
    pub fn reconcile(&self, messages: &[Message]) {
        let RetainedSignatures { by_call_id: signatures, call_ids: current_calls } = retained_signatures(messages);
        let mut ledger = self.0.lock();
        let previous_calls = ledger.previous_calls.clone();
        let mut active = HashSet::new();
        for (key, support) in &mut ledger.supports {
            if support.call_id.is_none() {
                support.call_id = signatures
                    .iter()
                    .find(|(id, values)| !previous_calls.contains(*id) && values.contains(&support.signature))
                    .map(|(id, _)| id.clone());
            }
            if support
                .call_id
                .as_ref()
                .and_then(|id| signatures.get(id))
                .is_some_and(|values| values.contains(&support.signature))
            {
                active.insert(key.clone());
            }
        }
        ledger.previous_calls = current_calls;
        ledger.active = active;
        let active = ledger.active.clone();
        for file in ledger.files.values_mut() {
            file.lines
                .retain(|_, credit| credit.iter().all(|support| active.contains(support)));
            if file
                .empty
                .as_ref()
                .is_some_and(|credit| !credit.iter().all(|support| active.contains(support)))
            {
                file.empty = None;
            }
        }
    }
    /// Revoke only evidence affected by a retry projection. Unsent earlier
    /// server continuation context remains available and is not reconstructed.
    pub fn reconcile_projection_change(&self, before: &[Message], after: &[Message]) {
        let affected = retained_signatures(before).call_ids;
        let signatures = retained_signatures(after).by_call_id;
        let mut ledger = self.0.lock();
        let mut active = ledger.active.clone();
        active.retain(|key| {
            ledger.supports.get(key).is_some_and(|support| {
                support.call_id.as_ref().is_some_and(|id| {
                    !affected.contains(id)
                        || signatures.get(id).is_some_and(|values| values.contains(&support.signature))
                })
            })
        });
        ledger.active = active.clone();
        for file in ledger.files.values_mut() {
            file.lines.retain(|_, credit| credit.iter().all(|key| active.contains(key)));
            if file
                .empty
                .as_ref()
                .is_some_and(|credit| !credit.iter().all(|key| active.contains(key)))
            {
                file.empty = None;
            }
        }
    }
}

impl ToolRegistry {
    /// Reconcile against the final request projection; never modifies history.
    pub fn reconcile_source_inspection(&self, messages: &[Message]) {
        self.edited_file_monitor_ref().source_inspection().reconcile(messages);
    }

    pub fn reconcile_source_projection_change(&self, before: &[Message], after: &[Message]) {
        self.edited_file_monitor_ref()
            .source_inspection()
            .reconcile_projection_change(before, after);
    }

    pub fn set_inspection_mode(&self, mode: InspectionMode) {
        self.edited_file_monitor_ref().source_inspection().set_mode(mode);
    }

    pub fn inspection_mode(&self) -> InspectionMode {
        self.edited_file_monitor_ref().source_inspection().mode()
    }

    pub(crate) async fn observe_source_output(&self, name: &str, args: &Value, output: &Value) {
        let inspection = self.edited_file_monitor_ref().source_inspection();
        if inspection.mode() == InspectionMode::Off || output.get("error").is_some() {
            return;
        }
        if name == tools::CODE_SEARCH {
            let Some(support) = read_support(name, args, output) else {
                return;
            };
            if let Some(results) = output.get("results").and_then(Value::as_array) {
                for result in results {
                    let (Some(path), Some(line), Some(snippet)) = (
                        result.get("path").and_then(Value::as_str),
                        result.get("line").and_then(Value::as_u64),
                        result.get("snippet").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    let Ok(path) = self.file_ops_tool().normalize_user_path(path).await else {
                        continue;
                    };
                    let Ok(content) = tokio::fs::read_to_string(&path).await else {
                        continue;
                    };
                    let source_lines: Vec<_> = content.lines().collect();
                    let Some(start) = usize::try_from(line).ok().and_then(|line| line.checked_sub(1)) else {
                        continue;
                    };
                    let exposed = snippet
                        .lines()
                        .enumerate()
                        .filter_map(|(offset, text)| {
                            let index = start + offset;
                            (source_lines.get(index).copied() == Some(text)).then_some(index)
                        })
                        .collect();
                    let support = inspection.register_support(support.clone());
                    inspection.insert_lines(path, &content, exposed, support, false);
                }
            }
            return;
        }
        let Some(text) = output_text(output) else { return };
        let Some(support) = read_support(name, args, output) else {
            return;
        };
        let file_read = name == tools::READ_FILE
            || (name == tools::UNIFIED_FILE && crate::tools::tool_intent::file_operation_action(args) == Some("read"));
        let target = if file_read {
            output.get("path").and_then(Value::as_str).map(|path| SourceReadTarget {
                path: PathBuf::from(path),
                raw_start: crate::tools::file_ops::source_line_offset(args),
            })
        } else {
            self.verbatim_shell_read(name, args)
        };
        let Some(SourceReadTarget { path, raw_start }) = target else {
            return;
        };
        let Some(path_text) = path.to_str() else { return };
        let Ok(path) = self.file_ops_tool().normalize_user_path(path_text).await else {
            return;
        };
        // Comparison validates completeness and positioning, but is never itself
        // exposure. Capture the revision now, before another request can edit it.
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            return;
        };
        let source_lines: Vec<_> = content.lines().collect();
        let lines = if let Some(start) = raw_start {
            text.lines()
                .enumerate()
                .map_while(|(index, line)| {
                    let index = start + index;
                    (source_lines.get(index).copied() == Some(line)).then_some(index)
                })
                .collect()
        } else if text == content
            || crate::tools::file_ops::restore_exact_text_content(text, content.len() as u64).as_deref()
                == Some(&content)
        {
            (0..source_lines.len()).collect()
        } else {
            text.lines()
                .filter_map(|line| {
                    let (number, body) = line.split_once(": ")?;
                    let index = number.parse::<usize>().ok()?.checked_sub(1)?;
                    (source_lines.get(index).copied() == Some(body)).then_some(index)
                })
                .collect()
        };
        let empty = content.is_empty() && text.is_empty() && file_read;
        let support = inspection.register_support(support);
        inspection.insert_lines(path, &content, lines, support, empty);
    }

    fn verbatim_shell_read(&self, name: &str, args: &Value) -> Option<SourceReadTarget> {
        if !crate::tools::tool_intent::is_command_run_tool_call(name, args) {
            return None;
        }
        let words = crate::tools::command_args::command_words(args).ok()??;
        let (path, start) = match words.as_slice() {
            [command, path] if command == "cat" && !path.starts_with('-') => (path, 0),
            [command, option, script, path] if command == "sed" && option == "-n" => {
                let range = script.strip_suffix('p')?;
                let (start, end) = range.split_once(',').unwrap_or((range, range));
                let start = start.parse::<usize>().ok()?.checked_sub(1)?;
                let end = end.parse::<usize>().ok()?;
                if end <= start {
                    return None;
                }
                (path, start)
            }
            _ => return None,
        };
        let cwd = self
            .pty_manager()
            .working_dir_candidate(crate::tools::command_args::working_dir_text(args))
            .ok()?;
        Some(SourceReadTarget { path: cwd.join(path), raw_start: Some(start) })
    }
}
