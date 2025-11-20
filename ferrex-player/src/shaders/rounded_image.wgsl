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
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) screen_position: vec2<f32>, // Position in screen space for SDF calculation
    @location(2) bounds_center: vec2<f32>,   // Center of the bounds for SDF
    @location(3) bounds_half_size: vec2<f32>, // Half size for SDF
    @location(4) corner_radius: f32,         // Pass radius to fragment shader
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

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    
    // Generate quad vertex position (0,0) to (1,1)
    let vertex_pos = vertex_position(input.vertex_index);
    
    // Calculate position within bounds using instance data
    let position = input.position + vertex_pos * input.size;
    
    // Transform to clip space
    output.clip_position = globals.transform * vec4<f32>(position, 0.0, 1.0);
    
    // Pass texture coordinates
    output.tex_coord = vertex_pos;
    
    // Pass data for SDF calculation in fragment shader
    output.screen_position = position;
    output.bounds_center = input.position + input.size * 0.5;
    output.bounds_half_size = input.size * 0.5;
    output.corner_radius = input.radius;
    
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
    
    // Apply alpha clipping
    color.a = color.a * alpha;
    
    return color;
}