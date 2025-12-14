//! Example: Distributed workflow using ACP inter-agent communication
//!
//! This example demonstrates:
//! 1. Discovering available agents
//! 2. Finding agents by capability
//! 3. Executing tasks on remote agents
//! 4. Aggregating results from multiple agents
//!
//! To run this example:
//! ```
//! cargo run --example acp_distributed_workflow
//! ```

use serde_json::json;
use std::collections::HashMap;
use vtcode_acp_client::{AcpClient, AgentInfo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== ACP Distributed Workflow Example ===\n");

    // 1. Initialize ACP client for the main agent
    let client = AcpClient::new("main-orchestrator".to_string())?;
    let registry = client.registry();

    // 2. Register some example remote agents
    println!("Registering remote agents...");

    let data_processor = AgentInfo {
        id: "data-processor".to_string(),
        name: "Data Processor".to_string(),
        base_url: "http://localhost:8081".to_string(),
        description: Some("Processes and transforms data".to_string()),
        capabilities: vec![
            "bash".to_string(),
            "python".to_string(),
            "data_transformation".to_string(),
        ],
        metadata: {
            let mut m = HashMap::new();
            m.insert("version".to_string(), json!("1.0.0"));
            m.insert("timeout".to_string(), json!(300));
            m
        },
        online: true,
        last_seen: None,
    };

    let model_trainer = AgentInfo {
        id: "model-trainer".to_string(),
        name: "Model Trainer".to_string(),
        base_url: "http://localhost:8082".to_string(),
        description: Some("Trains ML models".to_string()),
        capabilities: vec![
            "tensorflow".to_string(),
            "pytorch".to_string(),
            "model_training".to_string(),
        ],
        metadata: {
            let mut m = HashMap::new();
            m.insert("version".to_string(), json!("2.1.0"));
            m.insert("gpu_enabled".to_string(), json!(true));
            m
        },
        online: true,
        last_seen: None,
    };

    let report_generator = AgentInfo {
        id: "report-gen".to_string(),
        name: "Report Generator".to_string(),
        base_url: "http://localhost:8083".to_string(),
        description: Some("Generates reports and visualizations".to_string()),
        capabilities: vec!["reporting".to_string(), "visualization".to_string()],
        metadata: HashMap::new(),
        online: true,
        last_seen: None,
    };

    registry.register(data_processor).await?;
    registry.register(model_trainer).await?;
    registry.register(report_generator).await?;

    println!("Registered {} agents\n", registry.count().await);

    // 3. Discover agents
    println!("=== Discovery ===");
    println!("All registered agents:");
    for agent in registry.list_all().await? {
        println!(
            "  - {}: {} ({})",
            agent.id,
            agent.name,
            agent.capabilities.join(", ")
        );
    }
    println!();

    // 4. Find agents by capability
    println!("=== Finding Agents by Capability ===");

    let python_agents = registry.find_by_capability("python").await?;
    println!(
        "Agents with Python capability: {:?}",
        python_agents.iter().map(|a| &a.id).collect::<Vec<_>>()
    );

    let ml_agents = registry.find_by_capability("model_training").await?;
    println!(
        "Agents with Model Training capability: {:?}",
        ml_agents.iter().map(|a| &a.id).collect::<Vec<_>>()
    );
    println!();

    // 5. Demonstrate message construction (without actual HTTP calls)
    println!("=== Workflow Scenario: Data Processing Pipeline ===\n");

    // Scenario: Process data -> Train model -> Generate report
    println!("Step 1: Prepare data processing request");
    let data_request = json!({
        "input_file": "raw_data.csv",
        "operations": ["clean", "normalize", "split"],
        "output_format": "json"
    });
    println!(
        "  Request: {}\n",
        serde_json::to_string_pretty(&data_request)?
    );

    println!("Step 2: Prepare model training request");
    let training_request = json!({
        "data_path": "processed_data.json",
        "model_type": "neural_network",
        "epochs": 100,
        "batch_size": 32
    });
    println!(
        "  Request: {}\n",
        serde_json::to_string_pretty(&training_request)?
    );

    println!("Step 3: Prepare report generation request");
    let report_request = json!({
        "model_path": "trained_model.pkl",
        "metrics": ["accuracy", "precision", "recall"],
        "format": "html"
    });
    println!(
        "  Request: {}\n",
        serde_json::to_string_pretty(&report_request)?
    );

    // 6. Aggregate agent metadata
    println!("=== Agent Metadata Summary ===");
    let all_agents = registry.list_all().await?;
    let mut metadata_summary = HashMap::new();

    for agent in &all_agents {
        metadata_summary.insert(
            agent.id.clone(),
            json!({
                "name": agent.name,
                "capabilities": agent.capabilities,
                "metadata": agent.metadata,
                "online": agent.online,
            }),
        );
    }

    println!("{}\n", serde_json::to_string_pretty(&metadata_summary)?);

    // 7. Example error handling
    println!("=== Error Handling Example ===");

    match registry.find("non-existent-agent").await {
        Ok(_) => println!("Agent found"),
        Err(e) => println!("Expected error when looking up non-existent agent: {}\n", e),
    }

    // 8. Update agent status
    println!("=== Agent Status Management ===");
    registry.update_status("model-trainer", false).await?;
    println!("Marked 'model-trainer' as offline");

    let online_count = registry.list_online().await?.len();
    println!("Online agents: {}\n", online_count);

    registry.update_status("model-trainer", true).await?;
    println!("Marked 'model-trainer' as online again");

    println!("\n=== Workflow Example Complete ===");
    println!(
        "In production, actual HTTP requests would be sent to remote agents.\n\
         This example demonstrated the ACP client's discovery and orchestration \
         capabilities."
    );

    Ok(())
}
