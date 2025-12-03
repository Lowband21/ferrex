// Poster Title/Meta Text Shader
// Renders SDF text below poster widgets, NOT affected by flip rotation
// Text stays fixed regardless of poster flip state

struct Globals {
    transform: mat4x4<f32>,
    scale_factor: f32,
    atlas_is_srgb: f32,
    target_is_srgb: f32,
    text_scale: f32,
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
    // Text data
    @location(9) title_chars_0: vec4<u32>,   // title chars 0-15
    @location(10) title_chars_1: vec2<u32>,  // title chars 16-23
    @location(11) meta_chars: vec4<u32>,     // meta chars 0-15
    @location(12) text_params: vec4<f32>,    // [title_len, meta_len, reserved, reserved]
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) title_chars_0: vec4<u32>,
    @location(2) title_chars_1: vec2<u32>,
    @location(3) meta_chars: vec4<u32>,
    @location(4) text_params: vec4<f32>,
    @location(5) poster_width: f32,
    @location(6) opacity: f32,
    @location(7) text_zone_height: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(0) @binding(2) var font_atlas: texture_2d<f32>;

// Font atlas configuration (must match font_atlas.rs)
// Atlas is 1024 wide, but height is next_power_of_two(rows * cell_size) = 512
const FONT_ATLAS_WIDTH: f32 = 1024.0;
const FONT_ATLAS_HEIGHT: f32 = 512.0;  // 6 rows * 80px = 480 → 512
const FONT_GLYPH_SIZE: f32 = 64.0;
const FONT_SDF_PADDING: f32 = 8.0;
const FONT_CELL_SIZE: f32 = 80.0;  // 64 + 8*2
const FONT_GLYPHS_PER_ROW: i32 = 12; // floor(1024 / 80) = 12

// Text zone configuration (in logical pixels)
const TEXT_ZONE_HEIGHT: f32 = 50.0;
const TEXT_LEFT_PADDING: f32 = 8.0;  // Must be >= half cell width to avoid left clipping

// Baseline position within glyph cell (normalized 0-1)
// JetBrains Mono: ascender ~80%, descender ~20%, so baseline at ~75% from top
const BASELINE_OFFSET: f32 = 0.75;

fn vertex_position(vertex_index: u32) -> vec2<f32> {
    let x = f32(vertex_index & 1u);
    let y = f32((vertex_index >> 1u) & 1u);
    return vec2<f32>(x, y);
}

@vertex
fn vs_text(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    let position = input.position_and_size.xy;
    let size = input.position_and_size.zw;
    let opacity = input.radius_opacity_rotation_anim.y;
    let scale = input.scale_shadow_glow_type.x;  // Per-instance scale (hover or animation)
    let title_len = input.text_params.x;
    let meta_len = input.text_params.y;

    // Skip if no text
    if title_len <= 0.0 && meta_len <= 0.0 {
        output.clip_position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);  // Off-screen
        return output;
    }

    let vertex_pos = vertex_position(input.vertex_index);

    // Calculate scaled poster bounds (poster scales around its center)
    let center = position + size * 0.5;
    let scaled_size = size * scale;
    let scaled_position = center - scaled_size * 0.5;

    // Text zone is BELOW the SCALED poster, same width as scaled poster, no rotation applied
    // Scale text zone height with text_scale to accommodate larger text at high scales
    let scaled_text_zone_height = TEXT_ZONE_HEIGHT * globals.text_scale;
    let text_zone_start = vec2<f32>(scaled_position.x, scaled_position.y + scaled_size.y);
    let text_zone_size = vec2<f32>(scaled_size.x, scaled_text_zone_height);
    let position_final = text_zone_start + vertex_pos * text_zone_size;

    // Standard orthographic projection (NO flip rotation)
    let viewport_width = 2.0 / globals.transform[0][0];
    let viewport_height = 2.0 / abs(globals.transform[1][1]);
    let physical_pos = position_final * globals.scale_factor;
    let clip_x = (physical_pos.x / viewport_width) * 2.0 - 1.0;
    let clip_y = 1.0 - (physical_pos.y / viewport_height) * 2.0;

    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.local_pos = vertex_pos;
    output.title_chars_0 = input.title_chars_0;
    output.title_chars_1 = input.title_chars_1;
    output.meta_chars = input.meta_chars;
    output.text_params = input.text_params;
    output.poster_width = scaled_size.x;  // Use scaled width for text layout
    output.opacity = opacity;
    output.text_zone_height = scaled_text_zone_height;

    return output;
}

