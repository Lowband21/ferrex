// Back-face poster shader with menu baseline
// Uses the same instance layout as the front shader but simpler visuals:
// - Dimmed/blur-ready backface (blur can be added later)
// - Rounded-rect clip and border reuse common helpers

struct Globals {
    transform: mat4x4<f32>,
    scale_factor: f32,
    atlas_is_srgb: f32,
    target_is_srgb: f32,
    _padding3: f32,
    _padding4: vec4<f32>,
}

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position_and_size: vec4<f32>,
    @location(1) radius_opacity_rotation_anim: vec4<f32>,
    @location(2) theme_color_zdepth: vec4<f32>,
    @location(3) scale_shadow_glow_type: vec4<f32>,
    @location(4) hover_overlay_border_progress: vec4<f32>,
    @location(5) mouse_pos_and_padding: vec4<f32>,
    @location(6) progress_color_and_padding: vec4<f32>,
    @location(7) atlas_uvs: vec4<f32>,
    @location(8) atlas_layer: i32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) corner_radius_normalized: f32,
    @location(2) opacity: f32,
    @location(3) theme_color: vec3<f32>,
    @location(4) local_pos: vec2<f32>,
    @location(5) layer: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(1) @binding(0) var atlas_texture: texture_2d_array<f32>;

fn vertex_position(vertex_index: u32) -> vec2<f32> {
    let x = f32(vertex_index & 1u);
    let y = f32((vertex_index >> 1u) & 1u);
    return vec2<f32>(x, y);
}

fn apply_3d_transform(pos: vec2<f32>, center: vec2<f32>, z_depth: f32, scale: f32) -> vec2<f32> {
    let scaled = center + (pos - center) * scale;
    let perspective_factor = 1.0 / (1.0 - z_depth * 0.001);
    return center + (scaled - center) * perspective_factor;
}

@vertex
fn vs_main_back(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    let position = input.position_and_size.xy;
    let size = input.position_and_size.zw;
    let radius = input.radius_opacity_rotation_anim.x;
    let opacity = input.radius_opacity_rotation_anim.y;
    let rotation_y = input.radius_opacity_rotation_anim.z;
    let animation_progress = input.radius_opacity_rotation_anim.w;
    let theme_color = input.theme_color_zdepth.xyz;
    let z_depth = input.theme_color_zdepth.w;
    let scale = input.scale_shadow_glow_type.x;
    let atlas_uv_min = input.atlas_uvs.xy;
    let atlas_uv_max = input.atlas_uvs.zw;

    let vertex_pos = vertex_position(input.vertex_index);
    let position_final = position + vertex_pos * size;
    let center = position + size * 0.5;

    var transformed_pos = apply_3d_transform(position_final, center, z_depth, scale);

    // Flip handling (keep consistent with front shader)
    if abs(rotation_y) > 0.00001 {
        let pi = 3.14159265359;
        let norm_rotation = rotation_y - floor(rotation_y / pi) * pi;
        let flip_progress = sin(norm_rotation);
        let scale_x = abs(cos(norm_rotation));
        transformed_pos.x = center.x + (transformed_pos.x - center.x) * scale_x;
        let perspective_offset = flip_progress * 0.001;
        let depth = sin(norm_rotation) * 100.0;
        let perspective_scale = 1.0 / (1.0 + depth * perspective_offset);
        transformed_pos.x = center.x + (transformed_pos.x - center.x) * scale_x * perspective_scale;
        transformed_pos.y = center.y + (transformed_pos.y - center.y) * perspective_scale;
    }

    let viewport_width = 2.0 / globals.transform[0][0];
    let viewport_height = 2.0 / abs(globals.transform[1][1]);
    let physical_pos = transformed_pos * globals.scale_factor;
    let clip_x = (physical_pos.x / viewport_width) * 2.0 - 1.0;
    let clip_y = 1.0 - (physical_pos.y / viewport_height) * 2.0;
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);

    let widget_u = mix(vertex_pos.x, 1.0 - vertex_pos.x, 1.0); // backface flip
    let widget_v = vertex_pos.y;
    output.tex_coord = vec2<f32>(
        mix(atlas_uv_min.x, atlas_uv_max.x, widget_u),
        mix(atlas_uv_min.y, atlas_uv_max.y, widget_v)
    );

    let min_dimension = min(size.x, size.y);
    output.corner_radius_normalized = radius / min_dimension;
    output.opacity = opacity;
    output.theme_color = theme_color;
    output.local_pos = vertex_pos;
    output.layer = f32(input.atlas_layer);
    return output;
}

@fragment
fn fs_main_back(input: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture if UV is valid; otherwise use theme color
    let uv = input.tex_coord;
    let uv_oob = any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
    var linear_rgb: vec3<f32>;
    var alpha = 1.0;
    if uv_oob {
        linear_rgb = input.theme_color * 0.4;
    } else {
        let sampled = textureSample(atlas_texture, atlas_sampler, uv, i32(input.layer));
        let tex_rgb = sampled.rgb;
        linear_rgb = select(srgb_to_linear(tex_rgb), tex_rgb, globals.atlas_is_srgb > 0.5);
        alpha = sampled.a;
        // Dim for backface
        linear_rgb *= 0.4;
    }

    let dist = rounded_rect_sdf_normalized(input.local_pos, input.corner_radius_normalized);
    let aa = max(1e-3, fwidth(dist));
    let coverage = 1.0 - smoothstep(0.0, aa, dist);

    var final_color = to_premul(vec4<f32>(linear_rgb, alpha)) * (coverage * input.opacity);

    // Simple inner border
    let d = dist;
    let d_aa = max(1e-3, fwidth(d));
    let border_px = 1.2;
    let w = border_px * d_aa;
    let inner = smoothstep(-w, -w + d_aa, d);
    let edge = 1.0 - smoothstep(0.0, d_aa, d);
    let border_alpha = clamp(min(inner, edge), 0.0, 1.0);
    if border_alpha > 0.0 {
        let border_rgb = mix(linear_rgb, vec3<f32>(0.8, 0.9, 1.0), 0.4);
        let border_pm = vec4<f32>(border_rgb * border_alpha, border_alpha);
        final_color = over(border_pm, final_color);
    }

    if final_color.a > 0.0001 {
        final_color = vec4<f32>(final_color.rgb / final_color.a, final_color.a);
    }
    if globals.target_is_srgb <= 0.5 {
        final_color = vec4<f32>(linear_to_srgb(final_color.rgb), final_color.a);
    }
    return final_color;
}
