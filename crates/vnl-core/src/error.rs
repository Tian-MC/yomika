use serde::{Deserialize, Serialize};

/// Stable, machine-readable error categories exposed to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    NotFound,
    InvalidInput,
    Io,
    Database,
    Network,
    /// A required external program (wine, gamescope, ...) is missing.
    MissingDependency,
    /// Spawning or controlling a process failed.
    Process,
    /// The operation needs explicit user confirmation (e.g. overwriting saves).
    ConfirmationRequired,
    /// The feature exists in the contract but is not implemented yet.
    NotImplemented,
    Unsupported,
    Internal,
}

/// Error type returned by every Tauri command. Serialized as
/// `{ code, message, details }` so the UI can show a friendly message and
/// keep the technical details in an expandable section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), details: None }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, what)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn not_implemented(feature: &str) -> Self {
        Self::new(ErrorCode::NotImplemented, format!("{feature} is not implemented yet"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let code = if e.kind() == std::io::ErrorKind::NotFound { ErrorCode::NotFound } else { ErrorCode::Io };
        AppError::new(code, e.to_string())
    }
}
