//! Stats command implementation - show session statistics and performance metrics

use crate::config::types::CapabilityLevel;
use crate::config::types::{AgentConfig, OutputFormat, PerformanceMetrics};
use crate::core::agent::core::Agent;
use crate::tools::handlers::SessionSurface;
use crate::utils::colors::style;
use anyhow::Result;

/// Handle the stats command - display session statistics and performance metrics
pub async fn handle_stats_command(
    agent: &Agent,
    detailed: bool,
    format: String,
) -> Result<PerformanceMetrics> {
    let output_format = match format.to_lowercase().as_str() {
        "text" => OutputFormat::Text,
        "json" => OutputFormat::Json,
        "html" => OutputFormat::Html,
        _ => OutputFormat::Text,
    };

    println!("{}", style("Session Statistics").cyan().bold());

    let metrics = agent.performance_metrics();
    let tool_names = agent
        .tool_registry()
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;

    match output_format {
        OutputFormat::Text => display_text_stats(agent.config(), &metrics, detailed, &tool_names),
        OutputFormat::Json => display_json_stats(agent.config(), &metrics, &tool_names),
        OutputFormat::Html => display_html_stats(agent.config(), &metrics, &tool_names),
    }

    Ok(metrics)
}

fn display_text_stats(
    config: &AgentConfig,
    metrics: &PerformanceMetrics,
    detailed: bool,
    tool_names: &[String],
) {
    println!("{} Configuration:", style("[CONFIG]").dim());
    println!("  Model: {}", style(&config.model).cyan());
    println!("  Workspace: {}", style(config.workspace.display()).cyan());
    println!(
        "  Verbose Mode: {}",
        if config.verbose {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    println!("\n{} Tool Information:", style("").dim());
    let tool_count = tool_names.len();
    println!("  Available Tools: {}", style(tool_count).cyan());

    if detailed {
        println!("  Tools:");
        for tool_name in tool_names {
            println!("    • {}", style(tool_name).cyan());
        }
    }

    println!("\n{} Performance Metrics:", style("[METRICS]").dim());
    println!(
        "  Session Duration: {} seconds",
        style(metrics.session_duration_seconds).cyan()
    );
    println!("  API Calls: {}", style(metrics.total_api_calls).cyan());
    println!(
        "  Tool Executions: {}",
        style(metrics.tool_execution_count).cyan()
    );
    println!("  Errors: {}", style(metrics.error_count).red());
    println!(
        "  Recovery Rate: {:.1}%",
        style(metrics.recovery_success_rate * 100.0).green()
    );

    if let Some(tokens) = metrics.total_tokens_used {
        println!("  Total Tokens: {}", style(tokens).cyan());
    }

    println!(
        "  Avg Response Time: {:.0}ms",
        style(metrics.average_response_time_ms).cyan()
    );

    if detailed {
        println!("\n{} System Information:", style("[SYS]").dim());
        println!(
            "  Rust Version: {}",
            style(env!("CARGO_PKG_RUST_VERSION")).cyan()
        );
        println!(
            "  vtcode Version: {}",
            style(env!("CARGO_PKG_VERSION")).cyan()
        );
        println!(
            "  Build Profile: {}",
            if cfg!(debug_assertions) {
                "Debug"
            } else {
                "Release"
            }
        );
    }
}

fn display_json_stats(config: &AgentConfig, metrics: &PerformanceMetrics, tool_names: &[String]) {
    let stats = serde_json::json!({
        "configuration": {
            "model": config.model,
            "workspace": config.workspace,
            "verbose": config.verbose
        },
        "tools": {
            "count": tool_names.len(),
            "available": tool_names
        },
        "performance": {
            "session_duration_seconds": metrics.session_duration_seconds,
            "total_api_calls": metrics.total_api_calls,
            "total_tokens_used": metrics.total_tokens_used,
            "average_response_time_ms": metrics.average_response_time_ms,
            "tool_execution_count": metrics.tool_execution_count,
            "error_count": metrics.error_count,
            "recovery_success_rate": metrics.recovery_success_rate
        },
        "system": {
            "rust_version": env!("CARGO_PKG_RUST_VERSION"),
            "vtcode_version": env!("CARGO_PKG_VERSION"),
            "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" }
        }
    });

    let rendered = serde_json::to_string_pretty(&stats)
        .unwrap_or_else(|error| format!(r#"{{"error":"failed to format stats: {}"}}"#, error));
    println!("{}", rendered);
}

fn display_html_stats(config: &AgentConfig, metrics: &PerformanceMetrics, tool_names: &[String]) {
    println!("<!DOCTYPE html>");
    println!("<html><head><title>vtcode Statistics</title></head><body>");
    println!("<h1>vtcode Session Statistics</h1>");

    println!("<h2>Configuration</h2>");
    println!("<ul>");
    println!("<li><strong>Model:</strong> {}</li>", config.model);
    println!(
        "<li><strong>Workspace:</strong> {}</li>",
        config.workspace.display()
    );
    println!(
        "<li><strong>Verbose Mode:</strong> {}</li>",
        if config.verbose {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    println!("</ul>");

    println!("<h2>Tool Information</h2>");
    println!(
        "<p><strong>Available Tools:</strong> {}</p>",
        tool_names.len()
    );
    println!("<ul>");
    for tool_name in tool_names {
        println!("<li>{}</li>", tool_name);
    }
    println!("</ul>");

    println!("<h2>Performance Metrics</h2>");
    println!("<ul>");
    println!(
        "<li><strong>Session Duration:</strong> {} seconds</li>",
        metrics.session_duration_seconds
    );
    println!(
        "<li><strong>API Calls:</strong> {}</li>",
        metrics.total_api_calls
    );
    println!(
        "<li><strong>Tool Executions:</strong> {}</li>",
        metrics.tool_execution_count
    );
    println!("<li><strong>Errors:</strong> {}</li>", metrics.error_count);
    println!(
        "<li><strong>Recovery Rate:</strong> {:.1}%</li>",
        metrics.recovery_success_rate * 100.0
    );
    if let Some(tokens) = metrics.total_tokens_used {
        println!("<li><strong>Total Tokens:</strong> {}</li>", tokens);
    }
    println!(
        "<li><strong>Avg Response Time:</strong> {:.0}ms</li>",
        metrics.average_response_time_ms
    );
    println!("</ul>");

    println!("</body></html>");
}
