/// Default temperature for main LLM responses (0.0-1.0)
/// Controls randomness/creativity: 0=deterministic, 1=random
/// 0.7 provides balanced creativity and consistency
pub const DEFAULT_TEMPERATURE: f32 = 0.7;

/// Default temperature for prompt refinement (0.0-1.0)
/// Lower than main temperature for more deterministic refinement
pub const DEFAULT_REFINE_TEMPERATURE: f32 = 0.3;
