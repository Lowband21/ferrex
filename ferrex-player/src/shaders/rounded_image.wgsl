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
    @location(1) screen_position: vec2<f32>, // Position in screen space for SDF calculation
    @location(2) bounds_center: vec2<f32>,   // Center of the bounds for SDF
    @location(3) bounds_half_size: vec2<f32>, // Half size for SDF
    @location(4) corner_radius: f32,         // Pass radius to fragment shader
    @location(5) opacity: f32,               // Pass opacity to fragment shader
    @location(6) is_backface: f32,           // 1.0 if showing back of card, 0.0 if front
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
    let position = input.position + vertex_pos * input.size;
    let center = input.position + input.size * 0.5;
    
    // Apply flip rotation if needed
    var transformed_pos: vec4<f32>;
    if input.rotation_y > 0.0 {
        let rotated_3d = apply_flip_rotation(position, center, input.rotation_y);
        // Apply perspective scaling based on Z
        let perspective_scale = 1.0 / (1.0 + rotated_3d.z * 0.001);
        transformed_pos = vec4<f32>(rotated_3d.x, rotated_3d.y, 0.0, 1.0);
        transformed_pos.x = center.x + (transformed_pos.x - center.x) * perspective_scale;
    } else {
        transformed_pos = vec4<f32>(position, 0.0, 1.0);
    }
    
    // Transform to clip space
    output.clip_position = globals.transform * transformed_pos;
    
    // Pass texture coordinates (flip horizontally if showing back)
    let is_backface = f32(input.rotation_y > 1.5708); // > PI/2 means showing back
    output.tex_coord = vec2<f32>(mix(vertex_pos.x, 1.0 - vertex_pos.x, is_backface), vertex_pos.y);
    
    // Pass data for SDF calculation in fragment shader
    output.screen_position = position;
    output.bounds_center = center;
    output.bounds_half_size = input.size * 0.5;
    output.corner_radius = input.radius;
    output.opacity = input.opacity;
    output.is_backface = is_backface;
    
    return output;
}

// Signed distance function for rounded rectangle
fn rounded_rect_sdf(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let d = abs(p - center) - half_size + vec2<f32>(radius);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture
    var color = textureSample(image_texture, texture_sampler, input.tex_coord);
    
    // For backface, we could show a different texture or solid color
    // For now, just tint it darker to show it's the back
    if input.is_backface > 0.5 {
        color = vec4<f32>(color.rgb * 0.7, color.a);
    }
    
    // Calculate signed distance to rounded rectangle edge
    let dist = rounded_rect_sdf(
        input.screen_position, 
        input.bounds_center, 
        input.bounds_half_size, 
        input.corner_radius
    );
    
    // Anti-aliased alpha using smoothstep
    // Negative distance = inside, positive = outside
    // We use a 2-pixel smooth transition for better anti-aliasing
    let alpha = 1.0 - smoothstep(-2.0, 2.0, dist);
    
    // Apply alpha clipping and opacity
    color.a = color.a * alpha * input.opacity;
    
    return color;
}