# Link animations: extraction and conversion

Toon/Child Link's body and face animations, extracted from the GZLE01 Wind
Waker disc and converted to a preservation JSON schema shared through the
`gx` crate. This is the maintained reference for the whole pipeline; the
plan-level history lives in the session notes, not here.

**Scope** (operator-approved): body BCK and face BTP/BTK clips from the
**LkAnm**, **LkD00** and **LkD01** archives — gameplay plus both cutscene
sets. Everything else is out of scope by design: prop/effect animations
(they live in the same archives but animate other skeletons/materials),
other actors, BRK/BPK/BCA and the other J3D animation kinds, DAT blur data,
BAS sound-event *decoding*, runtime sampling/blending, and any renderer or
example changes. Nothing here feeds `toon_link`'s run path.

## Prerequisites

- A tww checkout containing the GZLE01 CISO at
  `tww/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso`
  and a built `tww/build/tools/dtk`. Set `TWW_DIR` in the project `.env` to
  the absolute filesystem path of this checkout, and load it with direnv
  (the project `.envrc` uses `dotenv`) or export `TWW_DIR` in your shell.
  Both extraction scripts require a nonempty `TWW_DIR`; there is no default
  checkout location or `--tww-dir` option. Documentation paths beginning
  with `tww/` are rooted at this checkout.
- `uv` (resolves the pinned
  `gclib @ 64127742467acb633d51685b9b1798ab45bb4034` used by the oracle and
  the extraction reader; the scripts are PEP-723 and fail loudly without it).
- The tooling is GZLE01-only. Other regions/revisions need separately
  reviewed manifests; a mismatched disc fails the golden checks rather than
  rewriting them.

## Commands

For a new clone, prepare and verify all Wind Waker assets with one command
from the repo root:

```bash
just toon_link tww-assets
```

This extracts the model, textures, environment palette, and animations;
converts the model and animation assets; and runs the model and animation
verification gates, including raw asset hashes, animation output hashes, and
independent oracle comparisons. It requires
`just`, Rust/Cargo, Bash, GNU coreutils, and the prerequisites above. Outputs
stay in the gitignored `examples/toon_link/assets/link/` tree. Once it succeeds,
run the example with `cargo run -p toon_link` (with the renderer's usual build
and runtime prerequisites installed).

The individual animation commands remain available:

```bash
just toon_link extract-link-animations   # disc -> assets/link/animations/raw
just toon_link convert-link-animations   # raw -> assets/link/animations/converted
just toon_link link-test-animations      # asset-free suite (synthetic fixtures)
just toon_link link-verify-animations    # real-asset gate (needs the assets)
```

`convert_link_animations <raw-dir> <out-dir> [--dump-canonical]` is the
Rust binary (`cargo run -p convert-link --bin convert_link_animations --`).
It requires the raw tree's validated `inventory.json`, rejects missing,
extra or hash-mismatched raw files, and parses *every* clip before
publishing output. `--dump-canonical` prints the normalized semantic dump
(the format diffed against the oracle) without writing anything. Conversion
needs neither `cl.bdl` nor the model conversion output.

## Selection policy

The frozen inventory is `scripts/link_animation_selection.json` (every
member of the three archives, included or excluded, with reason, size,
decompressed-content hash, RARC entry index and resource id). Every normal
extraction run must reproduce it byte-for-byte.

- **BCK** candidates are the `bcks/` members whose ANK1 animates exactly
  the CL skeleton's 42 joints (joint count read from a temporary copy of
  `Link.arc:bdl/cl.bdl`, never hardcoded). Prop BCKs (1–13 joints) are
  excluded with their joint count recorded.
- **BTP/BTK** candidates are `btp/`/`btk/` members whose material name
  tables target only CL material names (exact string match against
  `cl.bdl`'s MAT3 names). Foreign-target clips (`ItemGet_*`,
  `lightSaver`, `Bottle_MAT`, …) are excluded with their names recorded.
- A clip with **mixed** in-scope/out-of-scope targets is a hard failure,
  not a silent partial extract. An ambiguous case blocks selection until
  resolved against game source — the face table in
  `src/d/actor/d_a_player_main_data.inc` and the loader calls in
  `d_a_player_main.cpp` (`initTextureAnime`, `changeTextureAnime`) are the
  membership evidence: the game matches BTP/BTK material names against the
  CL model at runtime (`searchUpdateMaterialID`), which is exactly what the
  policy mirrors.
- Other directories (`bpk`, `brk`, `dat` in LkAnm) are not candidates;
  the extraction report counts and skips them.

Current frozen totals: 1435 included (594 BCK, 458 BTP, 383 BTK) and 48
excluded (41 prop BCKs, 1 foreign BTP, 6 prop BTKs).

## Golden manifests and bootstrap/promotion

