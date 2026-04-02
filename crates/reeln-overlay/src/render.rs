use std::path::Path;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::elements::{self, Element, GradientDirection, ImageFit, ImageSource};
use crate::error::OverlayError;
use crate::layout::resolve_dimension;
use crate::template::{self, Template, TemplateContext};
use crate::text;

/// Rasterize a template with the given context to an RGBA PNG file.
pub fn render_template_to_png(
    template: &Template,
    context: &TemplateContext,
    output: &Path,
) -> Result<(), OverlayError> {
    let width = template.canvas.width;
    let height = template.canvas.height;

    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| OverlayError::Render("failed to create pixmap".to_string()))?;

    for layer in &template.layers {
        render_layer(&mut pixmap, layer, context, width, height)?;
    }

    let png_data = pixmap
        .encode_png()
        .map_err(|e| OverlayError::Image(format!("PNG encode failed: {e}")))?;

    std::fs::write(output, png_data).map_err(OverlayError::Io)?;

    Ok(())
}

/// Substitute template variables in a dimension string, then resolve to pixels.
fn resolve_dim(
    value: &str,
    container_size: u32,
    context: &TemplateContext,
) -> Result<f32, OverlayError> {
    let resolved = template::substitute_variables(value, context);
    resolve_dimension(&resolved, container_size)
}

fn render_layer(
    pixmap: &mut Pixmap,
    layer: &Element,
    context: &TemplateContext,
    canvas_w: u32,
    canvas_h: u32,
) -> Result<(), OverlayError> {
    match layer {
        Element::Rect {
            bounds,
            fill,
            corner_radius,
            border,
            opacity,
        } => {
            let x = resolve_dim(&bounds.x, canvas_w, context)?;
            let y = resolve_dim(&bounds.y, canvas_h, context)?;
            let w = resolve_dim(&bounds.w, canvas_w, context)?;
            let h = resolve_dim(&bounds.h, canvas_h, context)?;

            let (r, g, b, a) = parse_element_color(fill, context)?;
            let mut paint = Paint::default();
            let alpha = (a as f32 / 255.0) * opacity;
            paint.set_color(
                Color::from_rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha)
                    .unwrap_or(Color::BLACK),
            );
            paint.anti_alias = true;

            let path = if *corner_radius > 0.0 {
                rounded_rect_path(x, y, w, h, *corner_radius)
            } else {
                rect_path(x, y, w, h)
            };

            if let Some(path) = path {
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }

            // Render border if specified.
            if let Some(border) = border {
                render_border(pixmap, x, y, w, h, *corner_radius, border, context)?;
            }
        }
        Element::Text {
            content,
            position,
            font,
            color,
            outline,
            alignment,
            max_width,
            visible,
        } => {
            // Check visibility condition.
            if let Some(condition) = visible
                && !template::evaluate_visibility(condition, context)
            {
                return Ok(());
            }

            let resolved = template::substitute_variables(content, context);
            if resolved.is_empty() {
                return Ok(());
            }

            let x = resolve_dim(&position.x, canvas_w, context)?;
            let y = resolve_dim(&position.y, canvas_h, context)?;

            // Substitute variables in color strings before rendering text
            let resolved_color = match color {
                elements::Color::Hex(s) => {
                    elements::Color::Hex(template::substitute_variables(s, context))
                }
            };
            let resolved_outline = outline.as_ref().map(|o| elements::OutlineSpec {
                color: match &o.color {
                    elements::Color::Hex(s) => {
                        elements::Color::Hex(template::substitute_variables(s, context))
                    }
                },
                width: o.width,
            });
            text::render_text_to_pixmap(
                pixmap,
                &resolved,
                x,
                y,
                &font.family,
                font.size,
                font.weight.as_deref(),
                &resolved_color,
                alignment,
                *max_width,
                font.auto_shrink,
                resolved_outline.as_ref(),
            )?;
        }
        Element::Image {
            source,
            position,
            size,
            fit,
            opacity,
        } => {
            let x = resolve_dim(&position.x, canvas_w, context)?;
            let y = resolve_dim(&position.y, canvas_h, context)?;
            let w = resolve_dim(&size.w, canvas_w, context)?;
            let h = resolve_dim(&size.h, canvas_h, context)?;

            let path_str = match source {
                ImageSource::File(p) => p.to_string_lossy().to_string(),
                ImageSource::Variable(var) => {
                    let resolved = template::substitute_variables(var, context);
                    if resolved.contains("{{") || resolved.is_empty() {
                        // Unresolved variable or empty — skip.
                        return Ok(());
                    }
                    resolved
                }
            };

            render_image(pixmap, &path_str, x, y, w, h, fit, *opacity)?;
        }
        Element::Gradient {
            bounds,
            stops,
            direction,
            corner_radius: _,
        } => {
            let x = resolve_dim(&bounds.x, canvas_w, context)?;
            let y = resolve_dim(&bounds.y, canvas_h, context)?;
            let w = resolve_dim(&bounds.w, canvas_w, context)?;
            let h = resolve_dim(&bounds.h, canvas_h, context)?;

            if stops.len() < 2 || w <= 0.0 || h <= 0.0 {
                return Ok(());
            }

            render_gradient(
                pixmap,
                &GradientParams { x, y, w, h },
                stops,
                direction,
                context,
            )?;
        }
    }
    Ok(())
}

