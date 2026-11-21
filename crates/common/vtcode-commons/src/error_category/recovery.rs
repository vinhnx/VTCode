use super::*;

impl ErrorCategory {
    /// Get recovery suggestions for this error category.
    /// Returns static strings to avoid allocation.
    #[must_use]
    pub fn recovery_suggestions(&self) -> Vec<Cow<'static, str>> {
        match self {
            ErrorCategory::Network => vec![
                Cow::Borrowed("Check network connectivity"),
                Cow::Borrowed("Retry the operation after a brief delay"),
                Cow::Borrowed("Verify external service availability"),
            ],
            ErrorCategory::Timeout => vec![
                Cow::Borrowed("Increase timeout values if appropriate"),
                Cow::Borrowed("Break large operations into smaller chunks"),
                Cow::Borrowed("Check system resources and performance"),
            ],
            ErrorCategory::RateLimit => vec![
                Cow::Borrowed("Wait before retrying the request"),
                Cow::Borrowed("Reduce request frequency"),
                Cow::Borrowed("Check provider rate limit documentation"),
            ],
            ErrorCategory::ServiceUnavailable => vec![
                Cow::Borrowed("The service is temporarily unavailable"),
                Cow::Borrowed("Retry after a brief delay"),
                Cow::Borrowed("Check service status page if available"),
            ],
            ErrorCategory::CircuitOpen => vec![
                Cow::Borrowed("This tool has been temporarily disabled due to repeated failures"),
                Cow::Borrowed("Wait for the circuit breaker cooldown period"),
                Cow::Borrowed("Try an alternative approach"),
            ],
            ErrorCategory::Authentication => vec![
                Cow::Borrowed("Verify your API key or credentials"),
                Cow::Borrowed("Check that your account is active and has sufficient permissions"),
                Cow::Borrowed("Ensure environment variables for API keys are set correctly"),
            ],
            ErrorCategory::InvalidParameters => vec![
                Cow::Borrowed("Check parameter names and types against the tool schema"),
                Cow::Borrowed("Ensure required parameters are provided"),
                Cow::Borrowed("Verify parameter values are within acceptable ranges"),
            ],
            ErrorCategory::ToolNotFound => vec![
                Cow::Borrowed("Verify the tool name is spelled correctly"),
                Cow::Borrowed("Check if the tool is available in the current context"),
            ],
            ErrorCategory::ResourceNotFound => vec![
                Cow::Borrowed("Verify file paths and resource locations"),
                Cow::Borrowed("Check if files exist and are accessible"),
                Cow::Borrowed("Use list_dir to explore available resources"),
            ],
            ErrorCategory::PermissionDenied => vec![
                Cow::Borrowed("Check file permissions and access rights"),
                Cow::Borrowed("Ensure workspace boundaries are respected"),
            ],
            ErrorCategory::PolicyViolation => vec![
                Cow::Borrowed("Review workspace policies and restrictions"),
                Cow::Borrowed("Use alternative tools that comply with policies"),
            ],
            ErrorCategory::PlanningPolicyViolation => vec![
                Cow::Borrowed("This operation is not allowed in the Planning workflow with read-only permissions"),
                Cow::Borrowed("Exit the Planning workflow to perform mutating operations"),
            ],
            ErrorCategory::SandboxFailure => vec![
                Cow::Borrowed("The sandbox denied this operation"),
                Cow::Borrowed("Check sandbox configuration and permissions"),
            ],
            ErrorCategory::ResourceExhausted => vec![
                Cow::Borrowed("Check your account usage limits and billing status"),
                Cow::Borrowed("Review resource consumption and optimize if possible"),
            ],
            ErrorCategory::Cancelled => vec![Cow::Borrowed("The operation was cancelled")],
            ErrorCategory::ExecutionError => vec![
                Cow::Borrowed("Review error details for specific issues"),
                Cow::Borrowed("Check tool documentation for known limitations"),
            ],
        }
    }
}
