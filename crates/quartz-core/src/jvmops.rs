//! JVM 编排：把「实例 + 版本 JSON + 过滤后的库 + 账号」装配成一次可执行的启动计划。
//!
//! 产物 `LaunchPlan` 是 UI / FFI / 各平台 HAL 的统一契约：
//! - 桌面端：HAL 用子进程执行（java_path + jvm_args + classpath + main_class + game_args）；
//! - 移动端（Android/iOS/鸿蒙）：JVM 进程内启动，jvm_args/classpath/game_args
//!   原样交给宿主壳层的 `JNI_CreateJavaVM` 封装层。
//!
//! 本模块是纯函数装配，不做 IO；测试全部离线可跑。

use crate::auth::Account;
use crate::error::Result;
use crate::meta;
use crate::model::{ArgEntry, ArgValue, Library, VersionJson};
use crate::rules::RuntimeCtx;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 平台 natives classifier 的候选顺序（先架构特定，再通用）。
pub fn natives_classifier_candidates(ctx: &RuntimeCtx) -> Vec<String> {
    match ctx.os.as_str() {
        "windows" => {
            if ctx.arch == "arm64" {
                vec!["natives-windows-arm64".into(), "natives-windows".into()]
            } else {
                vec!["natives-windows".into()]
            }
        }
        "osx" => {
            if ctx.arch == "arm64" {
                vec!["natives-macos-arm64".into(), "natives-macos".into()]
            } else {
                vec!["natives-macos".into()]
            }
        }
        "linux" => {
            if ctx.arch == "arm64" {
                vec!["natives-linux-arm64".into(), "natives-linux".into()]
            } else {
                vec!["natives-linux".into()]
            }
        }
        // Android/iOS/鸿蒙：natives 由移植版 LWJGL 的 so/dylib 提供，
        // 不走 Mojang natives 通道。
        _ => Vec::new(),
    }
}

