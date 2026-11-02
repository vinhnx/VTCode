//! Native GPUI desktop client for the VT Code local service.

#[cfg(target_os = "macos")]
rust_i18n::i18n!("locales", fallback = "en");

#[cfg(target_os = "macos")]
mod desktop;

#[cfg(target_os = "macos")]
fn main() {
    // `args_os`: the CLI passes its workspace as an OS string, and `args()`
    // would panic on non-UTF8 paths instead of showing invalid-folder.
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    desktop::run(desktop::parse_startup_link(&argv));
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("VT Code Desktop currently requires macOS 15 or later.");
    std::process::exit(1);
}
