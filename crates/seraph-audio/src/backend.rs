use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend not implemented yet")]
    NotImplemented,
    #[error("audio device not found")]
    DeviceNotFound,
    #[error("device lost: {0}")]
    DeviceLost(String),
    #[error("exclusive mode unavailable: {0}")]
    ExclusiveModeUnavailable(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, BackendError>;
