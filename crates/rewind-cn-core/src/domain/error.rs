use thiserror::Error;

#[derive(Debug, Error)]
pub enum RewindError {
    #[error("Validation error: {field} — {message}")]
    Validation { field: String, message: String },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Config error: {0}")]
    Config(String),
}

impl RewindError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }
}
