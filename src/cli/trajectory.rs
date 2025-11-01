use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
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

    let mut class_counts: HashMap<String, usize> = HashMap::new();
    let mut model_counts: HashMap<String, usize> = HashMap::new();
    let mut tool_ok: HashMap<String, usize> = HashMap::new();
    let mut tool_err: HashMap<String, usize> = HashMap::new();
    let mut total_routes = 0;
    let mut total_tools = 0;
    let mut recent_timestamps: Vec<i64> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<Rec>(&line) {
            match rec {
                Rec::Route {
                    selected_model,
                    class,
                    ts,
                    ..
                } => {
                    *class_counts.entry(class).or_insert(0) += 1;
                    *model_counts.entry(selected_model).or_insert(0) += 1;
                    total_routes += 1;
                    recent_timestamps.push(ts);
                }
                Rec::Tool { name, ok, ts, .. } => {
                    if ok {
                        *tool_ok.entry(name).or_insert(0) += 1;
                    } else {
                        *tool_err.entry(name).or_insert(0) += 1;
                    }
                    total_tools += 1;
                    recent_timestamps.push(ts);
                }
            }
        }
    }

    println!(
        "{} {}",
        style("Trajectory Report").magenta().bold(),
        style(log_path.display()).dim()
    );
    println!(
        "{} routes, {} tools",
        style(total_routes).cyan(),
        style(total_tools).cyan()
    );

    // Show time range if we have timestamps
    if !recent_timestamps.is_empty() {
        recent_timestamps.sort();
        let oldest = recent_timestamps.first().unwrap();
        let newest = recent_timestamps.last().unwrap();
        let oldest_time = format_timestamp(*oldest);
        let newest_time = format_timestamp(*newest);
        println!(
            "Time range: {} to {}",
            style(oldest_time).dim(),
            style(newest_time).dim()
        );
    }

    // Classes
    if !class_counts.is_empty() {
        println!("\n{}", style("Classes").bold());
        let mut classes: Vec<_> = class_counts.into_iter().collect();
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
    if !model_counts.is_empty() {
        println!("\n{}", style("Models").bold());
        let mut models: Vec<_> = model_counts.into_iter().collect();
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
    if !tool_ok.is_empty() || !tool_err.is_empty() {
        println!("\n{}", style("Tools").bold());
        let mut tools: Vec<_> = tool_ok
            .iter()
            .map(|(k, ok)| {
                let err = tool_err.get(k).copied().unwrap_or(0);
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
                style("✓").green()
            } else if rate >= 0.7 {
                style("⚠").yellow()
            } else {
                style("✗").red()
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

    Ok(())
}

fn format_timestamp(ts: i64) -> String {
    if let Some(dt) = DateTime::<Utc>::from_timestamp(ts, 0) {
        dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    } else {
        ts.to_string()
    }
}
