# Pre-P8 mini-phase: vec4 arrays in parameter blocks

Plan for the restricted arrays-in-uniform-buffers support scoped in
[`follow_up.md`](follow_up.md) §1 and motivated by master plan
([`../link_rendering.md`](../link_rendering.md)) risk #4: the shader-atlas
codegen currently `todo!`-panics on any array field in a parameter block,
which forced P6's BDA workaround for the P8 TEV interpreter. **Scope decided
with the user (2026-07-24): support `float4[N]`, `uint4[N]`, `int4[N]` — and
only those — in both std140 (uniform) and std430 (BDA-pointee "storage")
contexts, with generated const-assert layout proofs and alignment-test
coverage.** This is renderer/codegen work, independent of the converter and
example tracks; it unblocks (but does not implement) the master plan §3 flat
`ToonLinkParams` layout. Estimated: ~1 day. Line numbers verified at
`9d8e468`.

**Why this subset is safe by construction**: the std140 stride hazard exists
only for elements smaller than 16 bytes (`float[N]`, `float2[N]`, `float3[N]`
all get stride rounded up to 16, so a naive Rust array is silently wrong) and
for arrays of structs (struct-size rounding). For 16-byte vector elements,
size = alignment = stride = 16 in **both** std140 and std430 — no rounding,
no inter-element padding, and no layout divergence between the two rule sets.
`[glam::Vec4; N]` / `[glam::UVec4; N]` / `[glam::IVec4; N]` are exactly
N×16 contiguous bytes, and the existing per-field
`size_of::<{type}>() == {reflected size}` const asserts turn any stride
disagreement into a compile error.

**Goal**: after this phase, a shader can declare

```slang
struct Params {
    float4 konst[4];
    uint4  stageColor[8];
    int4   offsets[3];
}
```

