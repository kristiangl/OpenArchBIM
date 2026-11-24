//! The parametric material generator. A material's appearance can be a small set of parameters (an archetype, a
//! layout, unit sizes, joints, offsets, colours, a seed) instead of image files. From them this module makes, on
//! demand and always the same for the same parameters:
//! - a tileable domain (the least repeat of the units and their offsets), which is the material's UV tile size;
//! - albedo, normal, 16-bit height, roughness and metalness maps of that domain at any resolution;
//! - the matching model fill pattern: one hatch line family per run of joint, laid on the joint centrelines, so
//!   the vector pattern in Hidden Line, Shaded and Consistent Colours falls on the joints of the texture.
//!
//! Lengths are millimetres. The domain runs x to the right and y up from its lower left corner; images are written
//! top row first, so row 0 is y = height.

use serde::{Deserialize, Serialize};

use crate::styles::HatchLine;
use crate::view::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    /// Ceramic and stone tiles.
    #[default]
    Tile,
    /// Flooring, cladding and decking boards.
    WoodPlank,
    /// Solid timber, glulam and millwork.
    WoodSolid,
    /// Brickwork and concrete masonry units.
    Masonry,
    /// Cladding and roofing sheet.
    Metal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Joints straight through both ways.
    #[default]
    StackBond,
    /// Each course moved along by the offset (stretcher bond for masonry, staggered ends for boards).
    RunningBond,
    /// Rectangles at right angles, interlocked (turn the material 45 degrees for the diagonal look).
    Herringbone,
    /// Parallelograms meeting on a mitred centreline.
    Chevron,
    /// Squares of parallel units, turned in turn.
    Basketweave,
    /// Masonry: stretchers and headers in each course.
    Flemish,
    /// Masonry: a course of stretchers, then a course of headers.
    English,
    /// Boards: random lengths in each row, ends kept apart by the least stagger.
    Strip,
    /// Wide boards with narrow raised battens over their joints.
    BoardAndBatten,
    /// Solid timber with no joints.
    Seamless,
    /// Glue-laminated strips.
    Glulam,
    /// Metal: sinusoidal waves.
    Corrugated,
    /// Metal: trapezoidal ribs.
    Trapezoidal,
    /// Metal: pans with raised seams at their edges.
    StandingSeam,
    /// Metal: flat panels with reveal joints.
    Cassette,
    /// Metal: flat sheet with round holes.
    Perforated,
}

