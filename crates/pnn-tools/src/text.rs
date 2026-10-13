//! Text layout and rasterisation for the Text tool (cosmic-text: system fonts via fontdb,
//! shaping, swash rasterisation).
//!
//! Behaviour from paint.net 5's docs (`TextTool.html`): font, size (points at the image's
//! DPI, or "Fixed (96 DPI)"), bold, italic, underline, strikeout, left/centre/right
//! justification extending from the click point, antialiasing on/off, primary colour only (5.x
//! dropped fill styles for text). Not from paint.net: line height (1.2 × size), underline
//! and strikeout placement and thickness, and the aliased look (a 50% coverage threshold;
//! 5.x's "Sharp" DirectWrite modes have no Linux equivalent).

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
};
use pnn_core::Rect;

use crate::TextAlignment;

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    /// Em size in pixels.
    pub size_px: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub align: TextAlignment,
    pub antialias: bool,
}

/// Where each line went, for the caret.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineLayout {
    /// Byte offset of the line in the text.
    pub start: usize,
    pub top: f32,
    pub height: f32,
    pub left: f32,
    pub width: f32,
    /// (byte offset in the text, x) at each glyph's start, plus the line's end.
    pub stops: Vec<(usize, f32)>,
}

pub struct Rendered {
    /// Canvas area covered (clipped to the canvas); `None` if nothing is visible.
    pub bounds: Option<Rect>,
    /// Coverage over `bounds`, row-major.
    pub coverage: Vec<u8>,
    pub lines: Vec<LineLayout>,
}

/// Font size in pixels: points at `dpi`, or at 96 DPI for the "Fixed (96 DPI)" metric.
pub fn size_px(size: f32, points_at_image_dpi: bool, dpi: f64) -> f32 {
    let dpi = if points_at_image_dpi {
        dpi as f32
    } else {
        96.0
    };
    size * dpi / 72.0
}

/// Lay out and rasterise `text` with its anchor (top of the first line; left, centre or right
/// edge by alignment) at `origin`, onto a `width`×`height` canvas.
pub fn render(
    fonts: &mut FontSystem,
    cache: &mut SwashCache,
    text: &str,
    style: &TextStyle,
    origin: [f32; 2],
    width: u32,
    height: u32,
) -> Rendered {
    let size = style.size_px.max(1.0);
    let line_height = (size * 1.2).ceil();
    let mut buffer = Buffer::new(fonts, Metrics::new(size, line_height));
    buffer.set_size(None, None);
    let attrs = Attrs::new()
        .family(Family::Name(&style.family))
        .weight(if style.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        })
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        });
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);

    // Line starts in the text.
    let mut starts = vec![0usize];
    for (i, c) in text.char_indices() {
        if c == '\n' {
            starts.push(i + 1);
        }
    }
    let mut lines: Vec<LineLayout> = Vec::new();
    let mut glyphs: Vec<(cosmic_text::CacheKey, i32, i32)> = Vec::new();
    for run in buffer.layout_runs() {
        let left = match style.align {
            TextAlignment::Left => origin[0],
            TextAlignment::Center => origin[0] - run.line_w / 2.0,
            TextAlignment::Right => origin[0] - run.line_w,
        };
        let start = starts.get(run.line_i).copied().unwrap_or(0);
        let mut stops: Vec<(usize, f32)> = run
            .glyphs
            .iter()
            .map(|g| (start + g.start, left + g.x))
            .collect();
        stops.push((start + run.text.len(), left + run.line_w));
        let top = origin[1] + run.line_top;
        for g in run.glyphs {
            let p = g.physical((left, origin[1] + run.line_y), 1.0);
            glyphs.push((p.cache_key, p.x, p.y));
        }
        lines.push(LineLayout {
            start,
            top,
            height: run.line_height,
            left,
            width: run.line_w,
            stops,
        });
    }
    // Empty text still gets a caret line.
    if lines.is_empty() {
        let left = origin[0];
        lines.push(LineLayout {
            start: 0,
            top: origin[1],
            height: line_height,
            left,
            width: 0.0,
            stops: vec![(0, left)],
        });
    }

    // Rasterise into a canvas-sized-at-most area.
    let mut pieces: Vec<(i32, i32, u32, u32, Vec<u8>)> = Vec::new();
    for (key, x, y) in glyphs {
        let Some(img) = cache.get_image_uncached(fonts, key) else {
            continue;
        };
        let (w, h) = (img.placement.width, img.placement.height);
        if w == 0 || h == 0 {
            continue;
        }
        let alpha: Vec<u8> = match img.content {
            SwashContent::Mask => img.data.clone(),
            SwashContent::Color => img.data.chunks(4).map(|c| c[3]).collect(),
            SwashContent::SubpixelMask => img.data.chunks(4).map(|c| c[1]).collect(),
        };
        pieces.push((x + img.placement.left, y - img.placement.top, w, h, alpha));
    }
    // Underline and strikeout bars.
    let thickness = (size / 14.0).max(1.0).round();
    for (line, run) in lines.iter().zip(buffer.layout_runs()) {
        let baseline = origin[1] + run.line_y;
        let mut bar = |y: f32| {
            let w = line.width.ceil() as u32;
            if w > 0 {
                let (x0, y0) = (line.left.round() as i32, y.round() as i32);
                pieces.push((
                    x0,
                    y0,
                    w,
                    thickness as u32,
                    vec![255; (w * thickness as u32) as usize],
                ));
            }
        };
        if style.underline {
            bar(baseline + size * 0.1);
        }
        if style.strikeout {
            bar(baseline - size * 0.3);
        }
    }

    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y, w, h, _) in &pieces {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(x + *w as i32);
        y1 = y1.max(y + *h as i32);
    }
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(width as i32), y1.min(height as i32));
    if pieces.is_empty() || x1 <= x0 || y1 <= y0 {
        return Rendered {
            bounds: None,
            coverage: Vec::new(),
            lines,
        };
    }
    let r = Rect {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    };
    let mut coverage = vec![0u8; (r.width * r.height) as usize];
    for (px, py, w, h, alpha) in pieces {
        for j in 0..h as i32 {
            let y = py + j;
            if y < y0 || y >= y1 {
                continue;
            }
            for i in 0..w as i32 {
                let x = px + i;
                if x < x0 || x >= x1 {
                    continue;
                }
                let mut a = alpha[(j * w as i32 + i) as usize];
                if !style.antialias {
                    a = if a >= 128 { 255 } else { 0 };
                }
                let k = ((y - y0) as u32 * r.width + (x - x0) as u32) as usize;
                coverage[k] = coverage[k].max(a);
            }
        }
    }
    Rendered {
        bounds: Some(r),
        coverage,
        lines,
    }
}

