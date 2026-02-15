#[cfg(not(feature = "schema"))]
fn main() {
    eprintln!("The schema_dump example requires --features schema");
    std::process::exit(1);
}

#[cfg(feature = "schema")]
fn main() -> anyhow::Result<()> {
    let schema = vtcode_config::vtcode_config_schema_json()?;
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}
