pub const DEFAULT_MODEL: &str = MIMO_V2_5_PRO;

pub const MIMO_V2_5_PRO: &str = "mimo-v2.5-pro";
pub const MIMO_V2_5: &str = "mimo-v2.5";
pub const MIMO_V2_5_ASR: &str = "mimo-v2.5-asr";
pub const MIMO_V2_5_TTS: &str = "mimo-v2.5-tts";
pub const MIMO_V2_5_TTS_VOICECLONE: &str = "mimo-v2.5-tts-voiceclone";
pub const MIMO_V2_5_TTS_VOICEDESIGN: &str = "mimo-v2.5-tts-voicedesign";
pub const MIMO_V2_PRO: &str = "mimo-v2-pro";
pub const MIMO_V2_OMNI: &str = "mimo-v2-omni";
pub const MIMO_V2_TTS: &str = "mimo-v2-tts";

/// Models available via pay-as-you-go API
pub const PAYG_MODELS: &[&str] = &[MIMO_V2_5_PRO, MIMO_V2_5];

/// Models available via Token Plan (superset of pay-as-you-go)
pub const TOKEN_PLAN_MODELS: &[&str] = &[
    MIMO_V2_5_PRO,
    MIMO_V2_5,
    MIMO_V2_5_ASR,
    MIMO_V2_5_TTS,
    MIMO_V2_5_TTS_VOICECLONE,
    MIMO_V2_5_TTS_VOICEDESIGN,
    MIMO_V2_PRO,
    MIMO_V2_OMNI,
    MIMO_V2_TTS,
];

/// All supported models (union of both auth methods)
pub const SUPPORTED_MODELS: &[&str] = TOKEN_PLAN_MODELS;
