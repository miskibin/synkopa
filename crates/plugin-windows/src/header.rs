//! WAVE's header above a plugin's own interface (FL Studio's wrapper bar): the window's name,
//! a gear that opens WAVE's settings for the plugin, and a close button. The OS frame is
//! off; the header is the title bar (drag it to move the window).
//!
//! Drawn in software into a `0x00RRGGBB` pixel buffer, identical on every platform: Win32
//! blits it with `StretchDIBits`, X11 with `PutImage`. Text is Geist (the app's UI font,
//! SIL OFL, `fonts/`), rasterised by `fontdue`.

use std::sync::OnceLock;

/// Header height at 100 %, in pixels.
pub const HEIGHT: f32 = 30.0;
/// Width of each button at 100 %.
const BUTTON_W: f32 = 30.0;
/// Text size at 100 %.
const TEXT_PX: f32 = 12.0;
const PAD: f32 = 10.0;

// WAVE's dark theme (ui/src/styles.css)
const BG: u32 = 0x171b21; // --bg5
const RULE: u32 = 0x0a0c0f; // --bg0
const FG: u32 = 0xd5dae2; // --fg
const FG_DIM: u32 = 0x7d8592; // --fg4
const ICON: u32 = 0x9aa2ad; // --fg3
const ICON_HI: u32 = 0xf2f4f7; // --fg-hi
const HOVER: u32 = 0x252b34; // --line
const CLOSE_HOVER: u32 = 0xe05263; // --red
const ACCENT: u32 = 0x4f8ef7; // --blue

/// A part of the header under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Moves the window.
    Drag,
    /// Opens WAVE's settings for this plugin.
    Settings,
    Close,
}

/// What the header shows. `hover` / `pressed` are the button under the pointer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub hover: Option<Part>,
    pub pressed: Option<Part>,
}

/// Header height in device pixels at `scale`.
pub fn height(scale: f32) -> u32 {
    (HEIGHT * scale.max(0.5)).round() as u32
}

/// The part of a `width`-pixel header at (`x`, `y`), device pixels.
pub fn hit(width: u32, x: i32, y: i32, scale: f32) -> Option<Part> {
    let h = height(scale) as i32;
    if y < 0 || y >= h || x < 0 || x >= width as i32 {
        return None;
    }
    let bw = (BUTTON_W * scale).round() as i32;
    let right = width as i32;
    Some(if x >= right - bw {
        Part::Close
    } else if x >= right - 2 * bw {
        Part::Settings
    } else {
        Part::Drag
    })
}

struct Fonts {
    main: fontdue::Font,
    ext: fontdue::Font,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let load =
            |bytes: &[u8]| fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("the bundled Geist font parses");
        Fonts { main: load(include_bytes!("../fonts/Geist-Medium.ttf")), ext: load(include_bytes!("../fonts/Geist-Medium-LatinExt.ttf")) }
    })
}

/// The font that has `c`, and the character to draw (`?` when neither has it).
fn font_for(c: char) -> (&'static fontdue::Font, char) {
    let f = fonts();
    if f.main.lookup_glyph_index(c) != 0 {
        (&f.main, c)
    } else if f.ext.lookup_glyph_index(c) != 0 {
        (&f.ext, c)
    } else {
        (&f.main, '?')
    }
}

fn advance(c: char, px: f32) -> f32 {
    let (font, c) = font_for(c);
    font.metrics(c, px).advance_width
}

fn text_width(s: &str, px: f32) -> f32 {
    s.chars().map(|c| advance(c, px)).sum()
}

/// `s` cut to `max` pixels with an ellipsis (empty when not even that fits).
pub fn fit(s: &str, px: f32, max: f32) -> String {
    if text_width(s, px) <= max {
        return s.to_owned();
    }
    let dots = advance('…', px);
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let a = advance(c, px);
        if w + a + dots > max {
            break;
        }
        w += a;
        out.push(c);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}…")
    }
}

/// A `width` x `height` image to draw into.
pub struct Canvas<'a> {
    pub px: &'a mut [u32],
    pub width: u32,
    pub height: u32,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
        if alpha <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = (y as u32 * self.width + x as u32) as usize;
        let a = alpha.min(1.0);
        let mix = |shift: u32| {
            let d = ((self.px[i] >> shift) & 0xff) as f32;
            let s = ((color >> shift) & 0xff) as f32;
            ((d + (s - d) * a).round() as u32) << shift
        };
        self.px[i] = mix(16) | mix(8) | mix(0);
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        for y in y0.max(0)..y1.min(self.height as i32) {
            for x in x0.max(0)..x1.min(self.width as i32) {
                self.px[(y as u32 * self.width + x as u32) as usize] = color;
            }
        }
    }

    /// Text with its baseline at `y`, from `x`.
    fn text(&mut self, s: &str, x: f32, baseline: f32, px: f32, color: u32) {
        let mut pen = x;
        for c in s.chars() {
            let (font, c) = font_for(c);
            let (m, bitmap) = font.rasterize(c, px);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bitmap[row * m.width + col] as f32 / 255.0;
                    self.blend(gx + col as i32, gy + row as i32, color, a);
                }
            }
            pen += m.advance_width;
        }
    }

    /// Coverage-sampled shape: `inside(x, y)` in pixel coordinates, 4x4 samples per pixel.
    fn shape(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, inside: impl Fn(f32, f32) -> bool) {
        for y in y0..y1 {
            for x in x0..x1 {
                let mut n = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        if inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) {
                            n += 1;
                        }
                    }
                }
                self.blend(x, y, color, n as f32 / 16.0);
            }
        }
    }
}

