# Phase 0: asset extraction

Detailed plan for P0 of [`../link_rendering.md`](../link_rendering.md) §6.
Estimated: ½ day. Verification strategy follows [`tests.md`](tests.md) §P0.

**Goal**: a one-command, self-verifying, idempotent extraction of the four
Nintendo asset files from the disc image in `../tww` into the gitignored
`assets/link/raw/` tree, with committed golden hashes so every future run (and
every future machine) proves byte-identical inputs before any converter work
begins.

**Deliverables**

1. `scripts/extract_link.sh` — extraction + built-in verification
2. `just extract-link` recipe (`[unix]`, following the `beats`/`sprites`
   precedent — no Windows variant for now)
3. `.gitignore` entry for `/assets/`
4. `scripts/link_assets.sha256` — committed golden-hash manifest
   (bootstrapped by the first run; hashes of Nintendo-derived data are facts
   about the data, not the data — safe to commit)

## Inventory

Four files, all reached via `dtk vfs cp` with `:`-separated nested paths
(CISO → Yaz0 → RARC handled transparently by dtk):

| # | VFS path (inside disc) | Destination | Expected | What it is |
|---|---|---|---|---|
| 1 | `/files/res/Object/Link.arc:bdl/cl.bdl` | `assets/link/raw/cl.bdl` | ≈356 KiB | skinned body model (all chunks incl. MAT3/TEX1) |
| 2 | `/files/res/Object/Link.arc:tex/linktexbci4.bti` | `assets/link/raw/linktexbci4.bti` | ≈7.5 KiB | casual-clothes body texture (P9) |
| 3 | `/files/res/Object/System.arc:dat/toon.bti` | `assets/link/raw/toon.bti` | ≈1 KiB | shared toon ramp |
| 4 | `/files/res/Object/System.arc:dat/toonex.bti` | `assets/link/raw/toonex.bti` | ≈32 KiB | shared toonEX ramp |

Disc: `$TWW_DIR/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA,
Canada).ciso` (1.1 GiB, USA only). Tool: `$TWW_DIR/build/tools/dtk` (already
built). Note the disc filename contains spaces **and** the dtk path syntax
uses `:` separators — the entire `"$DISC:$VFS_PATH"` argument must be quoted
as one string.

Not extracted yet (deliberate): any `.bck` animation for P9's optional posed
render. Add it to the inventory + manifest when P9 starts; the script layout
below makes that a two-line change.

## Steps

### Step 1 — `.gitignore`

Add `/assets/` (leading slash: repo-root only, matching the existing entries'
style). Immediately verifiable: `git check-ignore -v assets/link/raw/cl.bdl`
names the new rule.

### Step 2 — the script

`scripts/extract_link.sh`, `#!/usr/bin/env bash` + `set -euo pipefail`,
executable bit set (precedent: `scripts/extract_beats.py`). Shape:

```sh
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."                      # repo root, works from anywhere

TWW_DIR="${TWW_DIR:-../tww}"
DISC="$TWW_DIR/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso"
DTK="$TWW_DIR/build/tools/dtk"
RAW=assets/link/raw
MANIFEST=scripts/link_assets.sha256

# -- preconditions: fail early with actionable messages --------------------
[ -d "$TWW_DIR" ] || die "tww checkout not found at '$TWW_DIR' (set TWW_DIR=...)"
[ -f "$DISC" ]    || die "disc image not found: $DISC"
[ -x "$DTK" ]     || die "dtk not built: $DTK (run the tww setup to build it)"

# -- extract (dtk vfs cp overwrites; idempotent by construction) -----------
mkdir -p "$RAW"
extract() { "$DTK" vfs cp "$DISC:$1" "$RAW/$2"; }
extract "/files/res/Object/Link.arc:bdl/cl.bdl"            cl.bdl
extract "/files/res/Object/Link.arc:tex/linktexbci4.bti"   linktexbci4.bti
extract "/files/res/Object/System.arc:dat/toon.bti"        toon.bti
extract "/files/res/Object/System.arc:dat/toonex.bti"      toonex.bti

# -- tier-1 verification: structural (works on the very first run) ---------
check_bdl_header  "$RAW/cl.bdl"          # see below
check_bti_header  "$RAW/linktexbci4.bti"
check_bti_header  "$RAW/toon.bti"
check_bti_header  "$RAW/toonex.bti"

# -- tier-2 verification: golden hashes -------------------------------------
if [ -f "$MANIFEST" ]; then
    sha256sum -c "$MANIFEST"
else
    sha256sum "$RAW"/* > "$MANIFEST"
    echo "BOOTSTRAP: wrote $MANIFEST — review and commit it."
fi
echo "OK: 4 files extracted and verified in $RAW/"
```

(`die()` = print to stderr + `exit 1`; helpers below.)

**Tier-1 structural checks** — these run even on a fresh machine with no
manifest yet, and prove the files are what they claim to be rather than
truncated garbage or the wrong archive member:

