//! # Utility Functions and Helpers
//!
//! This module provides various utility functions and helpers used throughout VTCode,
//! including configuration management, safety utilities, and common operations.
//!
//! ## Modules Overview
//!
//! ### Configuration Management (`dot_config`)
//! - **User Preferences**: Theme settings, UI preferences, cache configuration
//! - **Provider Configuration**: LLM provider settings and API keys
//! - **Dotfile Management**: `.vtcode` directory and configuration files
//!
//! ### Safety Utilities (`safety`)
//! - **Path Validation**: Workspace boundary checking
//! - **Command Sanitization**: Safe command execution
//! - **Input Validation**: User input sanitization
//!
//! ### ANSI and Colors (`ansi`, `colors`)
//! - **Terminal Colors**: ANSI color codes and styling
//! - **Color Management**: Theme support and color schemes
//! - **Cross-platform**: Works on different terminal types
//!
//! ### Git Integration (`vtcodegitignore`)
//! - **Gitignore Management**: Automatic `.vtcodegitignore` creation
//! - **Pattern Matching**: File exclusion patterns
//! - **Workspace Safety**: Prevents accidental file operations
//!
//! ## Basic Usage Examples
//!
//! ### Configuration Management
//! ```rust,no_run
//! use vtcode_core::utils::dot_config::{load_user_config, save_user_config, UserPreferences};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load user configuration
//!     let config = load_user_config().await?;
//!
//!     // Modify preferences
//!     let mut prefs = config.preferences;
//!     prefs.theme = "dark".to_string();
//!
//!     // Save changes
//!     save_user_config(&prefs).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Path Safety
//! ```rust,no_run
//! use vtcode_core::utils::safety::validate_workspace_path;
//! use std::path::PathBuf;
//!
//! let workspace = PathBuf::from("/home/user/project");
//! let file_path = PathBuf::from("src/main.rs");
//!
//! // Validate path is within workspace
//! match validate_workspace_path(&workspace, &file_path) {
//!     Ok(valid_path) => println!("Safe path: {}", valid_path.display()),
//!     Err(e) => eprintln!("Unsafe path: {}", e),
//! }
//! ```
//!
//! ### ANSI Colors
//! ```rust,no_run
//! use vtcode_core::utils::ansi::{colorize, Color};
//!
//! let message = "Hello, World!";
//! let colored = colorize(message, Color::Green);
//! println!("{}", colored); // Prints green text
//!
//! // Or use styling functions
//! let bold_text = vtcode_core::utils::ansi::bold("Important message");
//! let red_error = colorize("Error occurred", Color::Red);
//! ```
//!
//! ### Git Integration
//! ```rust,no_run
//! use vtcode_core::utils::vtcodegitignore::initialize_vtcode_gitignore;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let workspace = std::env::current_dir()?;
//!
//!     // Initialize .vtcodegitignore
//!     initialize_vtcode_gitignore(&workspace).await?;
//!
//!     println!("Git integration initialized");
//!     Ok(())
//! }
//! ```

pub mod ansi;
pub mod colors;
pub mod diff;
pub mod dot_config;
pub mod safety;
pub mod session_archive;
pub mod transcript;
pub mod utils;
pub mod vtcodegitignore;
