# Phase 3: geometry — baked bind pose, manifest v1

Detailed plan for P3 of [`../link_rendering.md`](../link_rendering.md) §6.
Estimated: 3–4 days. Verification strategy follows [`tests.md`](tests.md) §P3
with one re-weighting (Step 7): the file's own inverse bind matrices are the
skeleton oracle, so SuperBMD demotes to a manual overlay check. Depends on
P1/P2 (committed as `6431f0a`, `8a0a4af`): chunk table, `BeReader`, typed GX
enums, MAT3 parse (batch↔material wiring uses it), gates `link-verify-p1/p2`.

**Goal**: parse the six geometry chunks (INF1, VTX1, EVP1, DRW1, JNT1, SHP1),
evaluate the skeleton once at bind pose, resolve the per-packet matrix tables,
bake skinned vertices to model space, expand strips to indexed triangles, and
emit `assets/link/converted/` manifest v1 (`link.manifest.json` +
`link.vtx.bin`/`link.idx.bin`/`link.skin.bin`) plus a `--obj` debug export —
every stage gated by a canonical diff against an independent oracle or by
invariants the file itself supplies.

**Deliverables**

1. `src/bin/convert_link/bmd/{inf1,vtx1,evp1,drw1,jnt1,shp1}.rs` — typed
   parsers + inline tests
2. `src/bin/convert_link/pose.rs` — FK world matrices, DRW1/EVP1 resolution,
   the packet matrix-slot state machine, vertex baking, strip→list
3. `src/model_manifest.rs` — serde manifest types, registered in `src/lib.rs`
   (shared with the P6 example; serde/serde_json already deps);
   `src/bin/convert_link/output.rs` — manifest + flat binaries + `--obj`
4. `--dump-geometry` canonical table on stdout (the diff gate)
5. `scripts/link_geometry_table.py` — oracle (gclib INF1/VTX1/JNT1 parse +
   independent struct walks for EVP1/DRW1/SHP1, incl. its own display-list
   decoder; prototype already validated against the real file)
6. `just link-verify-geometry`, umbrella `just link-verify-p3`; golden hashes
   extended to the new outputs
7. Recorded facts below filled in

## File-format facts this phase relies on

**Struct layouts** (verified in tww; gclib field names agree):

- **INF1** (`J3DModelLoader.h:11–16`, gclib `inf1.py`): u16 load flags
  (low 4 bits = matrix scaling rule), u32 mtxGroupCount, u32 vertexCount,
  u32 hierarchy offset. Hierarchy = stream of 4-byte nodes
  `{u16 type, u16 index}`: 0x00 FINISH, 0x01 OPEN_CHILD, 0x02 CLOSE_CHILD,
  0x10 JOINT, 0x11 MATERIAL, 0x12 SHAPE. OPEN_CHILD nests under the
  *previous* node; this stream defines joint parentage **and draw order**
  (SHAPE nodes inherit the nearest MATERIAL ancestor).
- **VTX1** (gclib `vtx1.py`): u32 format-table offset at +8, then 13 u32
  array offsets (pos, nrm, NBT, color0, color1, tex0–7); format entries are
  0x10 bytes `{u32 attr, u32 compCount, u32 compType, u8 shift, pad}`
  terminated by attr 0xFF. Fixed-point integer components scale by
  `1 / 2^shift`.
- **EVP1** (`J3DModelLoader.h:27–33`, `readEnvelop` at
  `J3DModelLoader.cpp:341`): u16 envelope count, then offsets to: u8
  per-envelope joint counts, u16 joint indices (cumulative stream), f32
  weights (same stream shape), and **3×4 f32 inverse bind matrices indexed
  by joint** — the FK answer key (Step 7).
- **DRW1** (`J3DModelLoader.h:35–39`, `readDraw` at `:354`): u16 slot count,
  offsets to u8 flags and u16 indices. Flag 0 → index is a JNT1 joint;
  flag 1 → index is an EVP1 envelope.
