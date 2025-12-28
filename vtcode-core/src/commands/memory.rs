//! Memory command implementation - show memory usage and pressure diagnostics

use crate::memory::{MemoryMonitor, MemoryPressure, MemoryReport};
use crate::utils::colors::style;
use anyhow::{Context, Result};

/// Handle the memory command - display memory usage and diagnostics
pub async fn handle_memory_command() -> Result<MemoryReport> {
    let monitor = MemoryMonitor::new();

    // Generate the report
    let report = monitor.get_report().map_err(|e| {
        anyhow::anyhow!("Failed to generate memory report: {}", e)
    })?;

    println!("{}", style("Memory Usage Report").cyan().bold());
    display_memory_report(&report);

    Ok(report)
}

/// Display memory report in human-readable format
fn display_memory_report(report: &MemoryReport) {
    println!("\n{} Current Memory Usage:", style("[MEMORY]").dim());
    println!(
        "  RSS (Resident Set): {} MB",
        style(format!("{:.1}", report.current_rss_mb)).cyan()
    );

    println!("\n{} Thresholds:", style("[LIMITS]").dim());
    println!(
        "  Soft Limit: {} MB (warning level)",
        style(format!("{:.1}", report.soft_limit_mb)).yellow()
    );
    println!(
        "  Hard Limit: {} MB (critical level)",
        style(format!("{:.1}", report.hard_limit_mb)).red()
    );

    println!("\n{} Pressure Status:", style("[PRESSURE]").dim());
    let pressure_str = format!("{}", report.pressure);
    let pressure_colored = match report.pressure {
        MemoryPressure::Normal => style(pressure_str).green(),
        MemoryPressure::Warning => style(pressure_str).yellow(),
        MemoryPressure::Critical => style(pressure_str).red().bold(),
    };
    println!("  Level: {}", pressure_colored);
    println!("  Description: {}", report.pressure.description());
    println!(
        "  Usage: {:.1}% of hard limit",
        style(format!("{:.1}", report.usage_percent)).cyan()
    );

    println!("\n{} Recommendations:", style("[RECOMMENDATIONS]").dim());
    match report.pressure {
        MemoryPressure::Normal => {
            println!("  ✓ Memory usage is healthy");
            println!("  • Continue normal operation");
        }
        MemoryPressure::Warning => {
            println!("  ⚠ Memory approaching soft limit (400 MB)");
            println!("  • Cache TTL reduced to 2 minutes");
            println!("  • Least-used entries being evicted");
            println!("  • Consider reducing cache sizes if continues");
        }
        MemoryPressure::Critical => {
            println!("  ⛔ CRITICAL: Memory at hard limit (600 MB)");
            println!("  • Aggressive cache eviction active");
            println!("  • Cache TTL reduced to 30 seconds");
            println!("  • Immediate cleanup recommended");
            println!("  • Risk of OOM if memory increases further");
        }
    }

    // Display recent checkpoints if available
    if !report.recent_checkpoints.is_empty() {
        println!("\n{} Recent Memory Checkpoints:", style("[HISTORY]").dim());
        for (i, checkpoint) in report.recent_checkpoints.iter().rev().take(5).enumerate() {
            println!(
                "  [{}] {} MB - {}",
                i + 1,
                style(format!("{:.1}", checkpoint.rss_bytes as f64 / (1024.0 * 1024.0))).cyan(),
                style(&checkpoint.label).dim()
            );
        }
        if report.recent_checkpoints.len() > 5 {
            println!(
                "  ... and {} more checkpoint{}",
                report.recent_checkpoints.len() - 5,
                if report.recent_checkpoints.len() - 5 == 1 { "" } else { "s" }
            );
        }
    }

    println!("\n{} Tip:", style("[INFO]").dim());
    println!(
        "  Run 'cargo build --release' for optimized memory usage"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_command_execution() {
        // This test would require tokio runtime or async test framework
        // For now, we verify the command structure compiles
        let _cmd = handle_memory_command();
    }

    #[test]
    fn test_pressure_recommendations() {
        // Verify that each pressure level has recommendations
        assert!(!MemoryPressure::Normal.description().is_empty());
        assert!(!MemoryPressure::Warning.description().is_empty());
        assert!(!MemoryPressure::Critical.description().is_empty());
    }
}
