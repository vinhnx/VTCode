use anyhow::Result;

use crate::agent::runloop::ResumeSession;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum NextRuntimeArchiveId {
    Existing(String),
    Reserve {
        workspace_label: String,
        custom_suffix: Option<String>,
    },
}

pub(super) async fn create_session_archive(
    metadata: vtcode_core::utils::session_archive::SessionArchiveMetadata,
    reserved_archive_id: Option<String>,
) -> Result<vtcode_core::utils::session_archive::SessionArchive> {
    if let Some(identifier) = reserved_archive_id {
        return vtcode_core::utils::session_archive::SessionArchive::new_with_identifier(
            metadata, identifier,
        )
        .await;
    }

    vtcode_core::utils::session_archive::SessionArchive::new(metadata, None).await
}

pub(super) fn workspace_archive_label(workspace: &std::path::Path) -> String {
    workspace
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string())
}

pub(super) fn next_runtime_archive_id_request(
    workspace: &std::path::Path,
    resume: Option<&ResumeSession>,
) -> NextRuntimeArchiveId {
    if let Some(resume) = resume {
        if !resume.is_fork() {
            return NextRuntimeArchiveId::Existing(resume.identifier());
        }

        return NextRuntimeArchiveId::Reserve {
            workspace_label: workspace_archive_label(workspace),
            custom_suffix: resume.custom_suffix().map(str::to_owned),
        };
    }

    NextRuntimeArchiveId::Reserve {
        workspace_label: workspace_archive_label(workspace),
        custom_suffix: None,
    }
}

pub(super) async fn refresh_runtime_debug_context_for_next_session(
    workspace: &std::path::Path,
    resume: Option<&ResumeSession>,
) -> Result<()> {
    let session_id = match next_runtime_archive_id_request(workspace, resume) {
        NextRuntimeArchiveId::Existing(identifier) => identifier,
        NextRuntimeArchiveId::Reserve {
            workspace_label,
            custom_suffix,
        } if vtcode_core::utils::session_archive::history_persistence_enabled() => {
            vtcode_core::utils::session_archive::reserve_session_archive_identifier(
                &workspace_label,
                custom_suffix,
            )
            .await?
        }
        NextRuntimeArchiveId::Reserve {
            workspace_label,
            custom_suffix,
        } => vtcode_core::utils::session_archive::generate_session_archive_identifier(
            &workspace_label,
            custom_suffix,
        ),
    };

    crate::main_helpers::configure_runtime_debug_context(session_id.clone(), Some(session_id));
    Ok(())
}
