//! Workflow Optimizer Demo: Shows how detected patterns drive optimization.
//!
//! Pipeline:
//! 1. Execute tools and detect patterns
//! 2. Extract ML features
//! 3. Analyze for optimization opportunities
//! 4. Output recommendations

use serde_json::json;
use vtcode_tools::{
    CachedToolExecutor, WorkflowOptimizer,
    middleware::{LoggingMiddleware, MetricsMiddleware},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Workflow Optimizer Demo ===\n");

    // Step 1: Run tools and collect patterns
    println!("Step 1: Executing realistic workflow...\n");

    let executor = CachedToolExecutor::new()
        .with_middleware(LoggingMiddleware::new("workflow"))
        .with_middleware(MetricsMiddleware::new());

    // Simulate a real development workflow
    let workflows = vec![
        ("list_files", json!({"path": "/src"})),
        ("find_files", json!({"pattern": "*.rs"})),
        ("grep_file", json!({"pattern": "fn main"})),
        ("list_files", json!({"path": "/src"})),    // Repeat
        ("find_files", json!({"pattern": "*.rs"})), // Repeat
        ("grep_file", json!({"pattern": "pub fn"})),
        ("list_files", json!({"path": "/tests"})),
        ("find_files", json!({"pattern": "*.rs"})),
        ("grep_file", json!({"pattern": "test"})),
        ("list_files", json!({"path": "/src"})), // Repeat
    ];

    for (tool, args) in workflows {
        let _ = executor.execute(tool, args).await;
    }

    println!("\n  ✓ 10 tool calls completed\n");

    // Step 2: Collect metrics
    println!("Step 2: Analyzing execution patterns...\n");

    let patterns = executor.patterns().await;
    let features = executor.feature_vector().await;

    println!("  Patterns detected: {}", patterns.len());
    println!("  Feature vector: {:.2?}", features);
    println!("  Cache hits: {}", executor.cache_stats().await.hits);
    println!();

    // Step 3: Generate optimizations
    println!("Step 3: Generating optimization recommendations...\n");

    let optimizer = WorkflowOptimizer::from_detector(patterns.clone(), features.clone());

    let optimizations = optimizer.top_optimizations(5);
    println!(
        "  Found {} optimization opportunities:",
        optimizations.len()
    );
    println!(
        "  Estimated total improvement: {:.1}%\n",
        optimizer.estimated_total_improvement() * 100.0
    );

    // Step 4: Output recommendations
    println!("Step 4: Optimization Recommendations:\n");

    for (i, opt) in optimizations.iter().enumerate() {
        println!("  {}. {:?}", i + 1, opt.optimization_type);
        println!("     Tools: {:?}", opt.tools);
        println!(
            "     Expected improvement: {:.1}%",
            opt.expected_improvement * 100.0
        );
        println!("     Confidence: {:.1}%", opt.confidence * 100.0);
        println!("     Reason: {}", opt.reason);
        println!();
    }

    // Step 5: Show pattern analysis
    println!("Step 5: Detected Workflow Patterns:\n");

    for (i, pattern) in patterns.iter().take(3).enumerate() {
        println!("  Pattern {}:", i + 1);
        println!("    Sequence: {:?}", pattern.sequence);
        println!("    Frequency: {}", pattern.frequency);
        println!("    Success rate: {:.1}%", pattern.success_rate * 100.0);
        println!("    Confidence: {:.1}%", pattern.confidence * 100.0);
        println!("    Avg duration: {}ms", pattern.avg_duration_ms);
        println!();
    }

    // Step 6: Full report
    println!("Step 6: Execution Summary:\n");

    executor.report().await;

    // Step 7: Export optimization data
    println!("\nStep 7: Optimization Data (JSON):\n");

    let opt_json = optimizer.to_json();
    println!("{}", serde_json::to_string_pretty(&opt_json)?);

    // Step 8: Recommendations summary
    println!("\n\nStep 8: What To Do Next:\n");

    if optimizations.is_empty() {
        println!("  ✓ No optimizations found - workflow is already optimal!");
    } else {
        println!("  Based on detected patterns, we recommend:");
        for (i, opt) in optimizations.iter().enumerate() {
            if i >= 3 {
                break;
            }
            match opt.optimization_type {
                vtcode_tools::OptimizationType::Parallelize => {
                    println!("  {}. Parallelize: Run {:?} concurrently", i + 1, opt.tools);
                    println!(
                        "     → Saves ~{:.0}% execution time",
                        opt.expected_improvement * 100.0
                    );
                }
                vtcode_tools::OptimizationType::CacheResult => {
                    println!("  {}. Cache: Store results of {:?}", i + 1, opt.tools);
                    println!(
                        "     → Saves ~{:.0}% latency on repeated calls",
                        opt.expected_improvement * 100.0
                    );
                }
                vtcode_tools::OptimizationType::SkipRedundant => {
                    println!(
                        "  {}. Skip: Remove redundant calls to {:?}",
                        i + 1,
                        opt.tools
                    );
                    println!(
                        "     → Saves ~{:.0}% overhead",
                        opt.expected_improvement * 100.0
                    );
                }
                vtcode_tools::OptimizationType::Reorder => {
                    println!("  {}. Reorder: Rearrange tool execution sequence", i + 1);
                    println!(
                        "     → Saves ~{:.0}% total time",
                        opt.expected_improvement * 100.0
                    );
                }
                vtcode_tools::OptimizationType::Batch => {
                    println!("  {}. Batch: Group similar operations", i + 1);
                    println!(
                        "     → Saves ~{:.0}% execution time",
                        opt.expected_improvement * 100.0
                    );
                }
            }
        }

        println!(
            "\n  Total potential improvement: {:.1}%",
            optimizer.estimated_total_improvement() * 100.0
        );
    }

    println!("\n=== Demo Complete ===\n");
    println!("This shows the complete optimization pipeline:");
    println!("  1. Execute tools with cache + middleware");
    println!("  2. Detect workflow patterns");
    println!("  3. Extract ML features");
    println!("  4. Analyze for optimizations");
    println!("  5. Generate actionable recommendations");
    println!("  6. Measure potential improvement");

    Ok(())
}
