#language slang 2026

module fullscreen_triangle;

public struct FullscreenPosition {
    public float4 svPosition;
    public float2 pixel;
}

static const float2 fullScreenTriangleVerts[3] = {
    {0.0, 0.0},
    {0.0, 2.0},
    {2.0, 0.0},
};

static const float2x2 reflectY = float2x2(
    1.0,  0.0,
    0.0, -1.0,
);

// for use with exactly 3 vertices
public func fullscreenPosition(svVertexId: uint, resolution: float2) -> FullscreenPosition {
    let texCoord = fullScreenTriangleVerts[svVertexId];

    let position = float4(2 * texCoord - 1.0, 0.0, 1.0);

    // reflect Y-axis to match OpenGL
    var openglCoords = mul(reflectY, texCoord);
    // move origin to center
    openglCoords = openglCoords + float2(0.0, 1.0);
    // scale to screen
    openglCoords *= resolution;

    return FullscreenPosition(position, openglCoords);
}
