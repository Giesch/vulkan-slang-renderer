#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034"]
# ///

# Independent oracle for `just link-verify-geometry`: prints the canonical
# geometry table (claude_notes/link_rendering/phase_03.md, step 4;
# src/bin/convert_link/bmd/geometry_dump.rs is the format spec) so it diffs
# byte-for-byte against `convert_link --dump-geometry`. gclib supplies the
# INF1/VTX1/JNT1 fields; EVP1/DRW1/SHP1 are independent struct walks over the
# raw chunk bytes (including SHP1's display-list decoder), so the oracle never
# leans on our Rust for the sections where it has the most room to err.

import struct
import sys

from gclib.j3d import BDL

# --- formatting (must match geometry_dump.rs exactly) -----------------------


def f(v: float) -> str:
    return f"{v:.6f}"


def f3(v) -> str:
    return f"{f(v[0])},{f(v[1])},{f(v[2])}"


def s16(v: int) -> int:
    """gclib stores joint rotations unsigned; the field is signed s16."""
    return v - 0x10000 if v >= 0x8000 else v


def u8(d, o):
    return d[o]


def u16(d, o):
    return struct.unpack_from(">H", d, o)[0]


def u32(d, o):
    return struct.unpack_from(">I", d, o)[0]


def f32(d, o):
    return struct.unpack_from(">f", d, o)[0]


# gclib enum name → our canonical spelling
NODE_TYPE = {
    "JOINT": "JOINT",
    "MATERIAL": "MATERIAL",
    "SHAPE": "SHAPE",
    "OPEN_CHILD": "OPEN",
    "CLOSE_CHILD": "CLOSE",
    "FINISH": "FINISH",
}
ATTR = {"Position": "POS", "Normal": "NRM", "Tex0": "TEX0"}
COMP_TYPE = {"Float32": "F32", "Signed16": "S16"}
# raw GX values → our spellings, for the struct-walked chunks
SHAPE_MTX = {0: "Single", 1: "Billboard", 2: "BillboardY", 3: "Multi"}
DL_ATTR = {0x00: "PNMTXIDX", 0x09: "POS", 0x0A: "NRM", 0x0D: "TEX0"}
DL_INPUT = {0: "NONE", 1: "DIRECT", 2: "INDEX8", 3: "INDEX16"}
DL_WIDTH = {1: 1, 2: 1, 3: 2}
PRIM = {0x90: "TRIANGLES", 0x98: "TRIANGLESTRIP", 0xA0: "TRIANGLEFAN"}


def inf1_section(out, c):
    out.append(
        f"INF1 flags={c.load_flags:#06x} rule={c.matrix_scaling_rule.name} "
        f"packets={c.mtx_group_count} vertices={c.vertex_count} "
        f"nodes={len(c.flat_hierarchy)}"
    )
    for n, node in enumerate(c.flat_hierarchy):
        out.append(f"node {n} {NODE_TYPE[node.type.name]} {node.index}")


def vtx1_section(out, c):
    formats = [vf for vf in c.vertex_formats if vf.attribute_type.name != "NULL"]
    out.append(f"VTX1 formats={len(formats)}")
    for vf in formats:
        out.append(
            f"vtxfmt {ATTR[vf.attribute_type.name]} "
            f"count={vf.component_count_type.value} "
            f"type={COMP_TYPE[vf.component_type.name]} shift={vf.component_shift}"
        )
    # element counts via offset deltas (mirrors vtx1.rs): the next boundary is
    # the smallest present array offset greater than this one, else chunk end.
    offsets = c.vertex_data_offsets  # [pos, nrm, NBT, col0, col1, tex0..tex7]
    present = sorted(o for o in offsets if o != 0)
    chunk_len = c.size

    def count(off, stride):
        nxt = min((o for o in present if o > off), default=chunk_len)
        return (nxt - off) // stride

    out.append(
        f"vtxcount pos={count(offsets[0], 12)} nrm={count(offsets[1], 12)} "
        f"uv0={count(offsets[5], 4)}"
    )


def jnt1_section(out, c):
    out.append(f"JNT1 joints={c.joint_count}")
    for i, j in enumerate(c.joints):
        s = (j.scale.x, j.scale.y, j.scale.z)
        t = (j.translation.x, j.translation.y, j.translation.z)
        mn = (j.bounding_box_min.x, j.bounding_box_min.y, j.bounding_box_min.z)
        mx = (j.bounding_box_max.x, j.bounding_box_max.y, j.bounding_box_max.z)
        out.append(
            f"joint {i} {c.joint_names[i]} type={j.matrix_type} "
            f"nis={j.no_inherit_scale} s={f3(s)} "
            f"r={s16(j.rotation.x)},{s16(j.rotation.y)},{s16(j.rotation.z)} t={f3(t)} "
            f"radius={f(j.bounding_sphere_radius)} min={f3(mn)} max={f3(mx)}"
        )


