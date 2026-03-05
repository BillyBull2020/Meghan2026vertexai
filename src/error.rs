//! Unified error types for the Ironclaw engine.
//!
//! Every public function returns `Result<T, IronclawError>` —
//! no unwraps in production paths.

use std::fmt;

/// Central error type for all Ironclaw operations.
#[derive(Debug)]
pub enum IronclawError {
    /// Failed to parse a YAML profile.
    ProfileParse {
        path: String,
        source: serde_yaml::Error,
    },

    /// Failed to read a profile file from disk.
    ProfileIo {
        path: String,
        source: std::io::Error,
    },

    /// WebSocket connection or communication error.
    WebSocket(String),

    /// Google Cloud authentication failure.
    Auth(String),

    /// Session not found in the registry.
    SessionNotFound(String),

    /// Agent profile not found in the registry.
    AgentNotFound(String),

    /// Generic internal error.
    Internal(String),
}

impl fmt::Display for IronclawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfileParse { path, source } => {
                write!(f, "Failed to parse profile '{}': {}", path, source)
            }
            Self::ProfileIo { path, source } => {
                write!(f, "Failed to read profile '{}': {}", path, source)
            }
            Self::WebSocket(msg) => write!(f, "WebSocket error: {}", msg),
            Self::Auth(msg) => write!(f, "Auth error: {}", msg),
            Self::SessionNotFound(id) => write!(f, "Session not found: {}", id),
            Self::AgentNotFound(id) => write!(f, "Agent profile not found: {}", id),
            Self::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for IronclawError {}

impl From<tokio_tungstenite::tungstenite::Error> for IronclawError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(e.to_string())
    }
}

impl From<serde_json::Error> for IronclawError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("JSON serialization error: {}", e))
    }
}

impl From<reqwest::Error> for IronclawError {
    fn from(e: reqwest::Error) -> Self {
        Self::Internal(format!("HTTP request error: {}", e))
    }
}
