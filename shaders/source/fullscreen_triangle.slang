#language slang 2026

module fullscreen_triangle;

public struct FullscreenPosition {
    // [-1, 1] normalized device coordinates
    public float4 svPosition;
    // [0, 1] uv texture coordinates
    public float2 texCoord;
    // [-1, 1] with origin at the center
    public float2 centeredCoords;
}

// a screen-covering triangle in [0, 1] texture coordinates
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
public func fullscreenPosition(svVertexId: uint) -> FullscreenPosition {
    let texCoord = fullScreenTriangleVerts[svVertexId];
    let svPosition = float4(2 * texCoord - 1.0, 0.0, 1.0);

    // reflect Y-axis to match OpenGL & move origin to center
    // this assumes a negative-z camera position
    let centeredCoords = mul(reflectY, texCoord) + float2(-0.5, 0.5);

    return FullscreenPosition(svPosition, texCoord, centeredCoords);
}