fn gear(c: &mut Canvas, cx: f32, cy: f32, s: f32, color: u32) {
    let (outer, body, hole, teeth) = (6.6 * s, 4.9 * s, 2.0 * s, 8.0);
    let r = outer.ceil() as i32 + 1;
    c.shape(cx as i32 - r, cy as i32 - r, cx as i32 + r + 1, cy as i32 + r + 1, color, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let d = (dx * dx + dy * dy).sqrt();
        if d < hole {
            return false;
        }
        if d <= body {
            return true;
        }
        if d > outer {
            return false;
        }
        // a tooth: within half a tooth's angular width of its centre line
        let a = dy.atan2(dx) * teeth / std::f32::consts::TAU;
        let frac = (a - a.round()).abs();
        frac < 0.22
    });
}

fn cross(c: &mut Canvas, cx: f32, cy: f32, s: f32, color: u32) {
    let (half, th) = (4.2 * s, 0.75 * s);
    let r = half.ceil() as i32 + 2;
    c.shape(cx as i32 - r, cy as i32 - r, cx as i32 + r + 1, cy as i32 + r + 1, color, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        if dx.abs() > half + th || dy.abs() > half + th {
            return false;
        }
        let d1 = (dx - dy).abs() / std::f32::consts::SQRT_2;
        let d2 = (dx + dy).abs() / std::f32::consts::SQRT_2;
        d1.min(d2) <= th
    });
}

impl Header {
    /// Draw the header across the top `height(scale)` rows of `canvas`.
    pub fn render(&self, canvas: &mut Canvas, scale: f32) {
        let s = scale.max(0.5);
        let h = height(s) as i32;
        let w = canvas.width as i32;
        canvas.fill(0, 0, w, h, BG);
        canvas.fill(0, h - 1, w, h, RULE);
        let bw = (BUTTON_W * s).round() as i32;
        let state = |p: Part| (self.hover == Some(p), self.pressed == Some(p));
        // close
        let (hov, down) = state(Part::Close);
        if hov || down {
            canvas.fill(w - bw, 0, w, h - 1, CLOSE_HOVER);
        }
        cross(canvas, (w - bw) as f32 + bw as f32 / 2.0, (h - 1) as f32 / 2.0, s, if hov || down { ICON_HI } else { ICON });
        // settings
        let (hov, down) = state(Part::Settings);
        if hov || down {
            canvas.fill(w - 2 * bw, 0, w - bw, h - 1, if down { ACCENT } else { HOVER });
        }
        gear(canvas, (w - 2 * bw) as f32 + bw as f32 / 2.0, (h - 1) as f32 / 2.0, s, if hov || down { ICON_HI } else { ICON });
        // title and subtitle
        let px = TEXT_PX * s;
        let baseline = ((h - 1) as f32 / 2.0 + px * 0.36).round();
        let room = (w - 2 * bw) as f32 - 2.0 * PAD * s;
        let title = fit(&self.title, px, room);
        let tw = text_width(&title, px);
        canvas.text(&title, PAD * s, baseline, px, FG);
        let gap = 8.0 * s;
        if !self.subtitle.is_empty() && title == self.title && room - tw - gap > 20.0 * s {
            let sub = fit(&self.subtitle, px, room - tw - gap);
            canvas.text(&sub, PAD * s + tw + gap, baseline, px, FG_DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_is_split_into_drag_settings_and_close() {
        assert_eq!(height(1.0), 30);
        assert_eq!(height(2.0), 60);
        assert_eq!(hit(400, 5, 5, 1.0), Some(Part::Drag));
        assert_eq!(hit(400, 399, 5, 1.0), Some(Part::Close));
        assert_eq!(hit(400, 370, 5, 1.0), Some(Part::Close));
        assert_eq!(hit(400, 369, 5, 1.0), Some(Part::Settings));
        assert_eq!(hit(400, 340, 29, 1.0), Some(Part::Settings));
        assert_eq!(hit(400, 339, 5, 1.0), Some(Part::Drag));
        assert_eq!(hit(400, 5, 30, 1.0), None, "below the header is the plugin");
        assert_eq!(hit(800, 739, 10, 2.0), Some(Part::Settings));
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        assert_eq!(fit("Kick", 12.0, 500.0), "Kick");
        let cut = fit("A very long plugin name that cannot fit in the header", 12.0, 80.0);
        assert!(cut.ends_with('…') && cut.len() < 30, "{cut}");
        assert!(text_width(&cut, 12.0) <= 80.0);
        assert_eq!(fit("Name", 12.0, 2.0), "");
        assert_eq!(fit("Zażółć gęślą", 12.0, 500.0), "Zażółć gęślą", "Polish letters come from the latin-ext subset");
    }

    #[test]
    fn rendering_draws_the_name_and_both_icons() {
        let (w, s) = (360_u32, 1.0);
        let h = height(s);
        let mut px = vec![0; (w * h) as usize];
        let header = Header { title: "Vitalium".into(), subtitle: "VST3".into(), hover: Some(Part::Settings), pressed: None };
        header.render(&mut Canvas { px: &mut px, width: w, height: h }, s);
        let region =
            |x0: u32, x1: u32| (0..h).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| px[(y * w + x) as usize] != BG).count();
        assert!(region(10, 80) > 40, "the title is drawn");
        assert!(region(w - 60, w - 30) > 100, "the hovered gear button is highlighted");
        assert!(region(w - 30, w) > 10, "the close cross is drawn");
        assert_eq!(px[((h - 1) * w + 5) as usize], RULE, "a rule under the header");
        assert!(px.iter().all(|p| p >> 24 == 0), "no alpha byte: Win32 and X11 read 0x00RRGGBB");
    }
}
