#language slang 2026

module ownership;

import ownership_shared;

struct LocalParams {
    SharedPayload payload;
}

ParameterBlock<LocalParams> params;

[shader("vertex")]
float4 vertexMain(uint id : SV_VertexID) : SV_Position {
    return float4(float(id), 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragmentMain() : SV_Target {
    return params.payload.data.color * float(params.payload.mode);
}
