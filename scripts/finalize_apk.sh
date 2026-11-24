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
SRC_APK=${QUARTZ_RAW_APK:-/home/z/my-project/apps/shell-tauri/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk}
OUT_APK=/home/z/my-project/download/Quartz.apk
KS=/home/z/my-project/keystore/quartz-debug.keystore

mkdir -p /tmp/apkwork /home/z/my-project/download

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
data = open(p, "rb").read()
print("[pack] SHA-256 =", hashlib.sha256(data).hexdigest())
print("[pack] size =", len(data), "bytes")
EOF
echo "[pack] done -> $OUT_APK"
