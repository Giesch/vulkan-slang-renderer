#language slang 2026

module indirect_mesh;

import mltrs;

ParameterBlock<IndirectMeshParams> indirectMeshParams;

struct IndirectMeshParams {
    float4 tint;
    mltrs::ImmutableAddr<IndirectVertex> vertices;
}

struct IndirectVertex {
    float2 position;
    float2 uv;
}

struct IndirectMeshFragInput {
    float4 position : SV_Position;
    float2 uv;
}

// reading SV_DrawIndex makes this a GraphicsIndirect shader
[shader("vertex")]
IndirectMeshFragInput vertMain(uint vertexId : SV_VertexID, uint drawIndex : SV_DrawIndex) {
    let vertex = indirectMeshParams.vertices[vertexId];
    let depth = 0.5 + float(drawIndex) * 0.01;
    let position = float4(vertex.position, depth, 1.0);

    return IndirectMeshFragInput(position, vertex.uv);
}

[shader("fragment")]
float4 fragMain(IndirectMeshFragInput input) : SV_Target {
    let color = float4(input.uv, 0.0, 1.0);

    return color * indirectMeshParams.tint;
}
