#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034"]
# ///

# Independent semantic oracle for Link animations: reads the *original* raw
# files (never Rust-emitted values) and prints the canonical dump that
# `convert_link_animations --dump-canonical` must reproduce byte-for-byte.
#
# Independence layers:
# - The dump itself is a plain `struct`-based walk of the J3D1/ANK1/TPT1/TTK1
#   bytes implementing the J3D reader semantics from
#   J3DAnmLoader.cpp / J3DAnimation.cpp (key counts 0/1/n, tangent stride
#   3 for type 0 and 4 for any nonzero word, axis-major table grouping).
# - On top of that, every clip is cross-checked against the pinned gclib
#   parse where gclib exposes the data: the J3D header + BAS trailer, the
#   ANK1/TPT1/TTK1 header fields, and the full TTK1 track walk for clips
#   with identity remaps (gclib asserts identity remaps and cannot parse
#   the post set — exactly the gaps the struct walk covers). Any
#   disagreement is a hard error.
#
# Output format: see crates/convert-link/src/animation/output.rs (the Rust
# side is the format spec; this file must match it exactly, including the
# f32 bit-pattern rendering).

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from pathlib import Path

from gclib import j3d as gclib_j3d


class OracleError(Exception):
    pass


def f32hex(raw4: bytes) -> str:
    (bits,) = struct.unpack(">I", raw4)
    return f"f32:0x{bits:08X}"


def u8(data: bytes, off: int, what: str) -> int:
    if off >= len(data):
        raise OracleError(f"{what}: u8 at {off:#x} out of bounds")
    return data[off]


def u16(data: bytes, off: int, what: str) -> int:
    if off + 2 > len(data):
        raise OracleError(f"{what}: u16 at {off:#x} out of bounds")
    return struct.unpack_from(">H", data, off)[0]


def i16(data: bytes, off: int, what: str) -> int:
    if off + 2 > len(data):
        raise OracleError(f"{what}: i16 at {off:#x} out of bounds")
    return struct.unpack_from(">h", data, off)[0]


def u32(data: bytes, off: int, what: str) -> int:
    if off + 4 > len(data):
        raise OracleError(f"{what}: u32 at {off:#x} out of bounds")
    return struct.unpack_from(">I", data, off)[0]


def chunk_slice(data: bytes, what: str) -> bytes:
    """The single chunk at 0x20, clamped to the file. Vanilla BTK chunk-size
    words can overrun EOF by up to one 0x20 pad block; anything larger is
    corruption and rejected (mirroring the Rust bound)."""
    chunk_size = u32(data, 0x24, f"{what}: chunk size")
    if chunk_size < 0x18:
        raise OracleError(f"{what}: chunk size {chunk_size} smaller than its header")
    if 0x20 + chunk_size > len(data) + 0x20:
        raise OracleError(
            f"{what}: chunk claims {chunk_size} bytes, past the {len(data)}-byte file"
        )
    end = min(0x20 + chunk_size, len(data))
    return data[0x20:end]


def j3d_header(data: bytes, what: str, file_type: str, chunk_tag: bytes) -> None:
    if len(data) < 0x28:
        raise OracleError(f"{what}: smaller than a J3D1 header")
    if data[0:4] != b"J3D1":
        raise OracleError(f"{what}: magic {data[0:4]!r} is not J3D1")
    if data[4:8] != file_type.encode("ascii"):
        raise OracleError(f"{what}: file type {data[4:8]!r} is not {file_type!r}")
    if u32(data, 8, f"{what}: length") != len(data):
        raise OracleError(f"{what}: header length does not match the file")
    if u32(data, 0xC, f"{what}: chunk count") != 1:
        raise OracleError(f"{what}: not a single-chunk file")
    if data[0x20:0x24] != chunk_tag:
        raise OracleError(f"{what}: chunk {data[0x20:0x24]!r} is not {chunk_tag!r}")


def read_name_table(chunk: bytes, off: int, what: str) -> list[str]:
    count = u16(chunk, off, f"{what}: count")
    names = []
    for i in range(count):
        entry = off + 4 + 4 * i
        data_off = u16(chunk, entry + 2, f"{what}: entry {i}")
        start = off + data_off
        end = chunk.index(b"\0", start)
        try:
            names.append(chunk[start:end].decode("ascii"))
        except UnicodeDecodeError as ex:
            raise OracleError(f"{what}: name {i} is not ASCII") from ex
    return names


