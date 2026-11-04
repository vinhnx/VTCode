//! Agent Skills specification limits.
//!
//! Two validators enforce the same spec: [`SkillManifest::validate`] fails fast
//! on the first violation, while [`crate::enhanced_validator::ComprehensiveSkillValidator`]
//! collects every violation with a remediation hint. They differ in *reporting*
//! only, so the limits themselves live here once — otherwise changing a number
//! in one validator would silently desync the other.
//!
//! Each constant is a character or entry count from the spec, not a byte size.
//!
//! [`SkillManifest::validate`]: crate::types::SkillManifest::validate
//!
//! # Examples
//!
//! ```
//! use vtcode_skills::limits::MAX_SKILL_NAME_CHARS;
//!
//! assert_eq!(MAX_SKILL_NAME_CHARS, 64);
//! ```

/// Maximum characters in a skill `name` (lowercase letters, digits, hyphens).
pub const MAX_SKILL_NAME_CHARS: usize = 64;

/// Maximum characters in a skill `description`.
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 1024;

/// Maximum characters in a skill `license` string.
pub const MAX_LICENSE_CHARS: usize = 512;

/// Maximum characters in a skill `compatibility` string.
pub const MAX_COMPATIBILITY_CHARS: usize = 500;

/// Maximum number of entries in a skill's `allowed-tools` list.
pub const MAX_ALLOWED_TOOLS: usize = 16;

/// Count the space-delimited tools in an `allowed-tools` value.
///
/// This is how [`MAX_ALLOWED_TOOLS`] is measured, so it lives next to the limit
/// rather than being re-derived by each validator. Counting the iterator avoids
/// materializing a `Vec` that would only be read for its length.
///
/// # Examples
///
/// ```
/// use vtcode_skills::limits::allowed_tool_count;
///
/// assert_eq!(allowed_tool_count("Read Write Bash"), 3);
/// assert_eq!(allowed_tool_count("  Read   Write  "), 2);
/// assert_eq!(allowed_tool_count(""), 0);
/// ```
#[must_use]
pub fn allowed_tool_count(allowed_tools: &str) -> usize {
    allowed_tools.split_whitespace().count()
}
