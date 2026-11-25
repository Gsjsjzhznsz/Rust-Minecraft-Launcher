---
title: 账号与登录
description: 离线、微软账户与 authlib 外置登录
---

Quartz 支持三种登录方式，多账号可并存，随时切换。

## 离线模式

- 输入用户名即用，UUID 采用与官方一致的
  `nameUUIDFromBytes("OfflinePlayer:<name>")` 语义（可用 CLI 验证：
  `cargo run -p quartz-cli -- uuid Steve`）。
- 单人世界、局域网与离线服务器可用；正版验证服务器会拒绝离线 UUID。

## 微软账户（设备码流程）

1. 「账户」页选「微软登录」，应用向官方 OAuth 端点申请设备码。
2. 在任意浏览器打开 [microsoft.com/link](https://www.microsoft.com/link) 输入设备码并授权。
3. 应用走完 XBL → XSTS → Minecraft 证书全链路后自动登录。

合规说明：

- OAuth 必须使用项目**自有** Azure client_id —— 未配置时微软登录入口直接禁用
  （可在「设置」页填入自己的 client_id）。
- 全程官方流程，不触碰任何第三方代登录服务。

## authlib 外置登录

对接 authlib-injector 生态的外置认证服务器（自建或第三方皮肤站）：

- 填服务器地址 + 账号密码完成认证。
- agent 缺失时应用会自动下载并注入。

## 多账号管理

- 账号列表常驻侧栏头像处，点击即切换当前账号。
- 移除账号只删除本机凭据，不影响服务器侧状态。
