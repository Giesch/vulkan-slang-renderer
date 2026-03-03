#language slang 2026

module paint_brush;

ParameterBlock<BrushParams> brushParams;

public struct StrokePoint {
    public float2 position;
    public float2 _pad;
}

struct BrushParams {
    RWTexture2D<float4> canvas;
    StructuredBuffer<StrokePoint> strokePoints;
    uint pointCount;
    float brushRadius;
    float brushOpacity;
    float4 brushColor;
    float2 canvasSize;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel: uint2 = dispatchThreadID.xy;

    let outOfBounds = pixel.x >= uint(brushParams.canvasSize.x) || pixel.y >= uint(brushParams.canvasSize.y);
    if (outOfBounds)
        return;

    if (brushParams.pointCount == 0)
        return;

    let pixelCenter = float2(pixel) + 0.5;
    float4 existing = brushParams.canvas[pixel];

    for (uint i = 0; i < brushParams.pointCount; i++) {
        float2 brushPos = brushParams.strokePoints[i].position;
        float dist = distance(pixelCenter, brushPos);

        if (dist > brushParams.brushRadius)
            continue;

        // Smooth falloff using smoothstep
        float t = dist / brushParams.brushRadius;
        float alpha = (1.0 - smoothstep(0.6, 1.0, t)) * brushParams.brushOpacity;

        // Alpha blend: brush over existing
        float outAlpha = alpha + existing.a * (1.0 - alpha);
        float3 outColor;
        if (outAlpha > 0.0) {
            outColor = (brushParams.brushColor.rgb * alpha + existing.rgb * existing.a * (1.0 - alpha)) / outAlpha;
        } else {
            outColor = float3(0.0);
        }

        existing = float4(outColor, outAlpha);
    }

    brushParams.canvas[pixel] = existing;
}
