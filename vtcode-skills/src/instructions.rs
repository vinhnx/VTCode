use std::path::PathBuf;

/// Prefix for skill instructions (used to detect skill instruction messages).
pub const SKILL_INSTRUCTIONS_PREFIX: &str = "<skill>";

#[derive(Debug, Clone)]
pub struct SkillInstructions {
    pub name: String,
    pub path: PathBuf,
    pub contents: String,
}

impl SkillInstructions {
    /// Check if a message contains skill instructions.
    pub fn is_skill_instructions(text: &str) -> bool {
        text.starts_with(SKILL_INSTRUCTIONS_PREFIX)
    }

    /// Format skill instructions as an XML-wrapped message (Codex-compatible format).
    pub fn to_xml_message(&self) -> String {
        let path_str = self.path.to_string_lossy().replace('\\', "/");
        format!(
            "<skill>\n<name>{}</name>\n<path>{}</path>\n{}\n</skill>",
            self.name, path_str, self.contents
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_instructions_prefix_detection() {
        let msg = "<skill>\n<name>test</name>\n<path>/path</path>\ncontents\n</skill>";
        assert!(SkillInstructions::is_skill_instructions(msg));

        let non_skill = "Regular message without skill";
        assert!(!SkillInstructions::is_skill_instructions(non_skill));
    }

    #[test]
    fn test_skill_instructions_xml_format() {
        let skill = SkillInstructions {
            name: "test-skill".to_string(),
            path: PathBuf::from("/path/to/skill/SKILL.md"),
            contents: "# Test Skill\n\nInstructions here.".to_string(),
        };

        let xml = skill.to_xml_message();
        assert!(xml.starts_with("<skill>"));
        assert!(xml.contains("<name>test-skill</name>"));
        assert!(xml.contains("<path>/path/to/skill/SKILL.md</path>"));
        assert!(xml.contains("# Test Skill"));
        assert!(xml.ends_with("</skill>"));
    }
}
