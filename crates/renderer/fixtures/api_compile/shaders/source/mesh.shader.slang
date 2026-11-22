#language slang 2026

module mesh;

import mltrs;

ParameterBlock<MeshParams> meshParams;

struct MeshParams {
    float4 tint;
}

struct MeshVertex {
    float3 position;
    float3 color;
};

struct MeshFragInput {
    float4 position : SV_Position;
    float3 color;
};

[shader("vertex")]
MeshFragInput vertMain(MeshVertex vertex) {
    let position = float4(vertex.position, 1.0);

    return MeshFragInput(position, vertex.color);
}

[shader("fragment")]
float4 fragMain(MeshFragInput input) : SV_Target {
    let color = float4(input.color, 1.0);

    return color * meshParams.tint;
}