impl Layout {
    /// The layouts an archetype offers, the first being its default.
    pub fn for_archetype(a: Archetype) -> &'static [Layout] {
        use Layout::*;
        match a {
            Archetype::Tile => &[StackBond, RunningBond, Herringbone, Chevron, Basketweave],
            Archetype::WoodPlank => &[Strip, RunningBond, Herringbone, Chevron, BoardAndBatten],
            Archetype::WoodSolid => &[Seamless, Glulam],
            Archetype::Masonry => &[RunningBond, StackBond, Flemish, English],
            Archetype::Metal => &[Corrugated, Trapezoidal, StandingSeam, Cassette, Perforated],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    #[default]
    Matte,
    Satin,
    HighGloss,
    TexturedStone,
    /// Oiled or raw timber.
    Natural,
    Lacquered,
    MillFinish,
    Brushed,
    PowderCoated,
    Patina,
}

impl Finish {
    /// Face roughness for the finish (0 mirror, 1 fully diffuse).
    pub fn roughness(self) -> f64 {
        match self {
            Finish::Matte => 0.78,
            Finish::Satin => 0.45,
            Finish::HighGloss => 0.12,
            Finish::TexturedStone => 0.88,
            Finish::Natural => 0.68,
            Finish::Lacquered => 0.3,
            Finish::MillFinish => 0.32,
            Finish::Brushed => 0.42,
            Finish::PowderCoated => 0.55,
            Finish::Patina => 0.72,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MortarProfile {
    #[default]
    Flush,
    Raked,
    Concave,
    Weathered,
}

/// Everything the generator needs. Lengths in mm, percentages 0 to 100 unless noted. `unit_length` runs along a
/// course (tile width, plank length, brick length, panel width) and `unit_height` across it (tile height, plank
/// width, brick height, panel height).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneratorParams {
    pub archetype: Archetype,
    pub layout: Layout,
    pub unit_length: f64,
    pub unit_height: f64,
    /// Grout, mortar, seam or reveal width.
    pub joint_width: f64,
    /// How far the joint sits below the face.
    pub joint_depth: f64,
    /// Course offset: a percentage of a unit (with `offset_percent`) or a distance.
    pub offset: f64,
    pub offset_percent: bool,
    /// Boards: random end joints (Strip always is).
    pub random_stagger: bool,
    /// Boards: the least distance between end joints in neighbouring rows.
    pub min_stagger: f64,
    /// Per-unit tone variance.
    pub tone_variance: f64,
    pub finish: Finish,
    pub mortar_profile: MortarProfile,
    /// Masonry header length; 0 makes it half a stretcher less half a joint.
    pub header_length: f64,
    /// Masonry face roughness and pitting.
    pub pitting: f64,
    /// Masonry kiln burn variance (darker units here and there).
    pub burn_variance: f64,
    /// Wood: grain line density.
    pub grain_density: f64,
    /// Wood: knots.
    pub knot_frequency: f64,
    /// Wood: colour variation from board to board.
    pub plank_variation: f64,
    pub batten_width: f64,
    pub batten_depth: f64,
    /// Glulam strip width.
    pub lamination_width: f64,
    /// Wood solid: distance between growth rings.
    pub ring_spacing: f64,
    /// Wood solid: grain direction, 0 horizontal, 90 vertical.
    pub grain_angle: f64,
    /// Edge bevel (tiles, boards, panels).
    pub bevel: f64,
    /// Metal: corrugation or rib pitch.
    pub pitch: f64,
    /// Metal: corrugation or rib depth.
    pub rib_depth: f64,
    pub pan_width: f64,
    pub seam_height: f64,
    pub perforation_diameter: f64,
    pub perforation_pitch: f64,
    /// Metal: brushed grain.
    pub brushed: f64,
    pub primary_color: Color,
    pub joint_color: Color,
    /// Value, 0 to 200 % (100 leaves it).
    pub brightness: f64,
    /// Saturation, 0 to 200 % (100 leaves it).
    pub saturation: f64,
    /// Hue shift, -180 to 180 degrees.
    pub hue_shift: f64,
    pub seed: u32,
    /// Chevron angle from the horizontal, degrees.
    pub chevron_angle: f64,
}

impl Default for GeneratorParams {
    fn default() -> Self {
        GeneratorParams::preset(Archetype::Tile)
    }
}

impl GeneratorParams {
    /// Sensible starting values for an archetype (Australian sizes where they differ).
    pub fn preset(a: Archetype) -> GeneratorParams {
        let base = GeneratorParams {
            archetype: a,
            layout: Layout::for_archetype(a)[0],
            unit_length: 300.0,
            unit_height: 300.0,
            joint_width: 3.0,
            joint_depth: 2.0,
            offset: 50.0,
            offset_percent: true,
            random_stagger: false,
            min_stagger: 300.0,
            tone_variance: 15.0,
            finish: Finish::Satin,
            mortar_profile: MortarProfile::Flush,
            header_length: 0.0,
            pitting: 30.0,
            burn_variance: 20.0,
            grain_density: 50.0,
            knot_frequency: 20.0,
            plank_variation: 30.0,
            batten_width: 50.0,
            batten_depth: 19.0,
            lamination_width: 45.0,
            ring_spacing: 6.0,
            grain_angle: 0.0,
            bevel: 1.0,
            pitch: 76.0,
            rib_depth: 18.0,
            pan_width: 450.0,
            seam_height: 38.0,
            perforation_diameter: 5.0,
            perforation_pitch: 10.0,
            brushed: 40.0,
            primary_color: [214, 210, 200],
            joint_color: [170, 166, 158],
            brightness: 100.0,
            saturation: 100.0,
            hue_shift: 0.0,
            seed: 1,
            chevron_angle: 45.0,
        };
        match a {
            Archetype::Tile => base,
            Archetype::WoodPlank => GeneratorParams { unit_length: 1800.0, unit_height: 130.0, joint_width: 1.0, joint_depth: 2.0, random_stagger: true, finish: Finish::Natural, bevel: 1.5, primary_color: [176, 128, 82], joint_color: [70, 50, 34], ..base },
            Archetype::WoodSolid => GeneratorParams { unit_length: 1000.0, unit_height: 300.0, joint_width: 0.5, joint_depth: 0.5, finish: Finish::Natural, bevel: 0.0, primary_color: [196, 154, 104], joint_color: [150, 112, 74], ..base },
            Archetype::Masonry => GeneratorParams { unit_length: 230.0, unit_height: 76.0, joint_width: 10.0, joint_depth: 3.0, finish: Finish::Matte, bevel: 0.5, primary_color: [168, 84, 58], joint_color: [196, 190, 178], tone_variance: 25.0, ..base },
            Archetype::Metal => GeneratorParams { unit_length: 1200.0, unit_height: 600.0, joint_width: 12.0, joint_depth: 15.0, finish: Finish::PowderCoated, bevel: 0.0, primary_color: [92, 96, 98], joint_color: [40, 42, 44], tone_variance: 0.0, offset: 0.0, ..base },
        }
    }

    /// The primary colour after the HSV modifiers: the material's flat colour (Graphics shading colour when it
    /// follows the appearance).
    pub fn base_color(&self) -> Color {
        let c = hsv_mod(rgbf(self.primary_color), self);
        [to8(c[0]), to8(c[1]), to8(c[2])]
    }

    fn header(&self) -> f64 {
        if self.header_length > 0.0 { self.header_length } else { ((self.unit_length - self.joint_width) / 2.0).max(1.0) }
    }
}

/// What the generator makes.
#[derive(Debug, Clone)]
pub struct Generated {
    /// The tile's real size (the material's UV size).
    pub width_mm: f64,
    pub height_mm: f64,
    /// Pixel size of the maps.
    pub px: [u32; 2],
    /// RGB, row 0 at the top.
    pub albedo: Vec<u8>,
    /// Tangent-space normals, RGB (x right, y up).
    pub normal: Vec<u8>,
    /// 0 deepest, 65535 the face (`depth_mm` below it is 0).
    pub height: Vec<u16>,
    pub roughness: Vec<u8>,
    pub metalness: Vec<u8>,
    /// The relief the height map spans (POM depth).
    pub depth_mm: f64,
    /// The matching model pattern lines.
    pub hatch: Vec<HatchLine>,
    /// What had to be rounded to make the pattern repeat.
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------- layouts

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Face,
    Header,
    /// Drawn over whatever is under it (battens).
    Over,
}

#[derive(Debug, Clone)]
struct Unit {
    /// Convex, counter-clockwise, in domain mm (may run past the domain; it wraps).
    poly: Vec<[f64; 2]>,
    id: u32,
    /// Unit vector along the unit's length (wood grain, brushed grain).
    axis: [f64; 2],
    kind: Kind,
}

fn rect(x: f64, y: f64, w: f64, h: f64, id: u32, axis: [f64; 2], kind: Kind) -> Unit {
    Unit { poly: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], id, axis, kind }
}

struct Plan {
    w: f64,
    h: f64,
    units: Vec<Unit>,
    /// Joint centrelines for the hatch (when not simply the unit edges).
    lines: Option<Vec<([f64; 2], [f64; 2])>>,
    notes: Vec<String>,
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// How many courses before an offset course lines up again, and the offset used (rounded so that is at most 16).
fn repeat_of(cell: f64, offset: f64, notes: &mut Vec<String>) -> (usize, f64) {
    let c = (cell * 10.0).round() as i64;
    if c <= 0 {
        return (1, 0.0);
    }
    let mut o = ((offset * 10.0).round() as i64).rem_euclid(c);
    if o == 0 {
        return (1, 0.0);
    }
    let mut n = c / gcd(c, o);
    if n > 16 {
        let step = c as f64 / 16.0;
        let k = ((o as f64 / step).round() as i64).clamp(1, 15);
        o = (k as f64 * step).round() as i64;
        n = c / gcd(c, o);
        notes.push(format!("the course offset was rounded to {:.1} mm so the pattern repeats", o as f64 / 10.0));
    }
    (n.max(1) as usize, o as f64 / 10.0)
}

fn plan(p: &GeneratorParams) -> Result<Plan, String> {
    let j = p.joint_width.max(0.0);
    let l = p.unit_length.max(1.0);
    let hgt = p.unit_height.max(1.0);
    let (cw, ch) = (l + j, hgt + j);
    let mut notes = vec![];
    let along = [1.0, 0.0];
    let up = [0.0, 1.0];
    let offset = if p.offset_percent { cw * p.offset / 100.0 } else { p.offset };
    let mut units = vec![];
    let mut id = 0u32;
    let mut next = || { id += 1; id };
    use Layout::*;
    let allowed = Layout::for_archetype(p.archetype);
    if !allowed.contains(&p.layout) {
        return Err(format!("{:?} is not a layout for {:?}", p.layout, p.archetype));
    }
    let out = |w: f64, h: f64, units: Vec<Unit>, lines: Option<Vec<([f64; 2], [f64; 2])>>, notes: Vec<String>| -> Result<Plan, String> { Ok(Plan { w, h, units, lines, notes }) };
    match p.layout {
        StackBond => {
            units.push(rect(0.0, 0.0, cw, ch, next(), along, Kind::Face));
            out(cw, ch, units, None, notes)
        }
        RunningBond => {
            let (n, o) = repeat_of(cw, offset, &mut notes);
            for r in 0..n {
                let x0 = (o * r as f64).rem_euclid(cw);
                units.push(rect(x0, r as f64 * ch, cw, ch, next(), along, Kind::Face));
            }
            out(cw, ch * n as f64, units, None, notes)
        }
        Flemish => {
            let hc = p.header() + j;
            let period = cw + hc;
            // headers sit centred on the stretchers of the courses above and below
            let shift = (cw - hc) / 2.0;
            for r in 0..2 {
                let x0 = if r == 0 { 0.0 } else { shift };
                units.push(rect(x0, r as f64 * ch, cw, ch, next(), along, Kind::Face));
                units.push(rect(x0 + cw, r as f64 * ch, hc, ch, next(), along, Kind::Header));
            }
            // the second course's stretcher starts after its header
            units[2].poly.iter_mut().for_each(|q| q[0] += hc);
            units[3].poly.iter_mut().for_each(|q| q[0] -= cw);
            out(period, 2.0 * ch, units, None, notes)
        }
        English => {
            let hc = p.header() + j;
            // a header course over a stretcher course, headers centred on the perpends below (quarter bond)
            let period = cw.max(1.0);
            units.push(rect(0.0, 0.0, cw, ch, next(), along, Kind::Face));
            let n = (period / hc).round().max(1.0) as usize;
            let hc2 = period / n as f64;
            if (hc2 - hc).abs() > 0.05 {
                notes.push(format!("headers were sized {:.1} mm so a course of them matches a stretcher", hc2 - j));
            }
            for k in 0..n {
                units.push(rect(k as f64 * hc2 - hc2 / 2.0, ch, hc2, ch, next(), along, Kind::Header));
            }
            out(period, 2.0 * ch, units, None, notes)
        }
        Herringbone => {
            let k = (cw / ch).round().max(2.0);
            if ((cw / ch) - k).abs() > 0.01 {
                notes.push(format!("herringbone needs a length that is a whole number of widths: units are laid at {:.1} long", k * ch - j));
            }
            let (c, cl) = (ch, k * ch);
            for i in 0..(2 * k as usize) {
                let (tx, ty) = (i as f64 * c, i as f64 * c);
                units.push(rect(tx, ty, cl, c, next(), along, Kind::Face));
                units.push(rect(tx, ty + c, c, cl, next(), up, Kind::Face));
            }
            let d = 2.0 * cl;
            out(d, d, units, None, notes)
        }
        Chevron => {
            let a = p.chevron_angle.clamp(15.0, 75.0).to_radians();
            // the joints repeat along their own slope only when the rise over two columns is a whole number of
            // half bands: the length is rounded to make it so
            let r = cw * (2.0 * a).sin() / ch;
            let r2 = (r * 2.0).round().max(1.0) / 2.0;
            let cw = r2 * ch / (2.0 * a).sin();
            if (cw - (l + j)).abs() > 0.05 {
                notes.push(format!("chevron units are laid at {:.1} long so the joints repeat", cw - j));
            }
            let colw = cw * a.cos();
            let band = ch / a.cos();
            let rise = colw * a.tan();
            units.push(Unit { poly: vec![[0.0, 0.0], [colw, rise], [colw, rise + band], [0.0, band]], id: next(), axis: [a.cos(), a.sin()], kind: Kind::Face });
            units.push(Unit { poly: vec![[colw, rise], [2.0 * colw, 0.0], [2.0 * colw, band], [colw, rise + band]], id: next(), axis: [a.cos(), -a.sin()], kind: Kind::Face });
            out(2.0 * colw, band, units, None, notes)
        }
        Basketweave => {
            let n = (cw / ch).round().max(1.0);
            let b = n * ch;
            if ((cw / ch) - n).abs() > 0.01 {
                notes.push(format!("basketweave squares are {:.0} mm, so units are laid at {:.1} long", b, b - j));
            }
            for (bx, by, horiz) in [(0.0, 0.0, true), (b, 0.0, false), (0.0, b, false), (b, b, true)] {
                for i in 0..n as usize {
                    let o = i as f64 * ch;
                    if horiz {
                        units.push(rect(bx, by + o, b, ch, next(), along, Kind::Face));
                    } else {
                        units.push(rect(bx + o, by, ch, b, next(), up, Kind::Face));
                    }
                }
            }
            out(2.0 * b, 2.0 * b, units, None, notes)
        }
        Strip => {
            // two boards' length per row, six rows, each row cut at random lengths
            let rows = 6usize;
            let w = 2.0 * cw;
            let mut rng = Rng::new(p.seed ^ 0x5eed);
            let mut prev: Vec<f64> = vec![];
            for r in 0..rows {
                let mut best: Option<Vec<f64>> = None;
                for _try in 0..40 {
                    let start = rng.next() * w;
                    let split = start + cw * (0.6 + 0.8 * rng.next());
                    let cuts = vec![start.rem_euclid(w), split.rem_euclid(w)];
                    let near = |a: f64, b: f64| { let d = (a - b).rem_euclid(w); d.min(w - d) };
                    let gap = cuts.iter().flat_map(|c| prev.iter().map(move |q| near(*c, *q))).fold(f64::INFINITY, f64::min);
                    let own = near(cuts[0], cuts[1]);
                    let need = p.min_stagger.min(cw * 0.45);
                    if (gap >= need || prev.is_empty()) && own >= need {
                        best = Some(cuts);
                        break;
                    }
                    if best.is_none() { best = Some(cuts); }
                }
                let mut cuts = best.unwrap();
                cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let y = r as f64 * ch;
                let len1 = cuts[1] - cuts[0];
                units.push(rect(cuts[0], y, len1, ch, next(), along, Kind::Face));
                units.push(rect(cuts[1], y, w - len1, ch, next(), along, Kind::Face));
                prev = cuts;
            }
            out(w, rows as f64 * ch, units, None, notes)
        }
        BoardAndBatten => {
            // vertical boards `unit_height` wide, battens over their joints; one board length up the domain
            let bw = ch;
            let len = l.max(300.0);
            units.push(rect(0.0, 0.0, bw, len, next(), up, Kind::Face));
            let bt = p.batten_width.clamp(5.0, bw * 0.9);
            units.push(rect(-bt / 2.0, 0.0, bt, len, next(), up, Kind::Over));
            let lines = vec![([-bt / 2.0, 0.0], [-bt / 2.0, len]), ([bt / 2.0, 0.0], [bt / 2.0, len])];
            out(bw, len, units, Some(lines), notes)
        }
        Seamless => {
            units.push(rect(0.0, 0.0, l, hgt, next(), along, Kind::Face));
            out(l, hgt, units, Some(vec![]), notes)
        }
        Glulam => {
            let lam = p.lamination_width.max(5.0);
            let rows = ((hgt / lam).round() as usize).clamp(2, 40);
            for r in 0..rows {
                units.push(rect(0.0, r as f64 * lam, l, lam, next(), along, Kind::Face));
            }
            let lines = (0..rows).map(|r| ([0.0, r as f64 * lam], [l, r as f64 * lam])).collect();
            out(l, rows as f64 * lam, units, Some(lines), notes)
        }
        Corrugated | Trapezoidal => {
            let pc = p.pitch.max(5.0);
            let len = 600.0;
            units.push(rect(0.0, 0.0, pc, len, next(), up, Kind::Face));
            let lines = if p.layout == Corrugated {
                vec![([0.0, 0.0], [0.0, len])]
            } else {
                let top = pc * 0.2;
                vec![([pc / 2.0 - top / 2.0, 0.0], [pc / 2.0 - top / 2.0, len]), ([pc / 2.0 + top / 2.0, 0.0], [pc / 2.0 + top / 2.0, len])]
            };
            out(pc, len, units, Some(lines), notes)
        }
        StandingSeam => {
            let pw = p.pan_width.max(50.0);
            let len = 1200.0;
            units.push(rect(0.0, 0.0, pw, len, next(), up, Kind::Face));
            out(pw, len, units, Some(vec![([0.0, 0.0], [0.0, len])]), notes)
        }
        Cassette => {
            let (n, o) = repeat_of(cw, offset, &mut notes);
            for r in 0..n {
                units.push(rect((o * r as f64).rem_euclid(cw), r as f64 * ch, cw, ch, next(), along, Kind::Face));
            }
            out(cw, ch * n as f64, units, None, notes)
        }
        Perforated => {
            let pp = p.perforation_pitch.max(p.perforation_diameter + 1.0).max(2.0);
            // staggered holes: a rectangle holding two rows
            let (w, h) = (pp, pp * 3f64.sqrt());
            units.push(rect(0.0, 0.0, w, h, next(), along, Kind::Face));
            out(w, h, units, Some(vec![]), notes)
        }
    }
}

// ---------------------------------------------------------------- noise

struct Rng(u64);
impl Rng {
    fn new(seed: u32) -> Rng {
        Rng(u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn hash(a: u32, b: u32, c: u32) -> f64 {
    let mut h = a.wrapping_mul(0x8da6_b343) ^ b.wrapping_mul(0xd816_3841) ^ c.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    f64::from(h) / f64::from(u32::MAX)
}

/// Value noise with a period of (px, py) cells (so it tiles over a domain that holds whole cells).
fn vnoise(x: f64, y: f64, px: i64, py: i64, seed: u32) -> f64 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let at = |i: i64, j: i64| hash(i.rem_euclid(px.max(1)) as u32, j.rem_euclid(py.max(1)) as u32, seed);
    let (i, j) = (xi as i64, yi as i64);
    let a = at(i, j) + (at(i + 1, j) - at(i, j)) * s(fx);
    let b = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * s(fx);
    a + (b - a) * s(fy)
}

/// Tileable fractal noise over the domain: `cell` mm features, 0..1.
fn fbm(x: f64, y: f64, w: f64, h: f64, cell: f64, octaves: u32, seed: u32) -> f64 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut total = 0.0;
    let mut c = cell.max(0.5);
    for o in 0..octaves {
        let nx = (w / c).round().max(1.0);
        let ny = (h / c).round().max(1.0);
        sum += amp * vnoise(x / w * nx, y / h * ny, nx as i64, ny as i64, seed.wrapping_add(o * 977));
        total += amp;
        amp *= 0.5;
        c /= 2.0;
    }
    sum / total
}

/// Noise in a unit's own frame (needs no tiling: the unit is the same wherever it is drawn).
fn lnoise(x: f64, y: f64, seed: u32) -> f64 {
    vnoise(x, y, i64::MAX / 4, i64::MAX / 4, seed)
}

fn lfbm(x: f64, y: f64, octaves: u32, seed: u32) -> f64 {
    let (mut sum, mut amp, mut f, mut total) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * lnoise(x * f, y * f, seed.wrapping_add(o * 131));
        total += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / total
}

// ---------------------------------------------------------------- colour

fn rgbf(c: Color) -> [f64; 3] {
    [f64::from(c[0]) / 255.0, f64::from(c[1]) / 255.0, f64::from(c[2]) / 255.0]
}

fn to8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn to_hsv(c: [f64; 3]) -> [f64; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    let h = if d < 1e-9 {
        0.0
    } else if mx == c[0] {
        60.0 * ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    [h, if mx > 0.0 { d / mx } else { 0.0 }, mx]
}

fn from_hsv(h: [f64; 3]) -> [f64; 3] {
    let (hh, s, v) = (h[0].rem_euclid(360.0) / 60.0, h[1].clamp(0.0, 1.0), h[2].max(0.0));
    let c = v * s;
    let x = c * (1.0 - (hh % 2.0 - 1.0).abs());
    let (r, g, b) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

fn hsv_mod(c: [f64; 3], p: &GeneratorParams) -> [f64; 3] {
    let mut h = to_hsv(c);
    h[0] += p.hue_shift.clamp(-180.0, 180.0);
    h[1] *= p.saturation.clamp(0.0, 200.0) / 100.0;
    h[2] *= p.brightness.clamp(0.0, 200.0) / 100.0;
    from_hsv(h)
}

fn scale(c: [f64; 3], k: f64) -> [f64; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---------------------------------------------------------------- raster

/// The pixel size for a domain: the longer side `res` pixels, the other in proportion (or square).
pub fn pixel_size(w: f64, h: f64, res: u32, square: bool) -> [u32; 2] {
    let res = res.clamp(16, 8192);
    if square {
        return [res, res];
    }
    if w >= h { [res, ((f64::from(res) * h / w).round() as u32).max(4)] } else { [((f64::from(res) * w / h).round() as u32).max(4), res] }
}

/// Generate the maps at `res` pixels along the longer side (`square`: res x res, stretched).
pub fn generate(p: &GeneratorParams, res: u32, square: bool) -> Result<Generated, String> {
    let pl = plan(p)?;
    let (dw, dh) = (pl.w, pl.h);
    if !(dw.is_finite() && dh.is_finite() && dw > 0.0 && dh > 0.0) {
        return Err("the units need a size above zero".into());
    }
    if dw > 20000.0 || dh > 20000.0 {
        return Err("the pattern would repeat only every 20 m or more; use rounder sizes".into());
    }
    let [pw, ph] = pixel_size(dw, dh, res, square);
    let n = (pw * ph) as usize;
    let (sx, sy) = (dw / f64::from(pw), dh / f64::from(ph));
    // the unit that owns each pixel and how far in it is (mm from the cell edge, the joint centreline)
    let mut owner = vec![u32::MAX; n];
    let mut inset = vec![f64::NEG_INFINITY; n];
    let mut edge_up = vec![0i8; n];
    let mut over = vec![false; n];
    for (ui, u) in pl.units.iter().enumerate() {
        let mut poly = u.poly.clone();
        let area: f64 = (0..poly.len()).map(|i| { let (a, b) = (poly[i], poly[(i + 1) % poly.len()]); a[0] * b[1] - b[0] * a[1] }).sum();
        if area < 0.0 { poly.reverse(); }
        let edges: Vec<([f64; 2], [f64; 2])> = (0..poly.len()).map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let l = (ex * ex + ey * ey).sqrt().max(1e-9);
            (a, [ey / l, -ex / l]) // outward normal
        }).collect();
        let (x0, x1) = poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[0]), b.max(q[0])));
        let (y0, y1) = poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[1]), b.max(q[1])));
        let (i0, i1) = ((x0 / sx).floor() as i64 - 1, (x1 / sx).ceil() as i64 + 1);
        let (j0, j1) = ((y0 / sy).floor() as i64 - 1, (y1 / sy).ceil() as i64 + 1);
        for jy in j0..j1 {
            let y = (jy as f64 + 0.5) * sy;
            for ix in i0..i1 {
                let x = (ix as f64 + 0.5) * sx;
                let mut e = f64::INFINITY;
                let mut ny = 0.0;
                for (a, nrm) in &edges {
                    let d = -((x - a[0]) * nrm[0] + (y - a[1]) * nrm[1]);
                    if d < e { e = d; ny = nrm[1]; }
                }
                if e < -0.75 * sx.max(sy) { continue; }
                let col = ix.rem_euclid(i64::from(pw)) as usize;
                let row = (i64::from(ph) - 1 - jy.rem_euclid(i64::from(ph))) as usize;
                let k = row * pw as usize + col;
                let is_over = u.kind == Kind::Over && e >= 0.0;
                if (is_over && (!over[k] || e > inset[k])) || (!over[k] && !is_over && e > inset[k]) {
                    owner[k] = ui as u32;
                    inset[k] = e;
                    edge_up[k] = if ny > 0.5 { 1 } else if ny < -0.5 { -1 } else { 0 };
                    over[k] = is_over;
                }
            }
        }
    }

    // height (mm below the top of the relief), colour and roughness per pixel
    let g2 = p.joint_width.max(0.0) / 2.0;
    let face_rough = p.finish.roughness();
    let metal = p.archetype == Archetype::Metal && !matches!(p.finish, Finish::PowderCoated | Finish::Patina);
    let depth_mm = relief_mm(p);
    let prim = rgbf(p.primary_color);
    let joint = rgbf(p.joint_color);
    let tv = p.tone_variance.clamp(0.0, 100.0) / 100.0;
    let mut height = vec![0f64; n];
    let mut albedo = vec![[0f64; 3]; n];
    let mut rough = vec![0f64; n];
    for row in 0..ph as usize {
        let y = (f64::from(ph) - row as f64 - 0.5) * sy;
        for col in 0..pw as usize {
            let k = row * pw as usize + col;
            let x = (col as f64 + 0.5) * sx;
            let ui = owner[k];
            let e = inset[k];
            let u = pl.units.get(ui as usize);
            let uid = u.map_or(0, |u| u.id);
            let r1 = hash(p.seed, uid, 1);
            let r2 = hash(p.seed, uid, 2);
            let r3 = hash(p.seed, uid, 3);
            // local frame: s along the unit, t across, from its first corner
            let (s, t) = u.map_or((x, y), |u| {
                let a = u.poly[0];
                let (dx, dy) = (x - a[0], y - a[1]);
                // unwrap to the copy of the unit nearest this pixel
                let (dx, dy) = ((dx + dw / 2.0).rem_euclid(dw) - dw / 2.0, (dy + dh / 2.0).rem_euclid(dh) - dh / 2.0);
                let (ax, ay) = (u.axis[0], u.axis[1]);
                (dx * ax + dy * ay, -dx * ay + dy * ax)
            });
            let mut hmm;
            let mut c;
            let mut ro = face_rough;
            let in_joint = e < g2 && !over[k];
            match p.layout {
                Layout::Corrugated => {
                    let a = x.rem_euclid(p.pitch.max(5.0)) / p.pitch.max(5.0);
                    hmm = depth_mm * (0.5 - 0.5 * (a * std::f64::consts::TAU).cos());
                    c = prim;
                }
                Layout::Trapezoidal => {
                    let pc = p.pitch.max(5.0);
                    let a = (x.rem_euclid(pc) / pc - 0.5).abs();
                    let (top, side) = (0.1, 0.2);
                    hmm = depth_mm * (1.0 - smooth(top, top + side, a).min(1.0));
                    c = prim;
                }
                Layout::StandingSeam => {
                    let pw_ = p.pan_width.max(50.0);
                    let a = x.rem_euclid(pw_).min(pw_ - x.rem_euclid(pw_));
                    let seam = 5.0;
                    hmm = if a < seam { depth_mm } else if a < seam + 12.0 { depth_mm * (1.0 - smooth(seam, seam + 12.0, a)) * 0.15 } else { 0.0 };
                    c = prim;
                }
                Layout::Perforated => {
                    let pp = p.perforation_pitch.max(p.perforation_diameter + 1.0).max(2.0);
                    let rr = p.perforation_diameter.max(0.5) / 2.0;
                    let hh = pp * 3f64.sqrt();
                    let centres = [[0.0, 0.0], [pp, 0.0], [0.0, hh], [pp, hh], [pp / 2.0, hh / 2.0]];
                    let d = centres.iter().map(|q| ((x - q[0]).powi(2) + (y - q[1]).powi(2)).sqrt()).fold(f64::INFINITY, f64::min);
                    hmm = if d < rr { 0.0 } else { depth_mm };
                    c = if d < rr { scale(joint, 0.4) } else { prim };
                    if d < rr { ro = 0.95; }
                }
                Layout::Seamless => {
                    hmm = depth_mm;
                    c = prim;
                }
                _ => {
                    if in_joint {
                        let dc = e.max(0.0); // from the joint centreline
                        let jd = p.joint_depth.max(0.0);
                        let face = depth_mm;
                        let d = match (p.archetype, p.mortar_profile) {
                            (Archetype::Masonry, MortarProfile::Flush) => 0.6,
                            (Archetype::Masonry, MortarProfile::Concave) => jd * (1.0 - (dc / g2.max(1e-6)).powi(2)).max(0.0).sqrt(),
                            (Archetype::Masonry, MortarProfile::Weathered) => {
                                // recessed under the unit above, flush at the one below
                                let pos = match edge_up[k] { -1 => g2 - dc, 1 => g2 + dc, _ => g2 } / (2.0 * g2).max(1e-6);
                                jd * (1.0 - pos)
                            }
                            _ => jd,
                        };
                        hmm = (face - d).max(0.0);
                        let jn = fbm(x, y, dw, dh, 2.0, 3, p.seed.wrapping_add(17));
                        c = scale(joint, 0.92 + 0.16 * jn);
                        ro = 0.9;
                    } else {
                        // the face, with a bevel at its edge
                        let into = e - g2;
                        let bev = p.bevel.max(0.0);
                        hmm = depth_mm - if bev > 0.0 && into < bev { (bev - into) * 0.6 } else { 0.0 };
                        if over[k] {
                            hmm = depth_mm;
                        } else if p.layout == Layout::BoardAndBatten {
                            hmm = depth_mm - p.batten_depth.max(1.0);
                        }
                        c = prim;
                    }
                }
            }
            // surface character by archetype
            if !in_joint || matches!(p.layout, Layout::Seamless | Layout::Perforated | Layout::Corrugated | Layout::Trapezoidal | Layout::StandingSeam) {
                match p.archetype {
                    Archetype::Tile => {
                        let tone = 1.0 + (r1 - 0.5) * 0.5 * tv;
                        let mut hsv = to_hsv(c);
                        hsv[0] += (r2 - 0.5) * 12.0 * tv;
                        hsv[2] *= tone;
                        c = from_hsv(hsv);
                        if p.finish == Finish::TexturedStone {
                            let nz = fbm(x, y, dw, dh, 6.0, 4, p.seed.wrapping_add(uid));
                            c = scale(c, 0.85 + 0.3 * nz);
                            hmm -= (1.0 - nz) * 0.4;
                        } else {
                            let nz = fbm(x, y, dw, dh, 20.0, 3, p.seed.wrapping_add(5));
                            c = scale(c, 0.97 + 0.06 * nz);
                        }
                    }
                    Archetype::Masonry => {
                        let pit = p.pitting.clamp(0.0, 100.0) / 100.0;
                        let burn = p.burn_variance.clamp(0.0, 100.0) / 100.0;
                        let mut k = 1.0 + (r1 - 0.5) * 0.6 * tv;
                        if r3 < 0.25 * burn { k *= 0.65 + 0.2 * r2; }
                        if u.map_or(false, |u| u.kind == Kind::Header) { k *= 1.0 - 0.15 * burn; }
                        let speck = fbm(x, y, dw, dh, 1.5, 3, p.seed.wrapping_add(9));
                        let blot = fbm(x, y, dw, dh, 18.0, 3, p.seed.wrapping_add(10));
                        c = scale(c, k * (0.85 + 0.25 * blot) * (1.0 - 0.25 * pit * smooth(0.6, 0.9, speck)));
                        hmm -= pit * 0.8 * smooth(0.62, 0.85, speck);
                        // kiln flash darkening toward the edges
                        let ed = (e - g2).max(0.0);
                        c = scale(c, 1.0 - 0.12 * burn * (1.0 - smooth(0.0, 12.0, ed)));
                    }
                    Archetype::WoodPlank | Archetype::WoodSolid => {
                        let var = p.plank_variation.clamp(0.0, 100.0) / 100.0;
                        let dens = 0.4 + 1.6 * p.grain_density.clamp(0.0, 100.0) / 100.0;
                        let (gs, gt) = if p.archetype == Archetype::WoodSolid {
                            let a = p.grain_angle.to_radians();
                            (x * a.cos() + y * a.sin(), -x * a.sin() + y * a.cos())
                        } else { (s, t) };
                        let ring = if p.archetype == Archetype::WoodSolid { p.ring_spacing.max(1.0) } else { 6.0 / dens };
                        // wobble of the grain along the board, then rings across it
                        let w1 = if p.archetype == Archetype::WoodSolid { fbm(x, y, dw, dh, 160.0, 3, p.seed.wrapping_add(31)) } else { lfbm(gs / 220.0 + r1 * 40.0, gt / 60.0, 3, p.seed.wrapping_add(31)) };
                        let mut phase = (gt + w1 * 24.0 + r2 * 100.0) / ring;
                        // knots: a few per board, rings bending round them
                        let kf = p.knot_frequency.clamp(0.0, 100.0) / 100.0;
                        let mut knot = 0.0;
                        if p.archetype == Archetype::WoodPlank && kf > 0.0 {
                            for kk in 0..3u32 {
                                if hash(p.seed, uid, 40 + kk) > kf * 0.8 { continue; }
                                let ks = hash(p.seed, uid, 50 + kk) * p.unit_length.max(50.0);
                                let kt = (0.2 + 0.6 * hash(p.seed, uid, 60 + kk)) * p.unit_height.max(10.0);
                                let rad = 6.0 + 10.0 * hash(p.seed, uid, 70 + kk);
                                let d = (((s - ks) / 2.5).powi(2) + (t - kt).powi(2)).sqrt();
                                phase += 2.5 * (-d / (rad * 2.0)).exp();
                                knot = f64::max(knot, 1.0 - smooth(rad * 0.5, rad, d));
                            }
                        }
                        let fr = phase - phase.floor();
                        let late = smooth(0.55, 0.85, fr) * (1.0 - smooth(0.88, 1.0, fr));
                        let pores = lnoise(gs / 0.6, gt / 0.25, p.seed.wrapping_add(77));
                        let tone = 1.0 + (r1 - 0.5) * 0.6 * var + (r3 - 0.5) * 0.3 * tv;
                        let mut hsv = to_hsv(c);
                        hsv[0] += (r2 - 0.5) * 10.0 * var;
                        hsv[2] *= tone;
                        c = from_hsv(hsv);
                        c = scale(c, (1.0 - 0.28 * late) * (0.96 + 0.06 * pores));
                        c = mix(c, scale(c, 0.45), knot * 0.85);
                        hmm -= 0.15 * late + 0.05 * (1.0 - pores);
                    }
                    Archetype::Metal => {
                        let br = p.brushed.clamp(0.0, 100.0) / 100.0;
                        let streak = fbm(x, y * 0.02, dw, dh * 0.02, 0.4, 3, p.seed.wrapping_add(91));
                        if matches!(p.finish, Finish::Brushed | Finish::MillFinish) || br > 0.0 {
                            c = scale(c, 1.0 - 0.12 * br + 0.24 * br * streak);
                            ro += (streak - 0.5) * 0.15 * br;
                        }
                        if p.finish == Finish::Patina {
                            let blot = fbm(x, y, dw, dh, 60.0, 4, p.seed.wrapping_add(92));
                            c = mix(c, [0.36, 0.56, 0.48], smooth(0.45, 0.75, blot) * 0.6);
                        }
                        c = scale(c, 1.0 + (r1 - 0.5) * 0.2 * tv);
                    }
                }
            }
            // a little darkening down in the relief
            let depth01 = (hmm / depth_mm).clamp(0.0, 1.0);
            if !matches!(p.layout, Layout::Corrugated | Layout::Trapezoidal | Layout::StandingSeam) {
                c = scale(c, 0.8 + 0.2 * depth01);
            }
            height[k] = hmm.clamp(0.0, depth_mm);
            albedo[k] = hsv_mod(c, p);
            rough[k] = ro.clamp(0.02, 1.0);
        }
    }

    // normals from the height field (mm), with wrap
    let at = |c: i64, r: i64| height[(r.rem_euclid(i64::from(ph)) as usize) * pw as usize + c.rem_euclid(i64::from(pw)) as usize];
    let mut normal = vec![0u8; n * 3];
    for r in 0..i64::from(ph) {
        for c in 0..i64::from(pw) {
            let dx = (at(c + 1, r) - at(c - 1, r)) / (2.0 * sx);
            // rows run down: y up is row - 1
            let dy = (at(c, r - 1) - at(c, r + 1)) / (2.0 * sy);
            let (nx, ny, nz) = (-dx, -dy, 1.0);
            let l = (nx * nx + ny * ny + nz * nz).sqrt();
            let k = (r as usize * pw as usize + c as usize) * 3;
            normal[k] = to8(nx / l * 0.5 + 0.5);
            normal[k + 1] = to8(ny / l * 0.5 + 0.5);
            normal[k + 2] = to8(nz / l * 0.5 + 0.5);
        }
    }
    let hatch = hatch_for(&pl);
    let mut out_albedo = Vec::with_capacity(n * 3);
    for c in &albedo {
        out_albedo.extend_from_slice(&[to8(c[0]), to8(c[1]), to8(c[2])]);
    }
    Ok(Generated {
        width_mm: dw,
        height_mm: dh,
        px: [pw, ph],
        albedo: out_albedo,
        normal,
        height: height.iter().map(|h| ((h / depth_mm).clamp(0.0, 1.0) * 65535.0).round() as u16).collect(),
        roughness: rough.iter().map(|r| to8(*r)).collect(),
        metalness: vec![if metal { 255 } else { 0 }; n],
        depth_mm,
        hatch,
        notes: pl.notes,
    })
}

