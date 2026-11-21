#!/usr/bin/env bash
# 重装 Android SDK / NDK（沙箱回收后需要重新安装）
# 用法：后台运行，日志 /home/z/sdk-setup.log，完成标记 /home/z/sdk-setup.done
set -x
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
SDK=/home/z/android-sdk
mkdir -p "$SDK"

# 1. cmdline-tools
if [ ! -d "$SDK/cmdline-tools" ]; then
  cd /tmp
  curl -sSL -o cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip
  unzip -q cmdtools.zip -d /tmp/cmdtools-extract
  mkdir -p "$SDK/cmdline-tools"
  mv /tmp/cmdtools-extract/cmdline-tools "$SDK/cmdline-tools/latest"
  rm -rf /tmp/cmdtools.zip /tmp/cmdtools-extract
fi

SDKM="$SDK/cmdline-tools/latest/bin/sdkmanager"

# 2. 平台 / 构建工具 / NDK（NDK 显式版本写入 app/build.gradle.kts）
yes | "$SDKM" --licenses >/dev/null 2>&1
yes | "$SDKM" "platform-tools" "platforms;android-37" "build-tools;37.0.0" "build-tools;36.0.0" "ndk;27.0.12077973" >"$SDK/sdk_install.log" 2>&1

# 3. 校验
ls "$SDK/ndk" "$SDK/platforms" "$SDK/build-tools" > /home/z/sdk-setup.done 2>&1
echo "SETUP OK $(date)" >> /home/z/sdk-setup.done
