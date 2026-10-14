//! `damascene_wgpu::headless::Headless` regression tests: the
//! single-sample path and fractional scale factors both used to panic
//! (a 1-sample resolve source; a target one pixel short of the
//! runner's rounded-up viewport). Skips cleanly when no adapter is
//! available.

use damascene_core::prelude::*;
use damascene_wgpu::headless::Headless;

fn tree() -> El {
    column([text("headless").key("t")]).padding(Sides::all(8.0))
}

#[test]
fn renders_at_sample_count_one_and_fractional_scale() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let theme = Theme::default();
    let clear = theme.palette().background;
    for (sample_count, scale, w, h) in [(1u32, 1.0f32, 641.0f32, 300.0f32), (4, 1.5, 641.0, 300.0), (1, 1.5, 641.0, 300.0)] {
        let (width, height, pixels) = gpu
            .render_rgba8(
                tree(),
                theme.clone(),
                Rect::new(0.0, 0.0, w, h),
                scale,
                sample_count,
                clear,
            )
            .unwrap_or_else(|e| panic!("msaa {sample_count} scale {scale}: {e}"));
        assert_eq!(width, (w * scale).ceil() as u32, "msaa {sample_count} scale {scale}: width");
        assert_eq!(height, (h * scale).ceil() as u32, "msaa {sample_count} scale {scale}: height");
        assert_eq!(pixels.len(), (width * height * 4) as usize);
    }
}