in a `ParameterBlock` uniform struct, a nested struct, or a BDA pointee
struct, and `just shaders` generates `pub konst: [glam::Vec4; 4]` etc. with
compile-time `offset_of!`/`size_of` proofs; any *other* array element type
produces an immediate, actionable error (not a `todo!` panic). Bare `uint4`/
`int4` vector fields also work (they don't today), closing phase_06 risk #1's
"no generated file contains a `UVec4`" gap as a side effect.

**Deliverables**

1. `ScalarType::Int32` end to end (reflection, JSON model, scalar and vector
   field mapping)
2. Bare `uint4` → `glam::UVec4` and `int4` → `glam::IVec4` vector fields
3. `StructField::Array` JSON variant + reflection walker arm with a hard
   validation gate (16-byte vector elements only), replacing the `todo!` for
   arrays with a helpful error for everything outside the subset
4. Codegen: array fields emitted as `[glam::{Vec4,UVec4,IVec4}; N]` with the
   existing padding/offset/const-assert machinery extended to cover them;
   `field_size_tripwire` covers arrays
5. Two new alignment-test shaders (`shaders/test/std140_arrays.shader.slang`,
   `std430_arrays.shader.slang`) exercising all three element types in
   top-level, nested-struct, and BDA-pointee positions, plus unit tests for
   the rejection path
6. No changes to existing production shaders, examples, or `Cargo.toml`;
   snapshot churn is additions plus the *test* atlas index only; Recorded
   facts below filled in

## Codegen facts this phase relies on

All at `9d8e468`; re-verify line numbers before editing.

- **Reflection walker**: `reflect_struct_fields`
  (src/shaders/reflection/parameters.rs:163) matches on
  `field_type_layout.kind()` — Scalar (178) / Vector (189) / Matrix (229) /
  Struct (248) / Resource / Pointer arms; arrays fall to
  `todo!("field type layout kind not handled")` at **parameters.rs:411**.
  `scalar_from_slang` (its `todo!` at 433) knows Float32/Uint32/Uint64 only —
  **no Int32**. The walker is shared by plain params, nested structs, and BDA
  pointees (`in_pointer_pointee` recursion), so one Array arm covers both
  layout contexts.
- **JSON model** (src/shaders/json/parameters.rs): `StructField` enum
  (serde tag = "kind") with Scalar/Vector/Struct/Matrix/Resource/Pointer
  variants; `Binding::Uniform(OffsetSizeBinding { offset, size })` carries
  reflected placement. Additive enum variants don't perturb existing
  serialized output.
- **Field generation** (src/shaders/build_tasks.rs): `gather_struct_defs`
  (851) maps `StructField` → `GeneratedStructFieldDefinition { field_name,
  type_name: String, offset, size }` (1089). The vector arm (908–916) maps
  **Float32 only** — `(t, c) => panic!("vector not supported")` is where bare
  `uint4` dies today. The matrix arm's rejection message (986–997) is the
  house style for "unsupported with guidance."
- **Layout machinery is offset-driven, not derived**:
  `generate_std140_struct_fields` (778) / `generate_std430_struct_fields`
  (712) read reflected offsets via `field_offset_size` (1161), insert
  `_padding_N: [u8; N]` fields, and `check_rust_placeable` (1228) panics if a
  reflected offset isn't a multiple of the emitted Rust type's alignment.
  Three **string-keyed type tables** need array awareness: `field_alignment`
  (1179, GPU alignment), `rust_type_alignment` (1205, Rust alignment), and
  the test helper `rust_size_of` (~1600).
- **Const asserts are generated generically**: `layout_assert_lines` (1063)
  emits `offset_of!({struct}, {field}) == {reflected offset}` and
  `size_of::<{type_name}>() == {reflected size}` for every field with an
  offset; the templates just splice them
  (templates/shader_atlas_entry.rs.askama:49, shader_shared_module emits
  fields identically). **So array fields need zero template changes**, and
  `size_of::<[glam::UVec4; 8]>() == 128` — the stride proof — is emitted for
  free.
- **Test harness**: `alignment_tests` (build_tasks.rs:1495) compiles every
  shader in `shaders/test/` (currently 6 std140_\* + 6 std430_\* + 3
  pointer\_\* + `check_crate`), snapshots the generated `.rs`/`.json`, then
  **`cargo check`s the generated code in `shaders/test/check_crate`** — the
  const asserts are compile-verified in CI. `field_size_tripwire` (~1670)
  independently diffs every field's Rust size against its reflected GPU size.
  `small_matrix_fields_are_rejected` (~1581) is the pattern for rejection
  unit tests.
- **glam alignment facts**: `glam::Vec4` is align-16 (asserted in every
  generated file; requires glam without `scalar-math`); `glam::UVec4` /
  `glam::IVec4` are `#[repr(C)]` align-4. Harmless: element stride is
  `size_of` (16) regardless of alignment, generated structs are
  `#[repr(C, align(16))]`, `check_rust_placeable` checks against *Rust*
  alignment (4 divides every 16-multiple offset), and the `offset_of!`
  asserts prove final placement.

## Step 1 — `ScalarType::Int32` + bare `uint4`/`int4` vectors

Smallest slice first; arrays depend on it.

- Add `Int32` to `ScalarType` (JSON model) and an arm in `scalar_from_slang`
  (parameters.rs:433 area) for `slang::ScalarType::Int32`.
- Scalar field arm (build_tasks.rs:856): `ScalarType::Int32 => "i32"`
  (`field_alignment` already knows `"i32"`; add it to `rust_type_alignment`
  and the tests' `rust_size_of`).
- Vector arm (build_tasks.rs:908): add
  `(ScalarType::Uint32, 4) => "glam::UVec4"` and
  `(ScalarType::Int32, 4) => "glam::IVec4"`. Keep every other combination in
  the `panic!` (2/3-component integer vectors stay unsupported — not needed,
  and `UVec3`-style types have the vec3-padding trap).
- Extend the three string-keyed tables with `glam::UVec4` / `glam::IVec4`
  entries: GPU alignment 16, Rust alignment `align_of` (4), size 16.
- Confirm the generated-file preamble's align assert stays Vec4-only (the
  UVec4/IVec4 align-4 story is covered by `offset_of!` asserts, not an
  `align_of` assert — add a code comment saying why).

**Gate:** a scratch shader with `uint4 control; int4 signed;` uniform fields
generates, compiles, and asserts clean; `just test` byte-identical for all
existing snapshots (additive enum variants must not move existing output).

## Step 2 — reflection: `Array` walker arm + JSON variant + validation gate

JSON model (src/shaders/json/parameters.rs), house style (camelCase, tagged):

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrayStructField {
    pub field_name: String,
    pub binding: Binding,          // Uniform { offset, size = len * 16 }
    pub element_scalar_type: ScalarType,  // Float32 | Int32 | Uint32
    pub element_count: usize,      // array length N
    pub element_stride: usize,     // reflected; gate guarantees == 16
}
// StructField gains: Array(ArrayStructField)
```

Walker arm (`slang::TypeKind::Array` in `reflect_struct_fields`):

1. `element_count()` → N; `element_type_layout()` → element layout.
2. **Validation gate, factored as a pure function** so it unit-tests without
   slang fixtures:

   ```rust
   /// Only 16-byte vector elements (float4/int4/uint4) have
   /// stride == size in BOTH std140 and std430; everything else
   /// would need stride-aware padding the codegen doesn't model.
   fn validate_array_element(
       field_name: &str,
       element_kind: slang::TypeKind,   // must be Vector
       component_count: usize,          // must be 4
       scalar: ScalarType,              // Float32 | Int32 | Uint32
       reflected_stride: usize,         // must be 16
   ) -> anyhow::Result<()>
   ```

   Error message names the field and the way out (house style, like the
   matrix arm): `"array field '{name}': only float4/int4/uint4 element
   arrays are supported (16-byte stride); use a BDA buffer of flat structs,
   or named fields"`. This **replaces** the `todo!` outcome for the common
   case; other still-unknown type kinds keep the `todo!` at 411.
3. Stride comes from reflection, not derivation — the slang `TypeLayout`
   element-stride accessor for the uniform category (exact API name:
   verify at implementation; fallback: `binding.size / element_count`,
   asserting divisibility). Belt: the gate checks it equals 16; suspenders:
   the emitted `size_of` assert re-proves it at compile time.
4. The arm runs identically under `in_pointer_pointee` — arrays inside BDA
   pointee structs come along for free (std430 stride for vec4 elements is
   also 16). Reject arrays where no `Binding::Uniform` offset exists
   (vertex-input / varying contexts): bail with "arrays are only supported
   in uniform/pointee struct fields".

Unit tests beside `small_matrix_fields_are_rejected`: `validate_array_element`
accepts (Vector, 4, each of the three scalars, 16) and rejects scalar
elements, `float2`/`float3` elements, 16≠stride, and non-vector kinds, with
the message asserted.

**Gate:** `cargo check --all` + the new unit tests green (no consumer of the
variant yet — `gather_struct_defs` still rejects it until Step 3; leave a
`todo!` there for exactly one commit or land Steps 2+3 together).

## Step 3 — codegen: emit `[glam::…; N]` fields

- `gather_struct_defs` Array arm: type string
  `format!("[{elem}; {n}]")` with elem from the Step-1 mapping
  (`glam::Vec4` / `glam::UVec4` / `glam::IVec4`).
- `field_offset_size` (1161): `StructField::Array(a) => Some(&a.binding)` —
  offset/size flow into the existing padding + assert machinery untouched.
- Make the three string-keyed tables array-aware with one small parser
  (`fn parse_array_type(s: &str) -> Option<(&str, usize)>` for `"[T; N]"`):
  - `field_alignment`: array → alignment of element (16 for all three).
  - `rust_type_alignment`: array → element's Rust alignment.
  - tests' `rust_size_of`: array → N × element size (this is what makes
    `field_size_tripwire` cover arrays).
