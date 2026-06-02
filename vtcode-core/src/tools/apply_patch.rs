//! Patch tool facade that exposes Codex-compatible patch parsing and application.
//!
//! Actual patch parsing logic lives in `tools::editing::patch` so future edit
//! features can reuse the same primitives without depending on this facade.

use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::tools::editing::{Patch, PatchError, PatchHunk, PatchLine, PatchOperation};
pub use vtcode_utility_tool_specs::{
    APPLY_PATCH_ALIAS_DESCRIPTION, SEMANTIC_ANCHOR_GUIDANCE, with_semantic_anchor_guidance,
};

/// Input structure for the apply_patch tool
#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPatchInput {
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedApplyPatchInput {
    pub text: String,
    pub source_bytes: usize,
    pub was_base64: bool,
}

pub fn patch_source_from_args(args: &Value) -> Option<&str> {
    args.as_str()
        .or_else(|| args.get("input").and_then(|value| value.as_str()))
        .or_else(|| args.get("patch").and_then(|value| value.as_str()))
}

pub fn decode_apply_patch_input(args: &Value) -> anyhow::Result<Option<DecodedApplyPatchInput>> {
    let Some(source) = patch_source_from_args(args) else {
        return Ok(None);
    };

    let was_base64 = source.starts_with("base64:");
    let cap = effective_max_payload_bytes();
    let text = if was_base64 {
        let decoded = BASE64
            .decode(&source[7..])
            .with_context(|| "Failed to decode base64 patch")?;
        enforce_decoded_size_limit(decoded.len(), source.len(), was_base64, cap)?;
        String::from_utf8(decoded).with_context(|| "Decoded patch is not valid UTF-8")?
    } else {
        enforce_decoded_size_limit(source.len(), source.len(), was_base64, cap)?;
        source.to_string()
    };

    Ok(Some(DecodedApplyPatchInput {
        text,
        source_bytes: source.len(),
        was_base64,
    }))
}

fn enforce_decoded_size_limit(
    decoded_bytes: usize,
    source_bytes: usize,
    was_base64: bool,
    cap: usize,
) -> anyhow::Result<()> {
    // Cap applies to the *decoded* size to prevent base64 decompression-bomb-style attacks
    // where a small source expands to a very large patch.
    if decoded_bytes <= cap {
        return Ok(());
    }
    anyhow::bail!(
        "apply_patch payload too large after decoding: decoded={} bytes (source={} bytes, base64={}). \
         The per-patch cap is {} bytes; split the change into smaller patches or raise {}.",
        decoded_bytes,
        source_bytes,
        was_base64,
        cap,
        UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV
    )
}

/// Hard upper bound for any single `apply_patch` payload (and `unified_file` `edit`/`patch`
/// actions) after base64 decoding. The preflight cap mirrors this same value; both
/// share the same env-var override.
pub const UNIFIED_FILE_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

/// Maximum allowed decoded patch size — same as `UNIFIED_FILE_MAX_PAYLOAD_BYTES` but
/// exposed with a more specific name to clarify that it is enforced at decode time,
/// not preflight time.
pub const MAX_DECODED_PATCH_BYTES: usize = UNIFIED_FILE_MAX_PAYLOAD_BYTES;

/// Env var name that overrides both the preflight cap and the post-decode cap.
pub const UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV: &str = "VTCODE_UNIFIED_FILE_MAX_PAYLOAD_BYTES";

/// Resolve the effective cap, honoring the env-var override. A 1 KiB safety
/// floor is enforced so a sub-floor override can never silently disable the
/// post-decode cap; values below the floor fall back to the default. The same
/// floor is applied by the preflight resolver in `execution_kernel` so both
/// stages agree on the effective cap.
pub fn effective_max_payload_bytes() -> usize {
    std::env::var(UNIFIED_FILE_MAX_PAYLOAD_BYTES_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|value| *value >= 1024)
        .unwrap_or(UNIFIED_FILE_MAX_PAYLOAD_BYTES)
}

