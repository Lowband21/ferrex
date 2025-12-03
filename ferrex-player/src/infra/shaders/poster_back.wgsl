// Back-face poster shader with glassy menu buttons
// Renders a frosted glass effect with vertical button column

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
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) corner_radius_normalized: f32,
    @location(2) opacity: f32,
    @location(3) theme_color: vec3<f32>,
    @location(4) local_pos: vec2<f32>,
    @location(5) layer: f32,
    @location(6) mouse_pos: vec2<f32>,
    @location(7) aspect_ratio: f32,
    @location(8) progress_color: vec3<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(0) @binding(2) var font_atlas: texture_2d<f32>;
@group(1) @binding(0) var atlas_texture: texture_2d_array<f32>;

// ============================================================================
// Font Atlas Configuration
// These must match the values in font_atlas.rs
// ============================================================================
const FONT_ATLAS_WIDTH: f32 = 1024.0;
const FONT_ATLAS_HEIGHT: f32 = 512.0;  // 6 rows * 80px = 480 → 512
const FONT_GLYPH_SIZE: f32 = 64.0;
const FONT_SDF_PADDING: f32 = 8.0;
const FONT_CELL_SIZE: f32 = 80.0;  // 64 + 8*2
const FONT_GLYPHS_PER_ROW: i32 = 12; // floor(1024 / 80) = 12

