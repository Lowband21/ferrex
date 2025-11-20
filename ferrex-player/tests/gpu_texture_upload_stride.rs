#![cfg(feature = "gpu-tests")]

// Integration test that performs a padded texture upload and roundtrips it back
// to CPU memory to verify row padding semantics. Feature-gated to avoid
// requiring a GPU/device in all CI runs.

use iced_wgpu::wgpu; // reuse the same wgpu as the app

fn block_on<F: std::future::Future<Output = T>, T>(fut: F) -> T {
    futures::executor::block_on(fut)
}

#[test]
fn gpu_write_texture_padded_roundtrip() {
    // Create an instance and a default adapter/device (any backend)
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        match block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        })) {
            Ok(a) => a,
            Err(_e) => {
                // No adapter available in this environment; skip test gracefully.
                eprintln!("No compatible GPU adapter found; skipping gpu test");
                return;
            }
        };

    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("request_device");

    // Use a width that is not 256-aligned to exercise padding logic
    let width = 65u32; // 65 * 4 = 260
    let height = 3u32;
    let bytes_per_pixel = 4u32;
    let row = (width * bytes_per_pixel) as usize;

    let mut src = vec![0u8; row * height as usize];
    for y in 0..height as usize {
        for x in 0..row {
            src[y * row + x] = (y as u8).wrapping_mul(19).wrapping_add(x as u8);
        }
    }

    let (padded, padded_stride) =
        ferrex_player::infra::render::row_padding::pad_rows_rgba(
            &src, width, height,
        );

    assert_eq!(
        padded_stride % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize,
        0
    );

    // Create destination texture
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu-tests.texture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // Upload via write_texture with padded rows
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &padded,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded_stride as u32),
            rows_per_image: Some(height),
        },
        extent,
    );

    // Read back into a buffer (also padded) and compare row slices
    let readback_stride = padded_stride; // use same stride for simplicity
    let readback_size = (readback_stride * height as usize) as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-tests.readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-tests.encoder"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(readback_stride as u32),
                rows_per_image: None,
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait);
    let mapped = slice.get_mapped_range();

    for y in 0..height as usize {
        let src_off = y * row;
        let dst_off = y * readback_stride;
        assert_eq!(
            &mapped[dst_off..dst_off + row],
            &src[src_off..src_off + row]
        );
    }
}