- `check_bdl_header`: first 8 bytes are exactly `J3D2bdl4`, **and** the
  big-endian u32 at offset 8 (the J3D header's total-file-size field) equals
  the actual on-disk size. The second check is the valuable one — it catches
  truncated or partially-written files with zero knowledge of the content.
  Implementation: `head -c8` + `cmp`, and `od -An -tu1 -j8 -N4` folded into an
  integer, compared against `stat -c%s`.
- `check_bti_header`: file is > 0x20 bytes (BTI header size); byte 0 (image
  format) is a valid GX texture format ID — one of
  `{0,1,2,3,4,5,6,8,9,10,14}` (I4, I8, IA4, IA8, RGB565, RGB5A3, RGBA32, C4,
  C8, C14X2, CMPR); big-endian u16 width (offset 2) and height (offset 4)
  each in 1..=1024. Deeper validation (data-offset consistency, palette
  fields) belongs to the P1/P2 converter, which parses the header fully
  anyway — don't duplicate it in bash.
- Size sanity bands (guards against extracting the wrong member, which a
  hash bootstrap would happily enshrine): `cl.bdl` 300–400 KiB, `toon.bti`
  0.5–4 KiB, `toonex.bti` 16–64 KiB, `linktexbci4.bti` 4–16 KiB.

**Tier-2 golden hashes**: `sha256sum -c scripts/link_assets.sha256` on every
run after the first. The disc is a fixed artifact, so these hashes are
permanently stable — any future mismatch means a corrupted extraction or a
different disc image, both worth a hard stop. Bootstrap mode (manifest
missing) writes the manifest and asks for review + commit; it must **only**
run after tier-1 passes, so structurally-invalid files can never become the
golden record.

Failure behavior: any failed check exits nonzero and leaves `assets/link/raw/`
as-is (files are gitignored and overwritten on the next run — no cleanup
complexity). Downstream recipes get correctness by chaining: a later
`just convert-link` can simply depend on `extract-link` since a failed
extraction aborts the chain.

### Step 3 — justfile recipe

Match house style (comment line + `[unix]` attribute):

```just
# extract Link assets from the tww disc image (needs ../tww; override with TWW_DIR)
[unix]
extract-link:
    ./scripts/extract_link.sh
```

### Step 4 — record ground truth in this doc

After the first successful run, paste into the **Recorded facts** section
below: the `dtk vfs ls` listings for `Link.arc` and `System.arc` (name + size
per member), the four exact byte sizes, and the four hashes. That gives P1 a
checked-in reference for expected sizes without needing `../tww` on hand.

## Verification

The script self-verifies on every run (tiers 1–2 above). Phase-exit
verification is the following one-time checklist:

- [x] **Cross-check against dtk**: `dtk vfs ls` sizes (356 KiB / 7.56 KiB /
      1.03 KiB / 32.0 KiB) match extracted `stat -c%s` byte-for-byte.
- [x] **Bootstrap round-trip**: deleted `toon.bti`, re-ran — restored and
      `sha256sum -c` passed against the bootstrapped manifest.
- [x] **Idempotency**: second run clean, hash check passed.
- [x] **Tamper detection**: truncated `cl.bdl` + re-run → script re-extracts
      and passes. Gate proof: corrupted file fails `sha256sum -c` directly,
      and the tier-1 checks were exercised standalone against corrupted
      copies — `check_bdl_header` caught a truncated BDL via the
      header-size-vs-disk-size mismatch, `check_bti_header` caught an
      invalid format byte (7); pristine files pass both.
- [x] **Negative preconditions**: `TWW_DIR=/nonexistent` fails immediately
      with the actionable message, exit 1.
- [x] **Git hygiene**: all four raw files hit the `/assets/` rule
      (`git check-ignore -v`); working tree shows only the intended new
      files (script, manifest, recipe, `.gitignore`).
- [x] **Recorded facts** section below is filled in.

## Recorded facts (first run, 2026-07-03, dtk 1.7.6)

Sizes match `dtk vfs ls` exactly. Hashes live in the committed
`scripts/link_assets.sha256`.

```
cl.bdl           364544 bytes  sha256:95b0bec2…c64f27
linktexbci4.bti    7744 bytes  sha256:6f980e5f…b962b3
toon.bti           1056 bytes  sha256:601e74a2…726864
toonex.bti        32800 bytes  sha256:ac43e124…5ccc78
```

BTI headers (bonus ground truth for P2 — each size is self-consistent with
its format/dimensions, which independently validates the extraction):

| file | GX format | dims | size math |
|---|---|---|---|
| `toon.bti` | 0 = I4 (4bpp) | 256×8 | 32 hdr + 256·8/2 = 1056 ✓ |
| `toonex.bti` | 14 = CMPR (4bpp) | 256×256 | 32 hdr + 256²/2 = 32800 ✓ |
| `linktexbci4.bti` | 8 = C4 (4bpp palettized — hence "bci4") | 160×96 | 32 hdr + 32 palette + 160·96/2 = 7744 ✓ |

## Out of scope for P0

- No parsing beyond the header checks above (P1).
- No Windows recipe variant (unix-only precedent exists; revisit if needed).
- No BCK animation extraction (P9; two-line addition when needed).
- No Dolphin-side captures — the savestate + FIFO log from
  [`tests.md`](tests.md) §"Dolphin as an automated oracle" need the disc but
  not this script; capture them any time before P7/P8 (early is better, e.g.
  alongside P2).