def track_words(desc: tuple[int, int, int]) -> tuple[int, int]:
    count, _index, tangent_type = desc
    stride = 3 if tangent_type == 0 else 4
    return count, stride


def read_track_f32(
    chunk: bytes, desc: tuple[int, int, int], pool: tuple[int, int], what: str
) -> str:
    count, index, tangent_type = desc
    pool_off, pool_len = pool
    if count == 0:
        return "default"

    def word(i: int) -> bytes:
        start = pool_off + 4 * i
        raw = chunk[start : start + 4]
        if len(raw) < 4 or start + 4 > pool_off + pool_len:
            raise OracleError(f"{what}: pool word {i} outside the pool")
        return raw

    if count == 1:
        return f"constant={f32hex(word(index))}"
    stride = 3 if tangent_type == 0 else 4
    parts = []
    prev_time = None
    for k in range(count):
        words = [word(index + k * stride + j) for j in range(stride)]
        (time_bits,) = struct.unpack_from(">I", words[0], 0)
        if time_bits in (0x7F800000, 0xFF800000) or (time_bits >> 23) & 0xFF == 0xFF:
            raise OracleError(f"{what}: key {k} time is not finite")
        if prev_time is not None and not struct.unpack_from(">f", words[0], 0)[0] > prev_time:
            raise OracleError(f"{what}: key {k} time does not strictly increase")
        prev_time = struct.unpack_from(">f", words[0], 0)[0]
        t, v, ti = f32hex(words[0]), f32hex(words[1]), f32hex(words[2])
        to = f32hex(words[3]) if stride == 4 else ti
        parts.append(f"({t},{v},{ti},{to})")
    return f"keyed[ty={tangent_type}] " + ";".join(parts)


def read_track_i16(
    chunk: bytes, desc: tuple[int, int, int], pool: tuple[int, int], what: str
) -> str:
    count, index, tangent_type = desc
    pool_off, pool_len = pool
    if count == 0:
        return "default"

    def halfword(i: int) -> int:
        start = pool_off + 2 * i
        if start + 2 > pool_off + pool_len:
            raise OracleError(f"{what}: pool word {i} outside the pool")
        return i16(chunk, start, what)

    if count == 1:
        return f"constant={halfword(index)}"
    stride = 3 if tangent_type == 0 else 4
    parts = []
    prev_time = None
    for k in range(count):
        base = index + k * stride
        time = halfword(base)
        value = halfword(base + 1)
        ti = halfword(base + 2)
        to = halfword(base + 3) if stride == 4 else ti
        if prev_time is not None and time <= prev_time:
            raise OracleError(f"{what}: key {k} time does not strictly increase")
        prev_time = time
        parts.append(f"({time},{value},{ti},{to})")
    return f"keyed[ty={tangent_type}] " + ";".join(parts)


def read_axis(
    chunk: bytes, table_off: int, pools: tuple[tuple[int, int], tuple[int, int], tuple[int, int]], what: str
) -> str:
    def desc_at(off: int) -> tuple[int, int, int]:
        if off + 6 > len(chunk):
            raise OracleError(f"{what}: key descriptor at {off:#x} outside the chunk")
        return struct.unpack_from(">HHH", chunk, off)

    s_desc = desc_at(table_off)
    r_desc = desc_at(table_off + 6)
    t_desc = desc_at(table_off + 12)
    return (
        f"scale={read_track_f32(chunk, s_desc, pools[0], what + ' scale')}"
        f" rotation={read_track_i16(chunk, r_desc, pools[1], what + ' rotation')}"
        f" translation={read_track_f32(chunk, t_desc, pools[2], what + ' translation')}"
    )


# --- BCK ------------------------------------------------------------------------


