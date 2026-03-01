//! Error types for TUI session operations

/// Errors that can occur in TUI session operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Error during rendering operations
    #[error("Render error: {source}")]
    RenderError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Error during input processing
    #[error("Input error: {source}")]
    InputError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Error during state management
    #[error("State error: {source}")]
    StateError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Error during UI component operations
    #[error("UI error: {source}")]
    UIError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Error during message processing
    #[error("Message error: {source}")]
    MessageError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Error during cache operations
    #[error("Cache error: {source}")]
    CacheError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Resource exhausted error (e.g., memory, file handles)
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },
    /// Configuration error
    #[error("Configuration error: {source}")]
    ConfigError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
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