use vtcode_core::ui::styled::*;

fn main() {
    println!("Testing anstyle integration in VT Code:");

    // Test basic styles
    error("This is an error message");
    warning("This is a warning message");
    success("This is a success message");
    info("This is an info message");
    debug("This is a debug message");

    // Test bold styles
    println!(
        "{}This is bold text{}",
        Styles::bold().render(),
        Styles::bold().render_reset()
    );
    println!(
        "{}This is bold error text{}",
        Styles::bold_error().render(),
        Styles::bold_error().render_reset()
    );
    println!(
        "{}This is bold success text{}",
        Styles::bold_success().render(),
        Styles::bold_success().render_reset()
    );

    // Test custom styling
    let custom_style = Styles::header();
    println!(
        "{}This is custom styled text{}",
        custom_style.render(),
        custom_style.render_reset()
    );

    println!("All tests completed successfully!");
}
