use std::cell::RefCell;

use niri_config::ServerTitlebar;
use pangocairo::cairo::{self, ImageSurface};
use pangocairo::pango::{self, FontDescription};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::reexports::gbm::Format as Fourcc;
use smithay::utils::{Logical, Point, Size, Transform};

use crate::layout::TitlebarButton;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::utils::to_physical_precise_round;

pub const CAPTION_BUTTON_WIDTH: f64 = 46.0;

#[derive(Debug, Clone, PartialEq)]
struct CacheKey {
    phys_w: i32,
    phys_h: i32,
    scale_bits: u64,
    is_active: bool,
    is_maximized: bool,
    hovered_button: Option<TitlebarButton>,
    title: String,
    cfg: ServerTitlebar,
}

#[derive(Debug, Default)]
pub struct ServerTitlebarCache {
    cached: RefCell<Option<(CacheKey, TextureBuffer<GlesTexture>)>>,
}

impl ServerTitlebarCache {
    pub fn render<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        location: Point<f64, Logical>,
        size: Size<f64, Logical>,
        scale: f64,
        alpha: f32,
        is_active: bool,
        is_maximized: bool,
        hovered_button: Option<TitlebarButton>,
        title: &str,
        cfg: &ServerTitlebar,
    ) -> Option<PrimaryGpuTextureRenderElement> {
        let phys_w: i32 = to_physical_precise_round(scale, size.w);
        let phys_h: i32 = to_physical_precise_round(scale, size.h);
        if phys_w <= 2 || phys_h <= 2 {
            return None;
        }

        let key = CacheKey {
            phys_w,
            phys_h,
            scale_bits: scale.to_bits(),
            is_active,
            is_maximized,
            hovered_button,
            title: title.to_owned(),
            cfg: *cfg,
        };

        let mut guard = self.cached.borrow_mut();
        let needs_rebuild = match guard.as_ref() {
            Some((existing_key, _)) => existing_key != &key,
            None => true,
        };

        if needs_rebuild {
            let Ok(buf) = rasterize_titlebar(renderer.as_gles_renderer(), scale, &key) else {
                return None;
            };
            *guard = Some((key, buf));
        }

        let (_, ref buffer) = guard.as_ref()?;
        let elem = TextureRenderElement::from_texture_buffer(
            buffer.clone(),
            location,
            alpha,
            None,
            None,
            Kind::Unspecified,
        );
        Some(PrimaryGpuTextureRenderElement(elem))
    }
}

