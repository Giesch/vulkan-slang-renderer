#language slang 2026

module sprite_batch;

// Based on the official example in C here:
// https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Examples/PullSpriteBatch.c
// https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/TexturedQuad.frag.hlsl
// https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/TexturedQuad.frag.hlsl
//
// which uses the method described in this blog post:
// https://moonside.games/posts/sdl-gpu-sprite-batcher/

ParameterBlock<SpriteBatchParams> params;

struct SpriteBatchParams {
    StructuredBuffer<Sprite> sprites;
    float4x4 projectionMatrix;
    Sampler2D texture;
}

struct Sprite {
    float3 position;
    float rotation;

    float2 scale;
    float2 padding;

    float texU;
    float texV;
    float texW;
    float texH;

    float4 color;
}

struct VertexOutput {
    // TODO
    // why does color use the tex coord semantic in the sdl gpu example?
    //   is it for matching up with the fragment shader?
    // why is it ok to use a float2 instead of a float4 for position?
    //   it just discards the rest?
    float2 texCoord : TEXCOORD0;
    float4 color : TEXCOORD1;
    float4 position : SV_Position;
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

    coord *= sprite.scale;

    let c = cos(sprite.rotation);
    let s = sin(sprite.rotation);
    let rotationMatrix = float2x2(c, s, -s, c);
    coord = mul(coord, rotationMatrix);

    let coordWithDepth = float4(coord + sprite.position.xy, sprite.position.z, 1.0);
    let position = mul(params.projectionMatrix, coordWithDepth);

    float2 texCoordsByVert[4] = {
        float2(sprite.texU,               sprite.texV              ),
        float2(sprite.texU + sprite.texW, sprite.texV              ),
        float2(sprite.texU,               sprite.texV + sprite.texH),
        float2(sprite.texU + sprite.texW, sprite.texV + sprite.texH)
    };
    let texCoord = texCoordsByVert[vert];

    return VertexOutput(texCoord, sprite.color, position);
}

[shader("fragment")]
float4 fragMain(VertexOutput input) : SV_TARGET {
    return input.color * params.texture.Sample(input.texCoord);
}
