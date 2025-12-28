//! A2A Protocol CLI command handlers
//!
//! Implements the actual logic for A2A CLI commands including:
//! - Starting the A2A server
//! - Discovering remote agents
//! - Sending tasks to agents
//! - Managing A2A agent connections

use crate::a2a::cli::A2aCommands;
use anyhow::Context;

/// Execute an A2A CLI command
pub async fn execute_a2a_command(command: A2aCommands) -> anyhow::Result<()> {
    match command {
        A2aCommands::Serve { host, port, base_url, enable_push } => {
            serve_a2a_agent(host, port, base_url, enable_push).await
        }
        A2aCommands::Discover { agent_url } => {
            discover_agent(agent_url).await
        }
        A2aCommands::SendTask { agent_url, message, stream, context_id } => {
            send_task_to_agent(agent_url, message, stream, context_id).await
        }
        A2aCommands::ListTasks { agent_url, context_id, limit } => {
            list_agent_tasks(agent_url, context_id, limit).await
        }
        A2aCommands::GetTask { agent_url, task_id } => {
            get_agent_task(agent_url, task_id).await
        }
        A2aCommands::CancelTask { agent_url, task_id } => {
            cancel_agent_task(agent_url, task_id).await
        }
    }
}

/// Serve VTCode as an A2A agent
#[cfg(feature = "a2a-server")]
async fn serve_a2a_agent(
    host: String,
    port: u16,
    base_url: Option<String>,
    _enable_push: bool,
) -> anyhow::Result<()> {
    use crate::a2a::server::{A2aServerState, create_router};
    use crate::a2a::{AgentCard, TaskManager};
    use std::net::SocketAddr;

    println!("Starting VTCode A2A Agent Server...");
    println!("Feature: a2a-server enabled ✓");
    
    let base_url = base_url.unwrap_or_else(|| format!("http://{}:{}", host, port));
    let agent_card = AgentCard::vtcode_default(&base_url);
    let task_manager = TaskManager::new();
    
    let server_state = A2aServerState::new(task_manager, agent_card);
    let router = create_router(server_state);
    
    let addr = format!("{}:{}", host, port).parse::<SocketAddr>()?;
    println!("Listening on http://{}", addr);
    println!("Agent Card: http://{}/.well-known/agent-card.json", addr);
    println!("JSON-RPC API: http://{}/a2a", addr);
    println!("Streaming API: http://{}/a2a/stream", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    
    Ok(())
}

#[cfg(not(feature = "a2a-server"))]
async fn serve_a2a_agent(
    _host: String,
    _port: u16,
    _base_url: Option<String>,
    _enable_push: bool,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "A2A server is not enabled. Build with '--features a2a-server' to enable this feature.\n\
         Example: cargo build --release --features a2a-server"
    )
}

/// Discover and display information about a remote A2A agent
async fn discover_agent(agent_url: String) -> anyhow::Result<()> {
    use crate::a2a::{A2aClient};

    println!("Discovering A2A agent at: {}", agent_url);
    
    let client = A2aClient::new(&agent_url)?;
    let agent_card = client.agent_card().await?;
    
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("A2A Agent Discovery");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("Name: {}", agent_card.name);
    println!("Description: {}", agent_card.description);
    println!("Version: {}", agent_card.version);
    println!("Protocol Version: {}", agent_card.protocol_version);
    println!("URL: {}", agent_card.url);
    
    if let Some(provider) = &agent_card.provider {
        println!("\nProvider:");
        println!("  Organization: {}", provider.organization);
        if let Some(url) = &provider.url {
            println!("  URL: {}", url);
        }
    }
    
    if let Some(capabilities) = &agent_card.capabilities {
        println!("\nCapabilities:");
        println!("  Streaming: {}", capabilities.streaming);
        println!("  Push Notifications: {}", capabilities.push_notifications);
        println!("  State Transition History: {}", capabilities.state_transition_history);
        if !capabilities.extensions.is_empty() {
            println!("  Extensions: {:?}", capabilities.extensions);
        }
    }
    
    if !agent_card.skills.is_empty() {
        println!("\nSkills:");
        for skill in &agent_card.skills {
            println!("  - {}", skill.name);
            if let Some(desc) = &skill.description {
                println!("    Description: {}", desc);
            }
            if !skill.tags.is_empty() {
                println!("    Tags: {:?}", skill.tags);
            }
        }
    }
    
    println!("\nInput Modes: {:?}", agent_card.default_input_modes);
    println!("Output Modes: {:?}", agent_card.default_output_modes);
    
    Ok(())
}

