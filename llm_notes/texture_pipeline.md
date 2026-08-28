# Texture asset pipeline: `ctt` + KTX2

**Status: implemented, 2026-08-04**, against `main` @ `eef377e`, with §10's open
questions resolved as `--quality very-slow` and zstd on *both* format groups. All
measurements taken on Dan's laptop the same day with `ctt` 0.5.0 and KTX-Software's
`ktx` CLI; every number below is reproducible with the commands shown.

The current-state document is [`../docs/textures.md`](../docs/textures.md); where the
two disagree, it wins. Four things this design got wrong or left open were settled
during implementation, annotated in place below: §5.5 (the helper cannot be a
`Renderer` method), §6.1 (the drafted root recipe does not work), §9.3 (wrong header
offset), and all three of §10.

Adopts [`ctt`](https://github.com/cwfitzgerald/ctt) as a build-time texture encoder,
driven by per-example `just` recipes, emitting KTX2 that the existing loader
(`crates/mltrs/src/ktx.rs:29`) already accepts. The format is chosen per asset:
**BC7 for model/photographic textures, lossless `rgba8unorm` + zstd for pixel art.**

Companion documents: [`offscreen_testing.md`](offscreen_testing.md) — `just sweep` is
the gate that catches validation fallout from new texture formats, and it runs on
lavapipe, which matters here (see §6). [`build_reproducibility.md`](build_reproducibility.md)
— this note adds a second class of committed, regenerated binary artifact alongside
`shaders/compiled/`, with the same determinism question. [`bindless_textures.md`](bindless_textures.md)
— its palettized-material direction overlaps with §7.

---

## 1. Why

Every textured example except `suzanne` ships a PNG/JPG/BMP that `util::load_image`
(`crates/mltrs/src/util.rs:18`) decodes at startup into a `DynamicImage`, hands to
`Renderer::create_texture` (`renderer.rs:495`), and uploads as `R8G8B8A8_SRGB` with a
mip chain generated at runtime by blits. Three costs, paid on every launch of every
example:

1. **Decode time.** PNG/JPEG inflate + unfilter on the main thread during `setup`.
2. **VRAM.** 4 bytes/texel, forever, for content that would survive 1 byte/texel.
3. **A mip-generation pass** of `vkCmdBlitImage` calls per texture.

`suzanne` is the one example already on the KTX2 path, but its three files are
*uncompressed* `R8G8B8A8_SRGB` (vkFormat 43, 1024², 11 levels) totalling **16.8 MB**
committed. It demonstrates the loader without any of the payoff, and it is the single
largest thing in the repository.

`load_ktx2` already does the hard part — it reads any KTX2 whose `vkFormat` is a
concrete Vulkan format, validates the level sizes block-generically
(`ktx.rs:82`), and hands the mip data to `create_texture_with_mips`
(`renderer.rs:544`). What is missing is a tool to *produce* those files and a place to
put the invocation.

## 2. Why `ctt` and not the alternatives

`ctt` is the only Rust option that spans the whole job — decode source, generate mips,
encode, and write the container:

- **`intel_tex_2`** (Traverse Research) encodes BC1–BC7 but returns raw block bytes.
  The container and mip chain would be ours to write, and the `ktx2` crate we already
  depend on is **parse-only** — no writer in 0.5.
- **`block_compression`** is a pure-Rust WGSL port of the same Intel ISPC kernels, run
  on the GPU via `wgpu`. Attractive in isolation, but it drags `wgpu` into a workspace
  that is deliberately `ash`-only, and still leaves the container to us.
- **`libktx-rs` / `ktx2-rw`** bind Khronos KTX-Software and can read *and* write, but
  are heavier to build and less maintained than the encoder-side options.
- **KTX-Software's `ktx create --encode`** is what produced today's `suzanne` files
  (`ktx2ktx2`, per the comment at `examples/suzanne/src/main.rs:89`). Perfectly usable,
  but it is a separate non-Rust toolchain to document and install.

`ctt` is a unified frontend over bc7enc-rdo, Intel ISPC, etcpak, AMD Compressonator and
astcenc, with both a library and a CLI (`cargo install ctt-cli`), and is by cwfitzgerald,
who maintains `wgpu`. It is tri-licensed MIT/Apache-2.0/Zlib. We use the **CLI**, not the
library: keeping it out of `Cargo.toml` keeps a C++ toolchain out of
`cargo check --workspace`, and matches how `suzanne`'s assets are already produced by an
external tool.

## 3. Measurements

These are what decided the format split, so they are recorded rather than summarised.

### 3.1 BC7 is conditionally lossless, and pixel art is the condition

No GPU block format is lossless. BC1–BC7, ETC/EAC and ASTC are all fixed-rate by
construction — a constant byte budget per block is exactly what buys constant-time
random texel access. There is no "lossless BC7-like format" to reach for.

But BC7 **mode 6** stores two RGBA endpoints at 7 bits + 1 p-bit — 8-bit exact — with
4-bit per-texel indices, where index 0 and index 15 land precisely on the endpoints.
So **any 4×4 block containing at most 2 distinct 8-bit RGBA colors is representable
bit-exactly.** Counting distinct colors per block on the real assets:

| asset | 1 color | 2 colors | 3+ | ≤2 (bit-exact-capable) |
|---|---|---|---|---|
| `serenity_crt` castlevania 800×978 | 88.7% | 11.3% | 0.0% | **100.0%** |
| `space_invaders` sprite_sheet 480×288 | 87.2% | 11.4% | 1.4% | **98.6%** |
| `sprite_batch` ravioli 32×32 | 25.0% | 25.0% | 50.0% | 50.0% |
| `viking_room` 1024² | 15.2% | 3.8% | 81.0% | 19.1% |

`serenity_crt` reaches 100% because it is upscaled pixel art: each source pixel spans
several output texels, so a 4×4 block almost never straddles more than one edge.
`viking_room` at 19.1% is the expected photographic profile — genuinely lossy under
BC7, which is precisely what BC7 is for.

**Honest caveat:** this counts blocks BC7 *can* encode exactly, not blocks the encoder
*did*. `bc7e` at `--quality very-slow` searches modes and should take a zero-error
encoding where one exists, but this was not verified, because nothing available will
decode BC7 back to RGBA for a diff — `ktx extract` fails with "Requested format
conversion from VK_FORMAT_BC7_SRGB_BLOCK is not supported", and `ctt` with "unsupported
pixel format conversion: passthrough". Treat the table as a strong upper bound on
achievable exactness, not a proof of it. **This is the reason the pixel-art assets do
not go to BC7:** the lossless path below is not a bet.

### 3.2 File size and VRAM

Bytes. VRAM is level 0 only; a full chain adds ~33%.

| example | source | `rgba8`+zstd | bc7 | bc7+zstd | VRAM rgba8 | VRAM bc7 |
|---|---|---|---|---|---|---|
| `space_invaders` | 5,448 png | **3,637** | 138,432 | 4,852 | 552,960 | 138,240 |
| `serenity_crt` | 9,684 png | **7,702** | 784,192 | — | 3,129,600 | 784,000 |
| `sprite_batch` | 4,234 bmp | **461** | 1,216 | — | 4,096 | 1,024 |
| `viking_room` | 962,052 png | 1,362,525 | 1,048,768 | **730,276** | 4,194,304 | 1,048,576 |

The load-bearing result: for all three pixel-art assets, lossless
`-f rgba8unorm --zstd=19` is **smaller on disk than the source PNG**, bit-exact, and
skips PNG decode at startup (zstd inflate + memcpy instead). It is strictly better than
the status quo on every axis; VRAM is unchanged.

Note also that raw BC7 files are *larger* than the source PNG — BC7 is fixed-rate,
PNG is entropy-coded — but BC7+zstd lands below it (730,276 vs 962,052 for
`viking_room`) while keeping the 4× VRAM win. Supercompression is worth having on both
halves of the split.

### 3.3 zstd, not zlib

`ctt` offers both. On `serenity_crt`, `--zlib=9` produces **24,669** bytes — worse than
the 9,684-byte PNG — against zstd's 7,702. zlib is not viable here, even though
`flate2` is already in the dependency tree (via `png` ← `image`) and would have been
free. This settles the runtime dep: we need a zstd *decoder* (§5.3).

### 3.4 `suzanne` is the biggest single win

The committed files are uncompressed RGBA8, so `ktx extract` round-trips them to PNG
**losslessly**:

```
ktx extract --level 0 models/suzanne/suzanne0.ktx2 suzanne0.png
```

| | bytes |
|---|---|
| `suzanne0.ktx2` as committed (uncompressed, 11 levels) | 5,592,912 |
| `suzanne0.png` extracted | 232,164 |
| re-encoded `-f bc7 --mipmap --quality slow` | 1,398,560 |
| re-encoded with `--zstd=19` | 269,739 |

All three files total 16,778,736 bytes today. Replacing them with ~0.7 MB of PNG
sources plus ~0.8 MB of BC7+zstd is a **~15 MB repository reduction** and a 4× VRAM cut,
with **zero Rust change** to the example — it is already on the mips path.

## 4. The split

| group | `ctt` flags | rationale |
|---|---|---|
| models / photographic | `-f bc7 --mipmap --quality slow` | 4× VRAM; loss invisible on this content (§3.1 shows only 19% of blocks were ever exactly representable, and it does not matter) |
| pixel art | `-f rgba8unorm --zstd=19` | bit-exact, smaller than the PNG, no decode at startup; sidesteps the §3.1 caveat entirely |

`ctt` reads the PNG's sRGB metadata and emits **vkFormat 43 (`R8G8B8A8_SRGB`)** for
`-f rgba8unorm` — a format `format_block_info` (`renderer.rs:4049`) already handles. So
the lossless group needs no new format support at all, only zstd.

## 5. Engine prerequisites

All in `crates/renderer/src/renderer.rs` unless noted.

### 5.1 `format_block_info` — BC7 arms

`renderer.rs:4049` currently knows only `R8G8B8A8_{SRGB,UNORM}`. Add:

```rust
vk::Format::BC7_SRGB_BLOCK | vk::Format::BC7_UNORM_BLOCK => Some(FormatBlockInfo {
    block_bytes: 16,
    block_width: 4,
    block_height: 4,
}),
```

Nothing else changes for partial blocks: the `div_ceil` sizing at `ktx.rs:82` and the
buffer-offset alignment at `renderer.rs:4311` are already written block-generically.
That matters — `koch_curve` is 612×331 and `serenity_crt` 800×978, neither divisible
by 4.

### 5.2 `textureCompressionBC`

Not currently enabled. Two edits:

- `renderer.rs:3256` — add `.texture_compression_bc(true)` to the enabled
  `vk::PhysicalDeviceFeatures`.
- `renderer.rs:3011` — add `(features.texture_compression_bc, "textureCompressionBC")`
  to the `missing_features` list, so an unsupporting device is rejected at *selection*
  with a named reason, rather than surviving to the format check inside
  `create_texture_from_mips` (`renderer.rs:4222`) and failing per-texture.

Confirmed available where it must be: `vulkaninfo` under the sweep's pinned lavapipe
ICD reports `textureCompressionBC = true` (against `ETC2 = false`,
`ASTC_LDR = false`). BC is also the desktop-universal family, which is why this plan
does not consider ETC or ASTC.

### 5.3 zstd supercompression in `load_ktx2`

`ktx.rs:40` bails on any supercompression, and the `NOTE` at `ktx.rs:38` already
specifies the fix: decompress each level to its `uncompressed_byte_length`. Replace the
`bail!` with a match that accepts `SupercompressionScheme::Zstandard` and passes
everything else to the existing error, then inflate in the level loop before the size
assertion — so the assertion keeps checking the *decompressed* size, which is the
useful invariant.

Dependency: prefer the `zstd` crate (widely used; the workspace already builds C/C++
via `vk_mem` and SDL), with `ruzstd` as the pure-Rust decode-only alternative if a C
dep is unwanted. Add to the workspace `Cargo.toml` and `crates/mltrs`.

**This is required, not optional.** Without it the pixel-art half produces 553 KB and
3.1 MB files instead of beating the PNGs, and §3.2's entire argument collapses.

### 5.4 Sampler/level mismatch in `create_texture_from_mips`

`create_texture_from_mips` (`renderer.rs:4222`) builds its sampler from
`TextureOptions { filter, ..Default::default() }`, and `Default` sets `mipmaps: true` —
regardless of `mip_data.len()`. The comment at `renderer.rs:4679` states the invariant
that path breaks:

> the same flag that sized the image caps the LOD, so image and sampler can't disagree
> (a mismatch samples a level that isn't there)

On this path they *can* disagree. It is latent today because every KTX2 in the repo has
11 levels, but the pixel-art assets are deliberately single-level (§6), which makes it
live: `anisotropy_enable(true)` and `max_lod = LOD_CLAMP_NONE` over a one-level image.
Fix by deriving `mipmaps` from `mip_data.len() > 1`.

### 5.5 A load helper

`suzanne` spends eight lines per texture on `load_ktx2` + `create_texture_with_mips`
(`examples/suzanne/src/main.rs:94-101`), and three more examples are about to repeat it.
Add one entry point — `Renderer::create_texture_from_ktx2(&KtxTexture, TextureFilter)`,
or a free fn in `ktx.rs` — and convert `suzanne` to it in the same change so the new
call sites and the old one stay identical.

[Settled: it **has** to be the free fn. `KtxTexture` lives in `mltrs` and `Renderer`
in `mltrs-renderer`, and the dependency runs mltrs → renderer, so a `Renderer` method
cannot name the type. Shipped as `ktx::load_ktx2_texture(renderer, file_path, filter)`,
which takes the path rather than a `KtxTexture` and so subsumes the `load_ktx2` call
too — one line per call site instead of eight.]

Explicitly **not** needed for this scope: widening `create_texture_with_mips`
(`renderer.rs:544`) to take `TextureOptions`. See §7.

## 6. Recipes and per-example conversion

### 6.1 Wiring

A `textures` recipe in each converted example's justfile, plus a root aggregate
mirroring `just shaders`. Six examples need a justfile created (`viking_room`,
`depth_texture`, `koch_curve`, `suzanne`, `serenity_crt`, `sprite_batch`) and a `mod`
line in the root justfile beside the existing four. Each keeps the standard header
comment about just setting the working directory to the submodule dir — all paths are
crate-relative.

The root recipe *discovers* rather than hard-codes, so no example-specific knowledge
lands in the root justfile (per `CLAUDE.md`):

```just
# re-encode every example's source images to KTX2 (needs `cargo install ctt-cli`)
[unix]
textures example="all":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{example}}" = "all" ]; then
        for f in examples/*/justfile; do
            e=$(basename "$(dirname "$f")")
            if just "$e" --summary 2>/dev/null | grep -qw textures; then just "$e" textures; fi
        done
    else
        just "{{example}}" textures
    fi
```

[**Wrong — this does not work.** just has no per-submodule `--summary`;
`just space_invaders --summary` fails with `Justfile does not contain recipe
'space_invaders --summary'`, so the predicate is always false and the `all` arm
silently encodes nothing. Worse, it fails *quietly* in exactly the way that looks
like a clean run.

The discovery idea was dropped rather than repaired: the shipped recipe iterates an
explicit `texture_examples` list in the root justfile. Dynamic discovery hid the set
of converted examples behind a shell predicate and traded a visible list for a
failure mode that reads as success — the same shape as the bug above. The list is a
line to keep in sync, which `docs/textures.md` names as a step; that is cheaper than
a mechanism nobody can see the output of. The root justfile already carries one
`mod` line per example, so this is not new example-specific knowledge there either.]

Chain into existing pipelines with just's deferred-dependency `&&`, so the encode runs
*after* whatever produces its input:

```just
# examples/space_invaders/justfile
[unix]
sprites: && textures
    cd textures/space_invaders && aseprite --batch *.aseprite \
        --sheet sprite_sheet.png --data sprite_sheet.json \
        --filename-format "{title} {frame}" --format json-array

# encode the sprite sheet losslessly (bit-exact, and smaller than the png)
[unix]
textures:
    ctt textures/space_invaders/sprite_sheet.png \
        -o textures/space_invaders/sprite_sheet.ktx2 \
        -f rgba8unorm --zstd=19
```

### 6.2 What gets committed

Source images stay committed as the regenerable input, and the `.ktx2` is committed
next to them — the same arrangement as `shaders/compiled/`. `just sweep` therefore keeps
working in a bare container with **no new `assets_missing` case** in
`scripts/headless-sweep.sh:104`.

### 6.3 The examples

**BC7 group** — `-f bc7 --mipmap --quality slow`:

| example | source | call site to update |
|---|---|---|
| `suzanne` | 3× `models/suzanne/suzanne{0,1,2}.ktx2` → one-time `ktx extract` to PNG, commit the PNGs, delete the uncompressed ktx2 | **none** — already on the mips path |
| `viking_room` | `textures/viking_room.png` 1024² | `main.rs:96-98` |
| `depth_texture` | `textures/texture.jpg` 512² | `main.rs:97-99` |
| `koch_curve` | `textures/istockphoto-uffizi-blurred-612x612.jpg` 612×331 | `main.rs:59-60` |

`koch_curve`'s un-blurred `istockphoto-uffizi-612x612.jpg` is referenced by no code and
stays as-is.

**Lossless group** — `-f rgba8unorm --zstd=19`, no `--mipmap`:

| example | source | call site to update |
|---|---|---|
| `space_invaders` | `sprite_sheet.png` 480×288 | `load_texture`, `main.rs:627-631` |
| `serenity_crt` | `castlevania_pixel_art.png` 800×978 | `main.rs:63-65` |
| `sprite_batch` | `ravioli_atlas.bmp` 32×32 | `main.rs:82-83` |

`sprite_batch` joins for pipeline uniformity, not savings — it is a 4 KB asset and
only 50% of its blocks were exactly representable anyway (§3.1), which is another small
argument for the lossless path over BC7 there.

**Deliberate behavior change.** All three currently receive a runtime-generated mip
chain, because `create_texture` passes `TextureOptions::default()` with `mipmaps: true`.
Omitting `--mipmap` gives them a single level. This is correct for `Nearest`-filtered 2D
content that never minifies, and it is what makes §5.4 load-bearing — but confirm by eye
per example rather than assuming.

### 6.4 Excluded, and why

- **`watercolor`** — `load_paper_height_map` (`main.rs:1175`) reads `paper_height.png`
  with `image::open` and converts it to a `Vec<f32>` luma buffer on the CPU. It never
  becomes a GPU texture, so no container or block format applies to it. Its existing
  `paper-texture` recipe is untouched and it gets no `textures` recipe.
- **`multi_mesh`** — `create_textures` (`main.rs:426`) generates its images in memory
  (`solid_image`, `checker_image`). There are no files to convert. Its comment about a
  full mip chain destroying the wrap/filter test is worth preserving as-is.

## 7. Deferred: `toon_link`

The natural follow-up, deliberately out of scope, because it needs more than a recipe:

- **API change.** `create_texture_with_mips` (`renderer.rs:544`) takes only a
  `TextureFilter`, so the per-entry `wrap_u`/`wrap_v` that `texture_options`
  (`examples/toon_link/src/main.rs:600`) derives from the GX manifest cannot survive the
  switch. Widening it to `TextureOptions` — with `mipmaps` taken from the level count per
  §5.4 rather than from the caller — is the prerequisite.
  **Prerequisite met**: `create_texture_with_mips` takes `SamplerOptions`
  (`filter`, `wrap_u`, `wrap_v`) in `crates/renderer/src/renderer.rs`. The
  conversion itself (format, pipeline, `textures` recipe) is still not done.
- **Format.** `BC7_UNORM_BLOCK`, not sRGB. `texture_options` hardcodes
  `TextureColorSpace::Unorm` because GX has no sRGB anywhere and the fragment shader
  applies its own decode.
- **Pipeline.** The 44 PNGs are gitignored and produced by `convert_link`, so the recipe
  chains off `just toon_link convert-link`, and either the `gx` manifest's `entry.file`
  or the loader has to resolve `.ktx2`. The sweep's `assets_missing` case
  (`scripts/headless-sweep.sh:110`) already covers the machine-local half.
- **Worth reconsidering the format entirely.** These are GameCube CI4/CI8 *palettized*
  textures upstream. Re-palettizing them (§8) would be lossless, 4–8× smaller than RGBA8,
  and closer to the original data than any block format — and it composes with the
  material work in [`bindless_textures.md`](bindless_textures.md).

## 8. The option not taken: palettized textures

Vulkan has no paletted texture format, but the effect is reachable: an `R8_UINT` index
texture plus a palette in a uniform or storage buffer, resolved in the fragment shader,
sampled `Nearest`. It is the only approach that is **both** bit-exact and 4× smaller in
VRAM — strictly better than both halves of §4 on those two axes — and it is the
historically correct representation for every pixel-art asset here.

Not taken now because it costs a shader change per consumer, rules out linear filtering,
and cannot be expressed as a drop-in swap at the `create_texture` call site. Recorded
because §7 may make it worth revisiting for `toon_link` specifically.

## 9. Verification

1. `cargo check --workspace --all-targets`, then `just lint`, then `cargo fmt`.
2. `just textures` regenerates every `.ktx2`; `git status` must then be clean. This
   doubles as the test of whether `ctt` output is byte-deterministic — **if it is not,
   record that here and stop committing the artifacts blind**, because the
   `just pre-commit` `git add` of generated files assumes it.
3. Inspect one output per format group with `xxd -l 48 <file>`: vkFormat at offset
   0x0C (146 = `BC7_SRGB_BLOCK`, 43 = `R8G8B8A8_SRGB`), then width/height, `levelCount`,
   and `supercompressionScheme` at 0x20 (0 = none, 2 = zstd).
   [Correction: `supercompressionScheme` is at **0x2C**, not 0x20 — 0x20 is
   `layerCount`, 0x24 `faceCount`, 0x28 `levelCount`. Table in `docs/textures.md`.]
4. `just watch <example>` for each converted example, compared against `main` by eye.
   The pixel-art three matter most: they are the assets whose losslessness is the whole
   argument, and the mip-count change of §6.3 lands on them.
5. `just sweep` — the gate for validation fallout from the new formats, the sampler
   change in §5.4, and the single-level images. It exercises BC7 rather than skipping it,
   per §5.2.
6. `just test` is unaffected (no `build_tasks.rs` or template changes) but runs under
   `just pre-commit` regardless.

## 10. ~~Open questions~~ — all three settled

- **Determinism of `ctt` output** — ~~assumed, not measured~~. **Measured: deterministic.**
  Two full `just textures` runs over all nine artifacts (both format groups, including
  multithreaded BC7 at `--quality very-slow`) produced identical sha256s. Committing
  them is safe, and `just textures` + a dirty `git status` is now a meaningful check
  rather than a hopeful one.
- **`--quality` for the BC7 group** — **`very-slow`.** Still no BC7 decoder to measure
  the quality delta with, so this is decided on cost rather than benefit: `very-slow`
  takes ~0.5 s wall for a 1024² image with a full 11-level chain (~6 s CPU across 10
  cores). At that price there is no reason to encode lower.
- **Should the BC7 group also get `--zstd`?** — **Yes**, `--zstd=19` on both groups.

Two things worth recording that this note did not anticipate:

- **BC7 *grows* the repo for the JPEG-sourced examples.** §3.2 only measured
  `viking_room` (962 KB PNG → 1,008 KB BC7+zstd, roughly a wash). `koch_curve` pays
  207 KB of new artifact against a 23 KB source JPEG, and `depth_texture` 256 KB
  against 77 KB — BC7 is fixed-rate and JPEG is entropy-coded, so this is structural,
  not a bad encode. The trade is real and taken deliberately: it buys the 4× VRAM cut.
  `suzanne` swamps it — the whole change is **≈ −13.8 MB** committed.
- **`suzanne`'s PNG extraction is bit-exact**, not merely believed to be. Verified by
  round-tripping `ktx extract --level 0` → PNG → re-encode to uncompressed `rgba8unorm`
  and comparing the level-0 payload against the committed file byte for byte. So
  deleting the three uncompressed files cost nothing but the pre-baked mips, which the
  recipe regenerates.
