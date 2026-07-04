#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034",
#     "pillow",
# ]
# ///

# P2 texture gate (claude_notes/link_rendering/phase_02.md, step 7): for
# every .bti the converter re-emitted (GX bytes copied verbatim from cl.bdl)
# plus the three raw standalone .bti files, decode with gclib and pixel-diff
# against the converter's PNG. Two independent decoders over identical
# bytes; zero differing pixels required.

import sys
from pathlib import Path

from gclib.bti import BTI
from PIL import Image


def compare(bti_path: Path, png_path: Path) -> int:
    """Returns the number of differing RGBA pixels."""
    theirs = BTI(str(bti_path)).render().convert("RGBA")
    ours = Image.open(png_path).convert("RGBA")
    if theirs.size != ours.size:
        print(f"FAIL {png_path.name} (size {ours.size} != {theirs.size})")
        return 1
    a, b = theirs.tobytes(), ours.tobytes()
    if a == b:
        return 0
    diff = sum(
        1 for i in range(0, len(a), 4) if a[i : i + 4] != b[i : i + 4]
    )
    print(f"FAIL {png_path.name} ({diff} pixels differ)")
    return diff


def main() -> None:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <raw-dir> <tex-dir>", file=sys.stderr)
        sys.exit(2)
    raw_dir, tex_dir = Path(sys.argv[1]), Path(sys.argv[2])

    pairs: list[tuple[Path, Path]] = []
    for bti_path in sorted(tex_dir.glob("*.bti")):
        pairs.append((bti_path, bti_path.with_suffix(".png")))
    for raw in sorted(raw_dir.glob("*.bti")):
        pairs.append((raw, tex_dir / f"raw_{raw.stem}.png"))
    if not pairs:
        print(f"no .bti files found under {tex_dir} (run `just convert-link` first)", file=sys.stderr)
        sys.exit(2)

    failed = 0
    for bti_path, png_path in pairs:
        if not png_path.exists():
            print(f"FAIL {png_path.name} (missing)")
            failed += 1
            continue
        if compare(bti_path, png_path) == 0:
            print(f"OK   {png_path.name}")
        else:
            failed += 1
    print(f"{len(pairs) - failed}/{len(pairs)} textures pixel-identical")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
