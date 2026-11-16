use std::fmt;

#[derive(Debug, Clone)]
pub struct CoreError {
    message: String,
}

impl CoreError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CoreError {}

impl From<serde_json::Error> for CoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
