//! authlib-injector 外置登录（对标 FCL）：
//! - Yggdrasil 协议 `POST /authserver/authenticate` 完成密码登录；
//! - 下载 authlib-injector agent jar（GitHub Releases，含镜像回退）；
//! - 启动时由 jvmops 注入 `-javaagent:<jar>=<server_url>`，
//!   游戏会话请求被 agent 拦截改写到外置服务器。
//!
//! 协议参考：authlib-injector / yggdrasil 官方文档（authserver API v1）。

use super::{Account, AccountKind};
use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// authlib-injector 官方仓库最新 release（GitHub API）。
pub const AUTHLIB_INJECTOR_LATEST: &str =
    "https://api.github.com/repos/yggdrasil/authlib-injector/releases/latest";

#[derive(Debug, Deserialize)]
pub struct InjectorRelease {
    #[serde(rename = "tagName", alias = "tag_name")]
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<InjectorAsset>,
}

#[derive(Debug, Deserialize)]
pub struct InjectorAsset {
    pub name: String,
    #[serde(rename = "browserDownloadUrl", alias = "browser_download_url")]
    pub browser_download_url: String,
}

/// agent jar 落位（libraries/authlib-injector/authlib-injector.jar，固定名便于 jvmops 引用）。
pub fn injector_jar() -> PathBuf {
    paths::libraries_dir().join("authlib-injector").join("authlib-injector.jar")
}

fn injector_meta() -> PathBuf {
    paths::libraries_dir().join("authlib-injector").join("meta.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectorMeta {
    pub version: String,
    pub build_time: Option<String>,
}

pub fn installed_version() -> Option<String> {
    if !injector_jar().exists() {
        return None;
    }
    let text = std::fs::read_to_string(injector_meta()).ok()?;
    serde_json::from_str::<InjectorMeta>(&text).ok().map(|m| m.version)
}

/// 拉取最新 release 元数据。
pub async fn latest_release(client: &reqwest::Client) -> Result<InjectorRelease> {
    let rel = client
        .get(AUTHLIB_INJECTOR_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json::<InjectorRelease>()
        .await?;
    Ok(rel)
}

/// 下载并落位 agent jar（带 .qz-part 原子改名与镜像回退）。
/// 返回安装的版本号。
pub async fn download_injector(client: &reqwest::Client) -> Result<String> {
    let rel = latest_release(client).await?;
    let asset = rel
        .assets
        .iter()
        .find(|a| a.name.starts_with("authlib-injector-") && a.name.ends_with(".jar"))
        .ok_or_else(|| Error::Other("release 资产中未找到 authlib-injector jar".into()))?;

    let url = asset.browser_download_url.clone();
    let mut resp = crate::mirror::get_stream(client, &url).await?;
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = crate::mirror::next_chunk(&mut resp).await? {
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Err(Error::Other(format!("下载内容为空：{url}")));
    }

    let dir = injector_jar().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let part = dir.join("authlib-injector.jar.qz-part");
    std::fs::write(&part, &data)?;
    std::fs::rename(&part, injector_jar())?;
    serde_json::to_writer_pretty(
        std::fs::File::create(injector_meta())?,
        &InjectorMeta {
            version: rel.tag_name.clone(),
            build_time: None,
        },
    )?;
    Ok(rel.tag_name)
}

// ---------------------------------------------------------------------------
// Yggdrasil authenticate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct YggProfile {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct YggAuthResp {
    #[serde(default, rename = "accessToken", alias = "access_token")]
    pub access_token: String,
    #[serde(default, rename = "clientToken", alias = "client_token")]
    pub client_token: String,
    #[serde(default, rename = "availableProfiles", alias = "available_profiles")]
    pub available_profiles: Vec<YggProfile>,
    #[serde(default, rename = "selectedProfile", alias = "selected_profile")]
    pub selected_profile: Option<YggProfile>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "errorMessage", alias = "error_message")]
    pub error_message: Option<String>,
}

