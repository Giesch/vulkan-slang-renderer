# Shader enums: implementation plan

## Context

Slang shaders can declare C-style enums, but today they cannot cross the reflection boundary.
`shaders/source/paint_display.shader.slang:7` declares

```slang
enum DebugView : uint {
    Pigments = 0,
    WetAreaMask = 1,
}
```

and then has to declare the ParameterBlock field as `uint debugView` (line 26), comparing with
`displayParams.debugView == uint(DebugView.WetAreaMask)` (line 113). The generated Rust side sees
only `pub debug_view: u32`, so callers write magic numbers with no compiler help and no way to
discover the legal values.

The goal: let a shader declare `DebugView debugView;` directly in an exposed ParameterBlock, and
have codegen emit a matching C-style Rust enum. The same integer still crosses the boundary — the
GPU-side bytes are byte-for-byte identical to what `uint debugView` produces today. What we gain is
type safety and readability in the generated bindings.

## Settled decisions

- **Tag types supported**: `uint` → `#[repr(u32)]`, `int` → `#[repr(i32)]`, `uint16_t` →
  `#[repr(u16)]`, `uint8_t` → `#[repr(u8)]`. A bare `enum X { .. }` (no tag annotation) reflects as
  `int32` and is therefore accepted as `#[repr(i32)]`.
- **Scope**: enum fields are allowed anywhere a scalar is allowed today — std140 ParameterBlock
  structs, nested structs inside them, and std430 BDA pointee structs. Enums declared in shared
  `.slang` modules get hoisted into the shared generated module exactly like shared structs.
- **Generated shape**: derives + `repr` + `Default` (first case) + `From<E> for <tag>` +
  `TryFrom<tag> for E`. See §3.
- **New `EnumTagType` in the JSON schema, separate from `ScalarType`.** Widening `ScalarType`
  (`json/parameters.rs:267`) to add `Int32`/`Uint16`/`Uint8` would, as a side effect, silently start
  accepting plain `int`/`uint16_t`/`uint8_t` *scalar* fields that `scalar_from_slang`
  (`reflection/parameters.rs:428`) deliberately rejects today. Keep the blast radius on enums.
- **Out of scope**: enum-typed *vertex input* attributes. They would reach the `vk::Format` match at
  `build_tasks.rs:316` and hit its existing `todo!()`; that is an acceptable, loud failure.

## Verified facts

All line numbers verified at `27b6a98`; re-verify before editing.

1. **Slang lays an enum out as its tag type, discarding the enum identity.**
   `slang/source/slang/slang-type-layout.cpp:6128-6137`:
   ```cpp
   else if (auto enumDeclRef = declRef.as<EnumDecl>())
   {
       // We lay out an enumeration type as its tag type.
       return _createTypeLayout(context, enumDeclRef.getDecl()->tagType);
   }
   ```
   Confirmed empirically with `slangc -reflection-json`: a field declared `DebugView debugView`
   emits `{"kind": "scalar", "scalarType": "uint32", "offset": 8, "size": 4}` — no trace of the enum.
   **Consequence: `field_type_layout.kind()` returns `TypeKind::Scalar` for an enum field.** The
   existing `match field_type_layout.kind()` in `reflect_struct_fields` will *not* panic on an enum
   field; it will silently degrade it to a `u32`. This is the central constraint of the design.

2. **The enum IS recoverable from the *declared* type, off the variable rather than the layout.**
   `VariableLayout::ty()` → `Variable::ty()` → `spReflectionVariable_GetType`
   (`slang-reflection-api.cpp:3242-3256`) returns the declared type via `varLayout->varDecl`
   (`:3481-3489`), which for an enum field is the `EnumDeclRefType`. Its kind is
   `SLANG_TYPE_KIND_ENUM` (`:534-537`). `Type::name()` gives `"DebugView"`.