def dump_bck(data: bytes, what: str) -> str:
    j3d_header(data, what, "bck1", b"ANK1")
    chunk = chunk_slice(data, what)
    loop_a = u8(chunk, 0x08, f"{what}: loop")
    rot_shift = u8(chunk, 0x09, f"{what}: decimal shift")
    duration = u16(chunk, 0x0A, f"{what}: duration")
    joints = u16(chunk, 0x0C, f"{what}: joint count")
    table_off = u32(chunk, 0x14, f"{what}: table offset")
    scale_off = u32(chunk, 0x18, f"{what}: scale pool offset")
    rot_off = u32(chunk, 0x1C, f"{what}: rotation pool offset")
    trans_off = u32(chunk, 0x20, f"{what}: translation pool offset")
    # Pools are bounded by their declared element counts, not by the chunk.
    pools = (
        (scale_off, 4 * u16(chunk, 0x0E, f"{what}: scale count")),
        (rot_off, 2 * u16(chunk, 0x10, f"{what}: rotation count")),
        (trans_off, 4 * u16(chunk, 0x12, f"{what}: translation count")),
    )

    sound_off = u32(data, 0x1C, f"{what}: sound offset")
    if sound_off == 0xFFFFFFFF:
        bas = "absent"
    else:
        count = u16(data, sound_off, f"{what}: BAS count")
        length = 8 + count * 0x20
        declared_chunk_end = 0x20 + u32(data, 0x24, f"{what}: chunk size")
        if sound_off < declared_chunk_end:
            raise OracleError(
                f"{what}: BAS trailer at {sound_off:#x} starts inside the"
                f" declared chunk (ends {declared_chunk_end:#x})"
            )
        if sound_off + length > len(data):
            raise OracleError(
                f"{what}: BAS trailer at {sound_off:#x} spans {length} bytes,"
                f" past the {len(data)}-byte file"
            )
        bas = f"present off={sound_off} len={length}"

    out = [f"  duration={duration} loop={loop_a} rotation_shift={rot_shift} bas={bas}"]
    for j in range(joints):
        out.append(f"  joint {j}")
        for axis, axis_name in enumerate(("x", "y", "z")):
            track = read_axis(chunk, table_off + (j * 3 + axis) * 0x12, pools, f"{what}: joint {j} axis {axis_name}")
            out.append(f"    axis {axis_name} {track}")
    return "\n".join(out)


# --- BTP ------------------------------------------------------------------------


def dump_btp(data: bytes, what: str) -> str:
    j3d_header(data, what, "btp1", b"TPT1")
    chunk = chunk_slice(data, what)
    loop_a = u8(chunk, 0x08, f"{what}: loop")
    duration = u16(chunk, 0x0A, f"{what}: duration")
    anims = u16(chunk, 0x0C, f"{what}: anim count")
    values_count = u16(chunk, 0x0E, f"{what}: value count")
    table_off = u32(chunk, 0x10, f"{what}: table offset")
    values_off = u32(chunk, 0x14, f"{what}: values offset")
    remap_off = u32(chunk, 0x18, f"{what}: remap offset")
    names_off = u32(chunk, 0x1C, f"{what}: names offset")

    names = read_name_table(chunk, names_off, f"{what}: names")
    if len(names) != anims:
        raise OracleError(f"{what}: {anims} anim rows but {len(names)} names")

    out = [f"  duration={duration} loop={loop_a}"]
    for i in range(anims):
        row = table_off + 8 * i
        count = u16(chunk, row, f"{what}: row {i} count")
        index = u16(chunk, row + 2, f"{what}: row {i} index")
        texno = u8(chunk, row + 4, f"{what}: row {i} texno")
        remap = u16(chunk, remap_off + 2 * i, f"{what}: row {i} remap")
        if index + count > values_count:
            raise OracleError(f"{what}: row {i} samples exceed the value pool")
        samples = [u16(chunk, values_off + 2 * (index + k), what) for k in range(count)]
        out.append(
            f"  target {i} material={names[i]} remap={remap} texno={texno}"
            f" samples={len(samples)}:{','.join(str(s) for s in samples)}"
        )
    return "\n".join(out)


# --- BTK ------------------------------------------------------------------------


def btk_set(chunk: bytes, base: int, count_base: int, what: str):
    """(tables, remap, names, selectors, centers, pools S/R/T, counts)."""
    return (
        u32(chunk, base, f"{what}: tables offset"),
        u32(chunk, base + 4, f"{what}: remap offset"),
        u32(chunk, base + 8, f"{what}: names offset"),
        u32(chunk, base + 12, f"{what}: selectors offset"),
        u32(chunk, base + 16, f"{what}: centers offset"),
        u32(chunk, base + 20, f"{what}: scale pool offset"),
        u32(chunk, base + 24, f"{what}: rotation pool offset"),
        u32(chunk, base + 28, f"{what}: translation pool offset"),
        u16(chunk, count_base, f"{what}: scale count"),
        u16(chunk, count_base + 2, f"{what}: rotation count"),
        u16(chunk, count_base + 4, f"{what}: translation count"),
    )


