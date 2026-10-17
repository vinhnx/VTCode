//! Test-only helpers shared across config modules.
//!
//! Kept behind `#[cfg(test)]` so no production surface is added.

/// Assert that a fieldless config string enum keeps its three independent
/// string paths in lockstep for every variant:
///
/// 1. the derived serde wire form (`#[serde(rename_all = ...)]`),
/// 2. the hand-written `as_str()`, and
/// 3. the hand-written `parse()` (config ingestion).
///
/// The type system cannot tie these together, so a one-sided edit (a serde
/// rename, or a single `match` arm) silently stops a value written by one path
/// from round-tripping through another. Invoke inside the enum's own `mod tests`
/// so the private `as_str()`/`parse()` methods remain accessible.
///
/// Uses `panic!` rather than `.expect()` so this test-only helper stays free of
/// the production `unwrap`/`expect` governance scan (`check_no_unwrap_expect_prod.py`),
/// which does not treat `test_support.rs` as a test path.
///
/// Usage: `assert_string_enum_lockstep!(Kind, [Kind::A, Kind::B, ...]);`
macro_rules! assert_string_enum_lockstep {
    ($ty:ty, [$($variant:expr),+ $(,)?]) => {{
        $({
            let variant: $ty = $variant;
            let expected = variant.as_str();
            match ::serde_json::to_value(&variant) {
                Ok(serialized) => ::std::assert_eq!(
                    serialized,
                    ::serde_json::json!(expected),
                    "serde wire form drifted from as_str() for {variant:?}"
                ),
                Err(error) => panic!("enum failed to serialize for {variant:?}: {error}"),
            }
            match <$ty>::parse(expected) {
                Some(parsed) => ::std::assert_eq!(
                    parsed,
                    variant,
                    "parse() drifted from as_str() for {variant:?}"
                ),
                None => panic!("parse() rejected as_str() output {expected:?} for {variant:?}"),
            }
        })+
    }};
}

pub(crate) use assert_string_enum_lockstep;
