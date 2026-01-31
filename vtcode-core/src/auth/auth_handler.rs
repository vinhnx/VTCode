//! Authentication handler for different ACP auth methods.
//!
//! This module provides a unified interface for handling authentication
//! across different auth methods specified in the ACP protocol.

use anyhow::Result;
use std::collections::HashMap;
use vtcode_acp_client::AuthMethod;

/// Handles authentication based on the auth method type
#[derive(Debug, Clone)]
pub struct AuthHandler {
    /// Environment variables to set for the agent process
    pub env_vars: HashMap<String, String>,
    
    /// Arguments to pass to the agent process
    pub args: Vec<String>,
}

impl AuthHandler {
    /// Create a new auth handler for the given auth method
    pub fn new(method: &AuthMethod) -> Result<Self> {
        match method {
            AuthMethod::Agent { .. } => {
                // Agent handles auth itself - no special configuration needed
                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }

            AuthMethod::EnvVar {
                var_name,
                ..
            } => {
                // For env var auth, the client is responsible for setting the variable
                // We just validate the variable name here
                if var_name.is_empty() {
                    anyhow::bail!("Environment variable name cannot be empty");
                }
                if !var_name.chars().all(|c: char| c.is_alphanumeric() || c == '_') {
                    anyhow::bail!(
                        "Invalid environment variable name: '{}'. Must contain only alphanumeric characters and underscores.",
                        var_name
                    );
                }

                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }

            AuthMethod::Terminal { args, env, .. } => {
                // Terminal auth: pass args and env to the agent process
                Ok(Self {
                    env_vars: env.clone(),
                    args: args.clone(),
                })
            }

            // Legacy methods
            AuthMethod::ApiKey => {
                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }

            AuthMethod::OAuth2 => {
                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }

            AuthMethod::Bearer => {
                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }

            AuthMethod::Custom(_) => {
                Ok(Self {
                    env_vars: HashMap::new(),
                    args: Vec::new(),
                })
            }
        }
    }

    /// Set an environment variable for this auth handler
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Add an argument for this auth handler
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Merge another auth handler's configuration into this one
    pub fn merge(&mut self, other: &AuthHandler) {
        for (key, value) in &other.env_vars {
            self.env_vars.insert(key.clone(), value.clone());
        }
        self.args.extend(other.args.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_handler_agent() {
        let method = AuthMethod::Agent {
            id: "agent".to_string(),
            name: "Agent".to_string(),
            description: None,
        };
        let handler = AuthHandler::new(&method).unwrap();
        assert!(handler.env_vars.is_empty());
        assert!(handler.args.is_empty());
    }

    #[test]
    fn test_auth_handler_env_var() {
        let method = AuthMethod::EnvVar {
            id: "openai".to_string(),
            name: "OpenAI Key".to_string(),
            description: None,
            var_name: "OPENAI_API_KEY".to_string(),
            link: None,
        };
        let handler = AuthHandler::new(&method).unwrap();
        assert!(handler.env_vars.is_empty());
        assert!(handler.args.is_empty());
    }

    #[test]
    fn test_auth_handler_env_var_invalid_name() {
        let method = AuthMethod::EnvVar {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            var_name: "INVALID-VAR-NAME".to_string(), // Hyphens not allowed
            link: None,
        };
        assert!(AuthHandler::new(&method).is_err());
    }

    #[test]
    fn test_auth_handler_terminal() {
        let mut env = HashMap::new();
        env.insert("VAR1".to_string(), "value1".to_string());

        let method = AuthMethod::Terminal {
            id: "terminal".to_string(),
            name: "Terminal".to_string(),
            description: None,
            args: vec!["--login".to_string()],
            env,
        };
        let handler = AuthHandler::new(&method).unwrap();
        assert_eq!(handler.args.len(), 1);
        assert_eq!(handler.args[0], "--login");
        assert_eq!(handler.env_vars.get("VAR1").unwrap(), "value1");
    }

    #[test]
    fn test_auth_handler_with_env() {
        let method = AuthMethod::Agent {
            id: "agent".to_string(),
            name: "Agent".to_string(),
            description: None,
        };
        let handler = AuthHandler::new(&method)
            .unwrap()
            .with_env("MY_VAR", "my_value");
        assert_eq!(handler.env_vars.get("MY_VAR").unwrap(), "my_value");
    }

    #[test]
    fn test_auth_handler_with_arg() {
        let method = AuthMethod::Agent {
            id: "agent".to_string(),
            name: "Agent".to_string(),
            description: None,
        };
        let handler = AuthHandler::new(&method)
            .unwrap()
            .with_arg("--flag");
        assert_eq!(handler.args.len(), 1);
        assert_eq!(handler.args[0], "--flag");
    }

    #[test]
    fn test_auth_handler_merge() {
        let method1 = AuthMethod::Agent {
            id: "agent".to_string(),
            name: "Agent".to_string(),
            description: None,
        };
        let method2 = AuthMethod::Terminal {
            id: "terminal".to_string(),
            name: "Terminal".to_string(),
            description: None,
            args: vec!["--login".to_string()],
            env: {
                let mut m = HashMap::new();
                m.insert("VAR".to_string(), "val".to_string());
                m
            },
        };

        let mut handler1 = AuthHandler::new(&method1).unwrap();
        let handler2 = AuthHandler::new(&method2).unwrap();

        handler1.merge(&handler2);
        assert_eq!(handler1.args.len(), 1);
        assert_eq!(handler1.env_vars.get("VAR").unwrap(), "val");
    }
}
