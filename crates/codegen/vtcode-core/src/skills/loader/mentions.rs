use hashbrown::HashSet;

use crate::skills::types::SkillManifest;

#[derive(Debug, Clone, Copy)]
pub struct SkillMentionDetectionOptions {
    pub enable_auto_trigger: bool,
    pub enable_description_matching: bool,
    pub min_keyword_matches: usize,
}

impl Default for SkillMentionDetectionOptions {
    fn default() -> Self {
        Self {
            enable_auto_trigger: true,
            enable_description_matching: true,
            min_keyword_matches: 2,
        }
    }
}

/// Detect skill mentions using default routing options.
pub fn detect_skill_mentions(user_input: &str, available_skills: &[SkillManifest]) -> Vec<String> {
    detect_skill_mentions_with_options(user_input, available_skills, &SkillMentionDetectionOptions::default())
}

/// Detect skill mentions using explicit routing options.
///
/// Routing policy:
/// - Explicit `$skill-name` mentions always win.
/// - Description and manifest metadata keywords provide the only implicit signals.
pub fn detect_skill_mentions_with_options(
    user_input: &str,
    available_skills: &[SkillManifest],
    options: &SkillMentionDetectionOptions,
) -> Vec<String> {
    if !options.enable_auto_trigger {
        return Vec::new();
    }

    let mut mentions = Vec::new();
    let input_lower = user_input.to_lowercase();
    let input_keywords = extract_keywords(user_input);
    let min_matches = options.min_keyword_matches.max(1);

    for skill in available_skills {
        let skill_name_lower = skill.name.to_lowercase();
        let explicit_trigger = format!("${skill_name_lower}");
        if input_lower.contains(&explicit_trigger) {
            mentions.push(skill.name.clone());
            continue;
        }

        if !options.enable_description_matching {
            continue;
        }

        let skill_keywords = skill_routing_keywords(skill);
        let keyword_matches = overlap_count(&input_keywords, &skill_keywords);
        if keyword_matches >= min_matches {
            mentions.push(skill.name.clone());
        }
    }

    mentions.sort();
    mentions.dedup();
    mentions
}

fn overlap_count(input_keywords: &HashSet<String>, skill_keywords: &HashSet<String>) -> usize {
    input_keywords.intersection(skill_keywords).count()
}

fn skill_routing_keywords(skill: &SkillManifest) -> HashSet<String> {
    let mut keywords = extract_keywords(&skill.description);

    let Some(metadata) = &skill.metadata else {
        return keywords;
    };
    for metadata_keywords in [metadata.get("keywords"), metadata.get("tags")].into_iter().flatten() {
        match metadata_keywords {
            serde_json::Value::Array(values) => {
                for value in values {
                    if let Some(keyword) = value.as_str() {
                        keywords.extend(extract_keywords(keyword));
                    }
                }
            }
            serde_json::Value::String(value) => {
                keywords.extend(extract_keywords(value));
            }
            _ => {}
        }
    }

    keywords
}

fn extract_keywords(text: &str) -> HashSet<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "with", "from", "that", "this", "when", "where", "what", "your", "for", "into", "onto", "than",
        "then", "also", "only", "should", "would", "could", "have", "has", "had", "use", "using", "task", "tasks",
        "help", "need", "want",
    ];

    text.split(|c: char| !c.is_alphanumeric())
        .map(|part| part.trim().to_lowercase())
        .filter(|part| part.len() > 2)
        .filter(|part| !STOPWORDS.contains(&part.as_str()))
        .collect()
}
