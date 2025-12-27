use crate::tools::lsp::client::LspClient;
use crate::tools::traits::Tool;
use anyhow::{Context, Result};
use lsp_types::{
    GotoDefinitionParams, Hover, HoverParams, Location, LocationLink, PartialResultParams,
    Position, ReferenceContext, ReferenceParams, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use lsp_types::Uri;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod client;
pub mod manager;

/// LSP Tool Input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspInput {
    pub operation: LspOperation,
    pub server_command: Option<String>,
    pub server_args: Option<Vec<String>>,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub character: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LspOperation {
    Start,
    Stop,
    GotoDefinition,
    Hover,
    FindReferences,
}

pub struct LspTool {
    workspace_root: PathBuf,
    // Map server command name -> Client
    clients: Arc<Mutex<HashMap<String, Arc<LspClient>>>>,
    default_client: Arc<Mutex<Option<String>>>,
}

impl LspTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            clients: Arc::new(Mutex::new(HashMap::new())),
            default_client: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_or_start_client(
        &self,
        command: Option<String>,
        args: Option<Vec<String>>,
    ) -> Result<Arc<LspClient>> {
        let mut clients = self.clients.lock().await;
        let mut default = self.default_client.lock().await;

        let cmd = command.or(default.clone()).ok_or_else(|| {
            anyhow::anyhow!("No server command specified and no default server running")
        })?;

        if let Some(client) = clients.get(&cmd) {
            return Ok(client.clone());
        }

        // Needs starting
        let args = args.unwrap_or_default();
        let client = LspClient::new(&cmd, &args, self.workspace_root.clone()).await?;
        client
            .initialize()
            .await
            .context("Failed to initialize LSP server")?;

        clients.insert(cmd.clone(), client.clone());
        if default.is_none() {
            *default = Some(cmd);
        }

        Ok(client)
    }

    async fn resolve_path(&self, file_path: &str) -> Result<Uri> {
        let path = Path::new(file_path);
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };

        let canonical = full_path.canonicalize().unwrap_or(full_path);
        let url = url::Url::from_file_path(canonical).map_err(|_| anyhow::anyhow!("Invalid file path"))?;
        Ok(url.to_string().parse().map_err(|_| anyhow::anyhow!("Failed to parse Uri"))?)
    }
}

#[async_trait::async_trait]
impl Tool for LspTool {
    fn name(&self) -> &'static str {
        "lsp"
    }

    fn description(&self) -> &'static str {
        "Interacts with Language Server Protocol (LSP) servers for code intelligence."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["start", "stop", "goto_definition", "hover", "find_references"],
                    "description": "LSP operation to perform"
                },
                "server_command": {
                    "type": "string",
                    "description": "Command to start the server (e.g., 'rust-analyzer', 'gopls'). Required for 'start', optional if already running."
                },
                "server_args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments for the server command"
                },
                "file_path": {
                    "type": "string",
                    "description": "Target file path"
                },
                "line": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "0-based line number"
                },
                "character": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "0-based character/column number"
                }
            },
            "required": ["operation"]
        }))
    }

    async fn execute(&self, args: Value) -> Result<Value> {
        let input: LspInput = serde_json::from_value(args)?;

        match input.operation {
            LspOperation::Start => {
                let _ = self
                    .get_or_start_client(input.server_command, input.server_args)
                    .await?;
                Ok(json!({ "status": "started" }))
            }
            LspOperation::Stop => {
                // Simplification: stop all or specific? for now just clear clients
                let mut clients = self.clients.lock().await;
                // Ideally call shutdown on each
                clients.clear();
                *self.default_client.lock().await = None;
                Ok(json!({ "status": "stopped" }))
            }
            LspOperation::GotoDefinition => {
                let client = self.get_or_start_client(input.server_command, None).await?;
                let uri = self
                    .resolve_path(input.file_path.as_deref().unwrap_or(""))
                    .await?;
                let params = GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: Position {
                            line: input.line.unwrap_or(0),
                            character: input.character.unwrap_or(0),
                        },
                    },
                    work_done_progress_params: WorkDoneProgressParams {
                        work_done_token: None,
                    },
                    partial_result_params: PartialResultParams {
                        partial_result_token: None,
                    },
                };

                let result = client
                    .send_request("textDocument/definition", serde_json::to_value(params)?)
                    .await?;
                // Parse result to simplify output? The raw result is usually Location or Location[] or LocationLink[]
                Ok(json!({ "result": result }))
            }
            LspOperation::Hover => {
                let client = self.get_or_start_client(input.server_command, None).await?;
                let uri = self
                    .resolve_path(input.file_path.as_deref().unwrap_or(""))
                    .await?;
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: Position {
                            line: input.line.unwrap_or(0),
                            character: input.character.unwrap_or(0),
                        },
                    },
                    work_done_progress_params: WorkDoneProgressParams {
                        work_done_token: None,
                    },
                };
                let result = client
                    .send_request("textDocument/hover", serde_json::to_value(params)?)
                    .await?;
                Ok(json!({ "result": result }))
            }
            LspOperation::FindReferences => {
                let client = self.get_or_start_client(input.server_command, None).await?;
                let uri = self
                    .resolve_path(input.file_path.as_deref().unwrap_or(""))
                    .await?;
                let params = ReferenceParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: Position {
                            line: input.line.unwrap_or(0),
                            character: input.character.unwrap_or(0),
                        },
                    },
                    work_done_progress_params: WorkDoneProgressParams {
                        work_done_token: None,
                    },
                    partial_result_params: PartialResultParams {
                        partial_result_token: None,
                    },
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                };
                let result = client
                    .send_request("textDocument/references", serde_json::to_value(params)?)
                    .await?;
                Ok(json!({ "result": result }))
            }
        }
    }
}
