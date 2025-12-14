#language slang 2026

module sdf_2d;

import mvp;

ParameterBlock<SDF2D> sdf2d;

struct SDF2D {
    MVPMatrices mvp;
    float time;
}

struct Vertex {
    float3 position;
};

struct FragVertex {
    float4 position : SV_POSITION;
};

[shader("vertex")]
FragVertex vertexMain(Vertex vertex) {
    let position = sdf2d.mvp.project(vertex.position);
    return FragVertex(position);
}

[shader("fragment")]
float4 fragmentMain(FragVertex fragVertex) {
    let color = colorForPixel(fragVertex.position.xy, sdf2d.time);
    return float4(color, 1.0);
}

func colorForPixel(pixel: float2, time: float) -> float3 {
    // fract returns fractional part. fract(1.3) == 0.3
    float red = fract(pixel.y);
    float green = sin(time);
    float blue = fract(pixel.x);

    return float3(red, green, blue);
}
