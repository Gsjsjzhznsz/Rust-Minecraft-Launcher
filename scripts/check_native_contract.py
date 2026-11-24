#!/usr/bin/env python3
"""扫描合并 jar 全部 class，列出 native 方法声明；
与设备桥（libpojavexec.so）导出符号 diff —— 找漏配。"""
import re
import struct
import subprocess
import sys
import zipfile

sys.path.insert(0, "/home/z/my-project/scripts")
from classfile_dump import parse  # 复用解析器

JAR = sys.argv[1]
SO = sys.argv[2]

z = zipfile.ZipFile(JAR)
declared = {}
for name in z.namelist():
    if not name.endswith(".class"):
        continue
    try:
        cls, cp, acc, fields, methods = parse(z.read(name))
    except Exception as e:
        print(f"!! 解析失败 {name}: {e}")
        continue
    for m, _ in methods:
        parts = m.split()
        if "native" in parts and "abstract" not in parts:
            # m = "<flags> <descriptor> <name>"
            desc = parts[parts.index("native") + 1] if False else m.split()[-2]
            mname = m.split()[-1]
            declared.setdefault(cls, []).append(f"{mname}{desc}")

out = subprocess.run(["nm", "-D", "--defined-only", SO], capture_output=True, text=True).stdout
exports = set()
for line in out.splitlines():
    m = re.search(r"\s([A-Za-z_]\w*)$", line)
    if m:
        exports.add(m.group(1))

print(f"== jar native 声明共 {sum(len(v) for v in declared.values())} 个 ==")
missing = []
for cls, ms in sorted(declared.items()):
    jcls = cls.replace("/", ".")
    for sig in ms:
        mname = sig.split("(")[0]
        # JNI 短名：Java_ + 类路径下划线 + 方法名（重载需长名，这里全列出）
        short = f"Java_{cls.replace('/', '_')}_{mname}"
        ok = short in exports
        if not ok:
            missing.append((jcls, sig, short))
        print(f"  {'OK ' if ok else 'MISS'} {jcls}.{sig}")

print(f"\n== 缺失 {len(missing)} 个 ==")