// Character set: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .-:!?'&,"
// Index mapping (see font_atlas.rs for authoritative mapping):
// A-Z: 0-25, a-z: 26-51, 0-9: 52-61, space: 62, punctuation: 63-70
// NOTE: Menu labels only use A-Z (0-25), so direct indices work
fn char_to_glyph_index(c: i32) -> i32 {
    return c;
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

// Sample the SDF font atlas and return the distance field value
fn sample_font_sdf(uv: vec2<f32>) -> f32 {
    let sample = textureSample(font_atlas, atlas_sampler, uv);
    // SDF is stored in all channels (grayscale), 0.5 = edge
    return sample.r;
}

// Render a single character from the font atlas
// Returns coverage (0-1)
// aspect: poster width/height ratio (used to correct for non-square coordinates)
fn render_char(pos: vec2<f32>, char_pos: vec2<f32>, glyph_index: i32, char_scale: f32, aspect: f32) -> f32 {
    let glyph_uv = get_glyph_uv(glyph_index);

    // Calculate local position within the character cell
    // Apply aspect ratio correction to X to maintain proper character proportions
    let local = vec2<f32>(
        (pos.x - char_pos.x) * aspect / char_scale,
        (pos.y - char_pos.y) / char_scale
    );

    // Map to UV space (centered in cell)
    let uv = vec2<f32>(
        mix(glyph_uv.x, glyph_uv.z, local.x + 0.5),
        mix(glyph_uv.y, glyph_uv.w, local.y + 0.5)
    );

    // Check bounds
    if any(uv < vec2<f32>(glyph_uv.x, glyph_uv.y)) || any(uv > vec2<f32>(glyph_uv.z, glyph_uv.w)) {
        return 0.0;
    }

    // Sample SDF
    let sdf = sample_font_sdf(uv);

    // Scale-aware anti-aliasing
    // char_scale relates to screen size; calculate proper edge softness
    let screen_px_per_sdf_px = char_scale / (FONT_CELL_SIZE / FONT_ATLAS_WIDTH);
    let sdf_units_per_screen_px = 1.0 / (screen_px_per_sdf_px * FONT_SDF_PADDING);
    let edge_softness = clamp(sdf_units_per_screen_px * 0.5, 0.01, 0.5);

    return smoothstep(0.5 - edge_softness, 0.5 + edge_softness, sdf);
}

// Render a string label using the font atlas
// aspect: poster width/height ratio (used to correct for non-square coordinates)
fn render_atlas_label(pos: vec2<f32>, btn_index: i32, size: f32, aspect: f32) -> f32 {
    let btn_y_center = BUTTON_Y_START + (f32(btn_index) + 0.5) * BUTTON_HEIGHT;
    let label_right = 0.9; // 0.85; // Right edge anchor for text alignment

    var coverage = 0.0;
    let char_scale = size * 1.5;
    let char_spacing = size * 0.9;  // Spacing between character centers

    // Character indices: A=0..Z=25 (menu only uses uppercase)
    // Right-aligned: last character positioned at label_right
    // PLAY (4 chars)
    if btn_index == BTN_PLAY {
        let start_x = label_right - 3.0 * char_spacing; // 4 chars: positions 0,1,2,3
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x, btn_y_center), 15, char_scale, aspect)); // P
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing, btn_y_center), 11, char_scale, aspect)); // L
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 2.0, btn_y_center), 0, char_scale, aspect)); // A
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 3.0, btn_y_center), 24, char_scale, aspect)); // Y
    }
    // DETAILS (7 chars)
    else if btn_index == BTN_DETAILS {
        let start_x = label_right - 6.0 * char_spacing; // 7 chars: positions 0-6
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x, btn_y_center), 3, char_scale, aspect)); // D
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing, btn_y_center), 4, char_scale, aspect)); // E
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 2.0, btn_y_center), 19, char_scale, aspect)); // T
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 3.0, btn_y_center), 0, char_scale, aspect)); // A
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 4.0, btn_y_center), 8, char_scale, aspect)); // I
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 5.0, btn_y_center), 11, char_scale, aspect)); // L
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 6.0, btn_y_center), 18, char_scale, aspect)); // S
    }
    // WATCHED (7 chars)
    else if btn_index == BTN_WATCHED {
        let start_x = label_right - 6.0 * char_spacing; // 7 chars: positions 0-6
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x, btn_y_center), 22, char_scale, aspect)); // W
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing, btn_y_center), 0, char_scale, aspect)); // A
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 2.0, btn_y_center), 19, char_scale, aspect)); // T
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 3.0, btn_y_center), 2, char_scale, aspect)); // C
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 4.0, btn_y_center), 7, char_scale, aspect)); // H
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 5.0, btn_y_center), 4, char_scale, aspect)); // E
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 6.0, btn_y_center), 3, char_scale, aspect)); // D
    }
    // SOON (4 chars - placeholder for disabled button)
    else if btn_index == BTN_WATCHLIST {
        let start_x = label_right - 3.0 * char_spacing; // 4 chars: positions 0-3
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x, btn_y_center), 18, char_scale, aspect)); // S
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing, btn_y_center), 14, char_scale, aspect)); // O
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 2.0, btn_y_center), 14, char_scale, aspect)); // O
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 3.0, btn_y_center), 13, char_scale, aspect)); // N
    }
    // EDIT (4 chars)
    else if btn_index == BTN_EDIT {
        let start_x = label_right - 3.0 * char_spacing; // 4 chars: positions 0-3
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x, btn_y_center), 4, char_scale, aspect)); // E
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing, btn_y_center), 3, char_scale, aspect)); // D
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 2.0, btn_y_center), 8, char_scale, aspect)); // I
        coverage = max(coverage, render_char(pos, vec2<f32>(start_x + char_spacing * 3.0, btn_y_center), 19, char_scale, aspect)); // T
    }

    return coverage;
}

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
    let mouse_pos = input.mouse_pos_and_padding.xy;

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
    output.mouse_pos = mouse_pos; // Already normalized to 0-1 by Rust
    output.aspect_ratio = size.x / size.y;
    output.progress_color = input.progress_color_and_padding.xyz;
    return output;
}

// Menu button layout constants
// SYNC WARNING: Must match ferrex-player/src/infra/constants/menu.rs
// Run `cargo test shader_menu_constants_sync` to verify synchronization
const BUTTON_X_PADDING: f32 = 0.0;
const BUTTON_Y_START: f32 = 0.0;
const BUTTON_HEIGHT: f32 = 0.2;
const BUTTON_GAP: f32 = 0.0;
const BUTTON_RADIUS: f32 = 0.0;
const NUM_BUTTONS: i32 = 5;

// Button indices (must match menu::button_index::*)
const BTN_PLAY: i32 = 0;
const BTN_DETAILS: i32 = 1;
const BTN_WATCHED: i32 = 2;
const BTN_WATCHLIST: i32 = 3;
const BTN_EDIT: i32 = 4;

// Returns which button index the point is in, or -1 if none
fn get_button_index(pos: vec2<f32>) -> i32 {
    let y = pos.y;

    // With full coverage (no gaps), simply divide by button height
    if y < 0.0 || y > 1.0 {
        return -1;
    }

    let index = i32(y / BUTTON_HEIGHT);
    return select(index, NUM_BUTTONS - 1, index >= NUM_BUTTONS);
}

