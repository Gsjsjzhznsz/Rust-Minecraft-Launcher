//! 镜像回退端到端冒烟（需真实网络）：
//! 1. BMCLAPI 清单拉取（Mojang 官方源镜像优先）；
//! 2. GitHub raw 经加速代理取流；
//! 3. 停滞超时不误伤正常慢速流。
use quartz_core::mirror;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = quartz_core::meta::http_client();

    // ① Mojang 清单（BMCLAPI 优先）
    let text = mirror::get_text(&client, quartz_core::meta::VERSION_MANIFEST_URL).await?;
    let m: serde_json::Value = serde_json::from_str(&text)?;
    println!(
        "manifest via mirror: {} versions, latest release = {}",
        m["versions"].as_array().map(|a| a.len()).unwrap_or(0),
        m["latest"]["release"].as_str().unwrap_or("?")
    );

    // ② GitHub raw 经镜像取流
    let url = "https://raw.githubusercontent.com/FCL-Team/FoldCraftLauncher/main/FCL/src/main/jniLibs/arm64-v8a/libvgpu.so";
    let mut resp = mirror::get_stream(&client, url).await?;
    let mut got: u64 = 0;
    while let Some(c) = mirror::next_chunk(&mut resp).await? {
        got += c.len() as u64;
    }
    println!("libvgpu.so via mirror: {got} bytes");

    // ③ GitHub release 大文件只取响应头验证可达
    let url2 = "https://github.com/PojavLauncherTeam/android-openjdk-build-multiarch/releases/download/jre17-ec28559/jre17-arm64-20210825-release.tar.xz";
    let resp2 = mirror::get_stream(&client, url2).await?;
    println!(
        "jre17 pack reachable: content_length = {:?}",
        resp2.content_length()
    );

    println!("MIRROR_SMOKE_OK");
    Ok(())
}
