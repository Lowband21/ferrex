// Common WGSL helpers shared by poster shaders
// --------------------------------------------
// Color conversions, premultiplied alpha helpers, SDF utilities, and AA helpers
// used by both front and back poster pipelines.

// Convert sRGB color to linear color space
fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return pow(color, vec3<f32>(2.2));
}

// Convert linear color to sRGB color space
fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return pow(color, vec3<f32>(1.0 / 2.2));
}

fn to_premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn over(top: vec4<f32>, bottom: vec4<f32>) -> vec4<f32> {
    let out_a = top.a + bottom.a * (1.0 - top.a);
    let out_rgb = top.rgb + bottom.rgb * (1.0 - top.a);
    return vec4<f32>(out_rgb, out_a);
}

// Signed distance function for rounded rectangle in normalized coordinates (0-1)
fn rounded_rect_sdf_normalized(p: vec2<f32>, radius_normalized: f32) -> f32 {
    // p is in range 0-1, with (0.5, 0.5) at center
    let d = abs(p - vec2<f32>(0.5)) - vec2<f32>(0.5 - radius_normalized);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius_normalized;
}

// Create shadow effect based on depth and position
fn apply_shadow(color: vec3<f32>, local_pos: vec2<f32>, shadow_intensity: f32, z_depth: f32) -> vec3<f32> {
    // Shadow is darker at the bottom and lighter at the top
    let shadow_gradient = 1.0 - (local_pos.y * 0.3);

    // Shadow gets stronger with positive z-depth (coming toward viewer)
    let depth_factor = smoothstep(-10.0, 15.0, z_depth);

    // Calculate shadow darkening
    let shadow_amount = shadow_intensity * shadow_gradient * depth_factor * 0.4;

    // Apply shadow by darkening the color
    return color * (1.0 - shadow_amount);
}

// Create inner shadow for sunken effect
fn apply_inner_shadow(color: vec3<f32>, local_pos: vec2<f32>, z_depth: f32) -> vec3<f32> {
    // Only apply when z_depth is negative (sunken)
    if z_depth >= 0.0 {
        return color;
    }

    // Distance from edges
    let edge_dist = min(
        min(local_pos.x, 1.0 - local_pos.x),
        min(local_pos.y, 1.0 - local_pos.y)
    );

    // Inner shadow intensity based on edge distance
    let shadow_intensity = 1.0 - smoothstep(0.0, 0.15, edge_dist);
    let depth_factor = abs(z_depth) / 10.0;

    return color * (1.0 - shadow_intensity * depth_factor * 0.5);
}

// Apply border glow effect
fn apply_border_glow(color: vec4<f32>, dist_normalized: f32, glow_intensity: f32) -> vec4<f32> {
    if glow_intensity <= 0.0 {
        return color;
    }

    // Create glow around the border
    let glow_dist = abs(dist_normalized);
    // Use cheaper approximation instead of exp()
    // exp(-x*3) ≈ 1/(1 + 3x + 4.5x²) for small x
    let glow_factor = glow_dist * 3.0;
    let glow = glow_intensity / (1.0 + glow_factor + glow_factor * glow_factor * 1.5);

    // Add subtle blue-white glow
    let glow_color = vec3<f32>(0.8, 0.9, 1.0);

    return vec4<f32>(
        mix(color.rgb, glow_color, glow * 0.3),
        color.a
    );
}

// Render drop shadow
fn render_drop_shadow(tex_coord: vec2<f32>, radius_normalized: f32, shadow_intensity: f32, z_depth: f32) -> vec4<f32> {
    // Shadow offset based on z-depth
    let shadow_offset = vec2<f32>(0.02, 0.03) * z_depth / 10.0;
    let shadow_coord = tex_coord - shadow_offset;

    // Calculate distance from shadow shape
    let shadow_dist = rounded_rect_sdf_normalized(shadow_coord, radius_normalized);

    // Shadow blur based on z-depth
    let blur_radius = 0.05 * (1.0 + z_depth / 10.0);
    let shadow_alpha = 1.0 - smoothstep(-blur_radius, blur_radius, shadow_dist);

    // Shadow color (dark with transparency)
    let shadow_color = vec3<f32>(0.0, 0.0, 0.0);
    return vec4<f32>(shadow_color, shadow_alpha * shadow_intensity * 0.5);
}

// SDF for a circle that accounts for aspect ratio
fn circle_sdf(p: vec2<f32>, center: vec2<f32>, radius: f32) -> f32 {
    // Standard poster aspect ratio is 2:3 (width:height)
    let aspect_ratio = 2.0 / 3.0;
    // Adjust x coordinate to compensate for aspect ratio
    let adjusted_p = vec2<f32>(p.x * aspect_ratio, p.y);
    let adjusted_center = vec2<f32>(center.x * aspect_ratio, center.y);
    return length(adjusted_p - adjusted_center) - radius;
}

