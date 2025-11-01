//! Example demonstrating the new modern TUI implementation
//! This shows how to use the improved TUI system based on Ratatui best practices

use anyhow::Result;
use vtcode_core::ui::tui::modern_tui::{Event, ModernTui};

#[tokio::main]
async fn main() -> Result<()> {
    // Create a new modern TUI instance following Ratatui best practices
    let mut tui = ModernTui::new()?
        .tick_rate(4.0) // 4 ticks per second
        .frame_rate(30.0) // 30 frames per second
        .mouse(true) // Enable mouse capture
        .paste(true); // Enable bracketed paste

    // Enter the TUI - this sets up raw mode, alternate screen, etc.
    tui.enter().await?;

    println!("Modern TUI initialized with:");
    println!("- Frame rate: {} FPS", tui.frame_rate);
    println!("- Tick rate: {} Hz", tui.tick_rate);
    println!("- Mouse support: {}", tui.mouse);
    println!("- Paste support: {}", tui.paste);
    println!("\nPress 'q' or 'Ctrl+C' to quit");
    println!("Press other keys to see events...");

    // Main event loop
    loop {
        if let Some(event) = tui.event_rx.recv().await {
            match event {
                Event::Key(key_event) => {
                    // Handle key events
                    if key_event.code == crossterm::event::KeyCode::Char('q')
                        || (key_event
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && key_event.code == crossterm::event::KeyCode::Char('c'))
                    {
                        break; // Exit on 'q' or Ctrl+C
                    }

                    println!("Received key event: {:?}", key_event);
                }
                Event::Mouse(mouse_event) => {
                    println!("Received mouse event: {:?}", mouse_event);
                }
                Event::Resize(width, height) => {
                    println!("Terminal resized to: {}x{}", width, height);
                }
                Event::FocusGained => {
                    println!("Terminal focused");
                }
                Event::FocusLost => {
                    println!("Terminal unfocused");
                }
                Event::Paste(content) => {
                    println!("Pasted content: {}", content);
                }
                Event::Tick => {
                    // Periodic tick event (based on tick_rate)
                }
                Event::Render => {
                    // Render event (based on frame_rate) - good place to draw UI
                    // For this example, we're not drawing anything
                }
                Event::Init => {
                    println!("TUI initialized successfully");
                }
                Event::Quit | Event::Closed | Event::Error => {
                    break;
                }
            }
        }
    }

    // Exit the TUI - cleans up raw mode, alternate screen, etc.
    tui.exit().await?;

    println!("Modern TUI exited successfully");
    Ok(())
}