Three tracked goldens freeze the pipeline's outputs:

- `scripts/link_animation_selection.json` — the reviewed inventory
- `scripts/link_animation_assets.sha256` — hashes of the raw tree
- `scripts/link_animation_converted.sha256` — hashes of catalog + clips

Normal runs **verify** them and never rewrite them. Adding or changing
disc content requires an explicit, reviewed promotion:

1. `just toon_link extract-link-animations --bootstrap` — writes
   *candidate* manifests under the ignored
   `assets/link/animations/candidate/` tree only (its own raw tree,
   inventory and manifests). Nothing tracked is touched.
2. Review `candidate/selection.json` against the game source (see above).
3. Validate the candidate: convert it
   (`convert_link_animations assets/link/animations/candidate/raw …`) and
   diff the canonical dump against the oracle
   (`link_animation_oracle.py assets/link/animations/candidate/raw`).
4. Promote by copying `candidate/selection.json` to `scripts/`, regenerating
   the two `.sha256` goldens from verified trees, and committing them.
5. Run `just toon_link extract-link-animations` and `convert-link-animations`
   twice; both runs must be byte-identical and verify the new goldens.

The asset-free suite includes a synthetic end-to-end test of the whole
pre-promotion flow (`bootstrap_is_explicit`), proving tracked files and
production output stay untouched.

## Output layout

```
assets/link/animations/
  raw/<Archive>/<dir>/<member>   # decompressed original files
  raw/inventory.json             # validated identity + hashes for every clip
  converted/catalog.json         # the clip list (gx::animation_manifest)
  converted/clips/<A>/<m>.json   # one document per clip
  extraction_report.json         # full included/excluded report (ignored)
```

## Schema and units

Types live in `crates/gx/src/animation_manifest.rs` (serde, no graphics
dependencies); the semantics are documented on the types. The essentials:

- Rotations (and their key times/tangents) are source `i16` in **65536
  units per turn** (`0x4000` = quarter turn). The runtime additionally
  shifts each value left by the clip's `rotation_decimal_shift`; the shift
  is stored separately, never pre-applied. Scale/translation keys stay f32.
- Tracks are `default` (0 keys; scale 1, rotation/translation 0), `constant`
  (1 key), or `keyed` with the original tangent-type word: 0 = one shared
  tangent per key (stored time, value, tangent), any nonzero word = split
  tangents (time, value, in, out). Words beyond 1 occur in the wild; the
  J3D reader treats any nonzero word as split and so does the schema.
- BCK: joints in table order, each with **three axis groups** (x, y, z)
  holding that axis's {S, R, T} tracks — the axis-major layout of ANK1
  (`calcTransform` in J3DAnimation.cpp indexes `mAnmTable[joint*3+axis]`).
  BAS sound trailers are recorded (presence/offset/length) and kept in the
  raw files, never interpreted.
- BTP: rows with material name, author-time remap u16 (the runtime
  *overwrites* it by name lookup — do not read it as a CL ordinal),
  texture-map slot and stepped u16 texture indices. Sample count is
  independent of duration; nothing interpolates or collapses repeats.
- BTK: rows with material, remap, tex-matrix selector, center and three
  axis groups (s, t, q); the optional post track set is preserved whole;
  the raw matrix-calculation flag is kept (0 basic, 1 Maya; the loader maps
  anything else to basic).
- The loop attribute is the raw source byte, preserved as metadata — not a
  promised playback mode or tick rate.

## Verification

`just toon_link link-verify-animations` runs the real-asset gate:
raw-tree checks, byte-identical canonical parity between the Rust dump and
the independent Python oracle (`scripts/link_animation_oracle.py`) on every
clip, converted membership + hashes, a second conversion for repeatability,
tamper/deletion detection on copies, and the model-gate isolation check
(the unchanged `cargo test -p convert-link -- --include-ignored` passes
with the animation directories hidden, proving model gates never require
animation assets).

The oracle reads the original binaries only — never Rust output — and
cross-checks every clip against pinned gclib where gclib has coverage
(J3D/BAS headers, ANK1/TPT1/TTK1 header fields, the full TTK1 track walk).
gclib's known gaps are covered by the oracle's own struct walk and by
synthetic fixtures: gclib's TTK1 asserts identity remaps (real non-identity
remaps exist in BTPs), keeps the post set opaque, and its keyframe enum
rejects tangent words beyond 0/1 — the tests exercise exactly those cases.

The asset-free suite (`link-test-animations`) needs no game assets: Python
unittests drive the extraction script against a fake `dtk` and synthetic
RARC/J3D fixtures (`scripts/tests/`), Rust unit tests cover the parsers and
schema round-trip, and CLI integration tests invoke the real binary on
synthetic raw inventories. See [testing.md](testing.md).
