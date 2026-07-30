# Follow-up: deferred work, accepted limitations, verification debt

Work the link-rendering project ([`../link_rendering.md`](../link_rendering.md))
put off **without a landing phase**, plus limitations accepted as-is and
verification that was demoted or never run. Items already scheduled for
P7/P8/P9 are *not* duplicated here — they live in the master plan §6 table
(albedo pass, TEV interpreter, `BlendMode::DstAlpha` + eye write-mask
multi-pass, `--casual`, BCK pose, Dolphin golden frames). Each entry names
its source doc and, where one was stated, the condition for picking it back
up.

Started during P6 planning (2026-07-24), prompted by the uniform-array
codegen finding below.

## 1. ~~Shader-atlas codegen: array fields in parameter blocks~~ — DONE

**Landed 2026-07-24** as its own mini-phase, exactly as "Revisit when"
prescribed: commit `0d08a7d`, plan + recorded facts in
[`vec4_array_support.md`](vec4_array_support.md). The honest v1 shipped —
`float4[N]`/`uint4[N]`/`int4[N]` only (16-byte elements, stride == size in
both std140 and std430), hard actionable error otherwise — plus bare
`uint4`/`int4` vectors and `ScalarType::Int32`. The feared costs mostly
didn't materialize: zero template changes (the assert machinery is generic
over type-name strings), snapshot churn was additions + the test atlas index
only, ~1 session of work. Consequence: **P8 uses the master plan §3
flat-array `ToonLinkParams`**; phase_06 Step 1's BDA decision is superseded
(banner added there). The BDA path remains proven (sprite_batch) for
runtime-sized data. Original entry preserved below.

**The problem.** The reflection walker (`reflect_struct_fields`,
src/shaders/reflection/parameters.rs:163) handles Scalar / Vector / Matrix /
Struct / Resource / Pointer and hits
`todo!("field type layout kind not handled")` at **parameters.rs:411** for
anything else — including arrays. The JSON model `StructField` enum has no
Array variant and the askama templates have no array handling. So any
`uint4 foo[8]` / `float4 bar[4]` field in a `ParameterBlock` (or in a BDA
pointee struct — same walker) panics `just shaders`. This sank the master
plan §3's original `ToonLinkParams` sketch; P6 chose the BDA workaround
instead (`ImmutableAddr<TevStagePacked>` with pointer-indexed flat records
and named `konst0..konst3` / `reg0..reg3` fields — see
[`phase_06.md`](phase_06.md) Step 1).

**Tradeoffs weighed (P6 planning):**

- *For codegen support:* natural shader (`params.konst[i]`) and Rust
  (`[UVec4; 8]`) code; removes a permanent landmine for any future shader
  (loud `todo!`, but still a hole in an otherwise-complete codegen);
  better tooling ergonomics (UBO contents inspect nicely in RenderDoc,
  BDA pointers are raw device addresses you have to chase).
- *Against:* the plumbing (walker arm, JSON variant, template emission) is
  mechanical, but **std140 array stride is the real work** — stride rounds
  up to 16 bytes, so `float foo[4]` naively mapped to `[f32; 4]` is a
  *silent* layout mismatch (the exact failure mode risk #4 feared). Either
  build stride-aware wrapper types (complex) or restrict to
  16-byte-multiple elements with a hard error (honest but partial). Plus
  template/snapshot churn in a repo whose phases have been deliberately
  snapshot-neutral, and it sat on P8's critical path for one consumer.
- *BDA costs accepted instead:* the constraint relocates rather than
  disappears (pointee structs must be flat); minor perf asymmetry (BDA
  reads are ordinary global loads vs. driver-optimized UBO paths —
  negligible at Link's scale); more moving parts per consumer (extra
  buffer handle, `current_immutable_addr`, `_padding_0` fields). One
  genuine upside: buffers size at runtime (materials have 1–3 stages; a
  shared 24×8 buffer + base index fits naturally), where uniform arrays
  are compile-time fixed.

**Revisit when:** a second or third shader wants array fields. Do it as its
own phase; the honest v1 is 16-byte-stride elements only
(`uint4[N]`/`float4[N]`, stride == element size, no padding to get wrong)
with a hard error otherwise; the P8 TEV shader is a ready-made migration
test. Decision is fully reversible — the flat 16-byte-record layout ports
trivially to uniform arrays.

## 2. Renderer features deferred with no landing phase

- **FrameInputs migration** — the approved-but-unimplemented
  `../frame_inputs_api.md` plan replaces the `gpu_update` closure with
  declarative `frame_inputs` calls. P4 deliberately kept the closure API,
  sharing only the terminal-submit shape so the migration stays mechanical.
  (Master plan §4 note; phase_04.)
- **Picking + multi-draw** — picking stays on the legacy single-draw
  wrapper `draw_vertex_count_with_picking`, which `debug_assert`s an empty
  draw queue. *Revisit "when something needs to pick over a multi-draw
  frame"* by integrating a picking config into `submit_draws`. (Master plan
  §4.5; phase_04.)
