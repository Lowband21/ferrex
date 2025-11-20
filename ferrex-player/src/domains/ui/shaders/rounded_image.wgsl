// Rounded Image Shader for Iced
// Provides GPU-accelerated rounded rectangle clipping with anti-aliasing

struct Globals {
    transform: mat4x4<f32>,  // 64 bytes
    scale_factor: f32,       // 4 bytes
    _padding1: f32,          // 4 bytes padding
    _padding2: f32,          // 4 bytes padding
    _padding3: f32,          // 4 bytes padding
    _padding4: vec4<f32>,    // 16 bytes padding - total struct is 96 bytes
}


struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    // Instance attributes - packed into vec4s to reduce attribute count
    @location(0) position_and_size: vec4<f32>,           // position.xy, size.xy
    @location(1) radius_opacity_rotation_anim: vec4<f32>, // radius, opacity, rotation_y, animation_progress
    @location(2) theme_color_zdepth: vec4<f32>,          // theme_color.rgb, z_depth
    @location(3) scale_shadow_glow_type: vec4<f32>,      // scale, shadow_intensity, border_glow, animation_type
    @location(4) hover_overlay_border_progress: vec4<f32>, // is_hovered, show_overlay, show_border, progress
    @location(5) mouse_pos_and_padding: vec4<f32>,       // mouse_position.xy, unused, unused
    @location(6) progress_color_and_padding: vec4<f32>,  // progress_color.rgb, unused
    @location(7) atlas_uvs: vec4<f32>,                   // atlas_uv_min.xy, atlas_uv_max.xy
    @location(8) atlas_layer_and_padding: vec4<f32>,     // atlas_layer, unused, unused, unused
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,          // Texture coordinates in atlas space
    @location(1) corner_radius_normalized: f32, // Corner radius as fraction of size
    @location(2) opacity: f32,                  // Pass opacity to fragment shader
    @location(3) is_backface: f32,              // 1.0 if showing back of card, 0.0 if front
    @location(4) theme_color: vec3<f32>,         // Theme color for backface
    @location(5) shadow_params: vec4<f32>,      // shadow_intensity, z_depth, scale, border_glow
    @location(6) local_pos: vec2<f32>,          // Position within poster for effects (widget space 0-1)
    @location(7) animation_type: f32,           // Animation type for shader-specific effects
    @location(8) animation_progress: f32,        // Animation progress for debug visualization
    @location(9) hover_overlay_params: vec3<f32>, // is_hovered, show_overlay, show_border
    @location(10) mouse_position: vec2<f32>,    // Mouse position (normalized 0-1)
    @location(11) progress: f32,                // Progress percentage
    @location(12) progress_color: vec3<f32>,    // Progress bar color
    @location(13) atlas_layer: f32,             // Atlas texture array layer
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(1) @binding(0) var atlas_texture: texture_2d_array<f32>;

// Generate vertex positions and texture coordinates for a quad
fn vertex_position(vertex_index: u32) -> vec2<f32> {
    // Triangle strip positions for a quad:
    // 0: top-left, 1: top-right, 2: bottom-left, 3: bottom-right
    let x = f32(vertex_index & 1u);
    let y = f32((vertex_index >> 1u) & 1u);
    return vec2<f32>(x, y);
}


// Apply 3D rotation around Y-axis for flip animation (front face only)
// Note: This is only called for rotation_y from 0 to PI/2 (front face)
// The backface uses a cheaper squeeze effect instead
fn apply_flip_rotation(pos: vec2<f32>, center: vec2<f32>, rotation_y: f32) -> vec3<f32> {
    // Translate to origin (center of image)
    let translated = pos - center;

    // Use higher precision trig calculations
    // Add small smoothing to rotation for less jumpy animation
    let smoothed_rotation = rotation_y;
    let cos_theta = cos(smoothed_rotation);
    let sin_theta = sin(smoothed_rotation);

    // Rotate around Y-axis with higher precision
    // X coordinate rotates, Y stays fixed
    let rotated_x = translated.x * cos_theta;
    let rotated_z = translated.x * sin_theta;

    // Return 3D position (add back center x, keep y)
    return vec3<f32>(rotated_x + center.x, pos.y, rotated_z);
}