// Get UV coordinates for a glyph from the font atlas
fn get_glyph_uv(glyph_index: i32) -> vec4<f32> {
    let col = glyph_index % FONT_GLYPHS_PER_ROW;
    let row = glyph_index / FONT_GLYPHS_PER_ROW;

    let cell_u = f32(col) * FONT_CELL_SIZE / FONT_ATLAS_WIDTH;
    let cell_v = f32(row) * FONT_CELL_SIZE / FONT_ATLAS_HEIGHT;
    let cell_size_u = FONT_CELL_SIZE / FONT_ATLAS_WIDTH;
    let cell_size_v = FONT_CELL_SIZE / FONT_ATLAS_HEIGHT;

    return vec4<f32>(cell_u, cell_v, cell_u + cell_size_u, cell_v + cell_size_v);
}

// Unpack a character index from packed u32 arrays
fn unpack_title_char(chars_0: vec4<u32>, chars_1: vec2<u32>, char_idx: u32) -> u32 {
    let word_idx = char_idx / 4u;
    let byte_offset = (char_idx % 4u) * 8u;

    var word: u32;
    if word_idx == 0u {
        word = chars_0.x;
    } else if word_idx == 1u {
        word = chars_0.y;
    } else if word_idx == 2u {
        word = chars_0.z;
    } else if word_idx == 3u {
        word = chars_0.w;
    } else if word_idx == 4u {
        word = chars_1.x;
    } else {
        word = chars_1.y;
    }

    return (word >> byte_offset) & 0xFFu;
}

fn unpack_meta_char(chars: vec4<u32>, char_idx: u32) -> u32 {
    let word_idx = char_idx / 4u;
    let byte_offset = (char_idx % 4u) * 8u;

    var word: u32;
    if word_idx == 0u {
        word = chars.x;
    } else if word_idx == 1u {
        word = chars.y;
    } else if word_idx == 2u {
        word = chars.z;
    } else {
        word = chars.w;
    }

    return (word >> byte_offset) & 0xFFu;
}

// Render a single character, returns coverage (0-1)
// All positions are in PIXEL coordinates within the text zone
fn render_char(pixel_pos: vec2<f32>, char_pixel_pos: vec2<f32>, glyph_index: i32, font_size: f32) -> f32 {
    // Skip invalid glyph indices (71 chars total: 0-70)
    if glyph_index < 0 || glyph_index > 70 {
        return 0.0;
    }

    let glyph_uv = get_glyph_uv(glyph_index);

    // Position relative to character baseline, in font-size units
    let rel_x = (pixel_pos.x - char_pixel_pos.x) / font_size;
    let rel_y = (pixel_pos.y - char_pixel_pos.y) / font_size;

    // The cell contains a 48px glyph with 4px padding on each side (56px total)
    // Scale factor from glyph space to cell space
    let cell_scale = FONT_CELL_SIZE / FONT_GLYPH_SIZE;  // 56/48 = 1.167

    // Convert from glyph-relative to cell-relative (0 to 1 range)
    // X is centered, Y is baseline-aligned (baseline at BASELINE_OFFSET from top)
    let local_x = rel_x / cell_scale + 0.5;
    let local_y = rel_y / cell_scale + BASELINE_OFFSET;

    // Bounds check - if outside the cell, no coverage
    if local_x < 0.0 || local_x > 1.0 || local_y < 0.0 || local_y > 1.0 {
        return 0.0;
    }

    // Map to UV coordinates within the glyph's cell
    let uv = vec2<f32>(
        mix(glyph_uv.x, glyph_uv.z, local_x),
        mix(glyph_uv.y, glyph_uv.w, local_y)
    );

    // Sample SDF
    let sdf = textureSample(font_atlas, atlas_sampler, uv).r;

    // Scale-aware anti-aliasing using screen-space derivatives
    // Calculate how many screen pixels correspond to one SDF unit
    let screen_px_per_sdf_px = font_size / FONT_GLYPH_SIZE;
    let sdf_units_per_screen_px = 1.0 / (screen_px_per_sdf_px * FONT_SDF_PADDING);

    // Use fwidth for proper edge softness at any scale
    let edge_softness = clamp(sdf_units_per_screen_px * 0.5, 0.01, 0.5);

    return smoothstep(0.5 - edge_softness, 0.5 + edge_softness, sdf);
}

