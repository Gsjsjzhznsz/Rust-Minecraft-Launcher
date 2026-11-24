#!/usr/bin/env python3
"""反汇编 liblwjgl.so（aarch64）回调关键路径（符号表来自 readelf，反汇编用 capstone）。
用途：定位第一个鼠标回调崩溃（pc=0, lr=匿名段+0x99a8）的可疑 blr。"""
import re
import subprocess
import sys

from capstone import Cs, CS_ARCH_ARM64, CS_MODE_LITTLE_ENDIAN

SO = sys.argv[1] if len(sys.argv) > 1 else ".qz-tmp/gladiolus/lib/arm64-v8a/liblwjgl.so"

out = subprocess.run(["readelf", "-sW", SO], capture_output=True, text=True).stdout
syms = []
for line in out.splitlines():
    # 1330: 0000000000054304    52 FUNC    GLOBAL DEFAULT   14 name[@@VER]
    m = re.match(r"\s*\d+:\s+([0-9a-f]{16})\s+(\d+)\s+FUNC\s+\S+\s+\S+\s+\S+\s+(\S+?)(@@\S+)?\s*$", line)
    if m:
        syms.append((m.group(3), int(m.group(1), 16), int(m.group(2))))
print(f"FUNCS: {len(syms)}", file=sys.stderr)

with open(SO, "rb") as f:
    data = f.read()

md = Cs(CS_ARCH_ARM64, CS_MODE_LITTLE_ENDIAN)
md.skipdata = True


def disasm(addr, size, note=""):
    print(f"----- {note or hex(addr)} ({size}B) @ {addr:#x} -----")
    n = 0
    for ins in md.disasm(data[addr:addr + size], addr):
        print(f"  {ins.address:#08x}: {ins.mnemonic:10} {ins.op_str}")
        n += 1
        if n > 90:
            print("  ...")
            break
    print()


def find(name, exact=True):
    for s in syms:
        if s[0] == name:
            return s
    for s in syms:
        if name in s[0]:
            return s
    return None


def main():
    # 1) getCallbackHandler 本体（返回 handler 地址）
    s = find("Java_org_lwjgl_system_Callback_getCallbackHandler")
    if s:
        disasm(s[1], s[2], s[0])
    # 2) libffi 关键符号
    for name in ["ffi_prep_closure_loc", "ffi_closure_alloc", "ffi_closure_SYSV",
                 "ffi_closure_SYSV_inner", "ffi_prep_cif", "ffi_tramp"]:
        s = find(name)
        if s:
            disasm(s[1], min(s[2], 160), f"{s[0]} ({s[2]}B)")
    # 3) 含 handler/upcall 字样的静态函数
    for s in syms:
        if s[0] and ("handler" in s[0].lower() or "upcall" in s[0].lower()):
            disasm(s[1], min(s[2], 220), f"{s[0]} ({s[2]}B)")


if __name__ == "__main__":
    main()
