use thiserror::Error;

#[derive(Error, Debug)]
pub enum Night24Error {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("extension error: {0}")]
    Extension(String),

    #[error("tool execution error: {0}")]
    ToolExecution(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = anyhow::Result<T>;
