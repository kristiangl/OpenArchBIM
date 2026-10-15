//! Section views: a section line drawn in plan makes a view that looks along the line's normal.
//!
//! The line is the cut plane. Walls, floors and roofs crossing it are sliced into closed profiles that
//! are drawn with the category's cut weight and the material's cut pattern; what lies between the plane
//! and the far clip is projected the way an elevation shows it (see `Document::elevation_data`).
//!
//! A section stores everything the later tools edit (depth cueing, bends, centre break, a
//! reference to another view) with serde defaults, so project files saved before those tools still open.

use serde::{Deserialize, Serialize};

use crate::doc::{apply_patch, Document};
use crate::element::{Element, Id};
use crate::geom::Point;
use crate::site::{ElevFrame, ElevItem};
use crate::view::{View, ViewKind};

/// The cut plane sits a hair in front of the drawn line, so geometry lying exactly on the line is
/// sliced consistently instead of flickering between cut and not cut.
pub const PLANE_EPS: f64 = 1e-5;

/// Shortest section line that makes a view (mm).
pub const MIN_LENGTH_MM: f64 = 100.0;

/// What happens at the far clip distance (Far Clipping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FarClip {
    /// Clip at the distance and draw a line where the clip plane passes through geometry.
    #[default]
    WithLine,
    /// Clip at the distance with no line.
    WithoutLine,
    /// Show everything beyond the cut plane.
    NoClip,
}

/// Depth cueing fades what is far from the cut plane (Show Depth). Stored now, drawn in a later step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepthCue {
    pub enabled: bool,
    /// Where the fade starts and ends, as a percentage of the view depth.
    pub start_pct: f64,
    pub end_pct: f64,
    /// How faint the farthest geometry gets (0 to 1, 1 means no fade).
    pub min_intensity: f64,
}

impl Default for DepthCue {
    fn default() -> Self {
        DepthCue { enabled: false, start_pct: 20.0, end_pct: 100.0, min_intensity: 0.3 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// The level whose plan it was drawn in.
    pub level: Id,
    pub start: Point,
    pub end: Point,
    /// Looks to the other side of the line.
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub far_clip: FarClip,
    /// How deep the view looks. `None` fits the model.
    #[serde(default)]
    pub far_clip_mm: Option<f64>,
    /// Bottom and top of the view (absolute mm). `None` fits the levels and the model.
    #[serde(default)]
    pub z_range: Option<(f64, f64)>,
    /// The view this section makes.
    #[serde(default)]
    pub view: Option<Id>,
    /// The annotation family it is drawn with. `None` uses the first section marker family.
    #[serde(default)]
    pub family: Option<Id>,
    #[serde(default)]
    pub pinned: bool,
    /// Extra points of a jogged section, in order along the line (step 2).
    #[serde(default)]
    pub bends: Vec<Point>,
    /// The middle of the line is hidden in plan; the cut plane stays continuous (step 2).
    #[serde(default)]
    pub centre_break: bool,
    #[serde(default)]
    pub depth_cue: DepthCue,
    /// A reference section points at an existing view instead of making its own (step 2).
    #[serde(default)]
    pub reference: Option<Id>,
}

impl Section {
    pub fn new(level: Id, start: Point, end: Point) -> Section {
        Section {
            level,
            start,
            end,
            flipped: false,
            far_clip: FarClip::WithLine,
            far_clip_mm: None,
            z_range: None,
            view: None,
            family: None,
            pinned: false,
            bends: vec![],
            centre_break: false,
            depth_cue: DepthCue::default(),
            reference: None,
        }
    }

    pub fn length(&self) -> f64 {
        self.start.dist(&self.end)
    }

    /// The ends as the view sees them: `a` at the left of the view, `b` at the right.
    pub fn view_ends(&self) -> (Point, Point) {
        if self.flipped {
            (self.end, self.start)
        } else {
            (self.start, self.end)
        }
    }

    /// Unit vectors (right on screen, look) of the view, `None` for a line with no length.
    pub fn axes(&self) -> Option<(Point, Point)> {
        let (a, b) = self.view_ends();
        let l = a.dist(&b);
        if l < 1e-6 {
            return None;
        }
        let t = Point::new((b.x - a.x) / l, (b.y - a.y) / l);
        Some((t, Point::new(-t.y, t.x)))
    }
}

impl View {
    /// The view of a section.
    pub fn section_view(name: &str, section: Id) -> View {
        let mut v = View::elevation(name, 0, 0);
        v.view_kind = ViewKind::Section;
        v.elevation = None;
        v.section = Some(section);
        v
    }
}

/// A cut plane through the model and the box in front of it, in the section's own axes: `u` along
/// the line, `d` the distance in front of the plane.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slice {
    pub o: Point,
    pub right: Point,
    pub look: Point,
    pub len: f64,
    pub depth: f64,
    pub far: FarClip,
}

/// Where the cut plane passes through a closed outline: the stretch between two crossings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cut {
    pub u0: f64,
    pub u1: f64,
    pub p0: Point,
    pub p1: Point,
}

