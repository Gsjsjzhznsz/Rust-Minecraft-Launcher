#!/bin/sh
# Quartz v9 打包收尾：strip 壳层 so -> repack -> zipalign -> apksigner -> 验证
set -e
. /home/z/my-project/scripts/setup-env.sh
NDKBIN=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
# 工具链自适应：优先 build-tools 36，回退 34；strip 优先 NDK llvm-strip，回退 binutils-aarch64
detect_bt() {
  for d in $(ls -dv /home/z/my-project/.toolchain/android-sdk/build-tools/*/ 2>/dev/null); do
    [ -x "$d/zipalign" ] && [ -x "$d/apksigner" ] && { echo "${d%/}"; return; }
  done
  echo ""
}
BT=$(detect_bt)
[ -n "$BT" ] || { echo "[pack] ERROR: no build-tools with zipalign/apksigner found"; exit 1; }
echo "[pack] build-tools: $BT"
STRIP_BIN=""
if [ -x "$NDKBIN/llvm-strip" ]; then
  STRIP_BIN="$NDKBIN/llvm-strip"
elif [ -x /tmp/apkwork/btbin/usr/bin/aarch64-linux-gnu-strip ]; then
  STRIP_BIN=/tmp/apkwork/btbin/usr/bin/aarch64-linux-gnu-strip
  export LD_LIBRARY_PATH=/tmp/apkwork/btbin/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
fi
[ -n "$STRIP_BIN" ] || { echo "[pack] ERROR: no aarch64 strip tool found"; exit 1; }
echo "[pack] strip tool: $STRIP_BIN"
# SRC_APK 自动探测：环境变量 > gradle 重定向输出（buildDirectory 指向 /tmp）> 常规 gen/android 输出
SRC_APK=${QUARTZ_RAW_APK:-}
if [ -z "$SRC_APK" ]; then
  for c in /tmp/my-project/.qz-gradle-build/app/outputs/apk/universal/debug/app-universal-debug.apk \
           /home/z/my-project/apps/shell-tauri/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk; do
    [ -f "$c" ] && { SRC_APK="$c"; break; }
  done
fi
[ -n "$SRC_APK" ] && [ -f "$SRC_APK" ] || { echo "[pack] ERROR: 找不到壳层 APK（设置 QUARTZ_RAW_APK 或先跑 build-android.sh）"; exit 1; }
echo "[pack] SRC_APK: $SRC_APK"
OUT_APK=/home/z/my-project/download/Quartz.apk
KS=/home/z/my-project/keystore/quartz-debug.keystore

mkdir -p /tmp/apkwork /home/z/my-project/download

# 0) v0.2.23：ZL2 2.8.2 原版 libcutils.so 注入（jniLibs 通道）。
#    Mesa 栈唯一无 DT_SONAME 的按名依赖（libEGL_mesa/libzink_dri 的
#    DT_NEEDED）；clns 搜索路径 = nativeLibraryDir（进程创建时冻结），
#    数据目录里的预载无法按名命中 —— 与 ZL2 同款：其 APK lib/arm64-v8a
#    原样携带此文件，安装后落 nativeLibraryDir，按文件名解析。
#    sha256 锁定 = ZL2 2.8.2 APK 提取件（与 renderer-zink zip 内逐字节
#    相同，build_zinkpack.sh 同源），错位即构建失败。
LIBCUTILS_SRC=/home/z/my-project/apps/shell-tauri/src-tauri/bridge/libcutils.so
LIBCUTILS_SHA=04258bb1ccef8f5097ddc32ae1fb2d57142ffc5999e96fa36bfab85e02e9e347
mkdir -p /tmp/apkwork/jniextra
[ -f "$LIBCUTILS_SRC" ] || { echo "[pack] ERROR: $LIBCUTILS_SRC 缺失（ZL2 jniLibs 契约件）"; exit 1; }
echo "$LIBCUTILS_SHA  $LIBCUTILS_SRC" | sha256sum -c - || { echo "[pack] ERROR: libcutils.so sha256 不符（ZL2 2.8.2 契约破坏）"; exit 1; }
cp "$LIBCUTILS_SRC" /tmp/apkwork/jniextra/libcutils.so
echo "[pack] jniextra: libcutils.so 已锁定（ZL2 2.8.2 原版）"

# 1) 提取壳层 so 并 strip
python3 - "$SRC_APK" <<'EOF'
import zipfile, sys
with zipfile.ZipFile(sys.argv[1]) as z:
    with z.open("lib/arm64-v8a/libquartz_shell_lib.so") as f, open("/tmp/apkwork/libquartz_shell_lib.so","wb") as o:
        o.write(f.read())
print("[pack] so extracted")
EOF
$STRIP_BIN -o /tmp/apkwork/libquartz_shell_lib-final.so /tmp/apkwork/libquartz_shell_lib.so
echo "[pack] stripped: $(stat -c%s /tmp/apkwork/libquartz_shell_lib-final.so) bytes"

# 2) repack（保留 STORED / external_attr / META-INF services）
python3 /home/z/my-project/scripts/repack_apk.py /tmp/apkwork/quartz-repack.apk "$SRC_APK" 2>/dev/null \
  || QUARTZ_RAW_APK="$SRC_APK" python3 /home/z/my-project/scripts/repack_apk.py /tmp/apkwork/quartz-repack.apk

# 3) zipalign（-P 16：extractNativeLibs=false 的 so 页对齐）
"$BT/zipalign" -f -P 16 4 /tmp/apkwork/quartz-repack.apk /tmp/apkwork/quartz-aligned.apk

# 4) apksigner
"$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
  --ks-key-alias androiddebugkey --out "$OUT_APK" /tmp/apkwork/quartz-aligned.apk

# 5) 验证
"$BT/apksigner" verify --print-certs "$OUT_APK" | head -3
"$BT/zipalign" -c -P 16 4 "$OUT_APK" && echo "[pack] zipalign OK"
python3 - "$OUT_APK" <<'EOF'
import zipfile, sys, hashlib
p = sys.argv[1]
with zipfile.ZipFile(p) as z:
    names = z.namelist()
    assert z.testzip() is None, "corrupt"
    so_info = z.getinfo("lib/arm64-v8a/libquartz_shell_lib.so")
    services = [n for n in names if n.startswith("META-INF/services/")]
    print(f"[pack] entries={len(names)} so.compress={so_info.compress_type}(0=STORED) services={len(services)}")
    assert so_info.compress_type == 0, "so must be STORED"
    assert len(services) >= 5, "META-INF/services lost"
    # v0.2.23：ZL2 jniLibs 契约 —— libcutils.so 必须在 APK 且 STORED
    cu = z.getinfo("lib/arm64-v8a/libcutils.so")
    assert cu.compress_type == 0, "libcutils.so must be STORED"
    assert hashlib.sha256(z.read("lib/arm64-v8a/libcutils.so")).hexdigest() == \
        "04258bb1ccef8f5097ddc32ae1fb2d57142ffc5999e96fa36bfab85e02e9e347", \
        "libcutils.so sha256 mismatch in final APK"
    print("[pack] libcutils.so (ZL2 2.8.2) in APK: OK")
data = open(p, "rb").read()
print("[pack] SHA-256 =", hashlib.sha256(data).hexdigest())
print("[pack] size =", len(data), "bytes")
EOF
echo "[pack] done -> $OUT_APK"
