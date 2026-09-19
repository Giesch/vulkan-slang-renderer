# Generated; directories are tool-owned.
import BasicTriangle

ShaderAtlas := {}.{
	shader_names : List(Str)
	shader_names = ["basic_triangle"]

	basic_triangle = {
		reflection: BasicTriangle.reflection,
		stages: BasicTriangle.stages,
	}
}
