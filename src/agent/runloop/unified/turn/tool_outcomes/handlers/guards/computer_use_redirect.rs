//! Guard redirecting headless-browser screenshots to `computer_use`.
//!
//! When computer use is runnable in this session (advertised, enabled,
//! target configured, provider capable), a shell screenshot of a web page
//! is always the wrong tool: the approved isolated browser must handle it.
//! The guard blocks the shell call with a steering error so the model
//! re-issues the task as `computer_use { task }`. When computer use cannot
//! run, shell screenshots pass through untouched (graceful degradation).
//!
//! Unlike sibling guards this one never activates recovery: the model is
//! expected to retry immediately with the right tool, not wind down.
//!
//! Escape hatch: the master switch is human-operated. A session that needs
//! raw headless output disables computer use (or uses a profile without
//! the tool) and this guard goes quiet.

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::command_args::command_text;
use vtcode_core::tools::registry::labels::tool_action_label;

use super::super::ValidationResult;
use super::common::push_guard_failure_messages;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Headless-browser screenshot flag shared by Chromium (`--headless
/// --screenshot=...`) and Firefox (`--headless --screenshot ...`).
/// Case-sensitive on purpose: real flags are lowercase, and matching
/// `--SCREENSHOT` would only add false positives.
const SCREENSHOT_FLAG: &str = "--screenshot";

/// Whether this exec call must be redirected to `computer_use`.
///
/// Shared predicate used by the guard and its tests: blocking requires
/// ALL conditions, so a session that cannot run computer use never loses
/// shell screenshots. Each condition is independently falsifiable.
fn should_redirect_to_computer_use(
    command: &str,
    computer_use_advertised: bool,
    computer_use_enabled: bool,
    browser_target_configured: bool,
    provider_supports_computer_use: bool,
) -> bool {
    command.contains(SCREENSHOT_FLAG)
        && computer_use_advertised
        && computer_use_enabled
        && browser_target_configured
        && provider_supports_computer_use
}

/// Build the steering error content for a redirected screenshot call.
#[cold]
fn build_redirect_error_content() -> String {
    super::super::super::execution_result::build_error_content(
        "Blocked: headless-browser screenshots must use the computer_use tool in this session. Call `computer_use` with the task in plain words (e.g. task: 'screenshot https://example.com') instead of exec_command. Nothing ran."
            .to_string(),
        None,
        None,
        "computer_use_redirect",
    )
    .to_string()
}

