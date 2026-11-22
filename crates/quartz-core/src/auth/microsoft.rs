//! 微软账号登录链路：设备码授权 -> MSA 令牌 -> Xbox Live -> XSTS ->
//! Minecraft Services 会话 -> 档案。
//!
//! 各端点与请求/响应结构按微软与 Mojang 公开行为建模；上线前需用项目自己
//! 的 Azure 应用 client_id 完成联调。离线单测只覆盖结构解析。

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

pub const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const XBL_USER_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// 必须替换为项目自有 Azure Public Client ID（留空则拒绝登录）。
pub const DEFAULT_CLIENT_ID: &str = "";

// ---------------------------------------------------------------------------
// 设备码授权
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResp {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub message: Option<String>,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsaTokenResp {
    pub token_type: String,
    pub expires_in: u64,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

pub enum TokenOutcome {
    Success(MsaTokenResp),
    /// 尚未授权，按 interval 重试。
    Pending {
        interval: u64,
    },
    /// 明确失败（authorization_expired / authorization_denied 等）。
    Failed {
        error: String,
    },
}

pub async fn device_code_request(
    client: &reqwest::Client,
    client_id: &str,
) -> Result<DeviceCodeResp> {
    let resp = client
        .post(DEVICE_CODE_URL)
        .form(&[
            ("client_id", client_id),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<DeviceCodeResp>()
        .await?;
    Ok(resp)
}

pub async fn poll_token(
    client: &reqwest::Client,
    client_id: &str,
    device_code: &str,
) -> Result<TokenOutcome> {
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;

    let status = resp.status();
    let body: MsaTokenResp = resp.json().await?;

    if status.is_success() {
        return Ok(TokenOutcome::Success(body));
    }
    let err = body.error.clone().unwrap_or_else(|| status.to_string());
    if err == "authorization_pending" || err == "slow_down" {
        return Ok(TokenOutcome::Pending {
            interval: default_interval(),
        });
    }
    Ok(TokenOutcome::Failed { error: err })
}

// ---------------------------------------------------------------------------
// XBL / XSTS / Minecraft 会话
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XboxAuthResp {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: DisplayClaims,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayClaims {
    #[serde(rename = "xui", default)]
    pub xui: Vec<XuiClaim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XuiClaim {
    #[serde(rename = "uhs")]
    pub uhs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McLoginResp {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McProfile {
    pub id: String,
    pub name: String,
}

/// XBL 用户令牌（RPS ticket 前缀 "d="，消费者账号）。
pub async fn xbl_user_token(
    client: &reqwest::Client,
    msa_access_token: &str,
) -> Result<XboxAuthResp> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={msa_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    post_json(client, XBL_USER_AUTH_URL, &body).await
}

/// XSTS 令牌（依赖 XBL 用户令牌）。
pub async fn xsts_authorize(client: &reqwest::Client, xbl_token: &str) -> Result<XboxAuthResp> {
    let body = serde_json::json!({
        "Properties": {
            "UserTokens": [xbl_token],
            "SandboxId": "RETAIL"
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    post_json(client, XSTS_AUTH_URL, &body).await
}

/// 用 XBL3.0 令牌换 Minecraft 会话。
pub async fn mc_login(
    client: &reqwest::Client,
    uhs: &str,
    xsts_token: &str,
) -> Result<McLoginResp> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
    });
    post_json(client, MC_LOGIN_URL, &body).await
}

pub async fn profile(client: &reqwest::Client, mc_access_token: &str) -> Result<McProfile> {
    let resp = client
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_access_token)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<McProfile>().await?)
}

async fn post_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<T> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(body)
        .send()
        .await?;
    let resp = resp.error_for_status()?;
    Ok(resp.json::<T>().await?)
}

/// 完整设备码登录流程：返回可直接用于启动的账号。
pub async fn device_code_login(
    client: &reqwest::Client,
    client_id: &str,
    mut on_prompt: impl FnMut(&DeviceCodeResp) + Send,
) -> Result<crate::Account> {
    if client_id.is_empty() {
        return Err(Error::MissingConfig(
            "client_id 为空：请在 Azure 注册项目的 Public Client 并配置 DEFAULT_CLIENT_ID".into(),
        ));
    }
    let dc = device_code_request(client, client_id).await?;
    on_prompt(&dc);

    let msa = loop {
        match poll_token(client, client_id, &dc.device_code).await? {
            TokenOutcome::Success(m) => break m,
            TokenOutcome::Pending { interval } => {
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
            TokenOutcome::Failed { error } => {
                return Err(Error::Other(format!("MSA 授权失败: {error}")));
            }
        }
    };

    let xbl = xbl_user_token(client, &msa.access_token).await?;
    let xsts = xsts_authorize(client, &xbl.token).await?;
    let uhs = xsts
        .display_claims
        .xui
        .first()
        .map(|c| c.uhs.clone())
        .ok_or_else(|| Error::Other("XSTS 响应缺少 uhs".into()))?;
    let mc = mc_login(client, &uhs, &xsts.token).await?;
    let prof = profile(client, &mc.access_token).await?;

    Ok(crate::Account {
        kind: crate::auth::AccountKind::Microsoft,
        username: prof.name,
        // Mojang 返回不带连字符的 UUID，补齐标准格式。
        uuid: dashed_uuid(&prof.id),
        access_token: mc.access_token,
        refresh_token: msa.refresh_token.clone(),
        user_type: Some("msa".to_string()),
    })
}

/// "aa11bb22..." -> "aaaaaaaa-bbbb-..."（8-4-4-4-12）。
pub fn dashed_uuid(no_dash: &str) -> String {
    let s: String = no_dash.chars().filter(|c| *c != '-').collect();
    if s.len() != 32 {
        return s;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &s[0..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_code_resp() {
        let json = r#"{
            "device_code": "DAQABAAEAAADSwE",
            "user_code": "ABCD1234E",
            "verification_uri": "https://www.microsoft.com/link",
            "expires_in": 900,
            "interval": 5,
            "message": "To sign in, use a web browser to open the page ..."
        }"#;
        let dc: DeviceCodeResp = serde_json::from_str(json).unwrap();
        assert_eq!(dc.interval, 5);
        assert_eq!(dc.verification_uri, "https://www.microsoft.com/link");
    }

    #[test]
    fn parses_xbox_resp() {
        let json = r#"{
            "IssueInstant": "2020-12-07T19:52:08.4464055Z",
            "NotAfter": "2020-12-21T19:52:08.4464055Z",
            "Token": "eyJhbGciOiJIUzI1NiIs",
            "DisplayClaims": { "xui": [ { "uhs": "1234567890" } ] }
        }"#;
        let resp: XboxAuthResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.token, "eyJhbGciOiJIUzI1NiIs");
        assert_eq!(resp.display_claims.xui[0].uhs, "1234567890");
    }

    #[test]
    fn dashed_uuid_formatting() {
        assert_eq!(
            dashed_uuid("854c63d102bc4d4c9c0cafa0e141ac6d"),
            "854c63d1-02bc-4d4c-9c0c-afa0e141ac6d"
        );
    }

    #[test]
    fn parses_mc_profile() {
        let json = r#"{ "id": "854c63d102bc4d4c9c0cafa0e141ac6d", "name": "Steve" }"#;
        let p: McProfile = serde_json::from_str(json).unwrap();
        assert_eq!(p.name, "Steve");
    }
}
