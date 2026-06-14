// Background Shader for Ferrex Media Player
// Creates animated gradients, depth effects, and visual richness

struct Globals {
    // Transform and time
    transform: mat4x4<f32>,         // offset 0, size 64
    time_and_resolution: vec4<f32>, // time, 0, resolution.x, resolution.y (offset 64, size 16)
    scale_and_effect: vec4<f32>,    // scale_factor, effect_type, effect_param1, effect_param2 (offset 80, size 16)

    // Colors
    primary_color: vec4<f32>,       // offset 96, size 16
    secondary_color: vec4<f32>,     // offset 112, size 16

    // Texture and scroll
    texture_params: vec4<f32>,      // texture_aspect, scroll_offset_px, header_offset_px, backdrop_coverage_uv (offset 128, size 16)

    // Content-space offset (logical pixels) used to deterministically anchor high-frequency noise
    // to scrollable content movement (horizontal carousels + vertical lists/grids).
    content_offset_px: vec4<f32>,   // x, y, 0, 0 (offset 144, size 16)

    // Transition colors
    prev_primary_color: vec4<f32>,  // offset 160, size 16
    prev_secondary_color: vec4<f32>,// offset 176, size 16

    // Transition parameters
    transition_params: vec4<f32>,   // transition_progress, backdrop_opacity, backdrop_slide_offset, backdrop_scale (offset 192, size 16)

    // Gradient and depth
    gradient_center: vec4<f32>,     // gradient_center.x, gradient_center.y, 0, 0 (offset 208, size 16)
    depth_params: vec4<f32>,        // region_count, base_depth, shadow_intensity, shadow_distance (offset 224, size 16)
    ambient_light: vec4<f32>,       // light_dir.x, light_dir.y, 0, 0 (offset 240, size 16)

    // Depth regions (up to 4)
    region1_bounds: vec4<f32>,      // x, y, width, height (offset 256, size 16)
    region1_depth_params: vec4<f32>,// depth, edge_transition_type, edge_width, shadow_enabled (offset 272, size 16)
    region1_shadow_params: vec4<f32>,// shadow_intensity, z_order, border_width, border_opacity (offset 288, size 16)
    region1_border_color: vec4<f32>,// r, g, b, a (offset 304, size 16)

    region2_bounds: vec4<f32>,      // (offset 320, size 16)
    region2_depth_params: vec4<f32>,// (offset 336, size 16)
    region2_shadow_params: vec4<f32>,// (offset 352, size 16)
    region2_border_color: vec4<f32>,// (offset 368, size 16)

    region3_bounds: vec4<f32>,      // (offset 384, size 16)
    region3_depth_params: vec4<f32>,// (offset 400, size 16)
    region3_shadow_params: vec4<f32>,// (offset 416, size 16)
    region3_border_color: vec4<f32>,// (offset 432, size 16)

    region4_bounds: vec4<f32>,      // (offset 448, size 16)
    region4_depth_params: vec4<f32>,// (offset 464, size 16)
    region4_shadow_params: vec4<f32>,// (offset 480, size 16)
    region4_border_color: vec4<f32>,// (offset 496, size 16)

    // Total: 512 bytes (32 * 16)
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(1) var texture_sampler: sampler;
@group(1) @binding(2) var backdrop_texture: texture_2d<f32>;

// Generate vertex positions for a full-screen quad
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;

    // Generate quad vertices in clip space (-1 to 1)
    let x = f32(vertex_index & 1u) * 2.0 - 1.0;
    let y = f32((vertex_index >> 1u) & 1u) * 2.0 - 1.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0); // No Y flip
    output.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5); // Flip UV Y for texture sampling

    return output;
}

// Convert sRGB color to linear color space
fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return pow(color, vec3<f32>(2.2));
}

// Convert linear color to sRGB color space
fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return pow(color, vec3<f32>(1.0 / 2.2));
}

