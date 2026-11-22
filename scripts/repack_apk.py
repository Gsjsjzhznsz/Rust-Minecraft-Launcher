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
STRIPPED_SO = "/tmp/apkwork/libquartz_shell_lib.so"
DST = sys.argv[1] if len(sys.argv) > 1 else "/tmp/apkwork/quartz-repack.apk"

SKIP_PREFIXES = ("META-INF/",)

with zipfile.ZipFile(SRC) as zin, zipfile.ZipFile(DST, "w", allowZip64=True) as zout:
    for info in zin.infolist():
        name = info.filename
        if any(name.startswith(p) for p in SKIP_PREFIXES):
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