- **Mesh lifecycle** — shared meshes are teardown-only: no `destroy_mesh`,
  no streaming; buffers outlive the pipelines that draw them. (phase_04.)
- **u16 index buffers** — index buffers are u32-only. (Master plan §4.5;
  phase_04.)
- **Instancing and vertex offsets** — `instanceCount` stays 1,
  `vertexOffset` stays 0 in the multi-draw path. (phase_04.)
- **Mixed-draw-type example** — a committed example mixing `DrawIndexed` +
  `DrawVertexCount` in one frame; structurally supported by the type
  erasure, "cheap to add" when wanted. (phase_04.)
- **Remaining pipeline state** — stencil, depth bias, polygon mode,
  primitive topology, configurable `front_face` (hardcoded CCW),
  per-attachment blend, independent color/alpha blend factors, logic ops —
  all still hardcoded in `create_graphics_pipeline`. (Master plan §4.5;
  phase_05.)
- **Texture options not yet plumbed** — `ClampToBorder` + border color,
  3D/array/cube textures, anisotropy as an explicit knob (currently derived
  from `mipmaps`), block-compressed formats (`format_block_info` is where
  they'd be whitelisted; P2 decodes everything to RGBA8, so Link never
  needs them). (phase_05.)
- **Render-graph integration** — `../render-graph/04_design.md` is an
  independent design touching pipeline creation, ~~gated on
  `../bda_footguns/03_pipelined_current_read_plan.md`~~ (ungated since
  2026-07-28: that plan is superseded by `../remove_pipelined_compute.md`).
  P4/P5 stayed additive so the graph work doesn't have to undo them.
  (phase_05 risk #6.)

## 3. Converter gaps (one-model converter by design)

- **cl.bdl-specific gates** — `blockNum == 9` and `JNT1 == 42` are hard
  gates by design; *"generalize only if another model is ever fed in."*
  (phase_01.)
- **MDL3 parsing** — permanently skipped (master-plan decision; MAT3 carries
  everything needed). (phase_01, phase_03.)
- **Texture decode limits** — hard error on `mipmapCount != 1` (none exist
  in our inputs); C14X2 undecoded; no texture *encoding* path. (phase_02.)
- **Geometry limits** — billboard shapes, quad/line/point primitives,
  vertex colors, second UV, NBT arrays: all typed hard errors; none exist
  in cl.bdl. (phase_03.)
- **Skinning data emitted but unused** — `link.skin.bin` (4×(u8 joint,
  f32 weight)/vertex) and the manifest skeleton exist solely for future
  runtime skinning / BCK animation; nothing consumes them. (phase_03;
  master plan §4.5.)

## 4. Accepted limitations / footguns (documented, not planned work)

- **Release-build OOB draws render garbage silently** — the
  `queue_draw_index_range` bounds check is `debug_assert`-only;
  robustBufferAccess makes OOB index fetches non-faulting. Accepted in P4
  planning. (phase_04 risk #5.)
- **`disable_depth_test` legacy field survives** on
  `PipelineConfig`/`PipelineConfigBuilder` and *wins over*
  `raster_state.depth_test` when set — removing it rewrites every generated
  file (~20 snapshots). If ever removed, do it as its own commit with no
  other changes. (Master plan §4.3; phase_05 risk #1.)
- **Hot-reload interface panic** — `assert_shader_interface_unchanged`
  panics on reloaded-shader interface changes by design (replaces silent
  GPU-data corruption). Body edits hot-reload; struct-shape edits need
  `just shaders` + restart. (Master plan; phase_06 risk #7.)
- ~~**VMA leak check can't be automated** — SIGTERM/SIGINT skip `Drop` and
  SDL3 posts no Quit event for them; verifying a leak-free exit needs a
  real window close (P5 used a temporary frame-limit escape in app.rs).~~ —
  **disproven** (`build_reproducibility.md` §7.4). `timeout` without
  `--foreground` signals the whole process group, SDL converts SIGTERM into
  `SDL_QUIT`, and `drain_gpu()` plus `Drop for Renderer` both run — verified in
  both directions, with SIGKILL and `SDL_NO_SIGNAL_HANDLERS=1` as negative
  controls. `scripts/headless-sweep.sh` exercises teardown on every example, so
  this check is automated rather than merely automatable. (phase_04, phase_05,
  now closed.)
- **`color_write` is plumbed and unit-tested but has no runtime test
  object** — first real exercise is the P9 eye trick. (phase_05.)
- ~~**`uint4`/`UVec4` uniform codegen unproven**~~ — proven by the §1
  mini-phase (`0d08a7d`): bare `uint4`/`int4` fields have committed
  test-shader coverage (std140_arrays' `flags`/`bias`) and passed the
  runtime pattern-band check. (phase_06 risk #1, now closed.)
- **Draw order is semantically load-bearing** once blend/depth-write vary
  per pipeline — a real property of the multi-draw design, not a bug;
  multi_mesh documents it. (phase_05 risk #5.)
- **No Windows recipe variants** for the `link-*` justfile targets
  (unix-only precedent); revisit if needed. (phase_00, phase_01.)

## 5. Verification debt

- **Full Blender pass on converted geometry still outstanding** (P3 left it
  partial): done — face orientation (uniform red), rigid attachment,
  per-batch isolation; outstanding — weighted-region wireframe check,
  textured-UV check, DAE/noclip overlay; scale/pose only implied by the
  AABB. Confirmatory only — the numeric gates already cover the same
  ground. (phase_03; master plan §6 P3 row.)
- **SuperBMD demoted to manual-only** second opinion / tiebreaker (mono
  runtime, non-scriptable); gclib is the sole automated oracle everywhere.
  If SuperBMD won't run, noclip screenshot comparison is the fallback.
  (tests.md; phase_02/03.)
- **Stored-AABB cross-check dropped as redundant** — revisit only if a
  geometry discrepancy ever appears. (phase_03.)
- **Dolphin oracle suite — optional, not scheduled.** The savestate + FIFO
  capture (phase_00 flagged "capture them any time before P7/P8 — early is
  better"), golden reference frames (`just link-dolphin-refs` — a recipe that
  does not exist yet), FIFO analyzer TEV state, software-renderer tiebreaker,
  and dolphin-memory-engine lighting ground truth are all one-time-manual
  setups, none of them run. **P8 explicitly does not use them**
  (phase_08 decision 7): its oracle is per-feature noclip comparison plus the
  converter gate, the `tev_pack` unit tests and the in-example debug modes.
  Two things therefore ship reasoned rather than measured, and this is the
  entry that owns them: the **S10 clamp semantics** (master plan risk #6 — the
  software renderer is the only literal reference) and the exact daytime
  **`dKy_tevstr_c` light/ambient values** (risk #8 — dolphin-memory-engine
  reads them from emulated RAM). Also noted: mainline Dolphin has no
  per-TEV-stage intermediate dump either, so stage-level debugging falls to the
  optional CPU reference evaluator regardless. **Revisit when:** a specific
  feature is genuinely in dispute between us and noclip, or the hand-tuned
  lighting seeds become the thing blocking a color match. (tests.md §P8;
  phase_00; phase_08.)
- **toon_link is not CI-verifiable** — assets are machine-local
  (gitignored, disc-image-derived); the example bails without them, so the
  validation sweep's toon_link line only means something on a machine where
  `just convert-link` has run. (phase_06 risk #6.)
- **P7's runtime gates were never run** — the phase was implemented in a
  headless container (no video device, no converted assets), so the
  textured render, UV-vs-noclip comparison, alpha-cutout edges, draw-order
  and `depth_write` effects, validation sweep, hot reload and VMA check are
  all outstanding. Highest-value one is the **numeric gamma check**: it is
  the designated gate for the sRGB transfer *direction*, which the plan
  warns is the easiest thing in the phase to get backwards, and it is
  currently only reasoned about. (phase_07 Recorded facts.)

## 5b. ~~Codegen determinism~~ — DONE

**Landed 2026-07-26** as its own commit (`<hash>`). What shipped:

- The two copy-pasted `read_dir` walks became one sorted helper,
  `collect_slang_file_names(dir, suffix)` (src/shaders/build_tasks.rs:34), used
  for both the `.shader.slang` and `.compute.slang` lists.
- Three more unsorted iterations in the same file, found while fixing the two the
  entry named: `reflect_slang_module_types`'s shared-module list (the one with
  behavioral consequences — see below), the `alignment_tests` check-crate
  `mod.rs` generation, and `field_size_tripwire`'s `mismatches` accumulation
  (nondeterministic *failure messages*). `shader_branching_snapshots` already
  sorted; that was the idiom followed.
- Regression guard: `slang_file_names_are_sorted` asserts the helper's output is
  sorted and non-empty (a bare `is_sorted()` would pass vacuously on an empty
  vec).
- Churn was exactly as predicted: `src/generated/shader_atlas.rs` plus the two
  `shader_atlas.rs` snapshots, all three verified **pure line reorderings** by
  comparing the sorted added/removed line sets rather than by eyeballing the
  diff. No other generated file or compiled artifact changed, which is itself
  evidence the rest of the codegen was already order-independent.
- Also confirmed: `just shaders` run twice is now byte-identical, and the atlas
  field order matches a sorted listing of `shaders/source/`.

One detail the original entry didn't have, and the reason this took diagnosing in
P7: of the two things it named, only the **struct fields** were visibly affected in
the committed file. rustfmt's `reorder_modules` (on by default) re-sorts the
`pub mod` lines after generation, so `src/generated/shader_atlas.rs` always looked
tidy while `init()` and the struct body were scrambled. The snapshots are taken
*pre-rustfmt*, so they saw the raw order for both — hence a snapshot that
disagreed with a committed file that looked fine.

Transitive win beyond what the entry described: sorting the shader list also
pins struct *declaration* order inside generated shared-module files, since
`collect_shared_modules` pushes into each module's `Vec` in shader-iteration
order (the `BTreeMap` there only made the module keys deterministic).

**Consequence — the fix relocated a hazard rather than removing it.** Sorting
`reflect_slang_module_types` makes deterministic a map that resolves struct names
by *silent last-write-wins* (`src/shaders.rs:247`). The winner is now
reproducible but arbitrary ("whichever module name sorts last"). No collision
exists in `shaders/source/` today, so nothing is wrong right now — but the
silent-overwrite is worth fixing on its own terms, and is filed as
[`../tech_debt.md`](../tech_debt.md) §4 with the in-repo precedent for how
(`collect_shared_modules` already panics loudly on the analogous case).

Original entry preserved below.

- **The generated shader atlas is filesystem-order dependent.**
  `write_precompiled_shaders` (src/shaders/build_tasks.rs:28, and the
  compute list at :42) collects `.slang` sources straight from
  `std::fs::read_dir` with no sort, so `src/generated/shader_atlas.rs`
  emits its `pub mod` / struct-field lines in whatever order the
  filesystem hands back. That order differs between machines — and even
  between two states of the same directory — which makes the
  `generated_files` and `alignment_tests` snapshots for
  `src/generated/shader_atlas.rs` fail spuriously with a pure-reordering
  diff, on a pristine tree, with no source change at all. Sorting the
  `read_dir` results would make it deterministic. Discovered in P7, where
  it had to be diagnosed and worked around (the atlas snapshots were
  discarded rather than accepted) to tell real churn from noise.

## 6. Doc reconciliation

- ~~**Where does `tev_ir.rs` land?**~~ **Answered by
  [`phase_08.md`](phase_08.md) decision 1: P8.** phase_02's "Out of scope"
  said the TevMaterialDesc IR is built in *P6*; the master plan §6 puts the
  full interpreter + final subset gate in *P8*, and P6 as implemented builds
  no TEV code. P8 owns it, as a **validation-only** converter module — it
  changes no manifest bytes, so `scripts/link_converted.sha256` is its own
  correctness gate. It also has to live converter-side rather than in the
  example, because three of the fields the gate must assert on
  (`TexMatrix::projection`/`map_mode`/`is_maya`, `fog`, `indirect`) are parsed
  from MAT3 but dropped before the manifest.
