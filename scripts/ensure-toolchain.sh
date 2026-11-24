#!/bin/sh
# 沙箱回收后幂等重建构建工具链（cargo/rustup → tauri-cli → JDK21 → SDK/NDK）。
# 每项缺失才安装；装进 /home/z/my-project/.toolchain（项目内）。
# CI 兼容：JDK 在 sdkmanager 之前就绪；JAVA_HOME 自适应探测；set -x 全程行级追踪。
set -ex
PS4='+ L$LINENO: '
TC=/home/z/my-project/.toolchain
mkdir -p "$TC/bin" /tmp

echo "=== [1/5] Rust（stable + aarch64-linux-android + rust-src）==="
echo "[toolchain] host: $(uname -m) / disk: $(df -h /tmp | tail -1 | awk '{print $4}') free on /tmp"
if [ ! -x "$TC/cargo/bin/cargo" ]; then
  export RUSTUP_HOME="$TC/rustup" CARGO_HOME="$TC/cargo"
  ARCH=$(uname -m)
  curl -sSf --retry 3 -o /tmp/rustup-init "https://static.rust-lang.org/rustup/dist/$ARCH-unknown-linux-gnu/rustup-init" --max-time 300
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y --default-toolchain stable --profile minimal --no-modify-path >/dev/null
  "$TC/cargo/bin/rustup" target add aarch64-linux-android >/dev/null 2>&1
  "$TC/cargo/bin/rustup" component add rust-src >/dev/null 2>&1
  rm -f /tmp/rustup-init
fi
export RUSTUP_HOME="$TC/rustup" CARGO_HOME="$TC/cargo"
export PATH="$TC/cargo/bin:$PATH"
"$TC/cargo/bin/cargo" --version

echo "=== [2/5] tauri-cli（npm 预编译）==="
export NPM_CONFIG_PREFIX="$TC/npm-global"
if [ ! -x "$TC/npm-global/bin/tauri" ]; then
  npm install -g @tauri-apps/cli@^2 >/tmp/npm-tauri.log 2>&1 || {
    echo "[toolchain] npm install tauri-cli 失败，日志尾部："; tail -20 /tmp/npm-tauri.log; exit 1; }
fi
ln -sf "$TC/npm-global/bin/tauri" "$TC/bin/cargo-tauri" 2>/dev/null || true

echo "=== [3/5] JDK（优先 21；sdkmanager 依赖，必须在 SDK 前）==="
# 优先 JDK 21（与本地已验证环境一致）；无 21 则落到 runner 预装；都没有才下载
JAVA21=""
for d in "$TC"/jdk-21* /usr/lib/jvm/java-21-openjdk-amd64 /usr/lib/jvm/temurin-21-jdk-amd64; do
  [ -x "$d/bin/javac" ] && { JAVA21="$d"; break; }
done
if [ -n "$JAVA21" ]; then
  export JAVA_HOME="$JAVA21"
elif [ -n "$JAVA_HOME" ] && [ -x "$JAVA_HOME/bin/javac" ]; then
  echo "[toolchain] 系统无 JDK 21，使用预装 $JAVA_HOME"
else
  # 无任何系统 JDK → 安装 Adoptium 21 到 .toolchain
  curl -sSLf --retry 3 -o /tmp/jdk21.tar.gz "https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse?project=jdk" --max-time 500 \
    || { echo "[toolchain] JDK 下载失败 rc=$?"; exit 1; }
  tar -xzf /tmp/jdk21.tar.gz -C "$TC" && rm -f /tmp/jdk21.tar.gz
  for d in "$TC"/jdk-21*; do [ -x "$d/bin/javac" ] && { export JAVA_HOME="$d"; break; }; done
fi
[ -x "$JAVA_HOME/bin/javac" ] || { echo "[toolchain] 找不到可用 JDK"; exit 1; }
export PATH="$JAVA_HOME/bin:$PATH"
echo "[toolchain] JAVA_HOME=$JAVA_HOME"

echo "=== [4/5] Android SDK（cmdline-tools + platform 36 + build-tools 36 + NDK 27）==="
SDK="$TC/android-sdk"
SDKM=""
if [ ! -x "$SDK/ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" ]; then
  # sdkmanager 三级查找：自身 TC 缓存 → runner 预装 SDK（CI）→ 下载 cmdline-tools
  if [ -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
    SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"
    echo "[toolchain] sdkmanager: TC 缓存"
  elif [ -x /usr/local/lib/android/sdk/cmdline-tools/latest/bin/sdkmanager ]; then
    SDKM=/usr/local/lib/android/sdk/cmdline-tools/latest/bin/sdkmanager
    echo "[toolchain] sdkmanager: runner 预装 SDK"
  else
    echo "[toolchain] 下载 cmdline-tools…"
    cd /tmp
    curl -sSf --retry 3 -o cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip --max-time 240 \
      || { echo "[toolchain] cmdtools 下载失败 rc=$?"; exit 1; }
    unzip -q -o cmdtools.zip -d /tmp/sdk-cmdtools-extract \
      || { echo "[toolchain] cmdtools 解压失败 rc=$?"; exit 1; }
    mkdir -p "$SDK/cmdline-tools"
    rm -rf "$SDK/cmdline-tools/latest"
    mv /tmp/sdk-cmdtools-extract/cmdline-tools "$SDK/cmdline-tools/latest"
    rm -f cmdtools.zip
    cd - >/dev/null
    SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"
  fi
  yes 2>/dev/null | "$SDKM" --sdk_root="$SDK" --licenses >/dev/null 2>&1 || true
  yes 2>/dev/null | "$SDKM" --sdk_root="$SDK" "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;27.0.12077973" >/tmp/sdk-install.log 2>&1 \
    || { echo "[toolchain] sdkmanager 失败 rc=$?，日志尾部："; tail -40 /tmp/sdk-install.log; exit 1; }
fi
ls "$SDK/ndk/" >/dev/null || { echo "[toolchain] NDK 目录异常"; exit 1; }

echo "=== [5/5] 校验 ==="
ls "$JAVA_HOME/bin/javac" >/dev/null
echo "TOOLCHAIN_READY"
