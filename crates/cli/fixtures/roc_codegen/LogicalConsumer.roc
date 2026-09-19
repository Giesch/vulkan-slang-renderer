import pf.ShaderReflection
import ShaderTypes

LogicalConsumer := {}

vec4f = { x: 1.0, y: 2.0, z: 3.0, w: 4.0 }

vec4u = { x: 1, y: 2, z: 3, w: 4 }

vec4i = { x: -1, y: -2, z: -3, w: -4 }

nested : ShaderTypes.Nested
nested = { inner: [vec4u, vec4u], pad: 0.5 }

array_data : ShaderTypes.ArrayData
array_data = {
	lead: 1.0,
	konst: [vec4f, vec4f],
	stages: [vec4u],
	wedge: { x: 8.0, y: 9.0 },
	offsets: [vec4i],
	tail: 2.0,
	nested,
	flags: vec4u,
	bias: vec4i,
}

params : ShaderTypes.Params
params = {
	scale: 1.0,
	tex: DescriptorHandle(18446744073709551615),
	items: PointerAddress(42),
	mask: DescriptorHandle(7),
	tint: vec4f,
	offset: { x: 0.25, y: 0.5 },
}

resource : ShaderReflection.ResourceReference
resource = ResourceReference("texture-name")

expect List.len(array_data.konst) == 2
expect List.get(array_data.offsets, 0) == Ok(vec4i)
expect array_data.nested.pad == 0.5
expect ShaderTypes.mode_tag(Sparse) == 7
expect ShaderTypes.untagged_tag(Second) == 1
expect match params.tex {
	DescriptorHandle(raw) => raw == 18446744073709551615
}
expect match params.items {
	PointerAddress(raw) => raw == 42
}
expect match resource {
	ResourceReference(name) => name == "texture-name"
}
