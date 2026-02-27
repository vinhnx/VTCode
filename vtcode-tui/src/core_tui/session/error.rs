//! Error types for TUI session operations

use std::fmt;

/// Errors that can occur in TUI session operations
#[derive(Debug)]
pub enum SessionError {
    /// Error during rendering operations
    RenderError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Error during input processing
    InputError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Error during state management
    StateError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Error during UI component operations
    UIError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Error during message processing
    MessageError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Error during cache operations
    CacheError { source: Box<dyn std::error::Error + Send + Sync> },
    /// Resource exhausted error (e.g., memory, file handles)
    ResourceExhausted { resource: String },
    /// Configuration error
    ConfigError { source: Box<dyn std::error::Error + Send + Sync> },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::RenderError { source } => write!(f, "Render error: {}", source),
            SessionError::InputError { source } => write!(f, "Input error: {}", source),
            SessionError::StateError { source } => write!(f, "State error: {}", source),
            SessionError::UIError { source } => write!(f, "UI error: {}", source),
            SessionError::MessageError { source } => write!(f, "Message error: {}", source),
            SessionError::CacheError { source } => write!(f, "Cache error: {}", source),
            SessionError::ResourceExhausted { resource } => write!(f, "Resource exhausted: {}", resource),
            SessionError::ConfigError { source } => write!(f, "Configuration error: {}", source),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::RenderError { source } => Some(source.as_ref()),
            SessionError::InputError { source } => Some(source.as_ref()),
            SessionError::StateError { source } => Some(source.as_ref()),
            SessionError::UIError { source } => Some(source.as_ref()),
            SessionError::MessageError { source } => Some(source.as_ref()),
            SessionError::CacheError { source } => Some(source.as_ref()),
            SessionError::ResourceExhausted { .. } => None,
            SessionError::ConfigError { source } => Some(source.as_ref()),
        }
    }
}

impl SessionError {
    pub fn render<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::RenderError { source: Box::new(error) }
    }

    pub fn input<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::InputError { source: Box::new(error) }
    }

    pub fn state<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::StateError { source: Box::new(error) }
    }

    pub fn ui<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::UIError { source: Box::new(error) }
    }

    pub fn message<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::MessageError { source: Box::new(error) }
    }

    pub fn cache<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::CacheError { source: Box::new(error) }
    }

    pub fn config<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        SessionError::ConfigError { source: Box::new(error) }
    }

    pub fn resource_exhausted(resource: impl Into<String>) -> Self {
        SessionError::ResourceExhausted { resource: resource.into() }
    }
}

// Type alias for common result type
pub type SessionResult<T> = Result<T, SessionError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_construction() {
        let err = SessionError::render("Test render error");
        assert!(format!("{}", err).contains("Render error"));
    }

    #[test]
    fn test_error_with_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "IO error");
        let session_err = SessionError::input(io_err);
        assert!(format!("{}", session_err).contains("Input error"));
    }
    
    #[test]
    fn test_error_variants() {
        let render_err = SessionError::render("Render failed");
        let input_err = SessionError::input("Input failed");
        let state_err = SessionError::state("State failed");
        let ui_err = SessionError::ui("UI failed");
        let message_err = SessionError::message("Message failed");
        let cache_err = SessionError::cache("Cache failed");
        let resource_err = SessionError::resource_exhausted("memory");
        let config_err = SessionError::config("Config failed");
        
        assert!(format!("{}", render_err).contains("Render error"));
        assert!(format!("{}", input_err).contains("Input error"));
        assert!(format!("{}", state_err).contains("State error"));
        assert!(format!("{}", ui_err).contains("UI error"));
        assert!(format!("{}", message_err).contains("Message error"));
        assert!(format!("{}", cache_err).contains("Cache error"));
        assert!(format!("{}", resource_err).contains("Resource exhausted"));
        assert!(format!("{}", config_err).contains("Configuration error"));
    }
    
    #[test]
    fn test_error_sources() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "IO error");
        let session_err = SessionError::input(io_err);
        
        // Test that the source is properly chained
        if let Some(source) = session_err.source() {
            assert_eq!(format!("{}", source), "IO error");
        } else {
            panic!("Expected source error");
        }
    }
    
    #[test]
    fn test_result_type() {
        let result: SessionResult<()> = Err(SessionError::render("Test"));
        assert!(result.is_err());
        
        let result: SessionResult<()> = Ok(());
        assert!(result.is_ok());
    }
}