- No template changes (fields and assert lines are already generic strings).
  `#[derive(Serialize)]` on generated structs: `[T; N]` serializes via serde
  const generics; glam's serde feature covers `UVec4`/`IVec4` — verify the
  feature is on (it is for `Vec4` today; same impl family).

**Gate:** scratch shader with `float4 k[4]; uint4 s[8];` generates a struct
whose emitted asserts include `size_of::<[glam::Vec4; 4]>() == 64` and
`size_of::<[glam::UVec4; 8]>() == 128`, and it compiles.

## Step 4 — alignment-test shaders + snapshots

Two new committed test shaders, modeled on the existing pairs:

- **`shaders/test/std140_arrays.shader.slang`** — a `ParameterBlock` uniform
  struct stressing placement, not just existence:

  ```slang
  struct ArrayData {
      float  lead;          // forces the first array off offset 0 →
      float4 konst[4];      //   16-align padding before the array
      uint4  stages[8];
      float2 wedge;         // between arrays: trailing-position math
      int4   offsets[3];    // odd count
      float  tail;          // struct-size rounding after an array
  }
  struct Nested { uint4 inner[2]; float pad; }   // array inside nested struct
  ```

- **`shaders/test/std430_arrays.shader.slang`** — same field mix inside a
  BDA pointee struct (model on `pointer_pointee_layout.shader.slang`), so the
  std430 path and the pointee-size cross-assert
  (`computed std430 size of pointee … disagrees with slang reflection`,
  build_tasks.rs:878) both exercise arrays.