- **JNT1** (`J3DJointFactory.h:19`, gclib `jnt1.py`): 0x40-byte joints:
  u16 matrixType, u8 noInheritScale, scale f32×3, rotation s16×3
  (0x8000 = π), translation f32×3, bounding sphere/box. Parentage comes
  from INF1, not JNT1.
- **SHP1** (`J3DShapeFactory.h:12–49`): header offsets at +0x0C..+0x28:
  shape init data (0x28 each: `{u8 mtxType, u16 mtxGroupNum,
  u16 vtxDescListIndex (byte offset into the desc pool), u16 mtxInitDataIndex,
  u16 drawInitDataIndex, f32 radius, Vec min/max}`), index (remap) table,
  name table (0 in cl.bdl), GXVtxDescList pool (`{u32 attr, u32 inputType}`
  pairs, attr 0xFF terminated), u16 matrix table, display-list data,
  J3DShapeMtxInitData (`{u16 useMtxIndex, u16 useMtxCount,
  u32 firstUseMtxIndex}` per matrix group), J3DShapeDrawInitData
  (`{u32 dlSize, u32 dlOffset}` per group).
- **Display lists**: `u8 opcode, u16 vertexCount`, then per vertex one value
  per attribute in the shape's desc list — GX_DIRECT (1) = 1 byte,
  GX_INDEX8 (2) = 1 byte, GX_INDEX16 (3) = 2 bytes (GXEnum.h:265–268).
  Opcodes (GXEnum.h:7–13): 0x90 triangles, 0x98 strip, 0xA0 fan; 0x00 pads
  to the 32-byte-aligned dlSize. PNMTXIDX values are slot×3 (slot < 10).
- **Attr IDs** (GXEnum.h:199–226): PNMTXIDX=0, POS=9, NRM=10, CLR0=11,
  TEX0=13, NULL=0xFF.

**Probed facts** (oracle-prototype run against the real `cl.bdl`; the
canonical gate re-asserts all of this):

- **INF1**: scaling rule MAYA, 45 matrix groups, `vertexCount` 1591 (confirm
  meaning — expected = position-array element count), flat stream of 241
  nodes: 42 JOINT, 24 MATERIAL, 24 SHAPE, 75 OPEN/CLOSE pairs, 1 FINISH.
