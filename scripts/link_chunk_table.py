#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib"]
# ///

# Independent oracle for `just link-verify-p1`: prints the canonical chunk
# table (claude_notes/link_rendering/phase_01.md, step 4) for a .bdl file so
# it can be diffed byte-for-byte against `convert_link --info`. Offsets and
# sizes come from a plain struct walk over the raw bytes; counts come from
# gclib's parsed model where it exposes them (JNT1/SHP1/MAT3/TEX1). gclib
# leaves EVP1/DRW1 unparsed, so their u16 counts are read from gclib's chunk
# data at offset 8 (J3DModelLoader.h: mWEvlpMtxNum/mMtxNum).

import struct
import sys

from gclib import fs_helpers as fs
from gclib.j3d import BDL

COUNT_ATTRS = {
    "JNT1": "joint_count",
    "SHP1": "shape_count",
    "MAT3": "material_count",
    "TEX1": "num_textures",
}
RAW_COUNT_CHUNKS = {"EVP1", "DRW1"}


def main() -> None:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <file.bdl>", file=sys.stderr)
        sys.exit(2)
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()

    magic = data[0:4].decode("ascii")
    file_type = data[4:8].decode("ascii")
    file_size, num_chunks = struct.unpack_from(">II", data, 8)
    assert file_size == len(data), (file_size, len(data))
    print(f"{magic} {file_type} size={file_size} blocks={num_chunks}")

    bdl = BDL(path)
    counts: dict[str, int] = {}
    for chunk in bdl.chunks:
        if chunk.magic in COUNT_ATTRS:
            counts[chunk.magic] = getattr(chunk, COUNT_ATTRS[chunk.magic])
        elif chunk.magic in RAW_COUNT_CHUNKS:
            counts[chunk.magic] = fs.read_u16(chunk.data, 8)

    offset = 0x20
    for _ in range(num_chunks):
        fourcc = data[offset : offset + 4].decode("ascii")
        (size,) = struct.unpack_from(">I", data, offset + 4)
        count = counts.get(fourcc, "-")
        print(f"{fourcc} 0x{offset:06x} {size} {count}")
        offset += size
    assert offset == file_size, (offset, file_size)


if __name__ == "__main__":
    main()
