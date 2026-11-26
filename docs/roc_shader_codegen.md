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
- SPIR-V and reflection JSON: `<project>/shaders/compiled/`

`--source-dir`, `--roc-dir`, and `--compiled-dir` preserve their existing
process-current-directory-relative behavior when passed as relative paths.
Generated modules import every stage with an importer-relative
`import "..." as name : List(U8)`, including paths containing spaces, and the
shader's reflection JSON (`<stem>.json`, or `<stem>.comp.json` for compute, the
same files Rust codegen writes) as `reflection_json : Str`. Every generated
module places its `##` moduledoc after its imports.

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
`roc-platform/examples/*/` directory. The host retains its separate Rust shader
sources and bindings.

## Public modules

Import `ShaderAtlas` from `Generated/ShaderAtlas.roc`. It exposes:

- `shaders_source_dir : Str`, the shader source directory relative to the Roc
  project root (the directory containing `main.roc`), `/`-separated, such as
  `"shaders/source"` or `"../shared/shaders"`. It is absolute only when no
  relative path exists (another Windows drive). A host resolves it against the
  project root, so generated files stay machine-independent;
- `shader_names`, in deterministic source-name order;
- one snake_case value per graphics or compute shader, in the same order, with
  complete typed reflection and `stages.vertex` and `stages.fragment`, or
  `stages.compute`, as `List(U8)`.

### Shader modules

Each shader gets `Generated/<Shader>.roc`. Besides its types (below), it
exposes `reflection : GraphicsReflection` or `ComputeReflection`, preserving
original reflected source, entry-point names, and compute workgroup dimensions,
and `stages`.

Each shader module also exposes one `<snake_type>_type : StructType` value per
reflected struct type: parameter-block and push-constant element types, nested
struct fields, pointer pointees, and structured resource results. The values
are sorted by name, and `reflection` refers to them by name. Entry-point
parameter structs stay inline in `reflection`. Two struct types with the same
name and different fields in one shader fail generation.

### `ShaderReflection`

Generated modules import `pf.ShaderReflection`, so the app header must bind the
platform under the shorthand `pf`. Each module lists the `ShaderReflection`
types it uses in `exposing [...]` and names them unqualified. It calls the
`ShaderReflection` functions (`pack`, `array_bytes`, `f32_bytes`, `i32_bytes`,
`u32_bytes`, `u64_bytes`) qualified. `roc-platform/platform/ShaderReflection.roc`
defines:

- the lossless typed reflection schema. It preserves ordered lists, all
  binding/field/resource/pointer/enum/layout variants, option distinctions,
  numeric values, and original strings. Rust `usize` metadata is checked and
  emitted as `U64`; enum values remain `I64`, with their signed/unsigned tag
  kind;
- the logical vector, matrix, address, and handle types with their
  little-endian `to_bytes`, and `ElementPacker(elem)`.

### Types and packers

Logical structs and enums live with their Slang declaration's owner. Types
declared in a shader are nested in that shader's generated module, such as
`BasicTriangle.Vertex`, with their packers. Types declared in shared `.slang`
modules are emitted once in corresponding generated modules, such as
`Mltrs.MvpMatrices` and `Particle.Particle`. As in Rust codegen, modules one
directory below the source root are grouped under the directory's name.
References and packers import shared dependencies by their owning module.
There is no aggregate `ShaderTypes.roc`; regeneration removes a stale one from
the managed output directory. Two shaders may each declare a local type with
the same name.

The CPU can construct parameter blocks, push blocks, nested structs and
pointees, and their enums. A fragment-input struct only carries data between
shader stages and is not emitted. Each logical type is nominal, nested in its
owning module, with a derived `is_eq`, so `==` works. A structural record
literal or a bare tag constructs a value. A handle or address value uses the
payload constructor, `PointerAddress.(addr)`. Every struct with a GPU layout
also has the associated items `gpu_size : U32`,
`to_bytes : <Type> -> List(U8)`, and `packer : ElementPacker(<Type>)` (its
`gpu_size` as the stride, and `to_bytes`), reached as `<Module>.<Type>.packer`
or called as `value.to_bytes()`. They pack a logical value into the bytes the
GPU reads:

