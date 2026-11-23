#!/bin/sh
# Quartz v9 打包收尾：strip 壳层 so -> repack -> zipalign -> apksigner -> 验证
set -e
. /home/z/my-project/scripts/setup-env.sh
BT=/home/z/my-project/.toolchain/android-sdk/build-tools/36.0.0
NDKBIN=/home/z/my-project/.toolchain/android-sdk/ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64/bin
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
"$NDKBIN/llvm-strip" -o /tmp/apkwork/libquartz_shell_lib-final.so /tmp/apkwork/libquartz_shell_lib.so
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