@fragment
fn fs_text(input: VertexOutput) -> @location(0) vec4<f32> {
    let title_len = i32(input.text_params.x);
    let meta_len = i32(input.text_params.y);
    let poster_width = input.poster_width;

    // Skip if no text
    if title_len == 0 && meta_len == 0 {
        discard;
    }

    // Convert from 0-1 normalized to pixel coordinates within text zone
    // Use scaled text zone height passed from vertex shader
    let pixel_x = input.local_pos.x * poster_width;
    let pixel_y = input.local_pos.y * input.text_zone_height;
    let pixel_pos = vec2<f32>(pixel_x, pixel_y);

    var coverage = 0.0;

    // Title line configuration (base 14px from FontTokens.caption, scaled)
    let title_font_size = 14.0 * globals.text_scale;
    let title_y = 16.0 * globals.text_scale;  // Baseline Y position from top of zone
    let title_spacing = title_font_size * 0.65;  // Character spacing (monospace-ish)
    // Scale left padding to accommodate larger glyphs at high scales
    let scaled_left_padding = TEXT_LEFT_PADDING * globals.text_scale;

    // Render title characters
    for (var i = 0; i < title_len && i < 24; i = i + 1) {
        let glyph_idx = unpack_title_char(input.title_chars_0, input.title_chars_1, u32(i));
        if glyph_idx == 0xFFu {
            break;  // Null terminator
        }

        let char_x = scaled_left_padding + f32(i) * title_spacing;
        let char_pos = vec2<f32>(char_x, title_y);
        let char_coverage = render_char(pixel_pos, char_pos, i32(glyph_idx), title_font_size);
        coverage = max(coverage, char_coverage);
    }

    // Meta line configuration (base 12px from FontTokens.small, scaled)
    let meta_font_size = 12.0 * globals.text_scale;
    let meta_y = 34.0 * globals.text_scale;  // Baseline Y position from top of zone
    let meta_spacing = meta_font_size * 0.65;

    var meta_coverage = 0.0;
    for (var i = 0; i < meta_len && i < 16; i = i + 1) {
        let glyph_idx = unpack_meta_char(input.meta_chars, u32(i));
        if glyph_idx == 0xFFu {
            break;
        }

        let char_x = scaled_left_padding + f32(i) * meta_spacing;
        let char_pos = vec2<f32>(char_x, meta_y);
        let char_coverage = render_char(pixel_pos, char_pos, i32(glyph_idx), meta_font_size);
        meta_coverage = max(meta_coverage, char_coverage);
    }

    // Discard if no text coverage
    if coverage < 0.01 && meta_coverage < 0.01 {
        discard;
    }

    // Title text: white/light
    let title_color = vec3<f32>(0.95, 0.95, 0.95);
    // Meta text: secondary gray
    let meta_color = vec3<f32>(0.6, 0.6, 0.6);

    // Blend title and meta colors based on coverage
    var final_rgb = title_color * coverage + meta_color * meta_coverage;
    let final_alpha = max(coverage, meta_coverage) * input.opacity;

    // Apply premultiplied alpha
    var final_color = vec4<f32>(final_rgb * final_alpha, final_alpha);

    // Color space conversion if needed (target is linear, output to sRGB)
    if globals.target_is_srgb <= 0.5 {
        final_color = vec4<f32>(pow(final_color.rgb, vec3<f32>(1.0 / 2.2)), final_color.a);
    }

    return final_color;
}
