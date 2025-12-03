//! Render pipeline and GPU types for the color picker

use bytemuck::{Pod, Zeroable};

/// GPU uniform data for the color picker
///
/// Total size: 224 bytes (14 * 16 bytes, all 16-byte aligned)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ColorPickerGlobals {
    /// Projection/transform matrix (offset 0, size 64)
    pub transform: [f32; 16],

    /// Widget bounds (offset 64, size 16)
    /// [bounds.x, bounds.y, bounds.width, bounds.height]
    pub bounds: [f32; 4],

    /// Geometry parameters (offset 80, size 16)
    /// [wheel_radius, lightness (0-1), unused, unused]
    pub geometry: [f32; 4],

    /// Primary handle parameters (offset 96, size 16)
    /// [hue (0-1), saturation (0-1), hover_anim, drag_anim]
    pub handle_primary: [f32; 4],

    /// Complement 1 handle parameters (offset 112, size 16)
    /// [hue (0-1), saturation (0-1), hover_anim, active (0 or 1)]
    pub handle_comp1: [f32; 4],

    /// Complement 2 handle parameters (offset 128, size 16)
    /// [hue (0-1), saturation (0-1), hover_anim, active (0 or 1)]
    pub handle_comp2: [f32; 4],

    /// Visual parameters (offset 144, size 16)
    /// [handle_radius, border_width, time, scale_factor]
    pub visual_params: [f32; 4],

    /// Mouse state (offset 160, size 16)
    /// [mouse.x, mouse.y, hovered_handle_id (-1, 0, 1, 2), unused]
    pub mouse_state: [f32; 4],

    /// Primary color in sRGB (offset 176, size 16)
    /// [r, g, b, unused] - actual HSLuv-converted color
    pub primary_color_rgb: [f32; 4],

    /// Complement 1 color in sRGB (offset 192, size 16)
    /// [r, g, b, unused] - actual HSLuv-converted color
    pub comp1_color_rgb: [f32; 4],

    /// Complement 2 color in sRGB (offset 208, size 16)
    /// [r, g, b, unused] - actual HSLuv-converted color
    pub comp2_color_rgb: [f32; 4],
}

// Compile-time assertion to verify struct size
const _: () = assert!(
    std::mem::size_of::<ColorPickerGlobals>() == 224,
    "ColorPickerGlobals must be 224 bytes"
);
