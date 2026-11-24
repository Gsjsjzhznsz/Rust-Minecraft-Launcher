//! 内容管理：mods / resourcepacks / shaderpacks / saves / screenshots
//! 列表与删除（对标 FCL / ZL2 的内容页）。
//!
//! 目录来源：全局 .minecraft 标准目录；实例隔离模式下传 instance_dir，
//! 内容落到 <instance>/mods 等（与 FCL 的版本隔离语义一致）。

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mods,
    ResourcePacks,
    ShaderPacks,
    Saves,
    Screenshots,
    Logs,
}

impl ContentKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            ContentKind::Mods => "mods",
            ContentKind::ResourcePacks => "resourcepacks",
            ContentKind::ShaderPacks => "shaderpacks",
            ContentKind::Saves => "saves",
            ContentKind::Screenshots => "screenshots",
            ContentKind::Logs => "logs",
        }
    }

    pub fn parse(s: &str) -> Option<ContentKind> {
        match s {
            "mods" => Some(ContentKind::Mods),
            "resourcepacks" => Some(ContentKind::ResourcePacks),
            "shaderpacks" => Some(ContentKind::ShaderPacks),
            "saves" => Some(ContentKind::Saves),
            "screenshots" => Some(ContentKind::Screenshots),
            "logs" => Some(ContentKind::Logs),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentEntry {
    pub name: String,
    pub kind: ContentKind,
    /// 文件或目录的总字节数（目录递归统计）。
    pub size_bytes: u64,
    pub is_dir: bool,
    /// 最后修改时间（epoch 秒；不可得为 0）。
    pub modified_at: u64,
}

/// 内容目录解析：instance_dir 有值 -> <instance>/<dir>，否则全局 <mc>/<dir>。
pub fn content_dir(kind: ContentKind, instance_dir: Option<&Path>) -> PathBuf {
    let base = instance_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::paths::minecraft_dir);
    base.join(kind.dir_name())
}

/// 列出某类内容（不存在目录返回空列表而非报错）。
pub fn list_content(kind: ContentKind, instance_dir: Option<&Path>) -> Result<Vec<ContentEntry>> {
    let dir = content_dir(kind, instance_dir);
    let mut out: Vec<ContentEntry> = Vec::new();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(out),
    };
    for e in rd.flatten() {
        let path = e.path();
        let is_dir = path.is_dir();
        let size = if is_dir {
            dir_size(&path)
        } else {
            e.metadata().map(|m| m.len()).unwrap_or(0)
        };
        let modified_at = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(ContentEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            kind,
            size_bytes: size,
            is_dir,
            modified_at,
        });
    }
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(out)
}

/// 删除内容项（文件或目录递归）；返回是否真正删除。
pub fn delete_content(
    kind: ContentKind,
    name: &str,
    instance_dir: Option<&Path>,
) -> Result<bool> {
    let dir = content_dir(kind, instance_dir);
    // name 安全校验：拒绝路径穿越（../、绝对路径、分隔符）
    if name.contains('/') || name.contains('\\') || name == ".." || name.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "非法内容名：{name}"
        )));
    }
    let path = dir.join(name);
    if !path.exists() {
        return Ok(false);
    }
    if path.is_dir() {
        std::fs::remove_dir_all(&path)?;
    } else {
        std::fs::remove_file(&path)?;
    }
    Ok(true)
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_names_match_standard_layout() {
        assert_eq!(ContentKind::Mods.dir_name(), "mods");
        assert_eq!(ContentKind::ResourcePacks.dir_name(), "resourcepacks");
        assert_eq!(ContentKind::ShaderPacks.dir_name(), "shaderpacks");
        assert_eq!(ContentKind::Saves.dir_name(), "saves");
        assert_eq!(ContentKind::Screenshots.dir_name(), "screenshots");
    }

    #[test]
    fn parse_roundtrip() {
        for k in [
            ContentKind::Mods,
            ContentKind::ResourcePacks,
            ContentKind::ShaderPacks,
            ContentKind::Saves,
            ContentKind::Screenshots,
            ContentKind::Logs,
        ] {
            assert_eq!(ContentKind::parse(k.dir_name()), Some(k));
        }
        assert_eq!(ContentKind::parse("bogus"), None);
    }
}
