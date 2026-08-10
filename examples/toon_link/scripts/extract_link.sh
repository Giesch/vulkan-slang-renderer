#!/usr/bin/env bash
# Extract Toon Link assets from the Wind Waker disc image in the repo's sibling
# ../tww checkout.
# Plan and verification checklist: llm_notes/link_rendering/phase_00.md
set -euo pipefail
# examples/toon_link -- every path below is relative to this example's crate dir
cd "$(dirname "$0")/.."

# two levels down from the repo root, so the repo's sibling is ../../../tww
TWW_DIR="${TWW_DIR:-../../../tww}"
DISC="$TWW_DIR/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso"
DTK="$TWW_DIR/build/tools/dtk"
RAW=assets/link/raw
MANIFEST=scripts/link_assets.sha256

# die + be_u16/be_u32 + check_size/check_bdl_header/check_bti_header, shared
# with extract_ship.sh.
J3D_PROG=extract_link
. "$(dirname "$0")/_j3d_checks.sh"

check_dzs_chunks() { # file tag... -- walk the .dzs header and require every tag
    local file=$1; shift
    local count i tag found
    count=$(be_u32 "$file" 0)
    { [ "$count" -ge 1 ] && [ "$count" -le 64 ]; } \
        || die "$file: chunk count $count is implausible -- not a .dzs?"
    local -a tags=()
    for ((i = 0; i < count; i++)); do
        tags+=("$(dd if="$file" bs=1 skip=$((4 + i * 12)) count=4 status=none)")
    done
    for tag in "$@"; do
        found=0
        for t in "${tags[@]}"; do [ "$t" = "$tag" ] && found=1; done
        [ "$found" -eq 1 ] || die "$file: no '$tag' chunk (found: ${tags[*]})"
    done
}

# -- preconditions ----------------------------------------------------------
[ -d "$TWW_DIR" ] || die "tww checkout not found at '$TWW_DIR' (set TWW_DIR=/path/to/tww)"
[ -f "$DISC" ] || die "disc image not found: $DISC"
[ -x "$DTK" ] || die "dtk binary not found or not executable: $DTK (build it via the tww project setup)"

# -- extract (dtk vfs cp overwrites; idempotent by construction) -------------
mkdir -p "$RAW"
extract() { "$DTK" vfs cp "$DISC:$1" "$RAW/$2"; }
extract "/files/res/Object/Link.arc:bdl/cl.bdl"          cl.bdl
extract "/files/res/Object/Link.arc:tex/linktexbci4.bti" linktexbci4.bti
extract "/files/res/Object/System.arc:dat/toon.bti"      toon.bti
extract "/files/res/Object/System.arc:dat/toonex.bti"    toonex.bti
# Not part of Link's model: the ocean stage's environment palette, which is where
# the actor lighting colors his TEV stage 0 lerps between actually live. See
# scripts/link_env_colors.py.
extract "/files/res/Stage/sea/Stage.arc:dzs/stage.dzs"   sea_stage.dzs

# -- tier 1: structural checks (work on the very first run) ------------------
check_bdl_header "$RAW/cl.bdl"
check_bti_header "$RAW/linktexbci4.bti"
check_bti_header "$RAW/toon.bti"
check_bti_header "$RAW/toonex.bti"
check_size "$RAW/cl.bdl"          307200 409600
check_size "$RAW/linktexbci4.bti"   4096  16384
check_size "$RAW/toon.bti"           512   4096
check_size "$RAW/toonex.bti"       16384  65536
# EnvR -> Colo -> Pale is the exact chain link_env_colors.py walks.
check_dzs_chunks "$RAW/sea_stage.dzs" EnvR Colo Pale
check_size "$RAW/sea_stage.dzs"       4096 131072

# -- tier 2: golden hashes (bootstrapped on first run, gated by tier 1) ------
if [ -f "$MANIFEST" ]; then
    sha256sum --check --quiet "$MANIFEST" || die "golden hash mismatch against $MANIFEST"
else
    sha256sum "$RAW"/* > "$MANIFEST"
    echo "extract_link: BOOTSTRAP: wrote $MANIFEST -- review and commit it"
fi

echo "extract_link: OK: 5 files extracted and verified in examples/toon_link/$RAW/"
