// Example demonstrating dialoguer usage
//
// This example shows various prompt types available in dialoguer.

use anyhow::Result;
use dialoguer::{Confirm, Input, MultiSelect, Password, Select, theme::ColorfulTheme};

fn main() -> Result<()> {
    println!("=== dialoguer Example ===");

    // Use a colorful theme for better appearance
    let theme = ColorfulTheme::default();

    // Confirm prompt
    let confirmation = Confirm::with_theme(&theme)
        .with_prompt("Do you want to continue?")
        .default(true)
        .interact()?;

    if !confirmation {
        println!("Operation cancelled.");
        return Ok(());
    }

    // Input prompt
    let name: String = Input::with_theme(&theme)
        .with_prompt("Your name")
        .default("VT Code User".into())
        .interact_text()?;

    println!("Hello, {}!", name);

    // Select prompt
    let selections = &[
        "Create new project",
        "Analyze existing code",
        "Generate documentation",
        "Run tests",
        "Exit",
    ];

    let selection = Select::with_theme(&theme)
        .with_prompt("What would you like to do?")
        .items(&selections[..])
        .default(0)
        .interact()?;

    println!("You selected: {}", selections[selection]);

    // MultiSelect prompt
    let features = &[
        "Code completion",
        "Syntax highlighting",
        "Error detection",
        "Performance analysis",
        "Security scanning",
    ];

    let chosen_features: Vec<usize> = MultiSelect::with_theme(&theme)
        .with_prompt("Select features to enable")
        .items(&features[..])
        .defaults(&[true, true, false, false, false])
        .interact()?;

    println!("Enabled features:");
    for &index in &chosen_features {
        println!("  - {}", features[index]);
    }

    // Password prompt (only if they selected a security-related option)
    if chosen_features.contains(&4) {
        // Security scanning
        let password = Password::with_theme(&theme)
            .with_prompt("Enter API key for security scanning")
            .validate_with(|input: &String| -> Result<(), &str> {
                if input.len() < 8 {
                    Err("Password must be at least 8 characters long")
                } else {
                    Ok(())
                }
            })
            .interact()?;

        println!("API key accepted (length: {})", password.len());
    }

    println!("\nDemo completed successfully!");
    Ok(())
}