// Simple noise function for organic patterns
fn noise2d(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Better hash function for less patterned noise
fn hash2d(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth noise interpolation
fn smooth_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);

    let a = noise2d(i);
    let b = noise2d(i + vec2<f32>(1.0, 0.0));
    let c = noise2d(i + vec2<f32>(0.0, 1.0));
    let d = noise2d(i + vec2<f32>(1.0, 1.0));

    let u = f * f * (3.0 - 2.0 * f);

    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

// Smooth noise interpolation using hash2d (avoids trig in noise2d).
fn smooth_hash_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);

    let a = hash2d(i);
    let b = hash2d(i + vec2<f32>(1.0, 0.0));
    let c = hash2d(i + vec2<f32>(0.0, 1.0));
    let d = hash2d(i + vec2<f32>(1.0, 1.0));

    let u = f * f * (3.0 - 2.0 * f);
    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

// FBM (Fractal Brownian Motion) for more complex noise
fn fbm(p: vec2<f32>, octaves: i32) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var frequency = 1.0;

    for (var i = 0; i < octaves; i++) {
        value += amplitude * smooth_noise(p * frequency);
        frequency *= 2.0;
        amplitude *= 0.5;
    }

    return value;
}

// Linear gradient
fn linear_gradient(uv: vec2<f32>, angle: f32) -> f32 {
    let s = sin(angle);
    let c = cos(angle);
    let rotated = vec2<f32>(uv.x * c - uv.y * s, uv.x * s + uv.y * c);
    return rotated.y;
}

// Radial gradient
fn radial_gradient(uv: vec2<f32>, center: vec2<f32>) -> f32 {
    return length(uv - center);
}

// Transform a color to be suitable as a background
// Minimally adjusts the color to preserve gradient visibility
fn to_background_color(color: vec3<f32>) -> vec3<f32> {
    // Just return the color with minimal adjustment
    // This preserves the color differences for visible gradients
    return color * 0.98; // Almost no darkening to preserve contrast
}

// Check if a point is inside a rounded rectangle
fn in_rounded_rect(p: vec2<f32>, bounds: vec4<f32>, radius: f32) -> f32 {
    let rect_center = bounds.xy + bounds.zw * 0.5;
    let half_size = bounds.zw * 0.5;

    // Transform to local space
    let local_p = abs(p - rect_center);

    // Check if inside the basic rectangle
    if (local_p.x > half_size.x || local_p.y > half_size.y) {
        return 0.0;
    }

    // Check corners
    let corner_dist = length(local_p - half_size + radius);
    if (local_p.x > half_size.x - radius && local_p.y > half_size.y - radius) {
        return smoothstep(radius + 0.5, radius - 0.5, corner_dist); // Tighter transition
    }

    return 1.0;
}

// Calculate soft shadow for a depth region
fn calculate_shadow(p: vec2<f32>, region_bounds: vec4<f32>, depth: f32, corner_radius: f32,
                   shadow_dir: vec2<f32>, blur_radius: f32) -> f32 {
    // Offset position by shadow direction and depth
    let shadow_offset = shadow_dir * depth * 2.0;
    let shadow_p = p - shadow_offset;

    // Calculate shadow with soft edges
    var shadow = 0.0;
    let samples = 5;
    let blur_step = blur_radius / f32(samples);

    for (var i = 0; i < samples; i++) {
        let offset = f32(i) * blur_step;
        let test_p = shadow_p + shadow_dir * offset;
        shadow += in_rounded_rect(test_p, region_bounds, corner_radius);
    }

    return shadow / f32(samples);
}

// Calculate ambient occlusion for corners and edges
fn calculate_ao(p: vec2<f32>, region_bounds: vec4<f32>, corner_radius: f32, ao_radius: f32) -> f32 {
    let rect_center = region_bounds.xy + region_bounds.zw * 0.5;
    let half_size = region_bounds.zw * 0.5;
    let local_p = p - rect_center;

    // Distance to edges
    let edge_dist_x = half_size.x - abs(local_p.x);
    let edge_dist_y = half_size.y - abs(local_p.y);
    let min_edge_dist = min(edge_dist_x, edge_dist_y);

    // Stronger darkening in corners
    let corner_factor = smoothstep(ao_radius * 2.0, 0.0, edge_dist_x) *
                       smoothstep(ao_radius * 2.0, 0.0, edge_dist_y);

    // Edge darkening with sharper transition
    let edge_factor = smoothstep(ao_radius, ao_radius * 0.5, min_edge_dist);

    return 1.0 - (edge_factor * 0.6 + corner_factor * 0.8);
}