3. **On an `Enum`-kind `Type`, Slang overloads the struct-field API to mean cases.**
   - `spReflectionType_GetFieldCount` (`:572-575`) → number of `EnumCaseDecl`s
   - `spReflectionType_GetFieldByIndex` (`:603-607`) → each case as a `SlangReflectionVariable`
   - `spReflectionType_GetElementType` (`:687-694`) → the tag type
   - `spReflectionVariable_GetDefaultValueInt` (`:3414-3424`) → the case's integer value

   All four are already wrapped by the pinned `shader-slang` fork (`Cargo.lock` rev
   `40be8169f2a8a2ebe501cacabfd056a88054dc3f`, checked out at
   `~/.cargo/git/checkouts/slang-rs-fd5baabc349ce0cb/40be816/src/reflection/`): `ty.rs` has `kind`,
   `name`, `fields`, `element_type`, `scalar_type`; `variable.rs` has `default_value_int() ->
   Option<i64>`; `variable_layout.rs` has `ty()`. **No upstream crate change is needed.**

4. **Tag type reflection, measured** (slangc, `-target spirv -profile spirv_1_5`, fields packed
   between two `float`s):

   | Slang tag | reflected `scalarType` | size | offset in test struct |
   |---|---|---|---|
   | `uint` | `uint32` | 4 | 4 |
   | `int` | `int32` | 4 | 8 |
   | `uint16_t` | `uint16` | 2 | 12 |
   | `uint8_t` | `uint8` | 1 | 14 |
   | *(none)* | `int32` | 4 | 16 |

   Offsets are naturally aligned and tightly packed — alignment equals size for every tag type.

5. **`uint8_t`/`uint16_t` tags require device features the renderer does not enable.** The same test
   shader, when it actually *reads* the small fields, makes Slang emit:
   ```
   OpCapability UniformAndStorageBuffer8BitAccess
   OpCapability UniformAndStorageBuffer16BitAccess
   OpCapability Int16
   OpCapability Int8
   ```
   `src/renderer.rs:3373-3382` enables only `storage_buffer8_bit_access`, and only under
   `cfg!(debug_assertions)`. See §7 — this is a real deliverable, not a footnote.

6. **Two alignment helpers will silently do the wrong thing for a new type name.**
   - `field_alignment` (`build_tasks.rs:1180-1195`) falls through to `_ => 16`. An enum field named
     `DebugView` would be treated as 16-byte aligned, inflating `max_alignment` and therefore
     `expected_size` in `generate_std430_struct_fields` (`:714-776`) — which then trips the
     `assert_eq!(expected_size, ptr.pointee_size)` at `:878`.
   - `rust_type_alignment` (`build_tasks.rs:1205-1223`) returns `None` for unknown names, so
     `check_rust_placeable` (`:1228`) skips the check entirely rather than enforcing it.

7. **Shared-module hoisting only looks at structs.** `reflect_shared_module_types`
   (`src/shaders.rs:241-247`) filters `child.kind() == slang::DeclKind::Struct`. `DeclKind::Enum`
   exists in the generated bindings (value 7) and is re-exported by the crate.

## 1. Reflection: detect enum fields

`src/shaders/reflection/parameters.rs`, in `reflect_struct_fields` (`:163`).

Because of Verified fact 1, the enum check must happen **before** the existing
`match field_type_layout.kind()`, not as a new arm inside it:

```rust
for field in struct_type_layout.fields() {
    let field_name = field.name().unwrap().to_string();
    let field_type_layout = field.type_layout().unwrap();
    let binding = param_binding(field);

    // An enum's *layout* is its tag type's layout (slang-type-layout.cpp:6128),
    // so field_type_layout.kind() reports Scalar here. The enum identity only
    // survives on the declared type, reached through the variable.
    if let Some(declared) = field.ty()
        && declared.kind() == slang::TypeKind::Enum
    {
        fields.push(StructField::Enum(reflect_enum_field(
            field_name,
            binding.expect("enum field without binding"),
            declared,
        )?));
        continue;
    }

    let field_json = match field_type_layout.kind() { /* unchanged */ };
    ...
}
```

New helper in the same file, next to `scalar_from_slang` (`:428`):

