#!/bin/sh
# 安装 Android SDK cmdline-tools / platform-tools / android-34 / build-tools 34 / NDK 26.3.11579264
set -e
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64
mkdir -p /home/z/my-project/.toolchain/android-sdk/cmdline-tools
cd /tmp
curl -sSf -o cmdtools.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip
unzip -q -o cmdtools.zip -d /home/z/my-project/.toolchain/android-sdk/cmdline-tools
[ -d /home/z/my-project/.toolchain/android-sdk/cmdline-tools/latest ] || mv /home/z/my-project/.toolchain/android-sdk/cmdline-tools/cmdline-tools /home/z/my-project/.toolchain/android-sdk/cmdline-tools/latest
rm -f /tmp/cmdtools.zip
SDKM=/home/z/my-project/.toolchain/android-sdk/cmdline-tools/latest/bin/sdkmanager
yes | "$SDKM" --licenses > /tmp/sdk-licenses.log 2>&1 || true
"$SDKM" "platform-tools" "platforms;android-34" "build-tools;34.0.0" "ndk;26.3.11579264" > /tmp/sdk-install.log 2>&1
echo SDK_DONE
