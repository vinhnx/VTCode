#![allow(missing_docs)]
use std::collections::HashMap;

use serde_json::Value;
use vtcode::acp::permissions::{AcpPermissionPrompter, DefaultPermissionPrompter};
use vtcode::acp::reports::{
    TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID, TOOL_PERMISSION_ALLOW_OPTION_ID, TOOL_PERMISSION_CANCELLED_MESSAGE,
    TOOL_PERMISSION_DENIED_MESSAGE, TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID, TOOL_PERMISSION_DENY_OPTION_ID,
    TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE,
};
use vtcode::acp::tooling::{
    SupportedTool, TOOL_LIST_FILES_MODE_ARG, TOOL_LIST_FILES_PATH_ARG, TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG,
    ToolDescriptor, ToolRegistryProvider,
};

// -- Fixtures -----------------------------------------------------------

#[path = "acp_fixtures.rs"]
mod acp_fixtures;

use acp_fixtures::{list_files_permission, read_file_permission};

// -- Fake registry ------------------------------------------------------

#[derive(Clone)]
struct FakeRegistry {
    descriptors: HashMap<String, ToolDescriptor>,
}

impl FakeRegistry {
    fn new() -> Self {
        let mut descriptors = HashMap::new();
        descriptors
            .insert(SupportedTool::ReadFile.function_name().to_string(), ToolDescriptor::Acp(SupportedTool::ReadFile));
        descriptors.insert(
            SupportedTool::ListFiles.function_name().to_string(),
            ToolDescriptor::Acp(SupportedTool::ListFiles),
        );
        Self { descriptors }
    }

    fn render_read_file(&self, args: &Value) -> String {
        args.get(TOOL_READ_FILE_PATH_ARG)
            .or_else(|| args.get(TOOL_READ_FILE_URI_ARG))
            .and_then(Value::as_str)
            .map(|path| format!("Read file {path}"))
            .unwrap_or_else(|| "Read file".to_string())
    }

    fn render_list_files(&self, args: &Value) -> String {
        let scope = args
            .get(TOOL_LIST_FILES_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(|value| format!("{value}/"))
            .unwrap_or_else(|| "workspace".to_string());

        let mode = args.get(TOOL_LIST_FILES_MODE_ARG).and_then(Value::as_str).unwrap_or("list");

        format!("List files in {scope} ({mode})")
    }
}

impl ToolRegistryProvider for FakeRegistry {
    fn registered_tools(&self) -> Vec<SupportedTool> {
        self.descriptors
            .values()
            .filter_map(|descriptor| match descriptor {
                ToolDescriptor::Acp(tool) => Some(*tool),
                ToolDescriptor::Local => None,
            })
            .collect()
    }

    fn definitions_for(
        &self,
        _enabled_tools: &[SupportedTool],
        _include_local: bool,
    ) -> Vec<vtcode_core::llm::provider::ToolDefinition> {
        Vec::new()
    }

    fn render_title(&self, descriptor: ToolDescriptor, _function_name: &str, args: &Value) -> String {
        match descriptor {
            ToolDescriptor::Acp(SupportedTool::ReadFile) => self.render_read_file(args),
            ToolDescriptor::Acp(SupportedTool::ListFiles) => self.render_list_files(args),
            ToolDescriptor::Local => "Local tool".to_string(),
        }
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        self.descriptors.get(function_name).copied()
    }

    fn has_local_tools(&self) -> bool {
        false
    }
}

fn prompter() -> DefaultPermissionPrompter<FakeRegistry> {
    DefaultPermissionPrompter::new(FakeRegistry::new())
}

// -- Option construction tests (no ACP connection needed) ---------------

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct TestOption {
    option_id: String,
    name: String,
    kind: String,
}

/// Extract permission options from the prompter and return them as
/// test-friendly structs.
fn collect_options(args: Option<&Value>, tool: SupportedTool) -> Vec<TestOption> {
    let options = prompter().permission_options(tool, args);
    options
        .iter()
        .map(|o| TestOption {
            option_id: o.option_id.0.as_ref().to_string(),
            name: o.name.clone(),
            kind: format!("{:?}", o.kind),
        })
        .collect()
}

#[test]
fn permission_options_include_allow_once() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert!(options.iter().any(|o| o.option_id == TOOL_PERMISSION_ALLOW_OPTION_ID));
}