```rust
fn reflect_enum_field(
    field_name: String,
    binding: Binding,
    enum_type: &slang::reflection::Type,
) -> anyhow::Result<EnumStructField> { ... }
```

It must:

- read `enum_type.name()` for the type name (bail if absent — anonymous enums are unsupported)
- read `enum_type.element_type().scalar_type()` and map it through a new `enum_tag_from_slang`
  returning `EnumTagType`; bail with a clear message on anything outside
  uint32/int32/uint16/uint8 (e.g. `uint64_t` tags)
- iterate `enum_type.fields()`; for each case take `.name()` and `.default_value_int()`, bailing if
  a case has no constant value
- bail if the case list is empty (`Default` and `TryFrom` both need a first case), if two cases
  share a value (rustc rejects duplicate discriminants with a much worse message), or if a value
  does not fit the tag type

Do **not** reuse `scalar_from_slang` — it deliberately rejects `int32`/`uint16`/`uint8`.

Compute shaders reach the same function via `reflect_compute_entry_point` (`:439`), so they are
covered for free.

## 2. JSON schema

`src/shaders/json/parameters.rs`. Add a variant to `StructField` (`:80`) and three new types:

```rust
pub enum StructField {
    Scalar(ScalarStructField),
    Vector(VectorStructField),
    Struct(StructStructField),
    Matrix(MatrixStructField),
    Resource(ResourceStructField),
    Pointer(PointerStructField),
    Enum(EnumStructField),          // new
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumStructField {
    pub field_name: String,
    pub binding: Binding,
    pub enum_type: EnumFieldType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumFieldType {
    pub type_name: String,
    pub tag_type: EnumTagType,
    pub cases: Vec<EnumCase>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumCase {
    pub name: String,
    pub value: i64,
}

/// The integer type a slang enum is laid out as. Deliberately separate from
/// ScalarType: widening ScalarType would also start accepting plain int /
/// uint16_t / uint8_t scalar fields, which codegen does not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnumTagType {
    Uint32,
    Int32,
    Uint16,
    Uint8,
}
```

Serialized shape:

```json
{
  "kind": "enum",
  "fieldName": "debugView",
  "binding": { "kind": "uniform", "offset": 8, "size": 4 },
  "enumType": {
    "typeName": "DebugView",
    "tagType": "uint32",
    "cases": [
      { "name": "Pigments", "value": 0 },
      { "name": "WetAreaMask", "value": 1 }
    ]
  }
}
```

`EnumTagType` should carry `rust_type_name()` (`"u32"` / `"i32"` / `"u16"` / `"u8"`), `repr()`
(`"#[repr(u32)]"` …) and `size()` (4/4/2/1) helpers — every downstream consumer wants one of them.

Also extend `field_offset_size` (`build_tasks.rs:1161`) with
`StructField::Enum(e) => Some(&e.binding)`. It matches exhaustively, so the compiler will find it.

## 3. Codegen: enum definitions

`src/shaders/build_tasks.rs`.

New definition type, mirroring `GeneratedStructDefinition` (`:1023`):

```rust
#[derive(Debug, Clone, PartialEq)]
struct GeneratedEnumDefinition {
    type_name: String,
    /// Which slang module this type originated from (None = local to the shader)
    source_module: Option<String>,
    tag_rust_type: String,   // "u32" | "i32" | "u16" | "u8"
    repr: String,            // "#[repr(u32)]" ...
    expected_size: usize,    // 4 | 4 | 2 | 1
    cases: Vec<GeneratedEnumCase>,   // { variant_name: String, value: i64 }
}
```

Variant names come from `heck`'s `to_upper_camel_case`, matching how field names already go through
`to_snake_case`.

Thread a `enum_defs: &mut Vec<GeneratedEnumDefinition>` alongside the existing
`struct_defs: &mut Vec<GeneratedStructDefinition>` through `gather_struct_defs` (`:847`),
`generate_std140_struct_fields` (`:781`), `generate_std430_struct_fields` (`:714`) and both
`collect_*_shader_data` entry points (`:259`, `:564`). Add a `try_add_enum_def` mirroring
`try_add_struct_def` (`:1249`) that panics when two shaders define the same enum name with
different cases.

