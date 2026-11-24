#!/bin/sh
# 沙箱回收后幂等重建构建工具链（cargo/rustup → tauri-cli → SDK/NDK → JDK21）。
# 每项缺失才安装；装进 /home/z/my-project/.toolchain（项目内）。
set -e
TC=/home/z/my-project/.toolchain
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64

echo "=== [1/4] Rust（stable + aarch64-linux-android + rust-src）==="
if [ ! -x "$TC/cargo/bin/cargo" ]; then
  export RUSTUP_HOME="$TC/rustup" CARGO_HOME="$TC/cargo"
  ARCH=$(uname -m)
  curl -sSf -o /tmp/rustup-init "https://static.rust-lang.org/rustup/dist/$ARCH-unknown-linux-gnu/rustup-init" --max-time 300
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y --default-toolchain stable --profile minimal --no-modify-path >/dev/null
  "$TC/cargo/bin/rustup" target add aarch64-linux-android >/dev/null 2>&1
  "$TC/cargo/bin/rustup" component add rust-src >/dev/null 2>&1
  rm -f /tmp/rustup-init
fi
export RUSTUP_HOME="$TC/rustup" CARGO_HOME="$TC/cargo"
export PATH="$TC/cargo/bin:$PATH"
"$TC/cargo/bin/cargo" --version

echo "=== [2/4] tauri-cli（npm 预编译）==="
export NPM_CONFIG_PREFIX="$TC/npm-global"
if [ ! -x "$TC/npm-global/bin/tauri" ]; then
  npm install -g @tauri-apps/cli@^2 >/tmp/npm-tauri.log 2>&1
fi
ln -sf "$TC/npm-global/bin/tauri" "$TC/bin/cargo-tauri" 2>/dev/null || true

echo "=== [3/4] Android SDK（cmdline-tools + platform 36 + build-tools 36 + NDK 27）==="
SDK="$TC/android-sdk"
if [ ! -x "$SDK/ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" ]; then
  if [ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
    cd /tmp
    curl -sSf -o cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip --max-time 240
    unzip -q -o cmdtools.zip -d /tmp/sdk-cmdtools-extract
    mkdir -p "$SDK/cmdline-tools"
    rm -rf "$SDK/cmdline-tools/latest"
    mv /tmp/sdk-cmdtools-extract/cmdline-tools "$SDK/cmdline-tools/latest"
    rm -f cmdtools.zip
  fi
  SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"
  yes 2>/dev/null | "$SDKM" --licenses >/dev/null 2>&1 || true
  yes 2>/dev/null | "$SDKM" "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;27.0.12077973" >/tmp/sdk-install.log 2>&1
fi
ls "$SDK/ndk/" >/dev/null

echo "=== [4/4] JDK 21（javac/jar）==="
if [ ! -x "$TC"/jdk-21*/bin/javac ] 2>/dev/null && ! ls "$TC"/jdk-21*/bin/javac >/dev/null 2>&1; then
  curl -sSLf -o /tmp/jdk21.tar.gz "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse?project=jdk" --max-time 500
  tar -xzf /tmp/jdk21.tar.gz -C "$TC" && rm -f /tmp/jdk21.tar.gz
fi
ls "$TC"/jdk-21*/bin/javac >/dev/null
echo "TOOLCHAIN_READY"
