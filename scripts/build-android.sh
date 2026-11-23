#!/bin/sh
# Quartz Android debug APK 构建（根卷配额已满，全部输出走 PolarFS）
# v0.1.5：先构建 Rust 显示桥（quartz-bridge -> libpojavexec.so），
#         壳层 include_bytes 打包并在运行时落位覆盖 Pojav C 桥。
. /home/z/my-project/scripts/setup-env.sh
export TMPDIR=/tmp/my-project/.qz-tmp
export CARGO_TARGET_DIR=/tmp/my-project/.qz-cargo-target
export GRADLE_USER_HOME=/tmp/my-project/.qz-gradle-home
export _JAVA_OPTIONS="-Djava.io.tmpdir=/tmp/my-project/.qz-tmp"
# NDK 工具链（手动 cargo build 时 cc-rs/ring 需要）
NDKBIN=/home/z/my-project/.toolchain/android-sdk/ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64/bin
export PATH="$NDKBIN:$PATH"
export CC_aarch64_linux_android=$NDKBIN/aarch64-linux-android24-clang
export CXX_aarch64_linux_android=$NDKBIN/aarch64-linux-android24-clang++
export AR_aarch64_linux_android=$NDKBIN/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDKBIN/aarch64-linux-android24-clang
mkdir -p "$TMPDIR"

# ---- ① 显示桥（release：GL 调度/事件泵在游戏热路径上）----
cargo build -p quartz-bridge --release --target aarch64-linux-android \
  --manifest-path /home/z/my-project/Cargo.toml || exit 1
BRIDGE_OUT=$CARGO_TARGET_DIR/aarch64-linux-android/release/libquartz_bridge.so
mkdir -p /home/z/my-project/apps/shell-tauri/src-tauri/bridge
cp "$BRIDGE_OUT" /home/z/my-project/apps/shell-tauri/src-tauri/bridge/libpojavexec.so || exit 1
$NDKBIN/llvm-strip /home/z/my-project/apps/shell-tauri/src-tauri/bridge/libpojavexec.so
echo "[build] 显示桥就绪：$(ls -la /home/z/my-project/apps/shell-tauri/src-tauri/bridge/libpojavexec.so | awk '{print $5}') 字节"

# ---- ①.5 游戏退出捕获 hook jar（自研 SecurityManager，javac --release 17）----
HOOK_DIR=/home/z/my-project/apps/shell-tauri/src-tauri/bridge
rm -rf "$HOOK_DIR/classes" "$HOOK_DIR/quartz-game-hook.jar"
"$JAVA_HOME/bin/javac" --release 17 -d "$HOOK_DIR/classes" "$HOOK_DIR"/java/com/quartz/game/*.java || exit 1
"$JAVA_HOME/bin/jar" --create --file "$HOOK_DIR/quartz-game-hook.jar" -C "$HOOK_DIR/classes" . || exit 1
rm -rf "$HOOK_DIR/classes"
echo "[build] 退出捕获 hook 就绪：$(ls -la "$HOOK_DIR/quartz-game-hook.jar" | awk '{print $5}') 字节"

# ---- ② 壳层 APK（tauri android build）----
cd /home/z/my-project/apps/shell-tauri/src-tauri
# --target aarch64：仅编 arm64 ABI（默认全 ABI 编译耗时 4 倍）
exec tauri android build --apk --debug --target aarch64