// Film grain effect - anchored to content movement
//
// Use content-space position so the grain scrolls with virtual grids.
// We base the sampling on physical pixels (via scale factor)
// so resizing does not re-parameterize the noise.
fn film_grain(content_pos_px: vec2<f32>, intensity: f32) -> f32 {
    // Convert logical pixels (DIPs) to physical pixels for stability across DPI.
    let physical_pos_px = content_pos_px * globals.scale_and_effect.x;

    // Lower cell size = finer grain.
    // For “old TV static” the grain should approach per-physical-pixel frequency.
    let grain_cell_px = 1.5;
    let grain_coord = physical_pos_px / grain_cell_px;

    // Layer multiple octaves for more organic, less patterned result.
    //
    // IMPORTANT: Use smooth (interpolated) value noise instead of hashing the raw
    // coordinate. A discontinuous hash sampled at high frequency will “sparkle”
    // under fractional scroll offsets because the sampling point moves subpixel
    // across sharp discontinuities (temporal aliasing). Interpolated noise
    // makes the field continuous, so scrolling becomes a pure deterministic
    // translation rather than an apparent re-randomization.
    var grain = 0.0;
    grain += smooth_hash_noise(grain_coord) * 0.5;
    grain += smooth_hash_noise(grain_coord * 1.5 + vec2<f32>(17.32, 29.71)) * 0.3;
    grain += smooth_hash_noise(grain_coord * 2.7 + vec2<f32>(43.19, 61.23)) * 0.2;

    // Make grain centered around 0 and apply intensity.
    return (grain - 0.5) * intensity;
}

// ===== Region-Based Depth System =====

// Region information returned by lookup
struct RegionInfo {
    depth: f32,
    shadow_enabled: bool,
    shadow_intensity: f32,
    found: bool,
}

// Check if a point is inside a region
fn is_in_region(p: vec2<f32>, bounds: vec4<f32>) -> bool {
    return p.x >= bounds.x &&
           p.x <= bounds.x + bounds.z &&
           p.y >= bounds.y &&
           p.y <= bounds.y + bounds.w;
}

// Calculate minimum distance from point to region edges
fn distance_to_region_edge(p: vec2<f32>, bounds: vec4<f32>) -> f32 {
    let to_left = p.x - bounds.x;
    let to_right = (bounds.x + bounds.z) - p.x;
    let to_top = p.y - bounds.y;
    let to_bottom = (bounds.y + bounds.w) - p.y;
    return min(min(to_left, to_right), min(to_top, to_bottom));
}

// Apply edge transition based on distance
fn apply_edge_transition(base_depth: f32, region_depth: f32, edge_dist: f32,
                        edge_type: f32, edge_width: f32) -> f32 {
    if (edge_type == 0.0) { // Sharp
        return region_depth;
    } else if (edge_type == 1.0) { // Soft
        if (edge_dist < edge_width) {
            let t = smoothstep(0.0, edge_width, edge_dist);
            return mix(base_depth, region_depth, t);
        }
    } else if (edge_type == 2.0) { // Beveled
        if (edge_dist < edge_width) {
            let t = edge_dist / edge_width;
            return mix(base_depth, region_depth, t);
        }
    }
    return region_depth;
}

// ===== Light-Based Shadow Functions =====

// Calculate shadow intensity based on light direction and depth map
fn calculate_light_shadow(p: vec2<f32>, pixel_depth: f32, light_dir: vec2<f32>,
                         shadow_distance: f32, shadow_intensity: f32) -> f32 {
    // Light direction vector points FROM the light source
    // We trace from current pixel toward the light to see if anything blocks it
    let trace_dir = normalize(light_dir);

    // Check if the current pixel's region receives shadows
    let pixel_region = get_region_at_position(p);
    if (!pixel_region.shadow_enabled) {
        return 0.0; // No shadows for this region
    }

    // Calculate how far we need to trace based on depth and light angle
    let max_trace = shadow_distance;
    let step_size = 2.0; // Sample every 2 pixels for performance

    var shadow = 0.0;

    // Trace toward the light to find occluders between us and the light
    for (var dist = step_size; dist < max_trace; dist += step_size) {
        let sample_pos = p + trace_dir * dist;
        let sample_region = get_region_at_position(sample_pos);

        // Only cast shadows from regions that have shadows enabled
        if (sample_region.found && sample_region.shadow_enabled) {
            let sample_depth = sample_region.depth;

            // If sample is higher (less negative) than current pixel, it blocks light
            if (sample_depth > pixel_depth) {
                let height_diff = sample_depth - pixel_depth;
                let shadow_factor = height_diff * 0.1; // Scale height difference to shadow intensity

                // Soft shadow falloff with distance
                let distance_falloff = 1.0 - (dist / max_trace);
                shadow += shadow_factor * distance_falloff * sample_region.shadow_intensity;
            }
        }
    }

    // Apply global intensity and pixel's region shadow intensity
    return clamp(shadow * shadow_intensity * pixel_region.shadow_intensity, 0.0, 1.0);
}

