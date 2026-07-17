//! Terminal-specific configuration generators.
//!
//! Each terminal has its own module with configuration generation logic.

pub mod alacritty;
pub mod ghostty;
pub mod hyper;
pub mod iterm2;
pub mod kitty;
pub mod tabby;
pub mod vscode;
pub mod warp;
pub mod windows_terminal;
pub mod zed;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Placeholder test
    }
}