impl Slice {
    pub fn u(&self, p: Point) -> f64 {
        (p.x - self.o.x) * self.right.x + (p.y - self.o.y) * self.right.y
    }

    pub fn d(&self, p: Point) -> f64 {
        (p.x - self.o.x) * self.look.x + (p.y - self.o.y) * self.look.y
    }

    /// Keep the part of a polygon between the cut plane and the far clip.
    pub fn clip_ring(&self, ring: &[Point]) -> Vec<Point> {
        let r = clip_by(ring, |p| self.d(p), 0.0, true);
        if self.far == FarClip::NoClip {
            return r;
        }
        clip_by(&r, |p| self.d(p), self.depth, false)
    }

    /// The same for a polygon in three dimensions (a roof plane).
    pub fn clip_ring3(&self, poly: &[[f64; 3]]) -> Vec<[f64; 3]> {
        let d = |q: &[f64; 3]| self.d(Point::new(q[0], q[1]));
        let r = clip3(poly, d, 0.0, true);
        if self.far == FarClip::NoClip {
            return r;
        }
        clip3(&r, d, self.depth, false)
    }

    /// Keep the part of a drawing polygon within the width of the section.
    pub fn clip_width(&self, poly: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let r = clip2(poly, 0, 0.0, true);
        clip2(&r, 0, self.len, false)
    }

    /// The stretches of `plane` (a distance in front of the cut plane) that lie inside the rings, by the
    /// even-odd rule, so holes and islands come out right. Clipped to the width of the section.
    pub fn cuts(&self, rings: &[&[Point]], plane: f64) -> Vec<Cut> {
        let mut xs: Vec<(f64, Point)> = vec![];
        for ring in rings {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let (da, db) = (self.d(a) - plane, self.d(b) - plane);
                if (da < 0.0) == (db < 0.0) {
                    continue;
                }
                let t = da / (da - db);
                let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                xs.push((self.u(p), p));
            }
        }
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = vec![];
        for pair in xs.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            if b.0 - a.0 < 1e-6 {
                continue;
            }
            let c = Cut { u0: a.0, u1: b.0, p0: a.1, p1: b.1 };
            if let Some(c) = self.within_width(c) {
                out.push(c);
            }
        }
        out
    }

    fn within_width(&self, c: Cut) -> Option<Cut> {
        if c.u1 <= 0.0 || c.u0 >= self.len {
            return None;
        }
        let lerp = |u: f64| {
            let t = (u - c.u0) / (c.u1 - c.u0);
            Point::new(c.p0.x + (c.p1.x - c.p0.x) * t, c.p0.y + (c.p1.y - c.p0.y) * t)
        };
        let (u0, u1) = (c.u0.max(0.0), c.u1.min(self.len));
        Some(Cut { u0, u1, p0: lerp(u0), p1: lerp(u1) })
    }
}