fn rasterize_titlebar(
    renderer: &mut GlesRenderer,
    scale: f64,
    key: &CacheKey,
) -> anyhow::Result<TextureBuffer<GlesTexture>> {
    let width = key.phys_w;
    let height = key.phys_h;
    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;

    let bg = if key.is_active {
        key.cfg.active_color
    } else {
        key.cfg.inactive_color
    };
    let fg = if key.is_active {
        key.cfg.active_text_color
    } else {
        key.cfg.inactive_text_color
    };

    // 1. Titlebar background
    cr.set_source_rgba(bg.r as f64, bg.g as f64, bg.b as f64, bg.a as f64);
    cr.paint()?;

    // Subtle 1px bottom separator between titlebar and client area
    let is_light_bg = (0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b) > 0.55;
    if is_light_bg {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.08);
    } else {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
    }
    cr.rectangle(0.0, (height - 1) as f64, width as f64, 1.0);
    cr.fill()?;

    let btn_w = (CAPTION_BUTTON_WIDTH * scale).round();
    let show_min = key.cfg.allow_minimize;
    let total_btns_w = if show_min { btn_w * 3.0 } else { btn_w * 2.0 };
    let close_x = width as f64 - btn_w;
    let max_x = width as f64 - btn_w * 2.0;
    let btns_left_x = width as f64 - total_btns_w;

    // 2. Hover backgrounds for caption buttons (─ □ ✕ when We10X allows minimize; □ ✕ otherwise)
    if let Some(hovered) = key.hovered_button {
        match hovered {
            TitlebarButton::Close => {
                // Authentic Windows 10 / We10XOS #E81123 red close hover
                cr.set_source_rgba(0xe8 as f64 / 255.0, 0x11 as f64 / 255.0, 0x23 as f64 / 255.0, 1.0);
                cr.rectangle(close_x.max(0.0), 0.0, btn_w, height as f64);
                cr.fill()?;
            }
            TitlebarButton::Maximize => {
                if is_light_bg {
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.09);
                } else {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.11);
                }
                cr.rectangle(max_x.max(0.0), 0.0, btn_w, height as f64);
                cr.fill()?;
            }
            TitlebarButton::Minimize if show_min => {
                if is_light_bg {
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.09);
                } else {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.11);
                }
                cr.rectangle(btns_left_x.max(0.0), 0.0, btn_w, height as f64);
                cr.fill()?;
            }
            TitlebarButton::Minimize => {}
        }
    }

    // 3. Crisp 1px vector glyphs for Minimize (─, optional), Maximize/Restore (□/❐), Close (✕)
    let stroke_px = scale.round().max(1.0);
    let half = if (stroke_px as i32) % 2 == 1 { 0.5 } else { 0.0 };
    let icon_sz = (10.0 * scale).round();
    let cy = (height as f64 / 2.0).floor() + half;

    cr.set_line_width(stroke_px);

    if show_min && btns_left_x >= 0.0 {
        cr.set_source_rgba(fg.r as f64, fg.g as f64, fg.b as f64, fg.a as f64);
        let cx = (btns_left_x + btn_w / 2.0).floor();
        let x0 = cx - (icon_sz / 2.0).floor();
        cr.move_to(x0, cy);
        cr.line_to(x0 + icon_sz, cy);
        cr.stroke()?;
    }

    if max_x >= 0.0 {
        cr.set_source_rgba(fg.r as f64, fg.g as f64, fg.b as f64, fg.a as f64);
        let cx = (max_x + btn_w / 2.0).floor();
        if key.is_maximized {
            // Overlapping restore squares (❐)
            let r_sz = (8.0 * scale).round();
            let front_x = cx - (r_sz / 2.0).floor() - scale.round() + half;
            let front_y = (height as f64 / 2.0 - r_sz / 2.0).floor() + scale.round() + half;
            let off = (2.0 * scale).round();
            // Back window top & right edges
            cr.move_to(front_x + off, front_y);
            cr.line_to(front_x + off, front_y - off);
            cr.line_to(front_x + r_sz + off, front_y - off);
            cr.line_to(front_x + r_sz + off, front_y + r_sz - off);
            cr.line_to(front_x + r_sz, front_y + r_sz - off);
            cr.stroke()?;
            // Front window square
            cr.rectangle(front_x, front_y, r_sz, r_sz);
            cr.stroke()?;
        } else {
            let x0 = cx - (icon_sz / 2.0).floor() + half;
            let y0 = (height as f64 / 2.0 - icon_sz / 2.0).floor() + half;
            cr.rectangle(x0, y0, icon_sz, icon_sz);
            cr.stroke()?;
        }
    }

    if close_x >= 0.0 {
        if key.hovered_button == Some(TitlebarButton::Close) {
            cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        } else {
            cr.set_source_rgba(fg.r as f64, fg.g as f64, fg.b as f64, fg.a as f64);
        }
        let cx = (close_x + btn_w / 2.0).floor();
        let x0 = cx - (icon_sz / 2.0).floor() + half;
        let y0 = (height as f64 / 2.0 - icon_sz / 2.0).floor() + half;
        cr.move_to(x0, y0);
        cr.line_to(x0 + icon_sz, y0 + icon_sz);
        cr.move_to(x0 + icon_sz, y0);
        cr.line_to(x0, y0 + icon_sz);
        cr.stroke()?;
    }

    // 4. Window title text on the left
    let left_pad = (12.0 * scale).round();
    let avail_w = (btns_left_x - left_pad - (8.0 * scale).round()).floor() as i32;
    if avail_w > 16 && !key.title.is_empty() {
        let mut font = FontDescription::from_string("Noto Sans, Sans 12px");
        font.set_absolute_size(to_physical_precise_round(scale, font.size()));

        let layout = pangocairo::functions::create_layout(&cr);
        layout.context().set_round_glyph_positions(false);
        layout.set_font_description(Some(&font));
        layout.set_width(avail_w * pango::SCALE);
        layout.set_ellipsize(pango::EllipsizeMode::End);
        layout.set_single_paragraph_mode(true);
        layout.set_text(&key.title);

        let (_, text_h) = layout.pixel_size();
        let text_y = ((height - text_h) as f64 / 2.0).round();
        cr.move_to(left_pad, text_y);
        cr.set_source_rgba(fg.r as f64, fg.g as f64, fg.b as f64, fg.a as f64);
        pangocairo::functions::show_layout(&cr, &layout);
    }

    drop(cr);
    let data = surface.take_data()?;
    let buffer = TextureBuffer::from_memory(
        renderer,
        &data,
        Fourcc::Argb8888,
        (width, height),
        false,
        scale,
        Transform::Normal,
        Vec::new(),
    )?;
    Ok(buffer)
}
