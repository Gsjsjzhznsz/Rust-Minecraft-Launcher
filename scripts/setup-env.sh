#!/bin/sh
# Quartz 构建环境变量（source 本文件后再执行 cargo/gradle 命令）
# 工具链由 scripts/ensure-toolchain.sh 幂等安装进 .toolchain（项目内）：
#   rustup -> .toolchain/rustup + .toolchain/cargo（沙箱回收 ~/.cargo 后仍可用）
#   tauri-cli -> .toolchain/npm-global（npm 预编译二进制）
#   SDK/NDK -> .toolchain/android-sdk；JDK -> .toolchain/jdk-21*
# JDK 优先用 .toolchain 内的发行版；缺失时回退系统 OpenJDK 21（Debian 自带）
# JDK：外部已就绪（CI 预装 / ensure-toolchain 已选定）优先保持；其次 .toolchain 发行版；最后系统路径
TC=/home/z/my-project/.toolchain
if [ -x "$JAVA_HOME/bin/javac" ]; then
  : # keep
else
  JDK_DIR=$(ls -d "$TC"/jdk-21*/bin 2>/dev/null | head -1)
  if [ -n "$JDK_DIR" ]; then
    export JAVA_HOME=${JDK_DIR%/bin}
  else
    export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
  fi
fi
export ANDROID_HOME=$TC/android-sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
export ANDROID_NDK_ROOT=$NDK_HOME
export GRADLE_USER_HOME=/tmp/my-project/.qz-gradle-home
export CARGO_HOME=$TC/cargo
export RUSTUP_HOME=$TC/rustup
export NPM_CONFIG_PREFIX=$TC/npm-global
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export PATH="$TC/cargo/bin:$TC/npm-global/bin:$TC/bin:$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
