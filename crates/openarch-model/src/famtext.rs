//! Letters for Model Text: glyph outlines from a built-in font, laid out in lines on a work plane.
//!
//! The built-in font is DejaVu Sans (regular and bold; italic is the regular face sheared). Any other
//! font name is kept in the family and drawn with DejaVu Sans until fonts on the computer can be read.

use crate::geom::Point;
use serde::{Deserialize, Serialize};
use ttf_parser::{Face, OutlineBuilder};

const REGULAR: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

/// The font a new piece of text gets.
pub const DEFAULT_FONT: &str = "DejaVu Sans";
/// Fonts whose own letters are drawn.
pub const BUILT_IN_FONTS: [&str; 1] = [DEFAULT_FONT];
const MAX_CHARS: usize = 2000;
/// Slant of the italic: tan 12 degrees.
const SHEAR: f64 = 0.2126;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

pub struct Laid {
    /// Closed outlines (letters and the openings in them) in mm, the first baseline on y = 0 and the anchor on x = 0.
    pub rings: Vec<Vec<Point>>,
    pub warnings: Vec<String>,
}

struct Collect {
    rings: Vec<Vec<Point>>,
    cur: Vec<Point>,
    scale: f64,
    dx: f64,
    dy: f64,
    italic: bool,
}

impl Collect {
    fn at(&self, x: f32, y: f32) -> Point {
        let (px, py) = (x as f64 * self.scale, y as f64 * self.scale);
        Point::new(self.dx + px + if self.italic { py * SHEAR } else { 0.0 }, self.dy + py)
    }
    fn last(&self) -> Point {
        *self.cur.last().unwrap_or(&Point::new(0.0, 0.0))
    }
}

impl OutlineBuilder for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = self.at(x, y);
        self.cur.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.cur.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, c, p1) = (self.last(), self.at(x1, y1), self.at(x, y));
        const N: usize = 6;
        for i in 1..=N {
            let t = i as f64 / N as f64;
            let m = 1.0 - t;
            self.cur.push(Point::new(m * m * p0.x + 2.0 * m * t * c.x + t * t * p1.x, m * m * p0.y + 2.0 * m * t * c.y + t * t * p1.y));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, c1, c2, p1) = (self.last(), self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        const N: usize = 8;
        for i in 1..=N {
            let t = i as f64 / N as f64;
            let m = 1.0 - t;
            let (a, b, c, d) = (m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t);
            self.cur.push(Point::new(a * p0.x + b * c1.x + c * c2.x + d * p1.x, a * p0.y + b * c1.y + c * c2.y + d * p1.y));
        }
    }
    fn close(&mut self) {
        let mut ring = std::mem::take(&mut self.cur);
        ring.dedup_by(|a, b| a.dist(b) < 1e-6);
        if ring.len() > 1 && ring[0].dist(ring.last().unwrap()) < 1e-6 {
            ring.pop();
        }
        if ring.len() >= 3 && crate::geom::signed_area(&ring).abs() > 1e-9 {
            self.rings.push(ring);
        }
    }
}

