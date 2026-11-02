use super::*;
use crate::config::types::CapabilityLevel;
use crate::core::agent::hash_utils::hash_value;
use crate::prompts::context::UserPreferences;
use crate::skills::model::SkillMetadata;
use std::path::PathBuf;

// Retain the owned representation as a compatibility oracle, independent of
// the borrowed views and sorting helper used by production.
fn owned_context_digest(context: Option<&PromptContext>) -> u64 {
    let Some(context) = context else {
        return hash_value(&"context-free");
    };
    let mut languages = context.languages.clone();
    languages.sort();
    let mut tools = context.available_tools.clone();
    tools.sort();
    let mut skills = context.available_skills.clone();
    skills.sort();
    let mut metadata = context
        .available_skill_metadata
        .iter()
        .map(|skill| {
            (
                skill.name.clone(),
                skill.description.clone(),
                skill.short_description.clone(),
                skill.path.clone(),
                skill.scope,
                skill.manifest.as_ref().map(|manifest| format!("{manifest:?}")),
            )
        })
        .collect::<Vec<_>>();
    metadata.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.3.cmp(&right.3)));
    let preferences = context.user_preferences.as_ref().map(|preferences| {
        let mut languages = preferences.preferred_languages.clone();
        languages.sort();
        let mut frameworks = preferences.preferred_frameworks.clone();
        frameworks.sort();
        (languages, preferences.coding_style.clone(), frameworks)
    });
    hash_value(&(
        context.workspace.as_ref(),
        languages,
        context.project_type.as_deref(),
        tools,
        skills,
        metadata,
        preferences,
        context.capability_level.map(|level| format!("{level:?}")),
        context.current_directory.as_ref(),
    ))
}

fn context_fixture() -> PromptContext {
    PromptContext {
        workspace: Some(PathBuf::from("/workspace/cache-fixture")),
        languages: vec!["Rust".into(), "Go".into(), "Rust".into(), "Zig".into()],
        project_type: Some("library".into()),
        available_tools: vec!["z_search".into(), "a_read".into()],
        available_skills: vec![
            ("zeta".into(), "Inspect logs".into()),
            ("alpha".into(), "Edit code".into()),
        ],
        available_skill_metadata: vec![
            SkillMetadata {
                name: "zeta".into(),
                description: "Inspect logs".into(),
                short_description: Some("Log analysis".into()),
                path: PathBuf::from("/skills/zeta/SKILL.md"),
                scope: SkillScope::Repo,
                manifest: Some(Box::default()),
            },
            SkillMetadata {
                name: "alpha".into(),
                description: "Edit code".into(),
                short_description: None,
                path: PathBuf::from("/skills/alpha/SKILL.md"),
                scope: SkillScope::User,
                manifest: None,
            },
            SkillMetadata {
                name: "zeta".into(),
                description: "System log analysis".into(),
                short_description: None,
                path: PathBuf::from("/system/zeta/SKILL.md"),
                scope: SkillScope::System,
                manifest: None,
            },
        ],
        user_preferences: Some(UserPreferences {
            preferred_languages: vec!["Rust".into(), "Go".into()],
            coding_style: Some("four spaces".into()),
            preferred_frameworks: vec!["Tokio".into(), "Axum".into()],
        }),
        capability_level: Some(CapabilityLevel::FileReading),
        current_directory: Some(PathBuf::from("/workspace/cache-fixture/src")),
    }
}

#[test]
fn borrowed_context_digest_matches_owned_format_and_preserves_discovery_order() {
    assert_eq!(prompt_context_digest(None), owned_context_digest(None));
    let default = PromptContext::default();
    assert_eq!(prompt_context_digest(Some(&default)), owned_context_digest(Some(&default)));
    assert_ne!(prompt_context_digest(None), prompt_context_digest(Some(&default)));

    let context = context_fixture();
    let before = format!("{context:?}");
    let expected = owned_context_digest(Some(&context));
    assert_eq!(prompt_context_digest(Some(&context)), expected);
    assert_eq!(format!("{context:?}"), before, "hashing must not sort or change the source context");

    let mut reordered = context.clone();
    reordered.languages.reverse();
    reordered.available_tools.reverse();
    reordered.available_skills.reverse();
    reordered.available_skill_metadata.reverse();
    let preferences = reordered.user_preferences.as_mut().unwrap();
    preferences.preferred_languages.reverse();
    preferences.preferred_frameworks.reverse();
    assert_eq!(prompt_context_digest(Some(&reordered)), expected);
    assert_eq!(prompt_context_digest(Some(&reordered)), owned_context_digest(Some(&reordered)));
}

#[test]
fn borrowed_context_digest_invalidates_each_prompt_bearing_field() {
    let context = context_fixture();
    let initial = prompt_context_digest(Some(&context));
    let edits: &[fn(&mut PromptContext)] = &[
        |context| context.workspace = None,
        |context| context.languages.push("Python".into()),
        |context| {
            context.languages.remove(0);
        },
        |context| context.project_type = None,
        |context| context.available_tools[0].push_str("_changed"),
        |context| context.available_skills[0].1.push_str(" carefully"),
        |context| context.available_skill_metadata[0].name.push_str("_changed"),
        |context| context.available_skill_metadata[0].description.push_str(" carefully"),
        |context| context.available_skill_metadata[0].short_description = None,
        |context| context.available_skill_metadata[0].path.push("changed"),
        |context| context.available_skill_metadata[0].scope = SkillScope::Admin,
        |context| {
            context.available_skill_metadata[0]
                .manifest
                .as_mut()
                .unwrap()
                .description
                .push_str("changed")
        },
        |context| context.available_skill_metadata[0].manifest = None,
        |context| {
            context
                .user_preferences
                .as_mut()
                .unwrap()
                .preferred_languages
                .push("Python".into())
        },
        |context| context.user_preferences.as_mut().unwrap().coding_style = None,
        |context| {
            context
                .user_preferences
                .as_mut()
                .unwrap()
                .preferred_frameworks
                .push("Actix".into())
        },
        |context| context.user_preferences = None,
        |context| context.capability_level = None,
        |context| context.current_directory = None,
    ];
    for (index, edit) in edits.iter().enumerate() {
        let mut changed = context.clone();
        edit(&mut changed);
        let digest = prompt_context_digest(Some(&changed));
        assert_ne!(digest, initial, "edit {index} must invalidate the identity");
        assert_eq!(digest, owned_context_digest(Some(&changed)), "edit {index} must preserve hash compatibility");
    }
}

#[test]
fn sorted_context_views_borrow_original_elements() {
    let names = ["zeta".to_owned(), "alpha".to_owned()];
    let sorted = sorted_refs(&names);
    assert!(std::ptr::eq(sorted[0], &names[1]));
    assert!(std::ptr::eq(sorted[1], &names[0]));
    assert_eq!(names, ["zeta", "alpha"]);
}
