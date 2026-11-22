#!/usr/bin/env python3
"""Repack the Quartz debug APK with a stripped native library.

- drops old META-INF signature entries (apksigner re-signs afterwards)
- swaps lib/arm64-v8a/libquartz_shell_lib.so for the sstripped copy
- replicates each entry's original compression method (STORED entries must
  stay STORED for extractNativeLibs=false; zipalign restores offsets)
"""
import shutil
import sys
import zipfile

SRC = "/tmp/my-project/.qz-gradle-build/app/outputs/apk/universal/debug/app-universal-debug.apk"
STRIPPED_SO = "/tmp/apkwork/libquartz_shell_lib-final.so"
DST = sys.argv[1] if len(sys.argv) > 1 else "/tmp/apkwork/quartz-repack.apk"

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

with zipfile.ZipFile(DST) as z:
    bad = z.testzip()
    assert bad is None, f"corrupt entry: {bad}"
    import os
    print(f"repacked: {DST} ({os.path.getsize(DST)/1e6:.1f} MB, {len(z.namelist())} entries)")
