//! Track translations and embedded license documents used by the desktop app.

fn main() {
    println!("cargo:rerun-if-changed=locales");
    // The license viewer embeds these at compile time; rebuild when they change.
    for document in ["LICENSE", "LICENSE-MIT", "THIRD-PARTY-NOTICES"] {
        println!("cargo:rerun-if-changed=../../../{document}");
    }
}