- parameter blocks, push blocks, nested structs, and pointer pointees place
  each field at its reflected offset; a std140 block's size is its reflected
  end rounded up to 16, a push block's or pointee's size is the reflected one.
  A `Std430DataLayout` pointee follows the SPIR-V layout: `float3 a; float3 b;`
  places `b` at 16 in 32 bytes, `float3 a; float s;` tail-packs `s` at 12 in
  16 bytes, and `float3 a; float4 b;` places `b` at 16 in 32 bytes;
- enums pack through their associated `tag` function;
- one struct with two different layouts fails generation.

The packers use the `ShaderReflection` byte helpers: the qualified functions
(`ShaderReflection.pack`, `ShaderReflection.f32_bytes`,
`ShaderReflection.array_bytes`, ...) and the exposed types' own packers
(`Float4x4.to_bytes`, ...). A fixed array with the wrong length crashes.

Field types map to `ShaderReflection` types or to nested project types. Each
row names the exposed, unqualified form:

| Reflected value | Roc logical value |
| --- | --- |
| float/int/uint/u64 scalar | `F32` / `I32` / `U32` / `U64` |
| vector | `Float{n}` / `Int{n}` / `Uint{n}` / `Uint64x{n}` for widths 1 to 4, a record with `x`, `y`, `z`, `w` fields as applicable |
| supported matrix | `Float4x4` / `Int4x4` / `Uint4x4`, a row-major record with `row_0` through `row_3` of the vector alias |
| fixed vector array | `List(alias)`; required count/stride remain in reflection |
| physical pointer | nominal `PointerAddress` wrapping `U64`, built as `PointerAddress.(addr)` |
| descriptor handle | distinct nominal `DescriptorHandle` wrapping `U64`, built as `DescriptorHandle.(index)` |
| texture/storage resource | not supported; generation fails. Declare a `.Handle` field instead. |
| enum | nominal tag union with an associated `tag : <Enum> -> U32/I32` function |

The `ShaderReflection` type names a module may expose and the Roc builtin
modules and types (`List`, `Str`, `Result`, `U32`, ...) are reserved: a shader
struct, shader file, or shared Slang module named `Float3`, `Float4x4`,
`PointerAddress`, `ElementPacker`, `List`, or any other of them fails
generation.

Original reflected spelling is retained in reflection data. Generated nonempty
records use multiline fields with trailing commas. One blank line separates
sibling definitions inside every generated module. Generated values and fields
use snake_case; modules, types, and tags use PascalCase. Invalid or
reserved identifiers, normalized collisions, incompatible shared definitions,
bare texture or storage resource fields, and case-insensitive output filename
collisions fail before replacement.

## Guarantees and limits

Handles and addresses carry no ownership, dereference, or descriptor validity
guarantee. The `to_bytes` packers produce GPU bytes; the byte order of a
matrix follows the shader's declared layout (row-major, so `row_i` holds glam
column `i`).

The separate `roc-platform` host still renders its Rust-owned shader and uses
Rust bindings under `src/generated/`. Roc codegen does not change runtime
shader selection, hot reload, or platform/compiler pins.

## Verification

`just test` covers generation, schemas, names, paths, manifests, stale cleanup,
safety checks, std430 float3 pointee packers, and Rust regressions without
requiring Roc.

`just roc-codegen-test [ROC]` is the explicit real-compiler gate. It reports the
selected executable/version, generates fresh graphics and compute fixtures,
checks and formats every module against `roc-platform/platform/main.roc` as the
app's platform, and evaluates full imported SPIR-V contents, representative
reflection, module-owned types, and packer output from a consumer running in
another working directory. It also proves missing imports and false byte
expectations fail. `ROC` defaults to `roc` on
`PATH`; missing/incompatible compilers fail rather than skip. `just pre-commit`
runs this gate when `roc` is on `PATH`.
