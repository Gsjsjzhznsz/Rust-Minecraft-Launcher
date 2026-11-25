//! LWJGL3-for-Android pack 管理：在线下载官方移植版类与原生库。
//!
//! 来源：PojavLauncherTeam/PojavLauncher「gladiolus」官方 Release APK（单一一致构建）。
//! 从 APK 内提取以下文件（jar 与 so 保证互相配套，版本由同一 APK 决定）：
//! - `assets/components/lwjgl3/lwjgl-glfw-classes.jar`：
//!   官方 LWJGL jar 与安卓 GLFW 桥接类合并产物（org.lwjgl 全量类 +
//!   安卓 GLFW 实现）。其 `org.lwjgl.glfw.GLFW` 静态初始化会
//!   `System.loadLibrary("pojavexec")` 并以 `libpojavexec.so` 作为
//!   GLFW Functions 表载体（pojavInit/pojavCreateContext/... 均由它导出），
//!   启动时整体替换版本 JSON 里的 org.lwjgl:* 类路径（Mojang 分发的
//!   natives-linux 为 glibc 构建，bionic 无法加载）。
//! - 原生库（lib/arm64-v8a，bionic 构建，装入 java.library.path 与
//!   LD_LIBRARY_PATH）：
//!   - `libpojavexec.so`   GLFW 桥接 + 输入/剪贴板 native 实现（**必须**）
//!   - `liblwjgl.so`       LWJGL core
//!   - `liblwjgl_opengl.so` OpenGL bindings
//!   - `liblwjgl_stb.so`   STB bindings（MC 1.19+ 图像处理需要）
//!   - `liblwjgl_tinyfd.so` tinyfiledialogs bindings
//!   - `libopenal.so`      OpenAL 实现（openal 模块默认名 openal）
//!   - `libfreetype.so`    FreeType（freetype 模块默认名 freetype）
//!   - `libjnidispatch.so` JNA 的 bionic 构建（oshi-core → JNA；jar 内自带
//!     的 linux-aarch64 版为 glibc 构建，bionic 加载必败 —— System.loadLibrary
//!     优先级高于 jar 解包，本 so 在 java.library.path 即可拦截）
//!
//! Java 8 桥接类（cacio 等）不在本 pack 范围：1.20.4 直接子进程 JVM 不需要。

use crate::error::{Error, Result};
use crate::instance::DownloadProgress;
use crate::paths;
use std::path::PathBuf;

/// gladiolus 官方发布 APK（jar + 全部 bionic natives 单一来源）。
const POJAV_APK: &str =
    "https://github.com/PojavLauncherTeam/PojavLauncher/releases/download/gladiolus/PojavLauncher.apk";

/// APK 内需提取的文件清单：(APK 条目路径, 落盘相对路径)。
///
/// 注意：`libpojavexec.so` **不再**从 Pojav APK 提取 —— gladiolus 发布包内
/// 旧版 C 桥的 JNI_OnLoad 会 RegisterNatives 注册 jar 里不存在的
/// `nativeSetUseInputStackQueue`，RegisterNatives 直接 NoSuchMethodError
/// （v0.1.4 退出码 255 的根因）。该符号位现在由 Rust 重构的 quartz-bridge
/// 以同名 libpojavexec.so 落位覆盖（见壳层 game_bridge::ensure_bridge_lib）。
const APK_FILES: &[(&str, &str)] = &[
    (
        "assets/components/lwjgl3/lwjgl-glfw-classes.jar",
        "lwjgl-glfw-classes.jar",
    ),
    ("lib/arm64-v8a/liblwjgl.so", "natives/liblwjgl.so"),
    (
        "lib/arm64-v8a/liblwjgl_opengl.so",
        "natives/liblwjgl_opengl.so",
    ),
    ("lib/arm64-v8a/liblwjgl_stb.so", "natives/liblwjgl_stb.so"),
    (
        "lib/arm64-v8a/liblwjgl_tinyfd.so",
        "natives/liblwjgl_tinyfd.so",
    ),
    ("lib/arm64-v8a/libopenal.so", "natives/libopenal.so"),
    ("lib/arm64-v8a/libfreetype.so", "natives/libfreetype.so"),
    (
        "lib/arm64-v8a/libjnidispatch.so",
        "natives/libjnidispatch.so",
    ),
];

/// 安装目录：runtime_dir()/lwjgl3/（classes.jar 与 natives/）。
pub fn install_dir() -> PathBuf {
    paths::runtime_dir().join("lwjgl3")
}

/// 合并类 jar 路径（启动时替换 org.lwjgl:* 类路径）。
pub fn classes_jar() -> PathBuf {
    install_dir().join("lwjgl-glfw-classes.jar")
}

/// 原生库目录（java.library.path / LD_LIBRARY_PATH 注入）。
pub fn natives_dir() -> PathBuf {
    install_dir().join("natives")
}

