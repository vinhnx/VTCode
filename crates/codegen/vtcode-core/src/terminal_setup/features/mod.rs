//! Feature-specific configuration generators.
//!
//! Each feature generates terminal-specific configuration snippets.

pub mod copy_paste;
pub mod multiline;
pub mod notifications;
pub mod shell_integration;
pub mod theme_sync;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Placeholder test
    }
}