// Apply 3D transformations including z-depth and scale
fn apply_3d_transform(pos: vec2<f32>, center: vec2<f32>, z_depth: f32, scale: f32) -> vec2<f32> {
    // Apply scale from center
    let scaled = center + (pos - center) * scale;

    // Apply perspective based on z-depth
    // Positive z comes toward viewer, negative z goes away
    let perspective_factor = 1.0 / (1.0 - z_depth * 0.001);
    let perspectived = center + (scaled - center) * perspective_factor;

    return perspectived;
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Unpack vec4 attributes
    let position = input.position_and_size.xy;
    let size = input.position_and_size.zw;
    let radius = input.radius_opacity_rotation_anim.x;
    let opacity = input.radius_opacity_rotation_anim.y;
    let rotation_y = input.radius_opacity_rotation_anim.z;
    let animation_progress = input.radius_opacity_rotation_anim.w;
    let theme_color = input.theme_color_zdepth.xyz;
    let z_depth = input.theme_color_zdepth.w;
    let scale = input.scale_shadow_glow_type.x;
    let shadow_intensity = input.scale_shadow_glow_type.y;
    let border_glow = input.scale_shadow_glow_type.z;
    let animation_type = input.scale_shadow_glow_type.w;
    let is_hovered = input.hover_overlay_border_progress.x;
    let show_overlay = input.hover_overlay_border_progress.y;
    let show_border = input.hover_overlay_border_progress.z;
    let progress = input.hover_overlay_border_progress.w;
    let mouse_position = input.mouse_pos_and_padding.xy;
    let progress_color = input.progress_color_and_padding.xyz;
    let atlas_uv_min = input.atlas_uvs.xy;
    let atlas_uv_max = input.atlas_uvs.zw;
    let atlas_layer = input.atlas_layer_and_padding.x;

    // Generate quad vertex position (0,0) to (1,1)
    let vertex_pos = vertex_position(input.vertex_index);

    // Calculate position within bounds using instance data
    // These positions are in logical pixels
    let position_final = position + vertex_pos * size;
    let center = position + size * 0.5;

    // Apply 3D transformations
    var transformed_pos: vec2<f32>;

    // First apply scale and z-depth transformations
    transformed_pos = apply_3d_transform(position_final, center, z_depth, scale);

    // Flip effect
    if abs(rotation_y) > 0.00001 {
        let pi = 3.14159265359;
        let pi_half = 1.5708;

        // Normalize rotation to [0, PI] range for consistent behavior
        let norm_rotation = rotation_y - floor(rotation_y / pi) * pi;

        // Smooth transition function that peaks at PI/2
        // This creates a natural "squeeze" effect at the flip point
        let flip_progress = sin(norm_rotation);

        // Scale X based on the cosine of rotation
        // This naturally goes to 0 at PI/2 and back to 1
        let scale_x = abs(cos(norm_rotation));

        // Apply the transformation
        transformed_pos.x = center.x + (transformed_pos.x - center.x) * scale_x;

        // Optional: Add subtle perspective for more realism
        // The perspective effect is proportional to the flip progress
        let perspective_offset = flip_progress * 0.001;
        let z_depth = sin(norm_rotation) * 100.0; // Depth for perspective
        let perspective_scale = 1.0 / (1.0 + z_depth * perspective_offset);

        transformed_pos.x = center.x + (transformed_pos.x - center.x) * scale_x * perspective_scale;
        transformed_pos.y = center.y + (transformed_pos.y - center.y) * perspective_scale;
    }

    // Extract viewport dimensions from the projection matrix scale
    let viewport_width = 2.0 / globals.transform[0][0];
    let viewport_height = 2.0 / abs(globals.transform[1][1]);

    // Convert logical positions to physical pixels
    let physical_pos = transformed_pos * globals.scale_factor;

    // Apply standard orthographic projection
    // Map [0, viewport_width] to [-1, 1] and [0, viewport_height] to [1, -1] (Y is flipped)
    let clip_x = (physical_pos.x / viewport_width) * 2.0 - 1.0;
    let clip_y = 1.0 - (physical_pos.y / viewport_height) * 2.0;

    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);

    // Calculate texture coordinates in atlas space
    // vertex_pos is 0-1 in widget space, map to atlas UV coordinates
    let is_backface = select(0.0, 1.0, rotation_y >= 1.5708); // >= PI/2 (90 degrees), branchless
    let widget_u = mix(vertex_pos.x, 1.0 - vertex_pos.x, is_backface);
    let widget_v = vertex_pos.y;

    // Map widget UVs (0-1) to atlas UVs
    output.tex_coord = vec2<f32>(
        mix(atlas_uv_min.x, atlas_uv_max.x, widget_u),
        mix(atlas_uv_min.y, atlas_uv_max.y, widget_v)
    );

    // Pass normalized corner radius (as fraction of smaller dimension)
    let min_dimension = min(size.x, size.y);
    output.corner_radius_normalized = radius / min_dimension;
    output.opacity = opacity;
    output.is_backface = is_backface;
    output.theme_color = theme_color;

    // Pack shadow parameters
    output.shadow_params = vec4<f32>(shadow_intensity, z_depth, scale, border_glow);

    // Local position for effects (normalized 0-1 within poster)
    output.local_pos = vertex_pos;

    // Pass animation type and progress
    output.animation_type = animation_type;
    output.animation_progress = animation_progress;

    // Pass hover and overlay parameters
    output.hover_overlay_params = vec3<f32>(is_hovered, show_overlay, show_border);

    // Pass mouse position
    output.mouse_position = mouse_position;

    // Pass progress data
    output.progress = progress;
    output.progress_color = progress_color;

    // Pass atlas layer
    output.atlas_layer = atlas_layer;

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

