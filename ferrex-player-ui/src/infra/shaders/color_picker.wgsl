// Color Picker Shader
// Renders a 2D radial color wheel with draggable handles for color harmony

const PI: f32 = 3.14159265359;

// Uniform buffer matching ColorPickerGlobals in Rust
struct Globals {
    transform: mat4x4<f32>,
    bounds: vec4<f32>,          // bounds.x, bounds.y, bounds.width, bounds.height
    geometry: vec4<f32>,        // wheel_radius, lightness, unused, unused
    handle_primary: vec4<f32>,  // hue, sat, hover_anim, drag_anim
    handle_comp1: vec4<f32>,    // hue, sat, hover_anim, active
    handle_comp2: vec4<f32>,    // hue, sat, hover_anim, active
    visual_params: vec4<f32>,   // handle_radius, border_width, time, scale_factor
    mouse_state: vec4<f32>,     // mouse.xy, hovered_id, unused
    primary_color_rgb: vec4<f32>,  // actual HSLuv RGB color for primary
    comp1_color_rgb: vec4<f32>,    // actual HSLuv RGB color for complement 1
    comp2_color_rgb: vec4<f32>,    // actual HSLuv RGB color for complement 2
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) position: vec2<f32>,
}

// Vertex shader - generates a full-screen quad
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;

    // Generate quad vertices (triangle strip: 0,1,2,3)
    let x = f32(vertex_index & 1u) * 2.0 - 1.0;
    let y = f32((vertex_index >> 1u) & 1u) * 2.0 - 1.0;

    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);

    // These will be computed in fragment shader from clip_position
    output.position = vec2<f32>(0.0);

    return output;
}

// HSL to RGB conversion (approximation for visual rendering)
// Note: Actual color values use CPU-side HSLuv crate for precision
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let h6 = h * 6.0;
    let x = c * (1.0 - abs(h6 % 2.0 - 1.0));
    let m = l - c * 0.5;

    var rgb: vec3<f32>;
    if (h6 < 1.0) {
        rgb = vec3<f32>(c, x, 0.0);
    } else if (h6 < 2.0) {
        rgb = vec3<f32>(x, c, 0.0);
    } else if (h6 < 3.0) {
        rgb = vec3<f32>(0.0, c, x);
    } else if (h6 < 4.0) {
        rgb = vec3<f32>(0.0, x, c);
    } else if (h6 < 5.0) {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }

    return rgb + m;
}

// Render the 2D color wheel (hue=angle, saturation=radius)
fn render_wheel(pos: vec2<f32>, center: vec2<f32>, radius: f32, lightness: f32) -> vec4<f32> {
    let offset = pos - center;
    let dist = length(offset);

    // Outside wheel - transparent
    if (dist > radius) {
        let aa = fwidth(dist);
        let edge_alpha = 1.0 - smoothstep(radius - aa, radius + aa, dist);
        return vec4<f32>(0.0, 0.0, 0.0, edge_alpha * 0.0); // Fully transparent outside
    }

    // Hue from angle (0-1)
    let hue = (atan2(offset.y, offset.x) + PI) / (2.0 * PI);

    // Saturation from radius (center=0, edge=1)
    let saturation = dist / radius;

    // Convert to RGB
    let color = hsl_to_rgb(hue, saturation, lightness);

    // Anti-alias the edge
    let aa = fwidth(dist);
    let alpha = 1.0 - smoothstep(radius - aa, radius + aa, dist);

    return vec4<f32>(color, alpha);
}

