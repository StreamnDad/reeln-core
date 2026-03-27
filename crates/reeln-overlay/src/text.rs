//! Font loading, text shaping, measurement, and rendering via cosmic-text.
//!
//! Provides a `TextMeasurer` trait with two implementations:
//! - `SimpleTextMeasurer` — character-width estimate (no font loading)
//! - `CosmicTextMeasurer` — real font shaping via cosmic-text

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use tiny_skia::Pixmap;

use crate::elements::{self, Alignment, Color};
use crate::error::OverlayError;

/// Trait for abstracting text measurement operations.
pub trait TextMeasurer: Send + Sync {
    /// Measure the width and height of `text` rendered at `font_size`
    /// using the given `font_family`.
    fn measure(&self, text: &str, font_family: &str, font_size: f32) -> (f32, f32);
}

/// A simple character-width-based text measurer for use as a fallback.
///
/// Estimates width as `chars * font_size * 0.6` and height as `font_size * 1.2`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimpleTextMeasurer;

impl TextMeasurer for SimpleTextMeasurer {
    fn measure(&self, text: &str, _font_family: &str, font_size: f32) -> (f32, f32) {
        let width = text.chars().count() as f32 * font_size * 0.6;
        let height = font_size * 1.2;
        (width, height)
    }
}

/// Reduce font size until text fits within `max_width`.
///
/// Starts at `base_size` and reduces by 1.0 until the measured width fits
/// or `min_size` is reached. Returns the computed font size.
pub fn auto_shrink_font_size(
    measurer: &dyn TextMeasurer,
    text: &str,
    max_width: f32,
    font_family: &str,
    base_size: f32,
    min_size: f32,
) -> f32 {
    let mut size = base_size;
    while size > min_size {
        let (width, _) = measurer.measure(text, font_family, size);
        if width <= max_width {
            return size;
        }
        size -= 1.0;
    }
    min_size
}

/// Convenience function using the `SimpleTextMeasurer`.
pub fn measure_text(text: &str, font_family: &str, font_size: f32) -> (f32, f32) {
    SimpleTextMeasurer.measure(text, font_family, font_size)
}

// ── cosmic-text rendering ───────────────────────────────────────────

/// Render text onto a pixmap at the given position using cosmic-text.
///
/// Handles font loading, text shaping, alignment, auto-shrink, and
/// per-pixel alpha blending onto the existing pixmap content.
pub fn render_text_to_pixmap(
    pixmap: &mut Pixmap,
    text: &str,
    x: f32,
    y: f32,
    font_family: &str,
    font_size: f32,
    font_weight: Option<&str>,
    color: &Color,
    alignment: &Alignment,
    max_width: Option<f32>,
    auto_shrink_min: Option<f32>,
    outline: Option<&elements::OutlineSpec>,
) -> Result<(), OverlayError> {
    if text.is_empty() {
        return Ok(());
    }

    let (r, g, b, a) = elements::parse_color(match color {
        Color::Hex(s) => s,
    })?;

    let mut font_system = FontSystem::new();
    let mut swash_cache = SwashCache::new();

    // Determine effective font size with auto-shrink.
    let effective_size = if let (Some(max_w), Some(min_size)) = (max_width, auto_shrink_min) {
        auto_shrink_cosmic(
            &mut font_system,
            text,
            font_family,
            font_weight,
            font_size,
            max_w,
            min_size,
        )
    } else {
        font_size
    };

    let line_height = effective_size * 1.3;
    let metrics = Metrics::new(effective_size, line_height);
    let mut buffer = Buffer::new(&mut font_system, metrics);

    // Set buffer width for layout.
    let buf_width = max_width.unwrap_or(pixmap.width() as f32 - x);
    buffer.set_size(&mut font_system, Some(buf_width), None);

    // Build text attributes.
    let weight = match font_weight {
        Some("bold") | Some("Bold") => Weight::BOLD,
        Some("light") | Some("Light") => Weight(300),
        _ => Weight::NORMAL,
    };
    let attrs = Attrs::new()
        .family(Family::Name(font_family))
        .weight(weight);

    buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
    buffer.shape_until_scroll(&mut font_system, true);

    // Measure actual layout width for alignment.
    let layout_width: f32 = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max);

    let x_offset = match alignment {
        Alignment::Left => x,
        Alignment::Center => x + (buf_width - layout_width) / 2.0,
        Alignment::Right => x + buf_width - layout_width,
    };

    let text_color = cosmic_text::Color::rgba(r, g, b, a);

    // Render outline first (if specified) by drawing text offset in 4+ directions.
    if let Some(outline_spec) = outline {
        let (or, og, ob, oa) = elements::parse_color(match &outline_spec.color {
            Color::Hex(s) => s,
        })?;
        let outline_color = cosmic_text::Color::rgba(or, og, ob, oa);
        let ow = outline_spec.width;

        // Draw in 8 directions for outline.
        let offsets = [
            (-ow, 0.0),
            (ow, 0.0),
            (0.0, -ow),
            (0.0, ow),
            (-ow, -ow),
            (ow, -ow),
            (-ow, ow),
            (ow, ow),
        ];
        for (dx, dy) in offsets {
            draw_buffer_to_pixmap(
                pixmap,
                &buffer,
                &mut font_system,
                &mut swash_cache,
                outline_color,
                x_offset + dx,
                y + dy,
            );
        }
    }

    // Draw the main text.
    draw_buffer_to_pixmap(
        pixmap,
        &buffer,
        &mut font_system,
        &mut swash_cache,
        text_color,
        x_offset,
        y,
    );

    Ok(())
}

