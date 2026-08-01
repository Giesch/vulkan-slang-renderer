#language slang 2026

module std140_arrays;

// Test: float4/uint4/int4 arrays in a uniform buffer (std140), plus bare
// uint4/int4 vector fields. 16-byte vector elements are the only supported
// array element type: their stride equals their size in both std140 and
// std430, so a contiguous Rust array matches the GPU layout exactly.

ParameterBlock<ArrayData> params;

struct Nested {
    uint4 inner[2];  // offset 0, size 32
    float pad;       // offset 32, size 4 -> struct size rounds to 48
}

struct ArrayData {
    float lead;       // offset 0, size 4 (forces the first array off offset 0)
    float4 konst[4];  // offset 16 (12-byte padding), size 64
    uint4 stages[8];  // offset 80, size 128
    float2 wedge;     // offset 208, size 8 (trailing-position math between arrays)
    int4 offsets[3];  // offset 224 (8-byte padding), size 48 (odd count)
    float tail;       // offset 272, size 4 (struct-size rounding after an array)
    Nested nested;    // offset 288 (12-byte padding), size 48
    uint4 flags;      // offset 336, size 16 (bare integer vectors)
    int4 bias;        // offset 352, size 16
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    float4 acc = params.konst[id % 4] * params.lead;
    acc += float4(params.stages[id % 8]);
    acc += float4(params.wedge, params.tail, 1.0);
    acc += float4(params.offsets[id % 3]);
    acc += float4(params.nested.inner[id % 2]) * params.nested.pad;
    acc += float4(params.flags) + float4(params.bias);
    return acc;
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
