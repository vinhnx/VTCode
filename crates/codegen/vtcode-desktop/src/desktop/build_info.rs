//! Compile-time build identity shown by both About surfaces.
//!
//! `build.rs` stamps [`BUILD_VERSION`] from the commit that produced the binary
//! (or from `VTCODE_DESKTOP_BUILD_VERSION` when the packaging script exports
//! it). The packaging script writes the same value into `CFBundleVersion`, so
//! the native About panel and the in-app fallback card never disagree.

/// Marketing version from the workspace `Cargo.toml`.
pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build identifier displayed in parentheses after the marketing version.
pub(super) const BUILD_VERSION: &str = env!("VTCODE_DESKTOP_BUILD_VERSION");

/// `0.181.4 (86902fce7e1d)` — the version line both About surfaces show.
pub(super) fn display_version() -> String {
    format!("{VERSION} ({BUILD_VERSION})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_version_pairs_the_marketing_and_build_versions() {
        let shown = display_version();
        assert_eq!(shown, format!("{} ({})", env!("CARGO_PKG_VERSION"), BUILD_VERSION));
        assert!(!BUILD_VERSION.trim().is_empty(), "build.rs must stamp a build identifier");
    }
}
