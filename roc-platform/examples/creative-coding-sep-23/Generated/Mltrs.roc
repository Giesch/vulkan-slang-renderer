import pf.ShaderReflection exposing [Float4x4]

## Generated logical values, not GPU layouts.
## Fixed arrays are lists: required lengths are preserved in reflection.
Mltrs := {}.{
	MvpMatrices := {
		model : Float4x4,
		view : Float4x4,
		proj : Float4x4,
	}.{
		is_eq : _

		gpu_size : U32
		gpu_size = 192

		to_bytes : MvpMatrices -> List(U8)
		to_bytes = |value|
			ShaderReflection.pack(
				gpu_size.to_u64(),
				[
					(0, Float4x4.to_bytes(value.model)),
					(64, Float4x4.to_bytes(value.view)),
					(128, Float4x4.to_bytes(value.proj)),
				],
			)
	}
}
