#language slang 2026

module sprite_batch;

// based on the sprite_batch example

ParameterBlock<SpaceInvadersParams> params;

struct SpaceInvadersParams {
    StructuredBuffer<Sprite> sprites;
    float4x4 projectionMatrix;
    Sampler2D spriteSheet;
}

struct Sprite {
    float2 scale;
    float2 padding;

    float3 position;
    float rotation;

    float texU;
    float texV;
    float texW;
    float texH;

    float4 color;
}

struct VertexOutput {
    float2 texCoord : TEXCOORD0;
    float4 color : TEXCOORD1;
    float4 position : SV_Position;
    float2 spriteCoord : TEXCOORD2;
}

// a list of two clockwise triangle indicies into quadCorners to make a quad
static const uint triangleIndices[6] = {0, 1, 2, 3, 2, 1};

// the 4 verticies used in a quad
static const float2 quadCorners[4] = {
    {0.0f, 0.0f},
    {1.0f, 0.0f},
    {0.0f, 1.0f},
    {1.0f, 1.0f}
};

[shader("vertex")]
VertexOutput vertMain(uint id : SV_VertexID) {
    let sprite = params.sprites[id / 6];

    let vert = triangleIndices[id % 6];
    var coord = quadCorners[vert];
    let spriteCoord = coord;

    coord *= sprite.scale;

    let c = cos(sprite.rotation);
    let s = sin(sprite.rotation);
    let rotationMatrix = float2x2(c, s, -s, c);
    coord = mul(coord, rotationMatrix);

    let coordWithDepth = float4(coord + sprite.position.xy, sprite.position.z, 1.0);
    let position = mul(params.projectionMatrix, coordWithDepth);

    float2 texCoordsByVert[4] = {
        // NOTE V coordinates are flipped to match the projection
        {sprite.texU,               sprite.texV + sprite.texH},
        {sprite.texU + sprite.texW, sprite.texV + sprite.texH},
        {sprite.texU,               sprite.texV              },
        {sprite.texU + sprite.texW, sprite.texV              }
    };
    let texCoord = texCoordsByVert[vert];

    return VertexOutput(texCoord, sprite.color, position, spriteCoord);
}

struct FragInput {
    float2 texCoord : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 spriteCoord : TEXCOORD2;
}

static const bool debugSpriteEdges = false;

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    if (debugSpriteEdges) {
        let distToSidesX = min(input.spriteCoord.x, abs(input.spriteCoord.x - 1.0));
        let distToSidesY = min(input.spriteCoord.y, abs(input.spriteCoord.y - 1.0));
        let distToSides = min(distToSidesX, distToSidesY);
        if (distToSides < 0.01) {
            return float4(1.0, 0.0, 0.0, 1.0);
        }
    }

    let textureColor = params.spriteSheet.Sample(input.texCoord);
    return input.color * textureColor;
}
