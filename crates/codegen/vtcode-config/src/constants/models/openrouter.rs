// Include build-script-generated constants. On docs.rs, the build script
// detects the DOCS_RS env var and generates a placeholder file with all
// per-model constants so that #[cfg(not(docsrs))] source code compiles.
include!(concat!(env!("OUT_DIR"), "/openrouter_constants.rs"));
