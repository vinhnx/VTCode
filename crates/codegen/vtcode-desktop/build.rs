//! Track translations embedded by the desktop localization macro.

fn main() {
    println!("cargo:rerun-if-changed=locales/ui.yml");
}