// SDF for a full-width menu button (simple rectangular region)
fn menu_button_sdf(pos: vec2<f32>, btn_index: i32, aspect: f32) -> f32 {
    let btn_y_start = BUTTON_Y_START + f32(btn_index) * BUTTON_HEIGHT;
    let btn_center = vec2<f32>(0.5, btn_y_start + BUTTON_HEIGHT * 0.5);
    let btn_half_size = vec2<f32>(0.5, BUTTON_HEIGHT * 0.5);

    // Adjust for aspect ratio (x direction)
    let adjusted_pos = vec2<f32>((pos.x - 0.5) * aspect, pos.y - btn_center.y);
    let adjusted_size = vec2<f32>(btn_half_size.x * aspect, btn_half_size.y);

    // Box SDF (no corner radius)
    let q = abs(adjusted_pos) - adjusted_size;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
}

// ============================================================================
// Icon SDFs for menu buttons
// ============================================================================

// Play triangle icon (pointing right)
fn icon_play_sdf(p: vec2<f32>, size: f32) -> f32 {
    let k = sqrt(3.0);
    var q = p;
    q.x = abs(q.x) - size;
    q.y = q.y + size / k;
    if q.x + k * q.y > 0.0 {
        q = vec2<f32>(q.x - k * q.y, -k * q.x - q.y) / 2.0;
    }
    q.x -= clamp(q.x, -2.0 * size, 0.0);
    return -length(q) * sign(q.y);
}

// Circle SDF (for info icon base)
fn icon_circle_sdf(p: vec2<f32>, r: f32) -> f32 {
    return length(p) - r;
}

