//! Man page generation for VT Code CLI.
//!
//! Structural sections (NAME, SYNOPSIS, DESCRIPTION, OPTIONS, SUBCOMMANDS,
//! VERSION) are generated from the clap [`Cli`] command tree via
//! [`clap_mangen`], so they cannot drift from `--help` output. VT Code
//! specific sections (ENVIRONMENT, FILES, SAFETY, EXAMPLES) are appended on
//! top with [`roff`].

use std::io::Write;

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use clap_mangen::Man;
use roff::{Roff, bold, roman};

use crate::cli::args::{Cli, long_version};

/// Man page generator for VT Code CLI
pub struct ManPageGenerator;

impl ManPageGenerator {
    /// Generate man page for the main VT Code command
    pub fn generate_main_man_page() -> Result<String> {
        let mut cmd = Cli::command();
        cmd = cmd.name("vtcode");
        if cmd.get_about().is_none() && cmd.get_long_about().is_none() {
            cmd = cmd.about("Advanced coding agent with Decision Ledger");
        }
        if cmd.get_version().is_none() {
            let version: &'static str = Box::leak(long_version().into_boxed_str());
            cmd = cmd.version(version);
        }
        Self::render_page(cmd, None, &|buf| Self::append_main_sections(buf))
    }

    /// Generate man page for a specific command
    pub fn generate_command_man_page(command: &str) -> Result<String> {
        let mut cmd = Cli::command();
        cmd.build();
        let Some(sub) = cmd.find_subcommand(command).filter(|sub| !sub.is_hide_set()).cloned() else {
            bail!("Unknown command: {command}");
        };
        let name = sub.get_name().to_owned();
        let examples: &[&str] = match name.as_str() {
            "ask" => &[
                "vtcode ask \"what is a monad?\"",
                "echo \"summarize this\" | vtcode ask",
                "vtcode ask --output-format json \"explain ownership in Rust\"",
            ],
            "benchmark" => &["vtcode benchmark"],
            "check" => &["vtcode check ast-grep"],
            "chat" => &["vtcode chat"],
            "create-project" => &[
                "vtcode create-project myapp --feature web --feature auth",
                "vtcode create-project simple_app",
            ],
            "init" => &["vtcode init", "vtcode init --force"],
            "man" => &["vtcode man", "vtcode man chat", "vtcode man chat --output chat.1"],
            _ => &[],
        };
        let title = format!("vtcode-{name}");
        let sub = sub.display_name(&title);
        Self::render_page(sub, Some(title), &|buf| Self::append_command_sections(buf, examples))
    }

    /// Generate the main man page plus one page per visible subcommand into
    /// `dir` (`vtcode.1`, `vtcode-<command>.1`, ...). Returns the number of
    /// pages written.
    pub fn generate_all_man_pages(dir: &std::path::Path) -> Result<usize> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create man page directory {}", dir.display()))?;

