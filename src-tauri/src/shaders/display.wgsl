struct Transform {
    rect: vec4<f32>,
    clip: vec4<f32>,
    window: vec2<f32>,
    image_size: vec2<f32>,
    texture_size: vec2<f32>,
    pixelated: f32,
    _pad: f32,
    bg_primary: vec4<f32>,
    bg_secondary: vec4<f32>,
};

@group(0) @binding(0) var<uniform> transform: Transform;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct DisplayColor {
    image_matrix: mat3x3<f32>,
    background_matrix: mat3x3<f32>,
    input_transfer: u32,
    output_encoding: u32,
    _pad: vec2<u32>,
};
@group(0) @binding(3) var<uniform> color: DisplayColor;
@group(0) @binding(4) var display_curves: texture_2d<f32>;

fn decode_srgb(v: vec3<f32>) -> vec3<f32> {
    return select(pow(max((v + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}

fn encode_surface(rgb: vec3<f32>) -> vec3<f32> {
    if (color.output_encoding == 4u) {
        let count = f32(textureDimensions(display_curves).x);
        let x = (sqrt(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))) * (count - 1.0) + 0.5) / count;
        return vec3<f32>(
            textureSampleLevel(display_curves, samp, vec2<f32>(x.r, 0.5), 0.0).r,
            textureSampleLevel(display_curves, samp, vec2<f32>(x.g, 0.5), 0.0).g,
            textureSampleLevel(display_curves, samp, vec2<f32>(x.b, 0.5), 0.0).b);
    }
    if (color.output_encoding == 1u) { return rgb; }
    if (color.output_encoding == 3u) {
        // SDR reference white matches scRGB: 80 nits, expressed as PQ/10000 nits.
        let y = pow(max(rgb, vec3<f32>(0.0)) * (80.0 / 10000.0), vec3<f32>(2610.0 / 16384.0));
        return pow((3424.0 / 4096.0 + (2413.0 / 128.0) * y) / (1.0 + (2392.0 / 128.0) * y), vec3<f32>(2523.0 / 32.0));
    }
    if (color.output_encoding == 2u) {
        let a = abs(rgb);
        return sign(rgb) * select(1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * a, a <= vec3<f32>(0.0031308));
    }
    return select(1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * rgb, rgb <= vec3<f32>(0.0031308));
}

fn present_pixel(pixel: vec4<f32>, background: bool) -> vec4<f32> {
    var linear = decode_srgb(pixel.rgb);
    if (!background && color.input_transfer == 1u) {
        linear = pow(max(pixel.rgb, vec3<f32>(0.0)), vec3<f32>(2.19921875));
    } else if (!background && color.input_transfer == 3u) {
        linear = select(pow(max((pixel.rgb + 0.09929682680944) / 1.09929682680944, vec3<f32>(0.0)), vec3<f32>(1.0 / 0.45)), pixel.rgb / 4.5, pixel.rgb < vec3<f32>(0.0812428582986315));
    }
    var rgb = color.image_matrix * linear;
    if (background) { rgb = color.background_matrix * linear; }
    rgb = encode_surface(rgb);
    // Keep negative/extended linear components: these carry out-of-sRGB colors.
    // The render pass uses premultiplied alpha, including transparent images.
    return vec4<f32>(rgb * pixel.a, pixel.a);
}

struct VertexOutput {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0)
    );
    let pos = uvs[id];

    let uv_x = transform.clip.x + pos.x * transform.clip.z;
    let uv_y = transform.clip.y + pos.y * transform.clip.w;

    let half_pixel_x = 0.5;
    let half_pixel_y = 0.5;
    let outset_x = (pos.x * 2.0 - 1.0) * half_pixel_x;
    let outset_y = (pos.y * 2.0 - 1.0) * half_pixel_y;

    let screen_x = uv_x + outset_x;
    let screen_y = uv_y + outset_y;

    let ndc_x = (screen_x / transform.window.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen_y / transform.window.y) * 2.0;

    var out: VertexOutput;
    out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);

    out.uv = vec2<f32>(
        (uv_x - transform.rect.x) / transform.rect.z,
        (uv_y - transform.rect.y) / transform.rect.w
    );

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return present_pixel(transform.bg_secondary, true);
    }

    let adjusted_uv = in.uv * (transform.image_size / transform.texture_size);
    let half_texel = vec2<f32>(0.5, 0.5) / transform.texture_size;

    let min_uv = half_texel;
    let max_uv = (transform.image_size / transform.texture_size) - half_texel;

    if (transform.pixelated > 0.5) {
        let texel_coords = floor(adjusted_uv * transform.texture_size);
        let nearest_uv = (texel_coords + vec2<f32>(0.5, 0.5)) / transform.texture_size;

        let clamped_nearest = clamp(nearest_uv, min_uv, max_uv);
        return present_pixel(textureSample(tex, samp, clamped_nearest), false);
    } else {
        let clamped_uv = clamp(adjusted_uv, min_uv, max_uv);
        return present_pixel(textureSample(tex, samp, clamped_uv), false);
    }
}
