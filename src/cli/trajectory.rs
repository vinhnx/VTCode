use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Reverse;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::colors::style;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum Rec {
    #[serde(rename = "route")]
    Route {
        #[serde(rename = "turn")]
        _turn: usize,
        selected_model: String,
        class: String,
        ts: i64,
    },
    #[serde(rename = "tool")]
    Tool {
        #[serde(rename = "turn")]
        _turn: usize,
        name: String,
        #[serde(rename = "args")]
        _args: Value,
        ok: bool,
        ts: i64,
    },
    #[serde(rename = "prompt_cache_metrics")]
    PromptCacheMetrics {
        #[serde(rename = "turn")]
        _turn: usize,
        model: String,
        prompt_tokens: u32,
        #[serde(default)]
        cached_prompt_tokens: u32,
        #[serde(default)]
        cache_read_tokens: Option<u32>,
        #[serde(default)]
        cache_creation_tokens: Option<u32>,
        ts: i64,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PromptCacheModelStats {
    prompt_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    records: usize,
}

#[derive(Debug, Default)]
struct TrajectorySummary {
    class_counts: HashMap<String, usize>,
    model_counts: HashMap<String, usize>,
    tool_ok: HashMap<String, usize>,
    tool_err: HashMap<String, usize>,
    prompt_cache: HashMap<String, PromptCacheModelStats>,
    total_routes: usize,
    total_tools: usize,
    total_prompt_cache_records: usize,
    recent_timestamps: Vec<i64>,
}

fn summarize_trajectory<R: BufRead>(reader: R) -> Result<TrajectorySummary> {
    let mut summary = TrajectorySummary::default();

    for line in reader.lines() {
        let line = line?;
        let raw = line.trim();
        if raw.trim().is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<Rec>(raw) {
            match rec {
                Rec::Route {
                    selected_model,
                    class,
                    ts,
                    ..
                } => {
                    *summary.class_counts.entry(class).or_insert(0) += 1;
                    *summary.model_counts.entry(selected_model).or_insert(0) += 1;
                    summary.total_routes += 1;
                    summary.recent_timestamps.push(ts);
                }
                Rec::Tool { name, ok, ts, .. } => {
                    if ok {
                        *summary.tool_ok.entry(name).or_insert(0) += 1;
                    } else {
                        *summary.tool_err.entry(name).or_insert(0) += 1;
                    }
                    summary.total_tools += 1;
                    summary.recent_timestamps.push(ts);
                }
                Rec::PromptCacheMetrics {
                    model,
                    prompt_tokens,
                    cached_prompt_tokens,
                    cache_read_tokens,
                    cache_creation_tokens,
                    ts,
                    ..
                } => {
                    let stats = summary.prompt_cache.entry(model).or_default();
                    stats.prompt_tokens += prompt_tokens as u64;
                    stats.cache_read_tokens +=
                        cache_read_tokens.unwrap_or(cached_prompt_tokens) as u64;
                    stats.cache_creation_tokens += cache_creation_tokens.unwrap_or(0) as u64;
                    stats.records += 1;
                    summary.total_prompt_cache_records += 1;
                    summary.recent_timestamps.push(ts);
                }
            }
        }
    }

    Ok(summary)
}

pub async fn handle_trajectory_command(
    _cfg: &CoreAgentConfig,
    file: Option<PathBuf>,
    top: usize,
) -> Result<()> {
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let log_path = file.unwrap_or_else(|| workspace.join(".vtcode/logs/trajectory.jsonl"));
    let f =
        File::open(&log_path).with_context(|| format!("Failed to open {}", log_path.display()))?;
    let reader = BufReader::new(f);
    let mut summary = summarize_trajectory(reader)?;

    println!(
        "{} {}",
        style("Trajectory Report").magenta().bold(),
        style(log_path.display()).dim()
    );
    println!(
        "{} routes, {} tools, {} cache metrics",
        style(summary.total_routes).cyan(),
        style(summary.total_tools).cyan(),
        style(summary.total_prompt_cache_records).cyan()
    );

    // Show time range if we have timestamps
    if !summary.recent_timestamps.is_empty() {
        summary.recent_timestamps.sort();
        if let (Some(oldest), Some(newest)) = (
            summary.recent_timestamps.first(),
            summary.recent_timestamps.last(),
        ) {
            let oldest_time = format_timestamp(*oldest);
            let newest_time = format_timestamp(*newest);
            println!(
                "Time range: {} to {}",
                style(oldest_time).dim(),
                style(newest_time).dim()
            );
        }
    }

    // Classes
    if !summary.class_counts.is_empty() {
        println!("\n{}", style("Classes").bold());
        let mut classes: Vec<_> = summary.class_counts.into_iter().collect();
        classes.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        let total_class_usage: usize = classes.iter().map(|(_, c)| *c).sum();
        for (i, (k, v)) in classes.into_iter().take(top).enumerate() {
            let percentage = if total_class_usage > 0 {
                (v as f64) / (total_class_usage as f64) * 100.0
            } else {
                0.0
            };
            println!("{:>2}. {:<16} {:>4} ({:>5.1}%)", i + 1, k, v, percentage);
        }
    }

    // Models
    if !summary.model_counts.is_empty() {
        println!("\n{}", style("Models").bold());
        let mut models: Vec<_> = summary.model_counts.into_iter().collect();
        models.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        let total_model_usage: usize = models.iter().map(|(_, c)| *c).sum();
        for (i, (k, v)) in models.into_iter().take(top).enumerate() {
            let percentage = if total_model_usage > 0 {
                (v as f64) / (total_model_usage as f64) * 100.0
            } else {
                0.0
            };
            println!("{:>2}. {:<25} {:>4} ({:>5.1}%)", i + 1, k, v, percentage);
        }
    }

    // Tools
    if !summary.tool_ok.is_empty() || !summary.tool_err.is_empty() {
        println!("\n{}", style("Tools").bold());
        let mut tools: Vec<_> = summary
            .tool_ok
            .iter()
            .map(|(k, ok)| {
                let err = summary.tool_err.get(k).copied().unwrap_or(0);
                let total = ok + err;
                let rate = if total > 0 {
                    (*ok as f64) / (total as f64)
                } else {
                    0.0
                };
                (k.clone(), *ok, err, rate)
            })
            .collect();
        tools.sort_by(|a, b| b.1.cmp(&a.1));
        for (i, (name, ok, err, rate)) in tools.into_iter().take(top).enumerate() {
            let status = if rate >= 0.9 {
                style("[OK]").green()
            } else if rate >= 0.7 {
                style("[W]").cyan()
            } else {
                style("[NO]").red()
            };
            println!(
                "{:>2}. {:<20} {} ok: {:<4} err: {:<4} success: {:>5.1}%",
                i + 1,
                name,
                status,
                ok,
                err,
                rate * 100.0
            );
        }
    }

    if !summary.prompt_cache.is_empty() {
        println!("\n{}", style("Prompt Cache").bold());
        let mut cache_models: Vec<_> = summary.prompt_cache.into_iter().collect();
        cache_models.sort_by_key(|(_, stats)| {
            Reverse(stats.cache_read_tokens + stats.cache_creation_tokens)
        });
        for (i, (model, stats)) in cache_models.into_iter().take(top).enumerate() {
            let total_cache = stats.cache_read_tokens + stats.cache_creation_tokens;
            let hit_ratio = if total_cache > 0 {
                (stats.cache_read_tokens as f64 / total_cache as f64) * 100.0
            } else if stats.prompt_tokens > 0 {
                (stats.cache_read_tokens as f64 / stats.prompt_tokens as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "{:>2}. {:<25} read: {:<8} write: {:<8} hit: {:>5.1}% calls: {}",
                i + 1,
                model,
                format_token_count(stats.cache_read_tokens),
                format_token_count(stats.cache_creation_tokens),
                hit_ratio,
                stats.records
            );
        }
    }

    Ok(())
}

fn format_timestamp(ts: i64) -> String {
    if let Some(dt) = DateTime::<Utc>::from_timestamp(ts, 0) {
        dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    } else {
        ts.to_string()
    }
}

fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{PromptCacheModelStats, summarize_trajectory};
    use std::io::Cursor;

    #[test]
    fn summarize_trajectory_includes_prompt_cache_metrics() {
        let input = r#"
{"kind":"route","turn":1,"selected_model":"gpt-5","class":"default","ts":1}
{"kind":"prompt_cache_metrics","turn":1,"model":"gpt-5","prompt_tokens":1000,"cached_prompt_tokens":400,"ts":2}
{"kind":"prompt_cache_metrics","turn":2,"model":"claude-sonnet","prompt_tokens":1200,"cached_prompt_tokens":300,"cache_read_tokens":350,"cache_creation_tokens":50,"ts":3}
"#;

        let summary = summarize_trajectory(Cursor::new(input)).expect("summary");
        assert_eq!(summary.total_routes, 1);
        assert_eq!(summary.total_prompt_cache_records, 2);
        assert_eq!(
            summary.prompt_cache.get("gpt-5"),
            Some(&PromptCacheModelStats {
                prompt_tokens: 1000,
                cache_read_tokens: 400,
                cache_creation_tokens: 0,
                records: 1,
            })
        );
        assert_eq!(
            summary.prompt_cache.get("claude-sonnet"),
            Some(&PromptCacheModelStats {
                prompt_tokens: 1200,
                cache_read_tokens: 350,
                cache_creation_tokens: 50,
                records: 1,
            })
        );
    }
}
