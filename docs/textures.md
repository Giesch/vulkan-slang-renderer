# Textures

Current as of `main`. Every example texture is a **KTX2 file with pre-baked mip
levels**, encoded at build time by [`ctt`](https://github.com/cwfitzgerald/ctt)
and committed next to the source image it came from.

```bash
cargo install ctt-cli    # one-time; only needed to *change* an asset
just textures            # re-encode every example's images
just EXAMPLE textures    # re-encode one example's
```

Nothing in `cargo build` or `cargo run` needs `ctt` — the `.ktx2` is committed, the
same arrangement as `shaders/compiled/`.

## The two format groups

The format is chosen per asset, by what the content is:

| group | flags | why |
|---|---|---|
| models / photographic | `-f bc7 --mipmap --quality very-slow --zstd=19` | 4× less VRAM; the loss is invisible on this content |
| pixel art | `-f rgba8unorm --zstd=19` (no `--mipmap`) | bit-exact, and *smaller on disk than the source PNG* |

**No GPU block format is lossless.** BC1–BC7, ETC/EAC and ASTC are fixed-rate by
construction — a constant byte budget per block is exactly what buys constant-time
random texel access. BC7's mode 6 happens to represent any 4×4 block of ≤2 distinct
8-bit RGBA colors exactly, which covers ~100% of upscaled pixel art, but nothing
available will decode BC7 back to RGBA to *prove* the encoder took that path. So
pixel art takes the lossless route instead, where exactness is not a bet.

That route is strictly better than decoding a PNG at startup on every axis: smaller
file, no PNG decode (zstd inflate + memcpy instead), identical VRAM.

BC7 is the desktop-universal family, which is why ETC and ASTC are not considered.
It requires the `textureCompressionBC` device feature, which the renderer enables in
`create_logical_device` and *requires* in `choose_physical_device`, so a device that
lacks it is rejected by name rather than failing per-texture.

`choose_physical_device` goes one level further and proves that **every** format in
`TEXTURE_FORMATS` — both RGBA8 spellings and both BC7 ones — supports
`SAMPLED_IMAGE | TRANSFER_DST | SAMPLED_IMAGE_FILTER_LINEAR` before it accepts a
device. Nothing conformant can fail that (the Vulkan mandatory-format table covers
RGBA8, and `textureCompressionBC` covers BC7), which is the point: a device that
somehow can't is named at startup, and no texture load has to ask.

**Pixel art gets a single level on purpose.** `Nearest`-filtered 2D content never
minifies, so a mip chain is wasted.

**The renderer never generates mip levels.** Every level it uploads came from the
file, which is why there is no mip flag anywhere in the options.

**There is one upload path**, `create_texture_from_levels` in `renderer.rs`, and both
public entry points are calls into it:

| entry point | levels | format from | sampling |
|---|---|---|---|
| `create_texture_with_mips` | the file's chain | the file's `vkFormat` | `SamplerOptions` |
| `create_texture{,_with_options}` | one | `TextureOptions::color_space` | `TextureOptions::sampler` |

So `create_texture_with_options` is the one-level RGBA8 case of a mip upload — the
`color_space` is just how a caller holding raw bytes names the format a KTX2 file
would have carried. Everything downstream is shared: the staging buffer and its
block-aligned per-level offsets, the layout transitions, the image view, and the
sampler. The only format gate left there is membership of `TEXTURE_FORMATS`, since
device support was settled at startup. That sampler reads the **level count**, so a
one-level image gets `max_lod = 0` and anisotropy off instead of a LOD clamp pointing
at a level that isn't there.

Each level's byte length is checked against what its extent implies in the format's
block layout (`level_byte_len`), so a short slice fails before the copy rather than
reading past the level it was given. The KTX2 loader sizes its zstd output with the
same helper.

## Adding or changing a texture

1. Put the source image in the example's `textures/` (or `models/`).
2. Add a `textures` recipe to the example's justfile, and a `mod` line in the root
   justfile if it doesn't have one yet. Copy the closest existing example — the BC7
   ones are `viking_room`/`depth_texture`/`koch_curve`/`suzanne`, the lossless ones
   `serenity_crt`/`sprite_batch`/`space_invaders`.
3. **Add the example to `texture_examples` in the root justfile**, or `just textures`
   with no argument will skip it.
4. `just EXAMPLE textures`.
5. Load it with `mltrs::ktx::load_ktx2_texture(renderer, &file_path, filter)`.
6. Commit **both** the source and the `.ktx2`.

Where an encode feeds off another generated file, chain it with just's deferred
dependency so ordering is not left to the caller —
`examples/space_invaders/justfile` does `sprites: && textures`.

## Not on this path

- **`watercolor`** — `paper_height.png` is read on the CPU into a `Vec<f32>` luma
  buffer and never becomes a GPU texture, so no container or block format applies.
- **`multi_mesh`** — generates its 8×8 images in memory; there are no files to
  convert. Single-level suits it: a mip chain would average the checkerboard to flat
  gray at distance and destroy the wrap/filter test its panels exist for.
- **`toon_link`** — needs more than a recipe: its 44 PNGs are gitignored and produced
  by `convert_link`, and it wants `BC7_UNORM_BLOCK` rather than sRGB. Its per-entry
  `wrap_u`/`wrap_v` used to be the third blocker; `create_texture_with_mips` now takes
  `SamplerOptions`, so pairing `ktx::load_ktx2` with it carries them. See
  `llm_notes/texture_pipeline.md` §7.

## Determinism

`ctt` output is byte-deterministic — verified across both format groups, including
multithreaded BC7 at `--quality very-slow`. That is what makes committing the
artifacts safe. **`just textures` followed by a dirty `git status` is a real
regression**; investigate rather than committing the churn.

`--quality very-slow` costs about half a second for a 1024² image with a full mip
chain. For artifacts regenerated rarely and committed, that is free, so there is no
reason to encode at anything lower.

## Inspecting a file

```bash
xxd -l 48 examples/viking_room/textures/viking_room.ktx2
```

| offset | field | |
|---|---|---|
| `0x0C` | `vkFormat` | `146` = `BC7_SRGB_BLOCK`, `43` = `R8G8B8A8_SRGB` |
| `0x14` / `0x18` | width / height | |
| `0x28` | `levelCount` | `1` for the lossless group |
| `0x2C` | `supercompressionScheme` | `0` = none, `2` = zstd |

The loader (`crates/mltrs/src/ktx.rs`) accepts any KTX2 whose `vkFormat` is a concrete
Vulkan format known to `format_block_info`, 2D, non-array, non-cubemap, with either no
supercompression or zstd. It validates each level's **decompressed** size against the
size implied by the dimensions and the block layout, so a truncated or mislabelled
file fails at load rather than sampling garbage.