/// Draw a cosmic-text buffer onto a tiny-skia pixmap with alpha blending.
fn draw_buffer_to_pixmap(
    pixmap: &mut Pixmap,
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    color: cosmic_text::Color,
    x_offset: f32,
    y_offset: f32,
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();

    buffer.draw(font_system, swash_cache, color, |px, py, _w, _h, c| {
        let x = px + x_offset as i32;
        let y = py + y_offset as i32;
        if x < 0 || y < 0 || x >= pw || y >= ph {
            return;
        }
        let [sr, sg, sb, sa] = c.as_rgba();
        if sa == 0 {
            return;
        }
        let idx = (y * pw + x) as usize;
        if idx >= pixels.len() {
            return;
        }

        // Alpha-blend source over destination.
        let dst = pixels[idx];
        let (dr, dg, db, da) = (dst.red(), dst.green(), dst.blue(), dst.alpha());

        if da == 0 {
            // Destination is transparent, just write premultiplied.
            pixels[idx] = tiny_skia::PremultipliedColorU8::from_rgba(
                (sr as u16 * sa as u16 / 255) as u8,
                (sg as u16 * sa as u16 / 255) as u8,
                (sb as u16 * sa as u16 / 255) as u8,
                sa,
            )
            .unwrap();
        } else {
            // Source-over compositing (both are premultiplied).
            let src_a = sa as u16;
            let inv_a = 255 - src_a;
            let sr_pm = sr as u16 * src_a / 255;
            let sg_pm = sg as u16 * src_a / 255;
            let sb_pm = sb as u16 * src_a / 255;

            let nr = (sr_pm + dr as u16 * inv_a / 255).min(255) as u8;
            let ng = (sg_pm + dg as u16 * inv_a / 255).min(255) as u8;
            let nb = (sb_pm + db as u16 * inv_a / 255).min(255) as u8;
            let na = (src_a + da as u16 * inv_a / 255).min(255) as u8;

            pixels[idx] = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na).unwrap();
        }
    });
}

/// Auto-shrink font size using cosmic-text for accurate measurement.
fn auto_shrink_cosmic(
    font_system: &mut FontSystem,
    text: &str,
    font_family: &str,
    font_weight: Option<&str>,
    base_size: f32,
    max_width: f32,
    min_size: f32,
) -> f32 {
    let weight = match font_weight {
        Some("bold") | Some("Bold") => Weight::BOLD,
        _ => Weight::NORMAL,
    };

    let mut size = base_size;
    while size > min_size {
        let metrics = Metrics::new(size, size * 1.3);
        let mut buffer = Buffer::new(font_system, metrics);
        // Use a large width so text doesn't wrap during measurement.
        buffer.set_size(font_system, Some(10000.0), None);
        let attrs = Attrs::new()
            .family(Family::Name(font_family))
            .weight(weight);
        buffer.set_text(font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(font_system, true);

        let width: f32 = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);

        if width <= max_width {
            return size;
        }
        size -= 1.0;
    }
    min_size
}