// SDF for a rounded rectangle button that accounts for aspect ratio
fn button_sdf(p: vec2<f32>, center: vec2<f32>, size: vec2<f32>, corner_radius: f32) -> f32 {
    // Standard poster aspect ratio is 2:3 (width:height)
    let aspect_ratio = 2.0 / 3.0;
    // Adjust coordinates and size to compensate for aspect ratio
    let adjusted_p = vec2<f32>(p.x * aspect_ratio, p.y);
    let adjusted_center = vec2<f32>(center.x * aspect_ratio, center.y);
    let adjusted_size = vec2<f32>(size.x * aspect_ratio, size.y);

    let half_size = adjusted_size * 0.5;
    let d = abs(adjusted_p - adjusted_center) - half_size + corner_radius;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - corner_radius;
}

// SDF for a square button that maintains aspect ratio correctly
fn square_button_sdf(p: vec2<f32>, center: vec2<f32>, size: f32, corner_radius: f32) -> f32 {
    // Standard poster aspect ratio is 2:3 (width:height)
    let aspect_ratio = 2.0 / 3.0;
    // To create a square button in screen space, we need to make it rectangular in texture space
    // The button should be wider in texture coordinates to compensate for the poster's aspect ratio
    let adjusted_p = vec2<f32>(p.x, p.y);
    let adjusted_center = vec2<f32>(center.x, center.y);
    // Make the button wider in texture space so it appears square in screen space
    let adjusted_size = vec2<f32>(size / aspect_ratio, size);

    let half_size = adjusted_size * 0.5;
    let d = abs(adjusted_p - adjusted_center) - half_size + corner_radius;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - corner_radius;
}

// SDF for play triangle icon (symmetric triangle)
fn play_icon_sdf(p: vec2<f32>, center: vec2<f32>, size: f32) -> f32 {
    // For play icons inside square buttons, we don't need aspect ratio correction
    // The button itself handles the aspect ratio
    let rel_p = p - center;
    let half_size = size * 0.5;

    // Triangle vertices (pointing right) - symmetric triangle
    let v1 = vec2<f32>(-half_size * 0.5, -half_size * 0.866); // 60 degree triangle
    let v2 = vec2<f32>(-half_size * 0.5, half_size * 0.866);
    let v3 = vec2<f32>(half_size, 0.0);

    // SDF for triangle using edge distances
    let e1 = v2 - v1;
    let e2 = v3 - v2;
    let e3 = v1 - v3;

    let d1 = dot(rel_p - v1, vec2<f32>(-e1.y, e1.x)) / length(e1);
    let d2 = dot(rel_p - v2, vec2<f32>(-e2.y, e2.x)) / length(e2);
    let d3 = dot(rel_p - v3, vec2<f32>(-e3.y, e3.x)) / length(e3);

    return max(max(d1, d2), d3);
}

// SDF for edit pencil icon with aspect ratio correction
fn edit_icon_sdf(p: vec2<f32>, center: vec2<f32>, size: f32) -> f32 {
    // Standard poster aspect ratio is 2:3 (width:height)
    let aspect_ratio = 2.0 / 3.0;

    // Adjust coordinates to compensate for aspect ratio
    let adjusted_p = vec2<f32>(p.x * aspect_ratio, p.y);
    let adjusted_center = vec2<f32>(center.x * aspect_ratio, center.y);

    let rel_p = adjusted_p - adjusted_center;
    let half_size = size * 0.5;

    // Simple pencil shape: rotated rectangle
    let rotated_p = vec2<f32>(
        rel_p.x * 0.707 + rel_p.y * 0.707,
        -rel_p.x * 0.707 + rel_p.y * 0.707
    );

    let rect_size = vec2<f32>(half_size * 0.3 * aspect_ratio, half_size * 1.5);
    let d = abs(rotated_p) - rect_size;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// SDF for three dots (more options icon)
fn dots_icon_sdf(p: vec2<f32>, center: vec2<f32>, size: f32) -> f32 {
    let dot_radius = size * 0.15;
    let spacing = size * 0.45; // Increased spacing between dots

    // Note: circle_sdf already handles aspect ratio internally
    let d1 = circle_sdf(p, center + vec2<f32>(-spacing, 0.0), dot_radius);
    let d2 = circle_sdf(p, center, dot_radius);
    let d3 = circle_sdf(p, center + vec2<f32>(spacing, 0.0), dot_radius);

    return min(min(d1, d2), d3);
}
