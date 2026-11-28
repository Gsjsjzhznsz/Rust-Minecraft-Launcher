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
    /// 是否处于禁用态（FCL/ZL2 同款：文件以 .disabled 结尾）。
    pub disabled: bool,
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
        let name = e.file_name().to_string_lossy().into_owned();
        let disabled = name.ends_with(".disabled");
        out.push(ContentEntry {
            name,
            kind,
            size_bytes: size,
            is_dir,
            modified_at,
            disabled,
        });
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.modified_at));
    Ok(out)
}

/// 删除内容项（文件或目录递归）；返回是否真正删除。
pub fn delete_content(kind: ContentKind, name: &str, instance_dir: Option<&Path>) -> Result<bool> {
    let path = guard_name(kind, name, instance_dir)?;
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

/// 同名安全解析（拒绝路径穿越），返回完整路径。
fn guard_name(kind: ContentKind, name: &str, instance_dir: Option<&Path>) -> Result<PathBuf> {
    if name.contains('/') || name.contains('\\') || name == ".." || name.is_empty() {
        return Err(crate::error::Error::Other(format!("非法内容名：{name}")));
    }
    Ok(content_dir(kind, instance_dir).join(name))
}

/// 启用/禁用切换（FCL/ZL2 同款语义：改名 `.disabled` 后缀，不删文件）。
/// 返回切换后的新文件名。
pub fn toggle_content(
    kind: ContentKind,
    name: &str,
    instance_dir: Option<&Path>,
) -> Result<String> {
    let path = guard_name(kind, name, instance_dir)?;
    if !path.exists() {
        return Err(crate::error::Error::Other(format!("内容不存在：{name}")));
    }
    let new_name = if let Some(stripped) = name.strip_suffix(".disabled") {
        stripped.to_string()
    } else {
        format!("{name}.disabled")
    };
    let dest = path.with_file_name(&new_name);
    if dest.exists() {
        return Err(crate::error::Error::Other(format!(
            "目标已存在：{new_name}"
        )));
    }
    std::fs::rename(&path, &dest)?;
    Ok(new_name)
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
    fn toggle_roundtrip_and_traversal_guard() {
        let base = std::env::temp_dir().join(format!("qz-content-test-{}", std::process::id()));
        let mods = base.join("mods");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.jar"), b"x").unwrap();

        // 禁用 -> .disabled
        let n = toggle_content(ContentKind::Mods, "a.jar", Some(&base)).unwrap();
        assert_eq!(n, "a.jar.disabled");
        assert!(mods.join("a.jar.disabled").exists());
        // 列表应带 disabled 标记
        let list = list_content(ContentKind::Mods, Some(&base)).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].disabled);
        // 再启用 -> 原名
        let n2 = toggle_content(ContentKind::Mods, "a.jar.disabled", Some(&base)).unwrap();
        assert_eq!(n2, "a.jar");
        assert!(mods.join("a.jar").exists());
        // 路径穿越拒绝
        assert!(toggle_content(ContentKind::Mods, "../escape", Some(&base)).is_err());
        assert!(delete_content(ContentKind::Mods, "../escape", Some(&base)).is_err());
        let _ = std::fs::remove_dir_all(&base);
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
