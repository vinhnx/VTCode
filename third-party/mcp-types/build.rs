use schemars::schema::RootSchema;
use std::{env, fs, path::Path};
use typify::{TypeSpace, TypeSpaceSettings};

fn main() {
    // Suppress macOS malloc warnings in build output
    // IMPORTANT: Only remove vars, never set them to "0" as that triggers
    // "can't turn off malloc stack logging" warnings from xcrun
    #[cfg(target_os = "macos")]
    {
        // Unset all malloc-related environment variables that might cause warnings
        std::env::remove_var("MallocStackLogging");
        std::env::remove_var("MallocStackLoggingDirectory");
        std::env::remove_var("MallocScribble");
        std::env::remove_var("MallocGuardEdges");
        std::env::remove_var("MallocCheckHeapStart");
        std::env::remove_var("MallocCheckHeapEach");
        std::env::remove_var("MallocCheckHeapAbort");
        std::env::remove_var("MallocCheckHeapSleep");
        std::env::remove_var("MallocErrorAbort");
        std::env::remove_var("MallocCorruptionAbort");
        std::env::remove_var("MallocHelpOptions");
        std::env::remove_var("MallocStackLoggingNoCompact");
    }

    println!("cargo:rerun-if-env-changed=MCP_TYPES_FORCE_REGEN");

    if env::var_os("MCP_TYPES_FORCE_REGEN").is_some() {
        generate("2024-11-05");
    }
}

fn generate(version: &str) {
    let is_docsrs = env::var_os("DOCS_RS").is_some();

    // Don't regenerate schema when building on docs.rs
    if is_docsrs {
        println!("cargo:warning=docs.rs build detected, skipping schema regeneration");
        println!("cargo:warning=Using existing generated sources");
        return;
    }

    let schema_path = format!("./spec/{}-schema.json", version);
    let content = std::fs::read_to_string(schema_path).unwrap();
    let schema = serde_json::from_str::<RootSchema>(&content).unwrap();

    let mut type_space = TypeSpace::new(
        TypeSpaceSettings::default()
            .with_struct_builder(false)
            .with_derive("PartialEq".to_string()),
    );
    type_space.add_root_schema(schema).unwrap();

    let contents = syn::parse2::<syn::File>(type_space.to_stream()).unwrap();
    let contents = prettyplease::unparse(&contents);

    let file_name = format!("src/v{}/types.rs", version.replace("-", "_"));
    let mut out_file = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    out_file.push(file_name);

    // Handle read-only file system gracefully
    match fs::write(&out_file, contents) {
        Ok(_) => {},
        Err(e) if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem => {
            println!("cargo:warning=Read-only file system detected, skipping file write: {}", e);
        },
        Err(e) => {
            panic!("Failed to write file: {}", e);
        }
    }
}
