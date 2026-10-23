//! Native GPUI desktop client for the VT Code local service.

#[cfg(target_os = "macos")]
mod desktop;

#[cfg(target_os = "macos")]
fn main() {
    desktop::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("VT Code Desktop currently requires macOS 15 or later.");
    std::process::exit(1);
}