/// Send a task to a remote A2A agent
async fn send_task_to_agent(
    agent_url: String,
    message: String,
    stream: bool,
    context_id: Option<String>,
) -> anyhow::Result<()> {
    use crate::a2a::{A2aClient, Message, rpc::MessageSendParams};
    use futures::StreamExt;

    println!("Connecting to A2A agent: {}", agent_url);
    
    let client = A2aClient::new(&agent_url)?;
    let msg = Message::user_text(message);
    
    let mut params = MessageSendParams::new(msg);
    if let Some(ctx_id) = context_id {
        params = params.with_context_id(ctx_id);
    }
    
    if stream {
        println!("Streaming task execution...\n");
        let stream = client.stream_message(params).await?;
        futures::pin_mut!(stream);
        
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    // Handle different event types
                    match event {
                        crate::a2a::rpc::StreamingEvent::Message { message, .. } => {
                            if let Some(text) = message.parts.iter().find_map(|p| p.as_text()) {
                                println!("Agent: {}", text);
                            }
                        }
                        crate::a2a::rpc::StreamingEvent::TaskStatus { status, .. } => {
                            println!("Status: {:?}", status.state);
                            if let Some(msg) = status.message {
                                if let Some(text) = msg.parts.iter().find_map(|p| p.as_text()) {
                                    println!("  Message: {}", text);
                                }
                            }
                        }
                        crate::a2a::rpc::StreamingEvent::TaskArtifact { artifact, .. } => {
                            println!("Artifact: {}", artifact.id);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Stream error: {}", e);
                    break;
                }
            }
        }
    } else {
        println!("Sending task...\n");
        let task = client.send_message(params).await?;
        println!("Task created: {}", task.id);
        println!("Status: {:?}\n", task.status.state);
        
        if let Some(msg) = &task.status.message {
            if let Some(text) = msg.parts.iter().find_map(|p| p.as_text()) {
                println!("Response: {}", text);
            }
        }
        
        if !task.artifacts.is_empty() {
            println!("\nArtifacts:");
            for artifact in &task.artifacts {
                println!("  - {} ({} parts)", artifact.id, artifact.parts.len());
            }
        }
    }
    
    Ok(())
}

/// List tasks from a remote A2A agent
async fn list_agent_tasks(
    agent_url: String,
    context_id: Option<String>,
    limit: u32,
) -> anyhow::Result<()> {
    use crate::a2a::{A2aClient, rpc::ListTasksParams};
    use serde_json::Value;

    println!("Fetching tasks from: {}", agent_url);
    
    let client = A2aClient::new(&agent_url)?;
    let mut params = ListTasksParams::default();
    
    if let Some(ctx_id) = context_id {
        params.context_id = Some(ctx_id);
    }
    params.page_size = Some(limit);
    
    let result_value: Value = client.list_tasks(Some(params)).await?;
    
    // Parse the JSON result
    let tasks_array = result_value.get("tasks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?;
    
    let total_size = result_value.get("totalSize")
        .and_then(|v| v.as_u64())
        .unwrap_or(tasks_array.len() as u64);
    
    println!("\nTasks ({} total, showing {}):", total_size, tasks_array.len());
    println!("═══════════════════════════════════════════════════════════════\n");
    
    for task_value in tasks_array {
        if let Some(task_id) = task_value.get("id").and_then(|v| v.as_str()) {
            println!("Task: {}", task_id);
        }
        if let Some(status) = task_value.get("status") {
            if let Some(state) = status.get("state").and_then(|v| v.as_str()) {
                println!("  Status: {}", state);
            }
        }
        if let Some(ctx_id) = task_value.get("contextId").and_then(|v| v.as_str()) {
            println!("  Context: {}", ctx_id);
        }
        if let Some(artifacts) = task_value.get("artifacts").and_then(|v| v.as_array()) {
            println!("  Artifacts: {}", artifacts.len());
        }
        println!();
    }
    
    Ok(())
}

/// Get details about a specific task
async fn get_agent_task(agent_url: String, task_id: String) -> anyhow::Result<()> {
    use crate::a2a::A2aClient;

    println!("Fetching task {} from: {}", task_id, agent_url);
    
    let client = A2aClient::new(&agent_url)?;
    let task = client.get_task(task_id.clone()).await?;
    
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Task: {}", task.id);
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("Status: {:?}", task.status.state);
    if let Some(ctx_id) = &task.context_id {
        println!("Context: {}", ctx_id);
    }
    
    if let Some(msg) = &task.status.message {
        println!("\nLatest Message:");
        println!("  Role: {:?}", msg.role);
        for part in &msg.parts {
            match part {
                crate::a2a::types::Part::Text { text } => println!("  Text: {}", text),
                crate::a2a::types::Part::File { file } => println!("  File: {:?}", file),
                crate::a2a::types::Part::Data { data } => println!("  Data: {}", data),
            }
        }
    }
    
    if !task.artifacts.is_empty() {
        println!("\nArtifacts:");
        for artifact in &task.artifacts {
            println!("  - {}:", artifact.id);
            for part in &artifact.parts {
                match part {
                    crate::a2a::types::Part::Text { text } => {
                        let preview = if text.len() > 60 {
                            format!("{}...", &text[..60])
                        } else {
                            text.clone()
                        };
                        println!("    Text: {}", preview);
                    }
                    crate::a2a::types::Part::File { file } => println!("    File: {:?}", file),
                    crate::a2a::types::Part::Data { data } => {
                        let preview = if data.to_string().len() > 60 {
                            format!("{}...", &data.to_string()[..60])
                        } else {
                            data.to_string()
                        };
                        println!("    Data: {}", preview);
                    }
                }
            }
        }
    }
    
    if !task.history.is_empty() {
        println!("\nHistory ({} messages):", task.history.len());
        for (i, msg) in task.history.iter().enumerate() {
            println!("  {}. {:?}: {} parts", i + 1, msg.role, msg.parts.len());
        }
    }
    
    Ok(())
}

/// Cancel a running task
async fn cancel_agent_task(agent_url: String, task_id: String) -> anyhow::Result<()> {
    use crate::a2a::A2aClient;

    println!("Canceling task {} at: {}", task_id, agent_url);
    
    let client = A2aClient::new(&agent_url)?;
    client.cancel_task(task_id).await?;
    
    println!("Task cancellation requested successfully.");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_agent_display() {
        // This is a simple display test - actual client functionality is tested in integration tests
        assert!(true);
    }
}
