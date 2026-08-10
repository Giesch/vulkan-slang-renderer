#!/usr/bin/env bash
# Extract the King of Red Lions from the Wind Waker disc image in the repo's
# sibling ../tww checkout. Mirrors extract_link.sh's two-tier structure.
# Plan and verification checklist: llm_notes/ship_extraction.md
set -euo pipefail
# examples/toon_link -- every path below is relative to this example's crate dir
cd "$(dirname "$0")/.."

# two levels down from the repo root, so the repo's sibling is ../../../tww
TWW_DIR="${TWW_DIR:-../../../tww}"
DISC="$TWW_DIR/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso"
DTK="$TWW_DIR/build/tools/dtk"
RAW=assets/ship/raw
MANIFEST=scripts/ship_assets.sha256

# die + be_u16/be_u32 + check_size/check_bdl_header/check_bti_header, shared
# with extract_link.sh.
J3D_PROG=extract_ship
. "$(dirname "$0")/_j3d_checks.sh"

# -- preconditions ----------------------------------------------------------
[ -d "$TWW_DIR" ] || die "tww checkout not found at '$TWW_DIR' (set TWW_DIR=/path/to/tww)"
[ -f "$DISC" ] || die "disc image not found: $DISC"
[ -x "$DTK" ] || die "dtk binary not found or not executable: $DTK (build it via the tww project setup)"

# -- extract (dtk vfs cp overwrites; idempotent by construction) -------------
mkdir -p "$RAW"
extract() { "$DTK" vfs cp "$DISC:$1" "$RAW/$2"; }
# Ship.arc holds four models; daShip_c::createHeap() loads all four into one
# actor. These two are the boat itself: the hull/mast/sail rig, and the lion
# figurehead. vfncn.bdl (cannon) and vfncr.bdl (salvage arm) are equippable
# attachments and the archive's only CLR0 carriers -- deliberately out of scope.
extract "/files/res/Object/Ship.arc:bdl/fn_body.bdl"   fn_body.bdl
extract "/files/res/Object/Ship.arc:bdl/fn_head_h.bdl" fn_head_h.bdl
# fn_body's TEX1 carries a ZBtoonEX placeholder, so output.rs's RAMP_PREFIXES
# substitution needs the real runtime-injected ramps alongside it.
extract "/files/res/Object/System.arc:dat/toon.bti"    toon.bti
extract "/files/res/Object/System.arc:dat/toonex.bti"  toonex.bti

# -- tier 1: structural checks (work on the very first run) ------------------
check_bdl_header "$RAW/fn_body.bdl"
check_bdl_header "$RAW/fn_head_h.bdl"
check_bti_header "$RAW/toon.bti"
check_bti_header "$RAW/toonex.bti"
# measured exact sizes: 38240 / 77888
check_size "$RAW/fn_body.bdl"   32768 49152
check_size "$RAW/fn_head_h.bdl" 65536 98304
check_size "$RAW/toon.bti"        512  4096
check_size "$RAW/toonex.bti"    16384 65536

# -- tier 2: golden hashes (bootstrapped on first run, gated by tier 1) ------
if [ -f "$MANIFEST" ]; then
    sha256sum --check --quiet "$MANIFEST" || die "golden hash mismatch against $MANIFEST"
else
    sha256sum "$RAW"/* > "$MANIFEST"
    echo "extract_ship: BOOTSTRAP: wrote $MANIFEST -- review and commit it"
fi

echo "extract_ship: OK: 4 files extracted and verified in examples/toon_link/$RAW/"
