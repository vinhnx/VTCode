//! A2A Protocol CLI commands
//!
//! Provides command-line interface for:
//! - Serving VT Code as an A2A agent
//! - Discovering remote A2A agents
//! - Sending tasks to other agents
//! - Managing A2A agent connections

use clap::{Parser, Subcommand};

/// A2A Protocol commands
#[derive(Debug, Subcommand, Clone)]
pub enum A2aCommands {
    /// Serve VT Code as an A2A agent (requires a2a-server feature)
    ///
    /// Starts an HTTP server exposing A2A endpoints:
    /// - /.well-known/agent-card.json - Agent discovery
    /// - /a2a - JSON-RPC endpoint for task management
    /// - /a2a/stream - Server-Sent Events streaming
    ///
    /// Examples:
    ///   vtcode a2a serve --port 8080
    ///   vtcode a2a serve --host 0.0.0.0 --port 8080
    Serve {
        /// Host to bind the server to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,

        /// Base URL for the agent (used in agent card)
        #[arg(long)]
        base_url: Option<String>,

        /// Enable push notifications via webhooks
        #[arg(long)]
        enable_push: bool,
    },

    /// Discover and display information about a remote A2A agent
    ///
    /// Fetches and displays the agent card from the remote agent,
    /// showing capabilities, skills, and supported features.
    ///
    /// Examples:
    ///   vtcode a2a discover https://agent.example.com
    ///   vtcode a2a discover https://localhost:8080
    Discover {
        /// URL of the remote A2A agent
        agent_url: String,
    },

    /// Send a task to a remote A2A agent
    ///
    /// Sends a message to a remote agent and returns the task result.
    /// The agent will process the request and return structured results.
    ///
    /// Examples:
    ///   vtcode a2a send-task https://agent.example.com "Help me refactor this code"
    ///   vtcode a2a send-task https://localhost:8080 "Explain this error message"
    SendTask {
        /// URL of the remote A2A agent
        agent_url: String,

        /// The task/message to send to the agent
        message: String,

        /// Wait for task completion and stream progress
        #[arg(long)]
        stream: bool,

        /// Optional context ID for conversation tracking
        #[arg(long)]
        context_id: Option<String>,
    },

    /// List active tasks in a running A2A agent
    ///
    /// Queries a remote A2A agent for its current and recent tasks.
    ///
    /// Examples:
    ///   vtcode a2a list-tasks https://agent.example.com
    ///   vtcode a2a list-tasks https://localhost:8080 --context-id my-conversation
    ListTasks {
        /// URL of the remote A2A agent
        agent_url: String,

        /// Filter by context ID
        #[arg(long)]
        context_id: Option<String>,

        /// Maximum number of tasks to return
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },

    /// Get details about a specific task
    ///
    /// Retrieves the current status, artifacts, and history of a task.
    ///
    /// Examples:
    ///   vtcode a2a get-task https://agent.example.com task-123
    ///   vtcode a2a get-task https://localhost:8080 task-456
    GetTask {
        /// URL of the remote A2A agent
        agent_url: String,

        /// Task ID to retrieve
        task_id: String,
    },

    /// Cancel a running task
    ///
    /// Requests cancellation of a task that is currently being processed.
    ///
    /// Examples:
    ///   vtcode a2a cancel-task https://agent.example.com task-123
    CancelTask {
        /// URL of the remote A2A agent
        agent_url: String,

        /// Task ID to cancel
        task_id: String,
    },
}

/// A2A CLI configuration options
#[derive(Debug, Parser)]
pub struct A2aServeConfig {
    /// Host to bind the server to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    /// Base URL for the agent (used in agent card)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Enable push notifications via webhooks
    #[arg(long)]
    pub enable_push: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_serve_command() {
        let cmd = A2aCommands::Serve {
            host: "127.0.0.1".to_string(),
            port: 8080,
            base_url: Some("http://localhost:8080".to_string()),
            enable_push: false,
        };
        match cmd {
            A2aCommands::Serve { port, .. } => assert_eq!(port, 8080),
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_cli_discover_command() {
        let cmd = A2aCommands::Discover {
            agent_url: "https://example.com".to_string(),
        };
        match cmd {
            A2aCommands::Discover { agent_url } => {
                assert_eq!(agent_url, "https://example.com")
            }
            _ => panic!("Wrong command type"),
        }
    }
}
