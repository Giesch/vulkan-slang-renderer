# Synthetic fixture generators for the asset-free Link animation tests.
#
# Everything here is code-generated from format specs (RARC, J3D1, ANK1,
# TPT1, TTK1, ResNTAB) — no game bytes. The builders are independent of both
# the Rust converter and the oracle so tests can use them as ground to diff
# those implementations against.
#
# Part of the `link-test-animations` suite; run via scripts/tests/run_tests.py.

from __future__ import annotations

import struct
from io import BytesIO
from pathlib import Path


def be16(v: int) -> bytes:
    return struct.pack(">H", v & 0xFFFF)


def be32(v: int) -> bytes:
    return struct.pack(">I", v)


def bef32(v: float) -> bytes:
    return struct.pack(">f", v)


def pad32(chunk: bytes) -> bytes:
    while len(chunk) % 0x20 != 0:
        chunk += b"\0"
    return chunk


def name_table(names: list[str]) -> bytes:
    """ResNTAB: u16 count, u16 0xFFFF pad, {u16 hash, u16 offset} entries,
    then NUL-terminated strings; offsets relative to the table start."""
    out = bytearray()
    out += be16(len(names))
    out += be16(0xFFFF)
    data_off = 4 + 4 * len(names)
    entries = bytearray()
    strings = bytearray()
    for n in names:
        entries += be16(0)  # hash (unchecked on read)
        entries += be16(data_off)
        strings += n.encode("ascii") + b"\0"
        data_off += len(n) + 1
    out += entries
    out += strings
    return bytes(out)


def j3d_file(file_type: bytes, chunk: bytes) -> bytes:
    """A single-chunk J3D1 file (no BAS trailer). Writes the chunk size
    word (padded chunk length) into the chunk header."""
    chunk = bytearray(chunk)
    chunk[4:8] = be32(len(chunk))
    header = bytearray(0x20)
    header[0:4] = b"J3D1"
    header[4:8] = file_type
    header[8:12] = be32(0x20 + len(chunk))
    header[12:16] = be32(1)
    header[0x1C:0x20] = be32(0xFFFFFFFF)
    return bytes(header) + bytes(chunk)


# --- ANK1 (BCK) ------------------------------------------------------------------


def default_axis() -> dict:
    """All-default descriptors (count 0)."""
    return {"s": (0, 0, 0), "r": (0, 0, 0), "t": (0, 0, 0)}


def bck_chunk(
    joints: list[list[dict]],
    *,
    loop: int = 2,
    rotation_shift: int = 0,
    duration: int = 6,
    scale_pool: list[float] | None = None,
    rotation_pool: list[int] | None = None,
    translation_pool: list[float] | None = None,
) -> bytes:
    """joints: per joint, three axis dicts {s,r,t} of (count, index,
    tangent_type) descriptors, axis-major (x, y, z). Pools are word lists."""
    scale_pool = scale_pool or []
    rotation_pool = rotation_pool or []
    translation_pool = translation_pool or []
    header = bytearray(0x24)
    header[0:4] = b"ANK1"
    header[8] = loop
    header[9] = rotation_shift
    header[0x0A:0x0C] = be16(duration)
    header[0x0C:0x0E] = be16(len(joints))
    header[0x0E:0x10] = be16(len(scale_pool))
    header[0x10:0x12] = be16(len(rotation_pool))
    header[0x12:0x14] = be16(len(translation_pool))
    tables = bytearray()
    for axes in joints:
        for axis in axes:  # x, y, z — {S,R,T} per axis
            for key in ("s", "r", "t"):
                count, index, tangent = axis[key]
                tables += be16(count) + be16(index) + be16(tangent)
    header[0x14:0x18] = be32(0x24)
    pools = bytearray()
    scale_off = 0x24 + len(tables)
    for v in scale_pool:
        pools += bef32(v)
    rot_off = scale_off + 4 * len(scale_pool)
    for v in rotation_pool:
        pools += be16(v)
    trans_off = rot_off + 2 * len(rotation_pool)
    for v in translation_pool:
        pools += bef32(v)
    header[0x18:0x1C] = be32(scale_off)
    header[0x1C:0x20] = be32(rot_off)
    header[0x20:0x24] = be32(trans_off)
    return pad32(bytes(header) + bytes(tables) + bytes(pools))


