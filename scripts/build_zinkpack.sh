#!/bin/sh
# v0.2.20：Zink 渲染层包 —— 与 ZalithLauncher2 内置 Mesa 制品逐字节一致。
#
# 背景（用户指令「跟 ZL2 一模一样」）：此前 zink 包取自 GoyDevv/IronizedZink
# v1.1.0（第三方重打包）。实测比对（2026-11-27，sha256）：IronizedZink 的
# Mesa 五件套（libEGL_mesa/libglapi/libglxshim/libzink_dri/libcutils）与
# ZL2 2.8.2 官方 arm64 APK 内置的**逐字节相同**，仅 libc++_shared.so 不同
# （IZ 为 9.3MB 胖 NDK 构建，ZL2 为官方 NDK r25c 构建 1.0MB）。
# 本脚本改为直接从 ZL2 官方 Release APK 提取 —— 制品、来源、版本全部 1:1。
#
# 产物：download/renderer-zink-aarch64.zip（zip 根级平铺，与 turnip 包同模式）
#   libEGL_mesa.so / libglapi.so / libglxshim.so / libcutils.so /
#   libc++_shared.so / libzink_dri.so —— 六件全部保留 ZL2 APK 内的原始文件名
#   （与 ZL2 lib/arm64-v8a 逐文件同名同字节，字节 + 文件名级 1:1）。
#   v0.2.22 起 App 解包同样保留原名（不再落盘改名 libzink_dri.so →
#   libgallium_dri.so，那是 v0.2.20/21 的历史契约，见下方 v0.2.22 注记）。
#
# v0.2.21 事故复盘：v0.2.20 曾在本脚本内 mv 改名，zip 条目变 libgallium_dri.so
# 而 App 白名单仍找 libzink_dri.so —— extract 静默跳过 → gallium 驱动未落盘 →
# 用户看到「zink 下载失败」。修复：zip 恢复 ZL2 原名（改名职责归 App 解包层，
# 与 v0.2.19 前 APK 布局契约一致），App 端白名单加严格校验（缺条目硬失败点名）。
#
# v0.2.22（用户指令「跟 ZL2 完全一样」）：落盘名同步对齐 ZL2 —— App 白名单
# 改为 libzink_dri.so → libzink_dri.so（不再落盘改名；ZL2 APK 磁盘名就是
# libzink_dri.so，DT_SONAME=libgallium_dri.so 只是链接器内部标识）。本脚本
# zip 内容不变（仍是 ZL2 原名六件），包代数 rev 2→3 触发老用户重装。
#
# 制品指纹（ZL2 2.8.2 arm64-v8a，发布资产 sha256 由 Release API 核验）：
#   APK sha256 = 532503adeb3a9a5b7236ba027f2c9473fd3fb19a1cf0e1970262dca52294fca8
#   libEGL_mesa.so  302520 B
#   libglapi.so     404344 B
#   libglxshim.so   261600 B
#   libzink_dri.so  12506640 B（SONAME libgallium_dri.so）
#   libcutils.so     21320 B
#   libc++_shared.so 1027408 B

set -e

ROOT=/home/z/my-project
OUT=$ROOT/download/renderer-zink-aarch64.zip
CACHE=$ROOT/.zl2-cache
mkdir -p "$ROOT/download" "$CACHE"

ZL2_VER=2.8.2
ZL2_APK_URL="https://github.com/ZalithLauncher/ZalithLauncher2/releases/download/${ZL2_VER}/ZalithLauncher-2.8.2-arm64-v8a.apk"
ZL2_APK_SHA=532503adeb3a9a5b7236ba027f2c9473fd3fb19a1cf0e1970262dca52294fca8
APK="$CACHE/ZalithLauncher-${ZL2_VER}-arm64-v8a.apk"

echo "[zinkpack] 获取 ZL2 ${ZL2_VER} 官方 arm64 APK（缓存优先）"
if [ ! -f "$APK" ]; then
  curl -fSL --retry 3 --connect-timeout 30 -o "$APK" "$ZL2_APK_URL"
fi
ACTUAL_SHA=$(sha256sum "$APK" | awk '{print $1}')
if [ "$ACTUAL_SHA" != "$ZL2_APK_SHA" ]; then
  echo "[zinkpack] FATAL：ZL2 APK sha256 不匹配（期望 $ZL2_APK_SHA，实际 $ACTUAL_SHA）" >&2
  exit 1
fi
echo "[zinkpack] sha256 校验通过：$ACTUAL_SHA"

STAGE="$CACHE/zink-stage"
rm -rf "$STAGE" && mkdir -p "$STAGE"
unzip -o -q "$APK" \
  "lib/arm64-v8a/libEGL_mesa.so" \
  "lib/arm64-v8a/libglapi.so" \
  "lib/arm64-v8a/libglxshim.so" \
  "lib/arm64-v8a/libzink_dri.so" \
  "lib/arm64-v8a/libcutils.so" \
  "lib/arm64-v8a/libc++_shared.so" \
  -d "$STAGE"

cd "$STAGE/lib/arm64-v8a"
# v0.2.21：不再改名 —— zip 条目与 ZL2 APK lib/arm64-v8a 逐文件同名同字节；
# libzink_dri.so → libgallium_dri.so 的 SONAME 落盘改名由 App 端白名单执行

# 产物指纹打印（与脚本头注释核对）
echo "[zinkpack] 提取完成（保留 ZL2 原名），指纹："
sha256sum libEGL_mesa.so libglapi.so libglxshim.so libzink_dri.so libcutils.so libc++_shared.so
ls -la

rm -f "$OUT"
zip -q -j "$OUT" \
  libEGL_mesa.so libglapi.so libglxshim.so libzink_dri.so libcutils.so libc++_shared.so
echo "[zinkpack] 产物：$OUT"
ls -la "$OUT"
unzip -l "$OUT"
