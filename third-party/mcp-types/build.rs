use schemars::schema::RootSchema;
use std::{env, fs, path::Path};
use typify::{TypeSpace, TypeSpaceSettings};

fn main() {
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

    fs::write(&out_file, contents).unwrap();
}