/// The relief the height map spans, mm (the material's parallax depth).
pub fn relief_mm(p: &GeneratorParams) -> f64 {
    match p.layout {
        Layout::Corrugated | Layout::Trapezoidal => p.rib_depth.max(1.0),
        Layout::StandingSeam => p.seam_height.max(1.0),
        Layout::BoardAndBatten => p.batten_depth.max(1.0) + p.joint_depth.max(0.5),
        Layout::Perforated => 2.0,
        Layout::Seamless => 0.6,
        _ => (p.joint_depth.max(0.3) + p.bevel.max(0.0) * 0.5).max(0.5),
    }
}

/// The tile size (mm) the parameters repeat over, without making the maps.
pub fn domain(p: &GeneratorParams) -> Result<(f64, f64), String> {
    let pl = plan(p)?;
    Ok((pl.w, pl.h))
}

/// Only the matching hatch lines.
pub fn hatch(p: &GeneratorParams) -> Result<Vec<HatchLine>, String> {
    Ok(hatch_for(&plan(p)?))
}

// ---------------------------------------------------------------- the hatch

fn hatch_for(pl: &Plan) -> Vec<HatchLine> {
    let (w, h) = (pl.w, pl.h);
    let segs: Vec<([f64; 2], [f64; 2])> = match &pl.lines {
        Some(l) => l.clone(),
        None => pl.units.iter().flat_map(|u| (0..u.poly.len()).map(move |i| (u.poly[i], u.poly[(i + 1) % u.poly.len()]))).collect(),
    };
    let r2 = |v: f64| (v * 100.0).round() / 100.0;
    // axis-aligned runs merged on their line (mod the domain), the rest kept as they are (deduplicated)
    let mut horiz: std::collections::BTreeMap<i64, Vec<(f64, f64)>> = Default::default();
    let mut vert: std::collections::BTreeMap<i64, Vec<(f64, f64)>> = Default::default();
    let mut other: Vec<([f64; 2], [f64; 2])> = vec![];
    for (a, b) in segs {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.01 { continue; }
        if dy.abs() < 1e-6 {
            let y = r2(a[1].rem_euclid(h)) % r2(h);
            let x0 = a[0].min(b[0]);
            horiz.entry((y * 100.0).round() as i64).or_default().push((x0.rem_euclid(w), len));
        } else if dx.abs() < 1e-6 {
            let x = r2(a[0].rem_euclid(w)) % r2(w);
            let y0 = a[1].min(b[1]);
            vert.entry((x * 100.0).round() as i64).or_default().push((y0.rem_euclid(h), len));
        } else {
            // one direction for both ways round
            let (a, b) = if dx < 0.0 { (b, a) } else { (a, b) };
            let key = |q: [f64; 2]| [r2(q[0].rem_euclid(w)) % r2(w), r2(q[1].rem_euclid(h)) % r2(h)];
            let (ka, kb) = (key(a), [key(a)[0] + (b[0] - a[0]), key(a)[1] + (b[1] - a[1])]);
            if !other.iter().any(|(p, q)| (p[0] - ka[0]).abs() < 0.02 && (p[1] - ka[1]).abs() < 0.02 && (q[0] - kb[0]).abs() < 0.02 && (q[1] - kb[1]).abs() < 0.02) {
                other.push((ka, kb));
            }
        }
    }
    let mut out = vec![];
    let merge = |runs: &mut Vec<(f64, f64)>, period: f64| -> Vec<(f64, f64)> {
        // intervals on a circle of length `period`
        let mut iv: Vec<(f64, f64)> = runs.iter().map(|(s, l)| (*s, s + l.min(period))).collect();
        iv.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut m: Vec<(f64, f64)> = vec![];
        for (s, e) in iv {
            if let Some(last) = m.last_mut() {
                if s <= last.1 + 0.02 { last.1 = last.1.max(e); continue; }
            }
            m.push((s, e));
        }
        // the last run may wrap onto the first
        if m.len() > 1 {
            let (fs, fe) = m[0];
            let last = *m.last().unwrap();
            if last.1 >= period + fs - 0.02 {
                m.pop();
                m[0] = (last.0, (fe + period).max(last.1));
            }
        }
        m.into_iter().map(|(s, e)| (s, (e - s).min(period))).collect()
    };
    for (y, mut runs) in horiz {
        for (s, len) in merge(&mut runs, w) {
            out.push(family([s, y as f64 / 100.0], 1, 0, len, w, h));
        }
    }
    for (x, mut runs) in vert {
        for (s, len) in merge(&mut runs, h) {
            out.push(family([x as f64 / 100.0, s], 0, 1, len, w, h));
        }
    }
    for (a, b) in other {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        // the shortest lattice step along the segment
        let mut best = (1i64, 0i64, f64::INFINITY);
        for p in 0..=64i64 {
            for q in -64..=64i64 {
                if p == 0 && q == 0 { continue; }
                if gcd(p, q) != 1 { continue; }
                let (vx, vy) = (p as f64 * w, q as f64 * h);
                if vx * dx + vy * dy <= 0.0 { continue; }
                let err = (vx * dy - vy * dx).abs() / len;
                let vl = (vx * vx + vy * vy).sqrt();
                // a small misfit is fine over a short step; prefer the shortest that fits
                if err / vl < 1e-5 && vl < best.2 { best = (p, q, vl); }
            }
        }
        let (p, q) = if best.2.is_finite() {
            (best.0, best.1)
        } else {
            // nothing fits exactly: the closest step under 20 m
            let mut near = (1i64, 0i64, f64::INFINITY);
            for p in 0..=64i64 {
                for q in -64..=64i64 {
                    if (p == 0 && q == 0) || gcd(p, q) != 1 { continue; }
                    let (vx, vy) = (p as f64 * w, q as f64 * h);
                    if vx * dx + vy * dy <= 0.0 || (vx * vx + vy * vy).sqrt() > 20000.0 { continue; }
                    let err = (vx * dy - vy * dx).abs() / len;
                    if err < near.2 { near = (p, q, err); }
                }
            }
            (near.0, near.1)
        };
        out.push(family(a, p, q, len, w, h));
    }
    out
}

