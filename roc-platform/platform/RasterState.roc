## The fixed-function raster state a graphics pipeline is baked with.
##
## Mirrors the renderer's `RasterState`. `default` reproduces the renderer's
## original hardcoded pipeline, so leaving it alone is always a no-op.
RasterState := {
	blend : BlendMode,
	cull : CullMode,
	depth_test : DepthCompare,
	depth_write : Bool,
	color_write : ColorWrite,
	stencil : StencilMode,
}.{
	is_eq : _

	## How fragments combine with the color attachment: `Alpha` is
	## source-alpha over, `DstAlpha` weights by destination alpha (GX's
	## `GX_BL_DSTALPHA`), and `Opaque` disables blending.
	BlendMode := [Alpha, DstAlpha, Opaque].{
		is_eq : _
	}

	## Which triangle facing is discarded; the front face is counter-clockwise.
	CullMode := [Back, Front, None].{
		is_eq : _
	}

	## The depth test's comparison. Vulkan honors depth writes even when the
	## test is `Disabled`, so pair it with `depth_write: False`.
	DepthCompare := [Less, LessEqual, Always, Disabled].{
		is_eq : _
	}

	## The per-channel color write mask.
	ColorWrite := { r : Bool, g : Bool, b : Bool, a : Bool }.{
		is_eq : _
	}

	## The stencil test and write. `Write` stores the reference wherever the
	## depth test passes; `TestEqual` draws only where the stencil equals it.
	## Both need a depth buffer with a stencil component.
	StencilMode := [Disabled, Write(U8), TestEqual(U8)].{
		is_eq : _
	}

	default : RasterState
	default = {
		blend: Alpha,
		cull: Back,
		depth_test: Less,
		depth_write: Bool.True,
		color_write: { r: Bool.True, g: Bool.True, b: Bool.True, a: Bool.True },
		stencil: Disabled,
	}

	## No depth test and no depth writes.
	no_depth : RasterState
	no_depth = { ..default, depth_test: Disabled, depth_write: Bool.False }
}

expect RasterState.default.depth_write and RasterState.default.depth_test == Less

expect !RasterState.no_depth.depth_write and RasterState.no_depth.depth_test == Disabled

expect RasterState.no_depth.blend == RasterState.default.blend
