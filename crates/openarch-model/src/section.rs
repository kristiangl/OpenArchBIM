//! Section views: a section line drawn in plan makes a view that looks along the line's normal.
//!
//! The line is the cut plane. Walls, floors and roofs crossing it are sliced into closed profiles that
//! are drawn with the category's cut weight and the material's cut pattern; what lies between the plane
//! and the far clip is projected the way an elevation shows it (see `Document::elevation_data`).
//!
//! A line can be split into parallel runs joined by jogs square to them (Split Segment). Each run cuts at its own
//! depth and the view unrolls the runs side by side; they share one far clip plane. A reference section makes no
//! view of its own and points at an existing one. A break hides the middle of the line in one plan view; the cut
//! is still the whole line. Depth cueing is a setting of the view (Graphic Display Options), not of the section.

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

/// Shortest run of a jogged section line (mm).
pub const MIN_RUN_MM: f64 = 100.0;

/// Runs closer than this across the line are one run (mm): a run moved onto its neighbour's line merges with it.
pub const MERGE_MM: f64 = 1.0;

/// The shortest gap a break leaves in the line (mm).
pub const MIN_GAP_MM: f64 = 10.0;

/// A new break leaves this much of the line at each end, on paper (mm).
const BREAK_STUB_PAPER_MM: f64 = 12.0;

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

/// A break in the line in one plan view: the line shows `head_mm` from its head and `tail_mm` from its tail,
/// measured along the line, and nothing between. Only that view's drawing changes; the cut is the whole line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionBreak {
    pub view: Id,
    pub head_mm: f64,
    pub tail_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// The level whose plan it was drawn in.
    pub level: Id,
    /// The head of the line.
    pub start: Point,
    /// The tail of the line.
    pub end: Point,
    /// Looks to the other side of the line.
    #[serde(default)]
    pub flipped: bool,
    #[serde(default)]
    pub far_clip: FarClip,
    /// How deep the view looks, from the plane of the run at the head. `None` fits the model.
    #[serde(default)]
    pub far_clip_mm: Option<f64>,
    /// Bottom and top of the view (absolute mm). `None` fits the levels and the model.
    #[serde(default)]
    pub z_range: Option<(f64, f64)>,
    /// The view this section makes. A reference section has none.
    #[serde(default)]
    pub view: Option<Id>,
    /// The annotation family it is drawn with. `None` uses the first section marker family.
    #[serde(default)]
    pub family: Option<Id>,
    #[serde(default)]
    pub pinned: bool,
    /// The corners of a jogged line between the start and the end, in order along it: two for each jog (see `drawn`).
    #[serde(default)]
    pub bends: Vec<Point>,
    /// Breaks in the line, at most one for each plan view.
    #[serde(default)]
    pub breaks: Vec<SectionBreak>,
    /// Where the line jogs, the view draws a fold line.
    #[serde(default = "yes")]
    pub fold_lines: bool,
    /// A reference section points at this existing view (a section, callout or drafting view) instead of making its own.
    #[serde(default)]
    pub reference: Option<Id>,
}

fn yes() -> bool {
    true
}

/// The line in the axes it was drawn in: `t` along the run at the head, away from the head, and `n` to its left.
/// Each run is (s0, s1, d): from `s0` to `s1` along `t` and `d` across, all measured from the head.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub start: Point,
    pub t: Point,
    pub n: Point,
    pub runs: Vec<(f64, f64, f64)>,
}

impl Drawn {
    pub fn at(&self, s: f64, d: f64) -> Point {
        Point::new(self.start.x + self.t.x * s + self.n.x * d, self.start.y + self.t.y * s + self.n.y * d)
    }

    pub fn s(&self, p: Point) -> f64 {
        (p.x - self.start.x) * self.t.x + (p.y - self.start.y) * self.t.y
    }

    pub fn d(&self, p: Point) -> f64 {
        (p.x - self.start.x) * self.n.x + (p.y - self.start.y) * self.n.y
    }

    /// The corners of these runs from the head to the tail. Neighbouring runs that line up become one.
    pub fn points(&self, runs: &[(f64, f64, f64)]) -> Vec<Point> {
        let mut merged: Vec<(f64, f64, f64)> = vec![];
        for &r in runs {
            match merged.last_mut() {
                Some(m) if (m.2 - r.2).abs() < MERGE_MM => m.1 = r.1,
                _ => merged.push(r),
            }
        }
        merged.iter().flat_map(|&(s0, s1, d)| [self.at(s0, d), self.at(s1, d)]).collect()
    }
}

