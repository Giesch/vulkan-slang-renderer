#language slang 2026

module depth_texture;

import mvp;

// modified from the official tutorial example here:
// https://docs.vulkan.org/tutorial/latest/_attachments/27_shader_depth.slang

ParameterBlock<DepthTextureParams> params;

struct DepthTextureParams {
    MVPMatrices mvp;
    Sampler2D texture;
}

struct Vertex {
    float3 position;
    float3 color;
    float2 texCoord;
};

[shader("vertex")]
FragVertex vertMain(uint vertexIndex : SV_VertexID, Vertex vertex) {
    let position = params.mvp.project(vertex.position);

    if (vertexIndex == 0) {
        // this is intended as a demo of shader printf
        // run an example like 'just shader-debug viking_room' to try it
        printf(
          "first vertex position: %f, %f, %f\n",
          position.x, position.y, position.z
        );
    }

    return FragVertex(position, vertex.color, vertex.texCoord);
}

struct FragVertex {
    float4 position : SV_Position;
    float3 color;
    float2 texCoord;
};

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    let textureColor = params.texture.Sample(fragVertex.texCoord);

    // this is intended as a demo of hot reload
    // swap the commented line to try it
    let debugColor = float3(1.0);
    // let debugColor = float3(1.0, 0.0, 1.0);

    return float4(debugColor * textureColor.rgb, 1.0);
}
