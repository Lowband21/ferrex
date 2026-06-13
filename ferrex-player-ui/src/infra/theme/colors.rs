//! Color manipulation utilities for accent color variants
//!
//! Provides helpers for deriving hover, glow, and other color variants
//! from a base accent color.

use iced::Color;

/// Apply alpha to a base color
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// Brighten a color by adding to RGB components
///
/// This is a simple additive brightening that works well for accent colors.
pub fn brighten(color: Color, amount: f32) -> Color {
    Color {
        r: (color.r + amount).clamp(0.0, 1.0),
        g: (color.g + amount).clamp(0.0, 1.0),
        b: (color.b + amount).clamp(0.0, 1.0),
        a: color.a,
    }
}

/// Lighten a color by increasing its luminance (HSL-based)
///
/// This provides more perceptually uniform lightening than simple RGB addition.
pub fn lighten(color: Color, amount: f32) -> Color {
    let (h, s, l) = rgb_to_hsl(color.r, color.g, color.b);
    let new_l = (l + amount).clamp(0.0, 1.0);
    let (r, g, b) = hsl_to_rgb(h, s, new_l);
    Color {
        r,
        g,
        b,
        a: color.a,
    }
}

/// Darken a color by decreasing its luminance (HSL-based)
pub fn darken(color: Color, amount: f32) -> Color {
    lighten(color, -amount)
}

/// Convert RGB (0.0-1.0) to HSL (h: 0-360, s: 0-1, l: 0-1)
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        // Achromatic (gray)
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if (max - r).abs() < f32::EPSILON {
        let mut h = (g - b) / d;
        if g < b {
            h += 6.0;
        }
        h * 60.0
    } else if (max - g).abs() < f32::EPSILON {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };

    (h, s, l)
}

/// Convert HSL (h: 0-360, s: 0-1, l: 0-1) to RGB (0.0-1.0)
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s.abs() < f32::EPSILON {
        // Achromatic (gray)
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h_norm = h / 360.0;

    let r = hue_to_rgb(p, q, h_norm + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h_norm);
    let b = hue_to_rgb(p, q, h_norm - 1.0 / 3.0);

    (r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }

    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_alpha() {
        let color = Color::from_rgb(1.0, 0.5, 0.0);
        let result = with_alpha(color, 0.5);
        assert!((result.a - 0.5).abs() < f32::EPSILON);
        assert!((result.r - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_brighten() {
        let color = Color::from_rgb(0.5, 0.5, 0.5);
        let result = brighten(color, 0.2);
        assert!((result.r - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_brighten_clamped() {
        let color = Color::from_rgb(0.9, 0.9, 0.9);
        let result = brighten(color, 0.5);
        assert!((result.r - 1.0).abs() < f32::EPSILON);
    }
}
