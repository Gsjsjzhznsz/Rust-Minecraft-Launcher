//! 标准目录布局唯一权威（与官方 Minecraft / ZL2 互通）。
//!
//! Android 端根 = 应用外部目录 files/（scoped storage 下应用对
//! Android/data/<pkg>/files/ 有读写权限，无需额外存储权限）：
//!
//! ```text
//! files/
//! ├── .minecraft/
//! │   ├── versions/<v>/<v>.jar、<v>.json
//! │   ├── libraries/<group>/<artifact>/<ver>/<artifact>-<ver>[-classifier].jar
//! │   ├── assets/{indexes/<id>.json, objects/<hash[:2]>/<hash>}
//! │   ├── saves/  mods/  resourcepacks/  shaderpacks/
//! │   └── config/  logs/  options.txt
//! ├── accounts.json          # 离线账号（auth 模块读写）
//! └── jres/<jre-id>/         # M0 收尾：Pojav LWJGL 运行时/JVM pack
//! ```
//!
//! 桌面开发态：环境变量 `QUARTZ_GAME_ROOT` 覆盖根目录，否则 `~/.quartz-game`。
//! 所有落盘路径必须经由本模块获取，禁止在别处手拼路径。

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Android 应用包名（与 apps/shell-tauri/src-tauri/tauri.conf.json 的 identifier 一致）。
pub const ANDROID_PACKAGE_ID: &str = "com.quartz.launcher";

/// 运行时根目录覆盖（壳层启动时经平台 API 解析后注入）。
///
/// 背景（error13 / os error 13 = EACCES）：Android 11+ scoped storage 下，
/// 直接手拼 `/storage/emulated/0/Android/data/<pkg>/files` 在部分 ROM 上
/// 无法由应用创建（FUSE 拒绝 mkdir），必须经 `Context.getExternalFilesDir()`
/// 等平台 API 取真实可写路径。壳层（src-tauri）在启动时调用本函数注入；
/// 硬编码路径只作为最后回退。
static RUNTIME_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 注入运行时游戏根目录（仅首次生效；壳层在 ensure_layout 之前调用）。
pub fn set_runtime_root(root: PathBuf) -> bool {
    RUNTIME_ROOT.set(root).is_ok()
}

/// 运行时注入的根目录（未注入返回 None）。
pub fn runtime_root() -> Option<&'static PathBuf> {
    RUNTIME_ROOT.get()
}