/// A run of the line in the view's axes: across the view from `u0` to `u1`, cutting `d` in front of the plane of
/// the run at the head.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Run {
    pub u0: f64,
    pub u1: f64,
    pub d: f64,
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
            breaks: vec![],
            fold_lines: true,
            reference: None,
        }
    }

    /// The head, the bends and the tail.
    pub fn points(&self) -> Vec<Point> {
        let mut p = vec![self.start];
        p.extend(&self.bends);
        p.push(self.end);
        p
    }

    fn with_points(&self, pts: &[Point]) -> Section {
        let mut s = self.clone();
        s.start = pts[0];
        s.end = pts[pts.len() - 1];
        s.bends = pts[1..pts.len() - 1].to_vec();
        s
    }

    /// The runs of the line, or `None` when its corners do not make straight runs joined by square jogs.
    pub fn drawn(&self) -> Option<Drawn> {
        let pts = self.points();
        if pts.len() % 2 != 0 {
            return None;
        }
        let l = pts[0].dist(&pts[1]);
        if l < 1e-6 {
            return None;
        }
        let t = Point::new((pts[1].x - pts[0].x) / l, (pts[1].y - pts[0].y) / l);
        let mut dr = Drawn { start: pts[0], t, n: Point::new(-t.y, t.x), runs: vec![] };
        for k in 0..pts.len() / 2 {
            let (a, b) = (pts[2 * k], pts[2 * k + 1]);
            let (s0, s1, d0) = (dr.s(a), dr.s(b), dr.d(a));
            if s1 - s0 < 1.0 || (dr.d(b) - d0).abs() > 0.01 {
                return None;
            }
            if let Some(&(_, ps1, pd)) = dr.runs.last() {
                // a jog runs square across the line and goes somewhere
                if (s0 - ps1).abs() > 0.01 || (d0 - pd).abs() < MERGE_MM {
                    return None;
                }
            }
            dr.runs.push((s0, s1, d0));
        }
        Some(dr)
    }

    /// The width of the view: the runs end to end.
    pub fn length(&self) -> f64 {
        self.drawn().map_or_else(|| self.start.dist(&self.end), |d| d.runs.iter().map(|r| r.1 - r.0).sum())
    }

    /// The length along the line, jogs included: what a break measures.
    pub fn path_length(&self) -> f64 {
        self.points().windows(2).map(|w| w[0].dist(&w[1])).sum()
    }

    /// Unit vectors (right on screen, look) of the view, `None` when the line is not valid.
    pub fn axes(&self) -> Option<(Point, Point)> {
        let dr = self.drawn()?;
        Some(if self.flipped { (Point::new(-dr.t.x, -dr.t.y), Point::new(-dr.n.x, -dr.n.y)) } else { (dr.t, dr.n) })
    }

    /// Where the view starts (the left end of the view, on the plane of the run at the head) and the runs
    /// across it, left to right.
    pub fn view_runs(&self) -> Option<(Point, Vec<Run>)> {
        let dr = self.drawn()?;
        let (right, _) = self.axes()?;
        let left = if self.flipped { dr.runs.last()?.1 } else { dr.runs[0].0 };
        let o = dr.at(left, 0.0);
        let u = |p: Point| (p.x - o.x) * right.x + (p.y - o.y) * right.y;
        let sign = if self.flipped { -1.0 } else { 1.0 };
        let mut runs: Vec<Run> = dr
            .runs
            .iter()
            .map(|&(s0, s1, d)| {
                let (a, b) = (u(dr.at(s0, d)), u(dr.at(s1, d)));
                Run { u0: a.min(b), u1: a.max(b), d: d * sign }
            })
            .collect();
        runs.sort_by(|a, b| a.u0.total_cmp(&b.u0));
        Some((o, runs))
    }

    /// The break in this plan view, if the line is broken there.
    pub fn break_in(&self, view: Id) -> Option<&SectionBreak> {
        self.breaks.iter().find(|b| b.view == view)
    }

    /// Breaks that no longer leave a gap in a shorter line give up length at both ends alike.
    fn fit_breaks(&mut self) {
        let room = (self.path_length() - MIN_GAP_MM).max(0.0);
        for b in self.breaks.iter_mut() {
            let both = b.head_mm + b.tail_mm;
            if both > room && both > 0.0 {
                let k = room / both;
                b.head_mm *= k;
                b.tail_mm *= k;
            }
        }
    }
}

/// An edit of the runs and jogs of a section line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionEdit {
    /// Split the run under `at`; the part on the side of `to` moves across the line to `to` (Split Segment).
    Split { at: Point, to: Point },
    /// Move run `index` (counted from the head) across to `to`. On the line of a neighbour it merges with it.
    MoveRun { index: usize, to: Point },
    /// Slide jog `index` (counted from the head) along the line to `to`.
    MoveJog { index: usize, to: Point },
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