// Rounded box SDF for general shapes
fn icon_box_sdf(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Checkmark icon
fn icon_check_sdf(p: vec2<f32>, size: f32) -> f32 {
    let thickness = size * 0.22;
    // Left leg of check (going down-left to center)
    let p1 = vec2<f32>(-size * 0.5, size * 0.1);
    let p2 = vec2<f32>(-size * 0.1, -size * 0.4);
    // Right leg of check (going from center up-right)
    let p3 = vec2<f32>(size * 0.5, size * 0.5);

    // Line segment SDFs
    let d1 = sd_segment(p, p1, p2) - thickness;
    let d2 = sd_segment(p, p2, p3) - thickness;
    return min(d1, d2);
}

// Line segment SDF helper
fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// List/bookmark icon (three horizontal lines)
fn icon_list_sdf(p: vec2<f32>, size: f32) -> f32 {
    let bar_height = size * 0.12;
    let bar_width = size * 0.7;
    let spacing = size * 0.35;

    let d1 = icon_box_sdf(p - vec2<f32>(0.0, spacing), vec2<f32>(bar_width, bar_height), bar_height * 0.5);
    let d2 = icon_box_sdf(p, vec2<f32>(bar_width, bar_height), bar_height * 0.5);
    let d3 = icon_box_sdf(p + vec2<f32>(0.0, spacing), vec2<f32>(bar_width, bar_height), bar_height * 0.5);
    return min(min(d1, d2), d3);
}

// Edit/pencil icon
fn icon_edit_sdf(p: vec2<f32>, size: f32) -> f32 {
    // Pencil body (rotated rectangle)
    let angle = 0.785; // 45 degrees
    let c = cos(angle);
    let s = sin(angle);
    let rotated = vec2<f32>(c * p.x + s * p.y, -s * p.x + c * p.y);

    let body = icon_box_sdf(rotated + vec2<f32>(0.0, size * 0.15), vec2<f32>(size * 0.12, size * 0.45), size * 0.06);

    // Pencil tip (triangle approximated as small box)
    let tip_pos = rotated + vec2<f32>(0.0, -size * 0.35);
    let tip = icon_box_sdf(tip_pos, vec2<f32>(size * 0.08, size * 0.12), size * 0.02);

    return min(body, tip);
}

// ============================================================================
// Simple SDF Text Rendering
// ============================================================================

// Individual letter SDFs (simplified block letters)
fn letter_sdf(p: vec2<f32>, char_code: i32, size: f32) -> f32 {
    let t = size * 0.08; // stroke thickness
    let w = size * 0.3;  // letter width
    let h = size * 0.5;  // letter height

    // P
    if char_code == 0 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.3, 0.0), vec2<f32>(t, h), 0.0);
        let top_curve = icon_box_sdf(p + vec2<f32>(0.0, h * 0.35), vec2<f32>(w * 0.25, t), 0.0);
        let mid_curve = icon_box_sdf(p + vec2<f32>(0.0, h * 0.0), vec2<f32>(w * 0.25, t), 0.0);
        let right = icon_box_sdf(p + vec2<f32>(-w * 0.25, h * 0.175), vec2<f32>(t, h * 0.22), 0.0);
        return min(min(stem, top_curve), min(mid_curve, right));
    }
    // L
    if char_code == 1 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.2, 0.0), vec2<f32>(t, h), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.85), vec2<f32>(w * 0.3, t), 0.0);
        return min(stem, bottom);
    }
    // A
    if char_code == 2 {
        let left = icon_box_sdf(p + vec2<f32>(w * 0.22, 0.0), vec2<f32>(t, h), 0.0);
        let right = icon_box_sdf(p + vec2<f32>(-w * 0.22, 0.0), vec2<f32>(t, h), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.85), vec2<f32>(w * 0.3, t), 0.0);
        let mid = icon_box_sdf(p + vec2<f32>(0.0, h * 0.1), vec2<f32>(w * 0.2, t * 0.8), 0.0);
        return min(min(left, right), min(top, mid));
    }
    // Y
    if char_code == 3 {
        let stem = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.4), vec2<f32>(t, h * 0.5), 0.0);
        let left_arm = icon_box_sdf(p + vec2<f32>(w * 0.2, h * 0.4), vec2<f32>(t, h * 0.35), 0.0);
        let right_arm = icon_box_sdf(p + vec2<f32>(-w * 0.2, h * 0.4), vec2<f32>(t, h * 0.35), 0.0);
        return min(stem, min(left_arm, right_arm));
    }
    // D
    if char_code == 4 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.05, h * 0.85), vec2<f32>(w * 0.25, t), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.05, -h * 0.85), vec2<f32>(w * 0.25, t), 0.0);
        let curve = icon_box_sdf(p + vec2<f32>(-w * 0.2, 0.0), vec2<f32>(t, h * 0.7), 0.0);
        return min(min(stem, curve), min(top, bottom));
    }
    // E
    if char_code == 5 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.85), vec2<f32>(w * 0.35, t), 0.0);
        let mid = icon_box_sdf(p + vec2<f32>(0.05, 0.0), vec2<f32>(w * 0.25, t), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.85), vec2<f32>(w * 0.35, t), 0.0);
        return min(min(stem, top), min(mid, bottom));
    }
    // T
    if char_code == 6 {
        let stem = icon_box_sdf(p, vec2<f32>(t, h), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.85), vec2<f32>(w * 0.35, t), 0.0);
        return min(stem, top);
    }
    // I
    if char_code == 7 {
        return icon_box_sdf(p, vec2<f32>(t, h), 0.0);
    }
    // S
    if char_code == 8 {
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.7), vec2<f32>(w * 0.3, t), 0.0);
        let mid = icon_box_sdf(p, vec2<f32>(w * 0.25, t), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.7), vec2<f32>(w * 0.3, t), 0.0);
        let top_left = icon_box_sdf(p + vec2<f32>(w * 0.22, h * 0.35), vec2<f32>(t, h * 0.3), 0.0);
        let bot_right = icon_box_sdf(p + vec2<f32>(-w * 0.22, -h * 0.35), vec2<f32>(t, h * 0.3), 0.0);
        return min(min(min(top, mid), min(bottom, top_left)), bot_right);
    }
    // W
    if char_code == 9 {
        let left = icon_box_sdf(p + vec2<f32>(w * 0.35, 0.0), vec2<f32>(t, h), 0.0);
        let right = icon_box_sdf(p + vec2<f32>(-w * 0.35, 0.0), vec2<f32>(t, h), 0.0);
        let mid = icon_box_sdf(p, vec2<f32>(t, h * 0.7), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.85), vec2<f32>(w * 0.4, t), 0.0);
        return min(min(left, right), min(mid, bottom));
    }
    // C
    if char_code == 10 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h * 0.7), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.85), vec2<f32>(w * 0.35, t), 0.0);
        let bottom = icon_box_sdf(p + vec2<f32>(0.0, -h * 0.85), vec2<f32>(w * 0.35, t), 0.0);
        return min(stem, min(top, bottom));
    }
    // H
    if char_code == 11 {
        let left = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let right = icon_box_sdf(p + vec2<f32>(-w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let mid = icon_box_sdf(p, vec2<f32>(w * 0.25, t), 0.0);
        return min(min(left, right), mid);
    }
    // N
    if char_code == 12 {
        let left = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let right = icon_box_sdf(p + vec2<f32>(-w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let diag = icon_box_sdf(p, vec2<f32>(t * 0.8, h * 0.6), 0.0);
        return min(min(left, right), diag);
    }
    // O
    if char_code == 13 {
        let outer = icon_circle_sdf(p, h * 0.5);
        let inner = icon_circle_sdf(p, h * 0.3);
        return max(outer, -inner);
    }
    // R
    if char_code == 14 {
        let stem = icon_box_sdf(p + vec2<f32>(w * 0.25, 0.0), vec2<f32>(t, h), 0.0);
        let top = icon_box_sdf(p + vec2<f32>(0.0, h * 0.7), vec2<f32>(w * 0.25, t), 0.0);
        let mid = icon_box_sdf(p + vec2<f32>(0.05, h * 0.15), vec2<f32>(w * 0.2, t), 0.0);
        let curve = icon_box_sdf(p + vec2<f32>(-w * 0.15, h * 0.42), vec2<f32>(t, h * 0.25), 0.0);
        let leg = icon_box_sdf(p + vec2<f32>(-w * 0.15, -h * 0.45), vec2<f32>(t, h * 0.35), 0.0);
        return min(min(min(stem, top), min(mid, curve)), leg);
    }
    return 1000.0; // Unknown character
}

// Render a text label - returns coverage (0-1)
fn render_label(pos: vec2<f32>, btn_index: i32, size: f32) -> f32 {
    let btn_y_center = BUTTON_Y_START + (f32(btn_index) + 0.5) * BUTTON_HEIGHT;
    let label_x = 0.5; // Centered horizontally
    let local_p = vec2<f32>(pos.x - label_x, pos.y - btn_y_center);

    var d = 1000.0;
    let char_spacing = size * 0.7;

    // PLAY = P(0) L(1) A(2) Y(3)
    if btn_index == BTN_PLAY {
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 1.5, 0.0), 0, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 0.5, 0.0), 1, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 0.5, 0.0), 2, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 1.5, 0.0), 3, size));
    }
    // DETAILS = D(4) E(5) T(6) A(2) I(7) L(1) S(8)
    else if btn_index == BTN_DETAILS {
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 3.0, 0.0), 4, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 2.0, 0.0), 5, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 1.0, 0.0), 6, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(0.0, 0.0), 2, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 1.0, 0.0), 7, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 2.0, 0.0), 1, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 3.0, 0.0), 8, size));
    }
    // WATCHED = W(9) A(2) T(6) C(10) H(11) E(5) D(4)
    else if btn_index == BTN_WATCHED {
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 3.0, 0.0), 9, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 2.0, 0.0), 2, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 1.0, 0.0), 6, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(0.0, 0.0), 10, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 1.0, 0.0), 11, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 2.0, 0.0), 5, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 3.0, 0.0), 4, size));
    }
    // SOON = S(8) O(13) O(13) N(12) (placeholder for disabled)
    else if btn_index == BTN_WATCHLIST {
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 1.5, 0.0), 8, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 0.5, 0.0), 13, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 0.5, 0.0), 13, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 1.5, 0.0), 12, size));
    }
    // EDIT = E(5) D(4) I(7) T(6)
    else if btn_index == BTN_EDIT {
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 1.5, 0.0), 5, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(char_spacing * 0.5, 0.0), 4, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 0.5, 0.0), 7, size));
        d = min(d, letter_sdf(local_p + vec2<f32>(-char_spacing * 1.5, 0.0), 6, size));
    }

    let aa = max(0.001, fwidth(d));
    return 1.0 - smoothstep(0.0, aa * 1.5, d);
}