/// Enforce the computer-use redirect guard.
///
/// Returns `Some(ValidationResult::Blocked)` with a steering error when an
/// `exec_command` call carries a headless-browser screenshot flag while
/// computer use is runnable; otherwise `None` (pass through).
pub(crate) async fn enforce_computer_use_redirect_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    args: &Value,
) -> Option<ValidationResult> {
    if canonical_tool_name != tool_names::EXEC_COMMAND {
        return None;
    }
    let command = command_text(args).ok().flatten()?;
    // Fast path: skip the advertised-tools lock when no screenshot flag.
    if !command.contains(SCREENSHOT_FLAG) {
        return None;
    }
    let advertised = ctx.tools.read().await.iter().any(|tool| {
        tool.function
            .as_ref()
            .is_some_and(|function| function.name == tool_names::COMPUTER_USE)
    });
    let enabled = ctx.vt_cfg.is_some_and(|cfg| cfg.computer_use.enabled);
    let target_configured = ctx.vt_cfg.is_some_and(|cfg| cfg.computer_use.browser_build.is_some());
    let provider_capable = ctx.provider_client.supports_computer_use();
    if !should_redirect_to_computer_use(&command, advertised, enabled, target_configured, provider_capable) {
        return None;
    }

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Computer-use redirect guard stopped '{display_tool}': headless-browser screenshots must use the computer_use tool while it is runnable. Re-issue the task as computer_use instead of shelling around."
    );
    tracing::warn!(tool = %canonical_tool_name, "Redirecting shell screenshot to computer_use");
    // Deliberately no `activate_recovery`: the model must retry with
    // computer_use, and recovery mode would forbid further tool calls.
    push_guard_failure_messages(ctx, tool_call_id, canonical_tool_name, build_redirect_error_content(), &block_reason);

    Some(ValidationResult::Blocked)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::llm::provider as uni;

    use super::should_redirect_to_computer_use;

    const CHROME_SCREENSHOT: &str = "'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' --headless --screenshot=apple.com.png https://www.apple.com";
    const FIREFOX_SCREENSHOT: &str = "firefox --headless --screenshot out.png https://example.com";

    #[test]
    fn redirect_matrix_covers_flag_and_runnable_boundaries() {
        // (command, advertised, enabled, target, provider, expected)
        let cases: &[(&str, bool, bool, bool, bool, bool)] = &[
            (CHROME_SCREENSHOT, true, true, true, true, true),
            (FIREFOX_SCREENSHOT, true, true, true, true, true),
            // Compound commands are blocked whole: the model must split
            // non-screenshot steps from the computer_use task.
            ("cd /tmp && chrome --headless --screenshot=x.png https://a.com", true, true, true, true, true),
            ("ls -la /tmp", true, true, true, true, false),
            (CHROME_SCREENSHOT, false, true, true, true, false),
            (CHROME_SCREENSHOT, true, false, true, true, false),
            (CHROME_SCREENSHOT, true, true, false, true, false),
            (CHROME_SCREENSHOT, true, true, true, false, false),
            // Near-miss flag shapes must not redirect.
            ("chrome --headless --SCREENSHOT=x.png https://a.com", true, true, true, true, false),
            ("echo myscreenshot", true, true, true, true, false),
            ("grep screenshot log.txt", true, true, true, true, false),
        ];
        for (command, advertised, enabled, target, provider, expected) in cases {
            assert_eq!(
                should_redirect_to_computer_use(command, *advertised, *enabled, *target, *provider),
                *expected,
                "command={command:?} advertised={advertised} enabled={enabled} target={target} provider={provider}"
            );
        }
    }

    struct CuaCapableStubProvider;

    #[async_trait::async_trait]
    impl uni::LLMProvider for CuaCapableStubProvider {
        fn name(&self) -> &str {
            "cua-capable-stub"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            Ok(uni::LLMResponse {
                content: Some(String::new()),
                model: "stub-model".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }

        fn supports_computer_use(&self) -> bool {
            true
        }
    }

    async fn advertise_computer_use(
        ctx: &mut crate::agent::runloop::unified::turn::context::TurnProcessingContext<'_>,
    ) {
        ctx.tools.write().await.push(uni::ToolDefinition::function(
            "computer_use".to_string(),
            "stub".to_string(),
            json!({"type": "object"}),
        ));
    }

    #[tokio::test]
    async fn validation_blocks_shell_screenshot_when_cua_runnable() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        advertise_computer_use(&mut ctx).await;
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(CuaCapableStubProvider);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "redirect-1",
            "exec_command",
            &json!({"cmd": CHROME_SCREENSHOT}),
        )
        .await
        .expect("validate");
        assert!(matches!(result, super::super::super::ValidationResult::Blocked));
    }

    #[tokio::test]
    async fn validation_passes_shell_screenshot_when_cua_disabled() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        advertise_computer_use(&mut ctx).await;
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = false;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(CuaCapableStubProvider);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "redirect-2",
            "exec_command",
            &json!({"cmd": CHROME_SCREENSHOT}),
        )
        .await
        .expect("validate");
        assert!(!matches!(result, super::super::super::ValidationResult::Blocked));
    }

    #[tokio::test]
    async fn validation_passes_shell_screenshot_when_cua_not_advertised() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        // Tools list stays empty: no computer_use, no redirect, no
        // block without recourse.
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(CuaCapableStubProvider);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "redirect-3",
            "exec_command",
            &json!({"cmd": CHROME_SCREENSHOT}),
        )
        .await
        .expect("validate");
        assert!(!matches!(result, super::super::super::ValidationResult::Blocked));
    }
}
