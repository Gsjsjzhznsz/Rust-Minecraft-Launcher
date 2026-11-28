#!/usr/bin/env python3
"""Repack the Quartz debug APK with a stripped native library.

- drops old META-INF signature entries (apksigner re-signs afterwards)
- swaps lib/arm64-v8a/libquartz_shell_lib.so for the sstripped copy
- replicates each entry's original compression method (STORED entries must
  stay STORED for extractNativeLibs=false; zipalign restores offsets)
- v0.2.23: injects extra jniLibs from /tmp/apkwork/jniextra/*.so into
  lib/arm64-v8a/ as STORED entries (currently ZL2 2.8.2 原版 libcutils.so
  —— 无 DT_SONAME 的按名依赖必须住在 nativeLibraryDir，见 game_bridge.rs
  v0.2.23 机制注记)
"""
import glob
import os
import shutil
import sys
import zipfile

def _raw_apk() -> str:
    if os.environ.get("QUARTZ_RAW_APK"):
        return os.environ["QUARTZ_RAW_APK"]
    for cand in (
        "/home/z/my-project/apps/shell-tauri/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk",
        "/tmp/my-project/.qz-gradle-build/app/outputs/apk/universal/debug/app-universal-debug.apk",
    ):
        if os.path.exists(cand):
            return cand
    raise SystemExit("raw APK not found (set QUARTZ_RAW_APK)")

SRC = _raw_apk()
STRIPPED_SO = "/tmp/apkwork/libquartz_shell_lib-final.so"
DST = sys.argv[1] if len(sys.argv) > 1 else "/tmp/apkwork/quartz-repack.apk"

# v0.2.23：额外 jniLibs（STORED 注入 lib/arm64-v8a/；zipalign -P 16 在
# finalize_apk.sh 后续步骤统一做页对齐）
JNI_EXTRA_DIR = "/tmp/apkwork/jniextra"
extra_libs = sorted(glob.glob(os.path.join(JNI_EXTRA_DIR, "*.so")))

# 只剔除旧的签名文件；META-INF 其余条目（services/、androidx 版本标记等）
# 是运行时 ServiceLoader 依赖，绝不能丢——上一版全删导致 coroutines
# MainDispatcherFactory 加载不到，应用启动即闪退。
def is_signature(name: str) -> bool:
    if not name.startswith("META-INF/"):
        return False
    leaf = name.rsplit("/", 1)[-1]
    return leaf in ("MANIFEST.MF",) or leaf.endswith((".SF", ".RSA", ".EC"))

with zipfile.ZipFile(SRC) as zin, zipfile.ZipFile(DST, "w", allowZip64=True) as zout:
    for info in zin.infolist():
        name = info.filename
        if is_signature(name):
            continue
        if name == "lib/arm64-v8a/libquartz_shell_lib.so":
            with open(STRIPPED_SO, "rb") as f:
                data = f.read()
            zi = zipfile.ZipInfo(name, date_time=info.date_time)
            zi.compress_type = info.compress_type  # keep STORED
            zi.external_attr = info.external_attr
            zout.writestr(zi, data)
            continue
        data = zin.read(name)
        zi = zipfile.ZipInfo(name, date_time=info.date_time)
        zi.compress_type = info.compress_type
        zi.external_attr = info.external_attr
        zout.writestr(zi, data)
    # v0.2.23：jniLibs 注入（若目标条目已存在则报错防重复）
    existing = set(zipfile.ZipFile(SRC).namelist())
    for p in extra_libs:
        target = f"lib/arm64-v8a/{os.path.basename(p)}"
        if target in existing:
            raise SystemExit(f"jniextra 冲突：{target} 已在原 APK 中")
        with open(p, "rb") as f:
            data = f.read()
        zi = zipfile.ZipInfo(target, date_time=(2026, 1, 1, 0, 0, 0))
        zi.compress_type = zipfile.ZIP_STORED  # extractNativeLibs=false 要求
        zi.external_attr = 0o644 << 16
        zout.writestr(zi, data)
        print(f"injected: {target} ({len(data)} bytes, STORED)")

with zipfile.ZipFile(DST) as z:
    bad = z.testzip()
    assert bad is None, f"corrupt entry: {bad}"
    import os
    print(f"repacked: {DST} ({os.path.getsize(DST)/1e6:.1f} MB, {len(z.namelist())} entries)")