/// Sutherland-Hodgman against one side of a line. Good enough for the extents we take from the result,
/// even for polygons that are not convex.
fn clip_by(poly: &[Point], f: impl Fn(Point) -> f64, at: f64, keep_above: bool) -> Vec<Point> {
    let inside = |v: f64| if keep_above { v >= at } else { v <= at };
    let n = poly.len();
    let mut out = vec![];
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (fa, fb) = (f(a), f(b));
        if inside(fa) {
            out.push(a);
        }
        if inside(fa) != inside(fb) {
            let t = (at - fa) / (fb - fa);
            out.push(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
    }
    out
}

fn clip3(poly: &[[f64; 3]], f: impl Fn(&[f64; 3]) -> f64, at: f64, keep_above: bool) -> Vec<[f64; 3]> {
    let inside = |v: f64| if keep_above { v >= at } else { v <= at };
    let n = poly.len();
    let mut out = vec![];
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (fa, fb) = (f(&a), f(&b));
        if inside(fa) {
            out.push(a);
        }
        if inside(fa) != inside(fb) {
            let t = (at - fa) / (fb - fa);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }
    out
}

fn clip2(poly: &[[f64; 2]], axis: usize, at: f64, keep_above: bool) -> Vec<[f64; 2]> {
    let inside = |v: f64| if keep_above { v >= at } else { v <= at };
    let n = poly.len();
    let mut out = vec![];
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if inside(a[axis]) {
            out.push(a);
        }
        if inside(a[axis]) != inside(b[axis]) {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// Rectangle corners, counter-clockwise.
fn rect(u0: f64, u1: f64, z0: f64, z1: f64) -> Vec<[f64; 2]> {
    vec![[u0, z0], [u1, z0], [u1, z1], [u0, z1]]
}

impl Document {
    pub fn sections(&self) -> Vec<(Id, &Section)> {
        self.iter().filter_map(|(id, e)| if let Element::Section(s) = e { Some((id, s)) } else { None }).collect()
    }

    pub fn section(&self, id: Id) -> Option<&Section> {
        match self.get(id) {
            Some(Element::Section(s)) => Some(s),
            _ => None,
        }
    }

    /// The labels of a section marker with their text worked out. A view not yet on a sheet reads "-" for its number
    /// and its sheet.
    pub fn section_labels(&self, s: &Section) -> Vec<crate::annotation::DrawnLabel> {
        let Some((_, f)) = self.section_marker_family(s.family) else { return vec![] };
        let name = s.view.and_then(|v| self.view(v)).map(|v| v.name.clone()).unwrap_or_default();
        f.labels
            .iter()
            .map(|lb| crate::annotation::DrawnLabel {
                x: lb.x,
                y: lb.y,
                halign: lb.halign,
                valign: lb.valign,
                size_mm: lb.size_mm,
                bold: lb.bold,
                lines: lb
                    .lines
                    .iter()
                    .filter_map(|ln| {
                        let v = match ln.param.as_str() {
                            "View Name" => Some(name.clone()),
                            "View Number" | "Sheet Number" => Some("-".to_string()),
                            _ => None,
                        };
                        v.map(|v| format!("{}{}{}", ln.prefix, v, ln.suffix))
                    })
                    .collect(),
            })
            .collect()
    }

    /// The family a section marker draws with.
    pub fn section_marker_family(&self, family: Option<Id>) -> Option<(Id, &crate::annotation::AnnotationFamily)> {
        use crate::annotation::AnnotationKind;
        let id = family.filter(|f| self.annotation_family(*f).is_some_and(|f| f.kind == AnnotationKind::SectionMarker)).or_else(|| self.first_family(AnnotationKind::SectionMarker))?;
        self.annotation_family(id).map(|f| (id, f))
    }

    fn next_section_name(&self) -> String {
        let mut k = 1;
        loop {
            let name = format!("Section {k}");
            if !self.views().iter().any(|(_, v)| v.name == name) {
                return name;
            }
            k += 1;
        }
    }

    /// Draw a section line in the plan of `level`. Makes the section and its view in one undo step.
    pub fn add_section(&mut self, level: Id, start: Point, end: Point, flipped: bool) -> Result<Id, String> {
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        if start.dist(&end) < MIN_LENGTH_MM {
            return Err("the section line is too short".into());
        }
        let name = self.next_section_name();
        self.transact("Create section", |tx| {
            let mut s = Section::new(level, start, end);
            s.flipped = flipped;
            let id = tx.add(Element::Section(s.clone()));
            let view = tx.add(Element::View(View::section_view(&name, id)));
            s.view = Some(view);
            tx.update(id, Element::Section(s))?;
            Ok(id)
        })
    }

    /// Change a section with a JSON patch (ends, flip, far clip, range, family, depth cue, pin). One undo step.
    pub fn patch_section(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.section(id).cloned() else { return Err("not a section".into()) };
        let mut patch = patch.clone();
        if let Some(o) = patch.as_object_mut() {
            o.remove("view");
            o.remove("level");
        }
        let new: Section = apply_patch(&old, &patch)?;
        if new == old {
            return Ok(());
        }
        if new.start.dist(&new.end) < MIN_LENGTH_MM {
            return Err("the section line is too short".into());
        }
        if new.far_clip_mm.is_some_and(|v| !v.is_finite() || v < 100.0) {
            return Err("the far clip distance must be at least 100 mm".into());
        }
        if new.z_range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite() || b - a < 100.0) {
            return Err("the section must be at least 100 mm tall".into());
        }
        if new.family.is_some_and(|f| self.annotation_family(f).is_none()) {
            return Err("unknown section marker".into());
        }
        let d = &new.depth_cue;
        if !(0.0..=100.0).contains(&d.start_pct) || !(0.0..=100.0).contains(&d.end_pct) || d.start_pct > d.end_pct || !(0.0..=1.0).contains(&d.min_intensity) {
            return Err("depth cue values are out of range".into());
        }
        if old.pinned && new.pinned && (new.start != old.start || new.end != old.end || new.flipped != old.flipped) {
            return Err("the section is pinned; unpin it first".into());
        }
        self.transact("Change section", |tx| tx.update(id, Element::Section(new)))
    }

    /// Delete a section with its view.
    pub fn delete_section(&mut self, id: Id) -> Result<(), String> {
        let Some(s) = self.section(id).cloned() else { return Err("not a section".into()) };
        if s.pinned {
            return Err("the section is pinned; unpin it first".into());
        }
        self.transact("Delete section", |tx| {
            if let Some(v) = s.view {
                tx.remove(v)?;
            }
            tx.remove(id)
        })
    }

    /// The section a view shows.
    pub fn section_of_view(&self, view: Id) -> Option<(Id, &Section)> {
        let sid = self.view(view)?.section?;
        self.section(sid).map(|s| (sid, s))
    }

    pub(crate) fn section_slice(&self, view: Id) -> Option<Slice> {
        let (_, s) = self.section_of_view(view)?;
        let (a, _) = s.view_ends();
        let (right, look) = s.axes()?;
        let depth = match s.far_clip {
            FarClip::NoClip => 1.0e7,
            _ => s.far_clip_mm.unwrap_or_else(|| match self.model_bounds() {
                Some((lo, hi)) => {
                    let far = [Point::new(lo.x, lo.y), Point::new(hi.x, lo.y), Point::new(hi.x, hi.y), Point::new(lo.x, hi.y)]
                        .iter()
                        .map(|p| (p.x - a.x) * look.x + (p.y - a.y) * look.y)
                        .fold(f64::NEG_INFINITY, f64::max);
                    (far + 1000.0).max(2000.0)
                }
                None => 10000.0,
            }),
        };
        Some(Slice { o: a, right, look, len: s.length(), depth, far: s.far_clip })
    }

    /// Frame of a section view: the line is the width, the levels and model the height.
    pub fn section_frame(&self, view: Id) -> Option<ElevFrame> {
        let sl = self.section_slice(view)?;
        let (_, s) = self.section_of_view(view)?;
        let (z0, z1) = s.z_range.unwrap_or_else(|| {
            let (a, b) = self.model_z_extent();
            (a - 1500.0, b + 1500.0)
        });
        Some(ElevFrame { origin: [sl.o.x, sl.o.y], look: [sl.look.x, sl.look.y], right: [sl.right.x, sl.right.y], left_mm: 0.0, right_mm: sl.len, depth_mm: sl.depth, z_min: z0, z_max: z1 })
    }

    /// Whether the section line shows in a plan whose cut plane is at `z`.
    pub fn section_shows_at(&self, id: Id, z: f64) -> bool {
        let Some(s) = self.section(id) else { return false };
        match s.z_range {
            Some((a, b)) => z >= a && z <= b,
            None => {
                let (a, b) = self.model_z_extent();
                z >= a - 1500.0 && z <= b + 1500.0
            }
        }
    }

    /// Everything the cut plane passes through, as closed profiles in the section's own axes (u across, z up):
    /// wall layers with the openings of doors and windows taken out, floor layers, roofs. They draw over the
    /// projection. With the far clip on, `far_lines` also gets the outline of what the far clip plane passes through.
    pub(crate) fn section_cut_items(&self, sl: &Slice, hidden: &dyn Fn(Id) -> bool, far_lines: &mut Vec<ElevItem>) -> Vec<ElevItem> {
        let mut out: Vec<ElevItem> = vec![];
        let shade = |m: &str| self.material(m).map_or([200, 200, 200], |m| [m.graphics.shading_color[0], m.graphics.shading_color[1], m.graphics.shading_color[2]]);
        let far_wanted = sl.far == FarClip::WithLine;
        // ---- walls
        let layer_geo = self.wall_layer_geometry();
        let polys = self.wall_polygons();
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            if hidden(id) {
                continue;
            }
            let (z0, z1) = self.wall_extent(w);
            // openings of the doors and windows in this wall, as stretches of the plane
            let mut openings: Vec<(f64, f64, f64, f64)> = vec![];
            for (iid, i) in self.instances() {
                let Some(h) = i.host else { continue };
                if h.wall != id || hidden(iid) {
                    continue;
                }
                let Some(f) = self.instance_frame(iid) else { continue };
                let c = Point::new(f.origin.x + f.dir.x * f.x_off, f.origin.y + f.dir.y * f.x_off);
                let (hw, pad) = (f.width / 2.0, 1500.0);
                let ring = [
                    Point::new(c.x - f.dir.x * hw - f.left.x * pad, c.y - f.dir.y * hw - f.left.y * pad),
                    Point::new(c.x + f.dir.x * hw - f.left.x * pad, c.y + f.dir.y * hw - f.left.y * pad),
                    Point::new(c.x + f.dir.x * hw + f.left.x * pad, c.y + f.dir.y * hw + f.left.y * pad),
                    Point::new(c.x - f.dir.x * hw + f.left.x * pad, c.y - f.dir.y * hw + f.left.y * pad),
                ];
                for cut in sl.cuts(&[&ring], PLANE_EPS) {
                    openings.push((cut.u0, cut.u1, f.z0, f.z1));
                }
            }
            let push_cuts = |out: &mut Vec<ElevItem>, rings: &[&[Point]], material: Option<&str>, fill: [u8; 3], depth: f64| {
                for cut in sl.cuts(rings, PLANE_EPS) {
                    let holes: Vec<Vec<[f64; 2]>> = openings
                        .iter()
                        .filter_map(|&(a, b, h0, h1)| {
                            let (u0, u1, v0, v1) = (a.max(cut.u0), b.min(cut.u1), h0.max(z0), h1.min(z1));
                            (u1 - u0 > 0.5 && v1 - v0 > 0.5).then(|| rect(u0, u1, v0, v1))
                        })
                        .collect();
                    out.push(ElevItem { id, category: "Walls", depth, polys: vec![rect(cut.u0, cut.u1, z0, z1)], fill, cut: true, material: material.map(str::to_string), holes, ..Default::default() });
                }
            };
            if let Some(wlay) = layer_geo.get(&id) {
                for (k, layer) in wlay.layers.iter().enumerate().filter(|(_, l)| !l.is_void && l.thickness > 0.0) {
                    for shape in &layer.shapes {
                        let rings: Vec<&[Point]> = shape.iter().map(|r| r.as_slice()).collect();
                        push_cuts(&mut out, &rings, Some(&layer.material), shade(&layer.material), -1.0 + 0.001 * k as f64);
                        if far_wanted {
                            self.far_outline(sl, &rings, id, "Walls", z0, z1, far_lines);
                        }
                    }
                }
            } else if let Some(p) = polys.get(&id) {
                push_cuts(&mut out, &[p.as_slice()], None, [200, 200, 200], -1.0);
                if far_wanted {
                    self.far_outline(sl, &[p.as_slice()], id, "Walls", z0, z1, far_lines);
                }
            }
        }
        // ---- floors
        for (id, e) in self.iter() {
            let Element::Floor(fl) = e else { continue };
            if hidden(id) {
                continue;
            }
            let Some(surface) = self.floor_surface(fl) else { continue };
            let loops: Vec<Vec<Point>> = fl.loops().iter().map(|l| crate::curve::loop_polygon(l, 20.0)).collect();
            let rings: Vec<&[Point]> = loops.iter().filter(|l| l.len() >= 3).map(|l| l.as_slice()).collect();
            let ft = self.floor_type(fl.floor_type);
            let bands: Vec<(Option<&str>, f64, bool)> = match ft {
                Some(t) if !t.layers.is_empty() => t.layers.iter().map(|l| (Some(l.material.as_str()), l.thickness_mm, l.is_void)).collect(),
                _ => vec![(None, 200.0, false)],
            };
            for cut in sl.cuts(&rings, PLANE_EPS) {
                let (t0, t1) = (surface.z(cut.p0), surface.z(cut.p1));
                let mut off = 0.0;
                for (mat, th, void) in &bands {
                    let (hi, lo) = (off, off + th);
                    off += th;
                    if *void || *th <= 0.0 {
                        continue;
                    }
                    let poly = vec![[cut.u0, t0 - lo], [cut.u1, t1 - lo], [cut.u1, t1 - hi], [cut.u0, t0 - hi]];
                    out.push(ElevItem { id, category: "Floors", depth: -1.0 + 0.002, polys: vec![poly], fill: mat.map_or([170, 172, 176], shade), cut: true, material: mat.map(str::to_string), ..Default::default() });
                }
            }
            if far_wanted {
                let (top, thick) = (surface.base, bands.iter().map(|b| b.1).sum::<f64>());
                self.far_outline(sl, &rings, id, "Floors", top - thick, top, far_lines);
            }
        }
        // ---- roofs
        for (id, e) in self.iter() {
            let Element::Roof(r) = e else { continue };
            if hidden(id) {
                continue;
            }
            let Some(g) = self.roof_geometry(r) else { continue };
            let t = self.roof_type(r.roof_type);
            let mat = t.and_then(|t| t.layers.first()).map(|l| l.material.clone());
            let fill = mat.as_deref().map_or([150, 120, 110], shade);
            let thick = t.map_or(0.0, |t| t.thickness_mm());
            if thick <= 0.0 {
                continue;
            }
            // Sloping planes: where the plane crosses the polygon of each face, with the height of the surface at both ends.
            struct Seg {
                u0: f64,
                z0: f64,
                u1: f64,
                z1: f64,
                drop: f64,
            }
            let mut segs: Vec<Seg> = vec![];
            for f in g.faces.iter().filter(|f| !f.vertical) {
                let n = f.poly.len();
                let mut xs: Vec<(f64, f64)> = vec![];
                for i in 0..n {
                    let (a, b) = (f.poly[i], f.poly[(i + 1) % n]);
                    let (da, db) = (sl.d(Point::new(a[0], a[1])) - PLANE_EPS, sl.d(Point::new(b[0], b[1])) - PLANE_EPS);
                    if (da < 0.0) == (db < 0.0) {
                        continue;
                    }
                    let k = da / (da - db);
                    let p = Point::new(a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k);
                    xs.push((sl.u(p), a[2] + (b[2] - a[2]) * k));
                }
                xs.sort_by(|a, b| a.0.total_cmp(&b.0));
                let drop = thick / f.slope_deg.min(80.0).to_radians().cos().max(0.2);
                for pair in xs.chunks_exact(2) {
                    let ((u0, z0), (u1, z1)) = (pair[0], pair[1]);
                    if u1 - u0 < 1e-6 || u1 <= 0.0 || u0 >= sl.len {
                        continue;
                    }
                    let lerp = |u: f64| z0 + (z1 - z0) * (u - u0) / (u1 - u0);
                    let (a, b) = (u0.max(0.0), u1.min(sl.len));
                    segs.push(Seg { u0: a, z0: lerp(a), u1: b, z1: lerp(b), drop });
                }
            }
            segs.sort_by(|a, b| a.u0.total_cmp(&b.u0));
            // planes that meet at a ridge or valley are one profile
            let mut k = 0;
            while k < segs.len() {
                let mut chain = vec![&segs[k]];
                while k + 1 < segs.len() && (segs[k + 1].u0 - chain.last().unwrap().u1).abs() < 1.0 && (segs[k + 1].z0 - chain.last().unwrap().z1).abs() < 5.0 {
                    k += 1;
                    chain.push(&segs[k]);
                }
                k += 1;
                let mut poly: Vec<[f64; 2]> = vec![];
                for s in &chain {
                    poly.push([s.u0, s.z0]);
                    poly.push([s.u1, s.z1]);
                }
                for s in chain.iter().rev() {
                    poly.push([s.u1, s.z1 - s.drop]);
                    poly.push([s.u0, s.z0 - s.drop]);
                }
                out.push(ElevItem { id, category: "Roofs", depth: -1.0 + 0.003, polys: vec![poly], fill, cut: true, material: mat.clone(), ..Default::default() });
            }
            // Flat roofs are a slab at the base height.
            let rings: Vec<&[Point]> = g.flat.iter().flat_map(|r| std::iter::once(r.outer.as_slice()).chain(r.holes.iter().map(|h| h.as_slice()))).collect();
            if !rings.is_empty() {
                for cut in sl.cuts(&rings, PLANE_EPS) {
                    out.push(ElevItem { id, category: "Roofs", depth: -1.0 + 0.003, polys: vec![rect(cut.u0, cut.u1, g.base_z - thick, g.base_z)], fill, cut: true, material: mat.clone(), ..Default::default() });
                }
            }
        }
        out
    }

    /// Where the far clip plane passes through an outline: the profile drawn as thin lines behind everything.
    #[allow(clippy::too_many_arguments)]
    fn far_outline(&self, sl: &Slice, rings: &[&[Point]], id: Id, category: &'static str, z0: f64, z1: f64, out: &mut Vec<ElevItem>) {
        let mut lines: Vec<[[f64; 2]; 2]> = vec![];
        for cut in sl.cuts(rings, sl.depth) {
            let (a, b) = ([cut.u0, z0], [cut.u1, z1]);
            lines.extend([[[a[0], a[1]], [b[0], a[1]]], [[b[0], a[1]], [b[0], b[1]]], [[b[0], b[1]], [a[0], b[1]]], [[a[0], b[1]], [a[0], a[1]]]]);
        }
        if !lines.is_empty() {
            out.push(ElevItem { id, category, depth: sl.depth + 1.0, lines, stroke: false, ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn house() -> (Document, Id) {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        for ((ax, ay), (bx, by)) in [((0., 0.), (8000., 0.)), ((8000., 0.), (8000., 6000.)), ((8000., 6000.), (0., 6000.)), ((0., 6000.), (0., 0.))] {
            d.add_wall(wt, lvl, Point::new(ax, ay), Point::new(bx, by), 2700.0).unwrap();
        }
        (d, lvl)
    }

    #[test]
    fn a_section_makes_a_view_and_deletes_with_it() {
        let (mut d, lvl) = house();
        let n = d.views().len();
        let s = d.add_section(lvl, Point::new(4000.0, -1000.0), Point::new(4000.0, 7000.0), false).unwrap();
        assert_eq!(d.views().len(), n + 1);
        let v = d.section(s).unwrap().view.unwrap();
        assert_eq!(d.view(v).unwrap().view_kind, ViewKind::Section);
        assert_eq!(d.view(v).unwrap().name, "Section 1");
        assert!(d.add_section(lvl, Point::new(0.0, 0.0), Point::new(10.0, 0.0), false).is_err());
        d.undo();
        assert_eq!(d.views().len(), n);
        d.redo();
        d.delete_section(s).unwrap();
        assert_eq!(d.views().len(), n);
        assert!(d.section(s).is_none());
    }

    #[test]
    fn the_frame_looks_to_the_left_of_the_line_and_flips() {
        let (mut d, lvl) = house();
        // drawn from south to north, so the view looks west (left of the direction of drawing)
        let s = d.add_section(lvl, Point::new(4000.0, -1000.0), Point::new(4000.0, 7000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let f = d.elevation_frame(v).unwrap();
        assert!((f.look[0] + 1.0).abs() < 1e-9 && f.look[1].abs() < 1e-9, "{:?}", f.look);
        assert!((f.right[1] - 1.0).abs() < 1e-9);
        assert!((f.right_mm - 8000.0).abs() < 1e-6 && f.left_mm == 0.0);
        d.patch_section(s, &serde_json::json!({"flipped": true})).unwrap();
        let g = d.elevation_frame(v).unwrap();
        assert!((g.look[0] - 1.0).abs() < 1e-9);
        assert!((g.origin[1] - 7000.0).abs() < 1e-9);
    }

    #[test]
    fn the_cut_slices_walls_into_layers_and_leaves_the_far_walls_in_projection() {
        let (mut d, lvl) = house();
        // west to east through the middle, looking north
        let s = d.add_section(lvl, Point::new(-1000.0, 3000.0), Point::new(9000.0, 3000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let data = d.elevation_data(v).unwrap();
        let cuts: Vec<_> = data.items.iter().filter(|i| i.cut).collect();
        assert!(!cuts.is_empty(), "the east and west walls are cut");
        // both side walls are cut, each into several layers whose widths add up to the wall thickness
        let width = |lo: f64, hi: f64| -> f64 { cuts.iter().filter(|i| i.category == "Walls" && i.polys[0][0][0] >= lo && i.polys[0][1][0] <= hi).map(|i| i.polys[0][1][0] - i.polys[0][0][0]).sum() };
        let west = width(500.0, 1500.0);
        let east = width(8500.0, 9500.0);
        assert!(west > 100.0 && (west - east).abs() < 1.0, "west {west} east {east}");
        assert!(cuts.iter().all(|i| i.depth < 0.0), "cuts draw over the projection");
        assert!(cuts.iter().any(|i| i.material.is_some()));
        // the north wall is beyond the plane and shown in projection, uncut
        assert!(data.items.iter().any(|i| !i.cut && i.category == "Walls"));
        // nothing south of the plane is shown: the south wall is behind the viewer
        let south: Vec<_> = data.items.iter().filter(|i| !i.cut && i.category == "Walls" && i.depth < 100.0).collect();
        assert!(south.is_empty() || south.iter().all(|i| i.depth >= 0.0));
        // lateral limits: nothing outside the line
        for i in &data.items {
            for p in &i.polys {
                for q in p {
                    assert!(q[0] >= -1e-6 && q[0] <= 10000.0 + 1e-6, "{q:?}");
                }
            }
        }
    }

    #[test]
    fn a_window_in_the_cut_wall_leaves_a_hole() {
        let (mut d, lvl) = house();
        let ty = d.family_types().into_iter().find(|(_, t)| d.family(t.family).is_some_and(|f| f.category == crate::family::FamilyCategory::Windows)).unwrap().0;
        // the south wall runs west to east; a window 4000 along it, cut by a section through x = 4000
        let south = d.iter().find_map(|(id, e)| if let Element::Wall(w) = e { (w.path().pts[0].y == 0.0 && w.path().pts.last().unwrap().y == 0.0).then_some(id) } else { None }).unwrap();
        let win = d.place_on_wall(ty, south, Point::new(4000.0, 0.0)).unwrap();
        let s = d.add_section(lvl, Point::new(4000.0, -2000.0), Point::new(4000.0, 4000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let data = d.elevation_data(v).unwrap();
        let holed: Vec<_> = data.items.iter().filter(|i| i.cut && i.id == south && !i.holes.is_empty()).collect();
        assert!(!holed.is_empty(), "the window opening is a hole in the wall layers");
        let hole = &holed[0].holes[0];
        let f = d.instance_frame(win).unwrap();
        assert!((hole[0][1] - f.z0).abs() < 1.0 && (hole[2][1] - f.z1).abs() < 1.0);
    }

    #[test]
    fn floors_are_cut_in_layers_and_follow_a_slope() {
        let (mut d, lvl) = house();
        let ft = d.floor_types()[0].0;
        d.add_floor(ft, lvl, vec![Point::new(0.0, 0.0), Point::new(8000.0, 0.0), Point::new(8000.0, 6000.0), Point::new(0.0, 6000.0)]).unwrap();
        let s = d.add_section(lvl, Point::new(-1000.0, 3000.0), Point::new(9000.0, 3000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let data = d.elevation_data(v).unwrap();
        let slab: Vec<_> = data.items.iter().filter(|i| i.cut && i.category == "Floors").collect();
        assert!(!slab.is_empty());
        let thick: f64 = slab.iter().map(|i| i.polys[0][3][1] - i.polys[0][0][1]).sum();
        let want = d.floor_types()[0].1.thickness_mm();
        assert!((thick - want).abs() < 1.0, "{thick} vs {want}");
        // the slab runs from x=0 to 8000, which is u 1000 to 9000
        let u0 = slab.iter().map(|i| i.polys[0][0][0]).fold(f64::INFINITY, f64::min);
        let u1 = slab.iter().map(|i| i.polys[0][1][0]).fold(f64::NEG_INFINITY, f64::max);
        assert!((u0 - 1000.0).abs() < 1.0 && (u1 - 9000.0).abs() < 1.0, "{u0} {u1}");
    }

    #[test]
    fn a_sloped_floor_shows_its_slope_in_elevation() {
        let (mut d, lvl) = house();
        let ft = d.floor_types()[0].0;
        let rect = crate::sketch::rectangle(Point::new(0.0, 0.0), Point::new(8000.0, 6000.0), 0.0);
        let slope = crate::element::SlopeArrow { tail: Point::new(0.0, 0.0), head: Point::new(8000.0, 0.0), spec: crate::element::SlopeSpec::Slope { tail_offset_mm: 0.0, percent: 5.0 }, tail_level: None, head_level: None };
        let fl = d.add_floor_sketch(ft, lvl, rect, Some(slope), None).unwrap();
        let view = d.views().iter().find(|(_, v)| v.view_kind == ViewKind::Elevation).map(|(i, _)| *i).expect("a default elevation");
        let data = d.elevation_data(view).unwrap();
        let items: Vec<_> = data.items.iter().filter(|i| i.id == fl).collect();
        assert!(!items.is_empty(), "the floor is in the elevation");
        let zs: Vec<f64> = items.iter().flat_map(|i| i.polys.iter().flat_map(|p| p.iter().map(|q| q[1]))).collect();
        let (lo, hi) = (zs.iter().cloned().fold(f64::INFINITY, f64::min), zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        let thick = d.floor_types()[0].1.thickness_mm();
        // 5 % over the 8000 mm run is 400 mm of rise on top of the slab thickness
        assert!((hi - lo - (400.0 + thick)).abs() < 1.0, "{lo} {hi} {thick}");
    }

    #[test]
    fn far_clip_limits_depth_and_draws_a_line() {
        let (mut d, lvl) = house();
        let s = d.add_section(lvl, Point::new(-1000.0, 3000.0), Point::new(9000.0, 3000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        // the north wall is 3000 beyond the plane: a clip at 1500 leaves it out; the side walls are cut by the far plane
        d.patch_section(s, &serde_json::json!({"far_clip_mm": 1500.0})).unwrap();
        let data = d.elevation_data(v).unwrap();
        assert!((data.frame.depth_mm - 1500.0).abs() < 1e-9);
        assert!(data.items.iter().filter(|i| !i.cut && !i.lines.is_empty()).count() >= 1, "line where the far clip passes through the side walls");
        d.patch_section(s, &serde_json::json!({"far_clip": "without_line"})).unwrap();
        let data = d.elevation_data(v).unwrap();
        assert!(data.items.iter().all(|i| i.lines.is_empty()));
        d.patch_section(s, &serde_json::json!({"far_clip": "no_clip"})).unwrap();
        let data = d.elevation_data(v).unwrap();
        assert!(data.frame.depth_mm >= 1.0e6);
        assert!(data.items.iter().any(|i| !i.cut && i.category == "Walls" && i.depth > 2000.0), "the north wall shows with no clip");
    }

    #[test]
    fn a_plane_on_a_wall_face_still_cuts_cleanly() {
        let (mut d, lvl) = house();
        // a section line exactly on the face of the east wall (x = 8000 is the wall's location line)
        let s = d.add_section(lvl, Point::new(8000.0, -1000.0), Point::new(8000.0, 7000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let data = d.elevation_data(v).unwrap();
        for i in &data.items {
            for p in &i.polys {
                assert!(p.iter().all(|q| q[0].is_finite() && q[1].is_finite()));
            }
        }
    }

    #[test]
    fn a_roof_is_cut_across_its_planes() {
        let (mut d, lvl) = house();
        let rt = d.roof_types()[0].0;
        let up = d.levels()[1].0;
        let c = [Point::new(0.0, 0.0), Point::new(8000.0, 0.0), Point::new(8000.0, 6000.0), Point::new(0.0, 6000.0)];
        let edges: Vec<crate::roof::RoofEdge> = (0..4).map(|i| crate::roof::RoofEdge::new(crate::curve::Curve::line(c[i], c[(i + 1) % 4]))).collect();
        d.add_roof_sketch(rt, up, edges).unwrap();
        let s = d.add_section(lvl, Point::new(-1000.0, 3000.0), Point::new(9000.0, 3000.0), false).unwrap();
        let v = d.section(s).unwrap().view.unwrap();
        let data = d.elevation_data(v).unwrap();
        let roof: Vec<_> = data.items.iter().filter(|i| i.cut && i.category == "Roofs").collect();
        assert!(!roof.is_empty(), "the roof is cut");
        let zs: Vec<f64> = roof.iter().flat_map(|i| i.polys[0].iter().map(|q| q[1])).collect();
        let (lo, hi) = (zs.iter().cloned().fold(f64::INFINITY, f64::min), zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        assert!(hi - lo > 500.0, "the profile rises to a ridge: {lo} {hi}");
    }

    #[test]
    fn a_section_marker_saved_with_the_old_arrow_is_turned_round_and_gets_a_name() {
        use crate::annotation::{AnnShape, AnnotationKind};
        let mut d = Document::new_project();
        let (id, f) = d.annotation_families().into_iter().find(|(_, f)| f.kind == AnnotationKind::SectionMarker).map(|(i, f)| (i, f.clone())).unwrap();
        let mut old = f.clone();
        old.pointer = vec![AnnShape::Polygon { pts: vec![[4.0, 0.0], [7.0, 2.0], [7.0, -2.0]], filled: true }];
        old.labels.remove(0);
        d.transact("old", |tx| tx.update(id, Element::AnnotationFamily(old))).unwrap();
        d.ensure_annotation_defaults();
        let n = d.annotation_family(id).unwrap();
        assert_eq!(n.pointer, f.pointer);
        assert!(n.labels.iter().any(|l| l.lines.iter().any(|x| x.param == "View Name")));
    }

    #[test]
    fn older_project_files_without_section_data_still_open() {
        let s: Section = serde_json::from_value(serde_json::json!({"level": 1, "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1000.0, "y": 0.0}})).unwrap();
        assert_eq!(s.far_clip, FarClip::WithLine);
        assert!(!s.depth_cue.enabled && s.bends.is_empty() && s.reference.is_none());
    }
}