Coverage this buys, all automatic: insta snapshots of the generated `.rs` +
`.json`; the `check_crate` `cargo check` compiling every emitted
`offset_of!`/`size_of` assert; `field_size_tripwire` diffing each array
field's Rust size against reflection. Expected snapshot churn: the new
per-shader snaps + the *test* atlas-index snap; **every production snapshot
(`shaders/source/`) byte-identical.**

**Gate:** `just test` green with exactly that churn; `just lint` clean.

## Step 5 — runtime proof + wrap-up

Compile-time proofs don't watch the GPU actually read the data, so one
eyeball check (mirroring phase_06 Step 1's design, but now expected to
*pass*): a **throwaway, never-committed** shader with
`uint4 pattern[8]` + `float4 tints[4]` written from Rust with a known
pattern and rendered as color bands; verify the bands on screen, record the
result, delete the shader, confirm `git status` clean and a follow-up
`just shaders` + `just test` run byte-identical.

Then: `cargo fmt`, `just lint`, `cargo check --all`, and a quick
`timeout 3 just dev multi_mesh` to confirm nothing regressed in a real
example (no production shader changed, so this is a formality).

**Gate:** bands render correctly (each band's color matches the CPU-written
pattern value); no residue; full suite green.

## Test plan

**Automated (`just test` / CI):**

- Insta: additions only (std140_arrays / std430_arrays `.rs` + `.json`,
  test-atlas index); all production snapshots byte-identical.
- `alignment_tests`' `check_crate` compile — the const asserts for array
  fields are the load-bearing check.
- `field_size_tripwire` — now array-aware via `rust_size_of`.
- New unit tests: `validate_array_element` accept/reject matrix (element
  kind, component count, scalar type, stride ≠ 16), rejection message text.
- `cargo check --all`, `just lint`.

**Eyeball (results → Recorded facts):**

1. Step 5's pattern-band render: per-band color == CPU value for `uint4[8]`
   and `float4[4]`.
2. Generated `.rs` for the test shaders read once by a human: field order,
   padding, and asserts look like the std140_vectors precedent.
3. Rejection UX: temporarily add `float bad[8]` to a scratch shader — error
   names the field and suggests the alternatives; no `todo!` backtrace.

## Verification (exit checklist)

- [x] `ScalarType::Int32` + bare `uint4`/`int4` vectors generate and assert
      clean; existing snapshots byte-identical after Step 1
- [x] `StructField::Array` variant + walker arm landed; `todo!` no longer
      reachable for arrays; rejection message helpful and unit-tested
- [x] `[glam::Vec4|UVec4|IVec4; N]` fields emitted with offset/size asserts;
      no template changes needed (confirmed in the diff — zero template churn)
- [x] `field_alignment` / `rust_type_alignment` / `rust_size_of` array-aware
- [x] std140_arrays + std430_arrays test shaders committed; snapshots
      reviewed; `check_crate` compile green; `field_size_tripwire` green
- [x] Runtime pattern-band check passed and recorded; throwaway shader
      removed; tree clean
- [x] `just test`, `just lint`, `cargo check --all` green; production
      snapshot churn zero
- [x] Recorded facts filled in

## Recorded facts (fill in after gates pass)