struct GradientParams {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn render_gradient(
    pixmap: &mut Pixmap,
    params: &GradientParams,
    stops: &[elements::GradientStop],
    direction: &GradientDirection,
    context: &TemplateContext,
) -> Result<(), OverlayError> {
    let GradientParams { x, y, w, h } = *params;
    let x_start = x as u32;
    let y_start = y as u32;
    let x_end = (x + w) as u32;
    let y_end = (y + h) as u32;

    let pw = pixmap.width();
    let ph = pixmap.height();

    // Parse stop colors
    let parsed_stops: Vec<(f32, u8, u8, u8, u8)> = stops
        .iter()
        .map(|s| {
            let (r, g, b, a) = parse_element_color(&s.color, context)?;
            Ok((s.position, r, g, b, a))
        })
        .collect::<Result<Vec<_>, OverlayError>>()?;

    let pixels = pixmap.pixels_mut();

    for py in y_start..y_end.min(ph) {
        for px in x_start..x_end.min(pw) {
            let t = match direction {
                GradientDirection::Vertical => {
                    if h > 0.0 {
                        (py as f32 - y) / h
                    } else {
                        0.0
                    }
                }
                GradientDirection::Horizontal => {
                    if w > 0.0 {
                        (px as f32 - x) / w
                    } else {
                        0.0
                    }
                }
                GradientDirection::Angle(_) => {
                    // Fallback to vertical for angled gradients in Phase 1
                    if h > 0.0 { (py as f32 - y) / h } else { 0.0 }
                }
            };
            let t = t.clamp(0.0, 1.0);

            let (r, g, b, a) = interpolate_stops(&parsed_stops, t);
            let idx = (py * pw + px) as usize;
            if idx < pixels.len() {
                pixels[idx] = tiny_skia::PremultipliedColorU8::from_rgba(
                    (r as u16 * a as u16 / 255) as u8,
                    (g as u16 * a as u16 / 255) as u8,
                    (b as u16 * a as u16 / 255) as u8,
                    a,
                )
                .unwrap();
            }
        }
    }

    Ok(())
}

fn interpolate_stops(stops: &[(f32, u8, u8, u8, u8)], t: f32) -> (u8, u8, u8, u8) {
    if stops.is_empty() {
        return (0, 0, 0, 255);
    }
    if t <= stops[0].0 {
        return (stops[0].1, stops[0].2, stops[0].3, stops[0].4);
    }
    let last = stops.len() - 1;
    if t >= stops[last].0 {
        return (stops[last].1, stops[last].2, stops[last].3, stops[last].4);
    }

    for i in 0..last {
        let (pos0, r0, g0, b0, a0) = stops[i];
        let (pos1, r1, g1, b1, a1) = stops[i + 1];
        if t >= pos0 && t <= pos1 {
            let range = pos1 - pos0;
            let factor = if range > 0.0 { (t - pos0) / range } else { 0.0 };
            return (
                lerp_u8(r0, r1, factor),
                lerp_u8(g0, g1, factor),
                lerp_u8(b0, b1, factor),
                lerp_u8(a0, a1, factor),
            );
        }
    }

    (stops[last].1, stops[last].2, stops[last].3, stops[last].4)
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn parse_element_color(
    color: &elements::Color,
    context: &TemplateContext,
) -> Result<(u8, u8, u8, u8), OverlayError> {
    match color {
        elements::Color::Hex(s) => {
            let resolved = template::substitute_variables(s, context);
            elements::parse_color(&resolved)
        }
    }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();
    pb.finish()
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    // Clamp radius to half the smallest dimension
    let r = r.min(w / 2.0).min(h / 2.0);
    let _rect = tiny_skia::Rect::from_xywh(x, y, w, h)?;
    let mut pb = PathBuilder::new();
    // Approximate rounded rect with arcs (using quad curves)
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

// ── Image compositing ───────────────────────────────────────────────

/// Load an image from disk and composite it onto the pixmap.
#[allow(clippy::too_many_arguments)]
fn render_image(
    pixmap: &mut Pixmap,
    path: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fit: &ImageFit,
    opacity: f32,
) -> Result<(), OverlayError> {
    let img = image::open(path)
        .map_err(|e| OverlayError::Image(format!("failed to load image '{path}': {e}")))?
        .to_rgba8();

    let (src_w, src_h) = (img.width() as f32, img.height() as f32);
    if src_w == 0.0 || src_h == 0.0 {
        return Ok(());
    }

    // Compute source region and destination region based on fit mode.
    let (dst_w, dst_h, src_crop) = match fit {
        ImageFit::Fill => {
            // Stretch to fill the target bounds.
            (w, h, None)
        }
        ImageFit::Contain => {
            // Scale to fit within bounds, preserving aspect ratio.
            let scale = (w / src_w).min(h / src_h);
            (src_w * scale, src_h * scale, None)
        }
        ImageFit::Cover => {
            // Scale to cover bounds, cropping excess.
            let scale = (w / src_w).max(h / src_h);
            let scaled_w = src_w * scale;
            let scaled_h = src_h * scale;
            // Crop center.
            let crop_x = ((scaled_w - w) / 2.0 / scale) as u32;
            let crop_y = ((scaled_h - h) / 2.0 / scale) as u32;
            let crop_w = (w / scale) as u32;
            let crop_h = (h / scale) as u32;
            (w, h, Some((crop_x, crop_y, crop_w, crop_h)))
        }
    };

    // Resize the image.
    let cropped = if let Some((cx, cy, cw, ch)) = src_crop {
        image::imageops::crop_imm(
            &img,
            cx.min(img.width().saturating_sub(1)),
            cy.min(img.height().saturating_sub(1)),
            cw.min(img.width()),
            ch.min(img.height()),
        )
        .to_image()
    } else {
        img
    };

    let resized = image::imageops::resize(
        &cropped,
        dst_w as u32,
        dst_h as u32,
        image::imageops::FilterType::Lanczos3,
    );

    // Center within the target bounds for Contain mode.
    let (off_x, off_y) = match fit {
        ImageFit::Contain => ((w - dst_w) / 2.0, (h - dst_h) / 2.0),
        _ => (0.0, 0.0),
    };

    // Composite onto the pixmap with alpha blending.
    let px_w = pixmap.width() as i32;
    let px_h = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();

    for (iy, ix, pixel) in resized.enumerate_pixels() {
        let dx = (x + off_x) as i32 + ix as i32;
        let dy = (y + off_y) as i32 + iy as i32;
        if dx < 0 || dy < 0 || dx >= px_w || dy >= px_h {
            continue;
        }

        let [sr, sg, sb, sa] = pixel.0;
        let sa = (sa as f32 * opacity) as u8;
        if sa == 0 {
            continue;
        }

        let idx = (dy * px_w + dx) as usize;
        let dst = pixels[idx];
        let (dr, dg, db, da) = (dst.red(), dst.green(), dst.blue(), dst.alpha());

        let src_a = sa as u16;
        let inv_a = 255 - src_a;
        let nr = (sr as u16 * src_a / 255 + dr as u16 * inv_a / 255).min(255) as u8;
        let ng = (sg as u16 * src_a / 255 + dg as u16 * inv_a / 255).min(255) as u8;
        let nb = (sb as u16 * src_a / 255 + db as u16 * inv_a / 255).min(255) as u8;
        let na = (src_a + da as u16 * inv_a / 255).min(255) as u8;

        pixels[idx] = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na).unwrap();
    }

    Ok(())
}

// ── Border rendering ────────────────────────────────────────────────

/// Render a border stroke around a rectangle.
#[allow(clippy::too_many_arguments)]
fn render_border(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    corner_radius: f32,
    border: &elements::Border,
    context: &TemplateContext,
) -> Result<(), OverlayError> {
    let (r, g, b, a) = parse_element_color(&border.color, context)?;
    let mut paint = Paint::default();
    paint.set_color(
        Color::from_rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
        .unwrap_or(Color::BLACK),
    );
    paint.anti_alias = true;

    let path = if corner_radius > 0.0 {
        rounded_rect_path(x, y, w, h, corner_radius)
    } else {
        rect_path(x, y, w, h)
    };

    if let Some(path) = path {
        let stroke = tiny_skia::Stroke {
            width: border.width,
            ..tiny_skia::Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::{Canvas, Timing};

    fn simple_template(layers: Vec<Element>) -> Template {
        Template {
            name: "test".to_string(),
            version: 1,
            canvas: Canvas {
                width: 100,
                height: 100,
            },
            layers,
            timing: Timing::default(),
        }
    }

    #[test]
    fn test_render_empty_template() {
        let template = simple_template(vec![]);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());

        // Verify it's a valid PNG
        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn test_render_rect() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100%".to_string(),
                h: "100%".to_string(),
            },
            fill: elements::Color::Hex("#FF0000".to_string()),
            corner_radius: 0.0,
            border: None,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rect.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 8); // more than just header
    }

    #[test]
    fn test_render_rect_with_opacity() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "10".to_string(),
                y: "10".to_string(),
                w: "50".to_string(),
                h: "50".to_string(),
            },
            fill: elements::Color::Hex("#00FF00".to_string()),
            corner_radius: 0.0,
            border: None,
            opacity: 0.5,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opacity.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_rect_rounded() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "10".to_string(),
                y: "10".to_string(),
                w: "80".to_string(),
                h: "80".to_string(),
            },
            fill: elements::Color::Hex("#0000FF".to_string()),
            corner_radius: 10.0,
            border: None,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rounded.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_text_visible() {
        let layers = vec![Element::Text {
            content: "Hello {{name}}".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "50".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "Arial".to_string(),
                size: 24.0,
                weight: None,
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FFFFFF".to_string()),
            outline: None,
            alignment: crate::elements::Alignment::Left,
            max_width: None,
            visible: Some("{{show}}".to_string()),
        }];
        let template = simple_template(layers);
        let mut ctx = TemplateContext::new();
        ctx.insert("name".to_string(), "World".to_string());
        ctx.insert("show".to_string(), "true".to_string());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_text_hidden() {
        let layers = vec![Element::Text {
            content: "Hidden".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "50".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "Arial".to_string(),
                size: 24.0,
                weight: None,
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FFFFFF".to_string()),
            outline: None,
            alignment: crate::elements::Alignment::Left,
            max_width: None,
            visible: Some("{{hidden_flag}}".to_string()),
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new(); // hidden_flag not set
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hidden.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_text_no_visibility() {
        let layers = vec![Element::Text {
            content: "Always visible".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "50".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "Arial".to_string(),
                size: 24.0,
                weight: None,
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FFFFFF".to_string()),
            outline: None,
            alignment: crate::elements::Alignment::Left,
            max_width: None,
            visible: None,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_vis.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_gradient_vertical() {
        let layers = vec![Element::Gradient {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            stops: vec![
                elements::GradientStop {
                    color: elements::Color::Hex("#000000".to_string()),
                    position: 0.0,
                },
                elements::GradientStop {
                    color: elements::Color::Hex("#FFFFFF".to_string()),
                    position: 1.0,
                },
            ],
            direction: GradientDirection::Vertical,
            corner_radius: 0.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gradient.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_gradient_horizontal() {
        let layers = vec![Element::Gradient {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            stops: vec![
                elements::GradientStop {
                    color: elements::Color::Hex("#FF0000".to_string()),
                    position: 0.0,
                },
                elements::GradientStop {
                    color: elements::Color::Hex("#0000FF".to_string()),
                    position: 1.0,
                },
            ],
            direction: GradientDirection::Horizontal,
            corner_radius: 0.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gradient_h.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_gradient_angle_fallback() {
        let layers = vec![Element::Gradient {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            stops: vec![
                elements::GradientStop {
                    color: elements::Color::Hex("#FF0000".to_string()),
                    position: 0.0,
                },
                elements::GradientStop {
                    color: elements::Color::Hex("#0000FF".to_string()),
                    position: 1.0,
                },
            ],
            direction: GradientDirection::Angle(45.0),
            corner_radius: 0.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gradient_angle.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_gradient_too_few_stops() {
        let layers = vec![Element::Gradient {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            stops: vec![elements::GradientStop {
                color: elements::Color::Hex("#000000".to_string()),
                position: 0.0,
            }],
            direction: GradientDirection::Vertical,
            corner_radius: 0.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("single_stop.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_image_unresolved_variable_skips() {
        // Unresolved variable source should skip (no crash).
        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::Variable("{{avatar}}".to_string()),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "50".to_string(),
                h: "50".to_string(),
            },
            fit: crate::elements::ImageFit::Contain,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_image_from_file() {
        let dir = tempfile::tempdir().unwrap();

        // Create a small test PNG.
        let img = image::RgbaImage::from_fn(20, 20, |_, _| image::Rgba([255, 0, 0, 255]));
        let img_path = dir.path().join("red.png");
        img.save(&img_path).unwrap();

        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::File(img_path),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "10".to_string(),
            },
            size: crate::elements::Size {
                w: "40".to_string(),
                h: "40".to_string(),
            },
            fit: crate::elements::ImageFit::Fill,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let out_path = dir.path().join("output.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
    }

    #[test]
    fn test_render_image_contain_fit() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_fn(40, 20, |_, _| image::Rgba([0, 255, 0, 255]));
        let img_path = dir.path().join("wide.png");
        img.save(&img_path).unwrap();

        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::File(img_path),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "50".to_string(),
                h: "50".to_string(),
            },
            fit: crate::elements::ImageFit::Contain,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let out_path = dir.path().join("contain.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
    }

    #[test]
    fn test_render_image_cover_fit() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_fn(40, 20, |_, _| image::Rgba([0, 0, 255, 255]));
        let img_path = dir.path().join("wide.png");
        img.save(&img_path).unwrap();

        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::File(img_path),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "30".to_string(),
                h: "30".to_string(),
            },
            fit: crate::elements::ImageFit::Cover,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let out_path = dir.path().join("cover.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
    }

    #[test]
    fn test_render_image_with_opacity() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_fn(10, 10, |_, _| image::Rgba([255, 255, 255, 255]));
        let img_path = dir.path().join("white.png");
        img.save(&img_path).unwrap();

        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::File(img_path),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "10".to_string(),
                h: "10".to_string(),
            },
            fit: crate::elements::ImageFit::Fill,
            opacity: 0.5,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let out_path = dir.path().join("opacity.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
    }

    #[test]
    fn test_render_image_nonexistent_file() {
        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::File("/tmp/nonexistent_overlay_img.png".into()),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "50".to_string(),
                h: "50".to_string(),
            },
            fit: crate::elements::ImageFit::Fill,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fail.png");

        let result = render_template_to_png(&template, &ctx, &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_render_image_via_variable() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_fn(10, 10, |_, _| image::Rgba([128, 128, 128, 255]));
        let img_path = dir.path().join("var_img.png");
        img.save(&img_path).unwrap();

        let layers = vec![Element::Image {
            source: crate::elements::ImageSource::Variable("{{logo}}".to_string()),
            position: crate::elements::Position {
                x: "0".to_string(),
                y: "0".to_string(),
            },
            size: crate::elements::Size {
                w: "20".to_string(),
                h: "20".to_string(),
            },
            fit: crate::elements::ImageFit::Fill,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let mut ctx = TemplateContext::new();
        ctx.insert("logo".to_string(), img_path.to_string_lossy().to_string());
        let out_path = dir.path().join("var_out.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
    }

    #[test]
    fn test_render_zero_canvas() {
        let template = Template {
            name: "zero".to_string(),
            version: 1,
            canvas: Canvas {
                width: 0,
                height: 0,
            },
            layers: vec![],
            timing: Timing::default(),
        };
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero.png");

        let result = render_template_to_png(&template, &ctx, &path);
        assert!(result.is_err());
    }

    #[test]
    fn test_render_multiple_layers() {
        let layers = vec![
            Element::Rect {
                bounds: crate::elements::Bounds {
                    x: "0".to_string(),
                    y: "0".to_string(),
                    w: "100%".to_string(),
                    h: "100%".to_string(),
                },
                fill: elements::Color::Hex("#000000".to_string()),
                corner_radius: 0.0,
                border: None,
                opacity: 1.0,
            },
            Element::Rect {
                bounds: crate::elements::Bounds {
                    x: "10".to_string(),
                    y: "10".to_string(),
                    w: "80".to_string(),
                    h: "80".to_string(),
                },
                fill: elements::Color::Hex("#FF0000".to_string()),
                corner_radius: 5.0,
                border: None,
                opacity: 0.8,
            },
        ];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_interpolate_stops_at_start() {
        let stops = vec![(0.0, 0u8, 0u8, 0u8, 255u8), (1.0, 255, 255, 255, 255)];
        let (r, g, b, a) = interpolate_stops(&stops, 0.0);
        assert_eq!((r, g, b, a), (0, 0, 0, 255));
    }

    #[test]
    fn test_interpolate_stops_at_end() {
        let stops = vec![(0.0, 0u8, 0u8, 0u8, 255u8), (1.0, 255, 255, 255, 255)];
        let (r, g, b, a) = interpolate_stops(&stops, 1.0);
        assert_eq!((r, g, b, a), (255, 255, 255, 255));
    }

    #[test]
    fn test_interpolate_stops_midpoint() {
        let stops = vec![(0.0, 0u8, 0u8, 0u8, 255u8), (1.0, 254, 254, 254, 255)];
        let (r, _, _, _) = interpolate_stops(&stops, 0.5);
        assert_eq!(r, 127);
    }

    #[test]
    fn test_interpolate_stops_empty() {
        let (r, g, b, a) = interpolate_stops(&[], 0.5);
        assert_eq!((r, g, b, a), (0, 0, 0, 255));
    }

    #[test]
    fn test_interpolate_stops_before_first() {
        let stops = vec![(0.5, 100u8, 100u8, 100u8, 255u8), (1.0, 200, 200, 200, 255)];
        let (r, _, _, _) = interpolate_stops(&stops, 0.0);
        assert_eq!(r, 100);
    }

    #[test]
    fn test_lerp_u8_boundaries() {
        assert_eq!(lerp_u8(0, 255, 0.0), 0);
        assert_eq!(lerp_u8(0, 255, 1.0), 255);
    }

    #[test]
    fn test_lerp_u8_midpoint() {
        assert_eq!(lerp_u8(0, 254, 0.5), 127);
    }

    #[test]
    fn test_rect_path_creates_path() {
        let path = rect_path(0.0, 0.0, 100.0, 50.0);
        assert!(path.is_some());
    }

    #[test]
    fn test_rounded_rect_path_creates_path() {
        let path = rounded_rect_path(0.0, 0.0, 100.0, 50.0, 10.0);
        assert!(path.is_some());
    }

    #[test]
    fn test_rounded_rect_clamps_radius() {
        // Radius larger than half the smallest dimension should be clamped
        let path = rounded_rect_path(0.0, 0.0, 20.0, 10.0, 100.0);
        assert!(path.is_some());
    }

    #[test]
    fn test_render_invalid_color_in_rect() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            fill: elements::Color::Hex("not_a_color".to_string()),
            corner_radius: 0.0,
            border: None,
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_color.png");

        let result = render_template_to_png(&template, &ctx, &path);
        assert!(result.is_err());
    }

    // ── Border tests ──────────────────────────────────────────────

    #[test]
    fn test_render_rect_with_border() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "10".to_string(),
                y: "10".to_string(),
                w: "80".to_string(),
                h: "80".to_string(),
            },
            fill: elements::Color::Hex("#0000FF".to_string()),
            corner_radius: 0.0,
            border: Some(crate::elements::Border {
                width: 3.0,
                color: elements::Color::Hex("#FFFFFF".to_string()),
            }),
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("border.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_rect_with_border_and_radius() {
        let layers = vec![Element::Rect {
            bounds: crate::elements::Bounds {
                x: "5".to_string(),
                y: "5".to_string(),
                w: "90".to_string(),
                h: "90".to_string(),
            },
            fill: elements::Color::Hex("#FF0000".to_string()),
            corner_radius: 15.0,
            border: Some(crate::elements::Border {
                width: 2.0,
                color: elements::Color::Hex("#000000".to_string()),
            }),
            opacity: 1.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("border_radius.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    // ── Text rendering via template tests ───────────────────────

    #[test]
    fn test_render_text_draws_pixels() {
        let layers = vec![Element::Text {
            content: "Test".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "10".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "sans-serif".to_string(),
                size: 24.0,
                weight: None,
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FFFFFF".to_string()),
            outline: None,
            alignment: crate::elements::Alignment::Left,
            max_width: None,
            visible: None,
        }];
        let template = Template {
            name: "text_draw".to_string(),
            version: 1,
            canvas: Canvas {
                width: 200,
                height: 100,
            },
            layers,
            timing: Timing::default(),
        };
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text_draw.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());

        // Verify the PNG has content (not all zeros).
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 100, "PNG should have substantial content");
    }

    #[test]
    fn test_render_text_with_variable_substitution() {
        let layers = vec![Element::Text {
            content: "Score: {{score}}".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "10".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "sans-serif".to_string(),
                size: 20.0,
                weight: None,
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FF0000".to_string()),
            outline: None,
            alignment: crate::elements::Alignment::Left,
            max_width: None,
            visible: None,
        }];
        let template = Template {
            name: "var_text".to_string(),
            version: 1,
            canvas: Canvas {
                width: 300,
                height: 100,
            },
            layers,
            timing: Timing::default(),
        };
        let mut ctx = TemplateContext::new();
        ctx.insert("score".to_string(), "42".to_string());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("var_text.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_render_text_with_outline() {
        let layers = vec![Element::Text {
            content: "Outlined".to_string(),
            position: crate::elements::Position {
                x: "10".to_string(),
                y: "20".to_string(),
            },
            font: crate::elements::FontSpec {
                family: "sans-serif".to_string(),
                size: 32.0,
                weight: Some("bold".to_string()),
                auto_shrink: None,
            },
            color: elements::Color::Hex("#FFFFFF".to_string()),
            outline: Some(crate::elements::OutlineSpec {
                color: elements::Color::Hex("#000000".to_string()),
                width: 2.0,
            }),
            alignment: crate::elements::Alignment::Center,
            max_width: Some(200.0),
            visible: None,
        }];
        let template = Template {
            name: "outlined".to_string(),
            version: 1,
            canvas: Canvas {
                width: 200,
                height: 100,
            },
            layers,
            timing: Timing::default(),
        };
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outlined.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }

    // ── Full composite test ────────────────────────────────────

    #[test]
    fn test_render_full_overlay_composite() {
        let dir = tempfile::tempdir().unwrap();

        // Create a small logo image.
        let img = image::RgbaImage::from_fn(16, 16, |_, _| image::Rgba([0, 128, 255, 255]));
        let logo_path = dir.path().join("logo.png");
        img.save(&logo_path).unwrap();

        let layers = vec![
            // Background rect.
            Element::Rect {
                bounds: crate::elements::Bounds {
                    x: "0".to_string(),
                    y: "0".to_string(),
                    w: "100%".to_string(),
                    h: "100%".to_string(),
                },
                fill: elements::Color::Hex("#00000080".to_string()),
                corner_radius: 0.0,
                border: None,
                opacity: 1.0,
            },
            // Team logo.
            Element::Image {
                source: crate::elements::ImageSource::Variable("{{team_logo}}".to_string()),
                position: crate::elements::Position {
                    x: "5".to_string(),
                    y: "5".to_string(),
                },
                size: crate::elements::Size {
                    w: "30".to_string(),
                    h: "30".to_string(),
                },
                fit: crate::elements::ImageFit::Contain,
                opacity: 1.0,
            },
            // Scorer name.
            Element::Text {
                content: "{{scorer}}".to_string(),
                position: crate::elements::Position {
                    x: "40".to_string(),
                    y: "8".to_string(),
                },
                font: crate::elements::FontSpec {
                    family: "sans-serif".to_string(),
                    size: 20.0,
                    weight: Some("bold".to_string()),
                    auto_shrink: Some(10.0),
                },
                color: elements::Color::Hex("#FFFFFF".to_string()),
                outline: Some(crate::elements::OutlineSpec {
                    color: elements::Color::Hex("#000000".to_string()),
                    width: 1.0,
                }),
                alignment: crate::elements::Alignment::Left,
                max_width: Some(150.0),
                visible: None,
            },
        ];
        let template = Template {
            name: "goal_overlay".to_string(),
            version: 1,
            canvas: Canvas {
                width: 200,
                height: 40,
            },
            layers,
            timing: Timing::default(),
        };
        let mut ctx = TemplateContext::new();
        ctx.insert(
            "team_logo".to_string(),
            logo_path.to_string_lossy().to_string(),
        );
        ctx.insert("scorer".to_string(), "J. Smith".to_string());
        let out_path = dir.path().join("full_overlay.png");

        render_template_to_png(&template, &ctx, &out_path).unwrap();
        assert!(out_path.exists());
        let data = std::fs::read(&out_path).unwrap();
        assert!(data.len() > 100);
    }

    #[test]
    fn test_render_gradient_three_stops() {
        let layers = vec![Element::Gradient {
            bounds: crate::elements::Bounds {
                x: "0".to_string(),
                y: "0".to_string(),
                w: "100".to_string(),
                h: "100".to_string(),
            },
            stops: vec![
                elements::GradientStop {
                    color: elements::Color::Hex("#FF0000".to_string()),
                    position: 0.0,
                },
                elements::GradientStop {
                    color: elements::Color::Hex("#00FF00".to_string()),
                    position: 0.5,
                },
                elements::GradientStop {
                    color: elements::Color::Hex("#0000FF".to_string()),
                    position: 1.0,
                },
            ],
            direction: GradientDirection::Vertical,
            corner_radius: 0.0,
        }];
        let template = simple_template(layers);
        let ctx = TemplateContext::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grad3.png");

        render_template_to_png(&template, &ctx, &path).unwrap();
        assert!(path.exists());
    }
}
