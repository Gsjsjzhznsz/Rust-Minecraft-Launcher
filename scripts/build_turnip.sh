#!/bin/sh
# 构建 Turnip（Mesa freedreno Vulkan ICD，KGSL 内核后端 + Android WSI）打包 zip。
#
# 背景（v0.2.17）：骁龙设备的 zink 需要一个「免 root 可用」的 Vulkan 驱动。
# 厂商 Adreno blob 在 App 进程内不可加载（v0.2.15/16 实机日志：ICD dlsym 失败 +
# 系统 loader 路径 SIGSEGV）；社区预编译（FCL/ZL2 的 libvulkan_freedreno.so）是
# 老式 HAL 模块（只导出 HMI），Termux 构建缺 Android WSI —— 均不能被我们
# 的 VULKAN_PTR 直连消费端（IronizedZink 只 dlsym vkGetInstanceProcAddr）使用。
# 因此从 Mesa 源码自建：
#   - mesa-26.3.1（Termux 同款版本，免 root KGSL 实证可用）
#   - -Dfreedreno-kmds=kgsl（/dev/kgsl-3d0，非 root Adreno）
#   - -Dplatforms=android -Dandroid-stub=true（假库仅链接期，部署时用系统真库）
#   - 补丁 scripts/mesa-turnip-plain-gpa.patch：version script 增加
#     vk_icdGetInstanceProcAddr / vkGetInstanceProcAddr 导出（mesa 的
#     vulkan-android.sym 默认只导 HMI —— 这就是 FCL/ZL2 预编译「没有入口」
#     的根源），源码补 plain gpa 转发。
# 产物：download/renderer-turnip-aarch64.zip（libvulkan_freedreno.so +
# libc++_shared.so + freedreno_icd.json），随 Release 发布，运行时在线下载。
#
# 用法: sh scripts/build_turnip.sh
# 依赖: ensure-toolchain.sh（NDK）、meson/ninja/mako、glslang(>=12.2)、bison/flex/m4。
#       CI ubuntu-latest 自带 bison/flex/m4/pkg-config，glslang 由本脚本下载。
set -e
PROJ=/home/z/my-project
TC=$PROJ/.toolchain
NDK="$TC/android-sdk/ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64"
MESA_TAG=mesa-26.3.1
MESA_SHA=fec8830e9c94b1d67f3d8469467d5529fb2978cd

WORK=${TURNIP_WORK:-/tmp/mesa-turnip}
GLSLANG_TAG=16.6.0

echo "=== [1/6] 构建机工具（glslang / bison / flex / m4）==="
# glslang：CI runner 无预装；本地沙盒亦无 apt 权限 —— 用 Khronos 官方预编译
if ! command -v glslangValidator >/dev/null 2>&1; then
  mkdir -p "$WORK/glslang"
  curl -sL --retry 3 --max-time 300 -o "$WORK/glslang.tar.gz" \
    "https://github.com/KhronosGroup/glslang/releases/download/${GLSLANG_TAG}/glslang-${GLSLANG_TAG}-linux-x86_64-release.tar.gz"
  tar -xzf "$WORK/glslang.tar.gz" -C "$WORK/glslang"
  export PATH="$WORK/glslang/bin:$PATH"
fi
glslangValidator --version >/dev/null
command -v bison >/dev/null || { echo "缺 bison"; exit 1; }
command -v flex  >/dev/null || { echo "缺 flex";  exit 1; }
command -v m4    >/dev/null || { echo "缺 m4";    exit 1; }
command -v pkg-config >/dev/null || { echo "缺 pkg-config"; exit 1; }
python3 -c 'import mako' 2>/dev/null || python3 -m pip install -q mako pyyaml
# meson/ninja：runner 未必预装（pip 装到用户 bin，入 PATH）
command -v meson >/dev/null || python3 -m pip install -q meson ninja
export PATH="$HOME/.local/bin:$PATH"
command -v meson >/dev/null || { echo "缺 meson"; exit 1; }
command -v ninja >/dev/null || { echo "缺 ninja"; exit 1; }
echo "glslang/bison/flex/m4/pkg-config/mako/meson/ninja 就绪"

echo "=== [2/6] NDK ==="
[ -x "$NDK/bin/aarch64-linux-android29-clang" ] || { echo "NDK 缺失：$NDK"; exit 1; }

echo "=== [3/6] Mesa 源码（$MESA_TAG）==="
mkdir -p "$WORK"
if [ ! -d "$WORK/src/.git" ]; then
  git clone --depth 1 --branch "$MESA_TAG" \
    https://gitlab.freedesktop.org/mesa/mesa.git "$WORK/src"
