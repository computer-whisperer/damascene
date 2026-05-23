//! Media — images, icons, avatars.
//!
//! Apps construct `Image`s once (typically via `LazyLock` over a
//! decoded byte slice; here we synthesize test patterns in code so the
//! fixture is self-contained — no PNG dep). Equal pixel buffers share
//! a backend texture-cache slot, so the four `image(SOLID.clone())`
//! calls in the avatar row map to one GPU upload.

use std::sync::LazyLock;

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State;

const PIPEWIRE_VOLUME_SVG: &str = include_str!("../../icons/pipewire-volume.svg");

const LINEAR_HORIZONTAL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0" y1="32" x2="64" y2="32" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#ff5577"/>
      <stop offset="1" stop-color="#5577ff"/>
    </linearGradient>
  </defs>
  <rect x="4" y="4" width="56" height="56" rx="12" fill="url(#g)"/>
</svg>"##;

const LINEAR_DIAGONAL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="8" y1="8" x2="56" y2="56" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#22d3ee"/>
      <stop offset="0.5" stop-color="#3b82f6"/>
      <stop offset="1" stop-color="#8b5cf6"/>
    </linearGradient>
  </defs>
  <rect x="4" y="4" width="56" height="56" rx="12" fill="url(#g)"/>
</svg>"##;

const RADIAL_BBOX_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <radialGradient id="g" cx="35%" cy="30%" r="70%">
      <stop offset="0" stop-color="#fef3c7"/>
      <stop offset="0.5" stop-color="#f59e0b"/>
      <stop offset="1" stop-color="#7c2d12"/>
    </radialGradient>
  </defs>
  <circle cx="32" cy="32" r="28" fill="url(#g)"/>
</svg>"##;

const STROKED_GRADIENT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="8" y1="8" x2="56" y2="56" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#10b981"/>
      <stop offset="1" stop-color="#06b6d4"/>
    </linearGradient>
  </defs>
  <path d="M 12 32 A 20 20 0 1 1 52 32" fill="none" stroke="url(#g)" stroke-width="6" stroke-linecap="round"/>
  <line x1="12" y1="48" x2="52" y2="48" stroke="url(#g)" stroke-width="6" stroke-linecap="round"/>
</svg>"##;

static PIPEWIRE_VOLUME: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(PIPEWIRE_VOLUME_SVG).expect("pipewire icon parses"));
static LINEAR_HORIZONTAL: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(LINEAR_HORIZONTAL_SVG).expect("linear-horizontal parses"));
static LINEAR_DIAGONAL: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(LINEAR_DIAGONAL_SVG).expect("linear-diagonal parses"));
static RADIAL_BBOX: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(RADIAL_BBOX_SVG).expect("radial-bbox parses"));
static STROKED_GRADIENT: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(STROKED_GRADIENT_SVG).expect("stroked-gradient parses"));

static GRID_RG: LazyLock<Image> =
    LazyLock::new(|| make_gradient(64, 64, [255, 64, 64], [64, 96, 255]));
static GRID_GB: LazyLock<Image> =
    LazyLock::new(|| make_gradient(64, 64, [64, 200, 100], [40, 40, 60]));
// 16:9 wide gradient for the Size::Aspect demo — non-square so the
// aspect-derived height is visibly different from the El's width.
static GRID_WIDE: LazyLock<Image> =
    LazyLock::new(|| make_gradient(160, 90, [255, 180, 64], [80, 40, 200]));
static GRID_CHECKER: LazyLock<Image> = LazyLock::new(|| make_checker(64, 64, 8));
static GRID_RING: LazyLock<Image> = LazyLock::new(|| make_ring(64, 64));
static AVATAR_SOLID: LazyLock<Image> =
    LazyLock::new(|| Image::from_rgba8(32, 32, vec![255; 32 * 32 * 4]));

fn make_gradient(w: u32, h: u32, top_left: [u8; 3], bottom_right: [u8; 3]) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h - 2) as f32;
            let r = (top_left[0] as f32 * (1.0 - t) + bottom_right[0] as f32 * t) as u8;
            let g = (top_left[1] as f32 * (1.0 - t) + bottom_right[1] as f32 * t) as u8;
            let b = (top_left[2] as f32 * (1.0 - t) + bottom_right[2] as f32 * t) as u8;
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = r;
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_checker(w: u32, h: u32, cell: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let on = (((x / cell) + (y / cell)) & 1) == 0;
            let v = if on { 240 } else { 32 };
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = v;
            pixels[i + 1] = v;
            pixels[i + 2] = v;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_ring(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let r_outer = w.min(h) as f32 * 0.45;
    let r_inner = r_outer - 6.0;
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let on = d <= r_outer && d >= r_inner;
            let i = ((y * w + x) * 4) as usize;
            if on {
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = 255;
            } else {
                pixels[i + 3] = 0;
            }
        }
    }
    Image::from_rgba8(w, h, pixels)
}