pub fn parameter_schema(input_description: &str) -> Value {
    vtcode_utility_tool_specs::apply_patch_parameter_schema(input_description)
}

#[cfg(test)]
mod tests {
    use super::{
        APPLY_PATCH_ALIAS_DESCRIPTION, SEMANTIC_ANCHOR_GUIDANCE, decode_apply_patch_input,
        parameter_schema, patch_source_from_args, with_semantic_anchor_guidance,
    };
    use serde_json::json;

    #[test]
    fn patch_source_accepts_raw_string_and_object_fields() {
        assert_eq!(
            patch_source_from_args(&json!("*** Begin Patch\n*** End Patch\n")),
            Some("*** Begin Patch\n*** End Patch\n")
        );
        assert_eq!(patch_source_from_args(&json!({"input": "x"})), Some("x"));
        assert_eq!(patch_source_from_args(&json!({"patch": "y"})), Some("y"));
    }

    #[test]
    fn decode_apply_patch_input_supports_base64_payloads() {
        let payload = json!({
            "patch": "base64:KioqIEJlZ2luIFBhdGNoCioqKiBFbmQgUGF0Y2gK"
        });

        let decoded = decode_apply_patch_input(&payload)
            .expect("payload should decode")
            .expect("payload should be present");

        assert_eq!(decoded.text, "*** Begin Patch\n*** End Patch\n");
        assert_eq!(decoded.source_bytes, 47);
        assert!(decoded.was_base64);
    }

    #[test]
    fn decode_apply_patch_input_rejects_invalid_base64() {
        let error = decode_apply_patch_input(&json!({"input": "base64:not-valid"}))
            .expect_err("invalid base64 should fail");

        assert!(error.to_string().contains("Failed to decode base64 patch"));
    }

    #[test]
    fn decode_apply_patch_input_caps_decoded_size() {
        use base64::Engine;
        // Build a 1.5 MiB decoded payload via base64. The default cap is 1 MiB so the
        // decode must fail with the post-decode size error rather than producing the
        // oversized text.
        let large = "A".repeat(1_500_000);
        let encoded = base64::engine::general_purpose::STANDARD.encode(large.as_bytes());
        let payload = json!({ "input": format!("base64:{encoded}") });

        let error = decode_apply_patch_input(&payload)
            .expect_err("oversized decoded payload should be rejected");
        let message = error.to_string();
        assert!(
            message.contains("apply_patch payload too large after decoding"),
            "unexpected error message: {message}"
        );
        assert!(message.contains("base64=true"));
    }

    #[test]
    fn decode_apply_patch_input_caps_raw_payload_size() {
        // Even non-base64 inputs must respect the cap. Build a 1.5 MiB raw string
        // and confirm it is rejected.
        let large = "B".repeat(1_500_000);
        let payload = json!({ "input": large });

        let error = decode_apply_patch_input(&payload)
            .expect_err("oversized raw payload should be rejected");
        let message = error.to_string();
        assert!(
            message.contains("apply_patch payload too large after decoding"),
            "unexpected error message: {message}"
        );
        assert!(message.contains("base64=false"));
    }

    #[test]
    fn semantic_anchor_guidance_is_appended_once() {
        let base = "Patch in VT Code format.";
        let with_guidance = with_semantic_anchor_guidance(base);
        assert!(with_guidance.contains(SEMANTIC_ANCHOR_GUIDANCE));
        assert_eq!(
            with_semantic_anchor_guidance(&with_guidance),
            with_guidance,
            "guidance should not be duplicated"
        );
    }

    #[test]
    fn parameter_schema_keeps_alias_and_guidance_consistent() {
        let schema = parameter_schema("Patch in VT Code format");

        assert_eq!(
            schema["properties"]["patch"]["description"],
            APPLY_PATCH_ALIAS_DESCRIPTION
        );
        let input_description = schema["properties"]["input"]["description"]
            .as_str()
            .expect("input description");
        assert!(input_description.contains(SEMANTIC_ANCHOR_GUIDANCE));
    }
}
