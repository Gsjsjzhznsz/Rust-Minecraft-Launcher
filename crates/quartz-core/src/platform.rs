//! 平台 HAL（硬件抽象层）。
//!
//! 五端分层的关键边界：core 定义契约，各端提供实现。
//! - 桌面端（macOS ARM / Windows ARM）：子进程模式，直接 `Command` 启动 java；
//! - 移动端（Android / iOS / 鸿蒙）：进程内模式，`spawn` 由宿主壳层通过
//!   `JNI_CreateJavaVM`（Android）/ 静态链接 JVM（iOS/鸿蒙）完成，
//!   因此这里返回 `Error::Unsupported`，游戏参数仍由 LaunchPlan 提供。

use crate::download::DownloadTask;
use crate::error::{Error, Result};
use crate::jvmops::LaunchPlan;
use crate::renderbus::DeviceCaps;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Windows,
    Macos,
    Android,
    Ios,
    Harmonyos,
}

impl HostOs {
    /// rules 语境里的 OS 名。
    pub fn rule_name(&self) -> &'static str {
        match self {
            HostOs::Linux => "linux",
            HostOs::Windows => "windows",
            HostOs::Macos => "osx",
            HostOs::Android => "android",
            HostOs::Ios => "ios",
            HostOs::Harmonyos => "harmonyos",
        }
    }

    /// 渲染路由语境里的 OS 名（renderbus CATALOG 约定）。
    pub fn bus_name(&self) -> &'static str {
        match self {
            HostOs::Macos => "osx",
            other => other.rule_name(),
        }
    }
}

pub fn host_os() -> HostOs {
    #[cfg(target_os = "linux")]
    {
        HostOs::Linux
    }
    #[cfg(target_os = "windows")]
    {
        HostOs::Windows
    }
    #[cfg(target_os = "macos")]
    {
        HostOs::Macos
    }
    #[cfg(target_os = "android")]
    {
        HostOs::Android
    }
    #[cfg(target_os = "ios")]
    {
        HostOs::Ios
    }
}

pub fn host_arch() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "arm64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(target_arch = "x86")]
    {
        "x86"
    }
    #[cfg(target_arch = "arm")]
    {
        "arm"
    }
}

/// 平台 HAL 契约。
pub trait PlatformHal: Send + Sync {
    fn os(&self) -> HostOs;
    /// 保守的设备渲染能力（真实探测由壳层注入覆盖）。
    fn caps(&self) -> DeviceCaps;
    /// 桌面端子进程启动；移动端返回 Unsupported。
    fn spawn(&self, plan: &LaunchPlan) -> Result<crate::platform::GameProcess>;
    /// 下载 JVM 运行时的任务（默认实现：按 runtimes/<os>/<arch>/<major> 布局返回占位）。
    fn jvm_runtime_tasks(
        &self,
        runtimes_root: &std::path::Path,
        java_major: u32,
    ) -> Vec<DownloadTask> {
        // M1 里程碑接入真实运行时 pack 清单；当前返回空并记录布局约定。
        let _ = (runtimes_root, java_major);
        Vec::new()
    }
}

/// 子进程句柄（桌面端）。
pub struct GameProcess {
    pub child: std::process::Child,
}

impl GameProcess {
    pub fn wait(&mut self) -> Result<std::process::ExitStatus> {
        Ok(self.child.wait()?)
    }
}

/// 桌面端实现：子进程 JVM。
#[derive(Debug, Clone)]
pub struct DesktopHal {
    /// JVM 根目录（如 runtimes/osx/arm64/17/<pack>/），可覆盖。
    pub java_override: Option<std::path::PathBuf>,
}

impl PlatformHal for DesktopHal {
    fn os(&self) -> HostOs {
        host_os()
    }

    fn caps(&self) -> DeviceCaps {
        DeviceCaps::for_os(self.os().bus_name())
    }

    fn spawn(&self, plan: &LaunchPlan) -> Result<GameProcess> {
        let java = match &plan.java_path {
            Some(p) => p.clone(),
            None => self.java_override.clone().unwrap_or_else(default_java_exec),
        };
        let classpath =
            std::env::join_paths(&plan.classpath).map_err(|e| Error::Other(e.to_string()))?;
        // 现代版本 jvm_args 已含 -cp（由 jvmops 保证唯一）；旧版本由执行器补上。
        let has_cp_in_jvm = plan
            .jvm_args
            .iter()
            .any(|s| s == "-cp" || s.starts_with("-cp="));
        let mut cmd = std::process::Command::new(java);
        cmd.args(&plan.jvm_args);
        if !has_cp_in_jvm {
            cmd.arg("-cp").arg(&classpath);
        }
        cmd.arg(&plan.main_class)
            .args(&plan.game_args)
            .current_dir(&plan.working_dir);
        let child = cmd.spawn()?;
        Ok(GameProcess { child })
    }
}

/// 基础阶段占位：默认 java 取 PATH 上的可执行名。
fn default_java_exec() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        std::path::PathBuf::from("java.exe")
    } else {
        std::path::PathBuf::from("java")
    }
}

/// 移动端占位实现：进程内 JVM 由宿主壳层完成（见模块文档）。
#[derive(Debug, Clone, Default)]
pub struct MobileHal;

impl PlatformHal for MobileHal {
    fn os(&self) -> HostOs {
        host_os()
    }
    fn caps(&self) -> DeviceCaps {
        DeviceCaps::for_os(self.os().bus_name())
    }
    fn spawn(&self, _plan: &LaunchPlan) -> Result<GameProcess> {
        Err(Error::Unsupported(
            "移动端进程内 JVM 由 FFI/宿主壳层启动（JNI_CreateJavaVM），不使用子进程",
        ))
    }
}

/// 当前平台的默认 HAL。
pub fn default_hal() -> Box<dyn PlatformHal> {
    match host_os() {
        HostOs::Android | HostOs::Ios | HostOs::Harmonyos => Box::new(MobileHal),
        _ => Box::new(DesktopHal {
            java_override: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_context_names() {
        let os = host_os();
        // 在桌面 CI（linux）上规则名与总线名一致；移动名仅存在于对应平台。
        match os {
            HostOs::Linux | HostOs::Windows | HostOs::Macos => {
                assert_eq!(os.rule_name(), os.bus_name())
            }
            _ => {}
        }
        assert!(matches!(host_arch(), "arm64" | "x86_64" | "x86" | "arm"));
    }
}