// Get depth at a specific position by checking all regions
fn get_depth_at_position(p: vec2<f32>) -> f32 {
    let region_count = i32(globals.depth_params.x);
    let base_depth = globals.depth_params.y;

    var final_depth = base_depth;
    var highest_z_order = -1000.0;

    // Check each region
    for (var i = 0; i < region_count; i++) {
        let region_data = get_region_data(i);

        if (is_in_region(p, region_data.bounds)) {
            let z_order = region_data.shadow_params.y;

            if (z_order > highest_z_order) {
                let edge_dist = distance_to_region_edge(p, region_data.bounds);
                final_depth = apply_edge_transition(
                    base_depth,
                    region_data.depth_params.x,
                    edge_dist,
                    region_data.depth_params.y,
                    region_data.depth_params.z
                );
                highest_z_order = z_order;
            }
        }
    }

    return final_depth;
}

// Get complete region information at a position
fn get_region_at_position(p: vec2<f32>) -> RegionInfo {
    let region_count = i32(globals.depth_params.x);
    var info: RegionInfo;
    info.depth = globals.depth_params.y; // base_depth
    info.shadow_enabled = false;
    info.shadow_intensity = 0.0;
    info.found = false;

    var highest_z_order = -1000.0;

    for (var i = 0; i < region_count; i++) {
        let region_data = get_region_data(i);

        if (is_in_region(p, region_data.bounds)) {
            let z_order = region_data.shadow_params.y;

            if (z_order > highest_z_order) {
                info.depth = region_data.depth_params.x;
                info.shadow_enabled = region_data.depth_params.w > 0.5;
                info.shadow_intensity = region_data.shadow_params.x;
                info.found = true;
                highest_z_order = z_order;
            }
        }
    }

    return info;
}

// Helper to get region data by index
struct RegionData {
    bounds: vec4<f32>,
    depth_params: vec4<f32>,
    shadow_params: vec4<f32>,
    border_color: vec4<f32>,
}

fn get_region_data(index: i32) -> RegionData {
    var data: RegionData;

    switch (index) {
        case 0: {
            data.bounds = globals.region1_bounds;
            data.depth_params = globals.region1_depth_params;
            data.shadow_params = globals.region1_shadow_params;
            data.border_color = globals.region1_border_color;
        }
        case 1: {
            data.bounds = globals.region2_bounds;
            data.depth_params = globals.region2_depth_params;
            data.shadow_params = globals.region2_shadow_params;
            data.border_color = globals.region2_border_color;
        }
        case 2: {
            data.bounds = globals.region3_bounds;
            data.depth_params = globals.region3_depth_params;
            data.shadow_params = globals.region3_shadow_params;
            data.border_color = globals.region3_border_color;
        }
        case 3: {
            data.bounds = globals.region4_bounds;
            data.depth_params = globals.region4_depth_params;
            data.shadow_params = globals.region4_shadow_params;
            data.border_color = globals.region4_border_color;
        }
        default: {
            // Return empty data
        }
    }

    return data;
}

// ===== Border and Visual Effects =====

