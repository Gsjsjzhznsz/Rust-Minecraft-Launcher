//! Quartz CLI —— 用最短路径演示核心能力（全部命令可离线运行）。
//!
//! 常用示例：
//! ```text
//! qz versions --offline              # 解析内嵌 manifest 样例
//! qz plan 1.20.4 --player Steve      # 生成完整启动计划（JSON）
//! qz renderers --mc 1.20.4 --os android --vulkan
//! qz instances list --root ./.quartz-root
//! qz uuid Notch                      # 离线 UUID（Java nameUUIDFromBytes 语义）
//! qz mrpack demo.mrpack              # 解析整合包并列出客户端下载任务
//! qz uipack-check                    # ui-pack manifest 校验/灰度/任务计划（内置示例离线跑）
//! ```

use clap::{Parser, Subcommand};
use quartz_core::renderbus::{self, DeviceCaps};
use quartz_core::{host_arch, host_os, instance, modkit, RuntimeCtx};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "qz", version, about = "Quartz Launcher CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 版本清单（默认联网获取，--offline 解析内嵌样例）
    Versions {
        #[arg(long)]
        offline: bool,
    },
    /// 装配一次启动计划（当前版本使用内嵌 1.20.4 样例）
    Plan {
        version_id: String,
        #[arg(long, default_value = "Steve")]
        player: String,
        #[arg(long, default_value_t = 2048)]
        memory_mb: u32,
        #[arg(long, default_value = ".quartz-root")]
        root: PathBuf,
        #[arg(long)]
        window: Option<String>,
    },
    /// 设备渲染器选择演示
    Renderers {
        #[arg(long, default_value = "1.20.4")]
        mc: String,
        #[arg(long)]
        os: Option<String>,
        #[arg(long)]
        arch: Option<String>,
        #[arg(long)]
        vulkan: bool,
        #[arg(long)]
        gles3: bool,
        #[arg(long)]
        metal: bool,
        #[arg(long)]
        desktop_gl: bool,
    },
    #[command(subcommand)]
    Instances(InstancesCmd),
    /// Modrinth .mrpack 整合包解析
    Mrpack {
        file: PathBuf,
        #[arg(long, default_value = ".quartz-root/instances/demo")]
        instance_dir: PathBuf,
    },
    /// 离线账号 UUID
    Uuid { name: String },
    /// ui-pack manifest 校验 / 灰度判定 / 任务计划演示（docs/UI-PACK.md 协议 v0）
    UipackCheck {
        /// manifest 文件路径；省略时使用内置示例（离线演示）
        file: Option<PathBuf>,
        /// 灰度判定用的安装标识
        #[arg(long, default_value = "install-0001")]
        install_id: String,
    },
}