def evp1_section(out, c, joint_count):
    d = c.data.getvalue()
    count = u16(d, 8)
    count_off = u32(d, 0x0C)
    idx_off = u32(d, 0x10)
    wgt_off = u32(d, 0x14)
    inv_off = u32(d, 0x18)
    inv_count = (len(d) - inv_off) // 0x30
    out.append(f"EVP1 envelopes={count} invbinds={joint_count}")
    ii = 0
    for e in range(count):
        n = u8(d, count_off + e)
        parts = []
        for _ in range(n):
            j = u16(d, idx_off + ii * 2)
            w = f32(d, wgt_off + ii * 4)
            ii += 1
            parts.append(f"{j}:{f(w)}")
        out.append(f"env {e} {n} {' '.join(parts)}")
    for j in range(joint_count):
        vals = [f32(d, inv_off + j * 0x30 + k * 4) for k in range(12)]
        out.append(f"invbind {j} {','.join(f(v) for v in vals)}")
    assert inv_count >= joint_count, (inv_count, joint_count)


def drw1_section(out, c):
    d = c.data.getvalue()
    count = u16(d, 8)
    flags_off = u32(d, 0x0C)
    idx_off = u32(d, 0x10)
    out.append(f"DRW1 slots={count}")
    for i in range(count):
        flag = u8(d, flags_off + i)
        idx = u16(d, idx_off + i * 2)
        kind = "JOINT" if flag == 0 else "ENV"
        out.append(f"drw {i} {kind} {idx}")


def decode_dl(d, start, size, attrs):
    """Yield (prim_name, vertex_count) per primitive in a shape display list."""
    width = sum(DL_WIDTH[inp] for _, inp in attrs)
    pos = start
    end = start + size
    while pos < end:
        opcode = u8(d, pos)
        pos += 1
        if opcode == 0:
            break
        vcount = u16(d, pos)
        pos += 2
        pos += vcount * width
        yield (PRIM[opcode], vcount)


def shp1_section(out, c):
    d = c.data.getvalue()
    count = u16(d, 8)
    init_off = u32(d, 0x0C)
    remap_off = u32(d, 0x10)
    desc_off = u32(d, 0x18)
    mtxtab_off = u32(d, 0x1C)
    dl_off = u32(d, 0x20)
    mtxinit_off = u32(d, 0x24)
    drawinit_off = u32(d, 0x28)
    out.append(f"SHP1 shapes={count}")
    for s in range(count):
        remap = u16(d, remap_off + s * 2)
        base = init_off + remap * 0x28
        mtx_type = u8(d, base)
        group_num = u16(d, base + 2)
        desc_index = u16(d, base + 4)
        mtxinit_index = u16(d, base + 6)
        drawinit_index = u16(d, base + 8)
        radius = f32(d, base + 0x0C)
        mn = (f32(d, base + 0x10), f32(d, base + 0x14), f32(d, base + 0x18))
        mx = (f32(d, base + 0x1C), f32(d, base + 0x20), f32(d, base + 0x24))

        # vertex descriptor list, attr 0xFF terminated
        attrs = []
        p = desc_off + desc_index
        while True:
            attr = u32(d, p)
            p += 4
            if attr == 0xFF:
                break
            inp = u32(d, p)
            p += 4
            attrs.append((attr, inp))
        attr_str = " ".join(f"{DL_ATTR[a]}/{DL_INPUT[i]}" for a, i in attrs)
        out.append(
            f"shape {s} type={SHAPE_MTX[mtx_type]} groups={group_num} "
            f"attrs=[{attr_str}] radius={f(radius)} min={f3(mn)} max={f3(mx)}"
        )
        for g in range(group_num):
            mi = mtxinit_off + (mtxinit_index + g) * 8
            use_count = u16(d, mi + 2)
            first = u32(d, mi + 4)
            use_mtx = []
            for k in range(use_count):
                e = u16(d, mtxtab_off + (first + k) * 2)
                use_mtx.append("-" if e == 0xFFFF else str(e))
            di = drawinit_off + (drawinit_index + g) * 8
            dlsize = u32(d, di)
            dloff = u32(d, di + 4)
            prims = list(decode_dl(d, dl_off + dloff, dlsize, attrs))
            out.append(
                f"group {s} {g} use_mtx=[{','.join(use_mtx)}] "
                f"dlsize={dlsize} prims={len(prims)}"
            )
            for pnum, (ptype, vcount) in enumerate(prims):
                out.append(f"prim {s} {g} {pnum} {ptype} {vcount}")


def main() -> None:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <file.bdl>", file=sys.stderr)
        sys.exit(2)
    bdl = BDL(sys.argv[1])
    chunks = {c.magic: c for c in bdl.chunks}
    joint_count = chunks["JNT1"].joint_count

    out: list[str] = []
    inf1_section(out, chunks["INF1"])
    vtx1_section(out, chunks["VTX1"])
    jnt1_section(out, chunks["JNT1"])
    evp1_section(out, chunks["EVP1"], joint_count)
    drw1_section(out, chunks["DRW1"])
    shp1_section(out, chunks["SHP1"])
    print("\n".join(out))


if __name__ == "__main__":
    main()
