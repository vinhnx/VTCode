#![allow(missing_docs)]
use vtcode_core::ui::styled::*;

fn main() {
    // Test the migrated styling
    info("Testing migrated styling from main_modular.rs");
    println!("{}Debug message{}", Styles::debug().render(), Styles::debug().render_reset());
    println!("{}Info message with custom styling{}", Styles::info().render(), Styles::info().render_reset());

    println!("Migration test completed successfully!");
}
