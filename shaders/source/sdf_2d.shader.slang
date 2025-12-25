#language slang 2026

module sdf_2d;

ParameterBlock<SDF2DParams> params;

struct SDF2DParams {
    float4x4 projectionMatrix;
}

// a list of two clockwise triangle indicies into quadCorners to make a quad
static const uint triangleIndices[6] = {0, 1, 2, 3, 2, 1};

// the 4 verticies used in a quad
static const float2 quadCorners[4] = {
    {0.0f, 0.0f},
    {1.0f, 0.0f},
    {0.0f, 1.0f},
    {1.0f, 1.0f}
};

struct VertexOutput {
    float4 position : SV_Position;
    float2 texCoord : TEXCOORD0;
}

[shader("vertex")]
VertexOutput vertMain(uint id : SV_VertexID) {
    // https://www.saschawillems.de/blog/2016/08/13/vulkan-tutorial-on-rendering-a-fullscreen-quad-without-buffers/

    // id:      0,      1,      2
    // uv: (0, 0), (0, 2), (2, 0)
    let texCoord = float2((id << 1) & 2, id & 2);

    let position = float4(texCoord * 2.0 + -1.0, 0.0, 1.0);

    // printf("in vert shader; texCoord: %1.2v2f, position: %1.2v4f", texCoord, position);

    return VertexOutput(position, texCoord);
}

[shader("fragment")]
float4 fragMain(VertexOutput input) : SV_Target {
    let p = input.texCoord;
    // printf("in pixel shader; p: %1.2v2f", p);
    // let p = input.position;
    // printf("in frag shader; position = %d, %d, %d, %d", p.x, p.y, p.z, p.w);
    return float4(1.0, 0.0, 0.0, 1.0);
    // return input.position;
}
