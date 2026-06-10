//! GPU lifecycle test for [`damascene_wgpu::StreamingTexture`] (issue
//! #95): handle identity stays stable across frame writes (warm
//! bind-group cache), changes on resize (new GPU texture), and the
//! hash-dedup variant skips identical consecutive frames.
//!
//! Skips cleanly (passes) when no adapter is available, matching the
//! other GPU tests.

use damascene_wgpu::StreamingTexture;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("streaming_texture_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn streaming_texture_lifecycle() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };

    let mut tex = StreamingTexture::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, 4, 4);
    assert_eq!(tex.size(), (4, 4));
    let id_initial = tex.app_texture().id();

    // Same-size frame writes keep the handle identity stable — this is
    // what keeps the backend's per-texture bind-group cache warm.
    let frame_a = vec![0x11u8; 4 * 4 * 4];
    let frame_b = vec![0x22u8; 4 * 4 * 4];
    tex.write_frame(&device, &queue, 4, 4, &frame_a);
    tex.write_frame(&device, &queue, 4, 4, &frame_b);
    assert_eq!(tex.app_texture().id(), id_initial, "stable across writes");

    // Hash-dedup: a repeated frame is skipped, a new one uploads.
    assert!(tex.write_frame_if_changed(&device, &queue, 4, 4, &frame_a));
    assert!(!tex.write_frame_if_changed(&device, &queue, 4, 4, &frame_a));
    assert!(tex.write_frame_if_changed(&device, &queue, 4, 4, &frame_b));

    // Resize recreates the texture: new identity, new size.
    let frame_wide = vec![0x33u8; 8 * 4 * 4];
    tex.write_frame(&device, &queue, 8, 4, &frame_wide);
    assert_eq!(tex.size(), (8, 4));
    assert_ne!(tex.app_texture().id(), id_initial, "new identity on resize");

    queue.submit([]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .ok();
}
