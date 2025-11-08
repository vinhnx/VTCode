use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetrics {
    pub total_skills: u64,
    pub active_skills: u64,
    pub total_executions: u64,
    pub skill_stats: HashMap<String, SkillStats>,
    pub recent_skill_usage: VecDeque<SkillUsageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStats {
    pub name: String,
    pub language: String,
    pub execution_count: u64,
    pub success_count: u64,
    pub total_duration_ms: u64,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsageRecord {
    pub skill_name: String,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: DateTime<Utc>,
}

impl SkillMetrics {
    pub fn new() -> Self {
        Self {
            total_skills: 0,
            active_skills: 0,
            total_executions: 0,
            skill_stats: HashMap::new(),
            recent_skill_usage: VecDeque::with_capacity(100),
        }
    }

    pub fn record_created(&mut self, skill_name: String, language: String) {
        self.total_skills += 1;
        self.active_skills += 1;

        self.skill_stats.insert(
            skill_name.clone(),
            SkillStats {
                name: skill_name,
                language,
                execution_count: 0,
                success_count: 0,
                total_duration_ms: 0,
                created_at: Utc::now(),
                last_used: Utc::now(),
            },
        );
    }

    pub fn record_deleted(&mut self, skill_name: String) {
        if self.skill_stats.contains_key(&skill_name) {
            self.skill_stats.remove(&skill_name);
            self.active_skills = self.active_skills.saturating_sub(1);
        }
    }

    pub fn record_execution(&mut self, skill_name: String, duration_ms: u64, success: bool) {
        self.total_executions += 1;

        if let Some(stats) = self.skill_stats.get_mut(&skill_name) {
            stats.execution_count += 1;
            if success {
                stats.success_count += 1;
            }
            stats.total_duration_ms += duration_ms;
            stats.last_used = Utc::now();
        }

        let record = SkillUsageRecord {
            skill_name,
            success,
            duration_ms,
            timestamp: Utc::now(),
        };

        if self.recent_skill_usage.len() >= 100 {
            self.recent_skill_usage.pop_front();
        }
        self.recent_skill_usage.push_back(record);
    }

    pub fn reuse_ratio(&self) -> f64 {
        if self.active_skills > 0 && self.total_executions > 0 {
            self.total_executions as f64 / self.active_skills as f64
        } else {
            0.0
        }
    }

    pub fn get_skill_success_rate(&self, skill_name: &str) -> f64 {
        if let Some(stats) = self.skill_stats.get(skill_name) {
            if stats.execution_count > 0 {
                stats.success_count as f64 / stats.execution_count as f64
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    pub fn get_skill_avg_duration(&self, skill_name: &str) -> u64 {
        if let Some(stats) = self.skill_stats.get(skill_name) {
            if stats.execution_count > 0 {
                stats.total_duration_ms / stats.execution_count
            } else {
                0
            }
        } else {
            0
        }
    }

    pub fn get_underutilized_skills(&self, threshold: f64) -> Vec<String> {
        let avg_usage = if self.active_skills > 0 {
            self.total_executions as f64 / self.active_skills as f64
        } else {
            0.0
        };

        self.skill_stats
            .iter()
            .filter(|(_, stats)| (stats.execution_count as f64) < (avg_usage * threshold))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl Default for SkillMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_created() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("filter_test_files".to_string(), "python3".to_string());

        assert_eq!(metrics.total_skills, 1);
        assert_eq!(metrics.active_skills, 1);
        assert!(metrics.skill_stats.contains_key("filter_test_files"));
    }

    #[test]
    fn test_record_deleted() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("skill1".to_string(), "python3".to_string());
        metrics.record_deleted("skill1".to_string());

        assert_eq!(metrics.active_skills, 0);
        assert!(!metrics.skill_stats.contains_key("skill1"));
    }

    #[test]
    fn test_record_execution() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("analyze".to_string(), "python3".to_string());
        metrics.record_execution("analyze".to_string(), 1000, true);
        metrics.record_execution("analyze".to_string(), 950, true);

        assert_eq!(metrics.total_executions, 2);
        let stats = metrics.skill_stats.get("analyze").unwrap();
        assert_eq!(stats.execution_count, 2);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.total_duration_ms, 1950);
    }

    #[test]
    fn test_reuse_ratio() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("skill1".to_string(), "python3".to_string());
        metrics.record_created("skill2".to_string(), "javascript".to_string());
        metrics.record_execution("skill1".to_string(), 100, true);
        metrics.record_execution("skill1".to_string(), 100, true);
        metrics.record_execution("skill2".to_string(), 200, true);

        assert_eq!(metrics.reuse_ratio(), 3.0 / 2.0);
    }

    #[test]
    fn test_success_rate() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("test".to_string(), "python3".to_string());
        metrics.record_execution("test".to_string(), 100, true);
        metrics.record_execution("test".to_string(), 100, true);
        metrics.record_execution("test".to_string(), 100, false);

        let success_rate = metrics.get_skill_success_rate("test");
        assert!((success_rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_avg_duration() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("perf".to_string(), "javascript".to_string());
        metrics.record_execution("perf".to_string(), 500, true);
        metrics.record_execution("perf".to_string(), 600, true);
        metrics.record_execution("perf".to_string(), 400, true);

        let avg = metrics.get_skill_avg_duration("perf");
        assert_eq!(avg, 500);
    }

    #[test]
    fn test_underutilized_skills() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("popular".to_string(), "python3".to_string());
        metrics.record_created("unpopular".to_string(), "python3".to_string());

        for _ in 0..10 {
            metrics.record_execution("popular".to_string(), 100, true);
        }
        metrics.record_execution("unpopular".to_string(), 100, true);

        let underutilized = metrics.get_underutilized_skills(0.5);
        assert!(underutilized.contains(&"unpopular".to_string()));
    }
}
