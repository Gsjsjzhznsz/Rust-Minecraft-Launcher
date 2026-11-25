//! 启动器设置（settings.json）：非启动必需的用户偏好，全部可选。
//!
//! - extra_jvm_args：追加在版本 JSON JVM 参数之后（-cp/-Xmx 等核心参数除外）；
//! - azure_client_id：微软登录用项目自有 Azure Public Client ID；
//! - global_game_dir 是否使用实例隔离由 instance 体系决定，不在此存储。

use crate::error::Result;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 用户自定义 JVM 参数（追加于启动计划之后）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_jvm_args: Vec<String>,
    /// 微软设备码登录的 Azure 应用 client id（必须项目自有，禁止借用第三方）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_client_id: Option<String>,
    /// 全局内存分配兜底（实例未单独设置时使用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    /// 当前选中的游戏实例 id（None = 使用全局 .minecraft 目录）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_instance: Option<String>,
}

fn settings_path() -> PathBuf {
    paths::config_dir().join("quartz-settings.json")
}

/// 读取设置；缺失/损坏返回默认值（设置不应阻塞启动）。
pub fn load() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// 保存设置。
pub fn save(s: &Settings) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

/// 合并追加 JVM 参数（去重：与现有集合相同的参数不重复加入）。
pub fn add_jvm_args(args: Vec<String>) -> Result<Settings> {
    let mut s = load();
    for a in args {
        let a = a.trim().to_string();
        if a.is_empty() {
            continue;
        }
        if !s.extra_jvm_args.contains(&a) {
            s.extra_jvm_args.push(a);
        }
    }
    save(&s)?;
    Ok(s)
}

pub fn set_jvm_args(args: Vec<String>) -> Result<Settings> {
    let mut s = load();
    s.extra_jvm_args = args
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    save(&s)?;
    Ok(s)
}

pub fn set_azure_client_id(id: Option<String>) -> Result<Settings> {
    let mut s = load();
    s.azure_client_id = id.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    save(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_defaults() {
        let s = Settings::default();
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(text, "{}");
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert!(back.extra_jvm_args.is_empty());
    }
}
