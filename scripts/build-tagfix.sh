#!/bin/sh
# 编译 Quartz tagged-pointer 兼容层 shim（Android arm64-v8a），
# 产物编入壳层二进制（apps/shell-tauri/src-tauri/src/lib.rs include_bytes!）。
# 源码与方案说明：scripts/tagfix/quartz_tagfix.c
set -e

NDK=/home/z/my-project/.toolchain/android-sdk/ndk/27.0.12077973
BIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
SRC=/home/z/my-project/scripts/tagfix/quartz_tagfix.c
OUT=/home/z/my-project/apps/shell-tauri/src-tauri/tagfix/libqztagfix.so

mkdir -p "$(dirname "$OUT")"
"$BIN/aarch64-linux-android24-clang" \
    -shared -fPIC -O2 -fvisibility=hidden \
    -Wl,-soname,libqztagfix.so \
    -o "$OUT" "$SRC"

"$BIN/llvm-strip" "$OUT"
echo "== built: $OUT =="
ls -l "$OUT"
"$BIN/llvm-readelf" -h "$OUT" | head -14
echo "== dynamic =="
"$BIN/llvm-readelf" -d "$OUT" | rg "NEEDED|SONAME|INIT" || true
