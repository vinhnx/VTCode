fn cpp11(cc: &mut cc::Build) {
    cc.cpp(true);
    // `cfg!` in a build script reflects the host, not the target. Check the
    // target's OS/env so cross-compiling (e.g. a Unix host -> windows-msvc
    // target) does not pass the GCC-style `-std=c++11` flag to MSVC, and a
    // Windows host -> non-MSVC target still gets the flag it needs.
    let target_is_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !target_is_msvc {
        cc.flag("-std=c++11");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=cpp/");

    let mut cc = cc::Build::new();
    cpp11(&mut cc);
    cc.file("cpp/simdutfrs.cpp").compile("simdutfrs");
}
