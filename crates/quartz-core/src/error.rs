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

/// io 错误 → 人类可读消息：os 28 = ENOSPC（存储满）、os 13 = EACCES（权限不足），
/// 其余原样携带路径上下文 —— 下载管线写盘失败时给用户可行动提示而非裸 os error。
pub fn io_context(e: &std::io::Error, path: &std::path::Path) -> String {
    match e.raw_os_error() {
        Some(28) => format!(
            "写入失败（设备存储空间不足）：{}，请清理存储后重试",
            path.display()
        ),
        Some(13) => format!("写入失败（权限不足）：{}", path.display()),
        _ => format!("文件操作失败（{e}）：{}", path.display()),
    }
}