/// 规范化服务器地址（去尾部斜杠；必须 http/https）。
pub fn normalize_server(url: &str) -> Result<String> {
    let u = url.trim().trim_end_matches('/');
    if !u.starts_with("http://") && !u.starts_with("https://") {
        return Err(Error::Other(format!(
            "认证服务器地址需以 http(s):// 开头：{url}"
        )));
    }
    Ok(u.to_string())
}

/// Yggdrasil 密码登录 -> 账号（写入 accounts.json 并选中）。
pub async fn login(
    client: &reqwest::Client,
    server_url: &str,
    username: &str,
    password: &str,
) -> Result<Account> {
    let server = normalize_server(server_url)?;
    let client_token = client_token();
    let body = serde_json::json!({
        "agent": { "name": "MinecraftClient", "version": 1 },
        "username": username.trim(),
        "password": password,
        "clientToken": client_token,
    });
    let url = format!("{server}/authserver/authenticate");
    let resp = client.post(&url).json(&body).send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    let parsed: YggAuthResp = serde_json::from_str(&text).map_err(|e| {
        Error::Other(format!(
            "认证服务器响应异常（HTTP {status}）：{e}；{:.200}",
            text
        ))
    })?;
    if !status.is_success() {
        let msg = parsed
            .error_message
            .unwrap_or_else(|| parsed.error.unwrap_or_else(|| status.to_string()));
        return Err(Error::Other(format!("登录失败：{msg}")));
    }
    if parsed.access_token.is_empty() {
        return Err(Error::Other("登录失败：服务器未返回 accessToken".into()));
    }

    let profile = if let Some(p) = parsed.selected_profile {
        p
    } else if parsed.available_profiles.len() == 1 {
        parsed.available_profiles[0].clone()
    } else if parsed.available_profiles.is_empty() {
        return Err(Error::Other(
            "该账号在认证服务器上没有可用角色档案".into(),
        ));
    } else {
        let names: Vec<String> = parsed
            .available_profiles
            .iter()
            .map(|p| p.name.clone())
            .collect();
        return Err(Error::Other(format!(
            "账号有多个角色档案，请选择其一后重试：{}",
            names.join("、")
        )));
    };

    Ok(Account {
        kind: AccountKind::Authlib,
        username: profile.name,
        uuid: super::microsoft::dashed_uuid(&profile.id),
        access_token: parsed.access_token,
        refresh_token: None,
        user_type: Some("msa".into()),
        authlib_server: Some(server),
    })
}

/// clientToken：无需强随机，一次安装稳定即可（服务器用于识别客户端）。
fn client_token() -> String {
    use md5::{Digest, Md5};
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = Md5::new();
    h.update(format!("quartz-{nanos}"));
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_json() {
        let json = r#"{
            "tagName": "v1.2.5",
            "assets": [
                { "name": "authlib-injector-1.2.5.jar",
                  "browserDownloadUrl": "https://github.com/yggdrasil/authlib-injector/releases/download/v1.2.5/authlib-injector-1.2.5.jar" }
            ]
        }"#;
        let rel: InjectorRelease = serde_json::from_str(json).unwrap();
        assert_eq!(rel.tag_name, "v1.2.5");
        assert!(rel.assets[0].name.starts_with("authlib-injector-"));
    }

    #[test]
    fn parses_ygg_auth_resp_snake_and_camel() {
        // 部分服务器用 availableProfiles（驼峰），实现按驼峰为主、蛇形别名兼容。
        let camel = r#"{
            "accessToken": "tok",
            "clientToken": "ct",
            "availableProfiles": [{ "id": "854c63d102bc4d4c9c0cafa0e141ac6d", "name": "Steve" }],
            "selectedProfile": { "id": "854c63d102bc4d4c9c0cafa0e141ac6d", "name": "Steve" }
        }"#;
        let r: YggAuthResp = serde_json::from_str(camel).unwrap();
        assert_eq!(r.access_token, "tok");
        assert_eq!(r.selected_profile.as_ref().unwrap().name, "Steve");
    }

    #[test]
    fn normalize_server_rejects_bad() {
        assert!(normalize_server("example.com").is_err());
        assert_eq!(
            normalize_server("https://littleskin.cn/api/yggdrasil/").unwrap(),
            "https://littleskin.cn/api/yggdrasil"
        );
    }
}