// Get border contribution at a position
fn get_border_contribution(p: vec2<f32>) -> vec4<f32> {
    let region_count = i32(globals.depth_params.x);
    var border_color = vec4<f32>(0.0);

    for (var i = 0; i < region_count; i++) {
        let region_data = get_region_data(i);
        let border_width = region_data.shadow_params.z;
        let border_opacity = region_data.shadow_params.w;

        if (border_width > 0.0 && border_opacity > 0.0) {
            // Check if we're near the edge of this region
            if (is_in_region(p, region_data.bounds)) {
                let edge_dist = distance_to_region_edge(p, region_data.bounds);

                if (edge_dist < border_width) {
                    // Anti-aliased border
                    let alpha = 1.0 - smoothstep(0.0, border_width, edge_dist);
                    let this_border = vec4<f32>(
                        region_data.border_color.rgb,
                        alpha * border_opacity * region_data.border_color.a
                    );

                    // Blend with existing border (take highest alpha)
                    if (this_border.a > border_color.a) {
                        border_color = this_border;
                    }
                }
            }
        }
    }

    return border_color;
}

// Calculate subtle highlight based on surface orientation to light
fn calculate_highlight(pixel_depth: f32, light_dir: vec2<f32>) -> f32 {
    // For a 2D UI, we simulate that surfaces at higher depths
    // are slightly tilted toward the light
    if (pixel_depth > -2.0) { // Only highlight surfaces near or above base level
        // Simple highlight calculation based on depth
        let highlight_factor = (pixel_depth + 5.0) / 10.0; // Normalize depth range
        return clamp(highlight_factor * 0.05, 0.0, 0.1); // Very subtle highlight
    }
    return 0.0;
}


