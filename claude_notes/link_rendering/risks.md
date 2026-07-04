# Toon Link plan: risk walkthrough

Expanded explanations of the risks listed in
[`../link_rendering.md`](../link_rendering.md) §7 — for each: the underlying
mechanism, why it's uncertain, how it would fail visibly, and why the planned
mitigation works.

## 1. SHP1 matrix groups (the exploded-vertex risk)

The GameCube's GPU had space for only **10 position/normal matrices** loaded at
a time, but Link has 42 joints. J3D solves this by splitting each shape into
*packets*: before a packet's triangles are drawn, its **matrix table** says
which joint (or blended envelope) matrices to load into which of the 10
hardware slots. Then every vertex carries a `PNMTXIDX` attribute — an index
into those slots (stored pre-multiplied by 3, so `value / 3` = slot) — saying
"transform me by slot N."

Two traps make this the most dangerous part of the converter:

- **`0xFFFF` entries** in a packet's matrix table mean "keep whatever the
  *previous* packet left in this slot." So packets are not independent — you
  must process them in order, carrying slot state forward. Miss this and some
  vertices silently bind to a stale or garbage matrix.
- The indirection is three layers deep: vertex → slot → matrix-table entry →
  DRW1 entry → either a joint index or an EVP1 envelope index. An off-by-one at
  any layer still *runs* — it just transforms chunks of the mesh by the wrong
  joint.

The failure mode is unmistakable (limbs stretched across the scene, "exploded"
mesh) but hard to debug backward from. That's why the plan bakes in the
**weighted-identity check**: at bind pose, every EVP1-weighted vertex must come
out ≈ where it started (`Σw·(world·invBind) = I`). If that check passes but
rigid parts (hair, scabbard, belt) are detached, the bug is isolated to the
JNT1 world-matrix walk instead. It converts "something is wrong somewhere in
four layers of indirection" into a bisecting test.

## 2. GX fixed-point vertex formats

VTX1 doesn't store a fixed vertex layout. Each attribute (position, normal,
UVs, colors) has a format descriptor: component count, component *type*
(u8/s8/u16/s16/f32), and — for the integer types — a **fraction shift**: the
stored integer is fixed-point, and the real value is `stored / 2^shift`. A
position array might be s16 with shift 8, meaning divide everything by 256.

The risk is assuming (as most quick-and-dirty BMD parsers do) that positions
and UVs are f32. If Link's model uses s16 positions and you read them as f32,
you get garbage; if you read s16 but ignore the shift, the model renders at
256× scale or UVs tile wildly. It's not conceptually hard — just an easy place
to cut a corner that costs a confusing debugging session later. Mitigation is
simply to implement the format table generically from day one and have
`--info` print each attribute's declared format, so when something looks wrong
you can see at a glance what the data claims to be.

## 3. Winding after the Y-flip

Three conventions collide here. GX/OpenGL clip space has +Y up; Vulkan has +Y
*down*. This renderer's shaders handle that with a Y reflection in the
projection (the `mvp.slang` convention). But reflecting Y **mirrors the
geometry**, which reverses triangle winding: triangles that were
counter-clockwise on screen become clockwise. Meanwhile the Vulkan pipeline
culls back faces with CCW = front.

So even with perfectly parsed geometry, there's a 50/50 chance Link renders
*inside-out* — you see the interior surfaces of his tunic and the back of his
face. On top of that, GX's cull-mode enum (`GX_CULL_BACK` etc.) is defined
relative to GX's own winding convention, so the converter has to map it
through the flip correctly.

Rather than trying to reason this out a priori across three conventions, the
plan sidesteps it: **P6 renders with culling off** (which is why
`CullMode::None` is in the `RasterState` extension), so geometry correctness is
verified independently of winding. Then culling is switched on, and if the
model is inside-out, the converter flips index order per triangle — a
one-line, one-time fix.

## 4. Uniform array codegen (the one repo-local risk)

The TEV interpreter shader needs per-material config as uniform data:
`uint4 stageColor[8]`, `float4 konst[4]`, etc. The shader-atlas codegen
(`src/shaders/build_tasks.rs`) reflects Slang parameter blocks into
`#[repr(C)]` Rust structs, and it's proven for scalars, vectors, matrices, and
nested structs — but **no existing shader in the repo has an array field
inside a uniform block**, so that codegen path is unexercised. Two ways it
could fail: the reflection/codegen simply doesn't handle the array type (loud
failure at `just shaders` time — fine), or it generates a struct whose layout
doesn't match std140's array stride rules (silent failure: shader reads
garbage at some offset, materials look randomly wrong — nasty).

Two deliberate mitigations:

- The layout uses **only `uint4`/`float4` element types**, because for 16-byte
  elements std140's array stride equals the element size — there's no padding
  for the codegen to get wrong. That's why the sketch packs four selectors into
  each `uint4` instead of using `uint stageOp[32]` or an array of structs.
- **Test it first**: early in P6, a throwaway shader with one `uint4[8]` array,
  before any TEV logic exists. If it fails, the fallback is moving the config
  into a `StructuredBuffer` — a path sprite_batch already proves end-to-end, at
  the cost of slightly clunkier example code.

