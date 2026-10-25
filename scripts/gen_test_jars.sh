#!/usr/bin/env bash
# Generate synthetic NeoForge 1.21.1-style test mod jars (manifest + a valid
# class file carrying @Mod). No network, no Java toolchain needed: class
# bytes are produced by the imagine-jcf test builder format.
set -euo pipefail

OUT_DIR="${1:-testdata/out}"
mkdir -p "$OUT_DIR"

python3 - "$OUT_DIR" <<'PY'
import struct, sys, zipfile, io, os

out_dir = sys.argv[1]

def utf8(s: bytes) -> bytes:
    return struct.pack(">BH", 1, len(s)) + s

def class_ref(idx: int) -> bytes:
    return struct.pack(">BH", 7, idx)

def build_class(modid: str, this_name: str) -> bytes:
    # constant pool
    entries = bytearray()
    # 1 RuntimeVisibleAnnotations
    entries += utf8(b"RuntimeVisibleAnnotations")
    # 2 annotation descriptor (NeoForge 1.21.1)
    entries += utf8(b"Lnet/neoforged/fml/common/Mod;")
    # 3 "value"
    entries += utf8(b"value")
    # 4 modid
    entries += utf8(modid.encode())
    # 5 this class name
    entries += utf8(this_name.encode())
    # 6 class -> 5
    entries += class_ref(5)
    # 7 super name
    entries += utf8(b"java/lang/Object")
    # 8 class -> 7
    entries += class_ref(7)
    cp_count = 9

    out = bytearray()
    out += struct.pack(">I", 0xCAFEBABE)
    out += struct.pack(">HH", 0, 65)          # minor, major (Java 21)
    out += struct.pack(">H", cp_count)
    out += entries
    out += struct.pack(">H", 0x0021)          # access public super
    out += struct.pack(">H", 6)               # this_class
    out += struct.pack(">H", 8)               # super_class
    out += struct.pack(">H", 0)               # interfaces
    out += struct.pack(">H", 0)               # fields
    out += struct.pack(">H", 0)               # methods
    # one class attribute: RuntimeVisibleAnnotations
    attr = struct.pack(">H", 1)               # num annotations
    attr += struct.pack(">H", 2)              # type_index -> descriptor
    attr += struct.pack(">H", 1)              # one element-value pair
    attr += struct.pack(">H", 3)              # name "value"
    attr += b"s"
    attr += struct.pack(">H", 4)              # const value -> modid
    out += struct.pack(">H", 1)               # attributes_count
    out += struct.pack(">H", 1)               # attr name idx = 1
    out += struct.pack(">I", len(attr))
    out += attr
    return bytes(out)

def write_jar(path: str, toml: str, class_name: str, modid: str):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("META-INF/neoforge.mods.toml", toml)
        z.writestr(f"{class_name}.class", build_class(modid, class_name))
    with open(path, "wb") as f:
        f.write(buf.getvalue())

mods = [
    ("crussty-hello", "1.0.0", "CrusstyHello"),
    ("crussty-registry-test", "0.3.0", "RegistryTestMod"),
    ("crussty-events-test", "0.2.0", "EventsTestMod"),
]

for modid, version, cls in mods:
    toml = (
        f'modLoader="javafml"\n'
        f'loaderVersion="[4,)"\n'
        f'license="MIT"\n'
        f'[[mods]]\n'
        f'modId="{modid}"\n'
        f'version="{version}"\n'
        f'displayName="{modid} (c-imagine e2e)"\n'
        f'[[dependencies.{modid}]]\n'
        f'modId="neoforge"\n'
        f'type="required"\n'
        f'versionRange="[21.1.0,)"\n'
        f'ordering="NONE"\n'
        f'side="BOTH"\n'
    )
    write_jar(os.path.join(out_dir, f"{modid}-{version}.jar"), toml, cls, modid)
    print(f"generated {modid}-{version}.jar")

# deliberately broken jar: not a zip at all -> must surface as plan error
with open(os.path.join(out_dir, "broken-mod.jar"), "wb") as f:
    f.write(b"this is not a zip archive")
print("generated broken-mod.jar")
PY
