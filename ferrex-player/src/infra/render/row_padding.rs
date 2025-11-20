//! Utilities for CPU-side row padding for texture uploads.
//!
//! These helpers centralize padding so all CPU→texture copies
//! can share identical, well-tested logic.

use iced_wgpu::wgpu;

/// Compute the padded row stride in bytes for a given width and bytes-per-pixel,
/// aligned to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`.
pub fn compute_padded_stride(width: u32, bytes_per_pixel: u32) -> usize {
    let row = width as usize * bytes_per_pixel as usize;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize; // typically 256
    if row == 0 {
        return 0;
    }
    ((row + align - 1) / align) * align
}

/// Pads an RGBA image buffer to a 256-byte aligned stride.
///
/// Returns the padded buffer and the padded stride in bytes.
pub fn pad_rows_rgba(
    pixels: &[u8],
    width: u32,
    height: u32,
) -> (Vec<u8>, usize) {
    let bytes_per_pixel = 4u32;
    let row = (width * bytes_per_pixel) as usize;
    let padded_stride = compute_padded_stride(width, bytes_per_pixel);

    if row == 0 || height == 0 {
        return (Vec::new(), padded_stride);
    }

    // Fast path: already aligned
    if row == padded_stride {
        return (pixels.to_vec(), padded_stride);
    }

    let h = height as usize;
    let mut out = vec![0u8; padded_stride * h];
    for y in 0..h {
        let src = y * row;
        let dst = y * padded_stride;
        out[dst..dst + row].copy_from_slice(&pixels[src..src + row]);
    }
    (out, padded_stride)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_stride_is_aligned() {
        for width in 1..512u32 {
            let stride = compute_padded_stride(width, 4);
            assert_eq!(stride % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize, 0);
            let unpadded = width as usize * 4;
            if unpadded % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize == 0 {
                assert_eq!(stride, unpadded);
            } else {
                assert!(stride > unpadded);
            }
        }
    }

    #[test]
    fn pad_rows_copies_each_row_correctly() {
        // Use a width that is not 256-aligned to exercise padding logic
        let width = 65u32; // 65 * 4 = 260
        let height = 3u32;
        let row = (width * 4) as usize;

        let mut src = vec![0u8; (row * height as usize) as usize];
        for y in 0..height as usize {
            for x in 0..row {
                // Give each row a distinct pattern
                src[y * row + x] =
                    (y as u8).wrapping_mul(11).wrapping_add(x as u8);
            }
        }

        let (padded, stride) = pad_rows_rgba(&src, width, height);
        assert_eq!(stride % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize, 0);

        for y in 0..height as usize {
            let src_off = y * row;
            let dst_off = y * stride;
            assert_eq!(
                &padded[dst_off..dst_off + row],
                &src[src_off..src_off + row]
            );
        }
    }
}
