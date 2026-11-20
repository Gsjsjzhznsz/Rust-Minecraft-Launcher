//! 渲染路由：把调研阶段确定的「渲染矩阵」编码为可测试的选择器。
//!
//! 原则（与竞品一致的行业实践）：Rust 不重写任何 C/C++ 渲染组件，
//! gl4es / ANGLE / Zink / MetalANGLE / Mesa-Dozen 以「渲染器 pack」形式
//! 随包分发；本模块只负责按 OS/架构/设备能力/游戏版本选 pack。
//!
//! 依赖图谱：MC 1.17+ 需要桌面 GL 3.2（Profile32）；1.16- 仅需 GL 2.1
//! （Profile20）；Sodium/光影推荐 GL 4.6（Profile46，Zink）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GlProfile {
    /// GL 2.1 + 少量扩展（gl4es 提供的桌面语义）。
    Profile20,
    /// GL 3.2 core（MC 1.17+ 底线）。
    Profile32,
    /// GL 4.6（Sodium/光影推荐档）。
    Profile46,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Gles2,
    Gles3,
    Vulkan,
    Metal,
    D3d12,
    NativeGl,
}

/// 渲染器 pack 元数据（仅序列化输出，不需要反序列化）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererPack {
    pub id: &'static str,
    /// 该 pack 对游戏暴露的桌面 GL 语义档位。
    pub provides: GlProfile,
    /// 底层真实后端。
    pub backend: Backend,
    /// 适用的 OS（osx/windows/linux/android/ios/harmonyos）。
    pub os: &'static [&'static str],
    pub arch: &'static [&'static str],
    pub note: &'static str,
}

/// 内置 pack 目录（真实二进制以 renderpacks/ 在线仓库分发，此处为元数据）。
pub const CATALOG: &[RendererPack] = &[
    RendererPack {
        id: "gl4es",
        provides: GlProfile::Profile20,
        backend: Backend::Gles2,
        os: &["android", "ios", "harmonyos", "linux"],
        arch: &["arm64", "arm", "x86", "x86_64"],
        note: "GL2.1→GLES2，老版本/低配兜底（Pojav 系默认）",
    },
    RendererPack {
        id: "angle",
        provides: GlProfile::Profile32,
        backend: Backend::Gles3,
        os: &["android", "ios", "harmonyos", "windows", "linux"],
        arch: &["arm64", "x86_64"],
        note: "GLES3→Vulkan/Metal(D3D)，1.17+ 基础档",
    },
    RendererPack {
        id: "zink",
        provides: GlProfile::Profile46,
        backend: Backend::Vulkan,
        os: &["android", "harmonyos", "linux", "windows"],
        arch: &["arm64", "x86_64"],
        note: "GL4.6→Vulkan，1.17++/Sodium/光影推荐档",
    },
    RendererPack {
        id: "metalangle",
        provides: GlProfile::Profile32,
        backend: Backend::Metal,
        os: &["ios"],
        arch: &["arm64"],
        note: "GL→Metal，iOS 主力（MetalANGLE）",
    },
    RendererPack {
        id: "virgl",
        provides: GlProfile::Profile32,
        backend: Backend::Gles3,
        os: &["android"],
        arch: &["arm64", "x86_64"],
        note: "GL over GLES 兼容兜底",
    },
    RendererPack {
        id: "native-gl",
        provides: GlProfile::Profile46,
        backend: Backend::NativeGl,
        os: &["osx", "linux"],
        arch: &["arm64", "x86_64"],
        note: "桌面原生 GL（macOS ARM 4.1 / Linux 驱动）",
    },
    RendererPack {
        id: "dozen",
        provides: GlProfile::Profile46,
        backend: Backend::D3d12,
        os: &["windows"],
        arch: &["arm64", "x86_64"],
        note: "GL→D3D12（Mesa Dozen），Windows on ARM 主力",
    },
];

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceCaps {
    pub vulkan: bool,
    pub metal: bool,
    pub gles3: bool,
    pub desktop_gl_32: bool,
}

impl DeviceCaps {
    /// 按平台给出保守默认值（真实设备探测在 HAL/壳层完成）。
    pub fn for_os(os: &str) -> Self {
        match os {
            "ios" => Self {
                vulkan: false,
                metal: true,
                gles3: true,
                desktop_gl_32: false,
            },
            "osx" => Self {
                vulkan: false,
                metal: true,
                gles3: false,
                desktop_gl_32: true,
            },
            "windows" => Self {
                vulkan: true,
                metal: false,
                gles3: false,
                desktop_gl_32: false,
            },
            "android" | "harmonyos" => Self {
                vulkan: true,
                metal: false,
                gles3: true,
                desktop_gl_32: false,
            },
            _ => Self {
                vulkan: true,
                metal: false,
                gles3: true,
                desktop_gl_32: true,
            },
        }
    }
}

