#language slang 2026

module multi_mesh;

import mvp;

ParameterBlock<MultiMeshParams> params;

struct MultiMeshParams {
    MVPMatrices mvp;
    float4 tint;
}

struct Vertex {
    float3 position;
    // model-space; rotated into world space below so lighting stays correct
    // while each shape's model matrix animates
    float3 normal;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    return FragVertex(position, worldNormal);
}

struct FragVertex {
    float4 position : SV_Position;
    float3 normal;
};

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    let lightDir = normalize(float3(0.4, 1.0, 0.6));
    let normal = normalize(fragVertex.normal);
    let lambert = 0.35 + 0.65 * max(dot(normal, lightDir), 0.0);
    let color = params.tint.rgb * lambert;

    return float4(color, 1.0);
}