        let mut cmd = Cli::command();
        cmd.build();
        let main_page = Self::generate_main_man_page()?;
        let mut count = Self::write_page(dir, "vtcode", &main_page)?;
        for sub in cmd.get_subcommands().filter(|sub| !sub.is_hide_set()) {
            let name = sub.get_name();
            count += Self::write_page(dir, &format!("vtcode-{name}"), &Self::generate_command_man_page(name)?)?;
        }
        Ok(count)
    }

    /// Write one man page to `dir/<name>.1`, returning 1 on success.
    fn write_page(dir: &std::path::Path, name: &str, content: &str) -> Result<usize> {
        let path = dir.join(format!("{name}.1"));
        std::fs::write(&path, content).with_context(|| format!("failed to write man page {}", path.display()))?;
        Ok(1)
    }

    /// Render a clap-derived man page and append VT Code-specific sections.
    fn render_page(
        cmd: clap::Command,
        title: Option<String>,
        append: &dyn Fn(&mut Vec<u8>) -> Result<()>,
    ) -> Result<String> {
        let mut man = Man::new(cmd);
        if let Some(title) = title {
            man = man.title(title.to_uppercase());
        }
        let man = man
            .date(chrono::Utc::now().format("%Y-%m-%d").to_string())
            .manual(crate::config::constants::app::DISPLAY_NAME)
            .source(crate::config::constants::app::DISPLAY_NAME);

        let mut buf = Vec::new();
        man.render(&mut buf).context("failed to render clap-derived man page")?;
        append(&mut buf)?;
        String::from_utf8(buf).context("man page output is not valid UTF-8")
    }

    /// Append VT Code-specific sections to the main man page.
    fn append_main_sections(buf: &mut Vec<u8>) -> Result<()> {
        let mut roff = Roff::new();
        roff.control("SH", ["ENVIRONMENT"]);
        for (name, description) in [
            ("GEMINI_API_KEY", "API key for Google Gemini (default provider)"),
            ("OPENAI_API_KEY", "API key for OpenAI GPT models"),
            ("ANTHROPIC_API_KEY", "API key for Anthropic Claude models"),
            ("DEEPSEEK_API_KEY", "API key for DeepSeek models"),
            ("META_API_KEY", "API key for Meta AI Muse models"),
            ("MODEL_API_KEY", "Meta AI's documented API key variable"),
            ("ZAI_API_KEY", "API key for Z.AI GLM models"),
            ("MOONSHOT_API_KEY", "API key for Moonshot AI Kimi models"),
            ("OPENROUTER_API_KEY", "API key for OpenRouter models"),
            ("NVIDIA_API_KEY", "API key for NVIDIA NIM models"),
            ("MERGE_GATEWAY_API_KEY", "API key for Merge Gateway routes"),
            (
                "MERGE_GATEWAY_BASE_URL",
                "Optional Merge Gateway endpoint override; /v1/openai selects legacy compatibility",
            ),
            ("AI_GATEWAY_API_KEY", "API key for Vercel AI Gateway models"),
            (
                "VERCEL_AI_GATEWAY_BASE_URL",
                "Optional Vercel AI Gateway endpoint override (default: https://ai-gateway.vercel.sh/v1)",
            ),
        ] {
            roff.control("TP", []).text([bold(name)]).text([roman(description)]);
        }

        roff.control("SH", ["FILES"]);
        roff.control("TP", []).text([bold("vtcode.toml")]).text([roman(
            "Configuration file (current directory or the canonical user config directory)",
        )]);
        roff.control("TP", [])
            .text([bold(".vtcode/")])
            .text([roman("Project cache and context directory")]);

        roff.control("SH", ["SAFETY"]);
        roff.control("TP", []).text([roman(
            "apply_patch: reserve for reviewed diffs or small batches. For large refactors or critical files, stage local backups and prefer edit_file/write_file to avoid partial rewrites if a patch fails.",
        )]);
        roff.control("TP", []).text([roman(
            "Timeout governance: tune [timeouts] in vtcode.toml to clamp tool duration. VT Code warns once execution passes the configured warning threshold so you can cancel runaway commands.",
        )]);

        roff.control("SH", ["EXAMPLES"]);
        for (label, example) in [
            ("Start interactive chat:", "vtcode chat"),
            ("Ask a question:", "vtcode ask \"Explain Rust ownership\""),
            ("Create a web project:", "vtcode create-project myapp --feature web --feature auth"),
            ("Generate man page:", "vtcode man chat"),
            ("Run ast-grep checks for the current workspace:", "vtcode check ast-grep"),
        ] {
            roff.text([roman(label)]);
            roff.text([bold(format!("  {example}"))]);
        }

        roff.control("SH", ["SEE ALSO"]);
        roff.text([roman("Full documentation: https://github.com/vinhnx/vtcode")]);
        roff.text([roman("Related commands: cargo(1), rustc(1), git(1)")]);

        Self::append_roff(buf, &roff)
    }

    /// Append curated examples and a SEE ALSO section to a command man page.
    fn append_command_sections(buf: &mut Vec<u8>, examples: &[&str]) -> Result<()> {
        let mut roff = Roff::new();
        if !examples.is_empty() {
            roff.control("SH", ["EXAMPLES"]);
            for example in examples {
                roff.text([bold(format!("  {example}"))]);
            }
        }
        roff.control("SH", ["SEE ALSO"]);
        roff.text([bold("vtcode(1)")]);
        Self::append_roff(buf, &roff)
    }

    /// Append rendered roff content, stripping the duplicate preamble that
    /// [`Roff::render`] emits (clap_mangen already wrote one).
    fn append_roff(buf: &mut Vec<u8>, roff: &Roff) -> Result<()> {
        let rendered = roff.render();
        let body = match rendered.find(".SH ") {
            Some(index) => &rendered[index..],
            None => return Ok(()),
        };
        buf.write_all(body.as_bytes()).context("failed to append man page sections")?;
        Ok(())
    }
}
