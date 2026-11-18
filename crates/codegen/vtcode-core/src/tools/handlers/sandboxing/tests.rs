use super::*;
use std::path::{Path, PathBuf};

#[test]
fn test_external_sandbox_skips_exec_approval_on_request() {
    let result = default_exec_approval_requirement(
        AskForApproval::OnRequest,
        &SandboxConfig {
            policy: SandboxPolicy::ExternalSandbox,
            network_access: NetworkAccess::Restricted,
        },
    );

    assert_eq!(
        result,
        ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        }
    );
}

#[test]
fn test_restricted_sandbox_requires_exec_approval_on_request() {
    let result = default_exec_approval_requirement(
        AskForApproval::OnRequest,
        &SandboxConfig {
            policy: SandboxPolicy::ReadOnly,
            network_access: NetworkAccess::Restricted,
        },
    );

    assert_eq!(result, ExecApprovalRequirement::NeedsApproval { reason: None, proposed_execpolicy_amendment: None });
}

#[test]
fn reject_policy_still_flows_through_handler_wrapper() {
    let policy = AskForApproval::Reject(RejectConfig {
        sandbox_approval: true,
        rules: false,
        request_permissions: false,
        mcp_elicitations: false,
    });

    let requirement = default_exec_approval_requirement(
        policy,
        &SandboxConfig {
            policy: SandboxPolicy::ReadOnly,
            network_access: NetworkAccess::Restricted,
        },
    );

    assert_eq!(
        requirement,
        ExecApprovalRequirement::Forbidden {
            reason: "approval policy rejected sandbox approval prompt".to_string(),
        }
    );
}

#[test]
fn read_only_restricted_maps_to_canonical_read_only() {
    let policy = SandboxConfig {
        policy: SandboxPolicy::ReadOnly,
        network_access: NetworkAccess::Restricted,
    };

    assert_eq!(policy.to_canonical_policy(Path::new("/workspace")), CanonicalSandboxPolicy::read_only());
}

#[test]
fn read_only_full_maps_to_canonical_full_network() {
    let policy = SandboxConfig {
        policy: SandboxPolicy::ReadOnly,
        network_access: NetworkAccess::Full,
    };

    assert_eq!(
        policy.to_canonical_policy(Path::new("/workspace")),
        CanonicalSandboxPolicy::read_only_with_full_network()
    );
}

#[test]
fn workspace_write_restricted_maps_to_canonical_workspace_write() {
    let root = PathBuf::from("/workspace");
    let policy = SandboxConfig {
        policy: SandboxPolicy::WorkspaceWrite,
        network_access: NetworkAccess::Restricted,
    };

    assert_eq!(policy.to_canonical_policy(&root), CanonicalSandboxPolicy::workspace_write(vec![root]));
}

#[test]
fn workspace_write_full_maps_to_canonical_workspace_write_with_network() {
    let root = PathBuf::from("/workspace");
    let policy = SandboxConfig {
        policy: SandboxPolicy::WorkspaceWrite,
        network_access: NetworkAccess::Full,
    };

    assert_eq!(
        policy.to_canonical_policy(&root),
        CanonicalSandboxPolicy::workspace_write_full(
            vec![root],
            Vec::new(),
            None,
            ResourceLimits::default(),
            SeccompProfile::strict().with_network(),
        )
    );
}

#[test]
fn danger_full_access_maps_to_canonical_full_access() {
    let policy = SandboxConfig {
        policy: SandboxPolicy::DangerFullAccess,
        network_access: NetworkAccess::Restricted,
    };

    assert_eq!(policy.to_canonical_policy(Path::new("/workspace")), CanonicalSandboxPolicy::full_access());
}

#[test]
fn external_sandbox_maps_to_canonical_external_sandbox() {
    let policy = SandboxConfig {
        policy: SandboxPolicy::ExternalSandbox,
        network_access: NetworkAccess::Full,
    };

    assert_eq!(
        policy.to_canonical_policy(Path::new("/workspace")),
        CanonicalSandboxPolicy::ExternalSandbox {
            description: LEGACY_EXTERNAL_SANDBOX_DESCRIPTION.to_string(),
        }
    );
}

#[test]
fn limited_network_maps_to_restricted_network() {
    let policy = SandboxConfig {
        policy: SandboxPolicy::WorkspaceWrite,
        network_access: NetworkAccess::Limited,
    };

    assert_eq!(
        policy.to_canonical_policy(Path::new("/workspace")),
        CanonicalSandboxPolicy::workspace_write(vec![PathBuf::from("/workspace")])
    );
}

#[tokio::test]
async fn test_approval_store() {
    let store = ApprovalStore::new();

    assert!(!store.contains("test").await);

    store.set("test".to_string(), ReviewDecision::Approved).await;
    assert!(store.contains("test").await);
    assert_eq!(store.get("test").await, Some(ReviewDecision::Approved));
}

#[test]
fn test_sandbox_manager_platform_selection() {
    let manager = SandboxManager::new();

    // Auto preference respects policy
    let sandbox = manager.select_initial(
        &SandboxConfig {
            policy: SandboxPolicy::DangerFullAccess,
            ..Default::default()
        },
        SandboxablePreference::Auto,
    );
    assert_eq!(sandbox, SandboxType::None);

    // Forbid always returns None
    let sandbox = manager.select_initial(&SandboxConfig::default(), SandboxablePreference::Forbid);
    assert_eq!(sandbox, SandboxType::None);
}

#[test]
fn test_exec_tool_call_output() {
    let output = ExecToolCallOutput {
        stdout: "hello".to_string(),
        stderr: "".to_string(),
        exit_code: 0,
    };
    assert!(output.success());
    assert_eq!(output.combined_output(), "hello");

    let output = ExecToolCallOutput {
        stdout: "out".to_string(),
        stderr: "err".to_string(),
        exit_code: 1,
    };
    assert!(!output.success());
    assert_eq!(output.combined_output(), "out\nerr");
}
