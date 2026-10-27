//! Spot elevations, spot coordinates and spot slopes (Annotate > Spot Elevation, Spot
//! Coordinate, Spot Slope).
//!
//! A spot is one point on a surface, with a leader to its text. It stores the point in the plan,
//! the element it sits on (`host`) and which face of it (`surface`: top, core or bottom), never the
//! value: the height, coordinates or slope are worked out from the model every time it is drawn, so
//! moving a floor or changing a roof updates the text. A spot moves with its host; when the host is
//! deleted the spot goes with it (undo brings both back).
//!
//! The type decides where heights and coordinates are measured from (project base point, survey
//! point, a level, the shared site origin), whether coordinates are in project north or true north,
//! how a slope is written (angle, percentage, ratio, rise over run) and how the text looks.

use serde::{Deserialize, Serialize};

use crate::curve::point_in_polygon;
use crate::dimension::{format_length, num, wall_arc, LengthFormat, LengthUnit};
use crate::doc::Document;
use crate::edit::Xform;
use crate::element::*;
use crate::geom::Point;
use crate::view::ViewKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotKind {
    #[default]
    Elevation,
    Coordinate,
    Slope,
}

/// Which face of a compound element a spot measures: the top surface, the top of the structural
/// core, or the underside (soffit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotSurface {
    #[default]
    Top,
    Core,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevationOrigin {
    #[default]
    ProjectBase,
    Survey,
    /// The level of the view the spot is in.
    Relative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordOrigin {
    #[default]
    ProjectBase,
    Survey,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NorthRef {
    #[default]
    Project,
    True,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeFormat {
    /// Decimal degrees, 18.43 degrees.
    Angle,
    /// Gradient as a percentage, 33.3%.
    #[default]
    Percent,
    /// One in n, 1:3.
    Ratio,
    /// Rise per 12 of run, 4"/12".
    RiseRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeDir {
    /// The arrow points the way water would run.
    #[default]
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotSymbol {
    /// A circle with a cross.
    #[default]
    Target,
    Cross,
    Triangle,
    None,
}

fn metres3() -> LengthFormat {
    LengthFormat { unit: LengthUnit::Metres, decimals: 3, suppress_trailing_zeros: false, symbol: false, fraction: 16 }
}
fn d_text() -> f64 {
    2.5
}
fn d_off() -> f64 {
    0.8
}
fn d_symbol() -> f64 {
    3.0
}
fn s_top() -> String {
    "TOP".into()
}
fn s_bottom() -> String {
    "BOT".into()
}
fn s_core() -> String {
    "CORE".into()
}
fn s_elev() -> String {
    "Elev".into()
}
fn yes() -> bool {
    true
}
fn dec1() -> u8 {
    1
}

/// How spots of one kind look and where they measure from (a spot dimension type).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotType {
    pub name: String,
    pub kind: SpotKind,
    #[serde(default = "d_text")]
    pub text_size_mm: f64,
    /// Distance from the shoulder line to the text.
    #[serde(default = "d_off")]
    pub text_offset_mm: f64,
    #[serde(default)]
    pub symbol: SpotSymbol,
    #[serde(default = "d_symbol")]
    pub symbol_size_mm: f64,
    /// How heights and coordinates are written.
    #[serde(default = "metres3")]
    pub format: LengthFormat,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    // elevation
    #[serde(default)]
    pub elevation_origin: ElevationOrigin,
    /// Write TOP, CORE or BOT before the height.
    #[serde(default)]
    pub indicator: bool,
    #[serde(default = "s_top")]
    pub top_text: String,
    #[serde(default = "s_core")]
    pub core_text: String,
    #[serde(default = "s_bottom")]
    pub bottom_text: String,
    // coordinate
    #[serde(default)]
    pub coord_origin: CoordOrigin,
    #[serde(default)]
    pub north_ref: NorthRef,
    #[serde(default = "yes")]
    pub show_ns: bool,
    #[serde(default = "yes")]
    pub show_ew: bool,
    #[serde(default)]
    pub show_elevation: bool,
    /// One line each rather than all on one.
    #[serde(default = "yes")]
    pub stacked: bool,
    #[serde(default = "s_elev")]
    pub elevation_text: String,
    // slope
    #[serde(default)]
    pub slope_format: SlopeFormat,
    #[serde(default = "dec1")]
    pub slope_decimals: u8,
    #[serde(default)]
    pub slope_dir: SlopeDir,
    /// Draw the direction arrow at the point.
    #[serde(default = "yes")]
    pub slope_arrow: bool,
}

impl SpotType {
    pub fn new(name: &str, kind: SpotKind) -> SpotType {
        SpotType {
            name: name.into(),
            kind,
            text_size_mm: d_text(),
            text_offset_mm: d_off(),
            symbol: match kind {
                SpotKind::Elevation => SpotSymbol::Target,
                SpotKind::Coordinate => SpotSymbol::Cross,
                SpotKind::Slope => SpotSymbol::None,
            },
            symbol_size_mm: d_symbol(),
            format: metres3(),
            prefix: String::new(),
            suffix: String::new(),
            elevation_origin: ElevationOrigin::ProjectBase,
            indicator: false,
            top_text: s_top(),
            core_text: s_core(),
            bottom_text: s_bottom(),
            coord_origin: CoordOrigin::ProjectBase,
            north_ref: NorthRef::Project,
            show_ns: true,
            show_ew: true,
            show_elevation: false,
            stacked: true,
            elevation_text: s_elev(),
            slope_format: SlopeFormat::Percent,
            slope_decimals: 1,
            slope_dir: SlopeDir::Down,
            slope_arrow: true,
        }
    }

    /// The types a project starts with for one kind of spot.
    pub fn defaults(kind: SpotKind) -> Vec<SpotType> {
        match kind {
            SpotKind::Elevation => {
                let mut rl = SpotType::new("Spot Elevation - Project Base Point", kind);
                rl.prefix = "RL ".into();
                let mut survey = SpotType::new("Spot Elevation - Survey Point", kind);
                survey.elevation_origin = ElevationOrigin::Survey;
                survey.prefix = "RL ".into();
                let mut rel = SpotType::new("Spot Elevation - Relative to Level", kind);
                rel.elevation_origin = ElevationOrigin::Relative;
                rel.indicator = true;
                vec![rl, survey, rel]
            }
            SpotKind::Coordinate => {
                let a = SpotType::new("Spot Coordinate - Project Base Point", kind);
                let mut b = SpotType::new("Spot Coordinate - Survey Point, True North", kind);
                b.coord_origin = CoordOrigin::Survey;
                b.north_ref = NorthRef::True;
                let mut c = SpotType::new("Spot Coordinate - Shared, with Elevation", kind);
                c.coord_origin = CoordOrigin::Shared;
                c.north_ref = NorthRef::True;
                c.show_elevation = true;
                vec![a, b, c]
            }
            SpotKind::Slope => [("Percentage", SlopeFormat::Percent), ("Angle", SlopeFormat::Angle), ("Ratio", SlopeFormat::Ratio), ("Rise over Run", SlopeFormat::RiseRun)]
                .into_iter()
                .map(|(n, f)| {
                    let mut t = SpotType::new(&format!("Spot Slope - {n}"), kind);
                    t.slope_format = f;
                    if f == SlopeFormat::Angle {
                        t.slope_decimals = 2;
                    }
                    t
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub view: Id,
    #[serde(default)]
    pub kind: SpotKind,
    pub spot_type: Id,
    /// The point measured, in the plan (mm).
    pub target: Point,
    /// The element the point is on. A coordinate spot can stand free.
    #[serde(default)]
    pub host: Option<Id>,
    #[serde(default)]
    pub surface: SpotSurface,
    #[serde(default = "yes")]
    pub leader: bool,
    /// The elbow of the leader, as an offset from the target.
    #[serde(default)]
    pub bend: Option<[f64; 2]>,
    /// Where the leader meets the shoulder line the text stands on, as an offset from the target.
    #[serde(default)]
    pub shoulder: [f64; 2],
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub above: String,
    #[serde(default)]
    pub below: String,
    #[serde(default)]
    pub pinned: bool,
}

/// A face of an element at a point in the plan.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceHit {
    pub host: Id,
    pub surface: SpotSurface,
    pub label: String,
    /// Height of the face at the point, from the internal origin (mm).
    pub z: f64,
    /// Unit normal of the face, pointing away from the element.
    pub normal: [f64; 3],
    /// A ridge, hip, valley or corner: the slope here is not defined.
    pub singular: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpotDraw {
    pub id: Id,
    pub kind: SpotKind,
    pub type_id: Id,
    pub invalid: Option<String>,
    pub target: [f64; 2],
    pub bend: Option<[f64; 2]>,
    pub shoulder: [f64; 2],
    pub leader: bool,
    /// Which way the text runs from the shoulder: +1 to the right, -1 to the left.
    pub side: f64,
    pub symbol: SpotSymbol,
    /// The text, top line first.
    pub lines: Vec<String>,
    pub above: String,
    pub below: String,
    /// Slope spots: the direction of the arrow in the plan (a unit vector).
    pub arrow: Option<[f64; 2]>,
    pub pinned: bool,
    pub host: Option<Id>,
    /// What the spot reports, for Properties: a height in mm, a gradient, or none.
    pub value: Option<f64>,
}

struct Value {
    lines: Vec<String>,
    value: Option<f64>,
    arrow: Option<[f64; 2]>,
}

fn pt(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

fn plane_z(poly: &[[f64; 3]], p: Point) -> Option<([f64; 3], f64)> {
    // Newell's normal of the polygon
    let n = poly.len();
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        nx += (a[1] - b[1]) * (a[2] + b[2]);
        ny += (a[2] - b[2]) * (a[0] + b[0]);
        nz += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let l = (nx * nx + ny * ny + nz * nz).sqrt();
    if l < 1e-9 || (nz / l).abs() < 1e-6 {
        return None;
    }
    let (mut nx, mut ny, mut nz) = (nx / l, ny / l, nz / l);
    if nz < 0.0 {
        nx = -nx;
        ny = -ny;
        nz = -nz;
    }
    let a = poly[0];
    Some(([nx, ny, nz], a[2] - (nx * (p.x - a[0]) + ny * (p.y - a[1])) / nz))
}

fn near_polygon(p: Point, poly: &[Point], tol: f64) -> bool {
    if point_in_polygon(p, poly) {
        return true;
    }
    let n = poly.len();
    (0..n).any(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (vx, vy) = (b.x - a.x, b.y - a.y);
        let l2 = vx * vx + vy * vy;
        let t = if l2 < 1e-12 { 0.0 } else { (((p.x - a.x) * vx + (p.y - a.y) * vy) / l2).clamp(0.0, 1.0) };
        p.dist(&Point::new(a.x + vx * t, a.y + vy * t)) <= tol
    })
}

impl Document {
    pub fn spots(&self) -> Vec<(Id, &Spot)> {
        self.iter().filter_map(|(id, e)| if let Element::Spot(s) = e { Some((id, s)) } else { None }).collect()
    }

    pub fn spot(&self, id: Id) -> Option<&Spot> {
        match self.get(id) {
            Some(Element::Spot(s)) => Some(s),
            _ => None,
        }
    }

    pub fn spot_type(&self, id: Id) -> Option<&SpotType> {
        match self.get(id) {
            Some(Element::SpotType(t)) => Some(t),
            _ => None,
        }
    }

    pub fn spot_types(&self, kind: SpotKind) -> Vec<(Id, &SpotType)> {
        self.iter().filter_map(|(id, e)| if let Element::SpotType(t) = e { (t.kind == kind).then_some((id, t)) } else { None }).collect()
    }

    pub fn spots_in(&self, view: Id) -> Vec<(Id, &Spot)> {
        self.spots().into_iter().filter(|(_, s)| s.view == view).collect()
    }

    /// The faces of floors, roofs and walls that lie under a point in a plan view, highest first.
    /// Only elements on the view's level are looked at.
    pub fn surfaces_at(&self, view: Id, p: Point) -> Vec<SurfaceHit> {
        if self.is_elevation_like(view) {
            return self.elevation_spot_hits(view, p, 150.0);
        }
        let Some(v) = self.view(view) else { return vec![] };
        let on_level = |l: Id| v.level.map_or(true, |vl| vl == l);
        let mut out: Vec<SurfaceHit> = vec![];
        for (id, e) in self.iter() {
            match e {
                Element::Floor(f) if on_level(f.level) => {
                    let regions = f.regions(0.5);
                    let inside = regions.iter().any(|r| near_polygon(p, &r.outer, 0.5) && !r.holes.iter().any(|h| point_in_polygon(p, h)));
                    if !inside {
                        continue;
                    }
                    let (Some(surf), Some(ft)) = (self.floor_surface(f), self.floor_type(f.floor_type)) else { continue };
                    let top = surf.z(p);
                    let nl = (1.0 + surf.rise * surf.rise).sqrt();
                    let normal = [-surf.rise * surf.dir.x / nl, -surf.rise * surf.dir.y / nl, 1.0 / nl];
                    let above_core: f64 = ft.layers.iter().take_while(|l| !l.core).map(|l| l.thickness_mm).sum();
                    let has_core = ft.layers.iter().any(|l| l.core);
                    let mut push = |surface, z, label: &str, n: [f64; 3]| out.push(SurfaceHit { host: id, surface, label: format!("{label} ({})", ft.name), z, normal: n, singular: false });
                    push(SpotSurface::Top, top, "Floor top", normal);
                    if has_core && above_core > 0.0 {
                        push(SpotSurface::Core, top - above_core, "Floor core face", normal);
                    }
                    push(SpotSurface::Bottom, top - ft.thickness_mm(), "Floor bottom", [-normal[0], -normal[1], -normal[2]]);
                }
                Element::Roof(r) if on_level(r.level) => {
                    let (Some(g), Some(rt)) = (self.roof_geometry(r), self.roof_type(r.roof_type)) else { continue };
                    // every top plane the point lies on (two or more meet at a ridge, hip or valley)
                    let mut planes: Vec<([f64; 3], f64)> = vec![];
                    for f in g.faces.iter().filter(|f| !f.vertical) {
                        let poly: Vec<Point> = f.poly.iter().map(|q| Point::new(q[0], q[1])).collect();
                        if poly.len() >= 3 && near_polygon(p, &poly, 1.0) {
                            if let Some(pl) = plane_z(&f.poly, p) {
                                planes.push(pl);
                            }
                        }
                    }
                    for rg in &g.flat {
                        if near_polygon(p, &rg.outer, 0.5) && !rg.holes.iter().any(|h| point_in_polygon(p, h)) {
                            planes.push(([0.0, 0.0, 1.0], g.base_z));
                        }
                    }
                    for c in &g.caps {
                        let poly: Vec<Point> = c.iter().map(|q| Point::new(q[0], q[1])).collect();
                        if poly.len() >= 3 && near_polygon(p, &poly, 0.5) {
                            planes.push(([0.0, 0.0, 1.0], c[0][2]));
                        }
                    }
                    let Some(best) = planes.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)) else { continue };
                    let singular = planes.iter().any(|q| q.0[0] * best.0[0] + q.0[1] * best.0[1] + q.0[2] * best.0[2] < 0.999_999);
                    let nz = best.0[2].max(1e-6);
                    let above_core: f64 = rt.layers.iter().take_while(|l| !l.core).map(|l| l.thickness_mm).sum();
                    let has_core = rt.layers.iter().any(|l| l.core);
                    let mut push = |surface, z, label: &str, n: [f64; 3]| out.push(SurfaceHit { host: id, surface, label: format!("{label} ({})", rt.name), z, normal: n, singular });
                    push(SpotSurface::Top, best.1, "Roof top", best.0);
                    if has_core && above_core > 0.0 {
                        push(SpotSurface::Core, best.1 - above_core / nz, "Roof core face", best.0);
                    }
                    push(SpotSurface::Bottom, best.1 - rt.thickness_mm() / nz, "Roof bottom", [-best.0[0], -best.0[1], -best.0[2]]);
                }
                Element::Wall(w) if on_level(w.base_level) => {
                    let (hl, hr) = self.wall_extents(w);
                    let inside = if w.curve.is_some() {
                        match wall_arc(w) {
                            Some((arc, ccw)) => {
                                let (inner, outer) = if ccw { (arc.radius - hl, arc.radius + hr) } else { (arc.radius - hr, arc.radius + hl) };
                                let d = p.dist(&arc.centre);
                                d >= inner - 0.5 && d <= outer + 0.5 && arc.spans((p.y - arc.centre.y).atan2(p.x - arc.centre.x))
                            }
                            None => false,
                        }
                    } else {
                        let (dx, dy) = (w.end.x - w.start.x, w.end.y - w.start.y);
                        let len = dx.hypot(dy);
                        if len < 1e-6 {
                            false
                        } else {
                            let (ux, uy) = (dx / len, dy / len);
                            let (t, s) = ((p.x - w.start.x) * ux + (p.y - w.start.y) * uy, -(p.x - w.start.x) * uy + (p.y - w.start.y) * ux);
                            // s is measured to the right of the location line
                            t >= -0.5 && t <= len + 0.5 && s >= -hl - 0.5 && s <= hr + 0.5
                        }
                    };
                    if !inside {
                        continue;
                    }
                    let Some(base) = self.level(w.base_level).map(|l| l.elevation_mm + w.base_offset_mm) else { continue };
                    let top = match w.top_level.and_then(|l| self.level(l)) {
                        Some(l) => l.elevation_mm + w.top_offset_mm,
                        None => base + w.height_mm,
                    };
                    let name = self.wall_type(w.wall_type).map_or("Wall".to_string(), |t| t.name.clone());
                    out.push(SurfaceHit { host: id, surface: SpotSurface::Top, label: format!("Wall top ({name})"), z: top, normal: [0.0, 0.0, 1.0], singular: false });
                    out.push(SurfaceHit { host: id, surface: SpotSurface::Bottom, label: format!("Wall bottom ({name})"), z: base, normal: [0.0, 0.0, -1.0], singular: false });
                }
                _ => {}
            }
        }
        // tops first, then cores, then undersides; the highest of each kind first
        let rank = |s: SpotSurface| match s {
            SpotSurface::Top => 0,
            SpotSurface::Core => 1,
            SpotSurface::Bottom => 2,
        };
        out.sort_by(|a, b| rank(a.surface).cmp(&rank(b.surface)).then(b.z.total_cmp(&a.z)));
        out
    }

    /// A face of one element at a point.
    fn hit_for(&self, view: Id, host: Id, surface: SpotSurface, p: Point) -> Option<SurfaceHit> {
        if self.is_elevation_like(view) {
            // the line it stands on, wherever it has moved to
            return self.elevation_spot_hit(view, host, surface);
        }
        self.surfaces_at(view, p).into_iter().find(|h| h.host == host && h.surface == surface)
    }

    fn origin_z(&self, t: &SpotType, view: Id) -> f64 {
        match t.elevation_origin {
            ElevationOrigin::ProjectBase => self.project_location().map_or(0.0, |(_, l)| l.base.z),
            ElevationOrigin::Survey => self.project_location().map_or(0.0, |(_, l)| l.survey.z),
            ElevationOrigin::Relative => self.view(view).and_then(|v| v.level).and_then(|l| self.level(l)).map_or(0.0, |l| l.elevation_mm),
        }
    }

    /// What a spot reads now, or why it cannot.
    fn spot_value(&self, s: &Spot, t: &SpotType) -> Result<Value, String> {
        let hit = match s.host {
            Some(h) => Some(self.hit_for(s.view, h, s.surface, s.target).ok_or("the spot is no longer on its element")?),
            None => None,
        };
        match s.kind {
            SpotKind::Elevation => {
                let hit = hit.ok_or("a spot elevation needs an element to stand on")?;
                let z = hit.z - self.origin_z(t, s.view);
                let ind = if t.indicator {
                    match s.surface {
                        SpotSurface::Top => format!("{} ", t.top_text),
                        SpotSurface::Core => format!("{} ", t.core_text),
                        SpotSurface::Bottom => format!("{} ", t.bottom_text),
                    }
                } else {
                    String::new()
                };
                Ok(Value { lines: vec![format!("{ind}{}{}{}{}{}", t.prefix, s.prefix, format_length(z, &t.format), s.suffix, t.suffix)], value: Some(z), arrow: None })
            }
            SpotKind::Coordinate => {
                let loc = self.project_location().map(|(_, l)| l.clone());
                let (origin, angle) = match &loc {
                    Some(l) => (
                        match t.coord_origin {
                            CoordOrigin::ProjectBase => (l.base.x, l.base.y),
                            CoordOrigin::Survey => (l.survey.x, l.survey.y),
                            CoordOrigin::Shared => (l.shared_origin.x, l.shared_origin.y),
                        },
                        if t.north_ref == NorthRef::True { l.angle_true_north_deg } else { 0.0 },
                    ),
                    None => ((0.0, 0.0), 0.0),
                };
                // project north is the plan's up; true north is turned by the angle between them
                let (vx, vy) = (s.target.x - origin.0, s.target.y - origin.1);
                let (sn, cs) = (-angle).to_radians().sin_cos();
                let (east, north) = (vx * cs - vy * sn, vx * sn + vy * cs);
                let mut parts: Vec<String> = vec![];
                if t.show_ns {
                    parts.push(format!("{} {}", if north >= 0.0 { "N" } else { "S" }, format_length(north.abs(), &t.format)));
                }
                if t.show_ew {
                    parts.push(format!("{} {}", if east >= 0.0 { "E" } else { "W" }, format_length(east.abs(), &t.format)));
                }
                if t.show_elevation {
                    if let Some(h) = &hit {
                        parts.push(format!("{} {}", t.elevation_text, format_length(h.z - self.origin_z(&SpotType { elevation_origin: ElevationOrigin::ProjectBase, ..t.clone() }, s.view), &t.format)));
                    }
                }
                if parts.is_empty() {
                    return Err("the spot type shows nothing".into());
                }
                let mut lines: Vec<String> = if t.stacked { parts } else { vec![parts.join("  ")] };
                lines[0] = format!("{}{}{}", s.prefix, lines[0], "");
                if let Some(last) = lines.last_mut() {
                    last.push_str(&s.suffix);
                }
                Ok(Value { lines, value: Some(north), arrow: None })
            }
            SpotKind::Slope => {
                let hit = hit.ok_or("a spot slope needs a surface to stand on")?;
                if hit.singular {
                    return Err("the slope is not defined at a ridge, hip, valley or corner".into());
                }
                let mut n = hit.normal;
                if n[2] < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                if n[2].abs() < 1e-6 {
                    return Err("the slope is not defined on a vertical surface".into());
                }
                let h = n[0].hypot(n[1]);
                let grad = h / n[2];
                let text = match t.slope_format {
                    SlopeFormat::Angle => format!("{}\u{00B0}", num(grad.atan().to_degrees(), t.slope_decimals, true)),
                    SlopeFormat::Percent => format!("{}%", num(grad * 100.0, t.slope_decimals, true)),
                    SlopeFormat::Ratio => {
                        if grad < 1e-9 {
                            "level".to_string()
                        } else {
                            format!("1:{}", num(1.0 / grad, t.slope_decimals, true))
                        }
                    }
                    SlopeFormat::RiseRun => format!("{}\"/12\"", num(grad * 12.0, t.slope_decimals, true)),
                };
                // the upward normal of a tilted surface leans downhill, so its horizontal part is the way water runs
                let arrow = (h > 1e-9).then(|| {
                    let down = [n[0] / h, n[1] / h];
                    if t.slope_dir == SlopeDir::Down {
                        down
                    } else {
                        [-down[0], -down[1]]
                    }
                });
                Ok(Value { lines: vec![format!("{}{}{}{}{}", t.prefix, s.prefix, text, s.suffix, t.suffix)], value: Some(grad), arrow })
            }
        }
    }

    /// Draw a spot that may not be in the document yet.
    pub fn draw_spot(&self, id: Id, s: &Spot) -> SpotDraw {
        let t = self.spot_type(s.spot_type).cloned().unwrap_or_else(|| SpotType::new("Spot", s.kind));
        let mut tp = s.target;
        // in an elevation or section the point rides up and down with the line it stands on
        if let Some(h) = s.host.filter(|_| self.is_elevation_like(s.view)).and_then(|h| self.elevation_spot_hit(s.view, h, s.surface)) {
            tp.y = h.z;
        }
        let bend = s.bend.map(|b| [tp.x + b[0], tp.y + b[1]]);
        let shoulder = [tp.x + s.shoulder[0], tp.y + s.shoulder[1]];
        let from = bend.unwrap_or(pt(tp));
        let side = if shoulder[0] < from[0] - 1e-9 { -1.0 } else { 1.0 };
        let mut out = SpotDraw {
            id,
            kind: s.kind,
            type_id: s.spot_type,
            invalid: None,
            target: pt(tp),
            bend,
            shoulder,
            leader: s.leader,
            side,
            symbol: t.symbol,
            lines: vec![],
            above: s.above.clone(),
            below: s.below.clone(),
            arrow: None,
            pinned: s.pinned,
            host: s.host,
            value: None,
        };
        match self.spot_value(s, &t) {
            Ok(v) => {
                out.lines = v.lines;
                out.value = v.value;
                out.arrow = if t.slope_arrow { v.arrow } else { None };
            }
            Err(e) => out.invalid = Some(e),
        }
        out
    }

    pub fn spot_draw(&self, id: Id) -> Option<SpotDraw> {
        let s = self.spot(id)?;
        Some(self.draw_spot(id, s))
    }

    fn default_spot_type(&mut self, kind: SpotKind) -> Result<Id, String> {
        if let Some((id, _)) = self.spot_types(kind).first() {
            return Ok(*id);
        }
        self.transact("Add spot types", |tx| {
            let mut first = None;
            for t in SpotType::defaults(kind) {
                let id = tx.add(Element::SpotType(t));
                first.get_or_insert(id);
            }
            first.ok_or_else(|| "no spot type".to_string())
        })
    }

    fn new_spot(&self, view: Id, kind: SpotKind, target: Point, host: Option<Id>, surface: SpotSurface, shoulder: [f64; 2], leader: bool, tid: Id) -> Result<Spot, String> {
        match self.view(view) {
            Some(v) if matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::CeilingPlan | ViewKind::StructuralPlan | ViewKind::AreaPlan) => {}
            Some(_) if self.is_elevation_like(view) => {
                if kind != SpotKind::Elevation {
                    return Err("spot coordinates and spot slopes are placed in plan views".into());
                }
            }
            Some(_) => return Err("spot annotations are placed in plan, elevation and section views".into()),
            None => return Err("no such view".into()),
        }
        if kind != SpotKind::Coordinate && host.is_none() {
            return Err(if kind == SpotKind::Elevation { "click a floor, roof or wall to put a spot elevation on" } else { "click a floor or roof to put a spot slope on" }.into());
        }
        let s = Spot { view, kind, spot_type: tid, target, host, surface, leader, bend: None, shoulder, prefix: String::new(), suffix: String::new(), above: String::new(), below: String::new(), pinned: false };
        if let Some(h) = host {
            if self.hit_for(view, h, surface, target).is_none() {
                return Err("that point is not on that face".into());
            }
        }
        let t = self.spot_type(tid).cloned().unwrap_or_else(|| SpotType::new("Spot", kind));
        self.spot_value(&s, &t)?;
        Ok(s)
    }

    /// Place a spot. `shoulder` is where the leader ends, as an offset from the point.
    #[allow(clippy::too_many_arguments)]
    pub fn add_spot(&mut self, view: Id, kind: SpotKind, target: Point, host: Option<Id>, surface: SpotSurface, shoulder: [f64; 2], leader: bool, type_id: Option<Id>) -> Result<Id, String> {
        let tid = match type_id {
            Some(t) => match self.spot_type(t) {
                Some(ty) if ty.kind == kind => t,
                Some(_) => return Err("that spot type is for another kind of spot".into()),
                None => return Err("no such spot type".into()),
            },
            None => self.default_spot_type(kind)?,
        };
        let s = self.new_spot(view, kind, target, host, surface, shoulder, leader, tid)?;
        self.transact("Spot", |tx| Ok(tx.add(Element::Spot(s))))
    }

    /// A spot as it would be placed, for the ghost drawn while the tool is active.
    #[allow(clippy::too_many_arguments)]
    pub fn preview_spot(&self, view: Id, kind: SpotKind, target: Point, host: Option<Id>, surface: SpotSurface, shoulder: [f64; 2], leader: bool, type_id: Option<Id>) -> Result<SpotDraw, String> {
        let tid = type_id.filter(|t| self.spot_type(*t).is_some_and(|ty| ty.kind == kind)).or_else(|| self.spot_types(kind).first().map(|(i, _)| *i)).unwrap_or(0);
        let s = self.new_spot(view, kind, target, host, surface, shoulder, leader, tid)?;
        Ok(self.draw_spot(0, &s))
    }

    /// Change a spot: `spot_type`, `surface`, `leader`, `bend` (an offset or null), `shoulder`, `prefix`,
    /// `suffix`, `above`, `below`, `pinned`.
    pub fn patch_spot(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::Spot(old)) = self.get(id).cloned() else { return Err("not a spot".into()) };
        let mut s = old.clone();
        if let Some(v) = patch.get("spot_type").and_then(|v| v.as_u64()) {
            match self.spot_type(v) {
                Some(t) if t.kind == s.kind => s.spot_type = v,
                Some(_) => return Err("that spot type is for another kind of spot".into()),
                None => return Err("no such spot type".into()),
            }
        }
        if let Some(v) = patch.get("surface") {
            s.surface = serde_json::from_value(v.clone()).map_err(|_| "unknown surface".to_string())?;
        }
        if let Some(v) = patch.get("leader").and_then(|v| v.as_bool()) {
            s.leader = v;
        }
        if let Some(v) = patch.get("bend") {
            s.bend = if v.is_null() { None } else { Some(serde_json::from_value(v.clone()).map_err(|_| "bad bend".to_string())?) };
        }
        if let Some(v) = patch.get("shoulder") {
            s.shoulder = serde_json::from_value(v.clone()).map_err(|_| "bad shoulder".to_string())?;
        }
        for (key, slot) in [("prefix", &mut s.prefix), ("suffix", &mut s.suffix), ("above", &mut s.above), ("below", &mut s.below)] {
            if let Some(v) = patch.get(key).and_then(|v| v.as_str()) {
                *slot = v.to_string();
            }
        }
        if let Some(v) = patch.get("pinned").and_then(|v| v.as_bool()) {
            s.pinned = v;
        }
        if s == old {
            return Ok(());
        }
        if s.surface != old.surface {
            if let Some(h) = s.host {
                if self.hit_for(s.view, h, s.surface, s.target).is_none() {
                    return Err("that element has no such face at the point".into());
                }
            }
        }
        let t = self.spot_type(s.spot_type).cloned().unwrap_or_else(|| SpotType::new("Spot", s.kind));
        self.spot_value(&s, &t)?;
        self.transact("Change spot", |tx| tx.update(id, Element::Spot(s)))
    }

    /// The spots standing on any of `ids`.
    pub(crate) fn spots_on(&self, ids: &[Id]) -> Vec<Id> {
        self.spots().into_iter().filter(|(_, s)| s.host.is_some_and(|h| ids.contains(&h))).map(|(i, _)| i).collect()
    }

    /// Carry the spots standing on moved elements along with them.
    pub(crate) fn follow_spots(&mut self, spots: &[Id], x: Xform) -> Result<(), String> {
        for id in spots {
            let Some(Element::Spot(mut s)) = self.get(*id).cloned() else { continue };
            match self.elevation_frame(s.view) {
                // in an elevation or section only the part of the move across the view counts (the height comes from the line)
                Some(f) => {
                    let (o, r) = (Point::new(f.origin[0], f.origin[1]), Point::new(f.right[0], f.right[1]));
                    let at = x.apply(Point::new(o.x + r.x * s.target.x, o.y + r.y * s.target.x));
                    s.target.x = (at.x - o.x) * r.x + (at.y - o.y) * r.y;
                }
                None => s.target = x.apply(s.target),
            }
            self.transact("Move spot", |tx| tx.update(*id, Element::Spot(s)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn setup() -> (Document, Id, Id, Id) {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let ft = d.floor_types()[0].0;
        let view = d.views().iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan).unwrap().0;
        (d, lvl, ft, view)
    }

    fn slab(d: &mut Document, ft: Id, lvl: Id, x: f64, y: f64, w: f64, h: f64) -> Id {
        d.add_floor(ft, lvl, vec![p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h)]).unwrap()
    }

    fn text(d: &Document, id: Id) -> String {
        let dr = d.spot_draw(id).unwrap();
        assert!(dr.invalid.is_none(), "{:?}", dr.invalid);
        dr.lines.join(" | ")
    }

    #[test]
    fn a_spot_elevation_reads_the_face_it_stands_on_and_follows_the_floor() {
        let (mut d, lvl, ft, view) = setup();
        let f = slab(&mut d, ft, lvl, 0.0, 0.0, 5000.0, 4000.0);
        let hits = d.surfaces_at(view, p(1000.0, 1000.0));
        assert_eq!(hits[0].surface, SpotSurface::Top);
        let top = d.add_spot(view, SpotKind::Elevation, p(1000.0, 1000.0), Some(f), SpotSurface::Top, [500.0, 500.0], true, None).unwrap();
        let thickness = d.floor_type(ft).unwrap().thickness_mm();
        assert_eq!(text(&d, top), "RL 0.000");
        let bottom = d.add_spot(view, SpotKind::Elevation, p(1000.0, 1000.0), Some(f), SpotSurface::Bottom, [500.0, 500.0], true, None).unwrap();
        assert_eq!(text(&d, bottom), format!("RL {:.3}", -thickness / 1000.0));
        // the floor moves with its spot, and a change of level height changes the text
        d.transform_elements(&[f], Xform::Move { dx: 1000.0, dy: 0.0 }, Default::default()).unwrap();
        assert_eq!(d.spot(top).unwrap().target.x, 2000.0);
        assert_eq!(text(&d, top), "RL 0.000");
        let Some(Element::Floor(mut fl)) = d.get(f).cloned() else { panic!() };
        fl.offset_mm = 150.0;
        d.transact("test", |tx| tx.update(f, Element::Floor(fl))).unwrap();
        assert_eq!(text(&d, top), "RL 0.150");
        // deleting the floor takes its spots, and undo brings them back
        d.delete(&[f]).unwrap();
        assert!(d.spot(top).is_none() && d.spot(bottom).is_none());
        d.undo();
        assert!(d.spot(top).is_some());
        // a point off the floor has no face
        assert!(d.add_spot(view, SpotKind::Elevation, p(9000.0, 9000.0), Some(f), SpotSurface::Top, [0.0, 0.0], true, None).is_err());
    }

    #[test]
    fn the_type_chooses_the_datum_and_a_level_relative_height_uses_the_view() {
        let (mut d, lvl, ft, view) = setup();
        let f = slab(&mut d, ft, lvl, 0.0, 0.0, 5000.0, 4000.0);
        d.move_site_point(crate::site::SitePoint::Base, crate::site::P3 { x: 0.0, y: 0.0, z: 200.0 }).unwrap();
        let a = d.add_spot(view, SpotKind::Elevation, p(1000.0, 1000.0), Some(f), SpotSurface::Top, [0.0, 0.0], true, None).unwrap();
        assert_eq!(text(&d, a), "RL -0.200", "measured from a base point 200 mm up");
        let rel = d.spot_types(SpotKind::Elevation).into_iter().find(|(_, t)| t.elevation_origin == ElevationOrigin::Relative).unwrap().0;
        d.patch_spot(a, &serde_json::json!({ "spot_type": rel })).unwrap();
        assert_eq!(text(&d, a), "TOP 0.000", "relative to the level of the view, with its indicator");
    }

    #[test]
    fn a_spot_coordinate_is_measured_from_its_origin_in_project_or_true_north() {
        let (mut d, _, _, view) = setup();
        let c = d.add_spot(view, SpotKind::Coordinate, p(12500.0, -3000.0), None, SpotSurface::Top, [500.0, 500.0], true, None).unwrap();
        assert_eq!(text(&d, c), "N -3.000 | E 12.500".replace("N -3.000", "S 3.000"));
        // true north turned 90 degrees from project north swaps the axes
        d.patch_project_location(&serde_json::json!({ "angle_true_north_deg": 90.0 })).unwrap();
        let tn = d.spot_types(SpotKind::Coordinate).into_iter().find(|(_, t)| t.north_ref == NorthRef::True).unwrap().0;
        d.patch_spot(c, &serde_json::json!({ "spot_type": tn })).unwrap();
        assert_eq!(text(&d, c), "S 12.500 | W 3.000");
        // one line when the type says so is a type edit; here the elevation is added by a type that shows it
        assert!(d.add_spot(view, SpotKind::Slope, p(0.0, 0.0), None, SpotSurface::Top, [0.0, 0.0], true, None).is_err());
    }

    #[test]
    fn a_spot_slope_reads_the_gradient_in_the_type_s_format_and_refuses_ridges_and_walls() {
        let (mut d, lvl, ft, view) = setup();
        let f = slab(&mut d, ft, lvl, 0.0, 0.0, 6000.0, 3000.0);
        let Some(Element::Floor(fl0)) = d.get(f).cloned() else { panic!() };
        let loops = fl0.loops();
        let arrow = SlopeArrow { tail: p(0.0, 1500.0), head: p(6000.0, 1500.0), spec: SlopeSpec::Slope { tail_offset_mm: 0.0, percent: 5.0 }, tail_level: None, head_level: None };
        d.set_floor_sketch(f, loops.into_iter().flatten().collect(), Some(arrow), None).unwrap();
        let s = d.add_spot(view, SpotKind::Slope, p(2000.0, 1000.0), Some(f), SpotSurface::Top, [500.0, 500.0], true, None).unwrap();
        assert_eq!(text(&d, s), "5%");
        let ids: Vec<(Id, SlopeFormat)> = d.spot_types(SpotKind::Slope).into_iter().map(|(i, t)| (i, t.slope_format)).collect();
        let by = |fmt: SlopeFormat| ids.iter().find(|(_, f)| *f == fmt).unwrap().0;
        d.patch_spot(s, &serde_json::json!({ "spot_type": by(SlopeFormat::Ratio) })).unwrap();
        assert_eq!(text(&d, s), "1:20");
        d.patch_spot(s, &serde_json::json!({ "spot_type": by(SlopeFormat::Angle) })).unwrap();
        assert_eq!(text(&d, s), "2.86\u{00B0}");
        d.patch_spot(s, &serde_json::json!({ "spot_type": by(SlopeFormat::RiseRun) })).unwrap();
        assert_eq!(text(&d, s), "0.6\"/12\"");
        // the arrow points down the slope: the floor rises towards +x, so it runs towards -x
        let a = d.spot_draw(s).unwrap().arrow.unwrap();
        assert!(a[0] < -0.99, "{a:?}");
        assert_eq!(ids.len(), 4);
        // a wall's top is level: slope zero, no arrow; its face is vertical
        let wt = d.wall_types()[0].0;
        let w = d.add_wall(wt, lvl, p(8000.0, 0.0), p(12000.0, 0.0), 2700.0).unwrap();
        let ws = d.add_spot(view, SpotKind::Slope, p(10000.0, 0.0), Some(w), SpotSurface::Top, [0.0, 500.0], true, None).unwrap();
        assert_eq!(text(&d, ws), "0%");
        assert!(d.spot_draw(ws).unwrap().arrow.is_none());
        let _ = Curve::Line { a: p(0.0, 0.0), b: p(1.0, 0.0) };
    }
    #[test]
    fn spots_on_a_roof_read_the_plane_and_refuse_a_slope_on_the_ridge() {
        let mut d = Document::new_project();
        let (lvl, rt) = (d.levels()[1].0, d.roof_types()[0].0);
        let view = d.views().iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan && v.level == Some(lvl)).map(|(i, _)| *i).unwrap_or_else(|| d.add_floor_plan(lvl, "Level 2").unwrap());
        let c = [p(0.0, 0.0), p(10000.0, 0.0), p(10000.0, 6000.0), p(0.0, 6000.0)];
        let edges: Vec<crate::roof::RoofEdge> = (0..4).map(|i| crate::roof::RoofEdge::new(Curve::line(c[i], c[(i + 1) % 4]))).collect();
        let roof = d.add_roof_sketch(rt, lvl, edges).unwrap();
        let z = 3000.0 + 2000.0 * 22.5f64.to_radians().tan();
        let e = d.add_spot(view, SpotKind::Elevation, p(2000.0, 3000.0), Some(roof), SpotSurface::Top, [300.0, 300.0], true, None).unwrap();
        assert_eq!(text(&d, e), format!("RL {:.3}", z / 1000.0));
        let s = d.add_spot(view, SpotKind::Slope, p(2000.0, 3000.0), Some(roof), SpotSurface::Top, [300.0, 300.0], true, None).unwrap();
        assert_eq!(text(&d, s), "41.4%");
        // on the ridge two planes meet, so there is a height but no slope
        assert!(d.add_spot(view, SpotKind::Elevation, p(5000.0, 3000.0), Some(roof), SpotSurface::Top, [300.0, 300.0], true, None).is_ok());
        let err = d.add_spot(view, SpotKind::Slope, p(5000.0, 3000.0), Some(roof), SpotSurface::Top, [300.0, 300.0], true, None).unwrap_err();
        assert!(err.contains("ridge"), "{err}");
        // the underside is lower by the thickness over cos(slope)
        let hits = d.surfaces_at(view, p(2000.0, 3000.0));
        let (t, b) = (hits.iter().find(|h| h.surface == SpotSurface::Top).unwrap(), hits.iter().find(|h| h.surface == SpotSurface::Bottom).unwrap());
        let thick = d.roof_type(rt).unwrap().thickness_mm();
        assert!((t.z - b.z - thick / 22.5f64.to_radians().cos()).abs() < 1e-6);
    }

}
