//! 下载源镜像回退：GitHub 与 Mojang 官方源的可用性兜底（国内网络优先）。
//!
//! 真机网络现实（2026-11 逐 URL 验证）：GitHub Releases / raw.githubusercontent.com
//! 直连在国内网络下极易握手超时或传输中途重置；Mojang 官方源（piston-meta /
//! piston-data / libraries / resources）可达但慢。BMCLAPI 与 GitHub 加速代理
//! 为社区镜像，稳定性随时间波动 —— 因此本模块生成「候选源列表」，镜像优先、
//! 官方直连兜底：镜像失效时自动回退直连，镜像生效时国内体验显著提升。
//!
//! 候选策略（已验证）：
//! - Mojang 四类官方源 -> BMCLAPI（bmclapi2.bangbang93.com）宿主替换 + 直连；
//! - GitHub（release / raw）-> ghfast.top -> gh-proxy.com -> ghproxy.net -> 直连；
//! - 其余源（maven.fabricmc.net 等可达）-> 直连。
//!
//! 下载执行器对候选列表逐个尝试并做流停滞检测（[`next_chunk`]）：
//! 任一候选成功即返回；全部失败时保留最后一个错误（含 URL）供 UI 呈现。

use crate::error::{Error, Result};

/// GitHub 加速代理（镜像优先；前缀拼接原始完整 URL）。
/// 2026-11 实测可用性：ghfast.top / gh-proxy.com / ghproxy.net 均 206；
/// mirror.ghproxy.com、github.moeyy.xyz 已失效故不收录。
const GH_PROXIES: &[&str] = &["https://ghfast.top", "https://gh-proxy.com", "https://ghproxy.net"];

/// Mojang 官方源国内镜像（BMCLAPI）。
const BMCLAPI_BASE: &str = "https://bmclapi2.bangbang93.com";

/// 流停滞超时（秒）：读流间隔超过该值视为连接死亡（小区网络/代理抖动兜底）。
const STALL_SECS: u64 = 30;

/// 生成候选 URL 列表（镜像优先、直连兜底；保序去重）。
pub fn candidates(url: &str) -> Vec<String> {
    const PISTON_META: &str = "https://piston-meta.mojang.com/";
    const PISTON_DATA: &str = "https://piston-data.mojang.com/";
    const LIBRARIES: &str = "https://libraries.minecraft.net/";
    const RESOURCES: &str = "https://resources.download.minecraft.net/";
    const GH_RAW: &str = "https://raw.githubusercontent.com/";
    const GH_MAIN: &str = "https://github.com/";

    let mut out: Vec<String> = Vec::new();
    let mut push = |u: String| {
        if !out.contains(&u) {
            out.push(u);
        }
    };

    if let Some(rest) = url.strip_prefix(PISTON_META) {
        push(format!("{BMCLAPI_BASE}/{rest}"));
        push(url.to_string());
    } else if let Some(rest) = url.strip_prefix(PISTON_DATA) {
        push(format!("{BMCLAPI_BASE}/{rest}"));
        push(url.to_string());
    } else if let Some(rest) = url.strip_prefix(LIBRARIES) {
        push(format!("{BMCLAPI_BASE}/maven/{rest}"));
        push(url.to_string());
    } else if let Some(rest) = url.strip_prefix(RESOURCES) {
        push(format!("{BMCLAPI_BASE}/assets/{rest}"));
        push(url.to_string());
    } else if url.starts_with(GH_RAW) || url.starts_with(GH_MAIN) {
        for p in GH_PROXIES {
            push(format!("{p}/{url}"));
        }
        push(url.to_string());
    } else {
        push(url.to_string());
    }
    out
}

/// 按候选列表取流（跟随镜像回退；返回第一个校验通过的成功响应）。
pub async fn get_stream(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let urls = candidates(url);
    let mut last_err: Option<Error> = None;
    for u in &urls {
        match client.get(u).send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => return Ok(resp),
                Err(e) => last_err = Some(e.into()),
            },
            Err(e) => last_err = Some(e.into()),
        }
    }
    Err(Error::Other(format!(
        "下载失败（已尝试 {} 个来源，含镜像）：{}",
        urls.len(),
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "未知错误".into())
    )))
}