## 5. SRTG texgen (the heart of the toon look)

This is *the* cel-shading mechanism, so uncertainty here is uncertainty about
the signature visual. The chain: a GX **lighting channel** computes a
per-vertex lit color the ordinary way (N·L against the light, times material
color, plus ambient) — smooth, boring Lambert. Then a texgen of type **SRTG**
takes that rasterized color and uses its red/green components *as texture
coordinates*. That coordinate indexes the toon ramp texture, and the ramp — a
step function authored by Nintendo's artists — quantizes the smooth gradient
into the two-band cel look. The banding lives entirely in `toon.bti`'s pixels.

The uncertainties are all in the details: which channel feeds it (`COLOR0`? and
does alpha ride along?), exactly how R/G map to S/T, whether the coordinate
then passes through a texture matrix (Link might use identity, but if not,
ignoring it shifts where the shadow terminator falls), and which of `toon` vs
`toonEX` each material samples. Get any of these subtly wrong and Link still
*renders* — but the light/shadow boundary sits at the wrong angle, or the
banding is missing (smooth shading), or shadows are inverted.

Mitigation is that this doesn't need to be reasoned out from scratch: the **P2
MAT3 dump makes Link's actual texgen configs ground truth** before any shader
work starts, and noclip.website's `gx_material.ts` is a working,
pixel-verified SRTG implementation to check semantics against. The P8
verification step ("rotate the light, watch the terminator bands move") is
designed to exercise exactly this path.

## 6. S10 register semantics

TEV's intermediate registers aren't normalized floats — they're **signed
10-bit fixed point**, holding roughly [−4, +4) in units of 1/255. Materials
legitimately exploit this: a stage can subtract and go negative, or scale up
past 1.0, with a *later* stage bringing the value back into range; each stage
clamps its output **only if its clamp bit is set**. A float reimplementation
actually handles most of this fine — floats hold negative and >1 values
happily. The differences are at the edges: hardware saturates at ±4 where
floats keep going, and hardware quantizes to 1/255 steps where floats are
continuous.

The risk is low for Link (his materials are unlikely to push values near ±4),
which is why the mitigation is deliberately lazy: **always honor the per-stage
clamp bit** (that's semantics, not precision — skipping a set clamp bit gives
wrong colors, and clamping when it isn't set breaks the multi-stage patterns
above), but only add explicit ±4 saturation if a visual artifact actually
shows up. Don't pre-emulate hardware quirks nothing exercises.

## 7. Fog

Every GX material carries a fog block, and in-game, Link's materials get fog
parameters fed per-frame from the environment system
(`dKy_tevstr_c.mFogColor/mFogStartZ/mFogEndZ`). Our static scene has no
environment system, and a lone character floating in front of a clear color
doesn't need fog. The only real risk is if some Link material *depends* on the
fog stage for part of its look (very unlikely — fog in Wind Waker is
atmosphere/distance haze). So the converter treats fog-enabled as a **warning,
not an error**: note it in the dump, force it off, move on. Listed as a risk
mostly so a "why does the dump warn about fog?" moment isn't a surprise.

## 8. Lighting values

The actual daytime colors — what goes in C0 (light color) and K0/K1 (ambient)
— don't live anywhere convenient. In-game they're produced by the *kankyo*
(environment) system: per-room palettes indexed by time-of-day and weather,
crossfaded (`mColpatBlend`), then routed through
`settingTevStruct`/`setLightTevColorType`. Extracting the "true" noon-on-Outset
values means excavating those tables and replicating the blend logic — a
rabbit hole disproportionate to a static scene.

So v1 starts from **hand-tuned seeds** (the plan seeds
`lightColor ≈ (1.0, 0.98, 0.92)`, ambient ≈ `(0.45, 0.5, 0.55)`) against
noclip/Dolphin side-by-sides. The consequence of being off is mild and global
— Link reads slightly warmer or cooler than the game, uniformly — not
structurally wrong. Worth flagging because during P8 verification you need to
attribute differences correctly: "the *banding* is right but the *tint* is
off" is a lighting-constants problem to be tuned, not a TEV bug to be chased.

But there's a cheap upgrade from tuning to **ground truth**: rather than
excavating the kankyo tables statically, read the *computed* values out of
the running game. **dolphin-memory-engine** (pip-installable) reads emulated
RAM from outside Dolphin, and the tww decomp gives exact symbol addresses —
so a small script attached to Dolphin on noon-Outset reads Link's live
`dKy_tevstr_c` light/ambient colors directly (and can force the time-of-day
variable to exactly noon first). An hour of work, and the constants become
extracted facts instead of eyeballed approximations. Alternatively, the FIFO
analyzer shows the same values as the C0/K0/K1 register writes in a recorded
frame. See [`tests.md`](tests.md) §"Dolphin as an automated oracle".

---

Rough severity ranking: **#1 and #5 are the substantive ones** (structural
correctness of geometry and of the toon look), **#4 is the one most likely to
bite early** but has a proven escape hatch, **#2 and #3 are near-certain to
occur but trivial once anticipated**, and **#6–8 are
noted-so-they-don't-surprise-us tier**.
