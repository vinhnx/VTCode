pub const GROK_BUILD_0_1: &str = "grok-build-0.1";
pub const GROK_4_5: &str = "grok-4.5";
pub const GROK_4_3: &str = "grok-4.3";
pub const GROK_4_20_REASONING: &str = "grok-4.20-0309-reasoning";
pub const GROK_4_20_NON_REASONING: &str = "grok-4.20-0309-non-reasoning";
pub const GROK_4_20_MULTI_AGENT: &str = "grok-4.20-multi-agent-0309";

pub const DEFAULT_MODEL: &str = GROK_BUILD_0_1;

pub const SUPPORTED_MODELS: &[&str] = &[
    GROK_4_5,
    GROK_4_3,
    GROK_4_20_REASONING,
    GROK_4_20_NON_REASONING,
    GROK_BUILD_0_1,
    GROK_4_20_MULTI_AGENT,
];

pub const REASONING_MODELS: &[&str] = &[GROK_4_5, GROK_4_20_REASONING];