/// 游戏根目录（唯一权威）。优先级：运行时注入 > 环境变量 > 平台默认。
pub fn game_root() -> PathBuf {
    if let Some(root) = RUNTIME_ROOT.get() {
        return root.clone();
    }
    if let Ok(root) = std::env::var("QUARTZ_GAME_ROOT") {
        if !root.trim().is_empty() {
            return PathBuf::from(root);
        }
    }
    #[cfg(target_os = "android")]
    {
        PathBuf::from(format!(
            "/storage/emulated/0/Android/data/{ANDROID_PACKAGE_ID}/files"
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".quartz-game")
    }
}

/// 标准 .minecraft 目录。
pub fn minecraft_dir() -> PathBuf {
    game_root().join(".minecraft")
}

/// versions/<v>/
pub fn version_dir(version: &str) -> PathBuf {
    versions_dir().join(version)
}

/// versions/
pub fn versions_dir() -> PathBuf {
    minecraft_dir().join("versions")
}

/// versions/<v>/<v>.json
pub fn version_json(version: &str) -> PathBuf {
    version_dir(version).join(format!("{version}.json"))
}

/// versions/<v>/<v>.jar
pub fn version_jar(version: &str) -> PathBuf {
    version_dir(version).join(format!("{version}.jar"))
}

/// libraries/
pub fn libraries_dir() -> PathBuf {
    minecraft_dir().join("libraries")
}

/// libraries/<maven 相对路径>
pub fn library_path(maven_relative: &str) -> PathBuf {
    libraries_dir().join(maven_relative)
}

/// assets/
pub fn assets_dir() -> PathBuf {
    minecraft_dir().join("assets")
}

/// assets/indexes/
pub fn asset_indexes_dir() -> PathBuf {
    assets_dir().join("indexes")
}

/// assets/indexes/<id>.json
pub fn asset_index_file(index_id: &str) -> PathBuf {
    asset_indexes_dir().join(format!("{index_id}.json"))
}

/// assets/objects/
pub fn asset_objects_dir() -> PathBuf {
    assets_dir().join("objects")
}

/// assets/objects/<hash[:2]>/<hash>
pub fn asset_object_path(hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or(hash);
    asset_objects_dir().join(prefix).join(hash)
}

/// saves/ mods/ resourcepacks/ shaderpacks/ config/ logs/
pub fn saves_dir() -> PathBuf {
    minecraft_dir().join("saves")
}
pub fn mods_dir() -> PathBuf {
    minecraft_dir().join("mods")
}
pub fn resourcepacks_dir() -> PathBuf {
    minecraft_dir().join("resourcepacks")
}
pub fn shaderpacks_dir() -> PathBuf {
    minecraft_dir().join("shaderpacks")
}
pub fn config_dir() -> PathBuf {
    minecraft_dir().join("config")
}
pub fn logs_dir() -> PathBuf {
    minecraft_dir().join("logs")
}

/// .minecraft/options.txt
pub fn options_txt() -> PathBuf {
    minecraft_dir().join("options.txt")
}

/// files/accounts.json（注意：在游戏根，不在 .minecraft 内）
pub fn accounts_file() -> PathBuf {
    game_root().join("accounts.json")
}

/// files/jres/<jre-id>/
pub fn jres_dir() -> PathBuf {
    game_root().join("jres")
}

/// 首次运行：创建全部标准目录并放置空 options.txt（幂等）。
pub fn ensure_layout() -> std::io::Result<()> {
    let dirs = [
        minecraft_dir(),
        versions_dir(),
        libraries_dir(),
        asset_indexes_dir(),
        asset_objects_dir(),
        saves_dir(),
        mods_dir(),
        resourcepacks_dir(),
        shaderpacks_dir(),
        config_dir(),
        logs_dir(),
        jres_dir(),
    ];
    for dir in &dirs {
        fs::create_dir_all(dir)?;
    }
    let options = options_txt();
    if !options.exists() {
        fs::write(&options, "")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 单测试函数串行执行：RUNTIME_ROOT 是进程级全局状态，
    // 拆成多个并行测试会互相污染（先验证默认布局，再验证运行时覆盖）。
    #[test]
    fn layout_and_runtime_override() {
        // —— 默认布局：全部派生路径相对同一根 —— //
        let root = game_root();
        assert!(root.ends_with(".minecraft") || root.join(".minecraft").is_absolute());
        assert_eq!(version_jar("1.20.4"), versions_dir().join("1.20.4/1.20.4.jar"));
        assert_eq!(asset_object_path("abcdef12"), asset_objects_dir().join("ab/abcdef12"));
        assert_eq!(
            library_path("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar"),
            libraries_dir().join("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar")
        );
        // accounts.json 与 jres 位于游戏根（而非 .minecraft 内）
        assert_eq!(accounts_file(), root.join("accounts.json"));
        assert_eq!(jres_dir(), root.join("jres"));

        // —— 运行时覆盖（Android 壳层注入场景）—— //
        let override_root = std::env::temp_dir().join("quartz-test-root");
        assert!(set_runtime_root(override_root.clone()));
        // 重复注入被忽略（仅首次生效）
        assert!(!set_runtime_root(std::env::temp_dir().join("other")));
        assert_eq!(game_root(), override_root);
        assert_eq!(runtime_root(), Some(&override_root));
        assert_eq!(minecraft_dir(), override_root.join(".minecraft"));
        assert_eq!(accounts_file(), override_root.join("accounts.json"));
    }
}
