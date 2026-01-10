#language slang 2026

// based on the sprite_batch example
module space_invaders;

import projection;

ParameterBlock<SpaceInvadersParams> params;

struct SpaceInvadersParams {
    Projection projection;
    StructuredBuffer<Sprite> sprites;
    StructuredBuffer<DebugBox> debugBoxes;
    Sampler2D spriteSheet;
}

struct Sprite {
    float2 scale;
    uint flags; // used on the CPU
    uint debugBoxId; // uint.maxValue = none

    float3 position;
    float rotation;

    float texU;
    float texV;
    float texW;
    float texH;

    float4 color;
}

struct DebugBox {
    float4 color;

    // as 0-1 uv-style coords within the sprite
    float2 position;
    // as a proportion of the sprite
    float2 size;
}

struct FragInput {
    float4 position : SV_Position;
    float2 texCoord : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 spriteCoord : TEXCOORD2;
    uint debugBoxId : TEXCOORD3;
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
FragInput vertMain(uint id : SV_VertexID) {
    let spriteId = id / 6;
    let sprite = params.sprites[spriteId];

    let vert = triangleIndices[id % 6];
    var coord = quadCorners[vert];
    let spriteCoord = coord;

    coord *= sprite.scale;

    let c = cos(sprite.rotation);
    let s = sin(sprite.rotation);
    let rotationMatrix = float2x2(c, s, -s, c);
    coord = mul(coord, rotationMatrix);

    let coordWithDepth = float4(coord + sprite.position.xy, sprite.position.z, 1.0);
    let position = params.projection.project(coordWithDepth);

    let left = sprite.texU;
    let right = sprite.texU + sprite.texW;
    let top = sprite.texV;
    let bottom = sprite.texV + sprite.texH;
    float2 texCoordsByVert[4] = {
        // NOTE the y-axis is flipped to match the OpenGL-style projection
        {left, bottom}, {right, bottom},
        {left, top}, {right, top}
    };
    let texCoord = texCoordsByVert[vert];

    return FragInput(position, texCoord, sprite.color, spriteCoord, sprite.debugBoxId);
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let debugSpriteEdges = false;
    if (debugSpriteEdges && nearSidesOfSprite(input.spriteCoord)) {
        return float4(1.0, 0.0, 1.0, 1.0);
    }

    if (input.debugBoxId != uint.maxValue) {
        let debugBox = params.debugBoxes[input.debugBoxId];
        if (nearSidesOfDebugBox(input.spriteCoord, debugBox)) {
            return debugBox.color;
        }
    }

    let textureColor = params.spriteSheet.Sample(input.texCoord);

    return input.color * textureColor;
}

func nearSidesOfSprite(texPos: float2) -> bool {
    let distToSidesX = min(texPos.x, abs(texPos.x - 1.0));
    let distToSidesY = min(texPos.y, abs(texPos.y - 1.0));
    let distToSides = min(distToSidesX, distToSidesY);

    return distToSides < 0.01;
}

func nearSidesOfDebugBox(spriteCoord: float2, debugBox: DebugBox) -> bool {
    let distToSidesX = min(
        abs(spriteCoord.x - debugBox.position.x),
        abs(spriteCoord.x - (debugBox.position.x + debugBox.size.x))
    );
    let distToSidesY = min(
        abs(spriteCoord.y - debugBox.position.y),
        abs(spriteCoord.y - (debugBox.position.y + debugBox.size.y))
    );
    let distToSides = min(distToSidesX, distToSidesY);

    return distToSides < 0.01;
}