// Render a draggable handle at the given position
fn render_handle(
    pos: vec2<f32>,
    center: vec2<f32>,
    wheel_radius: f32,
    hue: f32,
    saturation: f32,
    actual_color: vec3<f32>,  // Actual HSLuv-computed RGB color
    hover_anim: f32,
    drag_anim: f32,
    handle_radius: f32,
    border_width: f32,
    is_primary: bool
) -> vec4<f32> {
    // Calculate handle position on 2D wheel
    let angle = hue * 2.0 * PI - PI;
    let r = saturation * wheel_radius;
    let handle_pos = center + vec2<f32>(cos(angle), sin(angle)) * r;

    let dist = length(pos - handle_pos);

    // Animate handle size based on hover/drag
    let size_mult = select(1.0, 1.1, is_primary); // Primary slightly larger
    let animated_r = handle_radius * size_mult * (1.0 + hover_anim * 0.25 + drag_anim * 0.15);

    // Early exit if outside glow range
    if (dist > animated_r * 2.5) {
        return vec4<f32>(0.0);
    }

    var result = vec4<f32>(0.0);
    let aa = fwidth(dist);

    // Outer glow when dragging or hovering - use actual color
    if (drag_anim > 0.01 || hover_anim > 0.01) {
        let glow_r = animated_r * 2.0;
        let glow_intensity = max(drag_anim * 0.5, hover_anim * 0.3);
        let glow_alpha = (1.0 - smoothstep(animated_r, glow_r, dist)) * glow_intensity;
        result = vec4<f32>(actual_color, glow_alpha);
    }

    // Handle fill - use actual HSLuv color for accuracy
    let inner_r = animated_r - border_width;

    let fill_alpha = 1.0 - smoothstep(inner_r - aa, inner_r + aa, dist);

    // White border
    let border_alpha = (1.0 - smoothstep(animated_r - aa, animated_r + aa, dist)) - fill_alpha;

    // Composite: glow behind, then fill, then border
    result = mix(result, vec4<f32>(actual_color, 1.0), fill_alpha);
    result = mix(result, vec4<f32>(1.0, 1.0, 1.0, 1.0), border_alpha * 0.95);

    return result;
}

// Alpha blending (premultiplied)
fn blend_over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let out_alpha = src.a + dst.a * (1.0 - src.a);
    if (out_alpha < 0.001) {
        return vec4<f32>(0.0);
    }
    let out_rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / out_alpha;
    return vec4<f32>(out_rgb, out_alpha);
}

// Fragment shader
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Get bounds (position is in viewport space from prepare(), size is original widget size)
    let bounds_pos = globals.bounds.xy;
    let bounds_size = globals.bounds.zw;
    let scale_factor = globals.visual_params.w;

    // clip_position.xy is in physical framebuffer coordinates
    // Convert to logical coordinates to match bounds
    let logical_pos = input.clip_position.xy / scale_factor;

    // Convert to widget-local coordinates
    // bounds_pos is now in viewport space (scroll-adjusted from prepare())
    let local_pos = logical_pos - bounds_pos;

    // Use smaller dimension to ensure circle fits and stays circular
    let widget_size = min(bounds_size.x, bounds_size.y);
    let center_local = vec2<f32>(widget_size * 0.5, widget_size * 0.5);

    // Wheel radius fills most of the widget
    let wheel_radius = widget_size * 0.42;
    let lightness = globals.geometry.y;

    // Handle radius proportional to wheel
    let handle_radius = max(wheel_radius * 0.05, 8.0);
    let border_width = globals.visual_params.y;

    // Use local coordinates for all calculations
    let pos = local_pos;
    let center = center_local;

    var final_color = vec4<f32>(0.0);

    // 1. Render the color wheel
    let wheel_color = render_wheel(pos, center, wheel_radius, lightness);
    final_color = blend_over(final_color, wheel_color);

    // 2. Render complement 2 handle (if active) - render first so it's behind
    if (globals.handle_comp2.w > 0.5) {
        let comp2 = render_handle(
            pos, center, wheel_radius,
            globals.handle_comp2.x,  // hue
            globals.handle_comp2.y,  // saturation
            globals.comp2_color_rgb.rgb,  // actual HSLuv color
            globals.handle_comp2.z,  // hover_anim
            0.0,                     // comp handles don't show drag_anim
            handle_radius * 0.9,     // slightly smaller
            border_width,
            false
        );
        final_color = blend_over(final_color, comp2);
    }

    // 3. Render complement 1 handle (if active)
    if (globals.handle_comp1.w > 0.5) {
        let comp1 = render_handle(
            pos, center, wheel_radius,
            globals.handle_comp1.x,  // hue
            globals.handle_comp1.y,  // saturation
            globals.comp1_color_rgb.rgb,  // actual HSLuv color
            globals.handle_comp1.z,  // hover_anim
            0.0,                     // comp handles don't show drag_anim
            handle_radius * 0.9,     // slightly smaller
            border_width,
            false
        );
        final_color = blend_over(final_color, comp1);
    }

    // 4. Render primary handle (on top)
    let primary = render_handle(
        pos, center, wheel_radius,
        globals.handle_primary.x,   // hue
        globals.handle_primary.y,   // saturation
        globals.primary_color_rgb.rgb,  // actual HSLuv color
        globals.handle_primary.z,   // hover_anim
        globals.handle_primary.w,   // drag_anim
        handle_radius,
        border_width,
        true
    );
    final_color = blend_over(final_color, primary);

    return final_color;
}
