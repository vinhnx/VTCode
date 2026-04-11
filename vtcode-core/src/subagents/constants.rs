use crate::config::constants::tools;

pub(crate) const SUBAGENT_TRANSCRIPT_LINE_LIMIT: usize = 200;
pub(crate) const SUBAGENT_MEMORY_BYTES_LIMIT: usize = 25 * 1024;
pub(crate) const SUBAGENT_MEMORY_LINE_LIMIT: usize = 200;
pub(crate) const SUBAGENT_MEMORY_HIGHLIGHT_LIMIT: usize = 4;
pub(crate) const SUBAGENT_HARD_CONCURRENCY_LIMIT: usize = 3;
pub(crate) const SUBAGENT_MIN_MAX_TURNS: usize = 2;
pub(crate) const SUBAGENT_PREVIEW_LINES: usize = 24;

pub(crate) const VAGUE_SUBAGENT_PROMPTS: &[&str] = &[
    "analyze",
    "analyse",
    "check",
    "current state",
    "explore",
    "help",
    "inspect",
    "inspect current state",
    "look",
    "look around",
    "report",
    "report findings",
    "report status",
    "review",
    "status",
    "summarise",
    "summarize",
    "summary",
];

pub(crate) const SUBAGENT_TOOL_NAMES: &[&str] = &[
    tools::SPAWN_AGENT,
    tools::SEND_INPUT,
    tools::WAIT_AGENT,
    tools::RESUME_AGENT,
    tools::CLOSE_AGENT,
];

pub(crate) const NON_MUTATING_TOOL_PREFIXES: &[&str] = &[
    tools::UNIFIED_SEARCH,
    tools::READ_FILE,
    tools::LIST_SKILLS,
    tools::LOAD_SKILL,
    tools::LOAD_SKILL_RESOURCE,
];