fi
cd "$WORK/src"
# 浅克隆 tag 与 pin SHA 一致（防 tag 被移动）
ACTUAL=$(git rev-parse HEAD)
if [ "$ACTUAL" != "$MESA_SHA" ]; then
  echo "mesa SHA 不符：$ACTUAL != $MESA_SHA"; exit 1
fi
# 补丁幂等：重跑构建时源码可能已带补丁
if grep -q 'vk_icdGetInstanceProcAddr;' src/vulkan/vulkan-android.sym 2>/dev/null \
  && grep -q 'Quartz: also expose the plain entry' src/freedreno/vulkan/tu_device.cc 2>/dev/null; then
  echo "补丁已应用，跳过"
else
  git apply --check "$PROJ/scripts/mesa-turnip-plain-gpa.patch"
  git apply "$PROJ/scripts/mesa-turnip-plain-gpa.patch"
fi
cd "$WORK"

echo "=== [4/6] meson 配置（android cross + KGSL）==="
cat > "$WORK/android-aarch64.ini" <<EOF
[binaries]
c = '$NDK/bin/aarch64-linux-android29-clang'
cpp = '$NDK/bin/aarch64-linux-android29-clang++'
ar = '$NDK/bin/llvm-ar'
strip = '$NDK/bin/llvm-strip'

[host_machine]
system = 'android'
cpu_family = 'aarch64'
cpu = 'aarch64'
endian = 'little'

[properties]
needs_exe_wrapper = true
EOF
rm -rf "$WORK/build"
meson setup build src --cross-file "$WORK/android-aarch64.ini" \
  -Dvulkan-drivers=freedreno \
  -Dgallium-drivers= \
  -Dfreedreno-kmds=kgsl \
  -Dplatforms=android \
  -Dandroid-stub=true \
  -Degl=disabled -Dgbm=disabled -Dglx=disabled \
  -Dllvm=disabled -Dzstd=disabled \
  -Dbuild-tests=false \
  2>&1 | tail -4

echo "=== [5/6] 编译 + strip ==="
ninja -C build src/freedreno/vulkan/libvulkan_freedreno.so 2>&1 | tail -2
# ICD json 是独立 custom target，显式构建（无 .aarch64 后缀的平台名不同，逐个尝试）
JSON_REL=""
for j in freedreno_icd.aarch64.json freedreno_icd.aarch32.json freedreno_icd.json; do
  if ninja -C build "src/freedreno/vulkan/$j" >/dev/null 2>&1 && [ -f "build/src/freedreno/vulkan/$j" ]; then
    JSON_REL="$j"; break
  fi
done
[ -n "$JSON_REL" ] || { echo "ICD json 构建失败"; exit 1; }
SO=build/src/freedreno/vulkan/libvulkan_freedreno.so
[ -f "$SO" ] || { echo "产物缺失：$SO"; exit 1; }
"$NDK/bin/llvm-strip" -o "$WORK/libvulkan_freedreno.so" "$SO"

# 导出自检：直连消费端（IronizedZink）只 dlsym vkGetInstanceProcAddr
SYMS=$("$NDK/bin/llvm-nm" -D --defined-only "$WORK/libvulkan_freedreno.so" 2>/dev/null | awk '{print $NF}')
for want in vkGetInstanceProcAddr vk_icdGetInstanceProcAddr vk_icdNegotiateLoaderICDInterfaceVersion; do
  echo "$SYMS" | grep -qx "$want" || { echo "导出缺失：$want"; exit 1; }
done
strings -a "$WORK/libvulkan_freedreno.so" | grep -q kgsl || { echo "KGSL 后端缺失"; exit 1; }

echo "=== [6/6] 打包 zip ==="
JSON_DIR="build/src/freedreno/vulkan/$JSON_REL"
[ -f "$JSON_DIR" ] || { echo "ICD json 缺失"; exit 1; }
STAGE="$WORK/pack"
rm -rf "$STAGE"; mkdir -p "$STAGE"
cp "$WORK/libvulkan_freedreno.so" "$STAGE/"
# C++ 运行时（NDK 同源，DT_NEEDED libc++_shared.so）
cp "$NDK/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$STAGE/"
# ICD json 指向同目录
python3 - "$JSON_DIR" "$STAGE/freedreno_icd.json" <<'EOF'
import json, sys
src, dst = sys.argv[1], sys.argv[2]
d = json.load(open(src))
d["ICD"]["library_path"] = "./libvulkan_freedreno.so"
json.dump(d, open(dst, "w"), indent=4)
EOF
mkdir -p "$PROJ/download"
rm -f "$PROJ/download/renderer-turnip-aarch64.zip"
(cd "$STAGE" && zip -q -r "$PROJ/download/renderer-turnip-aarch64.zip" .)
ls -la "$PROJ/download/renderer-turnip-aarch64.zip"
echo "TURNIP_PACK_READY"
