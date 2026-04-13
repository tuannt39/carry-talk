use serde::Serialize;

/// Unified error type for CarryTalk.
/// Each variant maps to a module boundary.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Audio capture: {0}")]
    AudioCapture(String),

    #[error("Resampler: {0}")]
    Resampler(String),

    #[error("WebSocket: {0}")]
    WebSocket(String),

    #[error("Storage: {0}")]
    Storage(String),

    #[error("Settings: {0}")]
    Settings(String),

    #[error("Authentication: {0}")]
    Auth(String),

    #[error("Session: {0}")]
    Session(String),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization: {0}")]
    Serde(#[from] serde_json::Error),
}

// Tauri commands require errors to be Serialize
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
