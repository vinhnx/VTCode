use serde::{Deserialize, Serialize};

/// Reinforcement learning strategy selection.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RlStrategy {
    Bandit,
    ActorCritic,
}

impl Default for RlStrategy {
    fn default() -> Self {
        RlStrategy::Bandit
    }
}

/// Bandit policy parameters.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BanditConfig {
    #[serde(default = "default_exploration")]
    pub exploration_epsilon: f32,

    #[serde(default = "default_window")]
    pub rolling_window: usize,

    #[serde(default = "default_latency_weight")]
    pub latency_weight: f32,
}

impl Default for BanditConfig {
    fn default() -> Self {
        Self {
            exploration_epsilon: default_exploration(),
            rolling_window: default_window(),
            latency_weight: default_latency_weight(),
        }
    }
}

/// Actor-critic tuning parameters.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActorCriticConfig {
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f32,

    #[serde(default = "default_discount_factor")]
    pub discount_factor: f32,

    #[serde(default = "default_trace_decay")]
    pub trace_decay: f32,
}

impl Default for ActorCriticConfig {
    fn default() -> Self {
        Self {
            learning_rate: default_learning_rate(),
            discount_factor: default_discount_factor(),
            trace_decay: default_trace_decay(),
        }
    }
}

/// Reward shaping knobs exposed to the runtime.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewardShapingConfig {
    #[serde(default = "default_success_reward")]
    pub success_reward: f32,

    #[serde(default = "default_timeout_penalty")]
    pub timeout_penalty: f32,

    #[serde(default = "default_latency_penalty")]
    pub latency_penalty_weight: f32,
}

impl Default for RewardShapingConfig {
    fn default() -> Self {
        Self {
            success_reward: default_success_reward(),
            timeout_penalty: default_timeout_penalty(),
            latency_penalty_weight: default_latency_penalty(),
        }
    }
}

/// RL configuration exposed under `[optimization]`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReinforcementLearningConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub strategy: RlStrategy,

    #[serde(default)]
    pub bandit: BanditConfig,

    #[serde(default)]
    pub actor_critic: ActorCriticConfig,

    #[serde(default)]
    pub reward_shaping: RewardShapingConfig,
}

impl Default for ReinforcementLearningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: RlStrategy::Bandit,
            bandit: BanditConfig::default(),
            actor_critic: ActorCriticConfig::default(),
            reward_shaping: RewardShapingConfig::default(),
        }
    }
}

fn default_exploration() -> f32 {
    0.1
}

fn default_window() -> usize {
    50
}

fn default_latency_weight() -> f32 {
    0.35
}

fn default_learning_rate() -> f32 {
    0.02
}

fn default_discount_factor() -> f32 {
    0.85
}

fn default_trace_decay() -> f32 {
    0.8
}

fn default_success_reward() -> f32 {
    1.0
}

fn default_timeout_penalty() -> f32 {
    -0.8
}

fn default_latency_penalty() -> f32 {
    0.25
}
