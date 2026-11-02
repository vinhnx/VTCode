use std::io::Write;

use anyhow::{Context, Result};
use vtcode_core::cli::ManPageGenerator;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

pub fn handle_all_man_pages(directory: &std::path::Path) -> Result<()> {
    let count = ManPageGenerator::generate_all_man_pages(directory)?;
    print_stdout_ignore_broken_pipe(&format!("Wrote {count} man pages to {}", directory.display()))
}

pub async fn handle_man_command(command: Option<String>, output: Option<std::path::PathBuf>) -> Result<()> {
    let content = match command.as_deref() {
        Some(cmd) => ManPageGenerator::generate_command_man_page(cmd)
            .with_context(|| format!("Failed to generate man page for {cmd}"))?,
        None => ManPageGenerator::generate_main_man_page().context("Failed to generate main man page")?,
    };

    if let Some(path) = output {
        write_file_with_context_sync(&path, &content, "man page")?;
        print_stdout_ignore_broken_pipe(&format!("Wrote man page to {}", path.display()))?;
    } else {
        print_stdout_ignore_broken_pipe(&content)?;
    }

    Ok(())
}

/// Write one block to stdout, treating a closed downstream pipe as success.
///
/// `println!` panics on `EPIPE` (e.g. `vtcode man | head`, quitting `less`
/// early). Piping large generated output is normal, so ignore
/// `ErrorKind::BrokenPipe` and only surface real I/O failures.
fn print_stdout_ignore_broken_pipe(text: &str) -> Result<()> {
    match writeln!(std::io::stdout().lock(), "{text}") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error).context("failed to write to stdout"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn all_man_pages_match_visible_cli_commands() {
        let directory = tempfile::tempdir().expect("temporary directory");
        handle_all_man_pages(directory.path()).expect("generate pages");
        let mut command = vtcode_core::Cli::command();
        command.build();
        let mut expected = vec!["vtcode.1".to_owned()];
        expected.extend(
            command
                .get_subcommands()
                .filter(|sub| !sub.is_hide_set())
                .map(|sub| format!("vtcode-{}.1", sub.get_name())),
        );
        expected.sort();
        let mut actual: Vec<_> = std::fs::read_dir(directory.path())
            .expect("pages")
            .map(|entry| entry.expect("page").file_name().to_string_lossy().into_owned())
            .collect();
        actual.sort();
        assert_eq!(actual, expected);
        let main = std::fs::read_to_string(directory.path().join("vtcode.1")).expect("main page");
        assert!(main.contains(".SH ENVIRONMENT"));
        let chat = std::fs::read_to_string(directory.path().join("vtcode-chat.1")).expect("chat page");
        assert!(chat.contains("vtcode chat"));
    }

    #[test]
    fn all_man_pages_propagates_output_errors() {
        let file = tempfile::NamedTempFile::new().expect("existing file");
        assert!(handle_all_man_pages(file.path()).is_err());
    }
}