def dump_btk(data: bytes, what: str) -> str:
    j3d_header(data, what, "btk1", b"TTK1")
    chunk = chunk_slice(data, what)
    loop_a = u8(chunk, 0x08, f"{what}: loop")
    rot_shift = u8(chunk, 0x09, f"{what}: decimal shift")
    duration = u16(chunk, 0x0A, f"{what}: duration")
    track_count = u16(chunk, 0x0C, f"{what}: track count")
    if track_count % 3 != 0:
        raise OracleError(f"{what}: track count {track_count} is not a multiple of 3")
    target_count = track_count // 3
    matrix_calc = u32(chunk, 0x5C, f"{what}: matrix calc type")

    main = btk_set(chunk, 0x14, 0x0E, f"{what}: main")
    names = read_name_table(chunk, main[2], f"{what}: names")
    if len(names) != target_count:
        raise OracleError(f"{what}: {target_count} rows but {len(names)} names")

    def render_set(set_, n, names, what_set: str) -> list[str]:
        (tables, remap, _names, selectors, centers, s_off, r_off, t_off, sc, rc, tc) = set_
        pools = ((s_off, 4 * sc), (r_off, 2 * rc), (t_off, 4 * tc))
        out = []
        for t in range(n):
            what_t = f"{what_set}: target {t}"
            center = ",".join(
                f32hex(chunk[centers + 12 * t + 4 * i : centers + 12 * t + 4 * i + 4])
                for i in range(3)
            )
            out.append(
                f"  target {t} material={names[t]} remap={u16(chunk, remap + 2 * t, what_t)}"
                f" texgen={u8(chunk, selectors + t, what_t)} center={center}"
            )
            for axis, axis_name in enumerate(("s", "t", "q")):
                track = read_axis(
                    chunk,
                    tables + (t * 3 + axis) * 0x12,
                    pools,
                    f"{what_t} axis {axis_name}",
                )
                out.append(f"    axis {axis_name} {track}")
        return out

    out = [f"  duration={duration} loop={loop_a} rotation_shift={rot_shift} matrix_calc={matrix_calc}"]
    out.extend(render_set(main, target_count, names, what))

    post_names_off = u32(chunk, 0x44, f"{what}: post names offset")
    post_track_count = u16(chunk, 0x34, f"{what}: post track count")
    if post_track_count % 3 != 0:
        raise OracleError(f"{what}: post track count {post_track_count} is not a multiple of 3")
    post_target_count = post_track_count // 3
    if post_names_off != 0 or post_target_count != 0:
        if post_names_off == 0 or post_target_count == 0:
            raise OracleError(f"{what}: inconsistent post set")
        post = btk_set(chunk, 0x3C, 0x36, f"{what}: post")
        post_names = read_name_table(chunk, post[2], f"{what}: post names")
        if len(post_names) != post_target_count:
            raise OracleError(
                f"{what}: post set has {post_target_count} rows but {len(post_names)} names"
            )
        out.append(f"  post targets={post_target_count}")
        out.extend(render_set(post, post_target_count, post_names, f"{what}: post"))
    else:
        out.append("  post targets=0")
    return "\n".join(out)


# --- gclib cross-check ------------------------------------------------------------


def _gclib_load(data: bytes, fmt: str, what: str):
    import tempfile

    cls = {"bck": gclib_j3d.BCK, "btp": gclib_j3d.BTP, "btk": gclib_j3d.BTK}[fmt]
    with tempfile.NamedTemporaryFile(suffix=f".{fmt}", delete=True) as tmp:
        tmp.write(data)
        tmp.flush()
        try:
            return cls(tmp.name)
        except Exception as ex:  # gclib rejecting a file the walk accepted
            raise OracleError(f"{what}: gclib failed to parse: {ex!r}") from ex


def _enum_u8(value, what: str, field: str) -> int:
    if hasattr(value, "value"):
        return value.value
    if isinstance(value, int):
        return value
    raise OracleError(f"{what}: gclib {field} is not an enum/int: {value!r}")