/// Measure text dimensions using cosmic-text for accurate shaping.
pub fn measure_text_cosmic(text: &str, font_family: &str, font_size: f32) -> (f32, f32) {
    let mut font_system = FontSystem::new();
    let metrics = Metrics::new(font_size, font_size * 1.3);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_size(&mut font_system, Some(10000.0), None);
    let attrs = Attrs::new().family(Family::Name(font_family));
    buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
    buffer.shape_until_scroll(&mut font_system, true);

    let width: f32 = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max);

    let height: f32 = buffer
        .layout_runs()
        .map(|run| run.line_top + run.line_height)
        .fold(0.0_f32, f32::max);

    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Alignment, Color, OutlineSpec};

    #[test]
    fn test_simple_measurer_empty() {
        let m = SimpleTextMeasurer;
        let (w, h) = m.measure("", "Arial", 20.0);
        assert!((w - 0.0).abs() < f32::EPSILON);
        assert!((h - 24.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_simple_measurer_single_char() {
        let m = SimpleTextMeasurer;
        let (w, h) = m.measure("A", "Arial", 10.0);
        assert!((w - 6.0).abs() < f32::EPSILON); // 1 * 10 * 0.6
        assert!((h - 12.0).abs() < f32::EPSILON); // 10 * 1.2
    }

    #[test]
    fn test_simple_measurer_multiple_chars() {
        let m = SimpleTextMeasurer;
        let (w, _) = m.measure("Hello", "Arial", 20.0);
        assert!((w - 60.0).abs() < 0.01); // 5 * 20 * 0.6
    }

    #[test]
    fn test_simple_measurer_ignores_font_family() {
        let m = SimpleTextMeasurer;
        let (w1, _) = m.measure("test", "Arial", 10.0);
        let (w2, _) = m.measure("test", "Helvetica", 10.0);
        assert!((w1 - w2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_simple_measurer_unicode() {
        let m = SimpleTextMeasurer;
        let (w, _) = m.measure("日本語", "Gothic", 10.0);
        assert!((w - 18.0).abs() < f32::EPSILON); // 3 chars * 10 * 0.6
    }

    #[test]
    fn test_auto_shrink_fits() {
        let m = SimpleTextMeasurer;
        // "Hi" at 20.0 => width = 2 * 20 * 0.6 = 24.0
        let size = auto_shrink_font_size(&m, "Hi", 100.0, "Arial", 20.0, 8.0);
        assert!((size - 20.0).abs() < f32::EPSILON); // already fits
    }

    #[test]
    fn test_auto_shrink_reduces() {
        let m = SimpleTextMeasurer;
        // "Hello World!" = 12 chars. At 20.0 => 12 * 20 * 0.6 = 144.0. max=100.
        // At 13.0 => 12 * 13 * 0.6 = 93.6 <= 100. Should return 13.0.
        let size = auto_shrink_font_size(&m, "Hello World!", 100.0, "Arial", 20.0, 8.0);
        // 12 * size * 0.6 <= 100 => size <= 100 / 7.2 ≈ 13.88
        // At 14: 12*14*0.6=100.8 > 100, at 13: 93.6 <= 100
        assert!((size - 13.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto_shrink_hits_min() {
        let m = SimpleTextMeasurer;
        // Very long text that won't fit even at min_size
        let long_text = "A".repeat(1000);
        let size = auto_shrink_font_size(&m, &long_text, 10.0, "Arial", 20.0, 8.0);
        assert!((size - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto_shrink_base_equals_min() {
        let m = SimpleTextMeasurer;
        let size = auto_shrink_font_size(&m, "Hello", 1.0, "Arial", 8.0, 8.0);
        assert!((size - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_measure_text_convenience() {
        let (w, h) = measure_text("AB", "Arial", 10.0);
        assert!((w - 12.0).abs() < f32::EPSILON);
        assert!((h - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_custom_measurer() {
        struct FixedMeasurer;
        impl TextMeasurer for FixedMeasurer {
            fn measure(&self, _text: &str, _font_family: &str, _font_size: f32) -> (f32, f32) {
                (50.0, 20.0)
            }
        }

        let m = FixedMeasurer;
        let (w, h) = m.measure("anything", "any", 10.0);
        assert!((w - 50.0).abs() < f32::EPSILON);
        assert!((h - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto_shrink_with_custom_measurer() {
        struct FixedMeasurer;
        impl TextMeasurer for FixedMeasurer {
            fn measure(&self, _text: &str, _font_family: &str, font_size: f32) -> (f32, f32) {
                // Always returns font_size * 10.0 as width
                (font_size * 10.0, font_size)
            }
        }

        let m = FixedMeasurer;
        // base=20 => width=200, max=150
        // 15 => 150, fits
        let size = auto_shrink_font_size(&m, "x", 150.0, "Arial", 20.0, 8.0);
        assert!((size - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto_shrink_empty_text() {
        let m = SimpleTextMeasurer;
        let size = auto_shrink_font_size(&m, "", 100.0, "Arial", 20.0, 8.0);
        assert!((size - 20.0).abs() < f32::EPSILON); // empty text width=0, fits immediately
    }

    // ── cosmic-text integration tests ──────────────────────────────

    #[test]
    fn test_measure_text_cosmic_non_empty() {
        let (w, h) = measure_text_cosmic("Hello", "sans-serif", 24.0);
        assert!(w > 0.0, "expected positive width, got {w}");
        assert!(h > 0.0, "expected positive height, got {h}");
    }

    #[test]
    fn test_measure_text_cosmic_empty() {
        let (w, _h) = measure_text_cosmic("", "sans-serif", 24.0);
        assert!((w - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_measure_text_cosmic_size_scales() {
        let (w1, _) = measure_text_cosmic("Hello", "sans-serif", 12.0);
        let (w2, _) = measure_text_cosmic("Hello", "sans-serif", 24.0);
        // Larger font should produce wider text.
        assert!(w2 > w1, "expected w2({w2}) > w1({w1})");
    }

    #[test]
    fn test_render_text_to_pixmap_basic() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Hello",
            10.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
        // Verify some pixels were drawn (not all transparent).
        let has_content = pixmap.pixels().iter().any(|p| p.alpha() > 0);
        assert!(has_content, "expected text pixels to be drawn");
    }

    #[test]
    fn test_render_text_to_pixmap_empty_text() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "",
            10.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
        // All pixels should be transparent.
        let all_transparent = pixmap.pixels().iter().all(|p| p.alpha() == 0);
        assert!(all_transparent);
    }

    #[test]
    fn test_render_text_to_pixmap_center_alignment() {
        let mut pixmap = Pixmap::new(400, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Hi",
            0.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#FF0000".to_string()),
            &Alignment::Center,
            Some(400.0),
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_text_to_pixmap_right_alignment() {
        let mut pixmap = Pixmap::new(400, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Hi",
            0.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#00FF00".to_string()),
            &Alignment::Right,
            Some(400.0),
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_text_to_pixmap_with_outline() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let outline = OutlineSpec {
            color: Color::Hex("#000000".to_string()),
            width: 2.0,
        };
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Outlined",
            10.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            None,
            None,
            Some(&outline),
        );
        assert!(result.is_ok());
        let has_content = pixmap.pixels().iter().any(|p| p.alpha() > 0);
        assert!(has_content);
    }

    #[test]
    fn test_render_text_to_pixmap_bold_weight() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Bold",
            10.0,
            10.0,
            "sans-serif",
            24.0,
            Some("bold"),
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_text_to_pixmap_auto_shrink() {
        let mut pixmap = Pixmap::new(100, 50).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Very long text that should shrink",
            0.0,
            0.0,
            "sans-serif",
            48.0,
            None,
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            Some(100.0),
            Some(8.0),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_text_invalid_color() {
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Hello",
            10.0,
            10.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("not_a_color".to_string()),
            &Alignment::Left,
            None,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_draw_buffer_to_pixmap_out_of_bounds() {
        // Text rendered far outside the pixmap should not crash.
        let mut pixmap = Pixmap::new(10, 10).unwrap();
        let result = render_text_to_pixmap(
            &mut pixmap,
            "Hello World!",
            -100.0,
            -100.0,
            "sans-serif",
            24.0,
            None,
            &Color::Hex("#FFFFFF".to_string()),
            &Alignment::Left,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
    }
}