/// A pattern line family for a segment from `a` of length `len` along the lattice step (p w, q h).
fn family(a: [f64; 2], p: i64, q: i64, len: f64, w: f64, h: f64) -> HatchLine {
    let (vx, vy) = (p as f64 * w, q as f64 * h);
    let period = (vx * vx + vy * vy).sqrt();
    let angle = vy.atan2(vx).to_degrees();
    let spacing = w * h / period;
    // b p - a q = 1 (extended Euclid) gives the lattice step to the next line
    let (g, x, y) = ext_gcd(p, q);
    let (bb, aa) = (x * g.signum(), -y * g.signum());
    let (tx, ty) = (aa as f64 * w, bb as f64 * h);
    let (dx, dy) = (vx / period, vy / period);
    let shift = (tx * dx + ty * dy).rem_euclid(period);
    let dashes = if len >= period - 0.02 { vec![] } else { vec![len, period - len] };
    HatchLine { angle_deg: angle, origin: a, shift, spacing, dashes }
}

/// (g, x, y) with p x + q y = g.
fn ext_gcd(p: i64, q: i64) -> (i64, i64, i64) {
    if q == 0 { (p, 1, 0) } else { let (g, x, y) = ext_gcd(q, p.rem_euclid(q)); (g, y, x - (p.div_euclid(q)) * y) }
}

