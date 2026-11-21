//! 标准目录布局唯一权威（与官方 Minecraft / ZL2 互通）。
//!
//! Android 端根目录按存储权限分级（壳层启动时解析后注入）：
//! - 已授权 READ/WRITE_EXTERNAL_STORAGE（targetSdk 28 旧模型）->
//!   `/storage/emulated/0/Quartz/files`（文件管理器可见、卸载不丢，推荐）；
//! - 未授权 -> 应用外部私有目录（可浏览，卸载丢失）-> 内部私有目录兜底。
//!
//! ```text
//! <根>/
//! ├── .minecraft/
//! │   ├── versions/<v>/<v>.jar、<v>.json
//! │   ├── libraries/<group>/<artifact>/<ver>/<artifact>-<ver>[-classifier].jar
//! │   ├── assets/{indexes/<id>.json, objects/<hash[:2]>/<hash>}
//! │   ├── saves/  mods/  resourcepacks/  shaderpacks/
//! │   └── config/  logs/  options.txt
//! ├── accounts.json          # 离线账号（auth 模块读写）
//! └── jres/<jre-id>/         # 公共布局侧的运行时目录（桌面端使用）
//! ```
//!
//! 注意：Android 上 JVM pack 必须放在可 exec 的内部目录（见 [`runtime_dir`]），
//! 公共目录 `jres/` 仅作为桌面端与文档布局的落点。
//!
//! 桌面开发态：环境变量 `QUARTZ_GAME_ROOT` 覆盖根目录，否则 `~/.quartz-game`。
//! 所有落盘路径必须经由本模块获取，禁止在别处手拼路径。

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Android 应用包名（与 apps/shell-tauri/src-tauri/tauri.conf.json 的 identifier 一致）。
pub const ANDROID_PACKAGE_ID: &str = "com.quartz.launcher";

/// 运行时根目录覆盖（壳层解析后注入；授权状态变化时允许重注入，由壳层负责迁移数据）。
///
/// 背景（error13 / os error 13 = EACCES）：Android 11+ scoped storage 下，
/// 直接手拼 `/storage/emulated/0/Android/data/<pkg>/files` 在部分 ROM 上
/// 无法由应用创建（FUSE 拒绝 mkdir），必须经 `Context.getExternalFilesDir()`
/// 等平台 API 取真实可写路径。硬编码路径只作为最后回退。
static RUNTIME_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 可 exec 的 JVM pack 根目录（Android = 内部私有目录；未注入时回退游戏根 jres/）。
///
/// 两个硬性平台约束（M0 启动链路）：
/// 1. Android 10（API 29）起 targetSdk >= 29 禁止 exec() 应用主目录文件 ——
///    壳层 APK 固定 targetSdk 28，`bin/java` 才能作为子进程拉起；
/// 2. `/storage/emulated/0`（FUSE）挂载为 noexec —— JVM pack 不能随游戏根放在公共目录。
static RUNTIME_JRES_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 注入运行时游戏根目录（覆盖旧值；壳层在 ensure_layout 之前调用）。
pub fn set_runtime_root(root: PathBuf) -> bool {
    *RUNTIME_ROOT
        .lock()
        .expect("RUNTIME_ROOT 锁中毒") = Some(root);
    true
}

/// 运行时注入的根目录（未注入返回 None）。
pub fn runtime_root() -> Option<PathBuf> {
    RUNTIME_ROOT
        .lock()
        .expect("RUNTIME_ROOT 锁中毒")
        .clone()
}

/// 注入可 exec 的 JVM pack 根目录（Android 壳层注入内部私有目录）。
pub fn set_runtime_jres_root(root: PathBuf) {
    *RUNTIME_JRES_ROOT
        .lock()
        .expect("RUNTIME_JRES_ROOT 锁中毒") = Some(root);
}

/// JVM pack 安装根目录（exec 必须可用）：注入值优先，回退游戏根 jres/。
pub fn runtime_dir() -> PathBuf {
    if let Some(p) = RUNTIME_JRES_ROOT
        .lock()
        .expect("RUNTIME_JRES_ROOT 锁中毒")
        .clone()
    {
        return p;
    }
    game_root().join("jres")
}

/// 游戏根目录（唯一权威）。优先级：运行时注入 > 环境变量 > 平台默认。
pub fn game_root() -> PathBuf {
    if let Some(root) = runtime_root() {
        return root;
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

        // —— 运行时覆盖（Android 壳层注入场景，可重入以支持授权后切换根目录）—— //
        let override_root = std::env::temp_dir().join("quartz-test-root");
        assert!(set_runtime_root(override_root.clone()));
        assert_eq!(game_root(), override_root);
        assert_eq!(runtime_root(), Some(override_root.clone()));
        assert_eq!(minecraft_dir(), override_root.join(".minecraft"));
        assert_eq!(accounts_file(), override_root.join("accounts.json"));

        // —— 授权后切换根目录（重注入覆盖旧值）—— //
        let public_root = std::env::temp_dir().join("quartz-test-public");
        assert!(set_runtime_root(public_root.clone()));
        assert_eq!(game_root(), public_root);

        // —— JVM pack 目录：注入优先，回退游戏根 jres/ —— //
        let jres_root = public_root.join("jres");
        assert_eq!(runtime_dir(), jres_root);
        let exec_root = std::env::temp_dir().join("quartz-test-jres");
        set_runtime_jres_root(exec_root.clone());
        assert_eq!(runtime_dir(), exec_root);
        set_runtime_jres_root(jres_root.clone());
        assert_eq!(runtime_dir(), jres_root);
    }
}
