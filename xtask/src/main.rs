use std::{
    env, fs, panic,
    path::{Path, PathBuf},
    process::Command,
};

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(about = "Repository automation tasks")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Build a release archive with man page and shell completions.
    PackageRelease(PackageReleaseArgs),
}

#[derive(Debug, Parser)]
struct PackageReleaseArgs {
    /// Rust target triple used to name the output archive.
    #[arg(long)]
    target: String,

    /// Version string without the leading v.
    #[arg(long)]
    version: String,

    /// Path to the already-built release binary for the selected target.
    #[arg(long)]
    binary: PathBuf,

    /// Output directory for generated artifacts and final archives.
    #[arg(long, default_value = "dist")]
    out_dir: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::PackageRelease(args) => package_release(args)?,
    }

    Ok(())
}

fn package_release(args: PackageReleaseArgs) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("..");
    let out_dir = workspace_root.join(&args.out_dir);
    let stage_dir = out_dir.join(format!("vtcode-{}-v{}", args.target, args.version));

    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)?;
    }
    fs::create_dir_all(&stage_dir)?;

    // Binary at archive root so self_update can find it via exact path match.
    fs::copy(workspace_root.join(&args.binary), stage_dir.join("vtcode"))?;

    // Extra files in subdirectories for cargo-binstall and install.sh.
    fs::create_dir_all(stage_dir.join("man/man1"))?;
    fs::create_dir_all(stage_dir.join("completions/bash"))?;
    fs::create_dir_all(stage_dir.join("completions/fish"))?;
    fs::create_dir_all(stage_dir.join("completions/zsh"))?;

    let man_page = vtcode_core::cli::ManPageGenerator::generate_main_man_page()?;
    fs::write(stage_dir.join("man/man1/vtcode.1"), man_page)?;

    match generate_completions(&stage_dir) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("warning: skipping shell completions: {e}");
            let _ = fs::remove_dir_all(stage_dir.join("completions"));
        }
    }

    let archive = out_dir.join(format!("vtcode-{}-v{}.tar.gz", args.target, args.version));

    if archive.exists() {
        fs::remove_file(&archive)?;
    }

    create_archive(&workspace_root, &out_dir, &archive, &stage_dir)?;
    println!("{}", archive.display());
    Ok(())
}

fn generate_completions(stage_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let result = panic::catch_unwind(|| {
        let mut cmd = vtcode_core::Cli::command();
        cmd.build();
        cmd
    });

    let mut cmd = match result {
        Ok(cmd) => cmd,
        Err(_) => {
            return Err("completion generation not supported with current CLI definition".into());
        }
    };

    for (shell, dir, filename) in [
        (clap_complete::Shell::Bash, "completions/bash", "vtcode"),
        (
            clap_complete::Shell::Fish,
            "completions/fish",
            "vtcode.fish",
        ),
        (clap_complete::Shell::Zsh, "completions/zsh", "_vtcode"),
    ] {
        let mut output = Vec::new();
        clap_complete::generate(shell, &mut cmd, "vtcode", &mut output);
        fs::write(stage_dir.join(dir).join(filename), output)?;
    }

    Ok(())
}

fn create_archive(
    _workspace_root: &Path,
    out_dir: &Path,
    archive: &Path,
    stage_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build list of entries from the stage directory.
    let mut entries: Vec<String> = Vec::new();
    for entry in fs::read_dir(stage_dir)? {
        let name = entry?.file_name().to_string_lossy().to_string();
        entries.push(name);
    }
    entries.sort();

    // Archive from stage_dir parent so entries have no directory prefix.
    // self_update needs the binary at the exact path "vtcode".
    let mut cmd = Command::new("tar");
    cmd.current_dir(out_dir)
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(stage_dir);
    for entry in &entries {
        cmd.arg(entry);
    }

    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err("tar command failed".into())
    }
}
