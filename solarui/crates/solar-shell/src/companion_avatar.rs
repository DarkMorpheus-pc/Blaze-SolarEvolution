// ==============================================================================
// Solar Omnibar Vector Companion ("Persephone" / "Köz")
// - Pure Cairo flat/matte vector rendering for 8 shapes & 16 expressions
// - Smooth cursor-tracking eye offset (damped max 5.5px radial shift)
// - Natural 4.2s blink pulse (120ms, zero continuous idle tick -> 0% idle CPU)
// - Reactive state overrides (Curious while waiting, Happy/Confused 2.5s)
// - Graphite + Blaze Ember GtkPopover customizer
// ==============================================================================

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, DrawingArea, Entry, Grid, Label, Orientation, Popover,
    ScrolledWindow, Window,
};
use solar_common::{
    CompanionCharacter, CompanionExpression, CompanionShape, SolarConfig, COMPANION_PALETTE,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::{PI, TAU};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompanionRenderParams {
    pub shape: CompanionShape,
    pub expression: CompanionExpression,
    pub color_hex: &'static str,
    pub gaze_x: f64,
    pub gaze_y: f64,
    pub is_blinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpressionEyeDescriptor {
    pub left_height_factor: f64,
    pub right_height_factor: f64,
    pub left_width_factor: f64,
    pub right_width_factor: f64,
    pub is_arc: bool,
    pub tilt_deg: f64,
}

/// Returns pure geometric descriptor for each of the 16 expressions so geometry
/// uniqueness and invariants are verifiable in headless tests.
pub fn expression_eye_descriptor(expr: CompanionExpression) -> ExpressionEyeDescriptor {
    match expr {
        CompanionExpression::Neutral => ExpressionEyeDescriptor {
            left_height_factor: 0.85,
            right_height_factor: 0.85,
            left_width_factor: 0.85,
            right_width_factor: 0.85,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Attentive => ExpressionEyeDescriptor {
            left_height_factor: 1.0,
            right_height_factor: 1.0,
            left_width_factor: 0.95,
            right_width_factor: 0.95,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Surprised => ExpressionEyeDescriptor {
            left_height_factor: 0.72,
            right_height_factor: 0.72,
            left_width_factor: 1.45,
            right_width_factor: 1.45,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Excited => ExpressionEyeDescriptor {
            left_height_factor: 1.15,
            right_height_factor: 1.15,
            left_width_factor: 1.15,
            right_width_factor: 1.15,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Happy => ExpressionEyeDescriptor {
            left_height_factor: 0.55,
            right_height_factor: 0.55,
            left_width_factor: 1.25,
            right_width_factor: 1.25,
            is_arc: true,
            tilt_deg: 0.0,
        },
        CompanionExpression::Laughing => ExpressionEyeDescriptor {
            left_height_factor: 0.65,
            right_height_factor: 0.65,
            left_width_factor: 1.30,
            right_width_factor: 1.30,
            is_arc: true,
            tilt_deg: 4.0,
        },
        CompanionExpression::Angry => ExpressionEyeDescriptor {
            left_height_factor: 0.92,
            right_height_factor: 0.92,
            left_width_factor: 1.10,
            right_width_factor: 1.10,
            is_arc: false,
            tilt_deg: 18.0,
        },
        CompanionExpression::Sad => ExpressionEyeDescriptor {
            left_height_factor: 0.84,
            right_height_factor: 0.84,
            left_width_factor: 1.08,
            right_width_factor: 1.08,
            is_arc: false,
            tilt_deg: -17.0,
        },
        CompanionExpression::Scared => ExpressionEyeDescriptor {
            left_height_factor: 0.90,
            right_height_factor: 0.90,
            left_width_factor: 0.72,
            right_width_factor: 0.72,
            is_arc: false,
            tilt_deg: -7.0,
        },
        CompanionExpression::Suspicious => ExpressionEyeDescriptor {
            left_height_factor: 0.30,
            right_height_factor: 0.62,
            left_width_factor: 1.25,
            right_width_factor: 1.10,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Confused => ExpressionEyeDescriptor {
            left_height_factor: 0.58,
            right_height_factor: 0.94,
            left_width_factor: 0.88,
            right_width_factor: 1.02,
            is_arc: false,
            tilt_deg: -6.0,
        },
        CompanionExpression::Curious => ExpressionEyeDescriptor {
            left_height_factor: 0.68,
            right_height_factor: 1.06,
            left_width_factor: 0.82,
            right_width_factor: 1.14,
            is_arc: false,
            tilt_deg: 5.0,
        },
        CompanionExpression::Proud => ExpressionEyeDescriptor {
            left_height_factor: 0.44,
            right_height_factor: 0.44,
            left_width_factor: 1.20,
            right_width_factor: 1.20,
            is_arc: true,
            tilt_deg: -3.0,
        },
        CompanionExpression::Shy => ExpressionEyeDescriptor {
            left_height_factor: 0.58,
            right_height_factor: 0.58,
            left_width_factor: 0.70,
            right_width_factor: 0.70,
            is_arc: false,
            tilt_deg: 3.0,
        },
        CompanionExpression::Unimpressed => ExpressionEyeDescriptor {
            left_height_factor: 0.26,
            right_height_factor: 0.26,
            left_width_factor: 1.32,
            right_width_factor: 1.32,
            is_arc: false,
            tilt_deg: 0.0,
        },
        CompanionExpression::Sleepy => ExpressionEyeDescriptor {
            left_height_factor: 0.42,
            right_height_factor: 0.42,
            left_width_factor: 1.18,
            right_width_factor: 1.18,
            is_arc: false,
            tilt_deg: 1.0,
        },
    }
}

/// Parses `#RRGGBB` into normalized `(r, g, b)` floats, falling back to `#1e1e22`.
pub fn parse_hex_rgb(hex: &str) -> (f64, f64, f64) {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&clean[0..2], 16),
            u8::from_str_radix(&clean[2..4], 16),
            u8::from_str_radix(&clean[4..6], 16),
        ) {
            return (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        }
    }
    (0x1e as f64 / 255.0, 0x1e as f64 / 255.0, 0x22 as f64 / 255.0)
}

/// Returns relative luminance in `[0.0, 1.0]`.
pub fn relative_luminance(hex: &str) -> f64 {
    let (r, g, b) = parse_hex_rgb(hex);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// High-contrast eye color (`#141415` on light surfaces such as `#e4e4e7`,
/// `#f4f4f6` on dark or vivid surfaces).
pub fn eye_color_for_body(hex: &str) -> (f64, f64, f64) {
    if relative_luminance(hex) > 0.65 {
        (0x14 as f64 / 255.0, 0x14 as f64 / 255.0, 0x15 as f64 / 255.0)
    } else {
        (0xf4 as f64 / 255.0, 0xf4 as f64 / 255.0, 0xf6 as f64 / 255.0)
    }
}

/// Computes damped radial cursor-tracking offset strictly within `max_offset` pixels.
pub fn compute_gaze_offset(
    pointer_x: f64,
    pointer_y: f64,
    avatar_cx: f64,
    avatar_cy: f64,
    max_offset: f64,
) -> (f64, f64) {
    if !pointer_x.is_finite()
        || !pointer_y.is_finite()
        || !avatar_cx.is_finite()
        || !avatar_cy.is_finite()
        || !max_offset.is_finite()
        || max_offset <= 0.0
    {
        return (0.0, 0.0);
    }

    let dx = pointer_x - avatar_cx;
    let dy = pointer_y - avatar_cy;
    let dist = dx.hypot(dy);
    if dist < 1e-6 {
        return (0.0, 0.0);
    }

    let damped_mag = max_offset * (dist / (dist + 64.0));
    let scale = damped_mag / dist;
    (dx * scale, dy * scale)
}

fn build_rounded_polygon_path(cr: &cairo::Context, pts: &[(f64, f64)], corner_radius: f64) {
    if pts.len() < 3 {
        return;
    }
    let n = pts.len();
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let curr = pts[i];
        let next = pts[(i + 1) % n];

        let v_prev = (prev.0 - curr.0, prev.1 - curr.1);
        let v_next = (next.0 - curr.0, next.1 - curr.1);
        let len_prev = v_prev.0.hypot(v_prev.1).max(1e-6);
        let len_next = v_next.0.hypot(v_next.1).max(1e-6);

        let trim = corner_radius.min(len_prev * 0.45).min(len_next * 0.45);
        let start = (
            curr.0 + (v_prev.0 / len_prev) * trim,
            curr.1 + (v_prev.1 / len_prev) * trim,
        );
        let end = (
            curr.0 + (v_next.0 / len_next) * trim,
            curr.1 + (v_next.1 / len_next) * trim,
        );

        if i == 0 {
            cr.move_to(start.0, start.1);
        } else {
            cr.line_to(start.0, start.1);
        }

        // Convert quadratic Bezier (start -> curr -> end) into exact cubic Bezier.
        let cp1 = (
            start.0 + (2.0 / 3.0) * (curr.0 - start.0),
            start.1 + (2.0 / 3.0) * (curr.1 - start.1),
        );
        let cp2 = (
            end.0 + (2.0 / 3.0) * (curr.0 - end.0),
            end.1 + (2.0 / 3.0) * (curr.1 - end.1),
        );
        cr.curve_to(cp1.0, cp1.1, cp2.0, cp2.1, end.0, end.1);
    }
    cr.close_path();
}

/// Builds the continuous closed Cairo path for any of the 8 companion body shapes.
pub fn trace_companion_shape_path(
    cr: &cairo::Context,
    shape: CompanionShape,
    cx: f64,
    cy: f64,
    radius: f64,
) {
    cr.new_path();
    let r = radius.max(2.0);
    match shape {
        CompanionShape::Circle => {
            cr.arc(cx, cy, r, 0.0, TAU);
            cr.close_path();
        }
        CompanionShape::Pebble => {
            let p0 = (cx - 0.08 * r, cy - 0.94 * r);
            let p1 = (cx + 0.96 * r, cy + 0.02 * r);
            let p2 = (cx + 0.02 * r, cy + 0.96 * r);
            let p3 = (cx - 0.96 * r, cy + 0.08 * r);
            cr.move_to(p0.0, p0.1);
            cr.curve_to(
                cx + 0.56 * r,
                cy - 0.96 * r,
                cx + 0.98 * r,
                cy - 0.52 * r,
                p1.0,
                p1.1,
            );
            cr.curve_to(
                cx + 0.94 * r,
                cy + 0.58 * r,
                cx + 0.54 * r,
                cy + 0.98 * r,
                p2.0,
                p2.1,
            );
            cr.curve_to(
                cx - 0.56 * r,
                cy + 0.94 * r,
                cx - 1.00 * r,
                cy + 0.60 * r,
                p3.0,
                p3.1,
            );
            cr.curve_to(
                cx - 0.92 * r,
                cy - 0.50 * r,
                cx - 0.62 * r,
                cy - 0.92 * r,
                p0.0,
                p0.1,
            );
            cr.close_path();
        }
        CompanionShape::Squircle => {
            let a = r * 0.94;
            let steps = 48;
            for i in 0..steps {
                let t = (i as f64 / steps as f64) * TAU;
                let cos_t = t.cos();
                let sin_t = t.sin();
                let exp = 2.0 / 3.8;
                let x = cx + a * cos_t.abs().powf(exp) * cos_t.signum();
                let y = cy + a * sin_t.abs().powf(exp) * sin_t.signum();
                if i == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }
            cr.close_path();
        }
        CompanionShape::Capsule => {
            let half_w = r * 1.06;
            let half_h = r * 0.70;
            let cap_dx = half_w - half_h;
            cr.arc(cx - cap_dx, cy, half_h, PI * 0.5, PI * 1.5);
            cr.arc(cx + cap_dx, cy, half_h, -PI * 0.5, PI * 0.5);
            cr.close_path();
        }
        CompanionShape::Triangle => {
            let pts = [
                (cx, cy - r * 0.94),
                (cx + r * 0.96, cy + r * 0.74),
                (cx - r * 0.96, cy + r * 0.74),
            ];
            build_rounded_polygon_path(cr, &pts, r * 0.30);
        }
        CompanionShape::Hexagon => {
            let mut pts = Vec::with_capacity(6);
            for i in 0..6 {
                let ang = (i as f64) * (TAU / 6.0);
                pts.push((cx + r * 1.02 * ang.cos(), cy + r * 0.96 * ang.sin()));
            }
            build_rounded_polygon_path(cr, &pts, r * 0.22);
        }
        CompanionShape::Cloud => {
            let p_bl = (cx - 0.56 * r, cy + 0.70 * r);
            let p_l = (cx - 0.76 * r, cy - 0.14 * r);
            let p_top = (cx + 0.02 * r, cy - 0.82 * r);
            let p_r = (cx + 0.74 * r, cy - 0.08 * r);
            let p_br = (cx + 0.56 * r, cy + 0.70 * r);
            cr.move_to(p_bl.0, p_bl.1);
            cr.curve_to(
                cx - 1.06 * r,
                cy + 0.68 * r,
                cx - 1.10 * r,
                cy + 0.04 * r,
                p_l.0,
                p_l.1,
            );
            cr.curve_to(
                cx - 0.66 * r,
                cy - 0.74 * r,
                cx - 0.28 * r,
                cy - 0.86 * r,
                p_top.0,
                p_top.1,
            );
            cr.curve_to(
                cx + 0.44 * r,
                cy - 0.80 * r,
                cx + 0.82 * r,
                cy - 0.44 * r,
                p_r.0,
                p_r.1,
            );
            cr.curve_to(
                cx + 1.10 * r,
                cy + 0.14 * r,
                cx + 1.02 * r,
                cy + 0.70 * r,
                p_br.0,
                p_br.1,
            );
            cr.curve_to(
                cx + 0.20 * r,
                cy + 0.74 * r,
                cx - 0.20 * r,
                cy + 0.74 * r,
                p_bl.0,
                p_bl.1,
            );
            cr.close_path();
        }
        CompanionShape::Droplet => {
            let top = (cx, cy - 0.94 * r);
            let belly_cy = cy + 0.16 * r;
            let belly_r = 0.78 * r;
            cr.move_to(top.0, top.1);
            cr.curve_to(
                cx + 0.18 * r,
                cy - 0.72 * r,
                cx + belly_r * 1.04,
                cy - 0.24 * r,
                cx + belly_r,
                belly_cy,
            );
            cr.arc(cx, belly_cy, belly_r, 0.0, PI);
            cr.curve_to(
                cx - belly_r * 1.04,
                cy - 0.24 * r,
                cx - 0.18 * r,
                cy - 0.72 * r,
                top.0,
                top.1,
            );
            cr.close_path();
        }
    }
}

/// Draws the vector body fill and 1px hairline border.
pub fn draw_companion_body(
    cr: &cairo::Context,
    shape: CompanionShape,
    color_hex: &str,
    width: f64,
    height: f64,
) -> (f64, f64, f64) {
    let cx = width * 0.5;
    let cy = height * 0.5;
    let r = (width.min(height) * 0.42).max(4.0);

    trace_companion_shape_path(cr, shape, cx, cy, r);

    let (br, bg, bb) = parse_hex_rgb(color_hex);
    cr.set_source_rgb(br, bg, bb);
    let _ = cr.fill_preserve();

    if relative_luminance(color_hex) > 0.65 {
        cr.set_source_rgba(0.08, 0.08, 0.09, 0.24);
    } else {
        cr.set_source_rgba(0.93, 0.93, 0.93, 0.16);
    }
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    (cx, cy, r)
}

fn fill_pill(cr: &cairo::Context, cx: f64, cy: f64, w: f64, h: f64, tilt_rad: f64) {
    let _ = cr.save();
    cr.translate(cx, cy);
    if tilt_rad.abs() > 1e-6 {
        cr.rotate(tilt_rad);
    }
    let hw = (w * 0.5).max(0.5);
    let hh = (h * 0.5).max(0.5);
    let rad = hw.min(hh);
    cr.new_path();
    cr.arc(hw - rad, -hh + rad, rad, -PI * 0.5, 0.0);
    cr.arc(hw - rad, hh - rad, rad, 0.0, PI * 0.5);
    cr.arc(-hw + rad, hh - rad, rad, PI * 0.5, PI);
    cr.arc(-hw + rad, -hh + rad, rad, PI, PI * 1.5);
    cr.close_path();
    let _ = cr.fill();
    let _ = cr.restore();
}

fn fill_ellipse(cr: &cairo::Context, cx: f64, cy: f64, rx: f64, ry: f64, tilt_rad: f64) {
    let _ = cr.save();
    cr.translate(cx, cy);
    if tilt_rad.abs() > 1e-6 {
        cr.rotate(tilt_rad);
    }
    cr.scale(rx.max(0.5), ry.max(0.5));
    cr.new_path();
    cr.arc(0.0, 0.0, 1.0, 0.0, TAU);
    let _ = cr.fill();
    let _ = cr.restore();
}

fn fill_slanted_lid_eye(
    cr: &cairo::Context,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    lid_tilt_rad: f64,
    lid_offset_y: f64,
) {
    let _ = cr.save();
    cr.translate(cx, cy);
    let _ = cr.save();
    cr.rotate(lid_tilt_rad);
    cr.rectangle(-rx * 2.0, lid_offset_y, rx * 4.0, ry * 3.0);
    cr.clip();
    let _ = cr.restore();

    cr.scale(rx.max(0.5), ry.max(0.5));
    cr.new_path();
    cr.arc(0.0, 0.0, 1.0, 0.0, TAU);
    let _ = cr.fill();
    let _ = cr.restore();
}

fn stroke_smile_arc(
    cr: &cairo::Context,
    cx: f64,
    cy: f64,
    w: f64,
    arch_h: f64,
    stroke_w: f64,
    tilt_rad: f64,
) {
    let _ = cr.save();
    cr.translate(cx, cy);
    if tilt_rad.abs() > 1e-6 {
        cr.rotate(tilt_rad);
    }
    cr.set_line_width(stroke_w.max(1.2));
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.new_path();
    let hw = w * 0.5;
    cr.move_to(-hw, arch_h * 0.35);
    cr.curve_to(
        -hw * 0.45,
        -arch_h * 0.85,
        hw * 0.45,
        -arch_h * 0.85,
        hw,
        arch_h * 0.35,
    );
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// Draws the expressive vector eyes for any of the 16 expressions (or blink frame).
pub fn draw_companion_eyes(
    cr: &cairo::Context,
    expr: CompanionExpression,
    color_hex: &str,
    cx: f64,
    cy: f64,
    radius: f64,
    gaze_x: f64,
    gaze_y: f64,
    is_blinking: bool,
) {
    let (er, eg, eb) = eye_color_for_body(color_hex);
    cr.set_source_rgb(er, eg, eb);

    let s = radius / 22.0;
    let base_dx = 5.2 * s;
    let ex_l = cx + gaze_x - base_dx;
    let ex_r = cx + gaze_x + base_dx;
    let ey = cy + gaze_y - 1.4 * s;

    if is_blinking {
        let blink_w = 4.8 * s;
        let blink_h = (2.2 * s).max(2.0);
        fill_pill(cr, ex_l, ey + 0.4 * s, blink_w, blink_h, 0.0);
        fill_pill(cr, ex_r, ey + 0.4 * s, blink_w, blink_h, 0.0);
        return;
    }

    match expr {
        CompanionExpression::Neutral => {
            fill_pill(cr, ex_l, ey, 3.3 * s, 8.2 * s, 0.0);
            fill_pill(cr, ex_r, ey, 3.3 * s, 8.2 * s, 0.0);
        }
        CompanionExpression::Attentive => {
            fill_pill(cr, ex_l, ey, 3.7 * s, 9.6 * s, 0.0);
            fill_pill(cr, ex_r, ey, 3.7 * s, 9.6 * s, 0.0);
        }
        CompanionExpression::Surprised => {
            let wide_dx = 5.6 * s;
            fill_ellipse(cr, cx + gaze_x - wide_dx, ey - 0.8 * s, 3.1 * s, 3.3 * s, 0.0);
            fill_ellipse(cr, cx + gaze_x + wide_dx, ey - 0.8 * s, 3.1 * s, 3.3 * s, 0.0);
        }
        CompanionExpression::Excited => {
            fill_ellipse(cr, ex_l, ey - 0.5 * s, 2.3 * s, 5.2 * s, -0.04);
            fill_ellipse(cr, ex_r, ey - 0.5 * s, 2.3 * s, 5.2 * s, 0.04);
        }
        CompanionExpression::Happy => {
            stroke_smile_arc(cr, ex_l, ey, 5.0 * s, 3.2 * s, 2.1 * s, 0.0);
            stroke_smile_arc(cr, ex_r, ey, 5.0 * s, 3.2 * s, 2.1 * s, 0.0);
        }
        CompanionExpression::Laughing => {
            let _ = cr.save();
            cr.set_line_width((2.3 * s).max(1.3));
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            for ex in [ex_l, ex_r] {
                cr.new_path();
                cr.move_to(ex - 2.6 * s, ey + 1.2 * s);
                cr.line_to(ex, ey - 1.9 * s);
                cr.line_to(ex + 2.6 * s, ey + 1.2 * s);
                let _ = cr.stroke();
            }
            let _ = cr.restore();
        }
        CompanionExpression::Angry => {
            fill_slanted_lid_eye(cr, ex_l, ey + 0.4 * s, 2.4 * s, 4.4 * s, 0.34, -0.8 * s);
            fill_slanted_lid_eye(cr, ex_r, ey + 0.4 * s, 2.4 * s, 4.4 * s, -0.34, -0.8 * s);
        }
        CompanionExpression::Sad => {
            fill_slanted_lid_eye(cr, ex_l, ey + 0.8 * s, 2.4 * s, 4.0 * s, -0.32, -0.6 * s);
            fill_slanted_lid_eye(cr, ex_r, ey + 0.8 * s, 2.4 * s, 4.0 * s, 0.32, -0.6 * s);
        }
        CompanionExpression::Scared => {
            fill_ellipse(cr, ex_l, ey - 0.6 * s, 1.7 * s, 4.2 * s, -0.14);
            fill_ellipse(cr, ex_r, ey - 0.6 * s, 1.7 * s, 4.2 * s, 0.14);
        }
        CompanionExpression::Suspicious => {
            fill_pill(cr, ex_l, ey + 0.4 * s, 4.8 * s, 2.2 * s, 0.0);
            fill_slanted_lid_eye(cr, ex_r, ey + 0.2 * s, 2.4 * s, 3.2 * s, -0.06, -0.5 * s);
        }
        CompanionExpression::Confused => {
            fill_pill(cr, ex_l, ey + 1.1 * s, 3.3 * s, 5.4 * s, -0.08);
            fill_ellipse(cr, ex_r, ey - 1.1 * s, 2.1 * s, 4.3 * s, 0.04);
        }
        CompanionExpression::Curious => {
            fill_ellipse(cr, ex_l, ey + 0.3 * s, 1.8 * s, 3.3 * s, 0.04);
            fill_ellipse(cr, ex_r, ey - 0.7 * s, 2.4 * s, 4.8 * s, 0.08);
        }
        CompanionExpression::Proud => {
            stroke_smile_arc(cr, ex_l, ey - 0.2 * s, 5.0 * s, 2.3 * s, 2.1 * s, -0.08);
            stroke_smile_arc(cr, ex_r, ey - 0.2 * s, 5.0 * s, 2.3 * s, 2.1 * s, 0.08);
        }
        CompanionExpression::Shy => {
            let shy_dx = 4.3 * s;
            fill_pill(cr, cx + gaze_x - shy_dx, ey + 1.9 * s, 2.6 * s, 5.4 * s, 0.06);
            fill_pill(cr, cx + gaze_x + shy_dx, ey + 1.9 * s, 2.6 * s, 5.4 * s, -0.06);
        }
        CompanionExpression::Unimpressed => {
            fill_pill(cr, ex_l, ey + 0.5 * s, 5.0 * s, 2.2 * s, 0.0);
            fill_pill(cr, ex_r, ey + 0.5 * s, 5.0 * s, 2.2 * s, 0.0);
        }
        CompanionExpression::Sleepy => {
            fill_slanted_lid_eye(cr, ex_l, ey + 0.7 * s, 2.7 * s, 3.4 * s, 0.04, 0.0);
            fill_slanted_lid_eye(cr, ex_r, ey + 0.7 * s, 2.7 * s, 3.4 * s, -0.04, 0.0);
        }
    }
}

/// Draws round scholar spectacles for Stolas (subtle translucent lenses, diagonal glint,
/// curved nose bridge, and side temple stems) adapting contrast to dark vs light body colors.
pub fn draw_companion_round_glasses(
    cr: &cairo::Context,
    color_hex: &str,
    cx: f64,
    cy: f64,
    radius: f64,
    gaze_x: f64,
    gaze_y: f64,
    draw_rims: bool,
) {
    let s = radius / 22.0;
    let gx = gaze_x * 0.48;
    let gy = gaze_y * 0.48;
    let lens_dx = 5.55 * s;
    let lens_r = 4.65 * s;
    let ly = cy + gy - 1.3 * s;
    let lx = cx + gx - lens_dx;
    let rx = cx + gx + lens_dx;
    let light_body = relative_luminance(color_hex) > 0.65;

    if !draw_rims {
        // Soft lens tint behind/under rims before eyes so lenses have depth
        let _ = cr.save();
        if light_body {
            cr.set_source_rgba(0.08, 0.09, 0.14, 0.08);
        } else {
            cr.set_source_rgba(0.96, 0.97, 1.0, 0.09);
        }
        cr.new_path();
        cr.arc(lx, ly, lens_r, 0.0, TAU);
        let _ = cr.fill();
        cr.new_path();
        cr.arc(rx, ly, lens_r, 0.0, TAU);
        let _ = cr.fill();
        let _ = cr.restore();
        return;
    }

    let _ = cr.save();
    // Subtle diagonal glass highlight inside each round lens
    if light_body {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    } else {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.24);
    }
    cr.set_line_width((0.95 * s).max(0.8));
    cr.set_line_cap(cairo::LineCap::Round);
    for center_x in [lx, rx] {
        cr.new_path();
        cr.move_to(center_x - 2.3 * s, ly - 1.2 * s);
        cr.line_to(center_x - 0.9 * s, ly - 2.6 * s);
        let _ = cr.stroke();
    }

    // Round spectacle rims + curved bridge + short side stems
    if light_body {
        cr.set_source_rgba(0.14, 0.14, 0.17, 0.94);
    } else {
        // Warm scholarly gold/ivory frame
        cr.set_source_rgba(0.96, 0.85, 0.56, 0.95);
    }
    cr.set_line_width((1.42 * s).max(1.05));
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.new_path();
    cr.arc(lx, ly, lens_r, 0.0, TAU);
    let _ = cr.stroke();

    cr.new_path();
    cr.arc(rx, ly, lens_r, 0.0, TAU);
    let _ = cr.stroke();

    // Nose bridge arch connecting inner rims
    let inner_gap_l = lx + lens_r * 0.96;
    let inner_gap_r = rx - lens_r * 0.96;
    cr.new_path();
    cr.move_to(inner_gap_l, ly - 0.55 * s);
    cr.curve_to(
        cx + gx - 0.45 * s,
        ly - 1.55 * s,
        cx + gx + 0.45 * s,
        ly - 1.55 * s,
        inner_gap_r,
        ly - 0.55 * s,
    );
    let _ = cr.stroke();

    // Temple stems extending slightly toward outer body edge
    cr.new_path();
    cr.move_to(lx - lens_r, ly - 0.35 * s);
    cr.line_to(lx - lens_r - 2.4 * s, ly - 0.95 * s);
    let _ = cr.stroke();

    cr.new_path();
    cr.move_to(rx + lens_r, ly - 0.35 * s);
    cr.line_to(rx + lens_r + 2.4 * s, ly - 0.95 * s);
    let _ = cr.stroke();

    let _ = cr.restore();
}

/// Complete vector draw routine supporting optional round spectacles (Stolas).
pub fn render_companion_with_accessories(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    shape: CompanionShape,
    expr: CompanionExpression,
    color_hex: &str,
    gaze_x: f64,
    gaze_y: f64,
    is_blinking: bool,
    wears_round_glasses: bool,
) {
    let (cx, cy, r) = draw_companion_body(cr, shape, color_hex, width, height);
    if wears_round_glasses {
        draw_companion_round_glasses(cr, color_hex, cx, cy, r, gaze_x, gaze_y, false);
        let eye_r = r * 0.84;
        draw_companion_eyes(
            cr,
            expr,
            color_hex,
            cx,
            cy,
            eye_r,
            gaze_x * 0.62,
            gaze_y * 0.62,
            is_blinking,
        );
        draw_companion_round_glasses(cr, color_hex, cx, cy, r, gaze_x, gaze_y, true);
    } else {
        draw_companion_eyes(cr, expr, color_hex, cx, cy, r, gaze_x, gaze_y, is_blinking);
    }
}

/// Complete vector draw routine for both full-size avatar and mini customizer previews.
pub fn render_companion(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    shape: CompanionShape,
    expr: CompanionExpression,
    color_hex: &str,
    gaze_x: f64,
    gaze_y: f64,
    is_blinking: bool,
) {
    render_companion_with_accessories(
        cr,
        width,
        height,
        shape,
        expr,
        color_hex,
        gaze_x,
        gaze_y,
        is_blinking,
        false,
    );
}

#[derive(Debug)]
struct CompanionAvatarState {
    shape: CompanionShape,
    base_expression: CompanionExpression,
    temporary_expression: Option<(CompanionExpression, Instant)>,
    color_hex: String,
    wears_glasses: bool,
    pointer_pos: Option<(f64, f64)>,
    is_blinking: bool,
    blink_generation: u64,
    temp_generation: u64,
}

impl CompanionAvatarState {
    fn effective_expression(&mut self) -> CompanionExpression {
        if let Some((expr, until)) = self.temporary_expression {
            if Instant::now() < until {
                return expr;
            }
            self.temporary_expression = None;
        }
        self.base_expression
    }
}

#[derive(Clone)]
pub struct CompanionAvatarWidget {
    area: DrawingArea,
    state: Rc<RefCell<CompanionAvatarState>>,
    window_ref: Rc<RefCell<Option<glib::WeakRef<Window>>>>,
}

impl CompanionAvatarWidget {
    pub fn new(
        shape: CompanionShape,
        expression: CompanionExpression,
        color_hex: &str,
        size: i32,
    ) -> Self {
        Self::new_with_glasses(shape, expression, color_hex, false, size)
    }

    pub fn new_with_glasses(
        shape: CompanionShape,
        expression: CompanionExpression,
        color_hex: &str,
        wears_glasses: bool,
        size: i32,
    ) -> Self {
        let area = DrawingArea::new();
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_size_request(size, size);
        area.set_halign(gtk4::Align::Center);
        area.set_valign(gtk4::Align::Center);
        area.add_css_class("companion-avatar-canvas");

        let state = Rc::new(RefCell::new(CompanionAvatarState {
            shape,
            base_expression: expression,
            temporary_expression: None,
            color_hex: color_hex.to_string(),
            wears_glasses,
            pointer_pos: None,
            is_blinking: false,
            blink_generation: 0,
            temp_generation: 0,
        }));
        let window_ref: Rc<RefCell<Option<glib::WeakRef<Window>>>> = Rc::new(RefCell::new(None));

        let st_draw = state.clone();
        let win_draw = window_ref.clone();
        area.set_draw_func(move |widget, cr, w, h| {
            let wf = w as f64;
            let hf = h as f64;
            let mut st = st_draw.borrow_mut();
            let expr = st.effective_expression();
            let max_gaze = 5.5 * (wf.min(hf) / 52.0);
            let (gx, gy) = if let Some((mx, my)) = st.pointer_pos {
                let mut center = (wf * 0.5, hf * 0.5);
                if let Some(Some(win)) = win_draw.borrow().as_ref().map(|wr| wr.upgrade()) {
                    let pt = gtk4::graphene::Point::new((wf * 0.5) as f32, (hf * 0.5) as f32);
                    if let Some(mapped) = widget.compute_point(&win, &pt) {
                        center = (mapped.x() as f64, mapped.y() as f64);
                    }
                }
                compute_gaze_offset(mx, my, center.0, center.1, max_gaze)
            } else {
                (0.0, 0.0)
            };

            render_companion_with_accessories(
                cr,
                wf,
                hf,
                st.shape,
                expr,
                &st.color_hex,
                gx,
                gy,
                st.is_blinking,
                st.wears_glasses,
            );
        });

        // Natural 4.2s blink pulse (120ms closed frame, zero continuous idle tick)
        let area_blink = area.downgrade();
        let st_blink = state.clone();
        glib::timeout_add_local(Duration::from_millis(4200), move || {
            let Some(widget) = area_blink.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !widget.is_visible() {
                return glib::ControlFlow::Continue;
            }
            let gen = {
                let mut st = st_blink.borrow_mut();
                st.is_blinking = true;
                st.blink_generation = st.blink_generation.wrapping_add(1);
                st.blink_generation
            };
            widget.queue_draw();

            let area_unblink = area_blink.clone();
            let st_unblink = st_blink.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || {
                let mut st = st_unblink.borrow_mut();
                if st.blink_generation == gen {
                    st.is_blinking = false;
                    if let Some(w) = area_unblink.upgrade() {
                        w.queue_draw();
                    }
                }
            });
            glib::ControlFlow::Continue
        });

        Self {
            area,
            state,
            window_ref,
        }
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    pub fn bind_window(&self, window: &Window) {
        *self.window_ref.borrow_mut() = Some(window.downgrade());
    }

    pub fn update_pointer(&self, x: f64, y: f64) {
        self.state.borrow_mut().pointer_pos = Some((x, y));
        self.area.queue_draw();
    }

    pub fn clear_pointer(&self) {
        self.state.borrow_mut().pointer_pos = None;
        self.area.queue_draw();
    }

    pub fn set_shape(&self, shape: CompanionShape) {
        self.state.borrow_mut().shape = shape;
        self.area.queue_draw();
    }

    pub fn set_expression(&self, expr: CompanionExpression) {
        let mut st = self.state.borrow_mut();
        st.base_expression = expr;
        st.temporary_expression = None;
        drop(st);
        self.area.queue_draw();
    }

    pub fn set_color(&self, hex: &str) {
        self.state.borrow_mut().color_hex = hex.to_string();
        self.area.queue_draw();
    }

    pub fn set_glasses(&self, wears_glasses: bool) {
        self.state.borrow_mut().wears_glasses = wears_glasses;
        self.area.queue_draw();
    }

    pub fn set_waiting(&self, waiting: bool) {
        if waiting {
            self.set_temporary_expression(CompanionExpression::Curious, Duration::from_secs(60));
        } else {
            let mut st = self.state.borrow_mut();
            st.temporary_expression = None;
            st.temp_generation = st.temp_generation.wrapping_add(1);
            drop(st);
            self.area.queue_draw();
        }
    }

    pub fn pulse_temporary_expression(&self, expr: CompanionExpression, duration_ms: u64) {
        self.set_temporary_expression(expr, Duration::from_millis(duration_ms));
    }

    fn set_temporary_expression(&self, expr: CompanionExpression, duration: Duration) {
        let gen = {
            let mut st = self.state.borrow_mut();
            st.temporary_expression = Some((expr, Instant::now() + duration));
            st.temp_generation = st.temp_generation.wrapping_add(1);
            st.temp_generation
        };
        self.area.queue_draw();

        let area_weak = self.area.downgrade();
        let st_ref = self.state.clone();
        glib::timeout_add_local_once(duration, move || {
            let mut st = st_ref.borrow_mut();
            if st.temp_generation == gen {
                st.temporary_expression = None;
                if let Some(w) = area_weak.upgrade() {
                    w.queue_draw();
                }
            }
        });
    }
}

pub struct CompanionCustomizerCallbacks {
    pub on_appearance_changed: Rc<dyn Fn(&solar_common::OmnibarConfig)>,
    pub on_persona_transition: Rc<dyn Fn(&str, &str, CompanionExpression)>,
}

/// Builds the Graphite + Blaze Ember `GtkPopover` for editing companion name,
/// 8 vector shapes (`4x2`), 16 expressions (`4x4`), and 12 flat colors (`6x2`).
pub fn build_companion_customizer_popover(
    parent_btn: &Button,
    avatar: &CompanionAvatarWidget,
    callbacks: CompanionCustomizerCallbacks,
) -> Popover {
    let popover = Popover::new();
    popover.set_parent(parent_btn);
    popover.set_has_arrow(true);
    popover.add_css_class("companion-popover");

    let cfg = SolarConfig::load();
    let active_shape = Rc::new(Cell::new(cfg.omnibar.companion_shape));
    let active_expr = Rc::new(Cell::new(cfg.omnibar.companion_expression));
    let active_color = Rc::new(RefCell::new(cfg.omnibar.companion_color.clone()));
    let active_glasses = Rc::new(Cell::new(cfg.omnibar.wears_round_glasses()));
    let programmatic_name_edit = Rc::new(Cell::new(false));

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(460)
        .min_content_width(336)
        .build();

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.add_css_class("companion-popover-box");

    // 0. Official 6-Character Roster (3x2 grid: Persephone, Köz, Solar, Stolas, Kıvılcım, Sis)
    let roster_hdr = Label::new(Some("Hazır karakterler"));
    roster_hdr.add_css_class("companion-section-lbl");
    roster_hdr.set_halign(gtk4::Align::Start);
    content.append(&roster_hdr);

    let roster_grid = Grid::new();
    roster_grid.set_row_spacing(6);
    roster_grid.set_column_spacing(6);
    roster_grid.set_column_homogeneous(true);
    let char_buttons: Rc<RefCell<Vec<(CompanionCharacter, Button)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let init_char = cfg.omnibar.active_character();

    for (idx, ch) in CompanionCharacter::ALL.iter().copied().enumerate() {
        let btn = Button::new();
        btn.add_css_class("companion-opt-btn");
        btn.set_tooltip_text(Some(&format!(
            "{} ({}) — {}",
            ch.display_name(),
            ch.role_title_tr(),
            ch.focus_summary_tr()
        )));
        if init_char == Some(ch) {
            btn.add_css_class("companion-opt-active");
        }
        let col_box = GtkBox::new(Orientation::Vertical, 2);
        col_box.set_halign(gtk4::Align::Center);
        let mini = DrawingArea::new();
        mini.set_content_width(30);
        mini.set_content_height(30);
        mini.set_size_request(30, 30);
        mini.set_draw_func(move |_, cr, w, h| {
            render_companion_with_accessories(
                cr,
                w as f64,
                h as f64,
                ch.default_shape(),
                ch.default_expression(),
                ch.default_color(),
                0.6,
                -0.4,
                false,
                ch.wears_round_glasses(),
            );
        });
        col_box.append(&mini);
        let lbl = Label::new(Some(ch.display_name()));
        lbl.add_css_class("companion-opt-lbl");
        col_box.append(&lbl);
        btn.set_child(Some(&col_box));
        roster_grid.attach(&btn, (idx % 3) as i32, (idx / 3) as i32, 1, 1);
        char_buttons.borrow_mut().push((ch, btn));
    }
    content.append(&roster_grid);

    // 1. Name section
    let name_hdr = Label::new(Some("Karakter adı"));
    name_hdr.add_css_class("companion-section-lbl");
    name_hdr.set_halign(gtk4::Align::Start);
    content.append(&name_hdr);

    let name_row = GtkBox::new(Orientation::Horizontal, 8);
    let name_entry = Entry::new();
    name_entry.set_hexpand(true);
    name_entry.add_css_class("companion-name-entry");
    name_entry.set_placeholder_text(Some("Karakter adı"));
    programmatic_name_edit.set(true);
    name_entry.set_text(&cfg.omnibar.effective_companion_name());
    programmatic_name_edit.set(false);
    name_row.append(&name_entry);

    let reset_name_btn = Button::with_label("Varsayılan isme dön");
    reset_name_btn.add_css_class("companion-reset-btn");
    name_row.append(&reset_name_btn);
    content.append(&name_row);

    // 2. Live Character & Expression Banner
    let banner = GtkBox::new(Orientation::Vertical, 4);
    banner.add_css_class("companion-meaning-banner");
    let banner_title = Label::new(Some(&format!(
        "{} · {}",
        cfg.omnibar.effective_companion_name(),
        cfg.omnibar.effective_companion_subtitle()
    )));
    banner_title.add_css_class("companion-banner-title");
    banner_title.set_halign(gtk4::Align::Start);
    banner.append(&banner_title);

    let banner_desc = Label::new(Some(cfg.omnibar.effective_companion_summary()));
    banner_desc.add_css_class("companion-banner-desc");
    banner_desc.set_wrap(true);
    banner_desc.set_xalign(0.0);
    banner_desc.set_halign(gtk4::Align::Start);
    banner.append(&banner_desc);
    content.append(&banner);

    // Track mini previews so changing color or shape updates every preview live
    let shape_buttons: Rc<RefCell<Vec<(CompanionShape, Button, DrawingArea)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let expr_buttons: Rc<RefCell<Vec<(CompanionExpression, Button, DrawingArea)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let color_buttons: Rc<RefCell<Vec<(String, Button)>>> = Rc::new(RefCell::new(Vec::new()));

    // 3. Shape 4x2 grid
    let shape_hdr = Label::new(Some("Şekil"));
    shape_hdr.add_css_class("companion-section-lbl");
    shape_hdr.set_halign(gtk4::Align::Start);
    content.append(&shape_hdr);

    let shape_grid = Grid::new();
    shape_grid.set_row_spacing(6);
    shape_grid.set_column_spacing(6);
    shape_grid.set_column_homogeneous(true);

    for (idx, shape) in CompanionShape::ALL.iter().copied().enumerate() {
        let btn = Button::new();
        btn.add_css_class("companion-opt-btn");
        if shape == active_shape.get() {
            btn.add_css_class("companion-opt-active");
        }
        let col_box = GtkBox::new(Orientation::Vertical, 2);
        col_box.set_halign(gtk4::Align::Center);
        let mini = DrawingArea::new();
        mini.set_content_width(28);
        mini.set_content_height(28);
        mini.set_size_request(28, 28);
        let col_rc = active_color.clone();
        let gl_rc = active_glasses.clone();
        mini.set_draw_func(move |_, cr, w, h| {
            let color = col_rc.borrow().clone();
            render_companion_with_accessories(
                cr,
                w as f64,
                h as f64,
                shape,
                CompanionExpression::Attentive,
                &color,
                0.8,
                -0.6,
                false,
                gl_rc.get(),
            );
        });
        col_box.append(&mini);
        let lbl = Label::new(Some(shape.label_tr()));
        lbl.add_css_class("companion-opt-lbl");
        col_box.append(&lbl);
        btn.set_child(Some(&col_box));

        shape_grid.attach(&btn, (idx % 4) as i32, (idx / 4) as i32, 1, 1);
        shape_buttons.borrow_mut().push((shape, btn.clone(), mini));

        let av = avatar.clone();
        let sh_state = active_shape.clone();
        let sb_ref = shape_buttons.clone();
        let eb_ref = expr_buttons.clone();
        let on_change = callbacks.on_appearance_changed.clone();
        btn.connect_clicked(move |_| {
            sh_state.set(shape);
            av.set_shape(shape);
            let mut updated = None;
            let _ = SolarConfig::update(|c| {
                c.omnibar.companion_shape = shape;
                updated = Some(c.omnibar.clone());
            });
            for (s, b, _) in sb_ref.borrow().iter() {
                if *s == shape {
                    b.add_css_class("companion-opt-active");
                } else {
                    b.remove_css_class("companion-opt-active");
                }
            }
            for (_, _, d) in eb_ref.borrow().iter() {
                d.queue_draw();
            }
            if let Some(ref c) = updated {
                on_change(c);
            }
        });
    }
    content.append(&shape_grid);

    // 4. Expression 4x4 grid
    let expr_hdr = Label::new(Some("İfade ve kişilik"));
    expr_hdr.add_css_class("companion-section-lbl");
    expr_hdr.set_halign(gtk4::Align::Start);
    content.append(&expr_hdr);

    let expr_grid = Grid::new();
    expr_grid.set_row_spacing(6);
    expr_grid.set_column_spacing(6);
    expr_grid.set_column_homogeneous(true);

    for (idx, expr) in CompanionExpression::ALL.iter().copied().enumerate() {
        let btn = Button::new();
        btn.add_css_class("companion-opt-btn");
        btn.set_tooltip_text(Some(&format!(
            "{} ({}) — {}",
            expr.title_tr(),
            expr.default_name(),
            expr.meaning_tr()
        )));
        if expr == active_expr.get() {
            btn.add_css_class("companion-opt-active");
        }
        let col_box = GtkBox::new(Orientation::Vertical, 2);
        col_box.set_halign(gtk4::Align::Center);
        let mini = DrawingArea::new();
        mini.set_content_width(28);
        mini.set_content_height(28);
        mini.set_size_request(28, 28);
        let sh_rc = active_shape.clone();
        let col_rc = active_color.clone();
        let gl_rc = active_glasses.clone();
        mini.set_draw_func(move |_, cr, w, h| {
            let color = col_rc.borrow().clone();
            render_companion_with_accessories(
                cr,
                w as f64,
                h as f64,
                sh_rc.get(),
                expr,
                &color,
                0.0,
                0.0,
                false,
                gl_rc.get(),
            );
        });
        col_box.append(&mini);
        let lbl = Label::new(Some(expr.short_label_tr()));
        lbl.add_css_class("companion-opt-lbl");
        col_box.append(&lbl);
        btn.set_child(Some(&col_box));

        expr_grid.attach(&btn, (idx % 4) as i32, (idx / 4) as i32, 1, 1);
        expr_buttons.borrow_mut().push((expr, btn.clone(), mini));

        let av = avatar.clone();
        let ex_state = active_expr.clone();
        let gl_state = active_glasses.clone();
        let eb_ref = expr_buttons.clone();
        let sb_ref_e = shape_buttons.clone();
        let cb_roster_e = char_buttons.clone();
        let bt_lbl = banner_title.clone();
        let bd_lbl = banner_desc.clone();
        let n_entry = name_entry.clone();
        let prog_flag = programmatic_name_edit.clone();
        let on_change = callbacks.on_appearance_changed.clone();
        let on_trans = callbacks.on_persona_transition.clone();
        btn.connect_clicked(move |_| {
            ex_state.set(expr);
            av.set_expression(expr);
            let mut prev_name = String::new();
            let mut prev_expr = expr;
            let mut next_name = String::new();
            let mut updated = None;
            let _ = SolarConfig::update(|c| {
                prev_name = c.omnibar.effective_companion_name();
                prev_expr = c.omnibar.companion_expression;
                c.omnibar.set_companion_expression(expr);
                next_name = c.omnibar.effective_companion_name();
                updated = Some(c.omnibar.clone());
            });

            if let Some(ref c) = updated {
                let wears = c.wears_round_glasses();
                gl_state.set(wears);
                av.set_glasses(wears);
                bt_lbl.set_text(&format!(
                    "{} · {}",
                    c.effective_companion_name(),
                    c.effective_companion_subtitle()
                ));
                bd_lbl.set_text(c.effective_companion_summary());
                let cur_ch = c.active_character();
                for (ch_item, cb_btn) in cb_roster_e.borrow().iter() {
                    if cur_ch == Some(*ch_item) {
                        cb_btn.add_css_class("companion-opt-active");
                    } else {
                        cb_btn.remove_css_class("companion-opt-active");
                    }
                }
                if !c.companion_name_custom {
                    prog_flag.set(true);
                    n_entry.set_text(&next_name);
                    prog_flag.set(false);
                }
                for (_, _, d) in sb_ref_e.borrow().iter() {
                    d.queue_draw();
                }
                for (_, _, d) in eb_ref.borrow().iter() {
                    d.queue_draw();
                }
                on_change(c);
            }
            if (prev_name != next_name || prev_expr != expr)
                && !prev_name.is_empty()
                && !next_name.is_empty()
            {
                on_trans(&prev_name, &next_name, expr);
            }

            for (e, b, _) in eb_ref.borrow().iter() {
                if *e == expr {
                    b.add_css_class("companion-opt-active");
                } else {
                    b.remove_css_class("companion-opt-active");
                }
            }
        });
    }
    content.append(&expr_grid);

    // 5. Color 6x2 grid
    let color_hdr = Label::new(Some("Renk"));
    color_hdr.add_css_class("companion-section-lbl");
    color_hdr.set_halign(gtk4::Align::Start);
    content.append(&color_hdr);

    let color_grid = Grid::new();
    color_grid.set_row_spacing(6);
    color_grid.set_column_spacing(6);
    color_grid.set_column_homogeneous(true);

    for (idx, hex) in COMPANION_PALETTE.iter().copied().enumerate() {
        let btn = Button::new();
        btn.add_css_class("companion-color-btn");
        if active_color.borrow().eq_ignore_ascii_case(hex) {
            btn.add_css_class("companion-color-active");
        }
        let dot = DrawingArea::new();
        dot.set_content_width(22);
        dot.set_content_height(22);
        dot.set_size_request(22, 22);
        dot.set_halign(gtk4::Align::Center);
        dot.set_valign(gtk4::Align::Center);
        dot.set_draw_func(move |_, cr, w, h| {
            let cx = w as f64 * 0.5;
            let cy = h as f64 * 0.5;
            let r = (w.min(h) as f64 * 0.44).max(4.0);
            let (rr, gg, bb) = parse_hex_rgb(hex);
            cr.arc(cx, cy, r, 0.0, TAU);
            cr.set_source_rgb(rr, gg, bb);
            let _ = cr.fill_preserve();
            if relative_luminance(hex) > 0.65 {
                cr.set_source_rgba(0.08, 0.08, 0.09, 0.28);
            } else {
                cr.set_source_rgba(0.93, 0.93, 0.93, 0.18);
            }
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        });
        btn.set_child(Some(&dot));
        color_grid.attach(&btn, (idx % 6) as i32, (idx / 6) as i32, 1, 1);
        color_buttons.borrow_mut().push((hex.to_string(), btn.clone()));

        let av = avatar.clone();
        let col_rc = active_color.clone();
        let cb_ref = color_buttons.clone();
        let sb_ref = shape_buttons.clone();
        let eb_ref = expr_buttons.clone();
        let on_change = callbacks.on_appearance_changed.clone();
        btn.connect_clicked(move |_| {
            *col_rc.borrow_mut() = hex.to_string();
            av.set_color(hex);
            let mut updated = None;
            let _ = SolarConfig::update(|c| {
                c.omnibar.companion_color = hex.to_string();
                updated = Some(c.omnibar.clone());
            });
            for (c_hex, b) in cb_ref.borrow().iter() {
                if c_hex.eq_ignore_ascii_case(hex) {
                    b.add_css_class("companion-color-active");
                } else {
                    b.remove_css_class("companion-color-active");
                }
            }
            for (_, _, d) in sb_ref.borrow().iter() {
                d.queue_draw();
            }
            for (_, _, d) in eb_ref.borrow().iter() {
                d.queue_draw();
            }
            if let Some(ref c) = updated {
                on_change(c);
            }
        });
    }
    content.append(&color_grid);

    // Wire 6-character roster buttons now that shape/expr/color button collections are populated
    for (ch, btn) in char_buttons.borrow().iter().cloned() {
        let av = avatar.clone();
        let sh_state = active_shape.clone();
        let ex_state = active_expr.clone();
        let col_state = active_color.clone();
        let gl_state = active_glasses.clone();
        let cb_roster = char_buttons.clone();
        let sb_ref = shape_buttons.clone();
        let eb_ref = expr_buttons.clone();
        let col_btns_ref = color_buttons.clone();
        let bt_lbl = banner_title.clone();
        let bd_lbl = banner_desc.clone();
        let n_entry = name_entry.clone();
        let prog_flag = programmatic_name_edit.clone();
        let on_change = callbacks.on_appearance_changed.clone();
        let on_trans = callbacks.on_persona_transition.clone();
        btn.connect_clicked(move |_| {
            let mut prev_name = String::new();
            let mut updated = None;
            let _ = SolarConfig::update(|c| {
                prev_name = c.omnibar.effective_companion_name();
                c.omnibar.apply_character_preset(ch);
                updated = Some(c.omnibar.clone());
            });
            if let Some(ref c) = updated {
                sh_state.set(c.companion_shape);
                ex_state.set(c.companion_expression);
                *col_state.borrow_mut() = c.companion_color.clone();
                let wears = c.wears_round_glasses();
                gl_state.set(wears);

                av.set_shape(c.companion_shape);
                av.set_expression(c.companion_expression);
                av.set_color(&c.companion_color);
                av.set_glasses(wears);

                prog_flag.set(true);
                n_entry.set_text(&c.effective_companion_name());
                prog_flag.set(false);

                bt_lbl.set_text(&format!(
                    "{} · {}",
                    c.effective_companion_name(),
                    c.effective_companion_subtitle()
                ));
                bd_lbl.set_text(c.effective_companion_summary());

                for (ch_item, cb_btn) in cb_roster.borrow().iter() {
                    if *ch_item == ch {
                        cb_btn.add_css_class("companion-opt-active");
                    } else {
                        cb_btn.remove_css_class("companion-opt-active");
                    }
                }
                for (s, b, d) in sb_ref.borrow().iter() {
                    if *s == c.companion_shape {
                        b.add_css_class("companion-opt-active");
                    } else {
                        b.remove_css_class("companion-opt-active");
                    }
                    d.queue_draw();
                }
                for (e, b, d) in eb_ref.borrow().iter() {
                    if *e == c.companion_expression {
                        b.add_css_class("companion-opt-active");
                    } else {
                        b.remove_css_class("companion-opt-active");
                    }
                    d.queue_draw();
                }
                for (c_hex, b) in col_btns_ref.borrow().iter() {
                    if c_hex.eq_ignore_ascii_case(&c.companion_color) {
                        b.add_css_class("companion-color-active");
                    } else {
                        b.remove_css_class("companion-color-active");
                    }
                }

                let next_name = c.effective_companion_name();
                if prev_name != next_name && !prev_name.is_empty() && !next_name.is_empty() {
                    on_trans(&prev_name, &next_name, c.companion_expression);
                }
                on_change(c);
            }
        });
    }

    // Name entry & reset wiring
    let prog_edit_chg = programmatic_name_edit.clone();
    let on_change_name = callbacks.on_appearance_changed.clone();
    let av_name = avatar.clone();
    let gl_name = active_glasses.clone();
    let bt_name = banner_title.clone();
    let bd_name = banner_desc.clone();
    let cb_roster_name = char_buttons.clone();
    let sb_name = shape_buttons.clone();
    let eb_name = expr_buttons.clone();
    name_entry.connect_changed(move |entry| {
        if prog_edit_chg.get() {
            return;
        }
        let text = entry.text().to_string();
        let mut updated = None;
        let _ = SolarConfig::update(|c| {
            if text.trim().is_empty() {
                c.omnibar.reset_companion_name();
            } else {
                c.omnibar.set_custom_companion_name(&text);
            }
            updated = Some(c.omnibar.clone());
        });
        if let Some(ref c) = updated {
            let wears = c.wears_round_glasses();
            gl_name.set(wears);
            av_name.set_glasses(wears);
            bt_name.set_text(&format!(
                "{} · {}",
                c.effective_companion_name(),
                c.effective_companion_subtitle()
            ));
            bd_name.set_text(c.effective_companion_summary());
            let cur_ch = c.active_character();
            for (ch_item, cb_btn) in cb_roster_name.borrow().iter() {
                if cur_ch == Some(*ch_item) {
                    cb_btn.add_css_class("companion-opt-active");
                } else {
                    cb_btn.remove_css_class("companion-opt-active");
                }
            }
            for (_, _, d) in sb_name.borrow().iter() {
                d.queue_draw();
            }
            for (_, _, d) in eb_name.borrow().iter() {
                d.queue_draw();
            }
            on_change_name(c);
        }
    });

    let prog_edit_rst = programmatic_name_edit.clone();
    let n_entry_rst = name_entry.clone();
    let on_change_rst = callbacks.on_appearance_changed.clone();
    let av_rst = avatar.clone();
    let gl_rst = active_glasses.clone();
    let bt_rst = banner_title.clone();
    let bd_rst = banner_desc.clone();
    let cb_roster_rst = char_buttons.clone();
    let sb_rst = shape_buttons.clone();
    let eb_rst = expr_buttons.clone();
    reset_name_btn.connect_clicked(move |_| {
        let mut next_name = String::new();
        let mut updated = None;
        let _ = SolarConfig::update(|c| {
            c.omnibar.reset_companion_name();
            next_name = c.omnibar.effective_companion_name();
            updated = Some(c.omnibar.clone());
        });
        prog_edit_rst.set(true);
        n_entry_rst.set_text(&next_name);
        prog_edit_rst.set(false);
        if let Some(ref c) = updated {
            let wears = c.wears_round_glasses();
            gl_rst.set(wears);
            av_rst.set_glasses(wears);
            bt_rst.set_text(&format!(
                "{} · {}",
                c.effective_companion_name(),
                c.effective_companion_subtitle()
            ));
            bd_rst.set_text(c.effective_companion_summary());
            let cur_ch = c.active_character();
            for (ch_item, cb_btn) in cb_roster_rst.borrow().iter() {
                if cur_ch == Some(*ch_item) {
                    cb_btn.add_css_class("companion-opt-active");
                } else {
                    cb_btn.remove_css_class("companion-opt-active");
                }
            }
            for (_, _, d) in sb_rst.borrow().iter() {
                d.queue_draw();
            }
            for (_, _, d) in eb_rst.borrow().iter() {
                d.queue_draw();
            }
            on_change_rst(c);
        }
    });

    // Ensure CSS provider applied if opened in standalone tests
    let _ = CssProvider::new();

    scroller.set_child(Some(&content));
    popover.set_child(Some(&scroller));

    let pop_cleanup = popover.downgrade();
    parent_btn.connect_destroy(move |_| {
        if let Some(p) = pop_cleanup.upgrade() {
            p.unparent();
        }
    });

    popover
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_non_transparent_pixels(surface: &mut cairo::ImageSurface) -> usize {
        surface.flush();
        let data = surface.data().expect("surface bytes");
        data.chunks_exact(4).filter(|px| px[3] > 20).count()
    }

    fn count_bright_or_dark_eye_pixels(
        surface: &mut cairo::ImageSurface,
        body_hex: &str,
    ) -> usize {
        surface.flush();
        let is_light_body = relative_luminance(body_hex) > 0.65;
        let data = surface.data().expect("surface bytes");
        data.chunks_exact(4)
            .filter(|px| {
                // ARGB32 little-endian: [B, G, R, A]
                if px[3] < 200 {
                    return false;
                }
                let r = px[2];
                let g = px[1];
                let b = px[0];
                if is_light_body {
                    r < 40 && g < 40 && b < 40
                } else {
                    r > 220 && g > 220 && b > 220
                }
            })
            .count()
    }

    #[test]
    fn gaze_offset_stays_bounded_and_matches_direction() {
        assert_eq!(compute_gaze_offset(100.0, 100.0, 100.0, 100.0, 5.5), (0.0, 0.0));
        assert_eq!(compute_gaze_offset(f64::NAN, 10.0, 0.0, 0.0, 5.5), (0.0, 0.0));

        let (gx, gy) = compute_gaze_offset(1000.0, -500.0, 50.0, 50.0, 5.5);
        assert!(gx > 0.0 && gy < 0.0);
        assert!(gx.hypot(gy) <= 5.5);

        let (gx_far, gy_far) = compute_gaze_offset(1e9, 1e9, 0.0, 0.0, 5.5);
        assert!(gx_far.hypot(gy_far) <= 5.5 + 1e-9);
    }

    #[test]
    fn eye_contrast_flips_on_light_body_color() {
        let dark_body_eye = eye_color_for_body("#1e1e22");
        assert!(dark_body_eye.0 > 0.9);
        let light_body_eye = eye_color_for_body("#e4e4e7");
        assert!(light_body_eye.0 < 0.15);
    }

    #[test]
    fn all_8_shapes_render_non_empty_vector_surfaces() {
        for shape in CompanionShape::ALL {
            let mut surface =
                cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                render_companion(
                    &cr,
                    52.0,
                    52.0,
                    shape,
                    CompanionExpression::Attentive,
                    "#1e1e22",
                    2.5,
                    -1.5,
                    false,
                );
            }
            let filled = count_non_transparent_pixels(&mut surface);
            assert!(
                filled > 500,
                "Shape {:?} rendered too few pixels ({})",
                shape,
                filled
            );
            let eyes = count_bright_or_dark_eye_pixels(&mut surface, "#1e1e22");
            assert!(eyes > 20, "Shape {:?} missing eye pixels ({})", shape, eyes);
        }
    }

    #[test]
    fn all_16_expressions_render_distinctly_on_both_dark_and_light_bodies() {
        let mut signatures = std::collections::HashSet::new();
        for expr in CompanionExpression::ALL {
            let desc = expression_eye_descriptor(expr);
            let sig = format!(
                "{:.2}:{:.2}:{:.2}:{:.2}:{}:{:.1}",
                desc.left_height_factor,
                desc.right_height_factor,
                desc.left_width_factor,
                desc.right_width_factor,
                desc.is_arc,
                desc.tilt_deg
            );
            assert!(
                signatures.insert(sig),
                "Duplicate geometric descriptor for {:?}",
                expr
            );

            for color in ["#1e1e22", "#e4e4e7"] {
                let mut surface =
                    cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
                {
                    let cr = cairo::Context::new(&surface).unwrap();
                    render_companion(&cr, 52.0, 52.0, CompanionShape::Pebble, expr, color, 1.0, 1.0, false);
                }
                let eyes = count_bright_or_dark_eye_pixels(&mut surface, color);
                assert!(
                    eyes > 8,
                    "Expression {:?} on {} rendered too few contrasting eye pixels ({})",
                    expr,
                    color,
                    eyes
                );
            }
        }
    }

    #[test]
    fn blink_frame_renders_thin_closed_lids() {
        let mut open_surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
        {
            let cr = cairo::Context::new(&open_surf).unwrap();
            render_companion(
                &cr,
                52.0,
                52.0,
                CompanionShape::Pebble,
                CompanionExpression::Attentive,
                "#1e1e22",
                0.0,
                0.0,
                false,
            );
        }
        let mut blink_surf = cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
        {
            let cr = cairo::Context::new(&blink_surf).unwrap();
            render_companion(
                &cr,
                52.0,
                52.0,
                CompanionShape::Pebble,
                CompanionExpression::Attentive,
                "#1e1e22",
                0.0,
                0.0,
                true,
            );
        }
        let open_eyes = count_bright_or_dark_eye_pixels(&mut open_surf, "#1e1e22");
        let blink_eyes = count_bright_or_dark_eye_pixels(&mut blink_surf, "#1e1e22");
        assert!(open_eyes > blink_eyes);
        assert!(blink_eyes > 4);
    }

    #[test]
    fn stolas_round_glasses_render_distinct_frame_on_dark_and_light_bodies() {
        for color in ["#6366f1", "#1e1e22", "#e4e4e7"] {
            let mut plain = cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
            {
                let cr = cairo::Context::new(&plain).unwrap();
                render_companion_with_accessories(
                    &cr,
                    52.0,
                    52.0,
                    CompanionShape::Squircle,
                    CompanionExpression::Curious,
                    color,
                    0.0,
                    0.0,
                    false,
                    false,
                );
            }
            let mut glasses = cairo::ImageSurface::create(cairo::Format::ARgb32, 52, 52).unwrap();
            {
                let cr = cairo::Context::new(&glasses).unwrap();
                render_companion_with_accessories(
                    &cr,
                    52.0,
                    52.0,
                    CompanionShape::Squircle,
                    CompanionExpression::Curious,
                    color,
                    0.0,
                    0.0,
                    false,
                    true,
                );
            }
            plain.flush();
            glasses.flush();
            let d1 = plain.data().unwrap();
            let d2 = glasses.data().unwrap();
            let diff_px = d1
                .chunks_exact(4)
                .zip(d2.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert!(
                diff_px > 120,
                "Stolas round glasses on {} modified too few pixels ({})",
                color,
                diff_px
            );
        }
    }
}