// Render button icon - returns coverage (0-1)
// aspect: poster width/height ratio (used to correct for non-square coordinates)
fn render_icon(pos: vec2<f32>, btn_index: i32, size: f32, aspect: f32) -> f32 {
    let btn_y_center = BUTTON_Y_START + (f32(btn_index) + 0.5) * BUTTON_HEIGHT;
    let icon_x = 0.15; // Left side with padding
    // Apply aspect ratio to X for proper proportions, flip Y for SDF math convention (Y-up)
    let local_p = vec2<f32>((pos.x - icon_x) * aspect, -(pos.y - btn_y_center));

    var d = 1000.0;
    let icon_size = size;

    if btn_index == BTN_PLAY {
        // Rotate play triangle to point right
        let rotated = vec2<f32>(-local_p.y, local_p.x);
        d = icon_play_sdf(rotated, icon_size);
    } else if btn_index == BTN_DETAILS {
        // Info icon: circle with dot and line
        let circle = abs(icon_circle_sdf(local_p, icon_size)) - icon_size * 0.15;
        let dot = icon_circle_sdf(local_p + vec2<f32>(0.0, icon_size * 0.5), icon_size * 0.12);
        let stem = icon_box_sdf(local_p + vec2<f32>(0.0, -icon_size * 0.15), vec2<f32>(icon_size * 0.1, icon_size * 0.35), 0.0);
        d = min(circle, min(dot, stem));
    } else if btn_index == BTN_WATCHED {
        d = icon_check_sdf(local_p, icon_size);
    } else if btn_index == BTN_WATCHLIST {
        d = icon_list_sdf(local_p, icon_size);
    } else if btn_index == BTN_EDIT {
        d = icon_edit_sdf(local_p, icon_size);
    }

    let aa = max(0.001, fwidth(d));
    return 1.0 - smoothstep(0.0, aa * 1.5, d);
}

