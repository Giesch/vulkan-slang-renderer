# Generated logical values, not GPU layouts.
# Fixed arrays are lists: required lengths are preserved in reflection.
import pf.ShaderReflection

ShaderTypes := {}.{
	FragInput : {
		position : ShaderReflection.Float4,
		color : ShaderReflection.Float3,
	}

	MvpMatrices : {
		model : ShaderReflection.Float4x4,
		view : ShaderReflection.Float4x4,
		proj : ShaderReflection.Float4x4,
	}

	Vertex : {
		position : ShaderReflection.Float3,
		color : ShaderReflection.Float3,
	}
}
