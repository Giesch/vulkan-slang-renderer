import CreativeCoding

## Generated; directories are tool-owned.
ShaderAtlas := {}.{

	## The shader sources, relative to the Roc project root.
	shaders_source_dir : Str
	shaders_source_dir = "shaders/source"

	shader_names : List(Str)
	shader_names = ["creative_coding"]

	creative_coding = {
		reflection: CreativeCoding.reflection,
		stages: CreativeCoding.stages,
	}
}
