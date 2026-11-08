//! Shared model-visible tool-output preview limits.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use vtcode_utility_tool_specs::{
    DEFAULT_MAX_OUTPUT_TOKENS, MAX_MAX_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS_FIELD, MIN_MAX_OUTPUT_TOKENS,
};

/// Default per-result preview budget for plan-mode inspections that omit an
/// explicit `max_output_tokens` (`2_000` tokens ≈ 8 KiB). Planning research
/// fans out across many reads, so the smaller default keeps a dozen previews
/// inside the `96 KiB` plan turn budget. Explicit large values on
/// non-verification calls are clamped to [`PLAN_MODE_MAX_OUTPUT_TOKENS`];
/// verification commands are never clamped.
pub(crate) const PLAN_MODE_DEFAULT_MAX_OUTPUT_TOKENS: usize = 2_000;

/// Maximum per-result preview budget for plan-mode non-verification calls
/// (`4_000` tokens ≈ 16 KiB). Session `session-vtcode-20260923T065602Z`
/// requested `12_000-30_000` tokens per inspection, exhausting the `96 KiB`
/// turn budget after two previews and forcing stubbed retries. Clamping keeps
/// at least six previews visible before exhaustion while spool paging (with
/// preview credit) stays available for full content.
pub(crate) const PLAN_MODE_MAX_OUTPUT_TOKENS: usize = 4_000;

/// Maximum per-result preview budget for execution-mode non-verification
/// calls (`6_000` tokens ≈ 24 KiB). Session `session-vtcode-20260923T064245Z`
/// ran `5_000-7_000`-token inspections plus unbounded (`10_000`-token
/// default) calls against the `64 KiB` execution turn budget, exhausting it
/// after two or three previews. The `6_000`-token bound keeps one default
/// inspection inside the turn budget with room for tracker echoes and several
/// small calls; verification commands are exempt so build/test output
/// stays authoritative, and spool paging (with preview credit) stays
/// available for full content.
pub(crate) const EXEC_MODE_MAX_OUTPUT_TOKENS: usize = 6_000;

/// Validates and returns the requested model-visible result preview budget.
///
/// A missing value resolves to the stable default. Values must be JSON integers
/// so callers cannot silently coerce floats or strings into a larger context
/// allocation than the model requested.
///
/// This is validation only (bounds-check); per-mode preview policy lives in
/// [`resolve_max_output_tokens`].
pub(crate) fn max_output_tokens(args: &Value) -> Result<usize> {
    max_output_tokens_uncapped(args)
}

/// Mode-aware variant of [`max_output_tokens`].
///
/// An omitted value on a non-verification call resolves to the mode default
/// ([`PLAN_MODE_DEFAULT_MAX_OUTPUT_TOKENS`] in planning,
/// [`EXEC_MODE_MAX_OUTPUT_TOKENS`] in execution). An explicit value on a
/// non-verification call is clamped to the mode max
/// ([`PLAN_MODE_MAX_OUTPUT_TOKENS`] in planning,
/// [`EXEC_MODE_MAX_OUTPUT_TOKENS`] in execution) so one large inspection
/// cannot exhaust the turn preview budget; verification commands keep the
/// full default so build/test output stays authoritative.
pub(crate) fn resolve_max_output_tokens(args: &Value, planning_active: bool, is_verification: bool) -> Result<usize> {
    if planning_active && !is_verification {
        if args.get(MAX_OUTPUT_TOKENS_FIELD).is_none() {
            return Ok(PLAN_MODE_DEFAULT_MAX_OUTPUT_TOKENS);
        }
        let tokens = max_output_tokens_uncapped(args)?;
        return Ok(tokens.min(PLAN_MODE_MAX_OUTPUT_TOKENS));
    }
    if !is_verification {
        if args.get(MAX_OUTPUT_TOKENS_FIELD).is_none() {
            return Ok(EXEC_MODE_MAX_OUTPUT_TOKENS);
        }
        let tokens = max_output_tokens_uncapped(args)?;
        return Ok(tokens.min(EXEC_MODE_MAX_OUTPUT_TOKENS));
    }
    max_output_tokens_uncapped(args)
}

fn max_output_tokens_uncapped(args: &Value) -> Result<usize> {
    let Some(value) = args.get(MAX_OUTPUT_TOKENS_FIELD) else {
        return Ok(DEFAULT_MAX_OUTPUT_TOKENS);
    };
    let Some(tokens) = value.as_u64() else {
        return Err(anyhow!(
            "max_output_tokens must be an integer between {MIN_MAX_OUTPUT_TOKENS} and {MAX_MAX_OUTPUT_TOKENS}"
        ));
    };
    let tokens = usize::try_from(tokens).with_context(|| {
        format!("max_output_tokens must be an integer between {MIN_MAX_OUTPUT_TOKENS} and {MAX_MAX_OUTPUT_TOKENS}")
    })?;
    if !(MIN_MAX_OUTPUT_TOKENS..=MAX_MAX_OUTPUT_TOKENS).contains(&tokens) {
        return Err(anyhow!(
            "max_output_tokens must be an integer between {MIN_MAX_OUTPUT_TOKENS} and {MAX_MAX_OUTPUT_TOKENS}"
        ));
    }
    Ok(tokens)
}