/// 读一个响应 chunk，带流停滞超时；返回 None 表示流结束。
pub async fn next_chunk(resp: &mut reqwest::Response) -> Result<Option<Vec<u8>>> {
    match tokio::time::timeout(std::time::Duration::from_secs(STALL_SECS), resp.chunk()).await {
        Err(_elapsed) => Err(Error::Other(format!(
            "连接停滞超过 {STALL_SECS} 秒，已中止（网络不稳，请重试；下载会从断点继续）"
        ))),
        Ok(Ok(None)) => Ok(None),
        Ok(Ok(Some(bytes))) => Ok(Some(bytes.to_vec())),
        Ok(Err(e)) => Err(e.into()),
    }
}

/// 按候选列表取全量文本（清单 / 版本 JSON 等小文件）。
pub async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let mut resp = get_stream(client, url).await?;
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = next_chunk(&mut resp).await? {
        data.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// 按候选列表取全量字节。
pub async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = get_stream(client, url).await?;
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = next_chunk(&mut resp).await? {
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_release_mirror_first_direct_last() {
        let url = "https://github.com/PojavLauncherTeam/android-openjdk-build-multiarch/releases/download/jre17-ec28559/jre17-arm64-20210825-release.tar.xz";
        let c = candidates(url);
        assert_eq!(c.len(), 4, "3 镜像 + 1 直连");
        assert!(c[0].starts_with("https://ghfast.top/https://github.com/"));
        assert!(c[1].starts_with("https://gh-proxy.com/"));
        assert!(c[2].starts_with("https://ghproxy.net/"));
        assert_eq!(c[3], url);
    }

    #[test]
    fn github_raw_mirror_first() {
        let url = "https://raw.githubusercontent.com/FCL-Team/FoldCraftLauncher/main/FCL/src/main/jniLibs/arm64-v8a/libvgpu.so";
        let c = candidates(url);
        assert_eq!(c.len(), 4);
        assert!(c[0].contains("ghfast.top/https://raw.githubusercontent.com/"));
        assert_eq!(c[3], url);
    }

    #[test]
    fn mojang_official_bmclapi_first() {
        let m = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        assert_eq!(
            candidates(m)[0],
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
        assert_eq!(candidates(m)[1], m);

        let d = "https://piston-data.mojang.com/v1/objects/fd19469fed4a4b4c15b2d5133985f0e3e7816a8a/client.jar";
        assert_eq!(
            candidates(d)[0],
            "https://bmclapi2.bangbang93.com/v1/objects/fd19469fed4a4b4c15b2d5133985f0e3e7816a8a/client.jar"
        );

        let l = "https://libraries.minecraft.net/com/mojang/brigadier/1.1.8/brigadier-1.1.8.jar";
        assert_eq!(
            candidates(l)[0],
            "https://bmclapi2.bangbang93.com/maven/com/mojang/brigadier/1.1.8/brigadier-1.1.8.jar"
        );

        let r = "https://resources.download.minecraft.net/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356";
        assert_eq!(
            candidates(r)[0],
            "https://bmclapi2.bangbang93.com/assets/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356"
        );
    }

    #[test]
    fn other_sources_direct_only() {
        let u = "https://maven.fabricmc.net/net/fabricmc/fabric-loader/maven-metadata.xml";
        assert_eq!(candidates(u), vec![u]);
        let h = "https://example.com/file.bin";
        assert_eq!(candidates(h), vec![h]);
    }

    #[test]
    fn no_duplicates() {
        let c = candidates("https://github.com/a/b/releases/download/v1/x.jar");
        let uniq: std::collections::BTreeSet<_> = c.iter().collect();
        assert_eq!(uniq.len(), c.len());
    }
}