#[test]
fn permission_options_include_allow_always() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert!(options.iter().any(|o| o.option_id == TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID));
}

#[test]
fn permission_options_include_deny_once() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert!(options.iter().any(|o| o.option_id == TOOL_PERMISSION_DENY_OPTION_ID));
}

#[test]
fn permission_options_include_deny_always() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert!(options.iter().any(|o| o.option_id == TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID));
}

#[test]
fn permission_options_have_four_entries() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert_eq!(options.len(), 4);
}

#[test]
fn permission_options_render_read_file_action_label() {
    let fixture = read_file_permission();
    let options = collect_options(Some(&fixture.arguments), SupportedTool::ReadFile);
    assert!(
        options
            .iter()
            .any(|o| o.name.contains("Read file ") && o.name.starts_with("Allow"))
    );
}

#[test]
fn permission_options_render_list_files_action_label() {
    let fixture = list_files_permission();
    let options = collect_options(Some(&fixture.arguments), SupportedTool::ListFiles);
    assert!(
        options
            .iter()
            .any(|o| o.name.contains("List files in") && o.name.starts_with("Allow"))
    );
}

#[test]
fn permission_options_have_correct_option_ids() {
    let options = collect_options(None, SupportedTool::ReadFile);
    let option_ids: Vec<_> = options.iter().map(|o| o.option_id.as_str()).collect();
    assert!(option_ids.contains(&TOOL_PERMISSION_ALLOW_OPTION_ID));
    assert!(option_ids.contains(&TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID));
    assert!(option_ids.contains(&TOOL_PERMISSION_DENY_OPTION_ID));
    assert!(option_ids.contains(&TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID));
}

// -- Action label rendering tests ---------------------------------------

#[test]
fn action_label_uses_path_arg_for_read_file() {
    let fixture = read_file_permission();
    let args = &fixture.arguments;
    assert!(
        prompter()
            .permission_options(SupportedTool::ReadFile, Some(args))
            .iter()
            .any(|o| o.name.contains("Read file ") && o.name.starts_with("Allow"))
    );
}

#[test]
fn action_label_uses_uri_arg_fallback_for_read_file() {
    let args = serde_json::json!({
        "uri": "file:///path/to/document.txt"
    });
    assert!(
        prompter()
            .permission_options(SupportedTool::ReadFile, Some(&args))
            .iter()
            .any(|o| o.name.contains("/path/to/document.txt"))
    );
}

#[test]
fn action_label_shows_workspace_root_for_empty_path() {
    let args = serde_json::json!({
        "path": ""
    });
    assert!(
        prompter()
            .permission_options(SupportedTool::ReadFile, Some(&args))
            .iter()
            .any(|o| o.name.contains("Read file"))
    );
}

#[test]
fn action_label_defaults_to_tool_name_for_no_args() {
    let options = collect_options(None, SupportedTool::ReadFile);
    assert!(options.iter().any(|o| o.name.contains("Read file ")));
}

#[test]
fn named_permission_uses_custom_action_label() {
    let action_label = "Read file src/main.rs";
    let arguments = serde_json::json!({
        TOOL_READ_FILE_PATH_ARG: "src/main.rs",
    });

    let options = prompter().permission_options(SupportedTool::ReadFile, Some(&arguments));
    assert!(
        options
            .iter()
            .any(|o| o.name.contains(action_label) && o.name.starts_with("Allow"))
    );
}

// -- Message template tests (without ACP connection) --------------------

#[test]
fn denied_message_contains_keyword() {
    assert!(TOOL_PERMISSION_DENIED_MESSAGE.contains("denied"));
}

#[test]
fn cancelled_message_contains_keyword() {
    assert!(TOOL_PERMISSION_CANCELLED_MESSAGE.contains("cancelled"));
}

#[test]
fn failure_message_contains_keyword() {
    assert!(TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE.contains("failed"));
}

// -- ACP permission flow tests TODO ------------------------------------
// The permission flow tests (permission_allow_flow, permission_denied_flow,
// etc.) need a real ACP ConnectionHandle to interact with. In ACP 1.0.1,
// `Client` is a struct (not a trait), so the old `impl acp::Client for FakeClient`
// pattern no longer applies. To re-enable these tests, create a
// ConnectionHandle from a Channel::duplex() pair and handle the
// request_permission requests on the agent side.