/// Caret position (x, line top, line height) for byte offset `at`.
pub fn caret(lines: &[LineLayout], at: usize) -> (f32, f32, f32) {
    let line = lines
        .iter()
        .rev()
        .find(|l| l.start <= at)
        .or(lines.first())
        .cloned()
        .unwrap_or_default();
    let x = line
        .stops
        .iter()
        .find(|(i, _)| *i >= at)
        .or(line.stops.last())
        .map_or(line.left, |s| s.1);
    (x, line.top, line.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        TextStyle {
            family: "DejaVu Sans".into(),
            size_px: 20.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            align: TextAlignment::Left,
            antialias: true,
        }
    }

    fn ink(r: &Rendered) -> usize {
        r.coverage.iter().filter(|&&c| c > 0).count()
    }

    #[test]
    fn renders_text_from_the_anchor_and_aligns() {
        let mut fonts = FontSystem::new();
        let mut cache = SwashCache::new();
        let r = render(
            &mut fonts,
            &mut cache,
            "Hello",
            &style(),
            [50.0, 20.0],
            300,
            100,
        );
        let b = r.bounds.expect("visible");
        assert!(b.x >= 50 && b.x <= 53 && b.y >= 20 && b.y < 30, "{b:?}");
        assert!(ink(&r) > 100);
        let w = r.lines[0].width;
        assert!(w > 40.0 && w < 70.0, "width {w}");
        let mut s = style();
        s.align = TextAlignment::Right;
        let rr = render(&mut fonts, &mut cache, "Hello", &s, [200.0, 20.0], 300, 100);
        let b = rr.bounds.unwrap();
        assert!(
            (b.x + b.width) as f32 <= 201.0 && (b.x + b.width) as f32 > 195.0,
            "{b:?}"
        );
        s.align = TextAlignment::Center;
        let rc = render(&mut fonts, &mut cache, "Hello", &s, [150.0, 20.0], 300, 100);
        assert!((rc.lines[0].left + w / 2.0 - 150.0).abs() < 0.5);
    }

    #[test]
    fn styles_change_the_ink() {
        let mut fonts = FontSystem::new();
        let mut cache = SwashCache::new();
        let plain = render(
            &mut fonts,
            &mut cache,
            "Text",
            &style(),
            [10.0, 10.0],
            200,
            60,
        );
        let mut s = style();
        s.bold = true;
        let bold = render(&mut fonts, &mut cache, "Text", &s, [10.0, 10.0], 200, 60);
        assert!(ink(&bold) > ink(&plain), "bold is heavier");
        let mut s = style();
        s.underline = true;
        let u = render(&mut fonts, &mut cache, "Text", &s, [10.0, 10.0], 200, 60);
        assert!(
            u.bounds.unwrap().height > plain.bounds.unwrap().height,
            "underline below"
        );
        let mut s = style();
        s.antialias = false;
        let a = render(&mut fonts, &mut cache, "Text", &s, [10.0, 10.0], 200, 60);
        assert!(a.coverage.iter().all(|&c| c == 0 || c == 255), "aliased");
    }

    #[test]
    fn caret_follows_lines() {
        let mut fonts = FontSystem::new();
        let mut cache = SwashCache::new();
        let r = render(
            &mut fonts,
            &mut cache,
            "ab\ncd",
            &style(),
            [10.0, 10.0],
            200,
            100,
        );
        assert_eq!(r.lines.len(), 2);
        let (x0, t0, _) = caret(&r.lines, 0);
        let (x2, _, _) = caret(&r.lines, 2);
        let (x3, t3, _) = caret(&r.lines, 3);
        assert_eq!(x0, 10.0);
        assert!(x2 > x0, "end of first line");
        assert_eq!(x3, 10.0);
        assert!(t3 > t0, "second line below");
        assert_eq!(size_px(12.0, true, 96.0), 16.0);
        assert_eq!(size_px(12.0, true, 300.0), 50.0);
        assert_eq!(size_px(12.0, false, 300.0), 16.0);
    }
}