pub fn view(animated_surface: Option<&AppTexture>, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Media"),
        paragraph(
            "Three families: raster `image`s, monochrome built-in icons, \
             and gradient-laden custom SVGs through `SvgIcon`. Avatars \
             stack the same primitives — image, fallback initials, or a \
             tinted shape.",
        )
        .muted(),
        section_card(
            "Animated surface (app-owned GPU texture)",
            "`surface(AppTexture)` composites pixels the app writes each frame — \
             3D viewports, video, animated images. The host writes a procedural \
             frame to a 96×96 RGBA8 source texture in `WinitWgpuApp::before_paint`; \
             Aetna samples it across each tile's resolved rect with bilinear \
             filtering, so the small source stretches to fill the cell — source \
             pixel dimensions and rendered size are independent. Three tiles, \
             three `SurfaceAlpha` modes, one shared texture.",
            [animated_surface_demo(animated_surface)],
        ),
        section_card(
            "Avatars",
            "Image, fallback initials, or a colored shape — same anatomy.",
            [phone_scrollable_row(
                phone,
                [
                    avatar_image(GRID_RG.clone()),
                    avatar_image(GRID_GB.clone()),
                    avatar_initials("CB"),
                    avatar_initials("AK"),
                    avatar_fallback("Max Leiter"),
                ],
                tokens::SPACE_3,
                Align::Center,
            )],
        ),
        section_card(
            "Raster images",
            "Test patterns generated in code so the fixture is self-contained.",
            [phone_scrollable_row(
                phone,
                [
                    tile(&GRID_RG, "gradient"),
                    tile(&GRID_GB, "moss"),
                    tile(&GRID_CHECKER, "checker"),
                    tile(&GRID_RING, "ring"),
                ],
                tokens::SPACE_3,
                Align::Center,
            )],
        ),
        section_card(
            "Tints share one texture",
            "Four references to the same Image with different `image_tint(...)` colors — content-hashed into one GPU upload.",
            [phone_scrollable_row(
                phone,
                [
                    tinted_avatar(Color::srgb_u8(96, 165, 250)),
                    tinted_avatar(Color::srgb_u8(244, 114, 182)),
                    tinted_avatar(Color::srgb_u8(248, 113, 113)),
                    tinted_avatar(Color::srgb_u8(132, 204, 22)),
                ],
                tokens::SPACE_2,
                Align::Center,
            )],
        ),
        section_card(
            "ImageFit modes",
            "Same image, four projections into identically-sized boxes.",
            [phone_scrollable_row(
                phone,
                [
                    fit_demo("Contain", ImageFit::Contain),
                    fit_demo("Cover", ImageFit::Cover),
                    fit_demo("Fill", ImageFit::Fill),
                    fit_demo("None", ImageFit::None),
                ],
                tokens::SPACE_3,
                Align::Start,
            )],
        ),
        section_card(
            "Size::Aspect",
            "Width fills the column; height = width × (90 / 160). The caption \
             below flows naturally around the derived height — change the \
             window width and the image reshapes while keeping the 16:9 ratio.",
            [aspect_demo()],
        ),
        section_card(
            "Built-in lucide icons (monochrome / MSDF)",
            "`icon(IconName::*)` paints through the monochrome MSDF atlas.",
            [phone_scrollable_row(
                phone,
                [
                    builtin_icon_tile(IconName::Activity, tokens::WARNING),
                    builtin_icon_tile(IconName::Bell, tokens::PRIMARY),
                    builtin_icon_tile(IconName::Check, tokens::SUCCESS),
                    builtin_icon_tile(IconName::AlertCircle, tokens::DESTRUCTIVE),
                    builtin_icon_tile(IconName::Settings, tokens::FOREGROUND),
                ],
                tokens::SPACE_3,
                Align::Stretch,
            )],
        ),
        section_card(
            "Gradient SVGs (custom / tessellated)",
            "App-supplied SvgIcon — gradients route through the tessellated path with per-vertex colour.",
            [phone_scrollable_row(
                phone,
                [
                    custom_icon_tile(&PIPEWIRE_VOLUME, "pipewire", 72.0),
                    custom_icon_tile(&LINEAR_HORIZONTAL, "linear h", 56.0),
                    custom_icon_tile(&LINEAR_DIAGONAL, "diagonal", 56.0),
                    custom_icon_tile(&RADIAL_BBOX, "radial", 56.0),
                    custom_icon_tile(&STROKED_GRADIENT, "stroked", 56.0),
                ],
                tokens::SPACE_3,
                Align::Center,
            )],
        ),
        section_card(
            "Programmatic vectors (vector() + PathBuilder)",
            "`vector(asset)` paints a programmatically-built `VectorAsset` through \
             the icon MSDF atlas. Single-solid-colour assets route to MSDF for crisp \
             scaling; multi-colour or gradient assets fall back to lyon tessellation. \
             Identical geometry hashes into one atlas slot, so a list of merge curves \
             with recurring (lane_delta, row_span) pairs shares cached rasterisations.",
            [vector_demo_row(phone)],
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

/// Wraps a row of fixed-size tiles in a horizontal scroll on phone so
/// they don't overflow the page column when their summed widths exceed
/// the viewport.
fn phone_scrollable_row<I>(phone: bool, tiles: I, gap: f32, align: Align) -> El
where
    I: IntoIterator<Item = El>,
{
    let tiles_row = row(tiles).gap(gap).align(align);
    if phone {
        scroll([tiles_row
            .width(Size::Hug)
            .padding(Sides::xy(0.0, tokens::RING_WIDTH))])
        .axis(Axis::Row)
        .height(Size::Hug)
        .width(Size::Fill(1.0))
    } else {
        tiles_row
    }
}

fn vector_demo_row(phone: bool) -> El {
    phone_scrollable_row(
        phone,
        [
            vector_tile("merge curve", curve_asset(0, 3, 4), 80.0, 100.0),
            vector_tile("steeper curve", curve_asset(0, 4, 3), 100.0, 80.0),
            vector_tile(
                "filled diamond",
                diamond_asset(Color::srgb_u8(244, 114, 182)),
                48.0,
                48.0,
            ),
            vector_tile(
                "rounded path",
                squiggle_asset(Color::srgb_u8(96, 165, 250)),
                120.0,
                48.0,
            ),
        ],
        tokens::SPACE_4,
        Align::Center,
    )
}

fn vector_tile(label: &str, asset: VectorAsset, w: f32, h: f32) -> El {
    column([
        El::new(Kind::Group)
            .padding(tokens::SPACE_2)
            .child(vector(asset).width(Size::Fixed(w)).height(Size::Fixed(h)))
            .surface_role(SurfaceRole::Sunken)
            .radius(tokens::RADIUS_MD),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn curve_asset(start_lane: i32, end_lane: i32, row_span: u32) -> VectorAsset {
    let lane_w = 24.0;
    let row_h = 24.0;
    let dx = (end_lane - start_lane) as f32 * lane_w;
    let dy = row_span as f32 * row_h;
    let path = PathBuilder::new()
        .move_to(0.0, 0.0)
        .cubic_to(0.0, dy * 0.5, dx, dy * 0.5, dx, dy)
        .stroke_solid(Color::srgb_u8(132, 204, 22), 2.0)
        .stroke_line_cap(VectorLineCap::Round)
        .build();
    VectorAsset::from_paths([0.0, 0.0, dx.abs().max(0.001), dy], vec![path])
}

fn diamond_asset(color: Color) -> VectorAsset {
    let r = 12.0;
    let path = PathBuilder::new()
        .move_to(r, 0.0)
        .line_to(2.0 * r, r)
        .line_to(r, 2.0 * r)
        .line_to(0.0, r)
        .close()
        .fill_solid(color)
        .build();
    VectorAsset::from_paths([0.0, 0.0, 2.0 * r, 2.0 * r], vec![path])
}

fn squiggle_asset(color: Color) -> VectorAsset {
    let path = PathBuilder::new()
        .move_to(0.0, 12.0)
        .quad_to(15.0, 0.0, 30.0, 12.0)
        .quad_to(45.0, 24.0, 60.0, 12.0)
        .stroke_solid(color, 2.0)
        .stroke_line_cap(VectorLineCap::Round)
        .build();
    VectorAsset::from_paths([0.0, 0.0, 60.0, 24.0], vec![path])
}

fn section_card<I: IntoIterator<Item = El>>(title: &str, blurb: &str, body: I) -> El {
    // Inline the titled_card construction so we can make the title wrap
    // — several media section titles are wider than phone-narrowed
    // cards ("Built-in lucide icons (monochrome / MSDF)" etc.).
    card([
        card_header([card_title(title).wrap_text().fill_width()]),
        card_content(
            std::iter::once(paragraph(blurb).muted().small())
                .chain(body)
                .collect::<Vec<_>>(),
        ),
    ])
}

fn tile(img: &LazyLock<Image>, label: &str) -> El {
    column([
        image((*img).clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(96.0))
            .image_fit(ImageFit::Contain)
            .radius(tokens::RADIUS_MD),
        // Cap the label to the tile width so longer labels truncate
        // rather than overflow when the tile row is squeezed on phone.
        text(label.to_string())
            .small()
            .muted()
            .width(Size::Fixed(96.0))
            .ellipsis()
            .center_text(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn tinted_avatar(tint: Color) -> El {
    image(AVATAR_SOLID.clone())
        .width(Size::Fixed(48.0))
        .height(Size::Fixed(48.0))
        .image_fit(ImageFit::Fill)
        .image_tint(tint)
        .radius(24.0)
}

fn aspect_demo() -> El {
    let w = GRID_WIDE.width() as f32;
    let h = GRID_WIDE.height() as f32;
    column([
        image(GRID_WIDE.clone())
            .width(Size::Fill(1.0))
            .height(Size::Aspect(h / w))
            .image_fit(ImageFit::Fill)
            .radius(tokens::RADIUS_MD)
            .stroke(tokens::BORDER),
        paragraph(
            "Caption text sitting below the aspect-sized image. Because the \
             image reports its derived height back to the column, this \
             paragraph lays out at exactly the right Y — no overlap, no gap.",
        )
        .muted()
        .small(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Stretch)
}

fn fit_demo(label: &str, fit: ImageFit) -> El {
    column([
        image(GRID_RG.clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(48.0))
            .image_fit(fit)
            .radius(tokens::RADIUS_SM)
            .stroke(tokens::BORDER),
        text(label.to_string())
            .small()
            .muted()
            .width(Size::Fixed(96.0))
            .ellipsis()
            .center_text(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn builtin_icon_tile(name: IconName, tint: Color) -> El {
    card([
        icon(name).icon_size(28.0).text_color(tint),
        // Tile cards in the icon grid get squeezed on phone; let the
        // label clip with ellipsis to the card's content rect.
        text(name.name())
            .small()
            .muted()
            .center_text()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
    .padding(tokens::SPACE_3)
    .radius(tokens::RADIUS_MD)
}

fn custom_icon_tile(svg: &LazyLock<SvgIcon>, label: &str, size: f32) -> El {
    card([
        icon((**svg).clone()).icon_size(size),
        text(label.to_string())
            .small()
            .muted()
            .center_text()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(tokens::SPACE_3)
    .radius(tokens::RADIUS_MD)
}

/// Three tiles, each showing the animated surface under a different
/// `SurfaceAlpha` mode against a colored backdrop. The shared
/// `AppTexture` is cheap to clone (Arc-backed); each tile owns its
/// own composite, so the same pixel data lights up the three blend
/// paths simultaneously.
fn animated_surface_demo(tex: Option<&AppTexture>) -> El {
    let Some(tex) = tex else {
        return paragraph(
            "This demo requires a host that allocates an `AppTexture` and \
             pushes a frame each tick (see `examples/src/bin/showcase.rs` \
             or `crates/aetna-ash-demo/src/lib.rs`). \
             Headless render bundles render this card as a placeholder.",
        )
        .muted()
        .small();
    };
    row([
        animated_surface_cell(
            "Premultiplied",
            tokens::PRIMARY,
            tex.clone(),
            SurfaceAlpha::Premultiplied,
        ),
        animated_surface_cell(
            "Straight",
            tokens::SECONDARY,
            tex.clone(),
            SurfaceAlpha::Straight,
        ),
        animated_surface_cell("Opaque", tokens::ACCENT, tex.clone(), SurfaceAlpha::Opaque),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Stretch)
}

fn animated_surface_cell(label: &str, backdrop: Color, tex: AppTexture, alpha: SurfaceAlpha) -> El {
    // The cell column uses the default `Align::Stretch` so the stack's
    // `Size::Fill(1.0)` width actually claims the cell's full width —
    // a `Center`-aligned column collapses Fill children to their
    // intrinsic, which is zero here since the stack's own children
    // all Fill recursively.
    column([
        text(label.to_string()).small().muted(),
        stack([
            // Backdrop — Premultiplied / Straight let it show through
            // wherever the texture has alpha < 1; Opaque overwrites.
            El::default()
                .fill(backdrop)
                .radius(tokens::RADIUS_MD)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            // The animated surface. `redraw_within(16ms)` is the
            // inside-out signal that drives the host's redraw loop:
            // while this tile is visible, Aetna asks the host for a
            // fresh frame within 16 ms, the host wakes up, and
            // before_paint writes the next animation frame. When the
            // user navigates to a different Media-page section that
            // doesn't include this tile, the request goes away and
            // the host idles automatically.
            surface(tex)
                .surface_alpha(alpha)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .redraw_within(std::time::Duration::from_millis(16)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fixed(120.0)),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}
