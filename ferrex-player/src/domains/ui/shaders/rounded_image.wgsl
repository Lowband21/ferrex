// Rounded Image Shader for Iced
// Provides GPU-accelerated rounded rectangle clipping with anti-aliasing

struct Globals {
    transform: mat4x4<f32>,
    scale_factor: f32,
    _padding: vec3<f32>, // Padding to align to 16 bytes
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
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) corner_radius_normalized: f32, // Corner radius as fraction of size
    @location(2) opacity: f32,                  // Pass opacity to fragment shader
    @location(3) is_backface: f32,              // 1.0 if showing back of card, 0.0 if front
    @location(4) theme_color: vec3<f32>,         // Theme color for backface
    @location(5) shadow_params: vec4<f32>,      // shadow_intensity, z_depth, scale, border_glow
    @location(6) local_pos: vec2<f32>,          // Position within poster for effects
    @location(7) animation_type: f32,           // Animation type for shader-specific effects
    @location(8) animation_progress: f32,        // Animation progress for debug visualization
    @location(9) hover_overlay_params: vec3<f32>, // is_hovered, show_overlay, show_border
    @location(10) mouse_position: vec2<f32>,    // Mouse position (normalized 0-1)
    @location(11) progress: f32,                // Progress percentage
    @location(12) progress_color: vec3<f32>,    // Progress bar color
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(1) var texture_sampler: sampler;
@group(1) @binding(2) var image_texture: texture_2d<f32>;

// Generate vertex positions and texture coordinates for a quad
fn vertex_position(vertex_index: u32) -> vec2<f32> {
    // Triangle strip positions for a quad:
    // 0: top-left, 1: top-right, 2: bottom-left, 3: bottom-right
    let x = f32(vertex_index & 1u);
    let y = f32((vertex_index >> 1u) & 1u);
    return vec2<f32>(x, y);
}


