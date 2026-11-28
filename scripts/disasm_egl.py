#!/usr/bin/env python3
"""Disassemble aarch64 functions from libEGL_mesa.so with capstone.
用法: python3 scripts/disasm_egl.py <so路径> <start_vaddr> <end_vaddr>
"""
import struct, sys
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_LITTLE_ENDIAN

SO = sys.argv[1] if len(sys.argv) > 1 else "/tmp/zl2libs/lib/arm64-v8a/libEGL_mesa.so"
START = int(sys.argv[2], 0)
END = int(sys.argv[3], 0)

data = open(SO, "rb").read()
e_phoff = struct.unpack_from("<Q", data, 0x20)[0]
e_phentsize = struct.unpack_from("<H", data, 0x36)[0]
e_phnum = struct.unpack_from("<H", data, 0x38)[0]
loads = []
for i in range(e_phnum):
    off = e_phoff + i * e_phentsize
    p_type = struct.unpack_from("<I", data, off)[0]
    if p_type == 1:
        p_offset, p_vaddr, _paddr, p_filesz = struct.unpack_from("<QQQQ", data, off + 8)
        loads.append((p_vaddr, p_offset, p_filesz))

def v2o(v):
    for pv, po, psz in loads:
        if pv <= v < pv + psz:
            return po + (v - pv)
    raise ValueError(f"vaddr {v:#x} not mapped")

md = Cs(CS_ARCH_ARM64, CS_MODE_LITTLE_ENDIAN)

code = data[v2o(START):v2o(END)]
for ins in md.disasm(code, START):
    print(f"{ins.address:#08x}  {ins.mnemonic:<10} {ins.op_str}")