// Aspect-corrected rounded rect SDF for uniform border thickness
fn rounded_rect_sdf_aspect(p: vec2<f32>, radius_normalized: f32, aspect: f32) -> f32 {
    // p is in range 0-1, with (0.5, 0.5) at center
    // Correct for aspect ratio so corners are truly circular
    let centered = p - vec2<f32>(0.5);
    let corrected = vec2<f32>(centered.x * aspect, centered.y);
    let half_size = vec2<f32>(0.5 * aspect, 0.5);
    let radius = radius_normalized * aspect; // Scale radius with aspect
    let d = abs(corrected) - half_size + radius;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

@fragment
fn fs_main_back(input: VertexOutput) -> @location(0) vec4<f32> {
    let pos = input.local_pos;
    let mouse = input.mouse_pos;
    let aspect = input.aspect_ratio;

    // Sample texture for background or use theme color
    let uv = input.tex_coord;
    let uv_oob = any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
    var bg_rgb: vec3<f32>;
    var bg_alpha = 1.0;

    if uv_oob {
        bg_rgb = input.theme_color;
    } else {
        let sampled = textureSample(atlas_texture, atlas_sampler, uv, i32(input.layer));
        let tex_rgb = sampled.rgb;
        bg_rgb = select(srgb_to_linear(tex_rgb), tex_rgb, globals.atlas_is_srgb > 0.5);
        bg_alpha = sampled.a;
    }

    // Frosted glass effect: dim and desaturate
    let luminance = dot(bg_rgb, vec3<f32>(0.299, 0.587, 0.114));
    let desaturated = mix(bg_rgb, vec3<f32>(luminance), 0.6);
    let frosted_bg = desaturated * 0.25;

    // Outer card rounded rect - use aspect-corrected SDF for uniform border
    let card_dist = rounded_rect_sdf_aspect(pos, input.corner_radius_normalized, aspect);
    let card_aa = max(1e-3, fwidth(card_dist));
    let card_coverage = 1.0 - smoothstep(0.0, card_aa, card_dist);

    // Start with frosted background
    var final_rgb = frosted_bg;
    var final_alpha = bg_alpha;

    // Get hovered button
    let hovered_btn = get_button_index(mouse);

    // Render each button
    for (var i: i32 = 0; i < NUM_BUTTONS; i = i + 1) {
        let btn_dist = menu_button_sdf(pos, i, aspect);
        let btn_aa = max(1e-3, fwidth(btn_dist));
        let btn_coverage = 1.0 - smoothstep(0.0, btn_aa, btn_dist);

        if btn_coverage > 0.0 {
            // Determine button state (grayed for Watchlist and Edit)
            let is_grayed = (i == BTN_WATCHLIST) || (i == BTN_EDIT);
            let is_hovered = (i == hovered_btn) && !is_grayed;

            // Dark tinted glass button colors
            var btn_base: vec3<f32>;
            if is_grayed {
                // Grayed out: very dark, muted
                btn_base = vec3<f32>(0.08, 0.08, 0.10);
            } else if is_hovered {
                // Hovered: slightly lighter dark glass with subtle theme tint
                let dark_tint = mix(vec3<f32>(0.15, 0.16, 0.18), input.theme_color * 0.3, 0.3);
                btn_base = dark_tint;
            } else {
                // Normal: dark tinted glass
                btn_base = vec3<f32>(0.06, 0.06, 0.08);
            }

            // Glass transparency (higher alpha for more opaque dark glass)
            let glass_alpha = select(0.85, 0.92, is_hovered);
            let grayed_alpha = 0.75;
            let btn_alpha = select(glass_alpha, grayed_alpha, is_grayed);

            // Blend button over background
            final_rgb = mix(final_rgb, btn_base, btn_coverage * btn_alpha);
        }
    }

    // Render icons and labels on top of buttons
    for (var i: i32 = 0; i < NUM_BUTTONS; i = i + 1) {
        let is_grayed = (i == BTN_WATCHLIST) || (i == BTN_EDIT);
        let is_hovered = (i == hovered_btn) && !is_grayed;

        // Darkened accent color for hovered icons/text
        let accent_darkened = input.progress_color * 0.7;

        // Icon (on left side) - pass aspect ratio for proper proportions
        let icon_coverage = render_icon(pos, i, 0.04, aspect);
        if icon_coverage > 0.0 {
            var icon_color: vec3<f32>;
            if is_grayed {
                icon_color = vec3<f32>(0.35, 0.35, 0.38);
            } else if is_hovered {
                icon_color = accent_darkened;
            } else {
                icon_color = vec3<f32>(0.85, 0.87, 0.92);
            }
            final_rgb = mix(final_rgb, icon_color, icon_coverage);
        }

        // Label text (centered) - using font atlas SDF, pass aspect ratio
        let label_coverage = render_atlas_label(pos, i, 0.06, aspect);
        if label_coverage > 0.0 {
            var label_color: vec3<f32>;
            if is_grayed {
                label_color = vec3<f32>(0.35, 0.35, 0.38);
            } else if is_hovered {
                label_color = accent_darkened;
            } else {
                label_color = vec3<f32>(0.8, 0.82, 0.88);
            }
            final_rgb = mix(final_rgb, label_color, label_coverage);
        }
    }

    // Render separator lines between buttons (subtle dark lines)
    for (var i: i32 = 1; i < NUM_BUTTONS; i = i + 1) {
        let sep_y = f32(i) * BUTTON_HEIGHT;
        let sep_dist = abs(pos.y - sep_y);
        let sep_width = 0.002; // Thin separator line
        let sep_coverage = 1.0 - smoothstep(0.0, sep_width, sep_dist);
        if sep_coverage > 0.0 {
            let sep_color = vec3<f32>(0.02, 0.02, 0.03);
            final_rgb = mix(final_rgb, sep_color, sep_coverage * 0.9);
        }
    }

    // Apply card coverage and opacity
    var final_color = to_premul(vec4<f32>(final_rgb, final_alpha)) * (card_coverage * input.opacity);

    // Card outer border (accent color) - thin line exactly at the edge
    let border_aa = max(1e-3, fwidth(card_dist));
    let border_thickness = border_aa * 1.5; // ~1.5 pixels
    // Border is centered on the edge (dist = 0), extending slightly in and out
    let border_alpha = smoothstep(border_thickness, 0.0, abs(card_dist)) * card_coverage;
    if border_alpha > 0.01 {
        let border_rgb = input.progress_color;
        let border_pm = vec4<f32>(border_rgb * border_alpha, border_alpha);
        final_color = over(border_pm, final_color);
    }

    // Un-premultiply and convert color space
    if final_color.a > 0.0001 {
        final_color = vec4<f32>(final_color.rgb / final_color.a, final_color.a);
    }
    if globals.target_is_srgb <= 0.5 {
        final_color = vec4<f32>(linear_to_srgb(final_color.rgb), final_color.a);
    }
    return final_color;
}
