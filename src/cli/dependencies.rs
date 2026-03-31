use anyhow::Result;
use crossterm::style::Stylize;
use vtcode_core::cli::args::{DependenciesSubcommand, ManagedDependency};
use vtcode_core::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use vtcode_core::tools::ripgrep_binary::RIPGREP_INSTALL_COMMAND;
use vtcode_core::tools::{AstGrepStatus, RipgrepStatus};

const SEARCH_TOOLS_INSTALL_COMMAND: &str = "vtcode dependencies install search-tools";

pub async fn handle_dependencies_command(command: DependenciesSubcommand) -> Result<()> {
    match command {
        DependenciesSubcommand::Install { dependency } => handle_install(dependency).await,
        DependenciesSubcommand::Status { dependency } => handle_status(dependency),
    }
}

async fn handle_install(dependency: ManagedDependency) -> Result<()> {
    match dependency {
        ManagedDependency::SearchTools => {
            println!("{} Installing the optional search tools bundle", "→".cyan());
            install_ripgrep()?;
            install_ast_grep().await?;
            Ok(())
        }
        ManagedDependency::Ripgrep => {
            install_ripgrep()?;
            Ok(())
        }
        ManagedDependency::AstGrep => {
            install_ast_grep().await?;
            Ok(())
        }
    }
}

fn handle_status(dependency: ManagedDependency) -> Result<()> {
    match dependency {
        ManagedDependency::SearchTools => {
            report_ripgrep_status();
            report_ast_grep_status();
            Ok(())
        }
        ManagedDependency::Ripgrep => {
            report_ripgrep_status();
            Ok(())
        }
        ManagedDependency::AstGrep => {
            report_ast_grep_status();
            Ok(())
        }
    }
}

fn print_ast_grep_next_steps() {
    println!(
        "{} For a local repository, run `vtcode init` to materialize `sgconfig.yml`, `rules/`, and `rule-tests/`.",
        "→".cyan()
    );
    println!("{} Then run `vtcode check ast-grep`.", "→".cyan());
}

fn install_ripgrep() -> Result<()> {
    match RipgrepStatus::check() {
        RipgrepStatus::Available { version } => {
            println!(
                "{} ripgrep already available: {}",
                "✓".green(),
                version.green()
            );
        }
        RipgrepStatus::NotFound => {
            println!(
                "{} Installing ripgrep using a supported system installer",
                "→".cyan()
            );
            RipgrepStatus::install()?;
            report_ripgrep_status();
        }
        RipgrepStatus::Error { reason } => {
            println!(
                "{} ripgrep was found but could not be verified: {}",
                "!".yellow(),
                reason
            );
            println!(
                "{} Retrying installation via `{}`",
                "→".cyan(),
                RIPGREP_INSTALL_COMMAND
            );
            RipgrepStatus::install()?;
            report_ripgrep_status();
        }
    }

    Ok(())
}

async fn install_ast_grep() -> Result<()> {
    match AstGrepStatus::check() {
        AstGrepStatus::Available {
            version,
            binary,
            managed,
        } => {
            println!(
                "{} ast-grep already available: {}",
                "✓".green(),
                version.green()
            );
            println!("{} Binary: {}", "→".cyan(), binary.display());
            println!(
                "{} Source: {}",
                "→".cyan(),
                if managed {
                    "VT Code-managed"
                } else {
                    "system PATH or override"
                }
            );
            print_ast_grep_next_steps();
        }
        AstGrepStatus::NotFound | AstGrepStatus::Error { .. } => {
            println!(
                "{} Installing ast-grep into {}",
                "→".cyan(),
                vtcode_core::tools::ast_grep_binary::managed_ast_grep_bin_dir().display()
            );

            let outcome = AstGrepStatus::install().await?;
            println!(
                "{} Installed {} at {}",
                "✓".green(),
                outcome.version.green(),
                outcome.binary_path.display()
            );

            if let Some(alias_path) = outcome.alias_path {
                println!(
                    "{} Installed compatibility alias at {}",
                    "→".cyan(),
                    alias_path.display()
                );
            }

            if let Some(warning) = outcome.warning {
                println!("{} {}", "⚠".yellow(), warning);
            }

            println!(
                "{} Add this to your shell if you want ast-grep outside VT Code:",
                "→".cyan()
            );
            println!(
                "  export PATH=\"{}:$PATH\"",
                outcome.managed_bin_dir.display()
            );
            print_ast_grep_next_steps();
        }
    }

    Ok(())
}

fn report_ripgrep_status() {
    match RipgrepStatus::check() {
        RipgrepStatus::Available { version } => {
            println!("{} ripgrep available: {}", "✓".green(), version.green());
            println!("{} Source: system PATH", "→".cyan());
        }
        RipgrepStatus::NotFound => {
            println!(
                "{} ripgrep is not installed. Run `{}` or `{}`.",
                "✗".red(),
                SEARCH_TOOLS_INSTALL_COMMAND,
                RIPGREP_INSTALL_COMMAND
            );
        }
        RipgrepStatus::Error { reason } => {
            println!("{} ripgrep check failed: {}", "✗".red(), reason);
        }
    }
}

fn report_ast_grep_status() {
    match AstGrepStatus::check() {
        AstGrepStatus::Available {
            version,
            binary,
            managed,
        } => {
            println!("{} ast-grep available: {}", "✓".green(), version.green());
            println!("{} Binary: {}", "→".cyan(), binary.display());
            println!(
                "{} Source: {}",
                "→".cyan(),
                if managed {
                    "VT Code-managed"
                } else {
                    "system PATH or override"
                }
            );
        }
        AstGrepStatus::NotFound => {
            println!(
                "{} ast-grep is not installed. Run `{}` or `{}`.",
                "✗".red(),
                SEARCH_TOOLS_INSTALL_COMMAND,
                AST_GREP_INSTALL_COMMAND
            );
        }
        AstGrepStatus::Error { reason } => {
            println!("{} ast-grep check failed: {}", "✗".red(), reason);
        }
    }
}