```
commit:                     (pending — this session's work, fill at commit)

slang stride API used:      TypeLayout::element_stride(ParameterCategory::Uniform)
                            (real accessor exists in the Giesch/slang-rs fork @ 40be816;
                            no size/count fallback needed). element_count() ->
                            Option<usize> and element_type_layout() are also on
                            TypeLayout; the element's scalar type comes from
                            element_type_layout().element_type_layout().scalar_type(),
                            same chain as the Vector arm.

pattern-band check:         PASSED. uint4 pattern[8] written as RGB
                            255-red/green/blue/yellow/magenta/cyan/white/128-gray;
                            float4 tints[4] as orange/teal/purple/dark-green.
                            Screenshot showed all 12 bands in exact CPU order
                            (8 top, 4 bottom), colors matching per band.

rejection UX:               "array field 'bad': only float4/int4/uint4 element
                            arrays are supported (16-byte stride); got element
                            kind Scalar with 0 components, scalar type None,
                            stride 16; use a BDA buffer of flat structs, or
                            named fields" — anyhow error through prepare_shaders'
                            unwrap, no todo! backtrace. Note: slang reports
                            stride 16 even for float[8] in std140 (the rounded
                            stride that makes scalar arrays hazardous); the gate
                            rejects on element kind, so the stride check is the
                            backstop for 16-byte-looking-but-wrong cases.

snapshot churn:             Added: alignment_tests@{shaders__compiled__std140_arrays.json,
                            shaders__compiled__std430_arrays.json,
                            src__generated__shader_atlas__std140_arrays.rs,
                            src__generated__shader_atlas__std430_arrays.rs};
                            modified: the *test* atlas index snap
                            (alignment_tests@src__generated__shader_atlas.rs).
                            Zero production (shaders/source) snapshot churn —
                            verified with INSTA_UPDATE=no full runs after Step 1
                            and at the end.

deviations discovered:
  1. Steps 2+3 landed together by necessity: gather_struct_defs and
     field_offset_size match exhaustively over StructField, so adding the
     Array variant is a compile error until both arms exist (the "todo! for
     one commit" option never existed). check_field_sizes in the tests is a
     third exhaustive match that needed an arm.
  2. The "why no UVec4/IVec4 align assert in the preamble" comment lives at
     rust_type_alignment in build_tasks.rs, not in the templates — a template
     comment would be emitted into every generated file and churn all
     snapshots.
  3. validate_array_element takes scalar_type: Option<ScalarType> (non-vector
     elements have no scalar type to report), slightly different from the
     planned signature.
  4. The committed test shaders additionally carry bare uint4/int4 fields
     (flags/bias) so Step 1's bare-integer-vector support has permanent
     committed coverage, not just the deleted Step-1 scratch gate.
  5. std140 struct sizes matched predictions exactly (ArrayData 368 bytes,
     Nested 48 incl. 12-byte tail padding); std430 pointee layout is
     byte-identical to the std140 struct for this field mix, as the
     16-byte-element theory predicts.
```

## Out of scope

- **Sub-16-byte element arrays** (`float[N]`, `float2[N]`, `float3[N]`) —
  the stride-rounding cases this subset exists to exclude; the gate's error
  points at the alternatives.
- **Arrays of structs, arrays of matrices, nested arrays** (`float4[4][2]`),
  unbounded/runtime-sized arrays.
- **2/3-component integer vectors** (`uint2`, `int3`, …) as bare fields —
  not needed; only the 4-component forms land.
- **P8's actual `ToonLinkParams` migration** — this phase only makes the
  master plan §3 flat layout *possible*; whether P8 uses it or keeps the BDA
  decision recorded in [`phase_06.md`](phase_06.md) Step 1 is P8 planning's
  call.
- **Planning-doc updates** ([`follow_up.md`](follow_up.md) §1's cost
  estimate, master plan risk #4, phase_06's recorded decision) — explicitly
  deferred per user instruction; reconcile them when this phase lands or at
  P8 planning, whichever comes first.

## Risks / open questions

1. **Slang's array reflection surface.** The rust `shader-slang` crate's
   exact accessors for element stride (and whether `element_count()` is on
   the type or the layout) need verification at implementation time. Fallback
   is arithmetic on the reflected binding size, which slang reports for the
   whole array; either way the gate pins stride == 16 and the const asserts
   re-prove it downstream, so an API misread cannot ship a wrong layout.
2. **`UVec4`/`IVec4` serde + alignment assumptions.** glam's integer vectors
   are align-4 and serde support rides the same feature as `Vec4`; both are
   believed fine (see Codegen facts) but are exactly the kind of thing the
   Step-1 gate exists to catch before arrays stack on top.
3. **Additive-variant snapshot neutrality.** Steps 1–2 add enum variants to
   serde types embedded in every compiled-shader JSON. Tagged additive
   variants don't change existing output, but the Step-1 gate (byte-identical
   existing snapshots) verifies rather than assumes it.
4. **`struct_defs_compatible` (build_tasks.rs:~1250) compares generated
   fields for shared types.** Array fields compare as their `type_name`
   strings — deterministic, so no action expected; noted in case a shared
   module ever declares the same struct with differing array lengths (the
   panic there is the correct outcome).
5. **`check_rust_placeable` with align-4 integer vectors** is strictly weaker
   than for `Vec4` (any 4-multiple offset passes). Not a gap: placement truth
   comes from the emitted `offset_of!` asserts, which are exact.
