#!/usr/bin/env python3
"""最小 Java class 文件解析器：输出常量池引用 + 方法/字段表（名字/描述符/访问标志）。
用于在无 JDK 的沙箱里反查 gladiolus 合并 jar 的契约细节。"""
import struct
import sys
import zipfile

U1 = "B"; U2 = ">H"; U4 = ">I"

ACC = {0x0001: "public", 0x0002: "private", 0x0004: "protected", 0x0008: "static",
       0x0010: "final", 0x0020: "super", 0x0040: "volatile", 0x0080: "transient",
       0x0100: "native", 0x0200: "interface", 0x0400: "abstract", 0x1000: "synthetic"}


def read_cp(f, count):
    cp = [None] * count
    i = 1
    while i < count:
        tag = struct.unpack(U1, f.read(1))[0]
        if tag in (7, 8, 16, 19, 20):  # Class, String, MethodType, Module, Package
            cp[i] = (tag, struct.unpack(U2, f.read(2))[0])
        elif tag == 15:  # MethodHandle: 1B kind + 2B index
            kind = struct.unpack(U1, f.read(1))[0]
            idx = struct.unpack(U2, f.read(2))[0]
            cp[i] = (tag, kind, idx)
        elif tag in (3, 4):  # Integer/Float
            cp[i] = (tag, struct.unpack(U4, f.read(4))[0])
        elif tag in (5, 6):  # Long/Double 占两槽
            cp[i] = (tag, struct.unpack(">Q", f.read(8))[0])
            i += 1
        elif tag in (1,):  # Utf8
            ln = struct.unpack(U2, f.read(2))[0]
            cp[i] = (tag, f.read(ln).decode("utf-8", "replace"))
        elif tag in (9, 10, 11, 12, 17, 18):  # refs / NameAndType
            a, b = struct.unpack(U2, f.read(2)), struct.unpack(U2, f.read(2))
            cp[i] = (tag, a[0], b[0])
        else:
            raise ValueError(f"未知常量池 tag {tag} @ {i}")
        i += 1
    return cp


def utf(cp, idx):
    return cp[idx][1]


def cls(cp, idx):
    return utf(cp, cp[idx][1])


def flags(n):
    return " ".join(v for k, v in ACC.items() if n & k)


def parse(data):
    f = __import__("io").BytesIO(data)
    magic, = struct.unpack(U4, f.read(4))
    assert magic == 0xCAFEBABE
    f.read(4)  # minor+major
    count, = struct.unpack(U2, f.read(2))
    cp = read_cp(f, count)
    acc, = struct.unpack(U2, f.read(2))
    this_class, = struct.unpack(U2, f.read(2))
    f.read(2)  # super
    for _ in range(struct.unpack(U2, f.read(2))[0]):  # interfaces
        f.read(2)
    fields = []
    for _ in range(struct.unpack(U2, f.read(2))[0]):
        fa, = struct.unpack(U2, f.read(2))
        fname = utf(cp, struct.unpack(U2, f.read(2))[0])
        fdesc = utf(cp, struct.unpack(U2, f.read(2))[0])
        na = struct.unpack(U2, f.read(2))[0]
        for _ in range(na):
            f.read(2)
            ln = struct.unpack(U4, f.read(4))[0]
            f.read(ln)
        fields.append(f"{flags(fa)} {fdesc} {fname}")
    methods = []
    for _ in range(struct.unpack(U2, f.read(2))[0]):
        ma, = struct.unpack(U2, f.read(2))
        mname = utf(cp, struct.unpack(U2, f.read(2))[0])
        mdesc = utf(cp, struct.unpack(U2, f.read(2))[0])
        na = struct.unpack(U2, f.read(2))[0]
        code_len = None
        for _ in range(na):
            aname = utf(cp, struct.unpack(U2, f.read(2))[0])
            ln = struct.unpack(U4, f.read(4))[0]
            body = f.read(ln)
            if aname == "Code":
                code_len = ln
        methods.append((f"{flags(ma)} {mdesc} {mname}", code_len))
    return cls(cp, this_class), cp, acc, fields, methods


def resolve(cp):
    """把字段/方法里的描述符引用展开成人类可读调用目标。"""
    out = []
    for entry in cp:
        if entry is None:
            continue
        tag = entry[0]
        if tag in (9, 10, 11) and len(entry) >= 3:  # Fieldref/Methodref/InterfaceMethodref
            a, b = entry[1], entry[2]
            try:
                target = f"{cls(cp, a)}.{utf(cp, cp[b][1])}:{utf(cp, cp[b][2])}"
            except Exception:
                target = "?"
            kind = {9: "get/putfield", 10: "invoke", 11: "invkif"}[tag]
            out.append(f"  [{kind}] {target}")
    return out


def main():
    jar, *wanted = sys.argv[1:]
    z = zipfile.ZipFile(jar)
    for name in wanted:
        data = z.read(name)
        _, cp, acc, fields, methods = parse(data)
        print(f"===== {name} =====")
        print("fields:")
        for fl in fields:
            print("  " + fl)
        print("methods:")
        for m, code_len in methods:
            print("  " + m + (f"  (code {code_len}B)" if code_len else ""))
        refs = resolve(cp)
        print("refs:")
        for r in refs:
            print(r)
        print()


if __name__ == "__main__":
    main()
