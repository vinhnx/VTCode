use std::time::SystemTime;

/// Reward signal emitted after an action completes.
#[derive(Debug, Clone)]
pub struct RewardSignal {
    pub action: String,
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub tokens_used: Option<u32>,
    pub emitted_at: SystemTime,
}

impl RewardSignal {
    pub fn new(action: impl Into<String>, success: bool, latency_ms: Option<u64>, tokens_used: Option<u32>) -> Self {
        Self {
            action: action.into(),
            success,
            latency_ms,
            tokens_used,
            emitted_at: SystemTime::now(),
        }
    }

    pub fn reward_value(&self, success_reward: f32, timeout_penalty: f32, latency_weight: f32) -> f32 {
        let latency_penalty = self.latency_ms.map(|ms| -(ms as f32) * latency_weight / 1000.0).unwrap_or(0.0);
        let base = if self.success { success_reward } else { timeout_penalty };
        base + latency_penalty
    }
}

/// Short-term memory of reward signals.
#[derive(Default, Debug)]
pub struct RewardLedger {
    pub signals: Vec<RewardSignal>,
    pub max_entries: usize,
}

impl RewardLedger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            signals: Vec::new(),
            max_entries,
        }
    }

    pub fn push(&mut self, signal: RewardSignal) {
        self.signals.push(signal);
        if self.signals.len() > self.max_entries {
            let drop = self.signals.len() - self.max_entries;
            self.signals.drain(0..drop);
        }
    }
}
