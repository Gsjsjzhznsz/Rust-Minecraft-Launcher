#!/bin/sh
# Quartz 构建环境变量（source 本文件后再执行 cargo/gradle 命令）
export JAVA_HOME=/home/z/my-project/.toolchain/jdk-21.0.13.1+2
export ANDROID_HOME=/home/z/my-project/.toolchain/android-sdk
export NDK_HOME=$ANDROID_HOME/ndk/26.3.11579264
export ANDROID_NDK_ROOT=$NDK_HOME
export GRADLE_USER_HOME=/home/z/my-project/.gradle-home
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export PATH="$HOME/.cargo/bin:$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
