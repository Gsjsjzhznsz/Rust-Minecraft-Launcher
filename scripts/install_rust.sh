#!/bin/sh
# 安装 Rust stable + 4 个 Android 交叉编译 target（先落文件再执行，不用 curl|sh）
set -e
ARCH=$(uname -m)
case "$ARCH" in
  x86_64) RUSTUP_ARCH="x86_64-unknown-linux-gnu" ;;
  aarch64) RUSTUP_ARCH="aarch64-unknown-linux-gnu" ;;
  *) echo "unsupported arch $ARCH"; exit 1 ;;
esac
curl -sSf -o /tmp/rustup-init "https://static.rust-lang.org/rustup/dist/$RUSTUP_ARCH/rustup-init"
chmod +x /tmp/rustup-init
/tmp/rustup-init -y --default-toolchain stable --profile minimal
"$HOME/.cargo/bin/rustup" target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
"$HOME/.cargo/bin/rustc" --version
echo RUST_DONE