/// 从库列表挑出当前上下文的 natives artifact。
pub fn select_native_artifacts<'a>(
    libs: &[&'a Library],
    ctx: &RuntimeCtx,
) -> Vec<(&'a Library, crate::model::Artifact)> {
    let candidates = natives_classifier_candidates(ctx);
    let mut out = Vec::new();
    for lib in libs {
        let Some(dl) = &lib.downloads else { continue };
        let Some(classifiers) = &dl.classifiers else {
            continue;
        };
        if let Some(c) = candidates.iter().find(|c| classifiers.contains_key(*c)) {
            if let Some(a) = classifiers.get(c) {
                out.push((*lib, a.clone()));
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    /// 桌面端子进程模式使用；移动端为 None（进程内 JVM）。
    pub java_path: Option<PathBuf>,
    pub jvm_args: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub natives_dir: PathBuf,
    pub working_dir: PathBuf,
    pub memory_mb: u32,
    pub java_major: u32,
    pub assets_index_name: Option<String>,
}

/// 装配输入。
pub struct PlanInputs<'a> {
    pub ctx: &'a RuntimeCtx,
    pub version: &'a VersionJson,
    /// 已通过 rules 过滤的库（meta::collect_library_tasks 使用同一批）。
    pub allowed_libs: &'a [&'a Library],
    pub libraries_root: &'a Path,
    pub natives_root: &'a Path,
    pub assets_root: &'a Path,
    pub versions_root: &'a Path,
    pub instance_dir: &'a Path,
    pub account: &'a Account,
    pub memory_mb: u32,
    pub java_major: u32,
    pub java_path: Option<PathBuf>,
    pub window: Option<(u32, u32)>,
}

pub fn build_launch_plan(input: &PlanInputs) -> Result<LaunchPlan> {
    let version = input.version;

    // ---- classpath：过滤后的库 jar + 客户端 jar（顺序重要） ----
    let mut classpath: Vec<PathBuf> = Vec::new();
    for lib in input.allowed_libs {
        if let Some(rel) = meta::library_jar_path(lib) {
            classpath.push(input.libraries_root.join(rel));
        }
    }
    let client_jar = input
        .versions_root
        .join(&version.id)
        .join(format!("{}.jar", version.id));
    classpath.push(client_jar);

    // ---- natives 目录：每个 natives jar 独立子目录，避免 so 冲突 ----
    let natives = select_native_artifacts(input.allowed_libs, input.ctx);
    let natives_dir = input.natives_root.join(&version.id);
    let _ = natives; // 提取动作由调用方/HAL 在下载后执行（见 extract_natives）

    // ---- 变量表 ----
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    vars.insert("auth_player_name".into(), input.account.username.clone());
    vars.insert(
        "auth_access_token".into(),
        input.account.access_token.clone(),
    );
    vars.insert("auth_uuid".into(), input.account.uuid.clone());
    vars.insert(
        "auth_session".into(),
        format!(
            "token:{}:{}",
            input.account.access_token, input.account.uuid
        ),
    );
    vars.insert(
        "user_type".into(),
        input
            .account
            .user_type
            .clone()
            .unwrap_or_else(|| "msa".into()),
    );
    vars.insert("version_name".into(), version.id.clone());
    vars.insert("version_type".into(), "Quartz".into());
    vars.insert(
        "game_directory".into(),
        input.instance_dir.display().to_string(),
    );
    vars.insert(
        "assets_root".into(),
        input.assets_root.display().to_string(),
    );
    let index_name = meta::assets_index_name(version).unwrap_or_else(|| "legacy".into());
    vars.insert("assets_index_name".into(), index_name.clone());
    vars.insert(
        "natives_directory".into(),
        natives_dir.display().to_string(),
    );
    vars.insert("launcher_name".into(), "Quartz".into());
    vars.insert("launcher_version".into(), env!("CARGO_PKG_VERSION").into());
    vars.insert("memory_mb".into(), input.memory_mb.to_string());
    if let Some((w, h)) = input.window {
        vars.insert("resolution_width".into(), w.to_string());
        vars.insert("resolution_height".into(), h.to_string());
    }
    vars.insert(
        "classpath".into(),
        std::env::join_paths(&classpath)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );

    // ---- JVM 参数 ----
    // 现代版本（1.13+）以版本 JSON 的 arguments.jvm 为准，启动器只补缺；
    // 旧版本（无 arguments.jvm）使用缺省集。保证 -Xmx / -cp 各出现一次。
    let mut jvm_args: Vec<String> = Vec::new();
    match &version.arguments {
        Some(args) if !args.jvm.is_empty() => {
            jvm_args.extend(expand_entries(&args.jvm, input.ctx, &vars));
        }
        _ => {
            jvm_args.push(format!("-Djava.library.path={}", natives_dir.display()));
            jvm_args.push("-Djna.tmpdir=${natives_directory}".to_string());
        }
    }
    if !jvm_args.iter().any(|s| s.starts_with("-Xmx")) {
        jvm_args.insert(0, format!("-Xmx{}M", input.memory_mb));
    }
    if !jvm_args.iter().any(|s| s == "-cp" || s.starts_with("-cp=")) {
        jvm_args.push("-cp".into());
        jvm_args.push(vars["classpath"].clone());
    }

    // ---- 游戏参数 ----
    let mut game_args: Vec<String> = Vec::new();
    if let Some(args) = &version.arguments {
        game_args.extend(expand_entries(&args.game, input.ctx, &vars));
    } else if let Some(legacy) = &version.minecraft_arguments {
        game_args.extend(legacy.split_whitespace().map(|s| expand_str(s, &vars)));
    }

    Ok(LaunchPlan {
        java_path: input.java_path.clone(),
        jvm_args,
        classpath,
        main_class: version.main_class.clone(),
        game_args,
        natives_dir,
        working_dir: input.instance_dir.to_path_buf(),
        memory_mb: input.memory_mb,
        java_major: input.java_major,
        assets_index_name: Some(index_name),
    })
}

/// 展开 arguments 数组：先按 rules 过滤，再做 ${var} 替换。
pub fn expand_entries(
    entries: &[ArgEntry],
    ctx: &RuntimeCtx,
    vars: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for e in entries {
        let keep = match e {
            ArgEntry::Str(_) => true,
            ArgEntry::Ruled { rules, .. } => crate::rules::evaluate(rules, ctx),
        };
        if !keep {
            continue;
        }
        match e {
            ArgEntry::Str(s) => out.push(expand_str(s, vars)),
            ArgEntry::Ruled { value, .. } => match value {
                ArgValue::One(s) => out.push(expand_str(s, vars)),
                ArgValue::Many(list) => {
                    out.extend(list.iter().map(|s| expand_str(s, vars)));
                }
            },
        }
    }
    out
}

/// `${key}` 占位符替换；未定义的占位符替换为空串（与官方启动器行为一致）。
pub fn expand_str(s: &str, vars: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                result.push_str(vars.get(key).map(String::as_str).unwrap_or(""));
                rest = &after[end + 1..];
            }
            None => {
                result.push_str("${");
                rest = after;
            }
        }
    }
    result.push_str(rest);
    result
}