/// Turn pattern lines about the origin.
pub fn rotate_hatch(lines: &[HatchLine], deg: f64) -> Vec<HatchLine> {
    let (s, c) = deg.to_radians().sin_cos();
    lines.iter().map(|l| HatchLine { angle_deg: l.angle_deg + deg, origin: [l.origin[0] * c - l.origin[1] * s, l.origin[0] * s + l.origin[1] * c], ..l.clone() }).collect()
}

// ---------------------------------------------------------------- files

fn png(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(data).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

impl Generated {
    pub fn albedo_png(&self) -> Result<Vec<u8>, String> {
        png(self.px[0], self.px[1], png::ColorType::Rgb, png::BitDepth::Eight, &self.albedo)
    }
    pub fn normal_png(&self) -> Result<Vec<u8>, String> {
        png(self.px[0], self.px[1], png::ColorType::Rgb, png::BitDepth::Eight, &self.normal)
    }
    /// 16-bit greyscale.
    pub fn height_png(&self) -> Result<Vec<u8>, String> {
        let bytes: Vec<u8> = self.height.iter().flat_map(|v| v.to_be_bytes()).collect();
        png(self.px[0], self.px[1], png::ColorType::Grayscale, png::BitDepth::Sixteen, &bytes)
    }
    pub fn roughness_png(&self) -> Result<Vec<u8>, String> {
        png(self.px[0], self.px[1], png::ColorType::Grayscale, png::BitDepth::Eight, &self.roughness)
    }
    pub fn metalness_png(&self) -> Result<Vec<u8>, String> {
        png(self.px[0], self.px[1], png::ColorType::Grayscale, png::BitDepth::Eight, &self.metalness)
    }
    /// The viewport's second texture: normal x and y and height (8 bit) in one RGB image (no alpha, so a browser
    /// canvas keeps the values exactly).
    pub fn surface_png(&self) -> Result<Vec<u8>, String> {
        let n = (self.px[0] * self.px[1]) as usize;
        let mut d = Vec::with_capacity(n * 3);
        for i in 0..n {
            d.extend_from_slice(&[self.normal[i * 3], self.normal[i * 3 + 1], (self.height[i] >> 8) as u8]);
        }
        png(self.px[0], self.px[1], png::ColorType::Rgb, png::BitDepth::Eight, &d)
    }
}

#[cfg(test)]
#[path = "matgen_tests.rs"]
mod tests;