/// One run's cut plane through the model and the box in front of it, in the section's own axes: `u` across the
/// view, `d` the distance in front of the plane of the run at the head. A straight section is one slice.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slice {
    pub o: Point,
    pub right: Point,
    pub look: Point,
    /// The stretch across the view this run covers.
    pub lo: f64,
    pub hi: f64,
    /// Where this run's cut plane stands.
    pub plane: f64,
    /// The far clip plane, shared by every run.
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

    /// Keep the part of a polygon in plan between the cut plane and the far clip, and across this run.
    pub fn clip_ring(&self, ring: &[Point]) -> Vec<Point> {
        let mut r = clip_by(ring, |p| self.d(p), self.plane, true);
        if self.far != FarClip::NoClip {
            r = clip_by(&r, |p| self.d(p), self.depth, false);
        }
        let r = clip_by(&r, |p| self.u(p), self.lo, true);
        clip_by(&r, |p| self.u(p), self.hi, false)
    }

    /// The same for a polygon in three dimensions (a roof plane).
    pub fn clip_ring3(&self, poly: &[[f64; 3]]) -> Vec<[f64; 3]> {
        let d = |q: &[f64; 3]| self.d(Point::new(q[0], q[1]));
        let u = |q: &[f64; 3]| self.u(Point::new(q[0], q[1]));
        let mut r = clip3(poly, d, self.plane, true);
        if self.far != FarClip::NoClip {
            r = clip3(&r, d, self.depth, false);
        }
        let r = clip3(&r, u, self.lo, true);
        clip3(&r, u, self.hi, false)
    }

    /// Keep the part of a drawing polygon within this run.
    pub fn clip_width(&self, poly: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let r = clip2(poly, 0, self.lo, true);
        clip2(&r, 0, self.hi, false)
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
        if c.u1 <= self.lo || c.u0 >= self.hi {
            return None;
        }
        let lerp = |u: f64| {
            let t = (u - c.u0) / (c.u1 - c.u0);
            Point::new(c.p0.x + (c.p1.x - c.p0.x) * t, c.p0.y + (c.p1.y - c.p0.y) * t)
        };
        let (u0, u1) = (c.u0.max(self.lo), c.u1.min(self.hi));
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
    /// and its sheet. A reference section names the view it points at and shows the family's Reference Label.
    pub fn section_labels(&self, s: &Section) -> Vec<crate::annotation::DrawnText> {
        let Some((_, f)) = self.section_marker_family(s.family) else { return vec![] };
        let name = s.reference.or(s.view).and_then(|v| self.view(v)).map(|v| v.name.clone()).unwrap_or_default();
        let reference = if s.reference.is_some() { f.reference_label.clone() } else { String::new() };
        let value = |param: &str| match param {
            "View Name" => Some(name.clone()),
            "View Number" | "Sheet Number" => Some("-".to_string()),
            "Reference Label" => (!reference.is_empty()).then(|| reference.clone()),
            _ => None,
        };
        self.symbol_texts(f, &value, false)
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

    /// Draw a section line that points at an existing view (Reference Other View). It makes no view of its own.
    pub fn add_reference_section(&mut self, level: Id, start: Point, end: Point, flipped: bool, target: Id) -> Result<Id, String> {
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        if start.dist(&end) < MIN_LENGTH_MM {
            return Err("the section line is too short".into());
        }
        self.check_reference_target(target)?;
        self.transact("Create reference section", |tx| {
            let mut s = Section::new(level, start, end);
            s.flipped = flipped;
            s.reference = Some(target);
            Ok(tx.add(Element::Section(s)))
        })
    }

    /// The views a reference section can point at: section views (of sections that make their own), callouts and
    /// drafting views.
    pub fn reference_targets(&self) -> Vec<(Id, &View)> {
        self.views().into_iter().filter(|(id, _)| self.check_reference_target(*id).is_ok()).collect()
    }

    fn check_reference_target(&self, target: Id) -> Result<(), String> {
        let v = self.view(target).ok_or("the referenced view does not exist")?;
        let ok = match v.view_kind {
            ViewKind::Section => v.section.is_some(),
            ViewKind::Callout | ViewKind::Drafting => true,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err("a reference section points at a section, callout or drafting view".into())
        }
    }

    /// Everything a changed section must still be: a line of straight runs and square jogs, long enough, with its
    /// runs in front of the far clip, breaks that leave a gap, and a view it makes or one it points at.
    fn check_section(&self, old: &Section, new: &Section) -> Result<(), String> {
        let Some(dr) = new.drawn() else { return Err("a section line is straight runs joined by jogs square to them".into()) };
        if new.length() < MIN_LENGTH_MM {
            return Err("the section line is too short".into());
        }
        if dr.runs.len() > 1 && dr.runs.iter().any(|r| r.1 - r.0 < MIN_RUN_MM - 1e-6) {
            return Err(format!("each run of a jogged section is at least {MIN_RUN_MM:.0} mm long"));
        }
        if new.far_clip_mm.is_some_and(|v| !v.is_finite() || v < 100.0) {
            return Err("the far clip distance must be at least 100 mm".into());
        }
        let deepest = new.view_runs().map_or(0.0, |(_, r)| r.iter().map(|r| r.d).fold(0.0, f64::max));
        if new.far_clip != FarClip::NoClip && new.far_clip_mm.is_some_and(|v| v < deepest + 100.0) {
            return Err("part of the section line is beyond the far clip".into());
        }
        if new.z_range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite() || b - a < 100.0) {
            return Err("the section must be at least 100 mm tall".into());
        }
        if new.family.is_some_and(|f| self.annotation_family(f).is_none()) {
            return Err("unknown section marker".into());
        }
        let len = new.path_length();
        for (k, b) in new.breaks.iter().enumerate() {
            if !self.view(b.view).is_some_and(|v| v.view_kind.is_plan()) || new.breaks[..k].iter().any(|x| x.view == b.view) {
                return Err("a section line is broken once in a plan view".into());
            }
            if !b.head_mm.is_finite() || !b.tail_mm.is_finite() || b.head_mm < 0.0 || b.tail_mm < 0.0 {
                return Err("the ends of a broken section line cannot be negative".into());
            }
            if b.head_mm + b.tail_mm > len - MIN_GAP_MM + 1e-6 {
                return Err("the two ends of the broken line would meet; click the break control to join the line".into());
            }
        }
        match (old.reference, new.reference) {
            (None, Some(_)) => return Err("a section that makes its own view cannot become a reference section".into()),
            (Some(_), None) => return Err("a reference section points at a view".into()),
            (_, Some(r)) => self.check_reference_target(r)?,
            _ => {}
        }
        let geometry = |s: &Section| (s.start, s.end, s.bends.clone(), s.flipped);
        if old.pinned && new.pinned && geometry(old) != geometry(new) {
            return Err("the section is pinned; unpin it first".into());
        }
        Ok(())
    }

    /// Change a section with a JSON patch (ends, bends, flip, far clip, range, family, breaks, fold lines, the view
    /// a reference section points at, pin). Moving an end of a jogged line moves it along its own run. One undo step.
    pub fn patch_section(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.section(id).cloned() else { return Err("not a section".into()) };
        let mut patch = patch.clone();
        if let Some(o) = patch.as_object_mut() {
            o.remove("view");
            o.remove("level");
        }
        let mut new: Section = apply_patch(&old, &patch)?;
        if new == old {
            return Ok(());
        }
        if !old.bends.is_empty() && new.bends == old.bends && (new.start != old.start || new.end != old.end) {
            if let Some(dr) = old.drawn() {
                let (first, last) = (dr.runs[0], dr.runs[dr.runs.len() - 1]);
                if new.start != old.start {
                    new.start = dr.at(dr.s(new.start).min(first.1 - MIN_RUN_MM), first.2);
                }
                if new.end != old.end {
                    new.end = dr.at(dr.s(new.end).max(last.0 + MIN_RUN_MM), last.2);
                }
            }
        }
        new.fit_breaks();
        self.check_section(&old, &new)?;
        self.transact("Change section", |tx| tx.update(id, Element::Section(new)))
    }

    /// Split a run, move a run across or slide a jog along the line. With `preview` nothing changes; either way the
    /// corners of the line the edit gives come back.
    pub fn edit_section(&mut self, id: Id, edit: SectionEdit, preview: bool) -> Result<Vec<Point>, String> {
        let Some(old) = self.section(id).cloned() else { return Err("not a section".into()) };
        if old.pinned {
            return Err("the section is pinned; unpin it first".into());
        }
        let dr = old.drawn().ok_or("the section line is not valid")?;
        let mut runs = dr.runs.clone();
        let name = match edit {
            SectionEdit::Split { at, to } => {
                let sa = dr.s(at);
                let k = runs.iter().position(|r| sa > r.0 && sa < r.1).ok_or("click on the section line where it should split")?;
                let r = runs[k];
                if sa - r.0 < MIN_RUN_MM || r.1 - sa < MIN_RUN_MM {
                    return Err(format!("split at least {MIN_RUN_MM:.0} mm from the ends of a run"));
                }
                let off = dr.d(to) - r.2;
                if off.abs() < MERGE_MM {
                    return Err("move the cursor across the line to say how far the new run moves".into());
                }
                let before = dr.s(to) < sa;
                let (a, b) = if before { (off, 0.0) } else { (0.0, off) };
                runs.splice(k..=k, [(r.0, sa, r.2 + a), (sa, r.1, r.2 + b)]);
                "Split section"
            }
            SectionEdit::MoveRun { index, to } => {
                let r = runs.get_mut(index).ok_or("the section line has no such run")?;
                r.2 = dr.d(to);
                "Move section run"
            }
            SectionEdit::MoveJog { index, to } => {
                if index + 1 >= runs.len() {
                    return Err("the section line has no such jog".into());
                }
                let (lo, hi) = (runs[index].0 + MIN_RUN_MM, runs[index + 1].1 - MIN_RUN_MM);
                if lo > hi {
                    return Err("the runs either side are too short to move the jog".into());
                }
                let s = dr.s(to).clamp(lo, hi);
                runs[index].1 = s;
                runs[index + 1].0 = s;
                "Move section jog"
            }
        };
        let pts = dr.points(&runs);
        if preview {
            return Ok(pts);
        }
        let mut new = old.with_points(&pts);
        if new == old {
            return Ok(pts);
        }
        new.fit_breaks();
        self.check_section(&old, &new)?;
        self.transact(name, |tx| tx.update(id, Element::Section(new)))?;
        Ok(pts)
    }

    /// Break the line in a plan view, or join it again (the break control). A new break leaves a short stretch of
    /// line at each end.
    pub fn toggle_section_break(&mut self, id: Id, view: Id) -> Result<bool, String> {
        let Some(old) = self.section(id).cloned() else { return Err("not a section".into()) };
        let Some(v) = self.view(view).filter(|v| v.view_kind.is_plan()) else { return Err("a section line is broken in a plan view".into()) };
        let mut new = old.clone();
        let broken = match new.breaks.iter().position(|b| b.view == view) {
            Some(k) => {
                new.breaks.remove(k);
                false
            }
            None => {
                let stub = (old.path_length() * 0.25).min(BREAK_STUB_PAPER_MM * v.settings.scale as f64);
                new.breaks.push(SectionBreak { view, head_mm: stub, tail_mm: stub });
                true
            }
        };
        self.check_section(&old, &new)?;
        self.transact(if broken { "Break section line" } else { "Join section line" }, |tx| tx.update(id, Element::Section(new)))?;
        Ok(broken)
    }

    /// How much of a broken line shows at its head and at its tail in a plan view.
    pub fn set_section_break(&mut self, id: Id, view: Id, head_mm: f64, tail_mm: f64) -> Result<(), String> {
        let Some(old) = self.section(id).cloned() else { return Err("not a section".into()) };
        let mut new = old.clone();
        let Some(b) = new.breaks.iter_mut().find(|b| b.view == view) else { return Err("the section line is not broken in this view".into()) };
        b.head_mm = head_mm;
        b.tail_mm = tail_mm;
        if new == old {
            return Ok(());
        }
        self.check_section(&old, &new)?;
        self.transact("Change section break", |tx| tx.update(id, Element::Section(new)))
    }

    /// Delete a section with its view, and the reference sections that point at that view.
    pub fn delete_section(&mut self, id: Id) -> Result<(), String> {
        let Some(s) = self.section(id).cloned() else { return Err("not a section".into()) };
        if s.pinned {
            return Err("the section is pinned; unpin it first".into());
        }
        let refs: Vec<Id> = match s.view {
            Some(v) => self.sections().iter().filter(|(_, x)| x.reference == Some(v)).map(|(i, _)| *i).collect(),
            None => vec![],
        };
        self.transact("Delete section", |tx| {
            if let Some(v) = s.view {
                for i in tx.doc_ref().view_details(v) {
                    tx.remove(i)?;
                }
                tx.remove(v)?;
            }
            for r in &refs {
                tx.remove(*r)?;
            }
            tx.remove(id)
        })
    }

    /// The section a view shows.
    pub fn section_of_view(&self, view: Id) -> Option<(Id, &Section)> {
        let sid = self.view(view)?.section?;
        self.section(sid).map(|s| (sid, s))
    }

    /// How deep the model reaches beyond a section's head plane: the far clip when none is set, and what depth
    /// cueing measures against when the view does not clip.
    pub(crate) fn section_fit_depth(&self, s: &Section) -> f64 {
        let (Some((_, look)), Some((o, runs))) = (s.axes(), s.view_runs()) else { return 10000.0 };
        let deepest = runs.iter().map(|r| r.d).fold(0.0, f64::max);
        match self.model_bounds() {
            Some((lo, hi)) => {
                let far = [Point::new(lo.x, lo.y), Point::new(hi.x, lo.y), Point::new(hi.x, hi.y), Point::new(lo.x, hi.y)]
                    .iter()
                    .map(|p| (p.x - o.x) * look.x + (p.y - o.y) * look.y)
                    .fold(f64::NEG_INFINITY, f64::max);
                (far + 1000.0).max(deepest + 1000.0).max(2000.0)
            }
            None => (deepest + 1000.0).max(10000.0),
        }
    }

    /// The slices of a section view, one for each run of its line, left to right.
    pub(crate) fn section_slices(&self, view: Id) -> Option<Vec<Slice>> {
        let (_, s) = self.section_of_view(view)?;
        let (right, look) = s.axes()?;
        let (o, runs) = s.view_runs()?;
        let depth = match s.far_clip {
            FarClip::NoClip => 1.0e7,
            _ => s.far_clip_mm.unwrap_or_else(|| self.section_fit_depth(s)),
        };
        Some(runs.iter().map(|r| Slice { o, right, look, lo: r.u0, hi: r.u1, plane: r.d, depth, far: s.far_clip }).collect())
    }

    /// Frame of a section view: the runs of the line side by side are the width, the levels and model the height.
    pub fn section_frame(&self, view: Id) -> Option<ElevFrame> {
        let slices = self.section_slices(view)?;
        let (_, s) = self.section_of_view(view)?;
        let (first, last) = (slices.first()?, slices.last()?);
        let (z0, z1) = s.z_range.unwrap_or_else(|| {
            let (a, b) = self.model_z_extent();
            (a - 1500.0, b + 1500.0)
        });
        Some(ElevFrame { origin: [first.o.x, first.o.y], look: [first.look.x, first.look.y], right: [first.right.x, first.right.y], left_mm: 0.0, right_mm: last.hi, depth_mm: first.depth, z_min: z0, z_max: z1 })
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
                for cut in sl.cuts(&[&ring], sl.plane + PLANE_EPS) {
                    openings.push((cut.u0, cut.u1, f.z0, f.z1));
                }
            }
            // an edited profile: where the plane crosses the wall, what the outline reaches; where it runs along the
            // wall, the outline over that stretch
            let profile = if w.curve.is_none() { self.wall_profile_shapes(w) } else { None };
            let wl = w.start.dist(&w.end).max(1e-9);
            let along = |p: Point| ((p.x - w.start.x) * (w.end.x - w.start.x) + (p.y - w.start.y) * (w.end.y - w.start.y)) / wl;
            let push_cuts = |out: &mut Vec<ElevItem>, rings: &[&[Point]], material: Option<&str>, fill: [u8; 3], depth: f64| {
                for cut in sl.cuts(rings, sl.plane + PLANE_EPS) {
                    if let Some(shapes) = &profile {
                        let (a0, a1) = (along(cut.p0), along(cut.p1));
                        let item = |polys: Vec<Vec<[f64; 2]>>, holes: Vec<Vec<[f64; 2]>>| ElevItem { id, category: "Walls", depth, polys, fill, cut: true, material: material.map(str::to_string), holes, ..Default::default() };
                        if (a1 - a0).abs() < 1.0 {
                            for (h0, h1) in self.wall_profile_column(w, (a0 + a1) / 2.0).unwrap_or_default() {
                                // the doors and windows the plane passes through, inside this piece
                                let holes = openings
                                    .iter()
                                    .filter_map(|&(a, b, v0, v1)| {
                                        let (u0, u1, v0, v1) = (a.max(cut.u0), b.min(cut.u1), v0.max(h0), v1.min(h1));
                                        (u1 - u0 > 0.5 && v1 - v0 > 0.5).then(|| rect(u0, u1, v0, v1))
                                    })
                                    .collect();
                                out.push(item(vec![rect(cut.u0, cut.u1, h0, h1)], holes));
                            }
                        } else {
                            let to_u = |x: f64| cut.u0 + (x - a0) / (a1 - a0) * (cut.u1 - cut.u0);
                            let view = |r: &Vec<Point>| r.iter().map(|p| [to_u(p.x), z0 + p.y]).collect::<Vec<[f64; 2]>>();
                            for s in shapes {
                                let rs: Vec<Vec<Point>> = s.iter().map(|r| crate::site::clip_x(r, a0.min(a1), a0.max(a1))).filter(|r| r.len() >= 3).collect();
                                if let Some(outer) = rs.first() {
                                    out.push(item(vec![view(outer)], rs[1..].iter().map(view).collect()));
                                }
                            }
                        }
                        continue;
                    }
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
            for cut in sl.cuts(&rings, sl.plane + PLANE_EPS) {
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
                    let (da, db) = (sl.d(Point::new(a[0], a[1])) - sl.plane - PLANE_EPS, sl.d(Point::new(b[0], b[1])) - sl.plane - PLANE_EPS);
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
                    if u1 - u0 < 1e-6 || u1 <= sl.lo || u0 >= sl.hi {
                        continue;
                    }
                    let lerp = |u: f64| z0 + (z1 - z0) * (u - u0) / (u1 - u0);
                    let (a, b) = (u0.max(sl.lo), u1.min(sl.hi));
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
                for cut in sl.cuts(&rings, sl.plane + PLANE_EPS) {
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
        // as saved then: shapes rather than lines and regions
        old.lines.clear();
        old.regions.clear();
        old.old_shapes = vec![AnnShape::Circle { c: [0.0, 0.0], r: 4.0, filled: false }, AnnShape::Line { a: [-4.0, 0.0], b: [4.0, 0.0] }];
        old.old_pointer = vec![AnnShape::Polygon { pts: vec![[4.0, 0.0], [7.0, 2.0], [7.0, -2.0]], filled: true }];
        old.labels.remove(0);
        d.transact("old", |tx| tx.update(id, Element::AnnotationFamily(old))).unwrap();
        d.ensure_annotation_defaults();
        let n = d.annotation_family(id).unwrap();
        assert_eq!((&n.lines, &n.regions), (&f.lines, &f.regions));
        assert!(n.labels.iter().any(|l| l.params.iter().any(|x| x.param == "View Name")));
    }

    #[test]
    fn older_project_files_without_section_data_still_open() {
        let s: Section = serde_json::from_value(serde_json::json!({"level": 1, "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1000.0, "y": 0.0}})).unwrap();
        assert_eq!(s.far_clip, FarClip::WithLine);
        assert!(s.bends.is_empty() && s.breaks.is_empty() && s.reference.is_none() && s.fold_lines);
        // files from step 1 carry a centre break flag and depth cue settings on the section; they still open
        let old = serde_json::json!({"level": 1, "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1000.0, "y": 0.0}, "centre_break": false,
            "depth_cue": {"enabled": false, "start_pct": 20.0, "end_pct": 100.0, "min_intensity": 0.3}});
        let s: Section = serde_json::from_value(old).unwrap();
        assert_eq!(s.length(), 1000.0);
    }

    /// The house with an internal wall that stops 1000 mm short of a section through the middle, west to east,
    /// looking north. The straight section does not see it (it is behind the line).
    fn house_with_stub() -> (Document, Id, Id, Id) {
        let (mut d, lvl) = house();
        let wt = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let stub = d.add_wall(wt, lvl, Point::new(6000.0, 0.0), Point::new(6000.0, 2000.0), 2700.0).unwrap();
        let s = d.add_section(lvl, Point::new(-1000.0, 3000.0), Point::new(9000.0, 3000.0), false).unwrap();
        (d, lvl, s, stub)
    }

    #[test]
    fn split_segment_jogs_the_line_and_the_moved_run_cuts_at_its_own_depth() {
        let (mut d, _, s, stub) = house_with_stub();
        let v = d.section(s).unwrap().view.unwrap();
        assert!(!d.elevation_data(v).unwrap().items.iter().any(|i| i.id == stub), "the stub wall is behind the straight line");
        // split at x = 4000; the cursor goes south of the line on the east side, so the east part moves 2000 south
        let pts = d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 1000.0) }, true).unwrap();
        assert_eq!(pts.len(), 4);
        assert!(d.section(s).unwrap().bends.is_empty(), "a preview changes nothing");
        d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 1000.0) }, false).unwrap();
        let sec = d.section(s).unwrap().clone();
        assert_eq!(sec.bends, vec![Point::new(4000.0, 3000.0), Point::new(4000.0, 1000.0)]);
        assert_eq!(sec.end, Point::new(9000.0, 1000.0));
        assert!((sec.length() - 10000.0).abs() < 1e-6, "the width is the runs end to end");
        let (_, runs) = sec.view_runs().unwrap();
        assert_eq!(runs.len(), 2);
        assert!((runs[1].d + 2000.0).abs() < 1e-6 && (runs[1].u0 - 5000.0).abs() < 1e-6);
        let data = d.elevation_data(v).unwrap();
        assert_eq!(data.folds, vec![5000.0], "a fold line where the line jogs");
        // the stub wall (x = 6000, u = 7000) is now cut by the east run
        let cut: Vec<_> = data.items.iter().filter(|i| i.id == stub && i.cut).collect();
        assert!(!cut.is_empty(), "the moved run cuts the stub wall");
        assert!(cut.iter().all(|i| i.polys[0].iter().all(|q| q[0] > 6800.0 && q[0] < 7200.0)));
        // the north wall lies beyond both runs: its pieces join again across the jog
        let north = d.iter().find_map(|(id, e)| if let Element::Wall(w) = e { (w.start.y == 6000.0 && w.end.y == 6000.0).then_some(id) } else { None }).unwrap();
        assert!(data.items.iter().any(|i| i.id == north && !i.cut && i.polys.iter().any(|p| p.iter().any(|q| q[0] < 4000.0) && p.iter().any(|q| q[0] > 6000.0))), "one face across the fold");
        // without fold lines the view draws none
        d.patch_section(s, &serde_json::json!({"fold_lines": false})).unwrap();
        assert!(d.elevation_data(v).unwrap().folds.is_empty());
        d.undo();
        d.undo();
        assert!(d.section(s).unwrap().bends.is_empty(), "the split is one undo step");
    }

    #[test]
    fn runs_move_across_and_merge_and_jogs_slide_along() {
        let (mut d, _, s, _) = house_with_stub();
        d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 1000.0) }, false).unwrap();
        // slide the jog east; it stops 100 mm short of the east end
        d.edit_section(s, SectionEdit::MoveJog { index: 0, to: Point::new(20000.0, 2000.0) }, false).unwrap();
        assert_eq!(d.section(s).unwrap().bends[0], Point::new(8900.0, 3000.0));
        // move the east run further south, then back onto the line of the west run: the jog goes
        d.edit_section(s, SectionEdit::MoveRun { index: 1, to: Point::new(8950.0, 500.0) }, false).unwrap();
        assert_eq!(d.section(s).unwrap().end, Point::new(9000.0, 500.0));
        d.edit_section(s, SectionEdit::MoveRun { index: 1, to: Point::new(8950.0, 3000.4) }, false).unwrap();
        let sec = d.section(s).unwrap();
        assert!(sec.bends.is_empty() && sec.end == Point::new(9000.0, 3000.0), "{sec:?}");
        // a split too near the end of a run, or with the cursor on the line, is refused
        assert!(d.edit_section(s, SectionEdit::Split { at: Point::new(8950.0, 3000.0), to: Point::new(8950.0, 2000.0) }, false).is_err());
        assert!(d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(5000.0, 3000.0) }, false).is_err());
    }

    #[test]
    fn the_ends_of_a_jogged_line_move_along_their_own_runs() {
        let (mut d, _, s, _) = house_with_stub();
        d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 1000.0) }, false).unwrap();
        // dragging the head off its line keeps it on the line of the head run
        d.patch_section(s, &serde_json::json!({"start": {"x": -2000.0, "y": 3500.0}})).unwrap();
        assert_eq!(d.section(s).unwrap().start, Point::new(-2000.0, 3000.0));
        // and the tail stays on the line of its run, at least 100 mm long
        d.patch_section(s, &serde_json::json!({"end": {"x": 3000.0, "y": 0.0}})).unwrap();
        assert_eq!(d.section(s).unwrap().end, Point::new(4100.0, 1000.0));
        // a bend that breaks the square jogs is refused
        assert!(d.patch_section(s, &serde_json::json!({"bends": [{"x": 4000.0, "y": 3000.0}, {"x": 4500.0, "y": 1000.0}]})).is_err());
    }

    #[test]
    fn a_flipped_jogged_section_unrolls_left_to_right_from_the_tail() {
        let (mut d, _, s, stub) = house_with_stub();
        // move the east part north instead, then look south
        d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 5000.0) }, false).unwrap();
        d.patch_section(s, &serde_json::json!({"flipped": true})).unwrap();
        let sec = d.section(s).unwrap().clone();
        let (o, runs) = sec.view_runs().unwrap();
        assert_eq!(o, Point::new(9000.0, 3000.0), "the view starts at the tail end, on the head run's plane");
        assert!(runs[0].u0 == 0.0 && (runs[0].d + 2000.0).abs() < 1e-6, "{runs:?}");
        assert!((runs[1].u0 - 5000.0).abs() < 1e-6 && runs[1].d == 0.0);
        let v = sec.view.unwrap();
        let f = d.elevation_frame(v).unwrap();
        assert!((f.look[1] + 1.0).abs() < 1e-9 && (f.right_mm - 10000.0).abs() < 1e-6);
        // looking south from y = 5000 on the east run, the stub (y 0 to 2000) is beyond the plane and in projection
        let data = d.elevation_data(v).unwrap();
        assert!(data.items.iter().any(|i| i.id == stub && !i.cut));
    }

    #[test]
    fn a_run_cannot_go_beyond_the_far_clip() {
        let (mut d, _, s, _) = house_with_stub();
        d.patch_section(s, &serde_json::json!({"far_clip_mm": 1500.0})).unwrap();
        assert!(d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 4500.0) }, false).is_err());
        assert!(d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 3500.0) }, false).is_ok());
    }

    #[test]
    fn a_pinned_section_cannot_be_split() {
        let (mut d, _, s, _) = house_with_stub();
        d.patch_section(s, &serde_json::json!({"pinned": true})).unwrap();
        assert!(d.edit_section(s, SectionEdit::Split { at: Point::new(4000.0, 3000.0), to: Point::new(6500.0, 1000.0) }, false).is_err());
    }

    #[test]
    fn a_break_hides_the_middle_of_the_line_in_one_plan_view() {
        let (mut d, lvl, s, _) = house_with_stub();
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan && v.level == Some(lvl)).unwrap().0;
        assert!(d.toggle_section_break(s, plan).unwrap());
        let b = d.section(s).unwrap().break_in(plan).unwrap().clone();
        // 12 mm on paper at 1:100 is 1200 mm, under a quarter of the 10 m line
        assert!((b.head_mm - 1200.0).abs() < 1e-6 && (b.tail_mm - 1200.0).abs() < 1e-6);
        d.set_section_break(s, plan, 3000.0, 2000.0).unwrap();
        assert!(d.set_section_break(s, plan, 6000.0, 4000.0).is_err(), "the ends would meet");
        // a shorter line keeps a gap: both ends give up length
        d.patch_section(s, &serde_json::json!({"end": {"x": 3000.0, "y": 3000.0}})).unwrap();
        let b = d.section(s).unwrap().break_in(plan).unwrap().clone();
        assert!(b.head_mm + b.tail_mm <= 4000.0 - MIN_GAP_MM + 1e-6 && (b.head_mm / b.tail_mm - 1.5).abs() < 1e-9);
        // breaks are made in plan views, and the break control joins the line again
        let v = d.section(s).unwrap().view.unwrap();
        assert!(d.toggle_section_break(s, v).is_err());
        assert!(!d.toggle_section_break(s, plan).unwrap());
        assert!(d.section(s).unwrap().breaks.is_empty());
    }

    #[test]
    fn a_reference_section_points_at_an_existing_view_and_goes_with_it() {
        let (mut d, lvl, s, _) = house_with_stub();
        let target = d.section(s).unwrap().view.unwrap();
        let n = d.views().len();
        let r = d.add_reference_section(lvl, Point::new(2000.0, -1000.0), Point::new(2000.0, 7000.0), false, target).unwrap();
        assert_eq!(d.views().len(), n, "a reference section makes no view");
        let rs = d.section(r).unwrap().clone();
        assert!(rs.view.is_none() && rs.reference == Some(target));
        let labels: Vec<String> = d.section_labels(&rs).iter().flat_map(|l| l.lines.concat()).collect();
        assert!(labels.contains(&"Section 1".to_string()), "{labels:?}");
        // it cannot point at a plan, cannot stop being a reference, and a section with a view cannot become one
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan).unwrap().0;
        assert!(d.add_reference_section(lvl, Point::new(0.0, 0.0), Point::new(1000.0, 0.0), false, plan).is_err());
        assert!(d.patch_section(r, &serde_json::json!({"reference": null})).is_err());
        assert!(d.patch_section(s, &serde_json::json!({"reference": target})).is_err());
        // a second section to point at instead
        let s2 = d.add_section(lvl, Point::new(0.0, 1000.0), Point::new(8000.0, 1000.0), false).unwrap();
        let t2 = d.section(s2).unwrap().view.unwrap();
        d.patch_section(r, &serde_json::json!({"reference": t2})).unwrap();
        assert!(d.reference_targets().iter().any(|(i, _)| *i == t2));
        // deleting the section it points at deletes the reference section too, in one undo step
        d.delete_section(s2).unwrap();
        assert!(d.section(r).is_none());
        d.undo();
        assert!(d.section(r).is_some() && d.section(s2).is_some());
        // the reference section's own delete leaves the view it points at
        d.delete_section(r).unwrap();
        assert!(d.view(t2).is_some());
    }

    #[test]
    fn the_reference_label_shows_on_reference_sections_only() {
        use crate::annotation::{AnnLabel, HAlign, LabelParam, VAlign};
        let (mut d, lvl, s, _) = house_with_stub();
        let (fid, mut f) = d.section_marker_family(None).map(|(i, f)| (i, f.clone())).unwrap();
        f.labels.push(AnnLabel::new(0.0, -5.0, HAlign::Center, VAlign::Top, "2.5mm", 10.0, vec![LabelParam::new("Reference Label", "", "", "SIM", false)]));
        d.set_annotation_family(fid, f).unwrap();
        let target = d.section(s).unwrap().view.unwrap();
        let r = d.add_reference_section(lvl, Point::new(2000.0, -1000.0), Point::new(2000.0, 7000.0), false, target).unwrap();
        let text = |id: Id| -> Vec<String> { d.section_labels(d.section(id).unwrap()).iter().flat_map(|l| l.lines.concat()).collect() };
        assert!(text(r).contains(&"SIM".to_string()));
        assert!(!text(s).contains(&"SIM".to_string()));
    }

    #[test]
    fn depth_cueing_fades_what_is_far_from_the_plane() {
        let (mut d, _, s, _) = house_with_stub();
        let v = d.section(s).unwrap().view.unwrap();
        assert!(d.elevation_data(v).unwrap().items.iter().all(|i| i.fade == 1.0), "off by default");
        let mut view = d.view(v).unwrap().clone();
        view.settings.graphic_display.depth_cue = true;
        view.settings.graphic_display.depth_cue_start = 0;
        view.settings.graphic_display.depth_cue_end = 100;
        view.settings.graphic_display.depth_cue_fade = 30;
        view.settings.discipline = crate::view::Discipline::Architectural;
        d.transact("cue", |tx| tx.update(v, Element::View(view.clone()))).unwrap();
        let data = d.elevation_data(v).unwrap();
        assert!(data.items.iter().filter(|i| i.cut).all(|i| i.fade == 1.0), "what the plane cuts is never faded");
        let seen: Vec<_> = data.items.iter().filter(|i| !i.cut && i.category == "Walls").collect();
        let (near, far) = seen.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), i| (a.min(i.depth), b.max(i.depth)));
        let fade_at = |dep: f64| seen.iter().find(|i| i.depth == dep).unwrap().fade;
        assert!(fade_at(far) < fade_at(near), "the far wall is fainter: {} vs {}", fade_at(far), fade_at(near));
        assert!(data.items.iter().all(|i| i.fade >= 0.3 - 1e-9), "never below the fade limit");
        // depth cueing belongs to architectural and coordination views
        view.settings.discipline = crate::view::Discipline::Structural;
        d.transact("cue", |tx| tx.update(v, Element::View(view))).unwrap();
        assert!(d.elevation_data(v).unwrap().items.iter().all(|i| i.fade == 1.0));
        // elevations fade too
        let ev = d.views().iter().find(|(_, v)| v.view_kind == ViewKind::Elevation).map(|(i, _)| *i).unwrap();
        let mut e = d.view(ev).unwrap().clone();
        e.settings.graphic_display.depth_cue = true;
        e.settings.graphic_display.depth_cue_fade = 20;
        e.settings.discipline = crate::view::Discipline::Coordination;
        d.transact("cue", |tx| tx.update(ev, Element::View(e))).unwrap();
        assert!(d.elevation_data(ev).unwrap().items.iter().any(|i| i.fade < 1.0));
    }
}