def bck_with_bas(chunk: bytes, entries: int, truncate: bool = False) -> bytes:
    """Append a BAS trailer (u16 count + 0x20/entry) and point the header at
    it. `truncate` cuts the trailer short to make an invalid file."""
    file = bytearray(j3d_file(b"bck1", chunk))
    off = len(file)
    file[0x1C:0x20] = be32(off)
    trailer = bytearray(be16(entries) + b"\0" * 6)
    trailer += b"\0" * (0x20 * entries)
    if truncate:
        trailer = trailer[: max(1, len(trailer) // 2)]
    file += trailer
    file[8:12] = be32(len(file))
    return bytes(file)


# --- TPT1 (BTP) --------------------------------------------------------------------


def btp_chunk(rows: list[dict], *, loop: int = 2, duration: int = 1) -> bytes:
    """rows: {material, remap, texno, samples: list[int]} — order preserved,
    duplicate materials allowed. All offsets are chunk-relative."""
    names = [r["material"] for r in rows]
    values: list[int] = []
    table = bytearray()
    for r in rows:
        index = len(values)
        values.extend(r["samples"])
        table += be16(len(r["samples"])) + be16(index)
        table += bytes([r["texno"], 0]) + be16(0)
    remap = b"".join(be16(r["remap"]) for r in rows)
    names_bytes = name_table(names)

    header = bytearray(0x20)
    header[0:4] = b"TPT1"
    header[8] = loop
    header[9] = 0xFF
    header[0x0A:0x0C] = be16(duration)
    header[0x0C:0x0E] = be16(len(rows))
    header[0x0E:0x10] = be16(len(values))
    values_off = 0x20 + len(table)
    remap_off = values_off + 2 * len(values)
    names_off = remap_off + len(remap)
    header[0x10:0x14] = be32(0x20)
    header[0x14:0x18] = be32(values_off)
    header[0x18:0x1C] = be32(remap_off)
    header[0x1C:0x20] = be32(names_off)
    return pad32(bytes(header) + bytes(table) + b"".join(be16(v) for v in values) + remap + names_bytes)


# --- TTK1 (BTK) ----------------------------------------------------------------------


def btk_chunk(
    targets: list[dict],
    *,
    post_targets: list[dict] | None = None,
    loop: int = 2,
    rotation_shift: int = 1,
    duration: int = 20,
    matrix_calc: int = 0,
) -> bytes:
    """targets: {material, remap, texgen, center [f,f,f], axes: 3 x {s,r,t}
    descriptors, scale_pool/rotation_pool/translation_pool} (pools shared by
    all targets in a set; tests set them on every target)."""
    header = bytearray(0x60)
    header[0:4] = b"TTK1"
    header[8] = loop
    header[9] = rotation_shift
    header[0x0A:0x0C] = be16(duration)
    header[0x0C:0x0E] = be16(3 * len(targets))
    body = bytearray()

    def build_set(ts: list[dict]) -> tuple[bytes, dict[str, int]]:
        tables = bytearray()
        for t in ts:
            for axis in t["axes"]:
                for key in ("s", "r", "t"):
                    count, index, tangent = axis[key]
                    tables += be16(count) + be16(index) + be16(tangent)
        remap = b"".join(be16(t["remap"]) for t in ts)
        names = name_table([t["material"] for t in ts])
        selectors = bytes(t["texgen"] for t in ts)
        centers = bytearray()
        for t in ts:
            for v in t["center"]:
                centers += bef32(v)
        scale = b"".join(bef32(v) for v in ts[0].get("scale_pool", []))
        rot = b"".join(be16(v) for v in ts[0].get("rotation_pool", []))
        trans = b"".join(bef32(v) for v in ts[0].get("translation_pool", []))
        data = bytes(tables) + remap + names + selectors + bytes(centers) + scale + rot + trans
        sizes = {
            "tables": len(tables),
            "remap": len(remap),
            "names": len(names),
            "selectors": len(selectors),
            "centers": len(centers),
            "scale": len(scale),
            "rot": len(rot),
            "trans": len(trans),
        }
        return data, sizes

    def _set_offsets(base: int, sizes: dict[str, int]) -> list[int]:
        order = ["tables", "remap", "names", "selectors", "centers", "scale", "rot", "trans"]
        out = []
        o = base
        for key in order:
            out.append(o)
            o += sizes[key]
        return out

    counts = {
        "scale": len(targets[0].get("scale_pool", [])),
        "rot": len(targets[0].get("rotation_pool", [])),
        "trans": len(targets[0].get("translation_pool", [])),
    }
    main, sizes = build_set(targets)
    header[0x0E:0x10] = be16(counts["scale"])
    header[0x10:0x12] = be16(counts["rot"])
    header[0x12:0x14] = be16(counts["trans"])
    offs = _set_offsets(0x60, sizes)
    for field, value in zip((0x14, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C, 0x30), offs):
        header[field : field + 4] = be32(value)
    body += main

    if post_targets is not None:
        post, post_sizes = build_set(post_targets)
        post_counts = {
            "scale": len(post_targets[0].get("scale_pool", [])),
            "rot": len(post_targets[0].get("rotation_pool", [])),
            "trans": len(post_targets[0].get("translation_pool", [])),
        }
        post_offs = _set_offsets(0x60 + len(main), post_sizes)
        header[0x34:0x36] = be16(3 * len(post_targets))
        header[0x36:0x38] = be16(post_counts["scale"])
        header[0x38:0x3A] = be16(post_counts["rot"])
        header[0x3A:0x3C] = be16(post_counts["trans"])
        for field, value in zip((0x3C, 0x40, 0x44, 0x48, 0x4C, 0x50, 0x54, 0x58), post_offs):
            header[field : field + 4] = be32(value)
        body += post
    header[0x5C:0x60] = be32(matrix_calc)
    return pad32(bytes(header) + bytes(body))


# --- synthetic cl.bdl --------------------------------------------------------------


def cl_bdl(joint_count: int, material_names: list[str]) -> bytes:
    """J3D2bdl4 with a real JNT1 and a zero-material MAT3 whose name table
    carries the material names. gclib parses both; zero materials means no
    0x14C material records need synthesizing, and the indirect list offset
    is aliased to the name-table offset so gclib skips the indirect pass."""
    # JNT1: count@0x08, joint data@0x0C, name table@0x14; joints 0x40 each.
    joint_record = bytearray(0x40)
    joint_record[3] = 0xFF  # Joint._padding_1
    joint_record[0x16:0x18] = b"\xFF\xFF"  # Joint._padding_2
    joints = bytes(joint_record) * joint_count
    jnt_names = name_table([f"jnt_{i}" for i in range(joint_count)])
    jnt1 = bytearray(0x18)
    jnt1[0:4] = b"JNT1"
    jnt1[8:0x0A] = be16(joint_count)
    jnt1[0x0C:0x10] = be32(0x18)
    jnt1[0x14:0x18] = be32(0x18 + len(joints))
    jnt1 = pad32(bytes(jnt1) + joints + jnt_names)
    jnt1 = b"JNT1" + be32(len(jnt1)) + jnt1[8:]

    # MAT3: chunk magic+size, count 0, pad, 30 section offsets, name table.
    # Offsets stored in the section table are relative to the *chunk* start
    # (magic+size included), so the name table sits after the 8-byte prefix.
    body = bytearray()
    body += be16(0)
    body += be16(0xFFFF)
    names_off = 8 + 4 + 4 * 30
    offsets = [0] * 30
    offsets[2] = names_off  # mat_names_table_offset
    offsets[3] = names_off  # indirect_list_offset aliased -> skipped
    for v in offsets:
        body += be32(v)
    body += name_table(material_names)
    body = pad32(bytes(body))
    mat3 = b"MAT3" + be32(8 + len(body)) + bytes(body)

    bdl = bytearray(0x20)
    bdl[0:4] = b"J3D2"
    bdl[4:8] = b"bdl4"
    bdl[12:16] = be32(2)
    bdl += jnt1
    bdl += mat3
    bdl[8:12] = be32(len(bdl))
    return bytes(bdl)


# --- RARC ---------------------------------------------------------------------------


def rarc(members: dict[str, bytes], *, compress: set[str] | None = None) -> bytes:
    """Build a minimal RARC from {relative path: content}, one level of
    subdirectories (the animation archives' layout)."""
    compress = compress or set()
    dirs: dict[str, list[str]] = {"": []}
    for rel in members:
        if "/" not in rel:
            # Root-level member: partition("/") would return the whole name
            # as the "directory" and drop the file.
            dirs[""].append(rel)
            continue
        head, _, name = rel.partition("/")
        dirs.setdefault(head, [])
        if name:
            dirs[head].append(name)

    nodes: list[dict] = [{"name": "archive", "files": dirs[""]}]
    for d in sorted(set(dirs) - {""}):
        nodes.append({"name": d, "files": dirs[d]})

    # String pool: node names, "." / "..", then member names.
    ordered: list[str] = ["." , ".."]
    for n in nodes:
        if n["name"] not in ordered:
            ordered.append(n["name"])
    for rel in sorted(members):
        nm = rel.partition("/")[2] if "/" in rel else rel
        if nm not in ordered:
            ordered.append(nm)
    strings: dict[str, int] = {}
    off = 0
    for s in ordered:
        strings[s] = off
        off += len(s) + 1
    string_data = b"".join(s.encode("ascii") + b"\0" for s in ordered)

    entries: list[dict] = []
    next_file_id = 0

    def emit(name: str, is_dir: bool, node_index: int, data: bytes | None, size: int) -> None:
        nonlocal next_file_id
        entries.append(
            {
                "id": 0xFFFF if is_dir else next_file_id,
                "name": name,
                "type": 0x02 if is_dir else 0x11,
                "name_offset": strings[name],
                "node_index": node_index,
                "data": data,
                "size": size,
            }
        )
        if not is_dir:
            next_file_id += 1

    for n_i, node in enumerate(nodes):
        node["first_index"] = len(entries)
        emit(".", True, n_i, None, 0x10)
        emit("..", True, 0 if n_i != 0 else 0xFFFFFFFF, None, 0x10)
        if n_i == 0:
            for child_i in range(1, len(nodes)):
                emit(nodes[child_i]["name"], True, child_i, None, 0x10)
        for name in sorted(node["files"]):
            rel = f"{node['name']}/{name}" if n_i != 0 else name
            data = members[rel]
            if rel in compress or name in compress:
                data = yaz0_compress(data)
            emit(name, False, 0, data, len(data))
        node["num_files"] = len(entries) - node["first_index"]

    file_data = bytearray()
    data_offsets: dict[int, int] = {}
    for i, e in enumerate(entries):
        if e["type"] == 0x02:
            continue
        data_offsets[i] = len(file_data)
        file_data += e["data"]
        while len(file_data) % 0x20 != 0:
            file_data += b"\0"

    num_nodes = len(nodes)
    node_list_off = 0x40
    entries_off = node_list_off + 0x10 * num_nodes
    strings_off = entries_off + 0x14 * len(entries)
    file_data_off = strings_off + len(string_data)
    while file_data_off % 0x20 != 0:
        string_data += b"\0"
        file_data_off += 1

    node_bytes = bytearray()
    for i, n in enumerate(nodes):
        node_type = b"ROOT" if i == 0 else n["name"].upper().encode("ascii")[:4].ljust(4, b" ")
        node_bytes += node_type
        node_bytes += be32(strings[n["name"]])
        node_bytes += be16(0)
        node_bytes += be16(n["num_files"])
        node_bytes += be32(n["first_index"])
    entry_bytes = bytearray()
    for i, e in enumerate(entries):
        entry_bytes += be16(e["id"])
        entry_bytes += be16(0)  # name hash (unchecked on read)
        entry_bytes += be32((e["type"] << 24) | e["name_offset"])
        entry_bytes += be32(e["node_index"] if e["type"] == 0x02 else data_offsets[i])
        entry_bytes += be32(e["size"])
        entry_bytes += be32(0)  # entries are 0x14 bytes; the 5th word is pad

    total = file_data_off + len(file_data)
    header = bytearray(0x20)
    header[0:4] = b"RARC"
    header[4:8] = be32(total)
    header[8:12] = be32(0x20)
    header[12:16] = be32(file_data_off - 0x20)
    header[16:20] = be32(len(file_data))
    header[20:24] = be32(len(file_data))  # all MRAM
    data_header = bytearray(0x20)
    data_header[0:4] = be32(num_nodes)
    data_header[4:8] = be32(node_list_off - 0x20)
    data_header[8:12] = be32(len(entries))
    data_header[12:16] = be32(entries_off - 0x20)
    data_header[16:20] = be32(len(string_data))
    data_header[20:24] = be32(strings_off - 0x20)
    data_header[24:26] = be16(next_file_id)
    data_header[26] = 1
    out = (
        bytes(header)
        + bytes(data_header)
        + b"\0" * (node_list_off - 0x40)
        + bytes(node_bytes)
        + bytes(entry_bytes)
        + string_data
        + bytes(file_data)
    )
    assert len(out) == total, (len(out), total)
    return out


def yaz0_compress(data: bytes) -> bytes:
    """Minimal valid Yaz0 stream: every group is 0xFF (all-literal) + eight
    bytes. Correct, if unoptimized; gclib and the extraction reader both
    decompress it back to exactly `data`."""
    out = bytearray()
    out += b"Yaz0"
    out += be32(len(data))
    out += be32(0x10)
    out += be32(0)
    pos = 0
    while pos < len(data):
        out += b"\xFF"
        out += data[pos : pos + 8]
        out += b"\0" * max(0, 8 - (len(data) - pos))
        pos += 8
    return bytes(out)


def yaz0_roundtrip(data: bytes) -> bytes:
    """Compress + decompress via gclib; used by tests to validate the
    all-literal encoder before fixtures rely on it."""
    from gclib.yaz0_yay0 import Yaz0

    return Yaz0.decompress(BytesIO(yaz0_compress(data))).getvalue()


FAKE_DTK = """#!/usr/bin/env bash
# Fake dtk for the asset-free test harness: serves fixture files from
# $FAKE_DTK_ROOT by basename of the requested VFS path.
set -euo pipefail
if [[ "${1:-}" != "vfs" || "${2:-}" != "cp" ]]; then
  echo "fake dtk: unsupported invocation: $*" >&2
  exit 3
fi
src="$3"
dest="$4"
name="${src##*:}"
name="${name##*/}"
root="${FAKE_DTK_ROOT:?FAKE_DTK_ROOT is unset}"
src_file="$root/$name"
if [[ ! -f "$src_file" ]]; then
  echo "fake dtk: no fixture for $src" >&2
  exit 4
fi
mkdir -p "$(dirname "$dest")"
cp -- "$src_file" "$dest"
"""


def write_fake_dtk(directory: Path) -> Path:
    path = directory / "dtk"
    path.write_text(FAKE_DTK)
    path.chmod(0o755)
    return path
