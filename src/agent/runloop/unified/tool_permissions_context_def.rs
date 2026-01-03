
/// Context for tool permission checks to reduce argument count
pub(crate) struct ToolPermissionsContext<'a, S: UiSession + ?Sized> {
    pub tool_registry: &'a mut ToolRegistry,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut S,
    pub default_placeholder: Option<String>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub hooks: Option<&'a LifecycleHookEngine>,
    pub justification: Option<&'a vtcode_core::tools::ToolJustification>,
    pub approval_recorder: Option<&'a vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger: Option<&'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>>,
    pub tool_permission_cache: Option<&'a Arc<RwLock<ToolPermissionCache>>>,
    pub hitl_notification_bell: bool,
}
