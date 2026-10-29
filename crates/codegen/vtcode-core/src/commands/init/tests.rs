use super::*;
use tempfile::TempDir;

fn write_file(dir: &TempDir, relative: &str, contents: &str) {
    let path = dir.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write file");
}

#[test]
fn no_questions_when_clear_signals_exist() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write_file(&workspace, "README.md", "# Demo\n");
    write_file(&workspace, "scripts/check.sh", "#!/bin/sh\ncargo check\n");

    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    assert!(plan.questions.is_empty());
    assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Skip);
}

#[test]
fn emits_verification_question_when_multiple_strong_candidates_exist() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write_file(
        &workspace,
        "README.md",
        "Run ./scripts/check.sh for the full gate.\nUse cargo nextest run during local work.\n",
    );
    write_file(&workspace, "scripts/check.sh", "#!/bin/sh\ncargo nextest run\n");
    write_file(
        &workspace,
        ".github/workflows/ci.yml",
        "jobs:\n  test:\n    steps:\n      - run: cargo nextest run\n",
    );

    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    assert!(
        plan.questions
            .iter()
            .any(|question| question.key == GuidedInitQuestionKey::VerificationCommand)
    );
}

#[test]
fn emits_orientation_question_when_multiple_docs_are_plausible() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "README.md", "See docs/ARCHITECTURE.md for design.\n");
    write_file(&workspace, "docs/ARCHITECTURE.md", "# Architecture\n");
    write_file(&workspace, "docs/modules/vtcode_docs_map.md", "# Docs Map\nUse this first.\n");

    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    assert!(
        plan.questions
            .iter()
            .any(|question| question.key == GuidedInitQuestionKey::OrientationDoc)
    );
}

#[test]
fn chosen_answers_override_heuristics() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write_file(&workspace, "README.md", "# Demo\n");
    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    let mut answers = GuidedInitAnswers::default();
    answers.insert(GuidedInitAnswer {
        key: GuidedInitQuestionKey::VerificationCommand,
        selected: "cargo nextest run".to_owned(),
        custom: None,
    });

    let rendered = render_agents_md(&plan, &answers).expect("rendered");
    assert!(rendered.contains("Default verification command: `cargo nextest run`."));
}

#[test]
fn critical_instruction_none_omits_section() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "README.md", "Use Conventional Commits.\n");
    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    let mut answers = GuidedInitAnswers::default();
    answers.insert(GuidedInitAnswer {
        key: GuidedInitQuestionKey::CriticalInstruction,
        selected: CONTROL_NONE.to_owned(),
        custom: None,
    });

    let rendered = render_agents_md(&plan, &answers).expect("rendered");
    assert!(!rendered.contains("## Important instructions"));
}

#[test]
fn render_is_deterministic_for_same_analysis_and_answers() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "README.md", "# Demo\n");
    let plan = prepare_guided_init(workspace.path(), false).expect("plan");
    let answers = GuidedInitAnswers::default();

    let left = render_agents_md(&plan, &answers).expect("left");
    let right = render_agents_md(&plan, &answers).expect("right");

    assert_eq!(left, right);
}

#[test]
fn existing_agents_requires_confirmation_without_force() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "AGENTS.md", "# Existing\n");

    let plan = prepare_guided_init(workspace.path(), false).expect("plan");

    assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Confirm);
}

#[test]
fn force_skips_confirmation_for_existing_agents() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "AGENTS.md", "# Existing\n");

    let plan = prepare_guided_init(workspace.path(), true).expect("plan");

    assert_eq!(plan.overwrite_state, GuidedInitOverwriteState::Force);
}

#[test]
fn write_agents_file_skips_existing_when_not_overwriting() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "AGENTS.md", "# Existing\n");

    let report = write_agents_file(workspace.path(), "# Fresh\n", false).expect("report");

    assert_eq!(report.status, GenerateAgentsFileStatus::SkippedExisting);
}

#[test]
fn blank_custom_answers_normalize_to_control_values() {
    let verification = GuidedInitAnswer::from_input(GuidedInitQuestionKey::VerificationCommand, None, Some("   "))
        .expect("verification");
    assert_eq!(verification.selected, CONTROL_GENERIC);
    assert_eq!(verification.custom, None);

    let critical =
        GuidedInitAnswer::from_input(GuidedInitQuestionKey::CriticalInstruction, None, Some("")).expect("critical");
    assert_eq!(critical.selected, CONTROL_NONE);
    assert_eq!(critical.custom, None);
}

#[test]
fn grounding_conflict_reopens_verification_choice() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    write_file(&workspace, "README.md", "# Demo\n");
    write_file(&workspace, "scripts/check.sh", "#!/bin/sh\ncargo check\n");

    let plan = prepare_guided_init(workspace.path(), false)
        .expect("plan")
        .with_grounding(GuidedInitGrounding {
            verification_command: Some("cargo nextest run".to_owned()),
            ..GuidedInitGrounding::default()
        });

    assert!(
        plan.questions
            .iter()
            .any(|question| question.key == GuidedInitQuestionKey::VerificationCommand)
    );
}

#[test]
fn grounding_summary_and_defaults_render_into_agents() {
    let workspace = TempDir::new().expect("workspace");
    write_file(&workspace, "README.md", "# Demo\n");

    let plan = prepare_guided_init(workspace.path(), false)
        .expect("plan")
        .with_grounding(GuidedInitGrounding {
            project_summary: Some("Terminal-first coding agent for repository work.".to_owned()),
            verification_command: Some("cargo nextest run".to_owned()),
            ..GuidedInitGrounding::default()
        });

    let rendered = render_agents_md(&plan, &GuidedInitAnswers::default()).expect("rendered");
    assert!(rendered.contains("Terminal-first coding agent for repository work."));
    assert!(rendered.contains("Default verification command: `cargo nextest run`."));
}