/// Removes execution-only output metadata before validating a legacy schema.
/// The field is retained in the original payload and reaches the execution
/// pipeline, but schemas authored before this common option remain strict.
pub(crate) fn args_without_output_metadata(args: &Value) -> Value {
    let mut args = args.clone();
    if let Some(object) = args.as_object_mut() {
        object.remove(MAX_OUTPUT_TOKENS_FIELD);
    }
    args
}

/// Whether a handler explicitly consumes the common output-limit field.
pub(crate) fn handler_accepts_output_metadata(schema: Option<&Value>) -> bool {
    schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .is_some_and(|properties| properties.contains_key(MAX_OUTPUT_TOKENS_FIELD))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn max_output_tokens_defaults_and_enforces_integer_bounds() {
        assert_eq!(max_output_tokens(&json!({})).unwrap(), DEFAULT_MAX_OUTPUT_TOKENS);
        assert_eq!(max_output_tokens(&json!({"max_output_tokens": 1})).unwrap(), 1);
        assert_eq!(max_output_tokens(&json!({"max_output_tokens": 50_000})).unwrap(), 50_000);
        assert!(max_output_tokens(&json!({"max_output_tokens": 0})).is_err());
        assert!(max_output_tokens(&json!({"max_output_tokens": 50_001})).is_err());
        assert!(max_output_tokens(&json!({"max_output_tokens": 1.0})).is_err());
        assert!(max_output_tokens(&json!({"max_output_tokens": "100"})).is_err());
    }

    #[test]
    fn plan_mode_clamp_applies_only_to_omitted_non_verification_defaults() {
        // Omitted value in planning resolves to the smaller inspection default.
        assert_eq!(resolve_max_output_tokens(&json!({}), true, false).unwrap(), PLAN_MODE_DEFAULT_MAX_OUTPUT_TOKENS);
        // The plan default resolves strictly below the execution default.
        assert!(
            resolve_max_output_tokens(&json!({}), true, false).unwrap()
                < resolve_max_output_tokens(&json!({}), false, false).unwrap()
        );
        // Explicit large values in planning are clamped to the plan max so a
        // single inspection cannot exhaust the turn preview budget.
        assert_eq!(
            resolve_max_output_tokens(&json!({"max_output_tokens": DEFAULT_MAX_OUTPUT_TOKENS}), true, false).unwrap(),
            PLAN_MODE_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            resolve_max_output_tokens(&json!({"max_output_tokens": 30_000}), true, false).unwrap(),
            PLAN_MODE_MAX_OUTPUT_TOKENS
        );
        // Small explicit values below the max are preserved.
        assert_eq!(resolve_max_output_tokens(&json!({"max_output_tokens": 1}), true, false).unwrap(), 1);
        assert_eq!(
            resolve_max_output_tokens(&json!({"max_output_tokens": PLAN_MODE_MAX_OUTPUT_TOKENS}), true, false).unwrap(),
            PLAN_MODE_MAX_OUTPUT_TOKENS
        );
        // Verification commands keep the full default in planning.
        assert_eq!(resolve_max_output_tokens(&json!({}), true, true).unwrap(), DEFAULT_MAX_OUTPUT_TOKENS);
        // Execution non-verification omits to the execution bound and clamps
        // explicit large values to it; small explicit values are preserved.
        assert_eq!(resolve_max_output_tokens(&json!({}), false, false).unwrap(), EXEC_MODE_MAX_OUTPUT_TOKENS);
        assert_eq!(
            resolve_max_output_tokens(&json!({"max_output_tokens": 7_000}), false, false).unwrap(),
            EXEC_MODE_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            resolve_max_output_tokens(&json!({"max_output_tokens": 30_000}), false, false).unwrap(),
            EXEC_MODE_MAX_OUTPUT_TOKENS
        );
        assert_eq!(resolve_max_output_tokens(&json!({"max_output_tokens": 250}), false, false).unwrap(), 250);
        // Execution verification keeps the full default, omitted or explicit.
        assert_eq!(resolve_max_output_tokens(&json!({}), false, true).unwrap(), DEFAULT_MAX_OUTPUT_TOKENS);
        assert_eq!(resolve_max_output_tokens(&json!({"max_output_tokens": 30_000}), false, true).unwrap(), 30_000);
        // Invalid values still error in every mode.
        assert!(resolve_max_output_tokens(&json!({"max_output_tokens": 0}), true, false).is_err());
        assert!(resolve_max_output_tokens(&json!({"max_output_tokens": "100"}), true, false).is_err());
    }

    #[test]
    fn output_metadata_is_removed_only_from_schema_validation_copy() {
        let original = json!({"input": "value", "max_output_tokens": 23});
        assert_eq!(args_without_output_metadata(&original), json!({"input": "value"}));
        assert_eq!(original["max_output_tokens"], 23);
    }

    #[test]
    fn handler_metadata_forwarding_requires_an_explicit_schema_field() {
        assert!(!handler_accepts_output_metadata(Some(&json!({"type": "object"}))));
        assert!(handler_accepts_output_metadata(Some(&json!({
            "type": "object",
            "properties": {"max_output_tokens": {"type": "integer"}}
        }))));
    }
}
