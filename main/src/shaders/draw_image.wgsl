// Description: A simple example of drawing an image fullscreen us.ing a texture.
// Does so by using a larger triangle that extends beyond the screen, 
// (-1,1)            (1,1)              (3,1)
// ----------------------------------------
// |                   *                .
// |                   *             .
// |     (image)       *          .
// |                   *       .
// |                   *    .
// |                   * .
// |********************  (1,1)
// |                .
// |             .
// |          .
// |       .
// |    .
// | .
// (-1,-3)
struct OurVertexShaderOutput {
    @builtin(position) position : vec4f,
    @location(0) texcoord : vec2f,
};

@vertex fn vs_main(@builtin(vertex_index) vertexIndex : u32) -> OurVertexShaderOutput {
    var pos = array(
    vec2f(-1.0, 1.0),
    vec2f(-1.0, -3.0),
    vec2f(3.0, 1.0),
    );

    var tex = array(
    vec2f(0.0, 0.0),
    vec2f(0.0, 2.0),
    vec2f(2.0, 0.0),
    );

    var vsOutput : OurVertexShaderOutput;
    vsOutput.position = vec4f(pos[vertexIndex], 0.0, 1.0);
    vsOutput.texcoord = tex[vertexIndex];
    return vsOutput;
}

@group(0) @binding(0) var ourTexture : texture_2d<f32>;
@group(0) @binding(1) var ourSampler : sampler;

@fragment fn fs_main(fsInput : OurVertexShaderOutput) -> @location(0) vec4f {
    return textureSample(ourTexture, ourSampler, fsInput.texcoord);
}
