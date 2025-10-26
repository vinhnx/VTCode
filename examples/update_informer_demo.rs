//! Example demonstrating the combined update-informer and self_update functionality

use anyhow::Result;
use vtcode_core::update::{UpdateConfig, UpdateManager};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("VTCode Update Integration Demo\n");

    // Create update manager with default configuration
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    println!("Checking for updates using update-informer...");

    // Check for updates (this will use update-informer for checking)
    match manager.check_for_updates().await {
        Ok(status) => {
            println!("Current version: {}", status.current_version);

            if let Some(latest) = &status.latest_version {
                println!("Latest version:  {}", latest);
            }

            if status.update_available {
                println!("\nAn update is available!");

                if let Some(url) = &status.download_url {
                    println!("Download URL: {}", url);
                }

                println!("\nYou can run 'vtcode update install' to install the update.");
            } else {
                println!("\nYou are running the latest version.");
            }
        }
        Err(e) => {
            eprintln!("Failed to check for updates: {}", e);
        }
    }

    Ok(())
}
