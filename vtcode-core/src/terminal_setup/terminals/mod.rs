//! Terminal-specific configuration generators.
//!
//! Each terminal has its own module with configuration generation logic.

pub mod ghostty;
pub mod kitty;
pub mod alacritty;
pub mod zed;
pub mod warp;
pub mod iterm2;
pub mod vscode;
pub mod windows_terminal;
pub mod hyper;
pub mod tabby;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Placeholder test
    }
}