/// 是否已安装（类 jar + 清单内全部 so 逐一判定）。
/// 注意：libpojavexec.so 不在判定内 —— v0.1.5 起它由壳层显示桥在启动前
/// 落位覆盖（quartz-bridge Rust 重构版），不再来自 Pojav APK 提取。
/// 完整性逐项校验：老版本安装的包可能缺后加的条目（如 libjnidispatch.so，
/// 缺失会导致 JNA/oshi 全线失败），缺任一文件即判未安装 → 触发重新下载。
pub fn is_installed() -> bool {
    if !classes_jar().exists() {
        return false;
    }
    APK_FILES.iter().all(|(_, rel)| {
        if rel.starts_with("natives/") {
            natives_dir().join(&rel["natives/".len()..]).exists()
        } else {
            install_dir().join(rel).exists()
        }
    })
}

/// 下载 + 安装（幂等；重装先清空）。进度经 mpsc 上报。
pub async fn download_and_install(
    client: &reqwest::Client,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    let dir = install_dir();
    let staging = paths::runtime_dir().join(".lwjgl3.staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("natives"))?;

    // 存储空间前置检查：154MB APK 下载 + 提取副本（v8 加固）。
    crate::space::ensure_free(&dir, 400 * 1024 * 1024, "下载 LWJGL 安卓包")
        .map_err(crate::error::Error::Other)?;

    // ---- ① 下载官方 APK（一次 154MB，换取 jar 与 natives 版本严格配套；镜像回退） ----
    let part = paths::runtime_dir().join(".pojav.apk.part");
    let resp = crate::mirror::get_stream(client, POJAV_APK).await?;
    let total = resp.content_length().unwrap_or(0);
    let mut cur: u64 = 0;
    let mut stream = resp;
    {
        use tokio::io::AsyncWriteExt;
        let mut out = tokio::io::BufWriter::new(
            tokio::fs::File::create(&part)
                .await
                .map_err(|e| Error::Other(crate::error::io_context(&e, &part)))?,
        );
        let mut last = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
        while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
            out.write_all(&chunk)
                .await
                .map_err(|e| Error::Other(crate::error::io_context(&e, &part)))?;
            cur += chunk.len() as u64;
            let now = tokio::time::Instant::now();
            if now.duration_since(last) >= tokio::time::Duration::from_millis(120) {
                last = now;
                let percent = if total > 0 {
                    ((cur as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                let _ = progress.try_send(DownloadProgress {
                    stage: "lwjgl-download".into(),
                    current: cur,
                    total,
                    percent,
                    file: "PojavLauncher.apk（LWJGL 组件来源）".into(),
                    done: false,
                    error: None,
                });
            }
        }
        out.flush().await?;
    }
    if total > 0 && cur != total {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Other(format!(
            "LWJGL 组件源下载不完整（{cur}/{total} 字节），请重试"
        )));
    }

    // ---- ② 提取 jar + natives（阻塞线程池） ----
    let part2 = part.clone();
    let staging2 = staging.clone();
    let files2 = APK_FILES.len();
    tokio::task::spawn_blocking(move || extract_from_apk(&part2, &staging2))
        .await
        .map_err(|e| Error::Other(format!("提取线程崩溃：{e}")))??;
    let _ = std::fs::remove_file(&part);

    // ---- ②.5 合并类 jar 去 sha1（checkHash 降噪） ----
    // jar 内嵌的是上游 LWJGL glibc natives 的 .sha1；LWJGL 的 checkHash 会拿
    // java.library.path 里加载文件的 sha1 与之比对（gladiolus bionic 构建必然
    // 不同），每次启动打出两条「Incompatible Java and native library versions」
    // 噪音。去掉 .sha1 后 checkHash 因 classesURL == null 直接跳过，无副作用。

    // ---- ③ 落位（原子改名） ----
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&staging, &dir)?;
    if !is_installed() {
        return Err(Error::Other(
            "LWJGL pack 安装不完整（缺 classes.jar 或核心 so）".into(),
        ));
    }
    let _ = progress
        .send(DownloadProgress {
            stage: "done".into(),
            current: 1,
            total: 1,
            percent: 100.0,
            file: format!("LWJGL3-Android（{files2} 个文件，gladiolus）"),
            done: true,
            error: None,
        })
        .await;
    Ok(dir)
}

/// 从 Pojav APK 提取指定条目到 dest（保持清单相对路径）。
fn extract_from_apk(apk: &std::path::Path, dest: &std::path::Path) -> Result<usize> {
    let file = std::fs::File::open(apk)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut count = 0usize;
    for (entry_path, rel) in APK_FILES {
        let Ok(mut entry) = archive.by_name(entry_path) else {
            return Err(Error::Other(format!(
                "Pojav APK 内缺少条目：{entry_path}（来源更新？）"
            )));
        };
        let out_path = dest.join(rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out_file = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
        count += 1;
    }
    // 合并类 jar 重写：剔除 META-INF/**.sha1（checkHash 降噪，见 download_and_install）
    let jar = dest.join("lwjgl-glfw-classes.jar");
    match strip_sha1_entries(&jar) {
        Ok(n) => log_debug(&format!("classes.jar 去除 {n} 个 .sha1 条目")),
        Err(e) => {
            // 重写失败不阻断安装：checkHash 只是噪音，保留原 jar 可照常运行
            log_debug(&format!("classes.jar 重写失败（保留原样）：{e}"));
        }
    }
    Ok(count)
}

fn log_debug(msg: &str) {
    eprintln!("[quartz-lwjgl] {msg}");
}

/// 重写 jar：丢弃 META-INF/ 下以 .sha1 结尾的条目，其余逐条复刻压缩参数。
fn strip_sha1_entries(jar: &std::path::Path) -> Result<usize> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let tmp = jar.with_extension("jar.tmp");
    let out = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(out);
    let mut dropped = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.starts_with("META-INF/") && name.ends_with(".sha1") {
            dropped += 1;
            continue;
        }
        let mut opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));
        if entry.is_dir() {
            opts = opts.unix_permissions(0o755);
            writer.add_directory(name, opts)?;
            continue;
        }
        writer.start_file(name, opts)?;
        std::io::copy(&mut entry, &mut writer)?;
    }
    writer.finish()?;
    std::fs::rename(&tmp, jar)?;
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_layout_follows_runtime() {
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qz-lw-{}", std::process::id()));
        paths::set_runtime_jres_root(root.clone());
        assert_eq!(classes_jar(), root.join("lwjgl3/lwjgl-glfw-classes.jar"));
        assert_eq!(natives_dir(), root.join("lwjgl3/natives"));
        assert!(!is_installed());
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
    }

    #[test]
    fn manifest_requires_pojavexec() {
        // 关键回归：pojavexec 符号位必须有人提供 —— v0.1.5 起不再从 Pojav APK
        // 提取（旧 C 桥 RegisterNatives 版本错位致 255），改由 Rust 显示桥落位。
        // 此处断言：提取清单不再包含 pojavexec，但 jar 必须在（桥的接口契约来源）。
        assert!(APK_FILES
            .iter()
            .all(|(p, _)| !p.ends_with("libpojavexec.so")));
        assert!(APK_FILES
            .iter()
            .any(|(p, _)| p.ends_with("libjnidispatch.so")));
        assert!(APK_FILES
            .iter()
            .any(|(p, _)| p.ends_with("liblwjgl_stb.so")));
        assert!(APK_FILES
            .iter()
            .any(|(p, _)| p.ends_with("lwjgl-glfw-classes.jar")));
    }

    #[test]
    fn is_installed_requires_every_apk_file() {
        // 老版本安装的包可能缺后加条目（libjnidispatch.so 缺失 => JNA/oshi 全挂），
        // 完整性校验必须逐项判定。
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qz-lw-full-{}", std::process::id()));
        paths::set_runtime_jres_root(root.clone());
        let dir = install_dir();
        std::fs::create_dir_all(natives_dir()).unwrap();
        std::fs::write(classes_jar(), b"jar").unwrap();
        for (_, rel) in APK_FILES {
            if let Some(name) = rel.strip_prefix("natives/") {
                std::fs::write(natives_dir().join(name), b"so").unwrap();
            }
        }
        assert!(is_installed());
        // 拿掉一个 so 即判定未安装（触发重新下载）
        std::fs::remove_file(natives_dir().join("libjnidispatch.so")).unwrap();
        assert!(!is_installed());
        let _ = std::fs::remove_dir_all(&dir);
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
    }

    #[test]
    fn strip_sha1_drops_only_sha1_entries() {
        // checkHash 降噪：META-INF/**.sha1 必须被剔除，其余条目（含目录、普通
        // 文件、MANIFEST）原样保留。
        let root = std::env::temp_dir().join(format!("qz-lw-sha1-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let jar = root.join("lwjgl-glfw-classes.jar");
        {
            use std::io::Write;
            let file = std::fs::File::create(&jar).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            let opts: zip::write::SimpleFileOptions = Default::default();
            zw.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            zw.write_all(b"Manifest-Version: 1.0").unwrap();
            zw.start_file("META-INF/linux/arm64/org/lwjgl/liblwjgl.so.sha1", opts)
                .unwrap();
            zw.write_all(b"deadbeef").unwrap();
            zw.start_file("org/lwjgl/glfw/GLFW.class", opts).unwrap();
            zw.write_all(b"class-bytes").unwrap();
            zw.finish().unwrap();
        }
        let dropped = strip_sha1_entries(&jar).unwrap();
        assert_eq!(dropped, 1);
        let mut archive =
            zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(&jar).unwrap()))
                .unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"META-INF/MANIFEST.MF".to_string()));
        assert!(names.contains(&"org/lwjgl/glfw/GLFW.class".to_string()));
        assert!(names
            .iter()
            .all(|n| !(n.starts_with("META-INF/") && n.ends_with(".sha1"))));
        let _ = std::fs::remove_dir_all(&root);
    }
}
