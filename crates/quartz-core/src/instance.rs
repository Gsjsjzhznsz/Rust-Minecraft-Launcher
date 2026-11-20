//! 实例体系：隔离的游戏目录 + 共享的全局仓库（assets / libraries / runtimes）。
//!
//! 目录布局（全局根 root/）：
//! ```text
//! root/
//! ├── instances/<id>/{saves,mods,resourcepacks,shaderpacks,config,logs,crash-reports}
//! ├── assets/{indexes,objects}
//! ├── libraries/
//! ├── versions/<id>/
//! ├── runtimes/<os>/<arch>/<jvm-major>/
//! └── renderpacks/
//! ```
//! 游戏以 `--gameDir instances/<id>` 运行；类路径与资产目录指向全局仓库，
//! 避免多实例重复占盘。instances.json 与 root 元数据由本模块读写。

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoaderSpec {
    Fabric { loader_version: String },
    Quilt { loader_version: String },
    NeoForge { version: String },
    Forge { version: String },
    OptiFine { version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderSpec>,
    pub java_major: u32,
    /// 为空表示按全局 runtimes 目录解析。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
    pub memory_mb: u32,
    /// 渲染器 pack id（见 renderbus），空表示自动选择。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    pub created_at: String,
}

/// 新建实例的输入。
#[derive(Debug, Clone)]
pub struct CreateInstance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: Option<LoaderSpec>,
    pub java_major: u32,
    pub memory_mb: u32,
}

pub struct InstanceStore {
    /// 启动器全局根目录。
    root: PathBuf,
}

impl InstanceStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn index_file(&self) -> PathBuf {
        self.instances_dir().join("instances.json")
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.instances_dir().join(id)
    }

    pub fn assets_root(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn libraries_root(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn versions_root(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn runtimes_root(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn renderpacks_root(&self) -> PathBuf {
        self.root.join("renderpacks")
    }

    /// 读取实例列表；文件不存在视为空列表。
    pub fn load(&self) -> Result<Vec<Instance>> {
        let path = self.index_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn save(&self, instances: &[Instance]) -> Result<()> {
        std::fs::create_dir_all(self.instances_dir())?;
        let text = serde_json::to_string_pretty(instances)?;
        let tmp = self.index_file().with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.index_file())?;
        Ok(())
    }

    /// 创建实例目录并登记。目录不存在则创建；id 重复报错。
    pub fn create(&self, input: CreateInstance) -> Result<Instance> {
        let mut instances = self.load()?;
        if instances.iter().any(|i| i.id == input.id) {
            return Err(crate::error::Error::Other(format!(
                "instance id already exists: {}",
                input.id
            )));
        }
        let dir = self.instance_dir(&input.id);
        for sub in [
            "saves",
            "mods",
            "resourcepacks",
            "shaderpacks",
            "config",
            "logs",
            "crash-reports",
        ] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let inst = Instance {
            id: input.id,
            name: input.name,
            mc_version: input.mc_version,
            loader: input.loader,
            java_major: input.java_major,
            java_path: None,
            memory_mb: input.memory_mb,
            renderer: None,
            created_at: iso_now(),
        };
        instances.push(inst.clone());
        self.save(&instances)?;
        Ok(inst)
    }

    /// 删除实例目录与登记。
    pub fn delete(&self, id: &str) -> Result<bool> {
        let mut instances = self.load()?;
        let before = instances.len();
        instances.retain(|i| i.id != id);
        if instances.len() == before {
            return Ok(false);
        }
        self.save(&instances)?;
        let dir = self.instance_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(true)
    }
}

/// 返回 ISO8601/UTC 风格时间戳（秒精度）。
/// 不引入时间库：UNIX epoch + 手工格式化（YYYY-MM-DDTHH:MM:SSZ）。
pub fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days（Howard Hinnant 算法）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_load_delete_roundtrip() {
        let root = test_root("instance-roundtrip");
        let store = InstanceStore::new(&root);
        assert!(store.load().unwrap().is_empty());

        let inst = store
            .create(CreateInstance {
                id: "dev-1".into(),
                name: "开发服整合".into(),
                mc_version: "1.20.4".into(),
                loader: Some(LoaderSpec::Fabric {
                    loader_version: "0.15.11".into(),
                }),
                java_major: 17,
                memory_mb: 2048,
            })
            .unwrap();

        assert!(store.instance_dir("dev-1").join("saves").exists());
        assert!(store.instance_dir("dev-1").join("shaderpacks").exists());

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].mc_version, "1.20.4");

        // 重复 id 报错
        assert!(store
            .create(CreateInstance {
                id: "dev-1".into(),
                name: "dup".into(),
                mc_version: "1.20.4".into(),
                loader: None,
                java_major: 17,
                memory_mb: 2048,
            })
            .is_err());

        assert!(store.delete("dev-1").unwrap());
        assert!(!store.instance_dir("dev-1").exists());
        assert!(store.load().unwrap().is_empty());
        assert_eq!(inst.java_major, 17);
    }

    #[test]
    fn iso_now_shape() {
        let t = iso_now();
        assert_eq!(t.len(), 20);
        assert!(t.ends_with('Z'));
        assert_eq!(&t[4..5], "-");
    }

    fn test_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qz-inst-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
