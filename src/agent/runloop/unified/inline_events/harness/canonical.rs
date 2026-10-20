use anyhow::Result;
use std::path::Path;

use vtcode_core::core::agent::events::SessionStoreSink;
use vtcode_core::exec::events::ThreadEvent;

/// Authoritative session-event sink for one run.
pub(crate) struct CanonicalEventSink {
    inner: SessionStoreSink,
}

impl CanonicalEventSink {
    pub(crate) fn matrix_persistence(&self) -> vtcode_core::subagents::matrix::MatrixPersistence {
        self.inner.matrix_persistence()
    }
    pub(crate) fn decision_validator(&self) -> vtcode_core::core::agent::events::DecisionEvidenceValidator {
        self.inner.decision_validator()
    }
    pub(crate) async fn explanation(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
    ) -> Result<vtcode_memory::explanation::ExplanationModel> {
        self.inner.explanation(scope).await
    }
    pub(crate) async fn explanation_page(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::ExplanationPage> {
        self.inner.explanation_page(scope, offset).await
    }
    pub(crate) async fn evidence(
        &self,
        reference: vtcode_memory::explanation::EvidenceRef,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::EvidencePage> {
        self.inner.evidence(reference, offset).await
    }
    /// Open the workspace-local canonical session store.
    pub(crate) async fn open(workspace: &Path, session_id: &str) -> Result<Self> {
        Ok(Self {
            inner: SessionStoreSink::open(workspace, session_id).await?,
        })
    }

    /// Enqueue one event while preserving the caller's order.
    pub(crate) fn emit(&self, event: &ThreadEvent) -> Result<()> {
        self.inner.emit(event)
    }

    pub(crate) async fn persist_checkpoint(&self, event: &ThreadEvent) -> Result<()> {
        self.inner.persist_checkpoint(event).await
    }

    pub(crate) async fn diagnostics(&self) -> Result<Vec<vtcode_core::exec::events::Diagnostic>> {
        self.inner.diagnostics().await
    }

    pub(crate) async fn planning_recovery(
        &self,
    ) -> Result<Option<vtcode_core::exec::events::planning::PlanningRecoverySnapshot>> {
        self.inner.planning_recovery().await
    }

    /// Drain all accepted events and report persistence failures.
    pub(crate) async fn close(&self) -> Result<()> {
        self.inner.close().await
    }
}
