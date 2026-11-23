#!/usr/bin/env python3
"""下载 IronizedZink APK 并解析 lib/arm64-v8a/*.so 的 DT_NEEDED 依赖与 SONAME。"""
import struct, sys, zipfile, os, urllib.request

APK_URL = "https://github.com/GoyDevv/IronizedZink/releases/download/v1.1.0/Ironized-Zink-v1.1.0.apk"
OUT = "/tmp/zinkwork/Ironized-Zink-v1.1.0.apk"
os.makedirs("/tmp/zinkwork", exist_ok=True)

def get_needed_and_soname(path):
    """解析 ELF64 的 DT_NEEDED / DT_SONAME / 是否链接 libc++_shared 等。"""
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"\x7fELF":
        return None, None, "not-elf"
    # ELF64 little-endian
    e_shoff = struct.unpack_from("<Q", data, 0x28)[0]
    e_shentsize = struct.unpack_from("<H", data, 0x3A)[0]
    e_shnum = struct.unpack_from("<H", data, 0x3C)[0]
    e_shstrndx = struct.unpack_from("<H", data, 0x3E)[0]
    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size, sh_link, _, sh_align, sh_entsize = struct.unpack_from("<IIQQQQIIQQ", data, off)
        sections.append(dict(name=sh_name, type=sh_type, offset=sh_offset, size=sh_size, link=sh_link))
    # shstrtab
    shstr = sections[e_shstrndx]
    strtab_data = data[shstr["offset"]:shstr["offset"]+shstr["size"]]
    def cstr(blob, off):
        end = blob.find(b"\x00", off)
        return blob[off:end].decode("utf-8", "replace")
    dynsym = None; dynstr = None; dynamic = None
    for s in sections:
        nm = cstr(strtab_data, s["name"])
        if nm == ".dynsym": dynsym = s
        elif nm == ".dynstr": dynstr = s
        elif nm == ".dynamic": dynamic = s
    needed, soname = [], None
    if dynstr is None or dynamic is None:
        return needed, soname, "no-dynamic"
    dynstr_data = data[dynstr["offset"]:dynstr["offset"]+dynstr["size"]]
    d = dynamic
    blob = data[d["offset"]:d["offset"]+d["size"]]
    for i in range(len(blob)//16):
        tag, val = struct.unpack_from("<qQ", blob, i*16)
        if tag == 1:  # DT_NEEDED
            needed.append(cstr(dynstr_data, val))
        elif tag == 14:  # DT_SONAME
            soname = cstr(dynstr_data, val)
    return needed, soname, "ok"

if not os.path.exists(OUT):
    print("downloading APK...")
    urllib.request.urlretrieve(APK_URL, OUT)
    print("size:", os.path.getsize(OUT))

z = zipfile.ZipFile(OUT)
print("=== APK lib/arm64-v8a entries ===")
libs = [n for n in z.namelist() if n.startswith("lib/arm64-v8a/") and n.endswith(".so")]
for n in libs:
    print(" ", n, z.getinfo(n).file_size)

print("\n=== DT_NEEDED / SONAME per lib ===")
info = {}
for n in libs:
    dest = "/tmp/zinkwork/" + os.path.basename(n)
    if not os.path.exists(dest):
        with z.open(n) as f, open(dest, "wb") as o:
            o.write(f.read())
    needed, soname, st = get_needed_and_soname(dest)
    info[os.path.basename(n)] = (needed, soname)
    print(f"--- {os.path.basename(n)} (SONAME={soname})")
    for d in needed or []:
        print(f"    NEEDED {d}")

# 提取 assets 里是否有额外 so（有些 zink 插件把 LLVM 等放 assets）
print("\n=== assets 中疑似库文件 ===")
for n in z.namelist():
    if "assets" in n and (n.endswith(".so") or "llvm" in n.lower() or "mesa" in n.lower()):
        print(" ", n, z.getinfo(n).file_size)