New arm in `gather_struct_defs`:

```rust
StructField::Enum(enum_field) => {
    try_add_enum_def(enum_defs, /* built from enum_field.enum_type */);

    Some(GeneratedStructFieldDefinition::new_with_align(
        enum_field.field_name.to_snake_case(),
        enum_field.enum_type.type_name.clone(),
        enum_field.enum_type.tag_type.size(),   // alignment == size, per Verified fact 4
    ))
}
```

Target output (for `enum DebugView : uint`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[repr(u32)]
pub enum DebugView {
    #[default]
    Pigments = 0,
    WetAreaMask = 1,
}

const _: () = assert!(std::mem::size_of::<DebugView>() == 4);

impl From<DebugView> for u32 {
    fn from(value: DebugView) -> u32 {
        value as u32
    }
}

impl TryFrom<u32> for DebugView {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::Pigments),
            1 => Ok(Self::WetAreaMask),
            other => Err(other),
        }
    }
}
```

`Debug, Clone, Serialize` match what generated structs already derive (`:890`, `:965`); `Copy`,
`PartialEq`, `Eq` and `Default` are added because an enum is a value type callers will compare and
construct. `#[default]` goes on the first case as declared in Slang, not the numerically smallest.

## 4. Alignment plumbing

Per Verified fact 6, a bare type-name lookup cannot classify an enum. Add an explicit alignment to
the field definition rather than teaching the name-based helpers about enum names:

- `GeneratedStructFieldDefinition` (`:1088`) gains `rust_align: Option<usize>`, set only by the new
  `new_with_align` constructor (`None` from `new` and `padding`). It derives `PartialEq` and feeds
  `struct_defs_compatible` (`:1243`), so this also strengthens the shared-type layout check.
- `field_alignment` (`:1180`) becomes `field_alignment(field: &GeneratedStructFieldDefinition)`,
  returning `field.rust_align.unwrap_or_else(|| field_alignment_by_name(&field.type_name))` with the
  existing body renamed. Only call site: `:746`.
- `check_rust_placeable` (`:1228`) consults `gen_field.rust_align` before falling back to
  `rust_type_alignment(&gen_field.type_name)`, so an enum field at a misaligned offset now fails
  loudly instead of being skipped.

Also add `"u16" => 2` and `"u8" => 1` (and `"i32" => 4`) to `rust_type_alignment` (`:1205`) so the
fallback path is right even if a future caller constructs the field without an explicit alignment.

Test-side: `check_field_sizes` (`:1618`) matches `StructField` exhaustively and needs a
`StructField::Enum(_) => {}` arm. The enum's own size is covered by the emitted
`size_of::<DebugView>() == 4` assert, which `alignment_tests` compiles for real (see §8).

## 5. Templates

`templates/shader_atlas_entry.rs.askama`, `templates/shader_compute_entry.rs.askama`,
`templates/shader_shared_module.rs.askama` each gain an `enum_defs` loop immediately *before* the
existing `{% for def in struct_defs %}` block. The three template structs
(`ShaderAtlasEntryModule` `:502`, `ShaderComputeEntryModule` `:512`, `SharedModuleTemplate` `:1447`)
each gain `enum_defs: Vec<GeneratedEnumDefinition>`.

The struct-emitting block is already duplicated verbatim across all three templates; duplicate the
enum block the same way rather than introducing `{% include %}` — consistency with the existing
style beats a one-off refactor here.

Precompute `trait_derive_line()`, `repr()` and the `TryFrom` arms as methods on
`GeneratedEnumDefinition` so the templates stay dumb, matching how `GeneratedStructDefinition`
exposes `trait_derive_line()` / `repr()` / `layout_assert_lines()` (`:1034-1086`).

## 6. Shared modules

- `src/shaders.rs:241-247`: accept `slang::DeclKind::Enum` in addition to `DeclKind::Struct` when
  building `type_to_module`. This is the whole hook — the map is keyed by bare type name, so an enum
  declared in e.g. `particle.slang` will be tagged and hoisted like any shared struct.
