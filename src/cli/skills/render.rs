use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct TraditionalSkillRow {
    pub(super) status: &'static str,
    pub(super) name: String,
    pub(super) description: String,
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
    println!("  .agents/skills/       (Repo skills, nearest directory first)");
    println!("  ~/.agents/skills/     (User skills)");
    println!("  /etc/codex/skills/    (Admin skills)");
}

pub(super) fn print_traditional_skills(rows: &[TraditionalSkillRow], warnings: &[String]) {
    if rows.is_empty() {
        return;
    }

    println!("Available Traditional Skills:");
    println!("{:-<70}", "");

    for row in rows {
        println!(
            "{} {}\n  {}\n",
            row.status, row.name, row.description
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
    println!("  • $CWD/.agents/skills/ ... repo-root/.agents/skills");
    println!("  • ~/.agents/skills/");
    println!("  • /etc/codex/skills/");
    println!("  • $CODEX_HOME/skills/.system/");

    println!("\nSkill Directory Structure:");
    println!("  my-skill/");
    println!("    ├── SKILL.md          (required: metadata + instructions)");
    println!("    ├── scripts/          (optional: executable scripts)");
    println!("    ├── references/       (optional: additional docs)");
    println!("    └── assets/           (optional: static resources)");

    println!("\nEnvironment Variables:");
    println!("  • HOME - Used to locate user skill directories");
    println!("  • CODEX_HOME - Used for bundled system skills cache");
}
