use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt::Write as _;

/// Fraction of mean usage below which the health report flags a skill as
/// underutilized (a candidate for pruning or merging).
const DOCTOR_UNDERUTILIZED_THRESHOLD: f64 = 0.5;

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
        // Idempotent: re-creating a tracked skill must not reset its history
        // (activation can follow a direct `/skills use`, which also registers).
        if self.skill_stats.contains_key(&skill_name) {
            return;
        }
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
        // Use remove directly - returns Option<V>, avoids redundant contains_key check
        if self.skill_stats.remove(&skill_name).is_some() {
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
            stats.total_duration_ms.checked_div(stats.execution_count).unwrap_or(0)
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

    /// Render a plain-text skill health report for `/skills doctor`.
    ///
    /// Pure over the current snapshot: per-skill usage rows sorted by name,
    /// then the skills below half of mean usage as prune-or-merge candidates.
    #[must_use]
    pub fn doctor_report(&self) -> String {
        if self.skill_stats.is_empty() {
            return "No skills tracked yet this session.".to_string();
        }

        let mut report = String::new();
        let removed = self.total_skills.saturating_sub(self.active_skills);
        let _ = writeln!(
            report,
            "Skill health: {} active, {} removed ({} total) | {} executions | reuse ratio {:.1}",
            self.active_skills,
            removed,
            self.total_skills,
            self.total_executions,
            self.reuse_ratio()
        );

        let mut names: Vec<&str> = self.skill_stats.keys().map(String::as_str).collect();
        names.sort_unstable();
        report.push_str("name | kind | execs | success | avg ms | last used\n");
        for name in names {
            let Some(stats) = self.skill_stats.get(name) else {
                continue;
            };
            let success_pct = self.get_skill_success_rate(name) * 100.0;
            let _ = writeln!(
                report,
                "{} | {} | {} | {:.0}% | {} | {}",
                stats.name,
                stats.language,
                stats.execution_count,
                success_pct,
                self.get_skill_avg_duration(name),
                stats.last_used.format("%Y-%m-%d %H:%M:%S UTC")
            );
        }

        let mut underutilized = self.get_underutilized_skills(DOCTOR_UNDERUTILIZED_THRESHOLD);
        underutilized.sort();
        if underutilized.is_empty() {
            report.push_str("Underutilized: none.");
        } else {
            let _ = writeln!(report, "Underutilized (below 50% of mean usage) - prune or merge candidates:");
            for name in underutilized {
                let execs = self.skill_stats.get(&name).map(|stats| stats.execution_count).unwrap_or(0);
                let _ = writeln!(report, "- {name} ({execs} execs)");
            }
        }
        report
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
        metrics.record_created("filter_test_files".to_owned(), "python3".to_owned());

        assert_eq!(metrics.total_skills, 1);
        assert_eq!(metrics.active_skills, 1);
        assert!(metrics.skill_stats.contains_key("filter_test_files"));
    }

    #[test]
    fn test_record_deleted() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("skill1".to_owned(), "python3".to_owned());
        metrics.record_deleted("skill1".to_owned());

        assert_eq!(metrics.active_skills, 0);
        assert!(!metrics.skill_stats.contains_key("skill1"));
    }

    #[test]
    fn test_record_execution() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("analyze".to_owned(), "python3".to_owned());
        metrics.record_execution("analyze".to_owned(), 1000, true);
        metrics.record_execution("analyze".to_owned(), 950, true);

        assert_eq!(metrics.total_executions, 2);
        let stats = metrics.skill_stats.get("analyze").unwrap();
        assert_eq!(stats.execution_count, 2);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.total_duration_ms, 1950);
    }

    #[test]
    fn test_reuse_ratio() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("skill1".to_owned(), "python3".to_owned());
        metrics.record_created("skill2".to_owned(), "javascript".to_owned());
        metrics.record_execution("skill1".to_owned(), 100, true);
        metrics.record_execution("skill1".to_owned(), 100, true);
        metrics.record_execution("skill2".to_owned(), 200, true);

        assert!((metrics.reuse_ratio() - 3.0 / 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("test".to_owned(), "python3".to_owned());
        metrics.record_execution("test".to_owned(), 100, true);
        metrics.record_execution("test".to_owned(), 100, true);
        metrics.record_execution("test".to_owned(), 100, false);

        let success_rate = metrics.get_skill_success_rate("test");
        assert!((success_rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_avg_duration() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("perf".to_owned(), "javascript".to_owned());
        metrics.record_execution("perf".to_owned(), 500, true);
        metrics.record_execution("perf".to_owned(), 600, true);
        metrics.record_execution("perf".to_owned(), 400, true);

        let avg = metrics.get_skill_avg_duration("perf");
        assert_eq!(avg, 500);
    }

    #[test]
    fn test_underutilized_skills() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("popular".to_owned(), "python3".to_owned());
        metrics.record_created("unpopular".to_owned(), "python3".to_owned());

        for _ in 0..10 {
            metrics.record_execution("popular".to_owned(), 100, true);
        }
        metrics.record_execution("unpopular".to_owned(), 100, true);

        let underutilized = metrics.get_underutilized_skills(0.5);
        assert!(underutilized.contains(&"unpopular".to_owned()));
    }

    #[test]
    fn test_record_created_is_idempotent_and_preserves_history() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("alpha".to_owned(), "agent_skill".to_owned());
        metrics.record_execution("alpha".to_owned(), 100, true);

        metrics.record_created("alpha".to_owned(), "agent_skill".to_owned());

        assert_eq!(metrics.total_skills, 1);
        assert_eq!(metrics.active_skills, 1);
        let stats = metrics.skill_stats.get("alpha").unwrap();
        assert_eq!(stats.execution_count, 1);
        assert_eq!(stats.success_count, 1);
    }

    #[test]
    fn test_doctor_report_empty_state() {
        let report = SkillMetrics::new().doctor_report();

        assert_eq!(report, "No skills tracked yet this session.");
    }

    #[test]
    fn test_doctor_report_flags_quiet_skill_and_keeps_busy_one() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("busy".to_owned(), "agent_skill".to_owned());
        metrics.record_created("quiet".to_owned(), "built_in".to_owned());
        for _ in 0..10 {
            metrics.record_execution("busy".to_owned(), 100, true);
        }
        metrics.record_execution("busy".to_owned(), 100, false);
        metrics.record_execution("quiet".to_owned(), 50, true);

        let report = metrics.doctor_report();

        assert!(report.contains("2 active"), "{report}");
        assert!(report.contains("12 executions"), "{report}");
        // Rows sorted by name with independently derived success math:
        // busy 10/11 -> 91%, quiet 1/1 -> 100%.
        let busy_pos = report.find("busy | agent_skill | 11 | 91% | 100 |").expect("busy row");
        let quiet_pos = report.find("quiet | built_in | 1 | 100% | 50 |").expect("quiet row");
        assert!(busy_pos < quiet_pos, "rows sorted by name:\n{report}");
        assert!(report.contains("- quiet (1 execs)"), "{report}");
        assert!(!report.contains("- busy ("), "{report}");
    }

    #[test]
    fn test_doctor_report_without_executions_names_no_candidates() {
        let mut metrics = SkillMetrics::new();
        metrics.record_created("idle".to_owned(), "agent_skill".to_owned());

        let report = metrics.doctor_report();

        assert!(report.contains("idle | agent_skill | 0 | 0% | 0 |"), "{report}");
        assert!(report.contains("Underutilized: none."), "{report}");
    }
}
