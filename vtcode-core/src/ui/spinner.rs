//! Loading spinner utilities for terminal UI using indicatif crate

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// A wrapper around indicatif's ProgressBar for easy spinner management
pub struct Spinner {
    pb: ProgressBar,
}

impl Spinner {
    /// Create a new spinner with the given message
    pub fn new(message: &str) -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.tick(); // Ensure spinner displays immediately

        Self { pb }
    }

    /// Create a new spinner with a download-style progress bar
    pub fn new_download_style(message: &str, total_size: u64) -> Self {
        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));
        pb.set_message(message.to_string());

        Self { pb }
    }

    /// Update the spinner message
    pub fn set_message(&self, message: &str) {
        self.pb.set_message(message.to_string());
    }

    /// Set the position for progress bar (useful for download-style spinners)
    pub fn set_position(&self, pos: u64) {
        self.pb.set_position(pos);
    }

    /// Set the total size for progress bar
    pub fn set_length(&self, len: u64) {
        self.pb.set_length(len);
    }

    /// Finish the spinner with a success message
    pub fn finish_with_message(&self, message: &str) {
        self.pb.finish_with_message(message.to_string());
    }

    /// Finish the spinner and clear the line
    pub fn finish_and_clear(&self) {
        self.pb.finish_and_clear();
    }

    /// Finish the spinner with an error message
    pub fn finish_with_error(&self, message: &str) {
        self.pb.abandon_with_message(format!("✦ {}", message));
    }

    /// Get a clone of the ProgressBar for use in other threads
    pub fn clone_inner(&self) -> ProgressBar {
        self.pb.clone()
    }

    /// Create a new spinner that runs in a tokio task for better async integration
    pub fn new_async(message: &str) -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.tick(); // Ensure spinner displays immediately

        Self { pb }
    }

    /// Run the spinner in a tokio task and return a handle to control it
    pub fn spawn_async(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Keep the spinner running until the task is cancelled
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                // The spinner will be automatically cleaned up when the task ends
            }
        })
    }
}

/// Show a simple loading spinner with the given message
/// This is a convenience function that creates and immediately starts a spinner
pub fn show_loading_spinner(message: &str) -> Spinner {
    let spinner = Spinner::new(message);
    spinner.pb.tick();
    spinner
}

/// Start a loading spinner in a background thread for long-running tasks
/// Returns a handle that can be used to control the spinner
pub fn start_loading_spinner(message: &str) -> Spinner {
    let spinner = Spinner::new(message);
    spinner.pb.tick();
    spinner
}

/// Start a download-style progress bar spinner
/// Useful for tasks with known total size
pub fn start_download_spinner(message: &str, total_size: u64) -> Spinner {
    let spinner = Spinner::new_download_style(message, total_size);
    spinner.pb.tick();
    spinner
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_spinner_creation() {
        let spinner = Spinner::new("Testing spinner");
        assert!(spinner.clone_inner().length().is_none());
        spinner.finish_and_clear();
    }

    #[test]
    fn test_show_loading_spinner() {
        let spinner = show_loading_spinner("Test message");
        thread::sleep(Duration::from_millis(200));
        spinner.finish_with_message("Done");
    }

    #[test]
    fn test_download_spinner() {
        let spinner = start_download_spinner("Downloading", 100);
        spinner.set_position(50);
        thread::sleep(Duration::from_millis(200));
        spinner.finish_with_message("Download complete");
    }
}