/// 从 natives jar 提取动态库（跳过 META-INF 与目录项）。
pub fn extract_natives(jar: &Path, dest: &Path) -> Result<usize> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    std::fs::create_dir_all(dest)?;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.starts_with("META-INF/") || name_str.ends_with('/') {
            continue;
        }
        let out_path = dest.join(&name);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::offline_account;
    use crate::fixtures;
    use crate::model::maven_rel_path;
    use std::io::Write;

    fn ctx() -> RuntimeCtx {
        RuntimeCtx::new("osx", "arm64")
    }

    fn version() -> VersionJson {
        serde_json::from_str(fixtures::VERSION_1_20_4).unwrap()
    }

    fn allowed_libs<'a>(v: &'a VersionJson, c: &RuntimeCtx) -> Vec<&'a Library> {
        v.libraries
            .iter()
            .filter(|l| crate::rules::evaluate(&l.rules, c))
            .collect()
    }

    #[test]
    fn plan_contains_classpath_and_substituted_args() {
        let root = std::env::temp_dir().join("qz-jvmops");
        let v = version();
        let libs = allowed_libs(&v, &ctx());
        let account = offline_account("Steve");
        let inst = root.join("instances/dev");

        let plan = build_launch_plan(&PlanInputs {
            ctx: &ctx(),
            version: &v,
            allowed_libs: &libs,
            libraries_root: &root.join("libraries"),
            natives_root: &root.join("natives"),
            assets_root: &root.join("assets"),
            versions_root: &root.join("versions"),
            instance_dir: &inst,
            account: &account,
            memory_mb: 2048,
            java_major: 17,
            java_path: None,
            window: None,
        })
        .unwrap();

        // JVM 参数包含内存与 natives 路径
        assert!(plan.jvm_args.iter().any(|s| s == "-Xmx2048M"));
        assert!(plan
            .jvm_args
            .iter()
            .any(|s| s.contains("Djava.library.path=")));

        // classpath：过滤后的库 + 客户端 jar，顺序正确
        let brigadier = root
            .join("libraries")
            .join(maven_rel_path("com.mojang:brigadier:1.1.8", None).unwrap());
        assert!(plan.classpath.contains(&brigadier));
        assert_eq!(
            plan.classpath.last().unwrap(),
            &root.join("versions/1.20.4/1.20.4.jar")
        );

        // 游戏参数已完成变量替换
        let joined = plan.game_args.join(" ");
        assert!(joined.contains("--username Steve"));
        assert!(joined.contains("--gameDir"));
        assert!(joined.contains(inst.display().to_string().as_str()));
        assert!(joined.contains("--accessToken 0"));
        // 未开启的特性规则被过滤
        assert!(!joined.contains("--demo"));
        assert!(!joined.contains("--width"));

        // main class 与 main class 前不混淆
        assert_eq!(plan.main_class, "net.minecraft.client.main.Main");
        assert_eq!(plan.java_major, 17);
    }

    #[test]
    fn natives_selection_by_os_and_arch() {
        let v = version();
        let libs = allowed_libs(&v, &ctx());
        let natives = select_native_artifacts(&libs, &ctx());
        assert!(!natives.is_empty());
        // lwjgl 的 macos natives 被选中（fixture 中 natives-macos 存在）
        assert!(natives
            .iter()
            .any(|(l, _)| l.name.starts_with("org.lwjgl:lwjgl:")));

        // windows arm64 优先 natives-windows-arm64（fixture 未提供 -> 空或回退）
        let wctx = RuntimeCtx::new("windows", "arm64");
        let wlibs = allowed_libs(&v, &wctx);
        let wnatives = select_native_artifacts(&wlibs, &wctx);
        if let Some((_, art)) = wnatives
            .iter()
            .find(|(l, _)| l.name.starts_with("org.lwjgl:lwjgl:"))
        {
            assert!(art.path.contains("natives-windows"));
        }
    }

    #[test]
    fn expand_str_replaces_known_and_blanks_unknown() {
        let mut vars = BTreeMap::new();
        vars.insert("a".to_string(), "1".to_string());
        assert_eq!(expand_str("${a}-${b}", &vars), "1-");
        assert_eq!(expand_str("no vars", &vars), "no vars");
        assert_eq!(expand_str("unclosed ${a", &vars), "unclosed ${a");
    }

    #[test]
    fn extract_natives_skips_meta_inf() {
        let root = std::env::temp_dir().join(format!("qz-nat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let jar = root.join("natives.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zw.start_file("META-INF/MANIFEST.MF", opts).unwrap();
        zw.write_all(b"Manifest-Version: 1.0").unwrap();
        zw.start_file("libtest.dylib", opts).unwrap();
        zw.write_all(b"binary").unwrap();
        zw.finish().unwrap();

        let dest = root.join("out");
        let n = extract_natives(&jar, &dest).unwrap();
        assert_eq!(n, 1);
        assert!(dest.join("libtest.dylib").exists());
        assert!(!dest.join("META-INF").exists());
    }
}
