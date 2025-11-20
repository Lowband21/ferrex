// Rounded Image Shader for Iced
// Provides GPU-accelerated rounded rectangle clipping with anti-aliasing

struct Globals {
    transform: mat4x4<f32>,
    scale_factor: f32,
    _padding: vec3<f32>, // Padding to align to 16 bytes
}


struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    // Instance attributes
    @location(0) position: vec2<f32>,    // Top-left position
    @location(1) size: vec2<f32>,        // Width and height
    @location(2) radius: f32,            // Corner radius
    @location(3) opacity: f32,           // Opacity for fade animations
    @location(4) rotation_y: f32,        // Y-axis rotation for flip animation
    @location(5) animation_progress: f32, // Animation progress (0.0 to 1.0)
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) corner_radius_normalized: f32, // Corner radius as fraction of size
    @location(2) opacity: f32,                  // Pass opacity to fragment shader
    @location(3) is_backface: f32,              // 1.0 if showing back of card, 0.0 if front
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

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    
    // Generate quad vertex position (0,0) to (1,1)
    let vertex_pos = vertex_position(input.vertex_index);
    
    // Calculate position within bounds using instance data
    // These positions are in logical pixels
    let position = input.position + vertex_pos * input.size;
    let center = input.position + input.size * 0.5;
    
    // Apply flip rotation if needed
    var final_pos: vec2<f32>;
    if input.rotation_y > 0.0 {
        let rotated_3d = apply_flip_rotation(position, center, input.rotation_y);
        // Apply perspective scaling based on Z
        let perspective_scale = 1.0 / (1.0 + rotated_3d.z * 0.001);
        final_pos = vec2<f32>(
            center.x + (rotated_3d.x - center.x) * perspective_scale,
            rotated_3d.y
        );
    } else {
        final_pos = position;
    }
    
    // The projection matrix has an unusual last row: [-1, 1, 0, 1]
    // This is causing issues with the transformation
    // Let's manually apply a corrected orthographic projection
    
    // Extract viewport dimensions from the projection matrix scale
    let viewport_width = 2.0 / globals.transform[0][0];
    let viewport_height = 2.0 / abs(globals.transform[1][1]);
    
    // Convert logical positions to physical pixels
    let physical_pos = final_pos * globals.scale_factor;
    
    // Apply standard orthographic projection
    // Map [0, viewport_width] to [-1, 1] and [0, viewport_height] to [1, -1] (Y is flipped)
    let clip_x = (physical_pos.x / viewport_width) * 2.0 - 1.0;
    let clip_y = 1.0 - (physical_pos.y / viewport_height) * 2.0;
    
    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    
    // Pass texture coordinates (flip horizontally if showing back)
    let is_backface = f32(input.rotation_y > 1.5708); // > PI/2 means showing back
    output.tex_coord = vec2<f32>(mix(vertex_pos.x, 1.0 - vertex_pos.x, is_backface), vertex_pos.y);
    
    // Pass normalized corner radius (as fraction of smaller dimension)
    let min_dimension = min(input.size.x, input.size.y);
    output.corner_radius_normalized = input.radius / min_dimension;
    output.opacity = input.opacity;
    output.is_backface = is_backface;
    
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

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    let sampled_color = textureSample(image_texture, texture_sampler, input.tex_coord);
    
    // Convert from sRGB to linear color space for processing
    var linear_rgb = srgb_to_linear(sampled_color.rgb);
    
    // For backface, we could show a different texture or solid color
    // For now, just tint it darker to show it's the back
    if input.is_backface > 0.5 {
        linear_rgb = linear_rgb * 0.7;
    }
    
    // Use texture coordinates for SDF (already in 0-1 range)
    let dist_normalized = rounded_rect_sdf_normalized(input.tex_coord, input.corner_radius_normalized);
    
    // Anti-aliased alpha using smoothstep
    // Use fwidth for screen-space derivatives to get proper anti-aliasing
    let fw = length(fwidth(input.tex_coord));
    let alpha = 1.0 - smoothstep(-fw, fw, dist_normalized);
    
    // Apply alpha clipping and opacity
    let final_alpha = sampled_color.a * alpha * input.opacity;
    
    // Convert back to sRGB for display
    let final_rgb = linear_to_srgb(linear_rgb);
    
    return vec4<f32>(final_rgb, final_alpha);
}