// Apply 3D rotation around Y-axis for flip animation
fn apply_flip_rotation(pos: vec2<f32>, center: vec2<f32>, rotation_y: f32) -> vec3<f32> {
    // Translate to origin (center of image)
    let translated = pos - center;
    
    // Apply Y-axis rotation
    let cos_theta = cos(rotation_y);
    let sin_theta = sin(rotation_y);
    
    // Rotate around Y-axis (x changes, y stays the same)
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
    
    // Then apply flip rotation if needed
    if rotation_y > 0.0 {
        let rotated_3d = apply_flip_rotation(transformed_pos, center, rotation_y);
        // Apply perspective scaling based on rotation Z
        let perspective_scale = 1.0 / (1.0 + rotated_3d.z * 0.001);
        transformed_pos = vec2<f32>(
            center.x + (rotated_3d.x - center.x) * perspective_scale,
            rotated_3d.y
        );
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
    
    // Pass texture coordinates (flip horizontally if showing back)
    // Show backface when rotation is past 90 degrees (including exactly at PI)
    let is_backface = f32(rotation_y >= 1.5708); // >= PI/2 (90 degrees)
    output.tex_coord = vec2<f32>(mix(vertex_pos.x, 1.0 - vertex_pos.x, is_backface), vertex_pos.y);
    
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
    let glow = exp(-glow_dist * 3.0) * glow_intensity;
    
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
    let aa_width = 0.002; // Anti-aliasing width
    
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
            let icon_alpha = smoothstep(0.002, -0.002, play_dist);
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
        let icon_alpha = smoothstep(0.002, -0.002, edit_dist) * edit_opacity;
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
        let icon_alpha = smoothstep(0.002, -0.002, dots_dist) * dots_opacity;
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
    
    // For backface, use theme color with dimming
    if input.is_backface > 0.5 {
        // Dim the theme color by 30% for backface (matching placeholder)
        linear_rgb = input.theme_color * 0.7;
    } else {
        // Front face: sample and display the poster texture
        let sampled_color = textureSample(image_texture, texture_sampler, input.tex_coord);
        linear_rgb = srgb_to_linear(sampled_color.rgb);
        alpha = sampled_color.a;
    }
    
    
    // Apply shadow effects based on z-depth
    linear_rgb = apply_shadow(linear_rgb, input.local_pos, shadow_intensity, z_depth);
    linear_rgb = apply_inner_shadow(linear_rgb, input.local_pos, z_depth);
    
    // Use texture coordinates for SDF (already in 0-1 range)
    let dist_normalized = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
    
    // Anti-aliased alpha using smoothstep
    // Use fwidth for screen-space derivatives to get proper anti-aliasing
    let fw = length(fwidth(input.tex_coord));
    let rounded_alpha = 1.0 - smoothstep(-fw, fw, dist_normalized);
    
    // Apply alpha clipping and opacity
    let final_alpha = alpha * rounded_alpha * input.opacity;
    
    // Convert back to sRGB for display
    var final_color = vec4<f32>(linear_to_srgb(linear_rgb), final_alpha);
    
    // Apply border glow effect
    final_color = apply_border_glow(final_color, dist_normalized, border_glow);
    
    // Render drop shadow behind the image
    if z_depth > 0.0 && shadow_intensity > 0.0 {
        let shadow = render_drop_shadow(input.tex_coord, input.corner_radius_normalized, shadow_intensity, z_depth);
        // Composite shadow behind the main image
        final_color = mix(shadow, final_color, final_color.a);
    }
    
    
    
    // Extract hover and overlay parameters
    let is_hovered = input.hover_overlay_params.x;
    let show_overlay = input.hover_overlay_params.y;
    let show_border = input.hover_overlay_params.z;
    
    // Render overlay when hovered and animation complete
    if show_overlay > 0.5 {
        let hover_dist = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
        
        // Dark tinted background (only inside poster bounds)
        if hover_dist < 0.0 {
            let overlay_tint = vec4<f32>(0.0, 0.0, 0.0, 0.5);
            final_color = mix(final_color, overlay_tint, overlay_tint.a);
        }
        
        // Render overlay buttons with mouse position
        let button_color = render_overlay_buttons(input.tex_coord, hover_dist, input.mouse_position, input.theme_color, input.progress_color);
        if button_color.a > 0.0 {
            final_color = mix(final_color, button_color, button_color.a);
        }
    }
    
    // Always render border - simple, clean approach
    {
        // Calculate distance from poster edge
        let poster_dist = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
        
        // Border widths - expand outward on hover
        let unhovered_width = 0.004;  // Thin border when not hovered
        let hovered_width = 0.005;    // Thicker border when hovered
        let border_width = select(unhovered_width, hovered_width, is_hovered > 0.5);
        
        // For hover state, we want the border to expand outward
        // So we check if we're in the border region differently based on hover state
        var in_border = false;
        var border_alpha = 0.0;
        
        if is_hovered > 0.5 {
            // Hovered: border expands outward from poster edge
            // We're in border if distance is between -border_width and +border_width
            in_border = poster_dist > -border_width && poster_dist < border_width;
            if in_border {
                // Anti-aliasing for both inner and outer edges
                let inner_alpha = smoothstep(-border_width, -border_width + 0.001, poster_dist);
                let outer_alpha = 1.0 - smoothstep(border_width - 0.001, border_width, poster_dist);
                border_alpha = min(inner_alpha, outer_alpha);
            }
        } else {
            // Not hovered: thin border just inside poster edge
            // We're in border if we're inside poster and close to edge
            in_border = poster_dist < 0.0 && poster_dist > -border_width;
            if in_border {
                // Anti-aliasing for inner edge only
                border_alpha = smoothstep(-border_width, -border_width + 0.001, poster_dist);
            }
        }
        
        // Apply border if we're in the border region
        if border_alpha > 0.0 {
            // Border color based on hover state
            let border_color = select(
                vec3<f32>(0.0, 0.0, 0.0),  // Black when not hovered
                input.progress_color,       // Progress color (blue) when hovered
                is_hovered > 0.5
            );
            
            // Apply border with proper alpha blending
            final_color = vec4<f32>(
                mix(final_color.rgb, border_color, border_alpha),
                max(final_color.a, border_alpha)
            );
        }
    }
    
    // Render watch status corner indicator if progress is valid (not -1.0)
    if input.progress >= 0.0 {
        // Standard poster aspect ratio is 2:3 (width:height)
        let aspect_ratio = 2.0 / 3.0;
        
        // Create a triangular indicator shaped like a folded corner
        // Adjust the size to account for aspect ratio so it appears square
        let fold_size_x = input.corner_radius_normalized * 2.5;
        let fold_size_y = fold_size_x * aspect_ratio;
        
        // Top-right corner origin
        let corner_origin = vec2<f32>(1.0, 0.0);
        let rel_pos = input.tex_coord - corner_origin;
        
        // Simple right triangle check - like a corner folded down
        // Triangle with vertices at: (0,0), (-fold_size_x,0), (0,fold_size_y)
        // Adjust the diagonal check to account for different x and y sizes
        let normalized_x = rel_pos.x / fold_size_x;
        let normalized_y = rel_pos.y / fold_size_y;
        let in_triangle = rel_pos.x <= 0.0 && 
                         rel_pos.y >= 0.0 && 
                         (normalized_y - normalized_x) <= 1.0;
        
        // Also check against the rounded corner boundary
        let corner_dist = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
        
        if in_triangle && corner_dist < 0.0 {
            // Calculate distance to diagonal edge for smooth anti-aliasing
            // For the normalized diagonal: normalized_y - normalized_x = 1
            // Distance formula: |ax + by + c| / sqrt(a² + b²)
            // Where line is: x/fold_size_x - y/fold_size_y + 1 = 0
            let a = 1.0 / fold_size_x;
            let b = -1.0 / fold_size_y;
            let c = 1.0;
            let diagonal_dist = abs(a * rel_pos.x + b * rel_pos.y + c) / sqrt(a*a + b*b);
            let edge_softness = 0.005;
            let triangle_alpha = smoothstep(-edge_softness, edge_softness, diagonal_dist);
            
            // Determine opacity based on watch status
            var indicator_opacity: f32;
            if input.progress == 0.0 {
                // Unwatched - colored indicator
                indicator_opacity = 0.85;
            } else if input.progress < 0.95 {
                // In progress - semi-transparent
                indicator_opacity = 0.6;
            } else {
                // Watched - very subtle
                indicator_opacity = 0.2;
            }
            
            // Apply the indicator color
            let indicator_color_srgb = linear_to_srgb(input.progress_color);
            let indicator_with_alpha = vec4<f32>(indicator_color_srgb, indicator_opacity * triangle_alpha);
            
            // Blend the indicator over the current color
            final_color = mix(final_color, indicator_with_alpha, indicator_with_alpha.a);
        }
    }
    
    // Render progress bar at bottom for in-progress media
    if input.progress > 0.0 && input.progress < 0.95 {
        // Progress bar dimensions
        let bar_height = 0.02; // 2% of poster height
        let bar_margin = 0.01; // 1% margin from bottom
        
        // Check if we're in the progress bar area
        if input.tex_coord.y > (1.0 - bar_height - bar_margin) {
            // Calculate progress bar boundaries
            let bar_start_y = 1.0 - bar_height - bar_margin;
            let bar_end_y = 1.0 - bar_margin;
            
            // Check if we're within the bar height
            if input.tex_coord.y >= bar_start_y && input.tex_coord.y <= bar_end_y {
                // Only show bar within poster bounds (including rounded corners)
                let poster_dist = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
                if poster_dist < 0.0 {
                    // Apply smooth fade at the very edges for anti-aliasing
                    let edge_fade = smoothstep(0.0, -0.01, poster_dist);
                    
                    // Determine if we're in the filled or unfilled part
                    if input.tex_coord.x <= input.progress {
                        // Filled part - use progress color
                        let bar_color = linear_to_srgb(input.progress_color);
                        let bar_with_alpha = vec4<f32>(bar_color, 0.8 * edge_fade);
                        final_color = mix(final_color, bar_with_alpha, bar_with_alpha.a);
                    } else {
                        // Unfilled part - dark transparent background
                        let bg_color = vec4<f32>(0.0, 0.0, 0.0, 0.4 * edge_fade);
                        final_color = mix(final_color, bg_color, bg_color.a);
                    }
                }
            }
        }
    }
    
    return final_color;
}