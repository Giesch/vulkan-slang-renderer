#language slang 2026

module std430_enums;

// Test: enum fields in a std430 BDA pointee, and inside a nested struct.
//
// NOTE deliberately no float4/Mat4 member anywhere: a 16-byte member would push
// max_alignment to 16 regardless, masking a name-based alignment lookup that
// treats the enum's type name as an unknown struct (and so assumes 16). Without
// the per-field alignment, Item's computed size rounds 8 up to 16 and disagrees
// with slang's reflected pointee size.

enum Kind : uint {
    Point = 0,
    Line = 1,
}

ParameterBlock<Params> params;

struct Params {
    LayoutPtr<Item, Std430DataLayout> items;
    Nested nested;
}

struct Item {
    float value;    // offset 0, size 4
    Kind kind;      // offset 4, size 4, align 4 -> Item is 8 bytes, align 4
}

struct Nested {
    float scale;    // offset 0, size 4
    Kind kind;      // offset 4, size 4
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    let item = params.items[id];
    let selected = item.kind == Kind.Line ? item.value : 0.0;
    return float4(selected, params.nested.scale, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    if (params.nested.kind == Kind.Point) {
        return float4(0.0);
    }
    return float4(1.0);
}
