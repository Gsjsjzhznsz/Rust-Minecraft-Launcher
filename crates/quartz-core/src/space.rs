//! 存储空间探测（下载前置检查 / 诊断横幅）。
//!
//! 真机事实（Android）：/storage/emulated/0（FUSE）与 /data（应用私有目录）
//! 共享同一个 userdata 分区 —— 游戏资源数百 MB 的下载失败残留（.qz-part /
//! .staging / .part）会把分区塞满，此后所有写入一律失败：轻则「下载失败」
//! 反复重试越塞越满，重则 WebView 无法初始化「一打开就闪退」。
//! 因此所有在线下载前必须做可用空间检查，失败时给出可行动的中文提示。

use std::path::Path;

/// 目标路径所在分区的可用字节（普通用户可写额度 f_bavail × f_frsize）。
/// 探测失败（路径不存在 / 平台不支持）返回 None —— 调用方应放行而不是拦截。
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c = CString::new(path.to_string_lossy().into_owned()).ok()?;
        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY：vfs 为 statvfs 零值占位，statvfs 由内核填充
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut vfs) };
        if rc != 0 {
            return None;
        }
        Some(vfs.f_bavail as u64 * vfs.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// 人类可读容量（MB / GB）。
pub fn human_bytes(n: u64) -> String {
    let mb = n / (1024 * 1024);
    if mb >= 1024 {
        format!("{:.1}GB", mb as f64 / 1024.0)
    } else {
        format!("{mb}MB")
    }
}

/// 空间不足的可行动报错文案（`what` 描述任务，如「下载 Java 运行时」）。
pub fn insufficient(need: u64, free: u64, what: &str) -> String {
    format!(
        "设备存储空间不足：仅剩 {}，{what}至少需要 {}。请清理存储空间后重试（启动器已在本次启动时自动清理上次下载的残留缓存）",
        human_bytes(free),
        human_bytes(need)
    )
}

/// 下载前置检查：剩余额度不可知则放行；不足则返回可行动错误文案。
pub fn ensure_free(path: &Path, need: u64, what: &str) -> Result<(), String> {
    match free_space(path) {
        Some(free) if free < need => Err(insufficient(need, free, what)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(0), "0MB");
        assert_eq!(human_bytes(512 * 1024 * 1024), "512MB");
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.0GB");
    }

    #[test]
    fn insufficient_message_mentions_both_sides() {
        let msg = insufficient(800 * 1024 * 1024, 123 * 1024 * 1024, "下载游戏");
        assert!(msg.contains("123MB"));
        assert!(msg.contains("800MB"));
        assert!(msg.contains("下载游戏"));
    }

    #[cfg(unix)]
    #[test]
    fn free_space_on_tmp() {
        let free = free_space(&std::env::temp_dir());
        assert!(free.unwrap_or(0) > 0);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_free_blocks_outrageous_need() {
        // 天文数字需求必须拦截（临时目录所在分区额度可探测时）
        if free_space(&std::env::temp_dir()).is_some() {
            assert!(ensure_free(&std::env::temp_dir(), u64::MAX / 2, "测试任务").is_err());
        }
        // 不存在的路径探测失败 -> 放行
        let ghost = std::env::temp_dir().join("qz-space-no-such-dir");
        assert!(ensure_free(&ghost, u64::MAX / 2, "测试任务").is_ok());
    }
}
