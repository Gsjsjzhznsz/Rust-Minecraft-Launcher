#!/bin/sh
# 沙箱回收后一键重建构建环境：Rust 工具链 + tauri-cli(npm 预编译) + Android SDK/NDK
# 后台运行：setsid sh scripts/bootstrap.sh，日志 /tmp/bootstrap.log
set -e
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
TC=/home/z/my-project/.toolchain
mkdir -p "$TC/bin"

echo "=== [1/3] Rust 工具链（复用 rustup shim，补 stable + android target） ==="
RUSTUP="$HOME/.cargo/bin/rustup"
if [ ! -x "$RUSTUP" ]; then
  ARCH=$(uname -m)
  curl -sSf -o /tmp/rustup-init "https://static.rust-lang.org/rustup/dist/$ARCH-unknown-linux-gnu/rustup-init"
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y --default-toolchain stable --profile minimal >/dev/null
  RUSTUP="$HOME/.cargo/bin/rustup"
fi
"$RUSTUP" toolchain install stable --profile minimal
"$RUSTUP" default stable
"$RUSTUP" target add aarch64-linux-android
"$RUSTUP" component add rust-src
"$HOME/.cargo/bin/cargo" --version

echo "=== [2/3] tauri-cli（npm 预编译二进制） ==="
export NPM_CONFIG_PREFIX=/home/z/.npm-global
if ! "$NPM_CONFIG_PREFIX/bin/tauri" --version >/dev/null 2>&1; then
  npm install -g @tauri-apps/cli@^2 >/tmp/npm-tauri.log 2>&1
fi
# 统一暴露为 cargo-tauri（tauri-cli 的 cargo 子命令入口）
ln -sf "$NPM_CONFIG_PREFIX/bin/tauri" "$TC/bin/cargo-tauri"
"$TC/bin/cargo-tauri" --version

echo "=== [3/3] Android SDK: platform-tools + android-36 + build-tools 36 + NDK 27 ==="
SDK="$TC/android-sdk"
if [ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
  cd /tmp
  curl -sSf -o cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip
  unzip -q -o cmdtools.zip -d /tmp/sdk-cmdtools-extract
  mkdir -p "$SDK/cmdline-tools"
  rm -rf "$SDK/cmdline-tools/latest"
  mv /tmp/sdk-cmdtools-extract/cmdline-tools "$SDK/cmdline-tools/latest"
  rm -f /tmp/cmdtools.zip
fi
SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"
yes | "$SDKM" --licenses >/dev/null 2>&1 || true
"$SDKM" "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;27.0.12077973" >/tmp/sdk-install.log 2>&1

echo "=== [4/4] 全量 JDK 21（gradle/javac 需要；JRE 不够） ==="
if [ ! -x "$TC/jdk-21.0.13.1+2/bin/javac" ]; then
  curl -sSf -o /tmp/jdk21.tar.gz "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse?project=jdk"
  mkdir -p "$TC/jdk-21.0.13.1+2"
  tar xzf /tmp/jdk21.tar.gz -C "$TC/jdk-21.0.13.1+2" --strip-components=1
  rm -f /tmp/jdk21.tar.gz
fi
"$TC/jdk-21.0.13.1+2/bin/javac" -version

ls "$SDK/ndk" "$SDK/platforms" "$SDK/build-tools"
echo "BOOTSTRAP_DONE"
