import pf.ShaderReflection
import HandleMixed
import Std140Arrays
import Std140Enums

LogicalConsumer := {}

vec4f = { x: 1.0, y: 2.0, z: 3.0, w: 4.0 }

vec4u = { x: 1, y: 2, z: 3, w: 4 }

vec4i = { x: -1, y: -2, z: -3, w: -4 }

nested : Std140Arrays.Nested
nested = { inner: [vec4u, vec4u], pad: 0.5 }

array_data : Std140Arrays.ArrayData
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

params : HandleMixed.Params
params = {
	scale: 1.0,
	tex: ShaderReflection.DescriptorHandle.(18446744073709551615),
	items: ShaderReflection.PointerAddress.(42),
	mask: ShaderReflection.DescriptorHandle.(7),
	tint: vec4f,
	offset: { x: 0.25, y: 0.5 },
}

expect List.len(array_data.konst) == 2
expect List.get(array_data.offsets, 0) == Ok(vec4i)
expect array_data.nested.pad == 0.5
expect Std140Enums.Mode.tag(Sparse) == 7
expect Std140Enums.Untagged.tag(Second) == 1
expect params.tex == ShaderReflection.DescriptorHandle.(18446744073709551615)
expect params.items == ShaderReflection.PointerAddress.(42)

## Packing requires every fixed array at its reflected length.
packed_array_data : Std140Arrays.ArrayData
packed_array_data = {
	lead: 1.0,
	konst: List.repeat(vec4f, 4),
	stages: List.repeat(vec4u, 8),
	wedge: { x: 8.0, y: 9.0 },
	offsets: List.repeat(vec4i, 3),
	tail: 2.0,
	nested: { inner: [vec4u, vec4u], pad: 0.5 },
	flags: vec4u,
	bias: vec4i,
}

expect List.len(HandleMixed.Params.to_bytes(params)) == U32.to_u64(HandleMixed.Params.gpu_size)
expect List.len(Std140Arrays.ArrayData.to_bytes(packed_array_data)) == U32.to_u64(Std140Arrays.ArrayData.gpu_size)

## `tail` is reflected at offset 272; 2.0 is 0x40000000 little-endian.
expect List.sublist(Std140Arrays.ArrayData.to_bytes(packed_array_data), { start: 272, len: 4 }) == [0, 0, 0, 64]