/// MC 版本 -> 所需 GL 档位。1.17+ 需 GL3.2；快照/预发布按现代版本处理；
/// 远古命名（b1.x / a1.x / rd- / inf-）按最低档处理。
pub fn required_profile(mc_version: &str) -> GlProfile {
    let v = mc_version.trim().to_ascii_lowercase();
    if v.starts_with("b1.") || v.starts_with("a1.") || v.starts_with("rd-") || v.starts_with("inf-")
    {
        return GlProfile::Profile20;
    }
    if let Some(rest) = v.strip_prefix("1.") {
        let minor: u32 = rest
            .split(['.', 'w', ' ', '-', '_'])
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        return if minor >= 17 {
            GlProfile::Profile32
        } else {
            GlProfile::Profile20
        };
    }
    // 快照（如 24w03a）等现代命名默认按 1.17+ 处理。
    GlProfile::Profile32
}

fn pack_fits(
    pack: &RendererPack,
    os: &str,
    arch: &str,
    required: GlProfile,
    caps: &DeviceCaps,
) -> bool {
    if !pack.os.contains(&os) || !pack.arch.contains(&arch) {
        return false;
    }
    if pack.provides < required {
        return false;
    }
    match pack.backend {
        Backend::Vulkan => caps.vulkan,
        Backend::Metal => caps.metal,
        Backend::Gles3 => caps.gles3,
        Backend::NativeGl => caps.desktop_gl_32,
        Backend::D3d12 => true, // Windows 自带 D3D12
        Backend::Gles2 => true,
    }
}

/// 选择渲染器 pack。策略：
/// 1. 需求 ≤ Profile20 时优先 gl4es（翻译层数最少、兼容面最大）；
/// 2. 否则按能力择优：桌面 GL > Zink(Vulkan) > Metal > ANGLE(GLES3) > VirGL；
/// 3. 无匹配返回 None（上层提示设备不满足）。
pub fn select(
    os: &str,
    arch: &str,
    required: GlProfile,
    caps: &DeviceCaps,
) -> Option<&'static RendererPack> {
    let fits: Vec<&RendererPack> = CATALOG
        .iter()
        .filter(|p| pack_fits(p, os, arch, required, caps))
        .collect();

    if required == GlProfile::Profile20 {
        if let Some(p) = fits.iter().find(|p| p.id == "gl4es") {
            return Some(*p);
        }
    }

    let rank = |p: &RendererPack| -> u8 {
        match (os, p.backend) {
            ("osx", Backend::NativeGl) => 0,
            ("ios", Backend::Metal) => 0,
            // WoA 上 Dozen 是最稳的桌面 GL 语义源，优先于依赖第三方 Vulkan 驱动
            ("windows", Backend::D3d12) => 0,
            (_, Backend::NativeGl) => 1,
            (_, Backend::Vulkan) => 2,
            (_, Backend::D3d12) => 2,
            (_, Backend::Metal) => 3,
            (_, Backend::Gles3) => 4,
            (_, Backend::Gles2) => 5,
        }
    };
    fits.into_iter().min_by_key(|p| (rank(p), p.provides))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(os: &str, arch: &str, mc: &str, caps: DeviceCaps) -> String {
        select(os, arch, required_profile(mc), &caps)
            .map(|p| p.id.to_string())
            .unwrap_or_else(|| "none".into())
    }

    #[test]
    fn android_matrix() {
        // 1.16.5 低配：gl4es 最小翻译
        assert_eq!(
            pick("android", "arm64", "1.16.5", DeviceCaps::for_os("android")),
            "gl4es"
        );
        // 1.20.4 有 Vulkan：zink（1.17+ 且 GL4.6 加持）
        let caps = DeviceCaps {
            vulkan: true,
            metal: false,
            gles3: true,
            desktop_gl_32: false,
        };
        assert_eq!(pick("android", "arm64", "1.20.4", caps), "zink");
        // 1.20.4 无 Vulkan 有 GLES3：angle
        let caps = DeviceCaps {
            vulkan: false,
            metal: false,
            gles3: true,
            desktop_gl_32: false,
        };
        assert_eq!(pick("android", "arm64", "1.20.4", caps), "angle");
        // 1.20.4 无任何高级能力：gl4es 无法满足 Profile32 → 无可用 pack
        //（对应立项书的性能红线：无加速路径不启动）
        let caps = DeviceCaps {
            vulkan: false,
            metal: false,
            gles3: false,
            desktop_gl_32: false,
        };
        assert_eq!(pick("android", "arm64", "1.20.4", caps), "none");
    }

    #[test]
    fn ios_and_desktop_matrix() {
        assert_eq!(
            pick("ios", "arm64", "1.20.4", DeviceCaps::for_os("ios")),
            "metalangle"
        );
        assert_eq!(
            pick("osx", "arm64", "1.20.4", DeviceCaps::for_os("osx")),
            "native-gl"
        );
        assert_eq!(
            pick("windows", "arm64", "1.20.4", DeviceCaps::for_os("windows")),
            "dozen"
        );
    }

    #[test]
    fn required_profile_routing() {
        assert_eq!(required_profile("1.16.5"), GlProfile::Profile20);
        assert_eq!(required_profile("1.17"), GlProfile::Profile32);
        assert_eq!(required_profile("1.20.4"), GlProfile::Profile32);
        assert_eq!(required_profile("b1.7.3"), GlProfile::Profile20);
        assert_eq!(required_profile("24w03a"), GlProfile::Profile32);
    }
}
