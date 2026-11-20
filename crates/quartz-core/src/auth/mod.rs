//! 账号体系：离线账号（含离线 UUID 算法）与微软账号（设备码 + XBL 链路）。
//!
//! 合规边界：
//! - 不缓存明文长期令牌到公共目录；生产实现应接入 OS keychain（iOS Keychain /
//!   Android Keystore / macOS Keychain）。
//! - `microsoft::DEFAULT_CLIENT_ID` 必须使用项目自己在 Azure 注册的
//!   Public Client ID，禁止硬编码第三方应用的 client_id。

pub mod microsoft;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub kind: AccountKind,
    pub username: String,
    /// 带连字符的标准 UUID 字符串。
    pub uuid: String,
    /// 会话令牌；离线账号固定为 "0"（与社区惯例一致）。
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_type: Option<String>,
}

/// Java `UUID.nameUUIDFromBytes` 的等价实现：MD5 -> RFC4122 v3。
/// Minecraft 离线 UUID = nameUUIDFromBytes("OfflinePlayer:<name>")。
pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{name}").as_bytes());
    let digest = hasher.finalize();
    let mut b: [u8; 16] = digest.into();
    // version 3
    b[6] = (b[6] & 0x0f) | 0x30;
    // variant IETF (10xx)
    b[8] = (b[8] & 0x3f) | 0x80;
    format_uuid(&b)
}

fn format_uuid(b: &[u8; 16]) -> String {
    let h = hex::encode(b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

pub fn offline_account(name: &str) -> Account {
    Account {
        kind: AccountKind::Offline,
        username: name.to_string(),
        uuid: offline_uuid(name),
        access_token: "0".to_string(),
        refresh_token: None,
        user_type: Some("legacy".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_is_v3_deterministic() {
        let a = offline_uuid("Steve");
        let b = offline_uuid("Steve");
        assert_eq!(a, b);
        // version nibble = 3
        assert_eq!(&a[14..15], "3");
        // variant top bits = 10 => 首字符为 8/9/a/b
        assert!("89ab".contains(a[19..20].to_lowercase().to_string().as_str()));
        // 大小写不同输入应产生不同 UUID（与 Java 语义一致，字节不同）
        assert_ne!(offline_uuid("Steve"), offline_uuid("steve"));
        assert_eq!(a.len(), 36);
    }
}
