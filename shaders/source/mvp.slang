#language slang 2026

module mvp;

// "GLM was originally designed for OpenGL,
// where the Y coordinate of the clip coordinates is inverted.
// The easiest way to compensate for that is to flip the sign
// on the scaling factor of the Y axis in the projection matrix.
// If you don’t do this, then the image will be rendered upside down."
// https://docs.vulkan.org/tutorial/latest/05_Uniform_buffers/00_Descriptor_set_layout_and_buffer.html
static const float4x4 reflectY = float4x4(
    1.0,  0.0, 0.0, 0.0,
    0.0, -1.0, 0.0, 0.0,
    0.0,  0.0, 1.0, 0.0,
    0.0,  0.0, 0.0, 1.0,
);

public struct MVPMatrices {
    float4x4 model;
    float4x4 view;
    float4x4 proj;

    [ForceInline]
    public func project(position: float3) -> float4 {
        let homogenousPosition = float4(position, 1.0);

        let projection = mul(reflectY, this.proj);
        let mvpMatrix = mul(projection, mul(this.view, this.model));

        return mul(mvpMatrix, homogenousPosition);
    }
};