// Render overlay buttons with hover states
fn render_overlay_buttons(tex_coord: vec2<f32>, hover_dist: f32, mouse_pos: vec2<f32>, theme_color: vec3<f32>, progress_color: vec3<f32>) -> vec4<f32> {
    // Only render inside poster bounds
    if hover_dist >= 0.0 {
        return vec4<f32>(0.0);
    }

    // Check if mouse is valid (not -1, -1)
    let has_mouse = mouse_pos.x >= 0.0 && mouse_pos.y >= 0.0;

    var button_color = vec4<f32>(0.0);

    // Center play button - using circle for cleaner appearance
    let center_button_pos = vec2<f32>(0.5, 0.5);
    let center_button_radius = 0.08; // 8% of poster size (circle radius)
    let center_button_dist = circle_sdf(tex_coord, center_button_pos, center_button_radius);

    // Check if mouse is over center button
    let center_hover = has_mouse && circle_sdf(mouse_pos, center_button_pos, center_button_radius) < 0.0;

    // For circles, we need proper anti-aliasing
    let aa_width = max(1e-3, fwidth(center_button_dist));

    if center_button_dist < -aa_width {
        // Fully inside button
        if center_hover {
            // Hovered state - clean solid color, no border
            button_color = vec4<f32>(progress_color, 1.0);
        } else {
            // Not hovered - transparent grey with white border
            let border_thickness = 0.004; // Slightly thicker border for visibility
            let inner_radius = center_button_radius - border_thickness;
            let inner_dist = circle_sdf(tex_coord, center_button_pos, inner_radius);

            if inner_dist < -aa_width {
                // Inside transparent grey area
                button_color = vec4<f32>(0.0, 0.0, 0.0, 0.6);
            } else if inner_dist < aa_width {
                // Anti-aliased edge between grey and white border
                let t = smoothstep(aa_width, -aa_width, inner_dist);
                button_color = mix(vec4<f32>(1.0, 1.0, 1.0, 1.0), vec4<f32>(0.0, 0.0, 0.0, 0.6), t);
            } else {
                // White border area
                button_color = vec4<f32>(1.0, 1.0, 1.0, 1.0);
            }
        }
    } else if center_button_dist < aa_width {
        // Anti-aliased outer edge
        let t = smoothstep(aa_width, -aa_width, center_button_dist);
        if center_hover {
            // Hovered - fade out solid color
            button_color = vec4<f32>(progress_color, t);
        } else {
            // Not hovered - fade out white border
            button_color = vec4<f32>(1.0, 1.0, 1.0, t);
        }
    }

    // Play icon - render on top of button background
    if center_button_dist < 0.0 {
        let play_icon_size = center_button_radius * 0.7; // Icon is 70% of button radius
        let play_dist = play_icon_sdf(tex_coord, center_button_pos, play_icon_size);
        if play_dist < 0.0 {
            let icon_color = select(
                vec4<f32>(1.0, 1.0, 1.0, 1.0), // White when not hovered
                vec4<f32>(0.0, 0.0, 0.0, 1.0), // Black when hovered
                center_hover
            );
            let play_aa = max(1e-3, fwidth(play_dist));
            let icon_alpha = smoothstep(play_aa, -play_aa, play_dist);
            button_color = mix(button_color, icon_color, icon_alpha);
        }
    }

    // Top-right edit button (icon only, no background)
    let edit_button_pos = vec2<f32>(0.85, 0.15);
    let edit_base_radius = 0.06;
    // Check hover at full size for consistent hit area
    let edit_hover = has_mouse && circle_sdf(mouse_pos, edit_button_pos, edit_base_radius) < 0.0;

    // Scale and opacity based on hover - larger base size
    let edit_scale = select(0.9, 1.1, edit_hover); // 90% when not hovered, 110% when hovered
    let edit_opacity = select(0.7, 1.0, edit_hover); // 70% opacity when not hovered

    // Edit icon only - no background
    let edit_icon_scale = 0.045 * edit_scale; // Slightly larger base size
    let edit_dist = edit_icon_sdf(tex_coord, edit_button_pos, edit_icon_scale);
    if edit_dist < 0.0 {
        let edit_aa = max(1e-3, fwidth(edit_dist));
        let icon_alpha = smoothstep(edit_aa, -edit_aa, edit_dist) * edit_opacity;
        let icon_color = vec4<f32>(1.0, 1.0, 1.0, icon_alpha);
        button_color = mix(button_color, icon_color, icon_color.a);
    }

    // Bottom-right more options button (icon only, no background)
    let dots_button_pos = vec2<f32>(0.85, 0.85);
    let dots_base_radius = 0.06;
    // Check hover at full size for consistent hit area
    let dots_hover = has_mouse && circle_sdf(mouse_pos, dots_button_pos, dots_base_radius) < 0.0;

    // Scale and opacity based on hover - larger base size
    let dots_scale = select(0.9, 1.1, dots_hover); // 90% when not hovered, 110% when hovered
    let dots_opacity = select(0.7, 1.0, dots_hover); // 70% opacity when not hovered

    // Dots icon only - no background
    let dots_icon_scale = 0.045 * dots_scale; // Slightly larger base size
    let dots_dist = dots_icon_sdf(tex_coord, dots_button_pos, dots_icon_scale);
    if dots_dist < 0.0 {
        let dots_aa = max(1e-3, fwidth(dots_dist));
        let icon_alpha = smoothstep(dots_aa, -dots_aa, dots_dist) * dots_opacity;
        let icon_color = vec4<f32>(1.0, 1.0, 1.0, icon_alpha);
        button_color = mix(button_color, icon_color, icon_color.a);
    }

    return button_color;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var linear_rgb: vec3<f32>;
    var alpha: f32 = 1.0;

    // Extract shadow parameters
    let shadow_intensity = input.shadow_params.x;
    let z_depth = input.shadow_params.y;
    let scale = input.shadow_params.z;
    let border_glow = input.shadow_params.w;

    // Extract hover state for early-out optimization
    let is_hovered = input.hover_overlay_params.x;
    let show_overlay = input.hover_overlay_params.y;
    let is_animating = input.animation_progress < 0.99;

    // For backface, use theme color with stronger dimming
    if input.is_backface > 0.5 {
        linear_rgb = input.theme_color * 0.5;
    } else {
        // Front face: build cross-fade from dimmed placeholder to texture
        // Invalid UVs are set to a tiny range (0.001, 0.001) when image fails to load
        let uv_range = length(input.tex_coord);
        let placeholder_rgb = input.theme_color * 0.5;
        if uv_range < 0.01 {  // Detect invalid/fallback UVs
        // No valid texture, use placeholder only
        linear_rgb = placeholder_rgb;
        alpha = 1.0;
        } else {
            // Valid UVs - sample the poster texture from atlas
            let sampled_color = textureSample(
                atlas_texture,
                atlas_sampler,
                input.tex_coord,
                i32(input.atlas_layer)
            );
            // Atlas uses Rgba8UnormSrgb: GPU converts sRGB to linear when sampling
            linear_rgb = sampled_color.rgb;
            alpha = sampled_color.a;
        }
    }

    // Calculate SDF and AA coverage once for reuse
    let dist = rounded_rect_sdf_normalized(input.local_pos, input.corner_radius_normalized);
    let aa = max(1e-3, fwidth(dist));
    let coverage = 1.0 - smoothstep(0.0, aa, dist);

    // FAST PATH: Skip expensive calculations for non-hovered, non-animating posters
    // But still render progress indicators and borders
    let needs_full_render = show_overlay > 0.5 || is_animating || abs(z_depth) > 0.01 || shadow_intensity > 0.01;

    if !needs_full_render {
        // Base color in pre-multiplied space with coverage and opacity
        var final_color = to_premul(vec4<f32>(linear_rgb, alpha)) * (coverage * input.opacity);

        // Pixel-accurate inside-only border using SDF-aware fwidth
        {
            let d = dist;
            let d_aa = max(1e-3, fwidth(d));
            let border_px = select(1.2, 1.6, is_hovered > 0.5);
            let w = border_px * d_aa;

            let inner = smoothstep(-w, -w + d_aa, d);
            let edge = 1.0 - smoothstep(0.0, d_aa, d);
            let border_alpha = clamp(min(inner, edge), 0.0, 1.0);

            if border_alpha > 0.0 {
                let border_rgb = select(vec3<f32>(0.0, 0.0, 0.0), input.progress_color, is_hovered > 0.5);
                let border_pm = vec4<f32>(border_rgb * border_alpha, border_alpha);
                final_color = over(border_pm, final_color);
            }
        }

        // Watch status corner indicator (top-right) after animation completes
        if input.progress >= 0.0 && dist < 0.0 && input.animation_progress >= 0.99 {
            let aspect_ratio = 2.0 / 3.0;
            let fold_size_x = input.corner_radius_normalized * 2.5;
            let fold_size_y = fold_size_x * aspect_ratio;
            let corner_origin = vec2<f32>(1.0, 0.0);
            let rel_pos = input.local_pos - corner_origin;

            let normalized_x = rel_pos.x / fold_size_x;
            let normalized_y = rel_pos.y / fold_size_y;
            let in_triangle = rel_pos.x <= 0.0 && rel_pos.y >= 0.0 && (normalized_y - normalized_x) <= 1.0;

            if in_triangle {
                let a = 1.0 / fold_size_x;
                let b = -1.0 / fold_size_y;
                let c = 1.0;
                let diagonal_dist = abs(a * rel_pos.x + b * rel_pos.y + c) / sqrt(a*a + b*b);
                let triangle_alpha = smoothstep(-0.005, 0.005, diagonal_dist);

                var indicator_opacity = select(0.85, 0.6, input.progress > 0.0);
                indicator_opacity = select(indicator_opacity, 0.2, input.progress >= 0.95);

                let ia = indicator_opacity * triangle_alpha;
                let indicator_pm = vec4<f32>(input.progress_color * ia, ia);
                final_color = over(indicator_pm, final_color);
            }
        }

        // Progress bar at bottom after animation completes
        if input.progress > 0.0 && input.progress < 0.95 && input.animation_progress >= 0.99 {
            let bar_start_y = 1.0 - 0.03;
            if input.local_pos.y >= bar_start_y && dist < 0.0 {
                let edge_fade = smoothstep(0.0, -0.01, dist);
                if input.local_pos.x <= input.progress {
                    let a = 0.8 * edge_fade;
                    let bar_pm = vec4<f32>(input.progress_color * a, a);
                    final_color = over(bar_pm, final_color);
                } else {
                    let a = 0.4 * edge_fade;
                    let bg_pm = vec4<f32>(vec3<f32>(0.0), a);
                    final_color = over(bg_pm, final_color);
                }
            }
        }
        return final_color;
    }

    // Apply shadow effects only when actually visible
    if shadow_intensity > 0.01 && abs(z_depth) > 0.01 {
        linear_rgb = apply_shadow(linear_rgb, input.local_pos, shadow_intensity, z_depth);
        // Only check inner shadow for negative z_depth
        if z_depth < -0.01 {
            linear_rgb = apply_inner_shadow(linear_rgb, input.local_pos, z_depth);
        }
    }

    // Build base pre-multiplied color with coverage and opacity
    var final_color = to_premul(vec4<f32>(linear_rgb, alpha)) * (coverage * input.opacity);

    // Apply border glow as a premultiplied overlay near the edge
    if border_glow > 0.0 {
        let glow_dist = abs(dist);
        let glow_edge = 1.5 * aa;
        let glow_t = 1.0 - smoothstep(0.0, glow_edge, glow_dist);
        let glow_alpha = glow_t * border_glow * 0.3;
        if glow_alpha > 0.0 {
            let glow_color = vec3<f32>(0.8, 0.9, 1.0);
            let glow_pm = vec4<f32>(glow_color * glow_alpha, glow_alpha);
            final_color = over(glow_pm, final_color);
        }
    }

    // Render drop shadow behind when significantly elevated
    if z_depth > 1.0 && shadow_intensity > 0.1 {
        let shadow = render_drop_shadow(input.local_pos, input.corner_radius_normalized, shadow_intensity, z_depth);
        let shadow_pm = to_premul(shadow);
        final_color = over(final_color, shadow_pm);
    }

    // Extract remaining hover parameters (show_overlay and is_hovered already extracted above)
    let show_border = input.hover_overlay_params.z;

    // Render overlay when hovered and animation complete
    if show_overlay > 0.5 {
        let hover_dist = dist;

        // Dark tinted background (only inside poster bounds); preserve alpha
        if hover_dist < 0.0 {
            let overlay_alpha = 0.5;
            let darken_factor = 1.0 - overlay_alpha;
            final_color.r *= darken_factor;
            final_color.g *= darken_factor;
            final_color.b *= darken_factor;
            // final_color.a unchanged
        }

        // Overlay buttons composed in pre-multiplied space
        let button_color = render_overlay_buttons(input.local_pos, hover_dist, input.mouse_position, input.theme_color, input.progress_color);
        if button_color.a > 0.0 {
            let button_pm = to_premul(button_color);
            final_color = over(button_pm, final_color);
        }
    }

    // Always render border - pixel-accurate, inside-only
    {
        let d = dist;
        let d_aa = max(1e-3, fwidth(d));
        let border_px = select(1.2, 1.6, is_hovered > 0.5);
        let w = border_px * d_aa;

        let inner = smoothstep(-w, -w + d_aa, d);
        let edge = 1.0 - smoothstep(0.0, d_aa, d);
        let border_alpha = clamp(min(inner, edge), 0.0, 1.0);

        if border_alpha > 0.0 {
            let border_rgb = select(vec3<f32>(0.0, 0.0, 0.0), input.progress_color, is_hovered > 0.5);
            let border_pm = vec4<f32>(border_rgb * border_alpha, border_alpha);
            final_color = over(border_pm, final_color);
        }
    }

    // Render watch status corner indicator if progress is valid and animation is complete
    if input.progress >= 0.0 && input.animation_progress >= 0.99 {
        let aspect_ratio = 2.0 / 3.0;

        // Create a triangular indicator shaped like a folded corner
        let fold_size_x = input.corner_radius_normalized * 2.5;
        let fold_size_y = fold_size_x * aspect_ratio;

        // Top-right corner origin
        let corner_origin = vec2<f32>(1.0, 0.0);
        let rel_pos = input.local_pos - corner_origin;

        // Triangle check in normalized coordinates
        let normalized_x = rel_pos.x / fold_size_x;
        let normalized_y = rel_pos.y / fold_size_y;
        let in_triangle = rel_pos.x <= 0.0 &&
                          rel_pos.y >= 0.0 &&
                          (normalized_y - normalized_x) <= 1.0;

        if in_triangle && dist < 0.0 {
            // Distance to diagonal edge for AA
            let inv_fold_x = 1.0 / fold_size_x;
            let inv_fold_y = 1.0 / fold_size_y;
            let a = inv_fold_x;
            let b = -inv_fold_y;
            let c = 1.0;
            let inv_norm = 1.0 / sqrt(a*a + b*b);
            let diagonal_dist = abs(a * rel_pos.x + b * rel_pos.y + c) * inv_norm;
            let edge_softness = 0.005;
            let triangle_alpha = smoothstep(-edge_softness, edge_softness, diagonal_dist);

            var indicator_opacity = select(0.85, 0.6, input.progress > 0.0);
            indicator_opacity = select(indicator_opacity, 0.2, input.progress >= 0.95);

            let ia = indicator_opacity * triangle_alpha;
            let indicator_pm = vec4<f32>(input.progress_color * ia, ia);
            final_color = over(indicator_pm, final_color);
        }
    }

    // Render progress bar at bottom for in-progress media (after animation completes)
    if input.progress > 0.0 && input.progress < 0.95 && input.animation_progress >= 0.99 {
        // Progress bar dimensions
        let bar_height = 0.02; // 2% of poster height
        let bar_margin = 0.01; // 1% margin from bottom

        // Check if we're in the progress bar area (use widget space)
        if input.local_pos.y > (1.0 - bar_height - bar_margin) {
            // Calculate progress bar boundaries
            let bar_start_y = 1.0 - bar_height - bar_margin;
            let bar_end_y = 1.0 - bar_margin;

            // Check if we're within the bar height
            if input.local_pos.y >= bar_start_y && input.local_pos.y <= bar_end_y {
                // Only show bar within poster bounds (including rounded corners)
                if dist < 0.0 {
                    // Apply smooth fade at the very edges for anti-aliasing
                    let edge_fade = smoothstep(0.0, -0.01, dist);

                    // Use select for branchless progress bar rendering
                    let is_filled = input.local_pos.x <= input.progress;
                    let bar_alpha = select(0.4, 0.8, is_filled) * edge_fade;
                    let bar_rgb = select(vec3<f32>(0.0, 0.0, 0.0), input.progress_color, is_filled);
                    let bar_pm = vec4<f32>(bar_rgb * bar_alpha, bar_alpha);
                    final_color = over(bar_pm, final_color);
                }
            }
        }
    }

    return final_color;
}
