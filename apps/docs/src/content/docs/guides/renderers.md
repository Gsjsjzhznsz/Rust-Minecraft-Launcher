---
title: 渲染层选择
description: Zink / ANGLE / 软渲染的选型逻辑与手动切换
---

Minecraft Java 版依赖 OpenGL，而移动端 GPU 只讲 Vulkan / GLES。
Quartz 用「渲染 pack」把 GL 翻译到底层 API，按 Minecraft 版本与设备能力自动路由，
也允许在「设置 → 渲染层」手动切换。

## 自动路由规则

| Minecraft 版本 | 需要的 GL 档位 | 选择 |
| --- | --- | --- |
| ≤ 1.16 | GL 2.1 | gl4es（GLES2）或软渲染兜底 |
| 1.17+（含快照） | GL 3.2+ | Zink（GL 4.6 → Vulkan），ANGLE 兼容档 |
| 桌面 / WoA | 原生或翻译层 | MetalANGLE（iOS）/ Mesa Dozen（Windows on ARM）/ 原生驱动 |

路由逻辑由 `quartz-core` 的 `renderbus` 实现（`required_profile()` 决定档位），
含快照与远古版本命名识别，单测覆盖。

## Android 当前档位

| 渲染层 | 原理 | 适用场景 | 体积 |
| --- | --- | --- | --- |
| **Zink (Vulkan)** | GL 4.6 → Vulkan | 1.17+ / Sodium / 光影，**推荐** | 约 45MB |
| **ANGLE (Vulkan)** | GLES 3 → Vulkan | 兼容备选，Zink 异常时切换 | 约 30MB |
| **软渲染** | 纯 CPU | 无 GPU / 极端兜底，性能低 | 约 12MB |

## 实用建议

- **1.17+ 白屏或花屏**：先确认渲染层为 Zink 且已安装完成；仍异常换 ANGLE。
- **老版本（1.12.2 等）**：默认路由即可，无需手动选 Vulkan 档。
- **Sodium / 光影**：只有 Zink 档提供完整 GL 4.6 能力。
- 切换渲染层后无需重装游戏，下一次启动即生效。
