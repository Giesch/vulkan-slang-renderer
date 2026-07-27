#language slang 2026

module std140_shared_enum;

// Test: an enum declared in a shared slang module is hoisted into that module's
// generated file and imported here, rather than duplicated per shader.

import shared_enum;

ParameterBlock<SharedEnumData> params;

struct SharedEnumData {
    float scale;        // offset 0, size 4
    SharedMode mode;    // offset 4, size 4
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.scale, 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    if (params.mode == SharedMode.Busy) {
        return float4(0.0);
    }
    return float4(1.0);
}