- `tag_source_modules` (`:1320`) takes `&mut [GeneratedStructDefinition]`; add a sibling for enum
  defs (or make it generic over a small `HasSourceModule` trait — two tiny functions is fine).
- `collect_shared_modules` (`:1341`) returns `BTreeMap<String, Vec<GeneratedStructDefinition>>`;
  widen it to carry enums per module too, and include enum names in `SharedModuleImport.type_names`
  (`:1334`) so `pub use super::<module>::{ .. }` covers them.
- `render_graphics_shader_file` (`:454`) / `render_compute_shader_file` (`:677`) filter
  `source_module.is_none()`; apply the same filter to `enum_defs`.

Note that `DebugView` in `paint_display.shader.slang` is declared in the *shader* module itself, so
it stays local (`source_module: None`) and is emitted directly into
`src/generated/shader_atlas/paint_display.rs`.

## 7. Renderer device features (uint8/uint16 tags)

Per Verified fact 5, a `uint8_t`/`uint16_t`-tagged enum read in a shader forces
`UniformAndStorageBuffer8BitAccess` / `UniformAndStorageBuffer16BitAccess` / `Int8` / `Int16`.
`src/renderer.rs:3373-3382` currently enables only `storage_buffer8_bit_access`, under
`cfg!(debug_assertions)`.

Move these out of the debug-only block and enable unconditionally:

```rust
let mut vulkan_11_features = vk::PhysicalDeviceVulkan11Features::default()
    .shader_draw_parameters(true)
    .uniform_and_storage_buffer16_bit_access(true);

let mut vulkan_12_features = vk::PhysicalDeviceVulkan12Features::default()
    .timeline_semaphore(true)
    .buffer_device_address(true)
    .uniform_and_storage_buffer8_bit_access(true)
    .shader_int8(true);
```

plus `shader_int16(true)` on the base `vk::PhysicalDeviceFeatures` — `shaderInt16` is a core 1.0
feature and lives there, not in the 1.1/1.2 structs. `storage_buffer8_bit_access` stays where it is
for shader println.

These are all Vulkan 1.2 roadmap-2024 features and universally available on desktop, but they are a
real device requirement — if that is unwelcome, the alternative is to reject `uint8_t`/`uint16_t`
tags in §1 and ship only `uint`/`int`. Decide before P3; P0–P2 are unaffected either way.

## 8. Test shaders and snapshots

- Add `shaders/test/` coverage — that directory feeds `alignment_tests` (`:1495`), which copies the
  generated `.rs` into `shaders/test/check_crate` and runs a real `cargo check`, so the emitted
  `size_of` / `offset_of` asserts are actually compiled. Cover in one or two test shaders:
  - all four tag types in one std140 ParameterBlock struct, with the small ones adjacent so the
    tight packing from Verified fact 4 is exercised
  - an enum field inside a std430 BDA pointee (this is the case that Verified fact 6's
    `field_alignment` bug would break, via the `expected_size` assert at `:878`)
  - an enum field in a nested struct
  - a non-zero-based / non-contiguous case list (e.g. `= 7`) and a negative value on an `: int` enum
- Add negative tests next to `small_matrix_fields_are_rejected` (`:1582`) and
  `default_layout_pointer_is_rejected` (`:1829`), using the same inline-source fixture pattern as
  `structured_buffer` (`:1885`): duplicate case values, empty enum, `uint64_t` tag.
- Snapshot churn to expect, and nothing else: new `.snap` files for the new test shaders, plus the
  `paint_display` `.rs` and `.json` snapshots changing in P5. Every other snapshot must stay
  byte-identical.

## 9. Migrate `paint_display`

Once the pipeline works, change `shaders/source/paint_display.shader.slang`:

- line 26: `uint debugView;` → `DebugView debugView;`
- line 113: `displayParams.debugView == uint(DebugView.WetAreaMask)` →
  `displayParams.debugView == DebugView.WetAreaMask`

