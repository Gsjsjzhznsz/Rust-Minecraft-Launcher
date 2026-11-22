#!/usr/bin/env python3
"""Super-strip aarch64 ELF: keep only loader-relevant bytes.

Bionic's linker loads shared objects exclusively via program headers
(PT_LOAD / PT_DYNAMIC / PT_NOTE / PT_ARM_EXIDX ...). Section headers and
non-alloc sections (.debug_info, .symtab ...) are pure tooling metadata;
truncating them keeps the library fully functional at runtime.

Usage: python3 sstrip_elf.py <in.so> <out.so>
"""
import struct
import sys


def sstrip(data: bytes) -> bytes:
    if data[:6] != b"\x7fELF\x02\x01":
        raise SystemExit("not a 64-bit little-endian ELF")
    e_phoff = struct.unpack_from("<Q", data, 0x20)[0]
    e_shoff = struct.unpack_from("<Q", data, 0x28)[0]
    e_phentsize = struct.unpack_from("<H", data, 0x36)[0]
    e_phnum = struct.unpack_from("<H", data, 0x38)[0]
    if e_phoff == 0 or e_phnum == 0:
        raise SystemExit("ELF has no program headers")

    keep_end = e_phoff + e_phnum * e_phentsize
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type, p_flags = struct.unpack_from("<IH", data, off)
        p_offset, p_vaddr, p_paddr, p_filesz = struct.unpack_from("<QQQQ", data, off + 8)
        if p_type == 0 or p_filesz == 0:
            continue
        keep_end = max(keep_end, p_offset + p_filesz)

    out = bytearray(data[:keep_end])
    # zero the section-header table pointers: loader ignores them entirely
    struct.pack_into("<Q", out, 0x28, 0)   # e_shoff
    struct.pack_into("<H", out, 0x3A, 0)   # e_shentsize
    struct.pack_into("<H", out, 0x3C, 0)   # e_shnum
    struct.pack_into("<H", out, 0x3E, 0)   # e_shstrndx
    return bytes(out)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as f:
        raw = f.read()
    stripped = sstrip(raw)
    with open(dst, "wb") as f:
        f.write(stripped)
    print(f"{src}: {len(raw)/1e6:.1f} MB -> {len(stripped)/1e6:.1f} MB")