#[derive(Subcommand)]
enum InstancesCmd {
    List {
        #[arg(long, default_value = ".quartz-root")]
        root: PathBuf,
    },
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "1.20.4")]
        mc: String,
        #[arg(long, default_value_t = 2048)]
        memory_mb: u32,
        #[arg(long, default_value = ".quartz-root")]
        root: PathBuf,
    },
    Rm {
        id: String,
        #[arg(long, default_value = ".quartz-root")]
        root: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli.cmd) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cmd: Cmd) -> quartz_core::Result<()> {
    match cmd {
        Cmd::Versions { offline } => {
            let manifest = if offline {
                quartz_core::meta::parse_version_manifest(
                    quartz_core::fixtures::VERSION_MANIFEST_SAMPLE,
                )?
            } else {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| quartz_core::Error::Other(e.to_string()))?;
                let client = reqwest::Client::new();
                rt.block_on(quartz_core::meta::fetch_version_manifest(&client))?
            };
            println!(
                "latest release={} snapshot={}",
                manifest.latest.release, manifest.latest.snapshot
            );
            println!("versions: {}", manifest.versions.len());
            for v in manifest.versions.iter().take(10) {
                println!("  {:<16} {}", v.id, v.kind);
            }
        }

        Cmd::Plan {
            version_id: _,
            player,
            memory_mb,
            root,
            window,
        } => {
            let version: quartz_core::model::VersionJson =
                serde_json::from_str(quartz_core::fixtures::VERSION_1_20_4)
                    .map_err(quartz_core::Error::from)?;
            let hal = quartz_core::platform::default_hal();
            let os_name = hal.os().rule_name().to_string();
            let arch = host_arch().to_string();

            let mut ctx = RuntimeCtx::new(&os_name, &arch);
            if window.is_some() {
                ctx = ctx.with_feature("has_custom_resolution", true);
            }

            // 渲染路由演示
            let required = renderbus::required_profile(&version.id);
            let caps = DeviceCaps::for_os(hal.os().bus_name());
            let pack = renderbus::select(hal.os().bus_name(), &arch, required, &caps);
            match pack {
                Some(p) => println!(
                    "# renderer: {} ({} backend, {})",
                    p.id,
                    serde_json::to_string(&p.backend).unwrap(),
                    p.note
                ),
                None => println!("# renderer: none —— 设备无可用加速路径，按立项书红线应拒绝启动"),
            }

            let allowed: Vec<&quartz_core::model::Library> = version
                .libraries
                .iter()
                .filter(|l| quartz_core::meta::library_allowed(l, &ctx))
                .collect();

            let account = quartz_core::auth::offline_account(&player);
            let inst_dir = root.join("instances").join("demo");
            let assets_root = root.join("assets");
            let natives_root = root.join("natives");

            let plan = quartz_core::jvmops::build_launch_plan(&quartz_core::jvmops::PlanInputs {
                ctx: &ctx,
                version: &version,
                allowed_libs: &allowed,
                libraries_root: &root.join("libraries"),
                natives_root: &natives_root,
                assets_root: &assets_root,
                versions_root: &root.join("versions"),
                instance_dir: &inst_dir,
                account: &account,
                memory_mb,
                java_major: 17,
                java_path: None,
                window: None,
            })?;

            println!("{}", serde_json::to_string_pretty(&plan)?);
        }

        Cmd::Renderers {
            mc,
            os,
            arch,
            vulkan,
            gles3,
            metal,
            desktop_gl,
        } => {
            let os_name = os.unwrap_or_else(|| host_os().bus_name().to_string());
            let arch_name = arch.unwrap_or_else(|| host_arch().to_string());
            let required = renderbus::required_profile(&mc);
            let caps = DeviceCaps {
                vulkan,
                metal,
                gles3,
                desktop_gl_32: desktop_gl,
            };
            println!(
                "# required profile: {:?}",
                serde_json::to_string(&required).unwrap()
            );
            match renderbus::select(&os_name, &arch_name, required, &caps) {
                Some(p) => println!(
                    "{} {} {:?}",
                    p.id,
                    serde_json::to_string(&p.backend).unwrap(),
                    p.provides
                ),
                None => println!("none"),
            }
        }

        Cmd::Instances(sub) => match sub {
            InstancesCmd::List { root } => {
                let store = instance::InstanceStore::new(root);
                for inst in store.load()? {
                    println!(
                        "{:<16} mc={} mem={}M loader={}",
                        inst.id,
                        inst.mc_version,
                        inst.memory_mb,
                        inst.loader
                            .as_ref()
                            .map(|l| serde_json::to_string(l).unwrap())
                            .unwrap_or_else(|| "-".into())
                    );
                }
            }
            InstancesCmd::Add {
                id,
                name,
                mc,
                memory_mb,
                root,
            } => {
                let store = instance::InstanceStore::new(root);
                let inst = store.create(instance::CreateInstance {
                    id,
                    name,
                    mc_version: mc,
                    loader: None,
                    java_major: 17,
                    memory_mb,
                })?;
                println!(
                    "created {} at {}",
                    inst.id,
                    store.instance_dir(&inst.id).display()
                );
            }
            InstancesCmd::Rm { id, root } => {
                let store = instance::InstanceStore::new(root);
                if store.delete(&id)? {
                    println!("deleted {id}");
                } else {
                    println!("no such instance: {id}");
                }
            }
        },

        Cmd::Mrpack { file, instance_dir } => {
            let index = modkit::mrpack::read_index(&file)?;
            println!(
                "pack: {} v{} (game={}, deps={:?})",
                index.name, index.version_id, index.game, index.dependencies
            );
            let tasks = modkit::mrpack::client_download_tasks(&index, &instance_dir);
            println!("client tasks: {}", tasks.len());
            for t in &tasks {
                println!("  {:<40} {} bytes", t.label, t.size.unwrap_or(0));
            }
        }

        Cmd::Uuid { name } => {
            let acc = quartz_core::auth::offline_account(&name);
            println!("{} -> {}", acc.username, acc.uuid);
        }

        Cmd::UipackCheck { file, install_id } => {
            let manifest = match &file {
                Some(p) => quartz_core::uipack::parse(&std::fs::read_to_string(p)?)?,
                None => quartz_core::uipack::parse(quartz_core::uipack::SAMPLE_MANIFEST)?,
            };
            let shell_version = env!("CARGO_PKG_VERSION");
            quartz_core::uipack::validate(&manifest, shell_version)?;
            println!(
                "manifest ok: v{} channel={} schema={} files={} minShell={}",
                manifest.version,
                manifest.channel,
                manifest.schema,
                manifest.files.len(),
                manifest.min_shell_version
            );
            println!(
                "rollout: percent={} salt={:?} hit({})={}",
                manifest.rollout.percent,
                manifest.rollout.salt,
                install_id,
                quartz_core::uipack::rollout_hit(&manifest, &install_id)
            );
            match quartz_core::uipack::rollback_target(&manifest) {
                Some(prev) => {
                    println!("rollback target: v{} ({})", prev.version, prev.manifest)
                }
                None => println!("rollback target: none"),
            }
            let staging = std::path::Path::new(".quartz-root/ui-staging");
            let tasks = quartz_core::uipack::download_tasks(&manifest, staging);
            println!("staging tasks: {} (sha256 校验 + 断点跳过)", tasks.len());
            for t in &tasks {
                println!("  {} <- {}", t.dest.display(), t.source.source_url());
            }
        }
    }

    Ok(())
}