- **VTX1**: exactly three attribute arrays — **positions f32 XYZ, normals
  f32 XYZ, tex0 s16 ST with shift 8** (÷256). No color arrays, no second UV,
  no NBT. (Risk #2 resolved: only UVs are fixed-point.)
- **JNT1**: joint names are lowercase (`link_root`, `center`, …, `cl_back`).
  **Every scale is exactly (1,1,1)** — the MAYA scaling rule,
  `noInheritScale` (12 joints), and segment-scale-compensate semantics are
  all moot for this model; FK reduces to `world = parent_world · T · R`.
  matrixType histogram {0: 8, 1: 33, 2: 1} — record meaning, unused by FK.
- **EVP1**: 120 envelopes; mix counts {2: 101, 3: 18, 4: 1}, 260 weight
  entries; 42 inverse bind matrices.
- **DRW1**: 270 slots = 30 rigid (flag 0) + 240 weighted (flag 1).
- **SHP1**: 24 shapes = 7 Multi_Matrix (skinned body parts, all carrying
  PNMTXIDX GX_DIRECT) + 17 Single_Matrix (rigid; face/eye/brow overlays).
  **No billboards.** 3 distinct attribute sets: (PNMTXIDX, POS, NRM, TEX0),
  (POS, NRM, TEX0), (POS, TEX0) — two eye shapes have no normals. All
  attribute reads are GX_INDEX16. **573 primitives, all 0x98 strips — no
  fans, no plain triangles** → 2,874 triangles total. 77 `0xFFFF`
  inherit-from-previous-packet entries in the matrix tables (risk #1's
  mechanism is exercised).

## Step 1 — chunk parsers (`bmd/*.rs`)

Same shape as P2: each parser takes the chunk slice, all offsets
chunk-relative, every enum byte through `gx/types` additions
(`Attr`, `AttrInputType`, `ComponentCount`, `ComponentType`, `PrimitiveType`,
`ShapeMatrixType`, `InfNodeType`, `MatrixScalingRule`), sentinel-free hard
invariants:

- `inf1.rs`: flat node list + a validated tree (single root joint; every
  OPEN has a preceding attachable node; balanced OPEN/CLOSE; FINISH last;
  joint/material/shape indices in range; joint count == 42 == JNT1's).
  Exposes `parents: Vec<Option<u16>>` for joints and the material→shape
  draw sequence.
- `vtx1.rs`: format table into typed descriptors; array element counts
  derived from the gaps between consecutive present array offsets (last one
  bounded by chunk end — same method gclib uses); decode helpers
  `pos(i) -> [f32; 3]`, `nrm(i)`, `uv0(i)` applying the fixed-point shift.
  Reject formats cl.bdl doesn't use (color/NBT arrays, non-f32 pos/nrm,
  index overflow) — Expectations-style strictness.
- `evp1.rs`: per-envelope `(joint, weight)` lists + `inv_bind: Vec<Mat3x4>`
  (glam `Mat4` built from 3×4 rows); weights of each envelope must sum to
  ≈1 (assert, record tolerance) and joint indices < 42.
- `drw1.rs`: `Vec<DrwSlot>` where `DrwSlot::Joint(u16) | Envelope(u16)`,
  indices range-checked against JNT1/EVP1.
- `jnt1.rs`: typed joints (rotation kept as raw s16 for the manifest;
  radians derived in pose.rs); scale == (1,1,1) asserted for cl.bdl.
- `shp1.rs`: shapes with attribute sets, matrix groups (resolved
  `use_mtx` slices), and decoded display lists — a
  `Vec<Primitive { prim_type, verts: Vec<VertexIndices> }>` per group,
  where `VertexIndices` holds `pnmtx_slot: Option<u8>` (value/3, asserting
  %3==0 and <10) and per-attr u16 indices. dlSize consumed exactly (trailing
  bytes must be zero padding); every index validated against VTX1 counts.

## Step 2 — `pose.rs`: FK, matrix slots, baking

```rust
pub struct BakedModel {
    pub vertices: Vec<BakedVertex>,   // pos [f32;3], nrm [f32;3], uv [f32;2]
    pub skin: Vec<[(u8, f32); 4]>,    // per vertex, zero-padded
    pub indices_per_shape: Vec<Vec<u32>>, // triangle lists, GX winding
    pub joint_world: Vec<Mat4>,       // 42 entries
}
```

- **FK**: `world(j) = world(parent(j)) · T(j) · R(j)` (scales all 1.0 —
  asserted). Rotation order Z·Y·X (J3D convention; the invBind identity
  check below catches it if wrong, and X·Y·Z is the one-line fallback).
- **Skinning matrices**: rigid DRW1 slot → `world(joint)`; weighted slot →
  `Σ wᵢ · world(jᵢ) · invBind(jᵢ)`.
- **Matrix-slot state machine**: a 10-slot `[Option<u16>; 10]` table
  persisting across a shape's groups **in file order**; each group loads its
  `use_mtx` entries into slots 0..count, `0xFFFF` = keep the slot's current
  value; reading an unset slot is a hard error. Note the state persists
  across groups *within* a shape; whether it persists across shapes is
  irrelevant if the invariant "every slot read after 0xFFFF was set earlier
  in the same shape" holds — assert exactly that, record the result.
- **Baking**: positions/normals transform by the slot's skinning matrix
  (rigid shapes without PNMTXIDX use the single matrix of group 0's slot 0);
  normals via inverse-transpose (pure rotations at bind pose, but keep the
  general path), renormalized. Missing normals emit (0,0,0) (their two
  materials have lighting disabled). Vertices dedup by
  `(pos_idx, nrm_idx, uv_idx, resolved_matrix_key)` so identical GX tuples
  under different matrices stay distinct.
- **Strip→list**: strip `[v0..vn]` → triangles `(i, i+1, i+2)` with odd
  triangles swapping the first two indices; fans `(0, i, i+1)` (implement +
  unit test both even though cl.bdl is strips-only; 0x90 lists pass
  through; anything else is a hard error). Emit GX-native winding; the
  P6 cull check (risk #3) decides whether the converter flips.

## Step 3 — manifest v1 + binaries (`output.rs`, `src/model_manifest.rs`)

Master-plan §2.3 with one **deviation, from probed facts**: cl.bdl has no
color arrays and one UV channel, and every MAT3 channel sources colors from
registers — so the vertex is `pos[3] nrm[3] uv0[2]` = 8 f32 = 32 bytes, not
the sketched 14-float layout. The manifest records the layout explicitly:

```jsonc
{
  "version": 1,
  "buffers": { "vertices": "link.vtx.bin", "indices": "link.idx.bin",
               "skinning": "link.skin.bin",
               "vertex_layout": ["position3f", "normal3f", "uv02f"],
               "vertex_count": 0, "index_count": 0 },
  "textures": [ /* P2 outputs by index: file, wrap, filter; ramp slots note
                   their runtime substitution (ZBtoonEX ← raw_toonex) */ ],
  "materials": [ /* name + record index + raster state + full TEV data,
                   serialized from the P2 Material structs */ ],
  "batches": [ { "material": 0, "shape": 0, "first_index": 0,
                 "index_count": 810 } /* INF1 draw order */ ],
  "skeleton": { "joints": [ { "name": "link_root", "parent": -1,
                              "t": [...], "r_s16": [...], "s": [1,1,1] } ] }
}
```

- `link.vtx.bin`/`link.idx.bin` little-endian; `link.skin.bin` = 4 ×
  (u8 joint + f32 weight) per vertex (unused until animation).
- Batches in INF1 traversal order; each carries its MAT3 slot so the P6
  example can order opaque→translucent by `pe_mode` (J3D's two-pass rule).
- Serializing materials into the manifest (TEV config for P6 uniforms) may
  land as a stub in P3 (`"materials": []`) if it drags — it's P6's input,
  gated then, and `mat3_dump.txt` already proves the parse.

## Step 4 — `--dump-geometry` and `--obj`

- `--dump-geometry`: canonical table on stdout, P1/P2 diff discipline —
  **raw file data only** (exact bytes → exact text; no computed floats, so
  no f32-vs-f64 formatting hazard): INF1 header + flat node list; VTX1
  formats + element counts; JNT1 joints verbatim (t/s as `%.6f` of stored
  f32, rotations as raw s16); EVP1 envelopes, weights, inverse binds
  verbatim; DRW1 slot table; SHP1 per shape: matrix type, attr set, groups
  with `use_mtx` tables (raw, `-` for 0xFFFF), per-group primitive summary
  (opcode + vertex count per primitive), and dl byte sizes. Nothing derived,
  everything diffable.
- `--obj`: positions/uvs/normals + one `g`/`usemtl` group per batch, plus a
  companion `.mtl` whose materials reference the P2 PNGs
  (`map_Kd tex/NN_<name>.png`, resolved via each material's texture slot 0)
  so Blender shows a textured Link — this pulls the UV-placement check
  forward from P7 into P3's visual pass. OBJ's `vt` convention has V=0 at
  the bottom while our PNGs are top-down, so the exporter writes `1−v`;
  a vertically-flipped face texture in Blender means that flip (or the UV
  decode) is wrong. Debug-only output, excluded from golden hashes. The
  verification procedure is spelled out in
  [Blender verification](#blender-verification-procedure) below.

## Step 5 — oracle script

`scripts/link_geometry_table.py` (pinned gclib, PEP-723): prints the same
canonical table. gclib supplies INF1 (flat hierarchy), VTX1 (formats +
arrays), JNT1 (joints); EVP1/DRW1/SHP1 sections are independent struct walks
per the layouts above **including its own display-list decoder** — the
prototype of exactly this script already ran clean against the real file
(Probed facts). SHP1 is where our Rust has the most room for silent error
(three-layer indirection + DL walk), and the python side reimplements all of
it from the tww structs, not from our code.

## Step 6 — recipes

```just
# P3 gate: canonical geometry diff + full conversion with all invariants
[unix]
link-verify-geometry:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --dump-geometry) <(./scripts/link_geometry_table.py assets/link/raw/cl.bdl)
    just convert-link >/dev/null   # runs the baking invariants (Step 7)
    echo "geometry table matches oracle"

# P3 gate: geometry + ignored real-file tests
[unix]
link-verify-p3: link-verify-geometry
    cargo test --bin convert_link -- --include-ignored
    echo "P3 VERIFIED"
```

After the gates pass: regenerate `scripts/link_converted.sha256` including
`link.manifest.json` + the three `.bin`s.

## Step 7 — verification strategy (why this is enough)

Three independent legs, strongest first:

1. **The file is its own skeleton oracle.** EVP1 stores the inverse bind
   matrix of every joint; at bind pose `world(j) · invBind(j) = I` must hold
   for all 42 joints. This checks our FK — composition order, parent wiring
   from INF1, rotation conversion — against data authored by Nintendo's
   exporter, with no third-party tool in the loop. Hard converter error with
   a max-deviation report (ε ~1e-3 on the 4×4 residual; record the actual
   max). The **weighted-identity check** (tests.md §P3) is its corollary:
   every EVP1-weighted vertex must bake to ≈ its stored position (weights
   sum to 1 and Σw·(world·invBind) = I); hard error, max distance recorded.
   Rigid shapes are deliberately *not* identity — they move from joint-local
   to model space; their gate is the AABB comparison below.
2. **Canonical diff over all raw geometry data** (Steps 4–5): every number
   our parsers extracted — hierarchy, formats, envelope tables, joint TRS,
   matrix tables, per-primitive vertex counts — byte-compared against an
   independent decode. This pins the parse layer completely; after it, only
   pose math can be wrong, and leg 1 covers that.
3. **Stored-bounds cross-check + mesh metrics.** SHP1 carries per-shape
   AABBs and JNT1 per-joint bounds: compare baked per-shape AABBs against
   the stored ones (space semantics for rigid shapes to be confirmed —
   start as a warning, promote to hard error once understood, record the
   answer). Triangle count must equal exactly 2,874 (Σ(n−2), deterministic);
   per-batch counts and the total go in Recorded facts. Overall AABB ≈
   Link ~100 units tall sanity check.

**SuperBMD (tests.md's named oracle) demotes to manual second opinion**:
legs 1–2 make an automated DAE diff redundant, and the mono toolchain is the
flakiest piece of the toolbox. It stays for the Blender overlay
(`--obj` vs DAE) as an eyeball check before P6, and as the tiebreaker if the
stored-bounds question (leg 3) stays ambiguous. noclip remains the visual
ground truth from P6 on.

**Unit tests** (synthetic, committed):

- strip→list: even/odd winding on a 5-vertex strip (hand-drawn); fan on 5
  vertices; degenerate (n<3) handling
- matrix-slot state machine: two synthetic groups where group 1 uses 0xFFFF
  → inherits group 0's entry; unset-slot read → typed error
- FK: 2-joint chain with 90° rotations vs hand-computed positions; invBind
  identity on the same chain
- envelope blend: 2-joint 50/50 synthetic envelope vs hand-computed
- VTX1 fixed-point: s16 shift-8 UV decode (0x0180 → 1.5)
- display-list walk: synthetic 2-attr DL with INDEX8+INDEX16 widths, strip +
  padding consumed exactly
- INF1 tree builder: synthetic nesting stream → parents + draw order;
  unbalanced stream → typed error

**Real-file tests** (`#[ignore]` + skip-if-missing, run via
`link-verify-p3`): all Probed-facts numbers asserted (node counts, formats,
envelope histogram, DRW1 split, 573 strips/2,874 tris, 3 attr sets, no
billboards); invBind identity + weighted identity max deviations under
epsilon; manifest round-trips through `src/model_manifest.rs`.

## Blender verification procedure

The manual leg of the exit checklist. Everything here is *observational* —
the numeric gates must already be green; this pass catches whole-model
wrongness the numbers can't express (proportions, placement, "does it look
like Link").

**Setup**

1. `just convert-link --obj` (writes `assets/link/converted/link.obj` +
   `link.mtl`).
2. Blender → File → Import → Wavefront (.obj). Keep the default axis
   mapping (−Z forward, Y up — matches the model's Y-up space) and enable
   **Split by Group**, so each `usemtl` batch imports as its own object and
   can be isolated in the outliner.

**Checks, in order** (each maps to a converter stage):

3. **Scale & pose** — N-panel → Item → Dimensions: expect ≈ 100 Blender
   units tall, standing upright, arms in bind pose. Wildly wrong dimensions
   or a mesh smeared across the scene = matrix-table/skinning failure
   (risk #1's "exploded mesh"); a model lying face-down = axis-mapping
   mistake, not a converter bug.
4. **Rigid attachment** — orbit the model: hair, ears, belt, scabbard and
   sword must sit attached to the body in sensible places. Detached or
   origin-clustered rigid parts with a correct body = JNT1 FK walk bug
   (these parts are stored joint-local; the skinned body would still look
   right).
5. **Weighted regions** — inspect shoulders, elbows, wrists, hips, knees in
   wireframe (Z → Wireframe): smooth continuous surfaces, no pinched rings
   or torn seams at joint boundaries. This is the weighted-identity check,
   visualized.
6. **Per-batch isolation** — in the outliner, solo each of the 24 group
   objects (click the eye icons, or select + `/` for local view): each
   should look like the body part its material name claims (`face`, `eyeL`,
   `sleeve`, `podA` = the pouch, `ear(N)` = body/tunic pieces sharing
   record 0). A group containing geometry from the wrong body part =
   INF1 draw-order pairing bug.
7. **Triangle count** — Overlays → Statistics (or the Scene Statistics in
   the status bar) with everything visible: exactly **2,874 triangles**.
8. **Textures & UVs** — switch viewport shading to Material Preview: the
   face decals, eyes and eyebrows must sit correctly on the head, the tunic
   pattern upright, the belt buckle centered. Features present but
   vertically mirrored = the `vt` V-flip is wrong; features scrambled =
   UV fixed-point decode (shift 8) is wrong. (Eye/brow batches layer
   several translucent quads — overlap artifacts here are expected; P6+
   handles their draw order.)
9. **Face orientation (early winding read, risk #3)** — Overlays → Face
   Orientation: outward faces render blue, inward red. Expect uniformly
   blue (possibly uniformly red — record which); a red/blue patchwork means
   inconsistent strip-expansion winding, which is a converter bug to fix
   *now*, before P6 turns culling on.
10. **DAE overlay (optional, SuperBMD)** — import SuperBMD's DAE of
    `cl.bdl` into the same scene, select both, front/side orthographic
    views (Numpad 1/3), toggle X-ray (Alt+Z): the two meshes should
    coincide with no ghosting/offset. If SuperBMD applies its own unit
    scale, match dimensions first; if it won't run under mono, compare
    against a noclip screenshot from the same angle instead.

Record outcomes (steps 3, 9, 10 especially) in Recorded facts.

## Verification (exit checklist)

- [ ] `just link-verify-geometry`: zero-line canonical diff vs the oracle
- [ ] `just link-verify-p3` green end-to-end
- [ ] invBind identity: max residual recorded, < ε
- [ ] weighted-identity: max baked-vs-stored distance recorded, < ε
- [ ] stored-AABB semantics resolved (skinned + rigid), check promoted to
      hard error, recorded
- [ ] triangle count == 2,874 exactly; per-batch counts recorded
- [ ] `INF1.vertexCount` (1591) meaning confirmed and recorded
- [ ] Blender verification procedure completed (all 10 steps in that
      section): scale/pose, rigid attachment, weighted regions, per-batch
      isolation, 2,874 triangles, textured UV check, face orientation,
      DAE/noclip overlay; outcomes recorded
- [ ] manifest v1 + binaries emitted; `src/model_manifest.rs` registered in
      lib; `just shaders` snapshots untouched
- [ ] golden hashes regenerated with the new outputs
- [ ] tamper tests: corrupted PNMTXIDX (%3≠0), out-of-range attr index,
      0xFFFF in a never-set slot → typed errors, no panics
- [ ] `just test` green without assets; `just lint` clean; no `Cargo.toml`
      diff; nothing under `assets/` staged
- [ ] Recorded facts filled in

## Recorded facts (fill in after gates pass)

```
invBind identity max residual: ...
weighted-identity max distance: ...  (over N weighted vertices)
stored-AABB space semantics (skinned/rigid): ...
INF1.vertexCount meaning: ...
final baked counts: vertices=..., indices=..., per-batch triangles=...
matrix-slot inheritance observations (cross-shape state? unset reads?): ...
JNT1 matrixType meaning (probed {0:8, 1:33, 2:1}): ...
envelope weight-sum tolerance observed: ...
rotation composition order confirmed: ...
Blender pass: dimensions observed: ...
Blender pass: face orientation (uniform blue/red/patchwork): ...
Blender pass: UV/texture observations: ...
Blender pass: DAE or noclip overlay result: ...
golden-hash update commit: ...
```

## Out of scope for P3

- Runtime skinning, BCK animation sampling (skin.bin + skeleton emitted for
  later)
- Billboard shapes (none exist — `ShapeMatrixType::Billboard/Y_Billboard`
  are hard errors), quads/lines/points primitives (hard errors)
- Vertex colors / second UV / NBT arrays (absent in cl.bdl; typed errors if
  encountered)
- Winding flip decision (P6, with culling off first — risk #3)
- Serializing TEV material configs into the manifest may slip to early P6
  (see Step 3); everything else in the manifest lands now
- Renderer changes of any kind (P4/P5); MDL3 (permanently skipped)

## Risks / open questions

1. **SHP1 indirection depth** (risk #1, the exploded-vertex risk) — matrix
   table state across packets, PNMTXIDX/3 slots, DRW1→EVP1 chains. Covered
   three ways: canonical diff of the raw tables (parse layer), invBind +
   weighted-identity (math layer), and the 0xFFFF-slot invariants (state
   layer). The 77 real inherit-entries mean the mechanism is genuinely
   exercised, not theoretical.
2. **Rotation composition order** — Z·Y·X assumed; the invBind identity
   check converts a silent transform bug into a loud numeric failure on all
   42 joints at once.
3. **Stored-bounds space for rigid shapes** — unknown until measured; kept a
   warning until resolved (exit-checklist item), never a silent pass.
4. **Vertex dedup key** — omitting the matrix from the key would silently
   weld vertices that bake differently; the weighted-identity distances and
   per-shape AABBs would both surface it. Key includes the resolved matrix
   identity from day one.
5. **f32 float formatting in the canonical table** — same answer as P2:
   only raw stored f32s are printed (`%.6f` of the identical bit pattern on
   both sides); all *computed* values verify via epsilon checks inside the
   converter, never via text diff.
6. **INF1 vertexCount semantics** (1591) — probably the position-array
   count; confirm against VTX1 array sizes rather than assuming (cheap,
   recorded).
7. **SuperBMD under mono** — now only a manual overlay tool; if it won't
   run, the fallback (noclip screenshot comparison) is already the P6 plan.
