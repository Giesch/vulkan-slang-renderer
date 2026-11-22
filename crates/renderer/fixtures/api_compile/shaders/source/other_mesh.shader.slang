#language slang 2026

module other_mesh;

import mltrs;

ParameterBlock<OtherMeshParams> otherMeshParams;

struct OtherMeshParams {
    float4 tint;
}

struct OtherVertex {
    float2 position;
    float2 uv;
};

struct OtherMeshFragInput {
    float4 position : SV_Position;
    float2 uv;
};

[shader("vertex")]
OtherMeshFragInput vertMain(OtherVertex vertex) {
    let position = float4(vertex.position, 0.5, 1.0);

    return OtherMeshFragInput(position, vertex.uv);
}

[shader("fragment")]
float4 fragMain(OtherMeshFragInput input) : SV_Target {
    let color = float4(input.uv, 0.0, 1.0);

    return color * otherMeshParams.tint;
}
