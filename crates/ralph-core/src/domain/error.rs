use thiserror::Error;

#[derive(Debug, Error)]
pub enum RalphError {
    #[error("Validation error: {field} — {message}")]
    Validation { field: String, message: String },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Storage error: {0}")]
    Storage(String),
}

impl RalphError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }
}