and update the Rust callers to pass `DebugView::WetAreaMask` instead of a literal. The GPU bytes are
unchanged, so the watercolor example must render identically.

## 10. Phases & verification

Every phase leaves the repo green: `cargo check --all`, `just shaders`, `just test`, `just lint`.

| Phase | Deliverable | Verify | Est. |
|---|---|---|---|
| **P0** | §1 + §2 — reflection detection and JSON schema. No codegen yet; enum fields reach `gather_struct_defs` and hit an explicit `todo!()`. | Add one enum field to a `shaders/test/` shader, run `just shaders`, inspect `shaders/compiled/*.json` with `jq` — the `"kind": "enum"` node has the right cases, tag and binding offset. `just test` green after `cargo insta accept` for the new JSON snapshot. | 0.5 day |
| **P1** | §3 + §5 — `GeneratedEnumDefinition`, the three template blocks, `uint` tag only. | `just shaders` emits the enum into `src/generated/shader_atlas/`; `cargo check --all` compiles the `size_of` assert; `just test` snapshot diff shows only the new enum. | 1 day |
| **P2** | §4 — alignment plumbing (`rust_align`, `field_alignment`, `check_rust_placeable`) + the std430 pointee test shader. | `alignment_tests` passes, including the `cargo check` of `check_crate`. Perturbation: hand-edit a generated enum to `#[repr(u16)]`, confirm `cargo check` fails on the size assert, then revert. | 0.5 day |
| **P3** | `int` / `uint16_t` / `uint8_t` tags (§1, §2, §3) + §7 renderer features. | `just test`; then `timeout 3 just dev <example using small tags>` with validation layers on — no `VUID-VkDeviceCreateInfo` or missing-capability errors. | 1 day |
| **P4** | §8 — negative tests for duplicate values, empty enum, unsupported tag, anonymous enum. | `just test`; each negative test asserts on the specific error message, not just that it panicked. | 0.5 day |
| **P5** | §9 — migrate `paint_display` + §6 shared-module hoisting, with a shared enum moved into a `.slang` module to exercise it. | `just shaders && just test`; `timeout 3 just dev watercolor` renders identically to `main` (compare screenshots); validation sweep over all examples. | 1 day |

Full verification at the end:

```sh
just shaders && git diff --stat src/generated   # only expected files changed
cargo check --all
just test
just lint
cargo fmt
for ex in basic_triangle depth_texture dragon koch_curve ray_marching sdf_2d \
          serenity_crt space_invaders sprite_batch viking_room watercolor; do
    timeout 3 just dev "$ex" || echo "FAILED: $ex"
done
```

## 11. Risks / open questions

- **`#[repr(u32)]` enums and out-of-range values.** A Rust enum holding a value outside its declared
  variants is undefined behavior. Data flows CPU → GPU only here — `GPUWrite` serializes the struct
  into a uniform buffer and nothing reads it back — so the CPU never materializes a value it did not
  construct. If a readback path (GPU picking, compute → CPU) ever wants an enum field, it must go
  through the generated `TryFrom` rather than `transmute`. Worth a comment in the generated file.
- **`field.ty()` on non-enum fields.** The new check calls `VariableLayout::ty()` on every field,
  including resources and pointers. It should be a cheap declared-type lookup, but if it turns out
  to be `None` or surprising for some existing field kind, the guard is `if let Some(..) && kind ==
  Enum`, which degrades to today's behavior. Confirm during P0 that no existing snapshot changes.
- **Enum inside a resource result type.** `ResourceResultType` (`json/parameters.rs:157`) has its
  own `Scalar`/`Vector`/`Struct` shape reached from `reflect_struct_fields` at `:284-320`. A struct
  result type recurses through `reflect_struct_fields`, so an enum there is handled; a bare enum
  result type is not. Not worth supporting — the existing `todo!()` at `:319` is the right failure.
- **Askama duplication.** Three near-identical enum blocks across templates. Accepted deliberately
  (§5), but if a fourth template appears, factor all of it out at once.
