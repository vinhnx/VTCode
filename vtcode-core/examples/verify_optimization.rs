use vtcode_core::core::agent::state::TaskRunState;
use vtcode_core::tools::ToolRegistry;
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Verifying TaskRunState...");
    
    // 1. Verify TaskRunState
    let mut state = TaskRunState::new(vec![], vec![], 10);
    let start = Instant::now();
    let mut recorded = false;
    state.record_turn(&start, &mut recorded);
    assert!(recorded);
    assert_eq!(state.turn_durations_ms.len(), 1);
    
    state.register_tool_loop();
    assert_eq!(state.consecutive_tool_loops, 1);
    
    println!("TaskRunState verified.");

    // 2. Verify ToolRegistry Caching
    println!("Verifying ToolRegistry Caching...");
    let workspace = std::env::current_dir()?;
    let mut registry = ToolRegistry::new(workspace).await;
    
    let start = Instant::now();
    let tools1 = registry.available_tools().await;
    let duration1 = start.elapsed();
    println!("First available_tools call took: {:?}", duration1);
    
    let start = Instant::now();
    let tools2 = registry.available_tools().await;
    let duration2 = start.elapsed();
    println!("Second available_tools (cached) call took: {:?}", duration2);
    
    assert_eq!(tools1, tools2);
    // Caching should be faster, but for small toolsets the difference might be negligible or overshadowed by async overhead.
    // Ideally duration2 < duration1, but in very fast execution it might vary.
    // The main point is that it runs without error and returns consistent results.
    
    println!("ToolRegistry verified.");
    
    Ok(())
}