def cross_check(data: bytes, fmt: str, what: str) -> None:
    """Agree with gclib on every field it exposes, else fail.

    gclib's BCK/BTP chunk objects expose only header fields; its TTK1 walks
    the full track set but asserts identity remaps and keeps the post set
    opaque. Those gaps are exactly what the struct walk above covers. For
    BTKs with non-identity remaps gclib refuses the file outright (its own
    assert), so the walk stands alone for those clips — synthetic
    differential tests cover that path instead.
    """
    local = chunk_slice(data, what)
    if fmt == "btk":
        track_count = u16(local, 0x0C, f"{what}: track count")
        remap_off = u32(local, 0x18, f"{what}: remap offset")
        remaps = [u16(local, remap_off + 2 * i, what) for i in range(track_count // 3)]
        if remaps != list(range(len(remaps))):
            return  # documented gclib limitation: non-identity remap
    parsed = _gclib_load(data, fmt, what)

    if fmt == "bck":
        ank = parsed.ank1
        checks = [
            (_enum_u8(ank.loop_mode, what, "loop_mode"), u8(local, 0x08, what), "loop_mode"),
            (ank.rotation_frac, u8(local, 0x09, what), "rotation_frac"),
            (ank.duration, u16(local, 0x0A, what), "duration"),
            (ank.anims_count, u16(local, 0x0C, what), "anims_count"),
            (ank.scale_table_count, u16(local, 0x0E, what), "scale_table_count"),
            (ank.rotation_table_count, u16(local, 0x10, what), "rotation_table_count"),
            (ank.translation_table_count, u16(local, 0x12, what), "translation_table_count"),
        ]
        # BAS trailer agreement (gclib slices it out of the same header word).
        sound_off = struct.unpack_from(">I", data, 0x1C)[0]
        gclib_sound = getattr(parsed, "bck_sound_data", None)
        if sound_off == 0xFFFFFFFF:
            if gclib_sound is not None:
                raise OracleError(f"{what}: gclib found a BAS trailer where the walk found none")
        else:
            if gclib_sound is None:
                raise OracleError(f"{what}: gclib missed the BAS trailer at {sound_off:#x}")
            count = struct.unpack_from(">H", gclib_sound, 0)[0]
            if len(gclib_sound) != 8 + count * 0x20:
                raise OracleError(f"{what}: gclib BAS length disagrees")
    elif fmt == "btp":
        tpt = parsed.tpt1
        checks = [
            (_enum_u8(tpt.loop_mode, what, "loop_mode"), u8(local, 0x08, what), "loop_mode"),
            (tpt.duration, u16(local, 0x0A, what), "duration"),
            (tpt.anims_count, u16(local, 0x0C, what), "anims_count"),
            (tpt.tex_index_count, u16(local, 0x0E, what), "tex_index_count"),
        ]
    else:
        ttk = parsed.ttk1
        checks = [
            (_enum_u8(ttk.loop_mode, what, "loop_mode"), u8(local, 0x08, what), "loop_mode"),
            (ttk.rotation_frac, u8(local, 0x09, what), "rotation_frac"),
            (ttk.duration, u16(local, 0x0A, what), "duration"),
            (ttk.matrix_mode.value, u32(local, 0x5C, what), "matrix_mode"),
        ]
        # Deep agreement on the main track set: gclib's own TTK1 walk (it
        # asserts identity remaps, so non-identity clips raise on load and
        # only the walk covers them). Reconstruct per-anim order from the
        # name table, accounting for repeated names.
        names = read_name_table(local, u32(local, 0x1C, what), what)
        tables_off = u32(local, 0x14, what)
        pools = {
            "scale": (u32(local, 0x28, what), 4),
            "rotation": (u32(local, 0x2C, what), 2),
            "translation": (u32(local, 0x30, what), 4),
        }
        seen: dict[str, int] = {}
        for i, name in enumerate(names):
            c = seen.get(name, 0)
            seen[name] = c + 1
            anim = parsed.ttk1.mat_name_to_anims[name][c]
            for axis, axis_name in enumerate(("s", "t", "q")):
                for kind, kind_off in (("scale", 0), ("rotation", 6), ("translation", 12)):
                    desc = struct.unpack_from(
                        ">HHH", local, tables_off + (i * 3 + axis) * 0x12 + kind_off
                    )
                    g = anim.tracks[f"{kind}_{axis_name}"]
                    if (g.count, g.index, _enum_u8(g.tangent_type, what, "tangent_type")) != desc:
                        raise OracleError(
                            f"{what}: target {i} {kind}_{axis_name}: gclib descriptor"
                            f" {(g.count, g.index, g.tangent_type)} != walk {desc}"
                        )
                    pool_off, elem = pools[kind]
                    if g.count == 1:
                        if elem == 2:
                            value = i16(local, pool_off + 2 * g.index, what)
                            if g.keyframes[0].value != value:
                                raise OracleError(f"{what}: target {i} {kind}_{axis_name} constant disagrees")
                        else:
                            raw = local[pool_off + 4 * g.index : pool_off + 4 * g.index + 4]
                            if f32hex(struct.pack(">f", g.keyframes[0].value)) != f32hex(raw):
                                raise OracleError(f"{what}: target {i} {kind}_{axis_name} constant disagrees")
                    elif g.count >= 2:
                        # Keyed tracks: every keyframe's time, value and both
                        # tangents must agree with gclib's decode, compared
                        # bit-exactly (f32) or exactly (i16). The tangent
                        # enum is a plain Enum, not IntEnum: normalize first.
                        tangent_u8 = _enum_u8(g.tangent_type, what, "tangent_type")
                        stride = 3 if tangent_u8 == 0 else 4
                        for k in range(g.count):
                            gk = g.keyframes[k]
                            if elem == 2:
                                base = pool_off + 2 * (g.index + k * stride)
                                walk_key = (
                                    i16(local, base, what),
                                    i16(local, base + 2, what),
                                    i16(local, base + 4, what),
                                )
                                if stride == 4:
                                    walk_key = walk_key + (i16(local, base + 6, what),)
                                else:
                                    # shared tangent: out repeats in
                                    walk_key = walk_key + (walk_key[2],)
                                gclib_key = (gk.time, gk.value, gk.tangent_in, gk.tangent_out)
                            else:
                                base = pool_off + 4 * (g.index + k * stride)
                                walk_key = tuple(
                                    f32hex(local[base + 4 * j : base + 4 * j + 4]) for j in range(stride)
                                )
                                gclib_key = tuple(
                                    f32hex(struct.pack(">f", v))
                                    for v in (gk.time, gk.value, gk.tangent_in, gk.tangent_out)
                                )
                                if stride == 3:
                                    # shared tangent: walk repeats in for out
                                    gclib_key = gclib_key[:3] + (gclib_key[2],)
                                    walk_key = walk_key[:3] + (walk_key[2],)
                            if walk_key != gclib_key:
                                raise OracleError(
                                    f"{what}: target {i} {kind}_{axis_name} key {k}:"
                                    f" gclib {gclib_key} != walk {walk_key}"
                                )

    for gclib_value, walk_value, field in checks:
        if gclib_value != walk_value:
            raise OracleError(f"{what}: gclib {field}={gclib_value} != walk {walk_value}")


# --- driver ----------------------------------------------------------------------


def load_inventory(raw_dir: Path) -> list[dict]:
    doc = __import__("json").loads((raw_dir / "inventory.json").read_text())
    if doc.get("version") != 1:
        raise OracleError(f"inventory version {doc.get('version')!r} unsupported")
    return doc["entries"]


def dump_all(raw_dir: Path) -> str:
    entries = sorted(load_inventory(raw_dir), key=lambda e: (e["archive"], e["member"]))
    out = []
    for entry in entries:
        data = (raw_dir / entry["archive"] / entry["member"]).read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        if sha != entry["sha256"]:
            raise OracleError(f"{entry['archive']}/{entry['member']}: raw hash differs from inventory")
        what = f"{entry['archive']}/{entry['member']}"
        body = {"bck": dump_bck, "btp": dump_btp, "btk": dump_btk}[entry["format"]](data, what)
        out.append(f"clip {what} {entry['format']} sha256={sha}\n{body}")
    return "\n".join(out) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Independent Link animation oracle")
    parser.add_argument("raw_dir", type=Path, help="raw tree with inventory.json")
    parser.add_argument("--no-cross-check", action="store_true", help="skip the gclib agreement layer")
    args = parser.parse_args(argv)
    try:
        text = dump_all(args.raw_dir)
        if not args.no_cross_check:
            for entry in sorted(load_inventory(args.raw_dir), key=lambda e: (e["archive"], e["member"])):
                data = (args.raw_dir / entry["archive"] / entry["member"]).read_bytes()
                cross_check(data, entry["format"], f"{entry['archive']}/{entry['member']}")
        sys.stdout.write(text)
        return 0
    except OracleError as ex:
        print(f"link_animation_oracle: error: {ex}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