// Main fragment shader
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let time = globals.time_and_resolution.x;
    let resolution = globals.time_and_resolution.zw;
    let aspect = resolution.x / resolution.y;
    let scroll_offset_px = globals.texture_params.y;
    let scroll_offset_uv = scroll_offset_px / resolution.y;
    let content_offset_px = globals.content_offset_px.xy;
    let content_px_pos = vec2<f32>(uv.x * resolution.x, uv.y * resolution.y) + content_offset_px;

    // Transform colors to be background-friendly
    let effect = i32(globals.scale_and_effect.y);
    let primary_bg = to_background_color(globals.primary_color.rgb);
    let secondary_bg = to_background_color(globals.secondary_color.rgb);

    var color = vec3<f32>(0.0);

    // Correct UV for aspect ratio
    var centered_uv = uv - 0.5;
    centered_uv.x *= aspect;

    if (effect == 0) {
        // Solid color
        color = primary_bg;
    } else if (effect == 1) {
        // Always render the radial gradient first
        let gradient_center = globals.gradient_center.xy;
        let dist = length(uv - gradient_center);
        let angle_from_center = atan2(uv.y - gradient_center.y, uv.x - gradient_center.x);
        let wave_distortion = sin(angle_from_center * 4.0 + time * 0.2) * 0.02;
        let gradient_radius = 1.2; // Slightly smaller for more visible gradient
        let t = smoothstep(0.0, gradient_radius, dist + wave_distortion);

        // Base gradient color
        color = mix(primary_bg, secondary_bg, t);

        // Add subtle depth with darker areas near edges
        let edge_fade = 1.0 - smoothstep(0.8, 1.2, length(centered_uv));
        color *= 0.95 + 0.05 * edge_fade;

        // Add a very subtle, scroll-anchored texture layer.
        //
        // We intentionally do not scroll the *entire* gradient, because doing so
        // would push UVs out of the 0..1 range on long scrolls and collapse the
        // gradient into a mostly-flat color. Instead, we keep the base gradient
        // stable and scroll only the high-frequency variation so the background
        // feels attached to the grid under fast scrolling.
        // Sample in pixel space so the texture is anchored to content movement
        // regardless of window aspect ratio.
        let detail_uv = content_px_pos / 96.0;
        let detail = smooth_hash_noise(detail_uv);
        color *= 0.94 + 0.06 * detail;

        // Subtle scroll-anchored grid lines for library-style grid views.
        //
        // We key off the depth region count because Library currently emits a
        // single region layout, while detail views emit multiple regions.
        // This keeps the effect localized without adding new uniforms.
        let region_count = i32(globals.depth_params.x);
        if (region_count == 1) {
            let cell = 160.0;      // DIP cell size (visual only)
            let line_w = 1.0;      // DIP line width

            let fx = fract(content_px_pos.x / cell);
            let fy = fract(content_px_pos.y / cell);

            let dx = min(fx, 1.0 - fx) * cell;
            let dy = min(fy, 1.0 - fy) * cell;

            let line_x = 1.0 - smoothstep(0.0, line_w, dx);
            let line_y = 1.0 - smoothstep(0.0, line_w, dy);

            let line = max(line_x, line_y);
            color *= 1.0 - (line * 0.025);
        }

    } else if (effect == 2) {
        // Subtle noise
        let scale = globals.scale_and_effect.z;
        let speed = globals.scale_and_effect.w;

        // Animated noise
        let noise_uv = centered_uv * scale
            + (content_offset_px / 96.0) * scale
            + vec2<f32>(time * speed * 0.1, 0.0);
        let n = fbm(noise_uv, 4);

        // Blend noise with gradient
        let gradient_t = radial_gradient(centered_uv, vec2<f32>(0.0));
        let base_color = mix(primary_bg, secondary_bg, gradient_t);

        // Add noise as subtle variation
        color = base_color * (0.9 + 0.1 * n);

    } else if (effect == 3) {
        // Floating particles (placeholder for now)
        // This would require more complex particle simulation
        color = mix(primary_bg, secondary_bg, 0.5);

    } else if (effect == 4) {
        // Wave ripple
        let frequency = globals.scale_and_effect.z;
        let amplitude = globals.scale_and_effect.w;

        // Create ripple effect from center
        let dist = length(centered_uv);
        let wave = sin(dist * frequency - time * 2.0) * amplitude;
        let t = saturate(dist + wave);

        color = mix(primary_bg, secondary_bg, t);
    } else if (effect == 5) {
        // Backdrop gradient - render radial gradient first, then overlay backdrop
        let gradient_center = globals.gradient_center.xy;
        let dist = length(uv - gradient_center);
        let gradient_radius = 1.2;
        let t = smoothstep(0.0, gradient_radius, dist);

        // Base gradient color
        color = mix(primary_bg, secondary_bg, t);

        // Now overlay the backdrop texture on top of the gradient
        // Extract actual viewport dimensions from the projection matrix
        let viewport_width = 2.0 / globals.transform[0][0];
        let viewport_height = 2.0 / abs(globals.transform[1][1]);

        // Get aspect mode from scale_and_effect.w (0.0 = auto, 1.0 = force 21:9)
        let aspect_mode = globals.scale_and_effect.w;
        let window_aspect = viewport_width / viewport_height;

        // Determine crop parameters based on mode and window dimensions
        var crop_factor_to_use: f32;
        var crop_bias_to_use: f32;

        if (aspect_mode > 0.5) {
            // Force 21:9 mode
            crop_factor_to_use = 16.0 / 21.0; // 0.762
            crop_bias_to_use = 0.3; // 30% from top
        } else {
            // Auto mode - choose based on window aspect
            if (window_aspect >= 1.0) {
                // Wide window - use 30:9 ultra-wide
                crop_factor_to_use = 16.0 / 30.0; // 0.533
                crop_bias_to_use = 0.05; // Only 5% from top - very aggressive top crop
            } else {
                // Tall window - use 21:9
                crop_factor_to_use = 16.0 / 21.0; // 0.762
                crop_bias_to_use = 0.3; // 30% from top
            }
        }

        // Get header offset from uniform buffer
        let header_offset = globals.texture_params.z;
        let header_offset_uv = header_offset / viewport_height;

        // Use pre-calculated backdrop coverage from Rust (single source of truth)
        let backdrop_screen_coverage = globals.texture_params.w;

        // Calculate visible backdrop height (from header to content start)
        let backdrop_visible_height = (backdrop_screen_coverage - header_offset_uv) * viewport_height;

        // Apply scroll offset (add to make backdrop scroll with content)
        let scroll_offset_uv = globals.texture_params.y / viewport_height;
        let scrolled_uv_y = uv.y + scroll_offset_uv;

        // Check if we're in the backdrop region (accounting for header offset)
        // backdrop_screen_coverage represents the Y position (in UV space) where backdrop ends
        // This is calculated from window top, so the region is [header_offset_uv, backdrop_screen_coverage]
        let backdrop_region_height = backdrop_screen_coverage - header_offset_uv;
        if (scrolled_uv_y >= header_offset_uv && scrolled_uv_y <= backdrop_screen_coverage) {
            // Map screen UV to texture UV for just the backdrop region
            // Map [header_offset_uv, backdrop_screen_coverage] to [0, 1]
            let backdrop_uv_y = (scrolled_uv_y - header_offset_uv) / backdrop_region_height;

            // Calculate aspect ratios
            let texture_aspect = globals.texture_params.x;
            let backdrop_aspect = viewport_width / backdrop_visible_height;

            var texture_uv = vec2<f32>(uv.x, backdrop_uv_y);

            // Apply 16:9 to selected aspect ratio cropping
            // We assume the source is 16:9 and we want to display it as the selected aspect
            let source_aspect = 16.0 / 9.0;  // 1.777...

            // If the texture is indeed 16:9, apply the crop
            if (abs(texture_aspect - source_aspect) < 0.01) {
                // Crop from slightly above center to preserve more of the upper content
                // Movie/TV backdrops typically have subjects and titles in the upper portion
                // Use the dynamically calculated crop factor and bias
                let total_crop = 1.0 - crop_factor_to_use; // Total amount to crop
                let crop_from_top = total_crop * crop_bias_to_use; // Amount to crop from top
                let crop_offset = crop_from_top; // Start sampling from this offset
                texture_uv.y = crop_offset + backdrop_uv_y * crop_factor_to_use;

                // No horizontal adjustment needed - we use full width
            } else {
                // Fallback to original cover behavior if not 16:9
                if (texture_aspect > backdrop_aspect) {
                    // Texture is wider - crop width to maintain aspect ratio
                    let scale = backdrop_aspect / texture_aspect;
                    texture_uv.x = (uv.x - 0.5) * scale + 0.5;
                } else {
                    // Texture is narrower - crop height to maintain aspect ratio
                    let scale = texture_aspect / backdrop_aspect;
                    texture_uv.y = backdrop_uv_y * scale;
                }
            }

            // Sample the backdrop texture and render directly (hard transition, no fade)
            let backdrop_color = textureSample(backdrop_texture, texture_sampler, texture_uv);
            color = backdrop_color.rgb;
        }
    }

    // Apply region-based depth and shadow effects
    let region_count = i32(globals.depth_params.x);
    if (region_count > 0) {
        // Convert UV to pixel position
        let pixel_pos = uv * resolution;

        // Get the depth and region info at this pixel
        let pixel_depth = get_depth_at_position(pixel_pos);
        let pixel_region = get_region_at_position(pixel_pos);

        // Only apply depth effects if we found a region with shadows enabled
        if (pixel_region.found && pixel_region.shadow_enabled) {
            // Apply light-based shading
            let light_dir = globals.ambient_light.xy;
            let shadow_intensity = globals.depth_params.z;
            let shadow_distance = globals.depth_params.w;

            // Base depth shading - surfaces at different depths have different base brightness
            let depth_shading = 1.0 + pixel_depth * 0.02;
            color = color * clamp(depth_shading, 0.8, 1.1);

            // Calculate light-based shadows
            let shadow = calculate_light_shadow(pixel_pos, pixel_depth, light_dir, shadow_distance, shadow_intensity);
            if (shadow > 0.0) {
                // Apply shadow as darkening
                color = color * (1.0 - shadow);
            }

            // Add subtle highlights on raised surfaces
            let highlight = calculate_highlight(pixel_depth, light_dir);
            if (highlight > 0.0) {
                // Apply highlight as brightening
                color = color + vec3<f32>(highlight);
            }
        }

        // Draw region borders
        let border = get_border_contribution(pixel_pos);
        if (border.a > 0.0) {
            // Blend border over the current color
            color = mix(color, border.rgb, border.a);
        }
    }

    // Apply film grain effect (content-anchored so it scrolls with the grid)
    let grain_intensity = 0.015; // Very subtle grain for softer appearance
    let grain = film_grain(content_px_pos, grain_intensity);
    color = color + vec3<f32>(grain);

    // Ensure color stays in valid range
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));

    return vec4<f32>(color, 1.0);
}
