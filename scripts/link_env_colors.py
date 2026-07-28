#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# Resolves the actor lighting colors Wind Waker feeds Link's TEV stage 0 out
# of a stage's `.dzs`, so `examples/toon_link.rs` can cite measured values
# instead of hand-tuned seeds.
#
# These two colors are the endpoints of the toon lerp:
#
#     stage 0:  PREV = mix(REG0, K0, ZBtoonEX.r)
#
# and the game overwrites both every frame in `setLightTevColorType_sub`
# (../tww/src/d/d_kankyo.cpp:1817-1829) from `dKy_tevstr_c`:
#
#     setTevColor(0)  -> GX_TEVREG0 <- mColorC0 <- Pale.mActor_C0   (shadow end)
#     setTevKColor(0) -> K0         <- mColorK0 <- Pale.mActor_K0   (lit end)
#
# NOTE the sibling branch at d_kankyo.cpp:1797-1816 swaps the two, but it is
# gated on `toon_proc_check()`, which unconditionally returns false in the
# retail build (d_kankyo.cpp:89-99). The mapping above is the live one.
#
# `setLight_actor` (d_kankyo.cpp:1328-1353) is what copies Pale -> tevstr,
# blending two palettes by time of day and two more by weather. This script
# reports one palette slot unblended, which is exact inside a schedule band
# whose two endpoints name the same slot -- the default daytime band does
# (see --time below).
#
# Chunk layout is a plain struct walk; the structs are
# ../tww/include/d/d_stage.h:103-133 and :162-164.
#
# Usage:
#     scripts/link_env_colors.py assets/link/raw/sea_stage.dzs
#     scripts/link_env_colors.py <dzs> --room 0 --weather 0 --time 2

import argparse
import struct
import sys

# stage_palet_info_class (d_stage.h:118-133), "Pale"
PALE_SIZE = 0x2C
PALE_ACTOR_C0 = 0x00
PALE_ACTOR_K0 = 0x03
# stage_pselect_info_class (d_stage.h:103-106), "Colo"
COLO_SIZE = 0x0C
# stage_envr_info_class (d_stage.h:162-164), "EnvR"
ENVR_SIZE = 0x08

# The default schedule, l_time_attribute[] in ../tww/src/d/d_kankyo_data.cpp:10-13,
# as (begin, end, palIdx0, palIdx1) over a 360-unit day. A band whose two indices
# agree is a plateau -- no blend, so a single slot is the exact answer there.
SCHEDULE = [
    (0.0, 90.0, 5, 5),
    (90.0, 105.0, 5, 0),
    (105.0, 120.0, 0, 1),
    (120.0, 150.0, 1, 2),
    (150.0, 270.0, 2, 2),
    (270.0, 285.0, 2, 3),
    (285.0, 300.0, 3, 4),
    (300.0, 315.0, 4, 5),
    (315.0, 360.0, 5, 5),
]


def chunks(data: bytes) -> dict[str, tuple[int, int]]:
    """tag -> (count, offset), from the .dzs header."""
    (count,) = struct.unpack_from(">I", data, 0)
    out = {}
    for i in range(count):
        tag, n, off = struct.unpack_from(">4sII", data, 4 + i * 12)
        out[tag.decode("ascii")] = (n, off)
    return out


def entry(data: bytes, table: tuple[int, int], size: int, index: int, tag: str) -> int:
    """Bounds-checked offset of one fixed-size record."""
    n, off = table
    if not 0 <= index < n:
        sys.exit(f"link_env_colors: {tag}[{index}] out of range (count {n})")
    return off + index * size


def rgb(data: bytes, offset: int) -> tuple[int, int, int]:
    return tuple(data[offset : offset + 3])


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("dzs", help="a stage .dzs (e.g. sea/Stage.arc:dzs/stage.dzs)")
    ap.add_argument(
        "--room", type=int, default=0, help="EnvR index; the room's envr id (default 0)"
    )
    ap.add_argument(
        "--weather",
        type=int,
        default=0,
        help="which of EnvR's 8 pselect slots; 0 is clear (default 0)",
    )
    ap.add_argument(
        "--time",
        type=int,
        default=2,
        help="which of Colo's 8 palette slots. Default 2 is the 150-270 schedule "
        "plateau (~10:00-18:00), the widest band and the only daytime one that "
        "needs no blend.",
    )
    args = ap.parse_args()

    data = open(args.dzs, "rb").read()
    table = chunks(data)
    for tag in ("EnvR", "Colo", "Pale"):
        if tag not in table:
            sys.exit(f"link_env_colors: {args.dzs} has no {tag} chunk")

    if not 0 <= args.weather < 8:
        sys.exit(f"link_env_colors: --weather {args.weather} is not in 0..7")
    if not 0 <= args.time < 8:
        sys.exit(f"link_env_colors: --time {args.time} is not in 0..7")

    envr = entry(data, table["EnvR"], ENVR_SIZE, args.room, "EnvR")
    colo_idx = data[envr + args.weather]
    colo = entry(data, table["Colo"], COLO_SIZE, colo_idx, "Colo")
    pale_idx = data[colo + args.time]
    pale = entry(data, table["Pale"], PALE_SIZE, pale_idx, "Pale")

    c0 = rgb(data, pale + PALE_ACTOR_C0)
    k0 = rgb(data, pale + PALE_ACTOR_K0)

    band = next(
        (b for b in SCHEDULE if b[2] == args.time and b[3] == args.time), None
    )
    when = f"schedule band {band[0]:g}-{band[1]:g}" if band else "no plateau band"

    print(f"# {args.dzs}")
    print(f"# EnvR[{args.room}][{args.weather}] -> Colo[{colo_idx}]"
          f"[{args.time}] -> Pale[{pale_idx}]  ({when})")
    print(f"Actor_C0 {c0[0]},{c0[1]},{c0[2]}   # -> GX_TEVREG0, the shadow end")
    print(f"Actor_K0 {k0[0]},{k0[1]},{k0[2]}   # -> konst K0, the lit end")


if __name__ == "__main__":
    main()
