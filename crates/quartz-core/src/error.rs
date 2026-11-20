//! 统一错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("hash mismatch for {0}: expected {1}, got {2}")]
    ShaMismatch(String, String, String),

    #[error("size mismatch for {0}: expected {1}, got {2}")]
    SizeMismatch(String, u64, u64),

    #[error("missing config: {0}")]
    MissingConfig(String),

    #[error("unsupported on this platform: {0}")]
    Unsupported(&'static str),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
