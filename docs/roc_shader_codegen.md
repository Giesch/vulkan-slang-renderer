# Roc shader codegen

`mltrs shaders compile` can generate importable Roc shader data without adding a
Roc dependency to normal builds or tests. Generation compiles Slang through the
same reflection crate as Rust codegen; it never invokes `roc`.

## Selection and paths

With no `--language`, the command checks only regular files directly inside
`--crate-dir` (default `.`):

- `main.roc` alone selects Roc.
- `Cargo.toml` alone selects Rust.
- Both or neither require `--language rust|roc`.

Explicit selection does not require a marker. Ancestor and descendant markers
are ignored. `--rust-dir` and explicit `--import-root` are Rust-only;
`--roc-dir` is Roc-only. Invalid selection/option combinations fail before any
output is changed.

Roc defaults are:

- input: `<project>/shaders/source/`
- Roc modules: `<project>/Generated/`
- SPIR-V: `<project>/shaders/compiled/`

`--source-dir`, `--roc-dir`, and `--compiled-dir` preserve their existing
process-current-directory-relative behavior when passed as relative paths.
Generated modules import every stage with an importer-relative
`import "..." as name : List(U8)`, including paths containing spaces.

The selected Roc and compiled directories are tool-owned and replaced after a
successful run. Empty readable source directories therefore produce a valid
empty `ShaderAtlas.roc` and remove stale managed files. Keep distinct
`--compiled-dir` values to retain Rust and Roc outputs at the same time. Source
and output trees must be disjoint; output directories must not overlap each
other or contain the project root.

The checked-in Roc example is `roc-platform/examples/basic-triangle/main.roc`.
Run `just roc-platform roc-shaders basic-triangle` to compile its
`shaders/source/` into its own `shaders/compiled/` and `Generated/` directories.
`just roc-platform roc-shaders` with no argument compiles every
`roc-platform/examples/*/` directory.
The host retains its separate Rust shader sources and bindings.

## Public modules

Import `ShaderAtlas` from `Generated/ShaderAtlas.roc`. It exposes:

- `shader_names`, in deterministic source-name order;
- one snake_case value per graphics or compute shader;
- complete typed reflection for that shader;
- `stages.vertex` and `stages.fragment`, or `stages.compute`, as `List(U8)`;
- original reflected source and entry-point names;
- compute workgroup dimensions.

Each shader module `Generated/<Module>.roc` also exposes one
`<snake_type>_type : ShaderReflection.StructType` value per reflected struct type:
parameter-block and push-constant element types, nested struct fields, pointer
pointees, and structured resource results. The values are sorted by name, and
`reflection` refers to them by name. Entry-point parameter structs stay inline
in `reflection`. Two struct types with the same name and different fields in
one shader fail generation.

Generated modules import `pf.ShaderReflection`, so the app header must bind the
platform under the shorthand `pf`. `roc-platform/platform/ShaderReflection.roc`
defines the lossless typed reflection schema and the shared logical types. The
schema preserves ordered lists, all binding/field/resource/pointer/enum/layout
variants, option distinctions, numeric values, and original strings. Rust
`usize` metadata is checked and emitted as `U64`; enum values remain `I64`, with
their signed/unsigned tag kind.

`ShaderTypes.roc` contains the project's reachable struct and enum types. Field
types map to `ShaderReflection` types or to nested project types:

| Reflected value | Roc logical value |
| --- | --- |
| float/int/uint/u64 scalar | `F32` / `I32` / `U32` / `U64` |
| vector | `ShaderReflection.Float{n}` / `Int{n}` / `Uint{n}` / `Uint64x{n}` for widths 1 to 4, a record with `x`, `y`, `z`, `w` fields as applicable |
| supported matrix | `ShaderReflection.Float4x4` / `Int4x4` / `Uint4x4`, a row-major record with `row_0` through `row_3` of the vector alias |
| fixed vector array | `List(alias)`; required count/stride remain in reflection |
| physical pointer | nominal `ShaderReflection.PointerAddress` wrapping `U64` |
| descriptor handle | distinct nominal `ShaderReflection.DescriptorHandle` wrapping `U64` |
| texture/storage resource | nominal `ShaderReflection.ResourceReference` wrapping `Str` |
| enum | typed cases plus an explicit numeric-tag function |

The `ShaderReflection` type names are reserved: a shader struct named `Float3`,
`Float4x4`, or `PointerAddress` fails generation.

Original reflected spelling is retained in reflection data. Generated nonempty
records use multiline fields with trailing commas. One blank line separates
sibling definitions inside every generated module. Generated values and fields
use snake_case; modules, types, and tags use PascalCase. Invalid or
reserved identifiers, normalized collisions, incompatible shared definitions,
and case-insensitive output filename collisions fail before replacement.

## Guarantees and limits

Generated values are logical application data. They do **not** guarantee GPU
memory layout, padding, ownership, pointer dereference, descriptor validity,
resource lookup, upload behavior, or renderer integration. No encoders,
pipeline builders, render-graph helpers, or host ABI are generated.

The separate `roc-platform` host still renders its Rust-owned shader and uses
Rust bindings under `src/generated/`. Roc codegen does not change runtime shader
selection, hot reload, or platform/compiler pins.

## Verification

`just test` covers generation, schemas, names, paths, manifests, stale cleanup,
safety checks, and Rust regressions without requiring Roc.

`just roc-codegen-test [ROC]` is the explicit real-compiler gate. It reports the
selected executable/version, generates fresh graphics and compute fixtures,
checks and formats every module against `roc-platform/platform/main.roc` as the
app's platform, and evaluates full imported SPIR-V contents and
representative reflection from a consumer running in another working directory.
It also proves missing imports and false byte expectations fail. `ROC` defaults
to `roc` on `PATH`; missing/incompatible compilers fail rather than skip.
`just pre-commit` runs this gate when `roc` is on `PATH`.
