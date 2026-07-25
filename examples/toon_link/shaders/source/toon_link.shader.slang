#language slang 2026

module toon_link;

import mvp;

ParameterBlock<ToonLinkParams> params;

struct ToonLinkParams {
    MVPMatrices mvp;
    // 0 = world-space normals as color (the P6 diagnostic), 1 = uv0 as color.
    // NOTE: changing this struct's shape while `just dev` runs trips
    // assert_shader_interface_unchanged — body edits hot-reload, shape
    // edits need `just shaders` + a restart.
    uint debugMode;
}

struct Vertex {
    float3 position;
    // model space; the converter bakes the bind pose, so no skinning here
    float3 normal;
    float2 uv0;
};

struct FragVertex {
    float4 position : SV_Position;
    float3 normal;
    float2 uv0;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    // the model matrix is rotation-free uniform scale in P6, so
    // rotateDirection is exact; renormalized per-fragment after interpolation
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    return FragVertex(position, worldNormal, vertex.uv0);
}

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    if (params.debugMode == 1) {
        // UV debug: red = u, green = v
        return float4(fragVertex.uv0, 0.0, 1.0);
    }
    let normal = normalize(fragVertex.normal);
    return float4(normal * 0.5 + 0.5, 1.0);
}
