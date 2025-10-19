use agent_client_protocol as acp;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Clone)]
pub struct PermissionFixture {
    #[serde(rename = "sessionId")]
    pub session_id: acp::SessionId,
    #[serde(rename = "toolCall")]
    pub tool_call: acp::ToolCall,
    pub arguments: Value,
}

pub fn read_file_permission() -> PermissionFixture {
    load_fixture(include_str!("fixtures/acp/permission_read_file.json"))
}

pub fn list_files_permission() -> PermissionFixture {
    load_fixture(include_str!("fixtures/acp/permission_list_files.json"))
}

fn load_fixture(contents: &str) -> PermissionFixture {
    serde_json::from_str(contents).expect("invalid ACP permission fixture")
}
