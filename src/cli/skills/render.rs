use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct TraditionalSkillRow {
    pub(super) status: &'static str,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) mode_suffix: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct CliToolRow {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) path: String,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedSkillSummary {
    pub(super) headline: String,
    pub(super) details: Vec<String>,
}

pub(super) fn print_list_header() {
    println!("Discovering skills from standard locations...\n");
}

pub(super) fn print_empty_list() {
    println!("No skills found.");
    println!("\nCreate a traditional skill:");
    println!("  vtcode skills create ./my-skill");
    println!("\nOr install skills in standard locations:");
    println!("  ~/.vtcode/skills/     (VT Code user skills)");
    println!("  .agents/skills/       (Project skills)");
    println!("  .vtcode/skills/       (Legacy project skills - deprecated)");
    println!("  ~/.claude/skills/     (Legacy compatibility)");
    println!("  ~/.codex/skills/      (Codex compatibility)");
}

pub(super) fn print_traditional_skills(rows: &[TraditionalSkillRow], warnings: &[String]) {
    if rows.is_empty() {
        return;
    }

    println!("Available Traditional Skills:");
    println!("{:-<70}", "");

    for row in rows {
        println!(
            "{} {}{}\n  {}\n",
            row.status, row.name, row.mode_suffix, row.description
        );
    }

    if warnings.is_empty() {
        return;
    }

    println!("\nCompatibility Notes:");
    for warning in warnings {
        println!("  {}", warning);
    }
    println!("\n  Use 'vtcode skills info <name>' for details and alternatives.");
}

pub(super) fn print_cli_tools(rows: &[CliToolRow]) {
    if rows.is_empty() {
        return;
    }

    println!("\nAvailable CLI Tool Skills:");
    println!("{:-<70}", "");

    for row in rows {
        println!(
            "⚡ {}\n  {}\n  Path: {}\n",
            row.name, row.description, row.path
        );
    }
}

pub(super) fn print_list_usage() {
    println!("\nUsage:");
    println!("  Load skill:    vtcode skills load <name>");
    println!("  Skill info:    vtcode skills info <name>");
    println!("  Use in chat:   /skills load <name>");
    println!("  Or:            /skills use <name> <input>");
}

pub(super) fn print_loaded_skill(summary: &LoadedSkillSummary) {
    println!("{}", summary.headline);
    for detail in &summary.details {
        println!("  {}", detail);
    }
}

pub(super) fn print_skill_ready(name: &str) {
    println!(
        "\nSkill is ready to use. Use it in chat mode or with: vtcode ask 'Use {} for...'",
        name
    );
}

pub(super) fn print_skill_config(workspace: &Path) {
    println!("Skill Configuration\n");
    println!("Workspace: {}", workspace.display());
    println!("\nSkill Search Paths (by precedence):");
    println!(
        "  • .github/skills/       (Agent Skills spec recommended - highest project precedence)"
    );
    println!("  • .agents/skills/       (VT Code native project skills)");
    println!("  • .vtcode/skills/       (Legacy VT Code project skills)");
    println!("  • .claude/skills/       (Claude Code legacy compatibility)");
    println!("  • .pi/skills/           (Pi framework project skills)");
    println!("  • .codex/skills/        (Codex compatibility)");
    println!("  • ./skills              (Generic project skills)");
    println!("  • ~/.vtcode/skills/     (VT Code user skills)");
    println!("  • ~/.copilot/skills/    (VS Code Copilot compatibility)");
    println!("  • ~/.claude/skills/     (Claude Code user compatibility)");
    println!("  • ~/.pi/agent/skills/   (Pi framework user skills)");
    println!("  • ~/.codex/skills/      (Codex user compatibility - lowest precedence)");

    println!("\nSkill Directory Structure:");
    println!("  my-skill/");
    println!("    ├── SKILL.md          (required: metadata + instructions)");
    println!("    ├── ADVANCED.md       (optional: additional guides)");
    println!("    ├── scripts/          (optional: executable scripts)");
    println!("    └── templates/        (optional: reference materials)");

    println!("\nEnvironment Variables:");
    println!("  • HOME - Used to locate user skill directories");
}