/// Lay `text` out. `size` is the height of a capital letter in mm; a new line steps down by the font's line height.
pub fn layout(font: &str, bold: bool, italic: bool, text: &str, size: f64, align: TextAlign) -> Result<Laid, String> {
    if !(size > 0.0) || !size.is_finite() {
        return Err("the text size must be above zero".into());
    }
    if text.chars().count() > MAX_CHARS {
        return Err(format!("model text is limited to {MAX_CHARS} characters"));
    }
    let mut warnings = Vec::new();
    if !font.trim().is_empty() && !BUILT_IN_FONTS.iter().any(|f| f.eq_ignore_ascii_case(font.trim())) {
        warnings.push(format!("the font '{}' is not available here: {DEFAULT_FONT} is used", font.trim()));
    }
    let face = Face::parse(if bold { BOLD } else { REGULAR }, 0).map_err(|_| "the built-in font could not be read".to_string())?;
    let upem = face.units_per_em() as f64;
    let cap = face.capital_height().map(|c| c as f64).filter(|c| *c > 0.0).unwrap_or(0.73 * upem);
    let scale = size / cap;
    let line_h = (face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) * scale;
    let advance = |c: char| -> f64 {
        let g = face.glyph_index(c).or_else(|| face.glyph_index(' '));
        g.and_then(|g| face.glyph_hor_advance(g)).unwrap_or((upem * 0.3) as u16) as f64 * scale
    };
    let mut col = Collect { rings: vec![], cur: vec![], scale, dx: 0.0, dy: 0.0, italic };
    let mut missing: Vec<char> = Vec::new();
    for (n, line) in text.replace('\t', "    ").split('\n').enumerate() {
        let line = line.trim_end_matches('\r');
        let width: f64 = line.chars().map(advance).sum();
        let mut pen = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -width / 2.0,
            TextAlign::Right => -width,
        };
        for ch in line.chars() {
            if let Some(g) = face.glyph_index(ch) {
                col.dx = pen;
                col.dy = -(n as f64) * line_h;
                col.cur.clear();
                if face.outline_glyph(g, &mut col).is_some() {
                    col.close();
                }
            } else if !ch.is_whitespace() && !missing.contains(&ch) {
                missing.push(ch);
            }
            pen += advance(ch);
        }
    }
    if !missing.is_empty() {
        warnings.push(format!("no letter for {} in {DEFAULT_FONT}", missing.iter().map(|c| format!("'{c}'")).collect::<Vec<_>>().join(", ")));
    }
    Ok(Laid { rings: col.rings, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::signed_area;

    fn bounds(rings: &[Vec<Point>]) -> (f64, f64, f64, f64) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for r in rings {
            for p in r {
                x0 = x0.min(p.x);
                x1 = x1.max(p.x);
                y0 = y0.min(p.y);
                y1 = y1.max(p.y);
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn a_capital_is_the_size_asked_for_and_o_has_an_opening() {
        let h = layout("DejaVu Sans", false, false, "H", 100.0, TextAlign::Left).unwrap();
        let (_, y0, _, y1) = bounds(&h.rings);
        assert!((y1 - y0 - 100.0).abs() < 1.0, "cap height {}", y1 - y0);
        assert!(y0.abs() < 1.0, "sits on the baseline");
        let o = layout("DejaVu Sans", false, false, "O", 100.0, TextAlign::Left).unwrap();
        assert_eq!(o.rings.len(), 2, "an outline and the hole");
        let (a, b) = (signed_area(&o.rings[0]).abs(), signed_area(&o.rings[1]).abs());
        assert!(a.max(b) > 1.5 * a.min(b));
    }

    #[test]
    fn alignment_lines_bold_italic_and_missing_letters() {
        let l = layout("", false, false, "ABC", 100.0, TextAlign::Left).unwrap();
        let c = layout("", false, false, "ABC", 100.0, TextAlign::Center).unwrap();
        let r = layout("", false, false, "ABC", 100.0, TextAlign::Right).unwrap();
        let (lx0, _, lx1, _) = bounds(&l.rings);
        let (cx0, _, cx1, _) = bounds(&c.rings);
        let (rx0, _, rx1, _) = bounds(&r.rings);
        let w = lx1 - lx0;
        assert!(lx0 >= -1.0 && lx0 < 20.0);
        assert!(((cx0 + cx1) / 2.0).abs() < 25.0, "centred about the anchor");
        assert!(rx1 <= 1.0 && rx1 > -20.0 && (rx1 - rx0 - w).abs() < 1e-6);
        // two lines step down
        let two = layout("", false, false, "A\nA", 100.0, TextAlign::Left).unwrap();
        let (_, y0, _, y1) = bounds(&two.rings);
        assert!(y1 - y0 > 200.0 && y0 < -100.0);
        // bold is wider; italic leans right at the top
        let reg = layout("", false, false, "l", 100.0, TextAlign::Left).unwrap();
        let bld = layout("", true, false, "l", 100.0, TextAlign::Left).unwrap();
        let (a0, _, a1, _) = bounds(&reg.rings);
        let (b0, _, b1, _) = bounds(&bld.rings);
        assert!(b1 - b0 > a1 - a0);
        let ital = layout("", false, true, "l", 100.0, TextAlign::Left).unwrap();
        let top_x = |rings: &[Vec<Point>]| rings.iter().flatten().filter(|p| p.y > 90.0).map(|p| p.x).fold(f64::MIN, f64::max);
        assert!(top_x(&ital.rings) > top_x(&reg.rings) + 10.0);
        // an unknown font and a letter the font lacks are reported, not fatal
        let w = layout("Arial", false, false, "A\u{4e2d}", 100.0, TextAlign::Left).unwrap();
        assert_eq!(w.warnings.len(), 2, "{:?}", w.warnings);
        assert!(!w.rings.is_empty());
        assert!(layout("", false, false, "A", 0.0, TextAlign::Left).is_err());
        assert!(layout("", false, false, "   ", 10.0, TextAlign::Left).unwrap().rings.is_empty());
    }
}
