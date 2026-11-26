#language slang 2026

module roc_no_blocks;

// Test: a graphics shader without parameter blocks or a push block.

[shader("vertex")]
float4 vertexMain(uint vertexId : SV_VertexID) : SV_Position {
    return float4(float(vertexId), 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragmentMain() : SV_Target {
    return float4(1.0);
}
