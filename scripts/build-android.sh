#!/bin/sh
# Quartz Android debug APK 构建（根卷配额已满，全部输出走 PolarFS）
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
mkdir -p "$TMPDIR"
cd /home/z/my-project/apps/shell-tauri/src-tauri
# --target aarch64：仅编 arm64 ABI（默认全 ABI 编译耗时 4 倍）
exec tauri android build --apk --debug --target aarch64

