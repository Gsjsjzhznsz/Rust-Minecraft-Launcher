//! Modrinth 整合包（.mrpack）解析：索引 JSON -> 下载任务（按 client 环境过滤）。
//!
//! .mrpack 本质是 zip，根目录含 `modrinth.index.json`。files[].downloads
//! 是镜像 URL 列表，取第一个作为下载源；env.client = "unsupported" 的文件
//! 在客户端实例中跳过。

use crate::download::{DownloadTask, HashSpec, Source};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    pub hashes: MrpackHashes,
    pub env: MrpackEnv,
    pub downloads: Vec<String>,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrpackHashes {
    pub sha1: String,
    pub sha512: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvSupport {
    Required,
    Optional,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackEnv {
    #[serde(default)]
    pub client: Option<EnvSupport>,
    #[serde(default)]
    pub server: Option<EnvSupport>,
}

/// 从 mrpack 文件读取索引。
pub fn read_index(mrpack_path: &Path) -> Result<MrpackIndex> {
    let file = std::fs::File::open(mrpack_path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut entry = zip.by_name("modrinth.index.json")?;
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text)?)
}

/// 生成客户端侧下载任务（过滤 client=unsupported）。
pub fn client_download_tasks(index: &MrpackIndex, instance_dir: &Path) -> Vec<DownloadTask> {
    index
        .files
        .iter()
        .filter(|f| f.env.client != Some(EnvSupport::Unsupported))
        .map(|f| DownloadTask {
            source: Source::Url(f.downloads.first().cloned().unwrap_or_default()),
            dest: instance_dir.join(&f.path),
            hash: Some(HashSpec::sha1(f.hashes.sha1.clone())),
            size: Some(f.file_size),
            label: format!("modpack:{}", f.path),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_index_json() -> String {
        serde_json::json!({
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "1.0.0",
            "name": "Demo Pack",
            "files": [
                {
                    "path": "mods/sodium.jar",
                    "hashes": {
                        "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "sha512": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    },
                    "env": { "client": "required", "server": "unsupported" },
                    "downloads": ["https://cdn.modrinth.com/data/AANobbMI/versions/xxx/sodium.jar"],
                    "fileSize": 1024
                },
                {
                    "path": "mods/serveronly.jar",
                    "hashes": {
                        "sha1": "cccccccccccccccccccccccccccccccccccccccc",
                        "sha512": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                    },
                    "env": { "client": "unsupported", "server": "required" },
                    "downloads": ["https://cdn.modrinth.com/data/xxx/versions/yyy/serveronly.jar"],
                    "fileSize": 512
                }
            ],
            "dependencies": { "minecraft": "1.20.4", "fabric-loader": "0.15.11" }
        })
        .to_string()
    }

    #[test]
    fn parse_and_filter_client_tasks() {
        let json = sample_index_json();
        let idx: MrpackIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(idx.name, "Demo Pack");
        assert_eq!(idx.dependencies.get("minecraft").unwrap(), "1.20.4");

        let tasks = client_download_tasks(&idx, Path::new("/instances/demo"));
        // client=unsupported 的 serveronly.jar 被过滤
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].dest, Path::new("/instances/demo/mods/sodium.jar"));
        assert_eq!(
            tasks[0].source.source_url(),
            "https://cdn.modrinth.com/data/AANobbMI/versions/xxx/sodium.jar"
        );
    }

    #[test]
    fn reads_index_from_zip_fixture() {
        let root = std::env::temp_dir().join(format!("qz-mrpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pack = root.join("demo.mrpack");

        let file = std::fs::File::create(&pack).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zw.start_file("modrinth.index.json", opts).unwrap();
        zw.write_all(sample_index_json().as_bytes()).unwrap();
        zw.finish().unwrap();

        let idx = read_index(&pack).unwrap();
        assert_eq!(idx.version_id, "1.0.0");
        assert_eq!(client_download_tasks(&idx, Path::new("/x")).len(), 1);
    }
}
