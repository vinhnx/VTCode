#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Clone)]
pub struct PermissionFixture {
    #[serde(rename = "sessionId")]
    #[allow(
        dead_code,
        reason = "Fixture field kept so decoded ACP payloads keep their wire shape."
    )]
    pub session_id: Value,
    #[serde(rename = "toolCall")]
    #[allow(
        dead_code,
        reason = "Fixture field kept so decoded ACP payloads keep their wire shape."
    )]
    pub tool_call: Value,
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
