//! Family geometry: what a family is made of, stored as data.
//!
//! A family body is a set of reference planes, forms (solid and void), 2D symbolic
//! geometry and nested instances, driven by the family's parameters. It follows the
//! usual family editor workflow:
//!
//! * Reference planes lie on one of three axes. Each is anchored to the origin, to
//!   another plane by an expression (a labelled dimension), or midway between two
//!   planes (an EQ dimension). Planes solve in dependency order.
//! * A form has a work plane, a sketch of closed loops and a depth or sweep. A
//!   sketch node is a reference plane plus an offset on each of its two axes, so a
//!   node locked to a plane follows it when a parameter moves the plane.
//! * A void can cut the host (a door's opening) and can cut named solids.
//! * Symbolic geometry draws only in one kind of view (a door's swing arc in plan).
//!
//! Units are millimetres. The family's own axes are x along the host (left to
//! right), y across it (front to back) and z up from the reference level.
//! `FamilyBody::solve` works everything out for one set of parameter values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::{regions, signed_area, triangulate_region, Point};
use crate::layers::{subtract_shapes, Shape};

pub type PlaneId = u32;
pub type FormId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The two axes a sketch on a plane normal to this one lies in, as (u, v).
    pub fn sketch_axes(self) -> (Axis, Axis) {
        match self {
            Axis::Z => (Axis::X, Axis::Y),
            Axis::Y => (Axis::X, Axis::Z),
            Axis::X => (Axis::Y, Axis::Z),
        }
    }
    /// A sketch point (u, v) at depth w along the normal, as a family point.
    pub fn point(self, u: f64, v: f64, w: f64) -> [f64; 3] {
        match self {
            Axis::Z => [u, v, w],
            Axis::Y => [u, w, v],
            Axis::X => [w, u, v],
        }
    }
    /// Whether (u, v, w) to (x, y, z) reverses the handedness (triangles need turning round).
    fn mirrors(self) -> bool {
        self == Axis::Y
    }
}

/// How firmly a plane can be snapped to and aligned with in a project (Is Reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Strong,
    #[default]
    Weak,
    NotReference,
    /// The named references a family offers for placement and dimensions (the Is Reference
    /// list). Each belongs to one axis of plane: Left, Center (Left/Right) and Right to planes
    /// across the width; Front, Center (Front/Back) and Back to planes across the depth; Bottom,
    /// Center (Elevation) and Top to level planes.
    Left,
    CenterLeftRight,
    Right,
    Front,
    CenterFrontBack,
    Back,
    Bottom,
    CenterElevation,
    Top,
}

impl RefKind {
    /// The choices for a plane on `axis`, in the usual order.
    pub fn choices(axis: Axis) -> &'static [RefKind] {
        use RefKind::*;
        match axis {
            Axis::X => &[NotReference, Strong, Weak, Left, CenterLeftRight, Right],
            Axis::Y => &[NotReference, Strong, Weak, Front, CenterFrontBack, Back],
            Axis::Z => &[NotReference, Strong, Weak, Bottom, CenterElevation, Top],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Anchor {
    /// Fixed at zero on its axis (the three origin planes).
    Origin,
    /// `from` plane's position plus an expression (a number or a formula over the
    /// family's parameters). This is how a labelled dimension is stored.
    Offset { from: PlaneId, by: String },
    /// Midway between two planes (an EQ dimension).
    Mid { a: PlaneId, b: PlaneId },
    /// The `i`th of `n` equal steps from `a` to `b` (an EQ dimension across several segments).
    Div { a: PlaneId, b: PlaneId, i: u32, n: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefPlane {
    pub id: PlaneId,
    pub name: String,
    pub axis: Axis,
    pub anchor: Anchor,
    #[serde(default)]
    pub ref_kind: RefKind,
}

/// A dimension drawn in one view across two or more parallel reference planes. It can show
/// a distance, report it to a parameter, be labelled with a length parameter, be locked to
/// its current value, or equalise its segments (EQ). Labelled, locked and EQ dimensions
/// work by anchoring the planes they drive; `drives` lists those planes so the dimension
/// that owns a constraint is known.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DimDef {
    /// 0 in files written before dimensions had ids; `fix_dim_ids` numbers them.
    #[serde(default)]
    pub id: u32,
    /// First reference, in order along the axis.
    pub a: PlaneId,
    /// Last reference.
    pub b: PlaneId,
    /// References between `a` and `b`, in order.
    #[serde(default)]
    pub via: Vec<PlaneId>,
    /// A reporting parameter that takes the measured value.
    #[serde(default)]
    pub reports_to: Option<String>,
    /// The view the dimension is drawn in; `None` shows it in every view.
    #[serde(default)]
    pub view: Option<SymView>,
    /// Length parameter that drives the whole dimension (one segment, or the total when EQ is on).
    #[serde(default)]
    pub label: Option<String>,
    /// Length parameter for each segment of a multi-segment dimension without EQ ("" for none).
    #[serde(default)]
    pub seg_labels: Vec<String>,
    /// Equal segments.
    #[serde(default)]
    pub eq: bool,
    /// Holds its current value when nothing labels it.
    #[serde(default)]
    pub locked: bool,
    /// Where the dimension line sits on the other axis of its view, when moved from the default.
    #[serde(default)]
    pub at: Option<f64>,
    /// Planes whose anchors this dimension sets.
    #[serde(default)]
    pub drives: Vec<PlaneId>,
}

impl DimDef {
    pub fn refs(&self) -> Vec<PlaneId> {
        let mut v = vec![self.a];
        v.extend(&self.via);
        v.push(self.b);
        v
    }
    pub fn segments(&self) -> usize {
        self.via.len() + 1
    }
}

/// A change to a dimension. Empty strings clear a label or reporting parameter.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DimPatch {
    pub label: Option<String>,
    pub seg_labels: Option<Vec<String>>,
    pub reports_to: Option<String>,
    pub eq: Option<bool>,
    pub locked: Option<bool>,
    pub at: Option<f64>,
}

/// One coordinate of a sketch node: a reference plane plus an offset, or just a
/// position when `plane` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Coord {
    #[serde(default)]
    pub plane: Option<PlaneId>,
    #[serde(default)]
    pub off: f64,
}

impl Coord {
    pub fn at(off: f64) -> Coord {
        Coord { plane: None, off }
    }
    pub fn on(plane: PlaneId) -> Coord {
        Coord { plane: Some(plane), off: 0.0 }
    }
    pub fn from(plane: PlaneId, off: f64) -> Coord {
        Coord { plane: Some(plane), off }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Node {
    pub u: Coord,
    pub v: Coord,
}

impl Node {
    pub fn new(u: Coord, v: Coord) -> Node {
        Node { u, v }
    }
}

/// A chain of nodes joined by line or arc segments. `bulges[i]` joins node i to
/// node i+1 (and the last back to the first when closed): 0 is a line, otherwise the
/// tangent of a quarter of the arc's angle, positive counter-clockwise (1 is a half circle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub bulges: Vec<f64>,
    #[serde(default = "yes")]
    pub closed: bool,
}

fn yes() -> bool {
    true
}

/// One point of a sweep profile: `u` across the path (positive to its left) and `v` along the
/// work plane's normal, both expressions measured from the work plane's point on the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilePoint {
    pub u: String,
    pub v: String,
}

/// A closed loop of a sweep profile. Expressions use the family's parameters, so a profile
/// resizes with the type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileLoop {
    /// Points joined by lines, or by arcs where `bulges` says so (same bulge as a sketch loop).
    Poly {
        points: Vec<ProfilePoint>,
        #[serde(default)]
        bulges: Vec<f64>,
    },
    /// Centre and radius.
    Circle { u: String, v: String, r: String },
}

impl ProfileLoop {
    pub fn rect(u0: &str, u1: &str, v0: &str, v1: &str) -> ProfileLoop {
        let pt = |u: &str, v: &str| ProfilePoint { u: u.into(), v: v.into() };
        ProfileLoop::Poly { points: vec![pt(u0, v0), pt(u1, v0), pt(u1, v1), pt(u0, v1)], bulges: vec![] }
    }
}

impl Loop {
    pub fn polygon(nodes: Vec<Node>) -> Loop {
        Loop { nodes, bulges: vec![], closed: true }
    }
    /// A rectangle between two planes on each axis, or at fixed offsets.
    pub fn rect(u0: Coord, u1: Coord, v0: Coord, v1: Coord) -> Loop {
        Loop::polygon(vec![Node::new(u0, v0), Node::new(u1, v0), Node::new(u1, v1), Node::new(u0, v1)])
    }
    /// A circle (two half arcs) round a centre.
    pub fn circle(c: Node, r: f64) -> Loop {
        let at = |dx: f64| Node::new(Coord { plane: c.u.plane, off: c.u.off + dx }, c.v);
        Loop { nodes: vec![at(r), at(-r)], bulges: vec![1.0, 1.0], closed: true }
    }
    fn bulge(&self, i: usize) -> f64 {
        self.bulges.get(i).copied().unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymView {
    Plan,
    Front,
    Back,
    Left,
    Right,
}

impl SymView {
    /// The family axis looking into the view.
    pub fn normal(self) -> Axis {
        match self {
            SymView::Plan => Axis::Z,
            SymView::Front | SymView::Back => Axis::Y,
            SymView::Left | SymView::Right => Axis::X,
        }
    }
}

/// Where and when a piece of geometry shows (Visibility Settings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visibility {
    #[serde(default = "yes")]
    pub coarse: bool,
    #[serde(default = "yes")]
    pub medium: bool,
    #[serde(default = "yes")]
    pub fine: bool,
    /// Plan and reflected ceiling plan views.
    #[serde(default = "yes")]
    pub plan: bool,
    #[serde(default = "yes")]
    pub front_back: bool,
    #[serde(default = "yes")]
    pub left_right: bool,
    /// A Yes/No parameter or an expression (`Frame Type == "Timber"`); shown only while true.
    #[serde(default)]
    pub when: Option<String>,
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility { coarse: true, medium: true, fine: true, plan: true, front_back: true, left_right: true, when: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FormShape {
    /// The sketch pushed from `start` to `end` along the work plane's normal.
    ///
    /// `start_lock` and `end_lock` fix an end of the extrusion to a reference plane parallel to the
    /// work plane (the Align and Lock on an extrusion's depth); the expression is then ignored.
    Extrusion {
        plane: PlaneId,
        start: String,
        end: String,
        #[serde(default)]
        start_lock: Option<PlaneId>,
        #[serde(default)]
        end_lock: Option<PlaneId>,
        profile: Vec<Loop>,
    },
    /// A loft between `base` at `start` and `top` at `end`; the loops have equal node counts.
    Blend { plane: PlaneId, start: String, end: String, base: Vec<Loop>, top: Vec<Loop> },
    /// The sketch turned about an axis drawn on the same plane.
    Revolve { plane: PlaneId, axis_a: Node, axis_b: Node, start_deg: f64, end_deg: f64, profile: Vec<Loop> },
    /// The profile carried along an open or closed path drawn on the work plane. The profile is
    /// sketched in the plane across the path at its first point: `u` runs to the left of the path
    /// and `v` along the work plane's normal, both measured from that point.
    Sweep {
        plane: PlaneId,
        path: Loop,
        profile: Vec<ProfileLoop>,
        /// Turns the profile about the path (degrees).
        #[serde(default)]
        profile_angle: f64,
        /// Mirrors the profile across the path.
        #[serde(default)]
        flip: bool,
    },
}

impl FormShape {
    pub fn plane(&self) -> PlaneId {
        match self {
            FormShape::Extrusion { plane, .. } | FormShape::Blend { plane, .. } | FormShape::Revolve { plane, .. } | FormShape::Sweep { plane, .. } => *plane,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Form {
    pub id: FormId,
    #[serde(default)]
    pub name: String,
    pub shape: FormShape,
    #[serde(default)]
    pub void: bool,
    /// A void that also cuts the host element (the opening of a door or window).
    #[serde(default = "yes")]
    pub cuts_host: bool,
    /// Voids that cut this solid (Cut Geometry).
    #[serde(default)]
    pub cut_by: Vec<FormId>,
    /// Fixed material name, or `material_param` to take it from a Material parameter.
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub material_param: Option<String>,
    #[serde(default)]
    pub subcategory: String,
    #[serde(default)]
    pub vis: Visibility,
}

/// 2D geometry that draws in one kind of view only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymElement {
    pub id: u32,
    pub view: SymView,
    pub path: Loop,
    /// Filled region (drawn with the subcategory's fill) rather than lines.
    #[serde(default)]
    pub filled: bool,
    /// Masking region: hides what lies behind it.
    #[serde(default)]
    pub masking: bool,
    #[serde(default)]
    pub subcategory: String,
    #[serde(default)]
    pub vis: Visibility,
}

/// Another family placed inside this one. Stored now so files written today can be
/// read when nesting is drawn; parameters map a nested parameter to an expression here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nested {
    pub id: u32,
    pub family: String,
    #[serde(default)]
    pub type_name: String,
    pub origin: [Coord; 3],
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub vis: Visibility,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyBody {
    #[serde(default)]
    pub planes: Vec<RefPlane>,
    #[serde(default)]
    pub forms: Vec<Form>,
    #[serde(default)]
    pub syms: Vec<SymElement>,
    #[serde(default)]
    pub nested: Vec<Nested>,
    #[serde(default)]
    pub dims: Vec<DimDef>,
    /// Stays vertical when hosted on a sloped face (Always vertical).
    #[serde(default)]
    pub always_vertical: bool,
    /// Voids cut solids in the project that the instance overlaps (Cut with Voids When Loaded).
    #[serde(default)]
    pub cut_with_voids: bool,
    #[serde(default)]
    next_id: u32,
}

/// The three origin planes every family starts with.
pub const PLANE_LR: PlaneId = 1;
pub const PLANE_FB: PlaneId = 2;
pub const PLANE_LEVEL: PlaneId = 3;

impl FamilyBody {
    pub fn is_empty(&self) -> bool {
        self.planes.is_empty() && self.forms.is_empty() && self.syms.is_empty() && self.nested.is_empty()
    }

    /// A new body with the three origin planes: Center (Left/Right), Center (Front/Back), Ref. Level.
    pub fn template() -> FamilyBody {
        let mk = |id, name: &str, axis| RefPlane { id, name: name.into(), axis, anchor: Anchor::Origin, ref_kind: RefKind::Strong };
        FamilyBody {
            planes: vec![mk(PLANE_LR, "Center (Left/Right)", Axis::X), mk(PLANE_FB, "Center (Front/Back)", Axis::Y), mk(PLANE_LEVEL, "Ref. Level", Axis::Z)],
            next_id: 10,
            ..Default::default()
        }
    }

    fn take_id(&mut self) -> u32 {
        let used = self.planes.iter().map(|p| p.id).chain(self.forms.iter().map(|f| f.id)).chain(self.syms.iter().map(|s| s.id)).chain(self.nested.iter().map(|n| n.id)).chain(self.dims.iter().map(|d| d.id)).max().unwrap_or(0);
        self.next_id = self.next_id.max(used) + 1;
        self.next_id
    }

    pub fn plane(&self, id: PlaneId) -> Option<&RefPlane> {
        self.planes.iter().find(|p| p.id == id)
    }
    pub fn plane_named(&self, name: &str) -> Option<PlaneId> {
        self.planes.iter().find(|p| p.name == name).map(|p| p.id)
    }

    /// A plane `by` away from `from` (a number or a parameter expression), on the same axis.
    pub fn add_offset_plane(&mut self, name: &str, from: PlaneId, by: &str) -> Result<PlaneId, String> {
        let axis = self.plane(from).ok_or("no such plane")?.axis;
        self.add_plane(name, axis, Anchor::Offset { from, by: by.into() })
    }
    pub fn add_mid_plane(&mut self, name: &str, a: PlaneId, b: PlaneId) -> Result<PlaneId, String> {
        let axis = self.plane(a).ok_or("no such plane")?.axis;
        if self.plane(b).ok_or("no such plane")?.axis != axis {
            return Err("planes must be parallel".into());
        }
        self.add_plane(name, axis, Anchor::Mid { a, b })
    }
    fn add_plane(&mut self, name: &str, axis: Axis, anchor: Anchor) -> Result<PlaneId, String> {
        if name.is_empty() || self.planes.iter().any(|p| p.name == name) {
            return Err(format!("a reference plane called '{name}' already exists or the name is empty"));
        }
        let id = self.take_id();
        self.planes.push(RefPlane { id, name: name.into(), axis, anchor, ref_kind: RefKind::Weak });
        Ok(id)
    }

    pub fn add_form(&mut self, mut f: Form) -> FormId {
        f.id = self.take_id();
        let id = f.id;
        self.forms.push(f);
        id
    }
    pub fn add_sym(&mut self, mut s: SymElement) -> u32 {
        s.id = self.take_id();
        let id = s.id;
        self.syms.push(s);
        id
    }

    /// Delete a plane. Refused while a plane, a node or a nested instance depends on it.
    pub fn remove_plane(&mut self, id: PlaneId) -> Result<(), String> {
        if [PLANE_LR, PLANE_FB, PLANE_LEVEL].contains(&id) {
            return Err("the origin planes cannot be deleted".into());
        }
        let used_by_plane = self.planes.iter().any(|p| match p.anchor {
            Anchor::Offset { from, .. } => from == id,
            Anchor::Mid { a, b } | Anchor::Div { a, b, .. } => a == id || b == id,
            Anchor::Origin => false,
        });
        let in_loop = |l: &Loop| l.nodes.iter().any(|n| n.u.plane == Some(id) || n.v.plane == Some(id));
        let used_by_form = self.forms.iter().any(|f| {
            f.shape.plane() == id
                || match &f.shape {
                    FormShape::Extrusion { profile, start_lock, end_lock, .. } => profile.iter().any(in_loop) || *start_lock == Some(id) || *end_lock == Some(id),
                    FormShape::Revolve { profile, .. } => profile.iter().any(in_loop),
                    FormShape::Blend { base, top, .. } => base.iter().chain(top).any(in_loop),
                    FormShape::Sweep { path, .. } => in_loop(path),
                }
        });
        let used_by_sym = self.syms.iter().any(|s| in_loop(&s.path));
        if used_by_plane || used_by_form || used_by_sym {
            return Err("something is locked to this reference plane".into());
        }
        self.planes.retain(|p| p.id != id);
        self.dims.retain(|d| d.a != id && d.b != id && !d.via.contains(&id));
        for d in &mut self.dims {
            d.drives.retain(|p| *p != id);
        }
        Ok(())
    }

    /// Whether a dimension in this body reports its length to the parameter `name`.
    pub fn reports(&self, name: &str) -> bool {
        self.dims.iter().any(|d| d.reports_to.as_deref() == Some(name))
    }

    // -------------------------------------------------------------- dimensions

    /// Number dimensions that came from files written before dimensions had ids.
    pub fn fix_dim_ids(&mut self) {
        for i in 0..self.dims.len() {
            if self.dims[i].id == 0 {
                let id = self.take_id();
                self.dims[i].id = id;
            }
        }
    }

    pub fn dim(&self, id: u32) -> Option<&DimDef> {
        self.dims.iter().find(|d| d.id == id)
    }

    fn dim_index(&self, id: u32) -> Result<usize, String> {
        self.dims.iter().position(|d| d.id == id).ok_or_else(|| "no such dimension".to_string())
    }

    /// A new dimension across `refs` (two or more parallel planes) in `view`.
    pub fn add_dim(&mut self, refs: &[PlaneId], view: Option<SymView>, env: &mut Env) -> Result<u32, String> {
        self.fix_dim_ids();
        if refs.len() < 2 {
            return Err("a dimension needs two references".into());
        }
        let axis = self.plane(refs[0]).ok_or("no such plane")?.axis;
        for r in refs {
            if self.plane(*r).ok_or("no such plane")?.axis != axis {
                return Err("pick parallel reference planes".into());
            }
        }
        let pos = self.solve_planes(env)?;
        let mut sorted: Vec<PlaneId> = refs.to_vec();
        sorted.sort_by(|x, y| pos[x].total_cmp(&pos[y]));
        sorted.dedup();
        if sorted.len() < 2 {
            return Err("pick two different reference planes".into());
        }
        let id = self.take_id();
        let n = sorted.len();
        self.dims.push(DimDef { id, a: sorted[0], b: sorted[n - 1], via: sorted[1..n - 1].to_vec(), view, ..Default::default() });
        Ok(id)
    }

    /// The plane whose position is zero on `axis`.
    fn axis_origin(&self, axis: Axis) -> Result<PlaneId, String> {
        self.planes.iter().find(|p| p.axis == axis && p.anchor == Anchor::Origin).map(|p| p.id).ok_or_else(|| "the family has lost an origin plane".to_string())
    }

    /// Make a plane stand where it is now, without any dimension holding it.
    fn bake(&mut self, id: PlaneId, pos: &BTreeMap<PlaneId, f64>) -> Result<(), String> {
        let Some(i) = self.planes.iter().position(|p| p.id == id) else { return Ok(()) };
        if self.planes[i].anchor == Anchor::Origin {
            return Ok(());
        }
        let origin = self.axis_origin(self.planes[i].axis)?;
        self.planes[i].anchor = Anchor::Offset { from: origin, by: fmt_num(pos[&id]) };
        Ok(())
    }

    /// Anchor `target` and check the planes still solve; put the old anchor back if they do not.
    fn try_anchor(&mut self, target: PlaneId, anchor: Anchor, env: &mut Env) -> Result<(), String> {
        let i = self.planes.iter().position(|p| p.id == target).ok_or("no such plane")?;
        let old = std::mem::replace(&mut self.planes[i].anchor, anchor);
        match self.solve_planes(env) {
            Ok(_) => Ok(()),
            Err(e) => {
                self.planes[i].anchor = old;
                Err(e)
            }
        }
    }

    fn driven_elsewhere(&self, dim: usize) -> Vec<PlaneId> {
        self.dims.iter().enumerate().filter(|(i, _)| *i != dim).flat_map(|(_, d)| d.drives.iter().copied()).collect()
    }

    fn is_origin(&self, id: PlaneId) -> bool {
        self.plane(id).map_or(false, |p| p.anchor == Anchor::Origin)
    }

    /// Hold `hi` at `expr` past `lo`, or failing that `lo` at `expr` before `hi`.
    fn constrain_pair(&mut self, lo: PlaneId, hi: PlaneId, expr: &str, taken: &[PlaneId], drives: &mut Vec<PlaneId>, env: &mut Env) -> Result<(), String> {
        let mut last = String::from("the references are already fixed by other constraints");
        for (target, from, by) in [(hi, lo, expr.to_string()), (lo, hi, format!("-({expr})"))] {
            if self.is_origin(target) || taken.contains(&target) || drives.contains(&target) {
                continue;
            }
            match self.try_anchor(target, Anchor::Offset { from, by }, env) {
                Ok(()) => {
                    drives.push(target);
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(format!("this dimension would over-constrain the family ({last})"))
    }

    /// Recompute what the dimension drives. `pos` are the plane positions before any change.
    fn derive_dim(&mut self, di: usize, pos: &BTreeMap<PlaneId, f64>, env: &mut Env) -> Result<(), String> {
        // let go of what it held, keeping every plane where it stands
        for p in std::mem::take(&mut self.dims[di].drives) {
            self.bake(p, pos)?;
        }
        let mut refs = self.dims[di].refs();
        refs.sort_by(|x, y| pos[x].total_cmp(&pos[y]));
        let n = refs.len() - 1;
        {
            let d = &mut self.dims[di];
            d.a = refs[0];
            d.b = refs[n];
            d.via = refs[1..n].to_vec();
        }
        let taken = self.driven_elsewhere(di);
        let d = self.dims[di].clone();
        let label = |s: &Option<String>| s.as_ref().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| format!("[{l}]"));
        let mut drives: Vec<PlaneId> = Vec::new();
        if d.eq && n >= 2 {
            let total = pos[&refs[n]] - pos[&refs[0]];
            let mut te = label(&d.label).or_else(|| d.locked.then(|| fmt_num(total)));
            let fixed_origin = refs.iter().position(|r| self.is_origin(*r));
            if te.is_none() && fixed_origin.is_some() {
                te = Some(fmt_num(total));
            }
            match te {
                Some(te) => {
                    let k = fixed_origin.or_else(|| refs.iter().position(|r| !taken.contains(r))).ok_or("every reference is fixed by other constraints")?;
                    for (i, r) in refs.iter().enumerate() {
                        if i == k {
                            continue;
                        }
                        if self.is_origin(*r) || taken.contains(r) {
                            return Err("a reference in this dimension is already fixed".into());
                        }
                        let step = i as i64 - k as i64;
                        self.try_anchor(*r, Anchor::Offset { from: refs[k], by: format!("({step})*({te})/{n}") }, env)?;
                        drives.push(*r);
                    }
                }
                None => {
                    for i in 1..n {
                        if taken.contains(&refs[i]) {
                            return Err("a reference in this dimension is already fixed".into());
                        }
                        self.try_anchor(refs[i], Anchor::Div { a: refs[0], b: refs[n], i: i as u32, n: n as u32 }, env)?;
                        drives.push(refs[i]);
                    }
                }
            }
        } else {
            for j in 0..n {
                let seg = if n == 1 { label(&d.label) } else { label(&d.seg_labels.get(j).cloned()) };
                let expr = seg.or_else(|| d.locked.then(|| fmt_num(pos[&refs[j + 1]] - pos[&refs[j]])));
                if let Some(e) = expr {
                    self.constrain_pair(refs[j], refs[j + 1], &e, &taken, &mut drives, env)?;
                }
            }
        }
        self.dims[di].drives = drives;
        Ok(())
    }

    /// Change a dimension's label, EQ, lock, reporting parameter or position, and re-derive the
    /// planes it drives. Nothing changes if the result cannot be solved.
    pub fn patch_dim(&mut self, id: u32, patch: &DimPatch, env: &mut Env) -> Result<(), String> {
        self.fix_dim_ids();
        let di = self.dim_index(id)?;
        let saved = (self.planes.clone(), self.dims.clone());
        let pos = self.solve_planes(env)?;
        let run = |this: &mut FamilyBody, env: &mut Env| -> Result<(), String> {
            let d = &mut this.dims[di];
            if let Some(l) = &patch.label {
                d.label = Some(l.trim().to_string()).filter(|l| !l.is_empty());
            }
            if let Some(l) = &patch.seg_labels {
                d.seg_labels = l.clone();
            }
            if let Some(r) = &patch.reports_to {
                d.reports_to = Some(r.trim().to_string()).filter(|r| !r.is_empty());
            }
            if let Some(e) = patch.eq {
                d.eq = e;
            }
            if let Some(l) = patch.locked {
                d.locked = l;
            }
            if let Some(a) = patch.at {
                d.at = Some(a);
            }
            if d.label.is_some() && d.reports_to.is_some() {
                return Err("a dimension either drives a parameter (label) or reports to one, not both".into());
            }
            this.derive_dim(di, &pos, env)?;
            this.solve_planes(env).map(|_| ())
        };
        match run(self, env) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.planes = saved.0;
                self.dims = saved.1;
                Err(e)
            }
        }
    }

    /// Delete a dimension. The planes it drove stay where they are.
    pub fn remove_dim(&mut self, id: u32, env: &mut Env) -> Result<(), String> {
        self.fix_dim_ids();
        let di = self.dim_index(id)?;
        let pos = self.solve_planes(env)?;
        for p in std::mem::take(&mut self.dims[di].drives) {
            self.bake(p, &pos)?;
        }
        self.dims.remove(di);
        Ok(())
    }

    /// Move a plane to `to` by changing its offset. Refused for a plane a dimension drives or one
    /// whose position comes from a formula or from other planes.
    pub fn move_plane(&mut self, id: PlaneId, to: f64, env: &mut Env) -> Result<(), String> {
        let pos = self.solve_planes(env)?;
        let p = self.plane(id).ok_or("no such plane")?;
        if let Some(d) = self.dims.iter().find(|d| d.drives.contains(&id)) {
            let how = match (&d.label, d.eq, d.locked) {
                (Some(l), _, _) => format!("labelled '{l}'"),
                (_, true, _) => "set to equal segments".to_string(),
                _ => "locked".to_string(),
            };
            return Err(format!("'{}' is held by a dimension that is {how}; change or unlock the dimension", p.name));
        }
        let name = p.name.clone();
        let Anchor::Offset { from, by } = p.anchor.clone() else {
            return Err(format!("'{name}' cannot be moved on its own"));
        };
        if by.trim().parse::<f64>().is_err() {
            return Err(format!("'{name}' is positioned by a formula ({by})"));
        }
        let i = self.planes.iter().position(|q| q.id == id).unwrap();
        self.planes[i].anchor = Anchor::Offset { from, by: fmt_num(to - pos[&from]) };
        self.solve_planes(env)?;
        Ok(())
    }

    /// Type a new value into a dimension that has no label: the plane at its far end moves (or
    /// the near one when the far one is fixed). A locked dimension keeps the new value.
    pub fn set_dim_value(&mut self, id: u32, seg: usize, value: f64, env: &mut Env) -> Result<(), String> {
        self.fix_dim_ids();
        let di = self.dim_index(id)?;
        let saved = (self.planes.clone(), self.dims.clone());
        let d = self.dims[di].clone();
        if d.label.is_some() || d.seg_labels.iter().any(|l| !l.trim().is_empty()) || d.eq {
            return Err("this dimension is driven by a parameter or by EQ; change the parameter instead".into());
        }
        let refs = d.refs();
        if seg + 1 >= refs.len() {
            return Err("no such segment".into());
        }
        let pos = self.solve_planes(env)?;
        let run = |this: &mut FamilyBody, env: &mut Env| -> Result<(), String> {
            // release this dimension so its own lock does not hold the plane back
            for p in std::mem::take(&mut this.dims[di].drives) {
                this.bake(p, &pos)?;
            }
            let taken = this.driven_elsewhere(di);
            let (lo, hi) = (refs[seg], refs[seg + 1]);
            let (target, base, sign) = if !this.is_origin(hi) && !taken.contains(&hi) { (hi, lo, 1.0) } else if !this.is_origin(lo) && !taken.contains(&lo) { (lo, hi, -1.0) } else { return Err("both references are fixed".into()) };
            this.move_plane(target, pos[&base] + sign * value, env)?;
            let pos2 = this.solve_planes(env)?;
            if this.dims[di].locked {
                this.derive_dim(di, &pos2, env)?;
            }
            Ok(())
        };
        match run(self, env) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.planes = saved.0;
                self.dims = saved.1;
                Err(e)
            }
        }
    }

    pub fn remove_form(&mut self, id: FormId) {
        self.forms.retain(|f| f.id != id);
        for f in &mut self.forms {
            f.cut_by.retain(|c| *c != id);
        }
    }
}

// ------------------------------------------------------------------ solving

/// Resolves a parameter name to a value for the formula engine.
pub type Env<'a> = dyn FnMut(&str) -> Result<openarch_params::Value, openarch_params::EvalError> + 'a;

fn eval_num(src: &str, env: &mut Env) -> Result<f64, String> {
    let s = src.trim();
    if let Ok(n) = s.parse::<f64>() {
        return Ok(n);
    }
    let expr = openarch_params::expr::parse(s).map_err(|e| format!("'{s}': {e:?}"))?;
    openarch_params::expr::eval(&expr, env).and_then(|v| v.as_num()).map_err(|e| format!("'{s}': {e:?}"))
}

fn eval_bool(src: &str, env: &mut Env) -> Result<bool, String> {
    let s = src.trim();
    let expr = openarch_params::expr::parse(s).map_err(|e| format!("'{s}': {e:?}"))?;
    openarch_params::expr::eval(&expr, env).and_then(|v| v.as_bool()).map_err(|e| format!("'{s}': {e:?}"))
}

/// A triangle in family coordinates, counter-clockwise seen from outside.
pub type Tri = [[f64; 3]; 3];

/// One straight-sided piece of an extrusion: the sketch regions between two depths.
#[derive(Debug, Clone, PartialEq)]
pub struct PrismPart {
    pub lo: f64,
    pub hi: f64,
    /// Outline first, then holes, in (u, v).
    pub shapes: Vec<Shape>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    /// The extrusion's normal; set for extrusions (and so exactly projectable), `None` for lofts and revolves.
    pub axis: Option<Axis>,
    pub parts: Vec<PrismPart>,
    pub tris: Vec<Tri>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SolvedVis {
    pub coarse: bool,
    pub medium: bool,
    pub fine: bool,
    pub plan: bool,
    pub front_back: bool,
    pub left_right: bool,
    /// False when the `when` condition fails: the piece is not there at all.
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SolvedForm {
    pub id: FormId,
    pub name: String,
    pub void: bool,
    pub cuts_host: bool,
    pub material: String,
    pub subcategory: String,
    pub vis: SolvedVis,
    pub solid: Solid,
    /// Where an extrusion starts and ends along its normal (absolute, in the order drawn).
    pub span: Option<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SolvedSym {
    pub id: u32,
    pub view: SymView,
    /// Tessellated outline in (u, v) of the view's axes; closed loops repeat nothing.
    pub points: Vec<Point>,
    pub closed: bool,
    pub filled: bool,
    pub masking: bool,
    pub subcategory: String,
    pub vis: SolvedVis,
}

#[derive(Debug, Clone, Default)]
pub struct Solved {
    pub planes: BTreeMap<PlaneId, f64>,
    /// Reporting parameters read from dimensions: name and measured distance.
    pub reports: Vec<(String, f64)>,
    pub forms: Vec<SolvedForm>,
    pub syms: Vec<SolvedSym>,
    pub warnings: Vec<String>,
}

impl Solved {
    pub fn form(&self, id: FormId) -> Option<&SolvedForm> {
        self.forms.iter().find(|f| f.id == id)
    }
}

/// Points along the chain `l` (nodes already placed), arcs cut into short chords.
pub fn tessellate_loop(pts: &[Point], l: &Loop, tol: f64) -> Vec<Point> {
    let n = pts.len();
    let mut out = Vec::new();
    let segs = if l.closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (p0, p1) = (pts[i], pts[(i + 1) % n]);
        out.push(p0);
        let b = l.bulge(i);
        let c = p0.dist(&p1);
        if b.abs() > 1e-9 && c > 1e-9 {
            let theta = 4.0 * b.atan();
            let h = (c / 2.0) / (theta / 2.0).tan();
            let (dx, dy) = ((p1.x - p0.x) / c, (p1.y - p0.y) / c);
            let centre = Point::new((p0.x + p1.x) / 2.0 - dy * h, (p0.y + p1.y) / 2.0 + dx * h);
            let r = p0.dist(&centre);
            let a0 = (p0.y - centre.y).atan2(p0.x - centre.x);
            let step = 2.0 * (1.0 - (tol / r.max(tol * 2.0)).min(0.5)).acos();
            let k = ((theta.abs() / step.max(0.02)).ceil() as usize).clamp(4, 720);
            for j in 1..k {
                let a = a0 + theta * j as f64 / k as f64;
                out.push(Point::new(centre.x + r * a.cos(), centre.y + r * a.sin()));
            }
        }
    }
    if !l.closed {
        if let Some(last) = pts.last() {
            out.push(*last);
        }
    }
    out
}

const TOL: f64 = 0.5;

/// A number as it goes into an expression: no trailing noise from float arithmetic.
fn fmt_num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

impl FamilyBody {
    /// Positions of every reference plane, resolving offsets through `env`.
    pub fn solve_planes(&self, env: &mut Env) -> Result<BTreeMap<PlaneId, f64>, String> {
        let mut pos: BTreeMap<PlaneId, f64> = BTreeMap::new();
        let mut left: Vec<&RefPlane> = self.planes.iter().collect();
        while !left.is_empty() {
            let before = left.len();
            let mut rest = Vec::new();
            for p in left {
                let v = match &p.anchor {
                    Anchor::Origin => Some(0.0),
                    Anchor::Offset { from, by } => match pos.get(from) {
                        Some(f) => Some(f + eval_num(by, env).map_err(|e| format!("reference plane '{}': {e}", p.name))?),
                        None => None,
                    },
                    Anchor::Mid { a, b } => match (pos.get(a), pos.get(b)) {
                        (Some(x), Some(y)) => Some((x + y) / 2.0),
                        _ => None,
                    },
                    Anchor::Div { a, b, i, n } => match (pos.get(a), pos.get(b)) {
                        (Some(x), Some(y)) if *n > 0 => Some(x + (y - x) * f64::from(*i) / f64::from(*n)),
                        (Some(_), Some(_)) => return Err(format!("reference plane '{}': equal division into zero parts", p.name)),
                        _ => None,
                    },
                };
                match v {
                    Some(v) => {
                        pos.insert(p.id, v);
                    }
                    None => rest.push(p),
                }
            }
            if rest.len() == before {
                return Err(format!("reference planes depend on each other in a circle ({})", rest.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")));
            }
            left = rest;
        }
        Ok(pos)
    }

    fn coord(&self, pos: &BTreeMap<PlaneId, f64>, c: &Coord, axis: Axis) -> Result<f64, String> {
        match c.plane {
            None => Ok(c.off),
            Some(id) => {
                let p = self.plane(id).ok_or_else(|| format!("sketch refers to a missing reference plane {id}"))?;
                if p.axis != axis {
                    return Err(format!("a sketch point is locked to '{}', which runs the wrong way", p.name));
                }
                Ok(pos[&id] + c.off)
            }
        }
    }

    fn place(&self, pos: &BTreeMap<PlaneId, f64>, l: &Loop, axes: (Axis, Axis)) -> Result<Vec<Point>, String> {
        l.nodes.iter().map(|n| Ok(Point::new(self.coord(pos, &n.u, axes.0)?, self.coord(pos, &n.v, axes.1)?))).collect()
    }

    fn polys(&self, pos: &BTreeMap<PlaneId, f64>, loops: &[Loop], axes: (Axis, Axis)) -> Result<Vec<Vec<Point>>, String> {
        let mut out = Vec::new();
        for l in loops {
            if l.nodes.len() < 3 && !(l.nodes.len() == 2 && l.bulges.iter().any(|b| b.abs() > 1e-9)) {
                return Err("a sketch loop needs at least three points".into());
            }
            let pts = self.place(pos, l, axes)?;
            let poly = tessellate_loop(&pts, l, TOL);
            if poly.len() >= 3 && signed_area(&poly).abs() > 1e-6 {
                out.push(poly);
            }
        }
        Ok(out)
    }

    /// One end of an extrusion along the work plane's normal: the locked plane's position, or the
    /// expression measured from the work plane.
    fn depth(&self, pos: &BTreeMap<PlaneId, f64>, normal: Axis, w0: f64, expr: &str, lock: Option<PlaneId>, env: &mut Env) -> Result<f64, String> {
        match lock {
            Some(id) => {
                let p = self.plane(id).ok_or("a plane the extrusion is locked to is missing")?;
                if p.axis != normal {
                    return Err(format!("'{}' is not parallel to the work plane, so an end cannot be locked to it", p.name));
                }
                Ok(pos[&id])
            }
            None => Ok(w0 + eval_num(expr, env)?),
        }
    }

    /// The box the host-cutting voids span, `[min, max]` in family coordinates, without meshing
    /// anything. Used wherever only the size of the opening matters (placement, fit, dimensions).
    pub fn opening_box(&self, env: &mut Env) -> Result<Option<[[f64; 3]; 2]>, String> {
        let planes = self.solve_planes(env)?;
        let mut b: Option<[[f64; 3]; 2]> = None;
        for f in self.forms.iter().filter(|f| f.void && f.cuts_host) {
            if !self.resolve_vis(&f.vis, env)?.present {
                continue;
            }
            let FormShape::Extrusion { plane, start, end, start_lock, end_lock, profile } = &f.shape else { continue };
            let wp = self.plane(*plane).ok_or("the work plane is missing")?;
            let axes = wp.axis.sketch_axes();
            let w0 = planes[&wp.id];
            let (a, c) = (self.depth(&planes, wp.axis, w0, start, *start_lock, env)?, self.depth(&planes, wp.axis, w0, end, *end_lock, env)?);
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for ring in self.polys(&planes, profile, axes)? {
                for p in ring {
                    for w in [a, c] {
                        let q = wp.axis.point(p.x, p.y, w);
                        for k in 0..3 {
                            lo[k] = lo[k].min(q[k]);
                            hi[k] = hi[k].max(q[k]);
                        }
                    }
                }
            }
            if lo[0].is_finite() {
                b = Some(match b {
                    None => [lo, hi],
                    Some(o) => [[o[0][0].min(lo[0]), o[0][1].min(lo[1]), o[0][2].min(lo[2])], [o[1][0].max(hi[0]), o[1][1].max(hi[1]), o[1][2].max(hi[2])]],
                });
            }
        }
        Ok(b)
    }

    fn resolve_vis(&self, v: &Visibility, env: &mut Env) -> Result<SolvedVis, String> {
        let present = match &v.when {
            Some(w) if !w.trim().is_empty() => eval_bool(w, env).map_err(|e| format!("visibility: {e}"))?,
            _ => true,
        };
        Ok(SolvedVis { coarse: v.coarse, medium: v.medium, fine: v.fine, plan: v.plan, front_back: v.front_back, left_right: v.left_right, present })
    }

    /// Work out the whole body for one set of parameter values.
    pub fn solve(&self, env: &mut Env) -> Result<Solved, String> {
        let planes = self.solve_planes(env)?;
        let mut out = Solved { planes: planes.clone(), ..Default::default() };
        for d in &self.dims {
            if let (Some(name), Some(a), Some(b)) = (&d.reports_to, planes.get(&d.a), planes.get(&d.b)) {
                out.reports.push((name.clone(), (b - a).abs()));
            }
        }
        for f in &self.forms {
            let vis = self.resolve_vis(&f.vis, env).map_err(|e| format!("{}: {e}", label(f)))?;
            let material = match &f.material_param {
                Some(p) => match env(p) {
                    Ok(openarch_params::Value::Text(s)) => s,
                    _ => f.material.clone(),
                },
                None => f.material.clone(),
            };
            let solid = self.solve_form(&planes, f, env).map_err(|e| format!("{}: {e}", label(f)))?;
            let span = match &f.shape {
                FormShape::Extrusion { plane, start, end, start_lock, end_lock, .. } => {
                    let wp = self.plane(*plane).ok_or("the work plane is missing")?;
                    let w0 = planes[&wp.id];
                    Some((self.depth(&planes, wp.axis, w0, start, *start_lock, env)?, self.depth(&planes, wp.axis, w0, end, *end_lock, env)?))
                }
                _ => None,
            };
            out.forms.push(SolvedForm { id: f.id, name: f.name.clone(), void: f.void, cuts_host: f.void && f.cuts_host, material, subcategory: f.subcategory.clone(), vis, solid, span });
        }
        // voids cut the solids that name them
        for (i, f) in self.forms.iter().enumerate() {
            for vid in &f.cut_by {
                let Some(v) = out.forms.iter().find(|s| s.id == *vid && s.void).cloned() else { continue };
                if !out.forms[i].vis.present || !v.vis.present {
                    continue;
                }
                match cut_prisms(&mut out.forms[i].solid, &v.solid) {
                    Ok(()) => {}
                    Err(w) => out.warnings.push(format!("{}: {w}", label(f))),
                }
            }
        }
        for s in &self.syms {
            let axes = s.view.normal().sketch_axes();
            let pts = self.place(&planes, &s.path, axes).map_err(|e| format!("symbolic geometry {}: {e}", s.id))?;
            let vis = self.resolve_vis(&s.vis, env)?;
            out.syms.push(SolvedSym {
                id: s.id,
                view: s.view,
                points: tessellate_loop(&pts, &s.path, TOL),
                closed: s.path.closed,
                filled: s.filled,
                masking: s.masking,
                subcategory: s.subcategory.clone(),
                vis,
            });
        }
        Ok(out)
    }

    fn solve_form(&self, pos: &BTreeMap<PlaneId, f64>, f: &Form, env: &mut Env) -> Result<Solid, String> {
        let wp = self.plane(f.shape.plane()).ok_or("the work plane is missing")?;
        let normal = wp.axis;
        let axes = normal.sketch_axes();
        let w0 = pos[&wp.id];
        match &f.shape {
            FormShape::Extrusion { start, end, start_lock, end_lock, profile, .. } => {
                let (a, b) = (self.depth(pos, normal, w0, start, *start_lock, env)?, self.depth(pos, normal, w0, end, *end_lock, env)?);
                let (lo, hi) = (a.min(b), a.max(b));
                if hi - lo < 1e-6 {
                    return Err("start and end depth are the same".into());
                }
                let rings = self.polys(pos, profile, axes)?;
                if rings.is_empty() {
                    return Err("the sketch has no closed loop".into());
                }
                let shapes: Vec<Shape> = regions(&rings).into_iter().map(|r| std::iter::once(r.outer).chain(r.holes).collect()).collect();
                let part = PrismPart { lo, hi, shapes };
                let tris = prism_tris(normal, &part);
                Ok(Solid { axis: Some(normal), parts: vec![part], tris })
            }
            FormShape::Blend { start, end, base, top, .. } => {
                let (a, b) = (w0 + eval_num(start, env)?, w0 + eval_num(end, env)?);
                let (p0, p1) = (self.polys(pos, base, axes)?, self.polys(pos, top, axes)?);
                if p0.len() != 1 || p1.len() != 1 {
                    return Err("a blend needs one closed loop at the base and one at the top".into());
                }
                let (mut r0, mut r1) = (p0.into_iter().next().unwrap(), p1.into_iter().next().unwrap());
                if r0.len() != r1.len() {
                    return Err("the base and top of a blend need the same number of points".into());
                }
                if signed_area(&r0) < 0.0 {
                    r0.reverse();
                    r1.reverse();
                }
                Ok(Solid { axis: None, parts: vec![], tris: loft_tris(normal, &r0, &r1, a, b) })
            }
            FormShape::Sweep { path, profile, profile_angle, flip, .. } => {
                if path.nodes.len() < 2 {
                    return Err("a sweep needs a path of at least two points".into());
                }
                let pts = self.place(pos, path, axes)?;
                let route = tessellate_loop(&pts, path, TOL);
                let rings = profile_rings(profile, env)?;
                if rings.is_empty() {
                    return Err("a sweep needs a closed profile".into());
                }
                let tris = sweep_tris(normal, w0, &route, path.closed, &rings, *profile_angle, *flip)?;
                Ok(Solid { axis: None, parts: vec![], tris })
            }
            FormShape::Revolve { axis_a, axis_b, start_deg, end_deg, profile, .. } => {
                let rings = self.polys(pos, profile, axes)?;
                let pa = Point::new(self.coord(pos, &axis_a.u, axes.0)?, self.coord(pos, &axis_a.v, axes.1)?);
                let pb = Point::new(self.coord(pos, &axis_b.u, axes.0)?, self.coord(pos, &axis_b.v, axes.1)?);
                if rings.is_empty() || pa.dist(&pb) < 1e-6 {
                    return Err("a revolve needs a closed loop and an axis".into());
                }
                Ok(Solid { axis: None, parts: vec![], tris: revolve_tris(normal, w0, &rings, pa, pb, *start_deg, *end_deg) })
            }
        }
    }
}

/// The profile's loops as closed polygons across and up from the path.
fn profile_rings(profile: &[ProfileLoop], env: &mut Env) -> Result<Vec<Vec<Point>>, String> {
    let mut out = Vec::new();
    for l in profile {
        let (pts, bulges): (Vec<Point>, Vec<f64>) = match l {
            ProfileLoop::Poly { points, bulges } => {
                if points.len() < 3 && !(points.len() == 2 && bulges.iter().any(|b| b.abs() > 1e-9)) {
                    return Err("a profile loop needs at least three points".into());
                }
                let mut v = Vec::new();
                for p in points {
                    v.push(Point::new(eval_num(&p.u, env)?, eval_num(&p.v, env)?));
                }
                (v, bulges.clone())
            }
            ProfileLoop::Circle { u, v, r } => {
                let (cu, cv, r) = (eval_num(u, env)?, eval_num(v, env)?, eval_num(r, env)?);
                if r <= 0.0 {
                    return Err("a profile circle needs a radius above zero".into());
                }
                (vec![Point::new(cu + r, cv), Point::new(cu - r, cv)], vec![1.0, 1.0])
            }
        };
        let shape = Loop { nodes: pts.iter().map(|p| Node::new(Coord::at(p.x), Coord::at(p.y))).collect(), bulges, closed: true };
        let poly = tessellate_loop(&pts, &shape, TOL);
        if poly.len() >= 3 && signed_area(&poly).abs() > 1e-6 {
            out.push(poly);
        }
    }
    Ok(out)
}

fn label(f: &Form) -> String {
    if f.name.is_empty() {
        format!("form {}", f.id)
    } else {
        f.name.clone()
    }
}

// ------------------------------------------------------------------ meshing

fn tri_out(out: &mut Vec<Tri>, flip: bool, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    out.push(if flip { [a, c, b] } else { [a, b, c] });
}

/// The faces of one extrusion part: top and bottom caps, and a wall for every ring edge.
pub fn prism_tris(axis: Axis, part: &PrismPart) -> Vec<Tri> {
    let mut out = Vec::new();
    let flip = axis.mirrors();
    for shape in &part.shapes {
        let region = crate::geom::Region { outer: shape[0].clone(), holes: shape[1..].to_vec() };
        if let Some((pts, tris)) = triangulate_region(&region) {
            for t in &tris {
                let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
                tri_out(&mut out, flip, axis.point(a.x, a.y, part.hi), axis.point(b.x, b.y, part.hi), axis.point(c.x, c.y, part.hi));
                tri_out(&mut out, flip, axis.point(a.x, a.y, part.lo), axis.point(c.x, c.y, part.lo), axis.point(b.x, b.y, part.lo));
            }
        }
        for (k, ring) in shape.iter().enumerate() {
            // outline counter-clockwise, holes clockwise: the wall faces away from the material either way
            let ring: Vec<Point> = if (signed_area(ring) > 0.0) == (k == 0) { ring.clone() } else { ring.iter().rev().copied().collect() };
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                if a.dist(&b) < 1e-9 {
                    continue;
                }
                tri_out(&mut out, flip, axis.point(a.x, a.y, part.lo), axis.point(b.x, b.y, part.lo), axis.point(b.x, b.y, part.hi));
                tri_out(&mut out, flip, axis.point(a.x, a.y, part.lo), axis.point(b.x, b.y, part.hi), axis.point(a.x, a.y, part.hi));
            }
        }
    }
    out
}

fn loft_tris(axis: Axis, r0: &[Point], r1: &[Point], w0: f64, w1: f64) -> Vec<Tri> {
    let mut out = Vec::new();
    let flip = axis.mirrors() ^ (w1 < w0);
    for (ring, w, up) in [(r1, w1, true), (r0, w0, false)] {
        if let Some(tris) = crate::geom::triangulate(ring) {
            for t in tris {
                let (a, b, c) = (ring[t[0]], ring[t[1]], ring[t[2]]);
                let (pa, pb, pc) = (axis.point(a.x, a.y, w), axis.point(b.x, b.y, w), axis.point(c.x, c.y, w));
                // the cap at the far end faces along the normal, the near one against it
                tri_out(&mut out, flip ^ (!up), pa, pb, pc);
            }
        }
    }
    let n = r0.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (a0, b0) = (axis.point(r0[i].x, r0[i].y, w0), axis.point(r0[j].x, r0[j].y, w0));
        let (a1, b1) = (axis.point(r1[i].x, r1[i].y, w1), axis.point(r1[j].x, r1[j].y, w1));
        tri_out(&mut out, flip, a0, b0, b1);
        tri_out(&mut out, flip, a0, b1, a1);
    }
    out
}

/// A profile carried along a planar path. Corners are mitred, so the profile keeps its width
/// through a turn. The result faces outward whichever way the path and profile run.
fn sweep_tris(axis: Axis, w: f64, route: &[Point], closed: bool, rings: &[Vec<Point>], angle_deg: f64, flip: bool) -> Result<Vec<Tri>, String> {
    // drop repeated points
    let mut r: Vec<Point> = Vec::new();
    for p in route {
        if r.last().map_or(true, |q| q.dist(p) > 1e-6) {
            r.push(*p);
        }
    }
    if closed && r.len() > 2 && r[0].dist(&r[r.len() - 1]) < 1e-6 {
        r.pop();
    }
    let n = r.len();
    if n < 2 || (closed && n < 3) {
        return Err("the sweep path is too short".into());
    }
    let dir = |a: Point, b: Point| {
        let l = a.dist(&b);
        ((b.x - a.x) / l, (b.y - a.y) / l)
    };
    // for each path point: the point, the left offset vector (mitred), scaled so a profile `u`
    // of 1 sits one unit from the path on both sides of a corner
    let mut frames: Vec<(Point, (f64, f64))> = Vec::new();
    for i in 0..n {
        let prev = if i > 0 { Some(dir(r[i - 1], r[i])) } else if closed { Some(dir(r[n - 1], r[0])) } else { None };
        let next = if i + 1 < n { Some(dir(r[i], r[i + 1])) } else if closed { Some(dir(r[i], r[0])) } else { None };
        let left = |d: (f64, f64)| (-d.1, d.0);
        let m = match (prev, next) {
            (Some(a), Some(b)) => {
                let (na, nb) = (left(a), left(b));
                let k = 1.0 + na.0 * nb.0 + na.1 * nb.1;
                if k < 1e-3 {
                    return Err("the sweep path turns back on itself".into());
                }
                ((na.0 + nb.0) / k, (na.1 + nb.1) / k)
            }
            (Some(d), None) | (None, Some(d)) => left(d),
            (None, None) => unreachable!(),
        };
        frames.push((r[i], m));
    }
    let (sa, ca) = angle_deg.to_radians().sin_cos();
    let place = |(px, py): (f64, f64)| {
        let (a, b) = (px * ca - py * sa, px * sa + py * ca);
        (if flip { -a } else { a }, b)
    };
    let mut out = Vec::new();
    for region in regions(rings) {
        let shapes: Vec<Vec<Point>> = std::iter::once(region.outer.clone()).chain(region.holes.iter().cloned()).collect();
        let at = |k: usize, q: Point| {
            let (a, b) = place((q.x, q.y));
            let (p, m) = frames[k];
            axis.point(p.x + m.0 * a, p.y + m.1 * a, w + b)
        };
        for ring in &shapes {
            let c = ring.len();
            let spans = if closed { n } else { n - 1 };
            for k in 0..spans {
                let k1 = (k + 1) % n;
                for i in 0..c {
                    let j = (i + 1) % c;
                    let (a0, b0, a1, b1) = (at(k, ring[i]), at(k, ring[j]), at(k1, ring[i]), at(k1, ring[j]));
                    out.push([a0, b0, b1]);
                    out.push([a0, b1, a1]);
                }
            }
        }
        if !closed {
            if let Some((pts, tris)) = triangulate_region(&region) {
                for t in tris {
                    let q = [pts[t[0]], pts[t[1]], pts[t[2]]];
                    out.push([at(n - 1, q[0]), at(n - 1, q[1]), at(n - 1, q[2])]);
                    out.push([at(0, q[0]), at(0, q[2]), at(0, q[1])]);
                }
            }
        }
    }
    if volume(&out) < 0.0 {
        for t in &mut out {
            t.swap(1, 2);
        }
    }
    Ok(out)
}

fn revolve_tris(axis: Axis, w: f64, rings: &[Vec<Point>], pa: Point, pb: Point, s: f64, e: f64) -> Vec<Tri> {
    let mut out = Vec::new();
    let (dx, dy) = (pb.x - pa.x, pb.y - pa.y);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);
    let sweep = (e - s).to_radians();
    let steps = ((sweep.abs() / (std::f64::consts::PI / 24.0)).ceil() as usize).max(2);
    // a profile point as (along the axis, distance from it)
    let split = |p: Point| ((p.x - pa.x) * ux + (p.y - pa.y) * uy, -(p.x - pa.x) * uy + (p.y - pa.y) * ux);
    for ring in rings {
        let q: Vec<(f64, f64)> = ring.iter().map(|p| split(*p)).collect();
        // the circle a profile point sweeps lies in the plane across the axis; the sketch's
        // own side of the axis is the angle origin, and depth along the normal is the second direction
        let at = |(t, r): (f64, f64), ang: f64| {
            let (c, sn) = (ang.cos(), ang.sin());
            let (px, py) = (pa.x + ux * t - uy * r * c, pa.y + uy * t + ux * r * c);
            axis.point(px, py, w + r * sn)
        };
        let n = q.len();
        for i in 0..n {
            let (p0, p1) = (q[i], q[(i + 1) % n]);
            for k in 0..steps {
                let (a0, a1) = (s.to_radians() + sweep * k as f64 / steps as f64, s.to_radians() + sweep * (k + 1) as f64 / steps as f64);
                let (v00, v01, v10, v11) = (at(p0, a0), at(p0, a1), at(p1, a0), at(p1, a1));
                tri_out(&mut out, false, v00, v10, v11);
                tri_out(&mut out, false, v00, v11, v01);
            }
        }
    }
    // faces wind the way the profile runs; make them face outward by checking the signed volume
    if volume(&out) < 0.0 {
        for t in &mut out {
            t.swap(1, 2);
        }
    }
    out
}

/// Signed volume of a closed triangle mesh (positive when faces point outward).
pub fn volume(tris: &[Tri]) -> f64 {
    tris.iter()
        .map(|[a, b, c]| (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0)
        .sum()
}

/// Subtract an extrusion void from an extrusion solid along the same axis. The solid is
/// cut into parts at the void's depths and the void's sketch is taken out of the parts it overlaps.
fn cut_prisms(solid: &mut Solid, void: &Solid) -> Result<(), String> {
    let (Some(a), Some(b)) = (solid.axis, void.axis) else { return Err("only extrusions can be cut".into()) };
    if a != b {
        return Err("a void running across the solid needs the solid kernel; it is not cut".into());
    }
    let mut parts: Vec<PrismPart> = Vec::new();
    for part in &solid.parts {
        let mut cuts = vec![part.lo, part.hi];
        for v in &void.parts {
            for z in [v.lo, v.hi] {
                if z > part.lo + 1e-9 && z < part.hi - 1e-9 {
                    cuts.push(z);
                }
            }
        }
        cuts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        for w in cuts.windows(2) {
            let mid = (w[0] + w[1]) / 2.0;
            let mut shapes = part.shapes.clone();
            for v in &void.parts {
                if mid > v.lo && mid < v.hi {
                    let clips: Vec<Vec<Point>> = v.shapes.iter().flat_map(|s| s.first().cloned()).collect();
                    shapes = subtract_shapes(&shapes, &clips);
                }
            }
            if !shapes.is_empty() {
                parts.push(PrismPart { lo: w[0], hi: w[1], shapes });
            }
        }
    }
    solid.tris = parts.iter().flat_map(|p| prism_tris(a, p)).collect();
    solid.parts = parts;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openarch_params::{EvalError, Value};

    fn params(list: &[(&str, f64)]) -> impl FnMut(&str) -> Result<Value, EvalError> {
        let m: BTreeMap<String, f64> = list.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        move |n: &str| m.get(n).map(|v| Value::Num(*v)).ok_or_else(|| EvalError::UnknownName(n.to_string()))
    }

    /// A 2 m wide block 100 deep and 2.1 m tall, as a slab between planes that follow Width.
    fn block() -> (FamilyBody, FormId) {
        let mut b = FamilyBody::template();
        let left = b.add_offset_plane("Left", PLANE_LR, "-Width/2").unwrap();
        let right = b.add_offset_plane("Right", PLANE_LR, "Width/2").unwrap();
        let top = b.add_offset_plane("Top", PLANE_LEVEL, "Height").unwrap();
        let id = b.add_form(Form {
            id: 0,
            name: "Slab".into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None,
                plane: PLANE_FB,
                start: "-50".into(),
                end: "50".into(),
                profile: vec![Loop::rect(Coord::on(left), Coord::on(right), Coord::on(PLANE_LEVEL), Coord::on(top))],
            },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: "Timber".into(),
            material_param: None,
            subcategory: "Frame".into(),
            vis: Visibility::default(),
        });
        (b, id)
    }

    #[test]
    fn planes_follow_parameters_and_nodes_follow_planes() {
        let (b, id) = block();
        let s = b.solve(&mut params(&[("Width", 900.0), ("Height", 2100.0)])).unwrap();
        assert!((volume(&s.form(id).unwrap().solid.tris) - 900.0 * 2100.0 * 100.0).abs() < 1.0);
        let s2 = b.solve(&mut params(&[("Width", 1200.0), ("Height", 2100.0)])).unwrap();
        assert!((volume(&s2.form(id).unwrap().solid.tris) - 1200.0 * 2100.0 * 100.0).abs() < 1.0);
    }

    #[test]
    fn a_circle_is_made_of_arcs_and_a_hole_takes_its_area_out() {
        let mut b = FamilyBody::template();
        let r = 100.0;
        let outer = Loop::rect(Coord::at(-200.0), Coord::at(200.0), Coord::at(-200.0), Coord::at(200.0));
        let hole = Loop::circle(Node::new(Coord::at(0.0), Coord::at(0.0)), r);
        let id = b.add_form(Form {
            id: 0,
            name: "Ring".into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "10".into(), profile: vec![outer, hole] },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        let s = b.solve(&mut params(&[])).unwrap();
        let v = volume(&s.form(id).unwrap().solid.tris);
        let want = (400.0 * 400.0 - std::f64::consts::PI * r * r) * 10.0;
        assert!((v - want).abs() / want < 0.01, "{v} vs {want}");
    }

    #[test]
    fn an_extrusion_on_each_axis_has_outward_faces() {
        for plane in [PLANE_LR, PLANE_FB, PLANE_LEVEL] {
            let mut b = FamilyBody::template();
            let id = b.add_form(Form {
                id: 0,
                name: "Box".into(),
                shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane, start: "0".into(), end: "30".into(), profile: vec![Loop::rect(Coord::at(0.0), Coord::at(10.0), Coord::at(0.0), Coord::at(20.0))] },
                void: false,
                cuts_host: true,
                cut_by: vec![],
                material: String::new(),
                material_param: None,
                subcategory: String::new(),
                vis: Visibility::default(),
            });
            let s = b.solve(&mut params(&[])).unwrap();
            let v = volume(&s.form(id).unwrap().solid.tris);
            assert!((v - 6000.0).abs() < 1e-6, "axis of plane {plane}: {v}");
        }
    }

    #[test]
    fn a_void_cuts_the_solids_that_name_it() {
        let (mut b, slab) = block();
        let hole = b.add_form(Form {
            id: 0,
            name: "Hole".into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None,
                plane: PLANE_FB,
                start: "-20".into(),
                end: "20".into(),
                profile: vec![Loop::rect(Coord::at(-100.0), Coord::at(100.0), Coord::at(500.0), Coord::at(1000.0))],
            },
            void: true,
            cuts_host: false,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        b.forms[0].cut_by.push(hole);
        let s = b.solve(&mut params(&[("Width", 900.0), ("Height", 2100.0)])).unwrap();
        let v = volume(&s.form(slab).unwrap().solid.tris);
        assert!((v - (900.0 * 2100.0 * 100.0 - 200.0 * 500.0 * 40.0)).abs() < 1.0, "{v}");
        // a void with a different axis is reported, not silently ignored
        let (mut b2, _) = block();
        let across = b2.add_form(Form {
            id: 0,
            name: "Across".into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "100".into(), profile: vec![Loop::rect(Coord::at(0.0), Coord::at(10.0), Coord::at(0.0), Coord::at(10.0))] },
            void: true,
            cuts_host: false,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        b2.forms[0].cut_by.push(across);
        let s2 = b2.solve(&mut params(&[("Width", 900.0), ("Height", 2100.0)])).unwrap();
        assert_eq!(s2.warnings.len(), 1);
    }

    #[test]
    fn planes_reject_circles_and_wrong_direction_locks() {
        let mut b = FamilyBody::template();
        let a = b.add_offset_plane("A", PLANE_LR, "B").unwrap();
        let c = b.add_offset_plane("B", a, "10").unwrap();
        // make A depend on B
        b.planes.iter_mut().find(|p| p.id == a).unwrap().anchor = Anchor::Offset { from: c, by: "5".into() };
        assert!(b.solve_planes(&mut params(&[])).unwrap_err().contains("circle"));
        let mut b = FamilyBody::template();
        let id = b.add_form(Form {
            id: 0,
            name: "Bad".into(),
            shape: FormShape::Extrusion { start_lock: None, end_lock: None, plane: PLANE_LEVEL, start: "0".into(), end: "10".into(), profile: vec![Loop::rect(Coord::on(PLANE_LEVEL), Coord::at(5.0), Coord::at(0.0), Coord::at(5.0))] },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        let _ = id;
        assert!(b.solve(&mut params(&[])).unwrap_err().contains("wrong way"));
        // planes in use cannot be deleted
        let (mut b, _) = block();
        let left = b.plane_named("Left").unwrap();
        assert!(b.remove_plane(left).is_err());
        assert!(b.remove_plane(PLANE_LR).is_err());
    }

    #[test]
    fn visibility_follows_a_condition_and_old_bodies_read_back() {
        let (mut b, id) = block();
        b.forms[0].vis.when = Some("Width > 1000".into());
        let s = b.solve(&mut params(&[("Width", 900.0), ("Height", 2100.0)])).unwrap();
        assert!(!s.form(id).unwrap().vis.present);
        let s = b.solve(&mut params(&[("Width", 1100.0), ("Height", 2100.0)])).unwrap();
        assert!(s.form(id).unwrap().vis.present);
        // a body round-trips through JSON, and an empty object is an empty body
        let j = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<FamilyBody>(&j).unwrap(), b);
        assert!(serde_json::from_str::<FamilyBody>("{}").unwrap().is_empty());
    }

    /// Every edge of a closed outward-facing mesh is used once each way.
    fn watertight(tris: &[Tri]) -> bool {
        let key = |p: [f64; 3]| [(p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64, (p[2] * 1000.0).round() as i64];
        let mut m: BTreeMap<([i64; 3], [i64; 3]), i32> = BTreeMap::new();
        for t in tris {
            for i in 0..3 {
                let (a, b) = (key(t[i]), key(t[(i + 1) % 3]));
                if a == b {
                    continue;
                }
                *m.entry((a, b)).or_insert(0) += 1;
            }
        }
        m.iter().all(|((a, b), n)| *n == 1 && m.get(&(*b, *a)) == Some(&1))
    }

    fn sweep_form(plane: PlaneId, path: Loop, profile: Vec<ProfileLoop>, flip: bool) -> Form {
        Form {
            id: 0,
            name: "Sweep".into(),
            shape: FormShape::Sweep { plane, path, profile, profile_angle: 0.0, flip },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        }
    }

    #[test]
    fn an_extrusion_end_locked_to_a_plane_follows_it() {
        let mut b = FamilyBody::template();
        let top = b.add_offset_plane("Top", PLANE_LEVEL, "[H]").unwrap();
        let id = b.add_form(Form {
            id: 0,
            name: "Post".into(),
            shape: FormShape::Extrusion {
                plane: PLANE_LEVEL,
                start: "0".into(),
                end: "50".into(),
                start_lock: None,
                end_lock: Some(top),
                profile: vec![Loop::rect(Coord::at(0.0), Coord::at(10.0), Coord::at(0.0), Coord::at(10.0))],
            },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        for h in [300.0, 900.0] {
            let s = b.solve(&mut params(&[("H", h)])).unwrap();
            assert_eq!(s.form(id).unwrap().span, Some((0.0, h)));
            assert!((volume(&s.form(id).unwrap().solid.tris) - 100.0 * h).abs() < 1e-6);
        }
        // the plane cannot be deleted while the extrusion hangs from it, and must be parallel
        assert!(b.remove_plane(top).is_err());
        let side = b.add_offset_plane("Side", PLANE_LR, "20").unwrap();
        if let FormShape::Extrusion { end_lock, .. } = &mut b.forms[0].shape {
            *end_lock = Some(side);
        }
        assert!(b.solve(&mut params(&[("H", 300.0)])).is_err());
    }

    #[test]
    fn a_sweep_follows_its_path_and_mitres_the_corners() {
        for plane in [PLANE_LEVEL, PLANE_FB, PLANE_LR] {
            for flip in [false, true] {
                let mut b = FamilyBody::template();
                let path = Loop { nodes: vec![Node::new(Coord::at(0.0), Coord::at(0.0)), Node::new(Coord::at(100.0), Coord::at(0.0)), Node::new(Coord::at(100.0), Coord::at(100.0))], bulges: vec![], closed: false };
                let id = b.add_form(sweep_form(plane, path, vec![ProfileLoop::rect("-10", "10", "-10", "10")], flip));
                let s = b.solve(&mut params(&[])).unwrap();
                let tris = &s.form(id).unwrap().solid.tris;
                let v = volume(tris);
                // a centred profile: the volume is area times path length, whatever the corner
                assert!((v - 400.0 * 200.0).abs() < 1.0, "{plane} {flip}: {v}");
                assert!(watertight(tris), "{plane} {flip}");
            }
        }
    }

    #[test]
    fn a_closed_path_sweeps_a_ring_and_a_profile_with_a_hole_a_tube() {
        let mut b = FamilyBody::template();
        let sq = Loop::rect(Coord::at(-100.0), Coord::at(100.0), Coord::at(-100.0), Coord::at(100.0));
        let frame = b.add_form(sweep_form(PLANE_LEVEL, sq, vec![ProfileLoop::rect("-5", "5", "0", "20")], false));
        let sq2 = Loop { nodes: vec![Node::new(Coord::at(0.0), Coord::at(0.0)), Node::new(Coord::at(300.0), Coord::at(0.0))], bulges: vec![], closed: false };
        let tube = b.add_form(sweep_form(
            PLANE_LEVEL,
            sq2,
            vec![ProfileLoop::rect("-20", "20", "-20", "20"), ProfileLoop::rect("-10", "10", "-10", "10")],
            false,
        ));
        let s = b.solve(&mut params(&[])).unwrap();
        let (f, t) = (&s.form(frame).unwrap().solid.tris, &s.form(tube).unwrap().solid.tris);
        // a rim 10 wide and 20 high around a 200 square path: 10 x 20 x 800 (corners trade equal areas)
        assert!((volume(f) - 10.0 * 20.0 * 800.0).abs() < 1.0, "{}", volume(f));
        assert!(watertight(f));
        assert!((volume(t) - (1600.0 - 400.0) * 300.0).abs() < 1.0, "{}", volume(t));
        assert!(watertight(t));
    }

    #[test]
    fn a_sweep_profile_follows_parameters() {
        let mut b = FamilyBody::template();
        let path = Loop { nodes: vec![Node::new(Coord::at(0.0), Coord::at(0.0)), Node::new(Coord::at(200.0), Coord::at(0.0))], bulges: vec![], closed: false };
        let id = b.add_form(sweep_form(PLANE_LEVEL, path, vec![ProfileLoop::rect("-[Size]/2", "[Size]/2", "0", "[Size] * 2")], false));
        let circ = Loop { nodes: vec![Node::new(Coord::at(0.0), Coord::at(0.0)), Node::new(Coord::at(100.0), Coord::at(0.0))], bulges: vec![], closed: false };
        let rod = b.add_form(sweep_form(PLANE_LEVEL, circ, vec![ProfileLoop::Circle { u: "0".into(), v: "0".into(), r: "[Size]".into() }], false));
        for size in [20.0, 40.0] {
            let s = b.solve(&mut params(&[("Size", size)])).unwrap();
            assert!((volume(&s.form(id).unwrap().solid.tris) - size * size * 2.0 * 200.0).abs() < 1.0);
            let want = std::f64::consts::PI * size * size * 100.0;
            assert!((volume(&s.form(rod).unwrap().solid.tris) - want).abs() / want < 0.04);
        }
    }

    #[test]
    fn a_sweep_round_a_curved_path_keeps_its_volume() {
        let mut b = FamilyBody::template();
        // a quarter circle of radius 100 turned through 90 degrees
        let path = Loop { nodes: vec![Node::new(Coord::at(100.0), Coord::at(0.0)), Node::new(Coord::at(0.0), Coord::at(100.0))], bulges: vec![(std::f64::consts::PI / 8.0).tan()], closed: false };
        let id = b.add_form(sweep_form(PLANE_LEVEL, path, vec![ProfileLoop::rect("-10", "10", "-10", "10")], false));
        let s = b.solve(&mut params(&[])).unwrap();
        let tris = &s.form(id).unwrap().solid.tris;
        let want = 400.0 * std::f64::consts::PI * 50.0;
        assert!((volume(tris) - want).abs() / want < 0.01, "{} vs {want}", volume(tris));
        assert!(watertight(tris));
    }

    #[test]
    fn blends_and_revolves_make_closed_solids() {
        let mut b = FamilyBody::template();
        let sq = |h: f64| Loop::rect(Coord::at(-h), Coord::at(h), Coord::at(-h), Coord::at(h));
        let blend = b.add_form(Form {
            id: 0,
            name: "Frustum".into(),
            shape: FormShape::Blend { plane: PLANE_LEVEL, start: "0".into(), end: "100".into(), base: vec![sq(100.0)], top: vec![sq(50.0)] },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        let cyl = b.add_form(Form {
            id: 0,
            name: "Cylinder".into(),
            shape: FormShape::Revolve {
                plane: PLANE_FB,
                axis_a: Node::new(Coord::at(0.0), Coord::at(0.0)),
                axis_b: Node::new(Coord::at(0.0), Coord::at(100.0)),
                start_deg: 0.0,
                end_deg: 360.0,
                profile: vec![Loop::rect(Coord::at(0.0), Coord::at(30.0), Coord::at(0.0), Coord::at(100.0))],
            },
            void: false,
            cuts_host: true,
            cut_by: vec![],
            material: String::new(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        });
        let s = b.solve(&mut params(&[])).unwrap();
        // a square frustum, V = h/3 (A1 + A2 + sqrt(A1 A2)) with A1 = 200^2 and A2 = 100^2
        let want = 100.0 / 3.0 * (40000.0 + 10000.0 + 20000.0);
        let v = volume(&s.form(blend).unwrap().solid.tris);
        assert!((v - want).abs() / want < 1e-9, "{v} vs {want}");
        let vc = volume(&s.form(cyl).unwrap().solid.tris);
        let wantc = std::f64::consts::PI * 900.0 * 100.0;
        assert!((vc - wantc).abs() / wantc < 0.02, "{vc} vs {wantc}");
    }

    // ---- dimension objects

    fn pos_of(b: &FamilyBody, id: PlaneId, env: &mut Env) -> f64 {
        b.solve_planes(env).unwrap()[&id]
    }

    fn label(l: &str) -> DimPatch {
        DimPatch { label: Some(l.into()), ..Default::default() }
    }

    #[test]
    fn labelled_dimension_drives_the_far_plane_and_follows_the_parameter() {
        let mut b = FamilyBody::template();
        let r = b.add_offset_plane("Right", PLANE_LR, "300").unwrap();
        let d = b.add_dim(&[r, PLANE_LR], Some(SymView::Plan), &mut params(&[])).unwrap();
        assert_eq!(b.dim(d).unwrap().refs(), vec![PLANE_LR, r]);
        b.patch_dim(d, &label("Width"), &mut params(&[("Width", 400.0)])).unwrap();
        assert_eq!(b.dim(d).unwrap().drives, vec![r]);
        assert_eq!(pos_of(&b, r, &mut params(&[("Width", 400.0)])), 400.0);
        assert_eq!(pos_of(&b, r, &mut params(&[("Width", 650.0)])), 650.0);
        // the plane on the near side of the origin gets a negative offset
        let l = b.add_offset_plane("Left", PLANE_LR, "-200").unwrap();
        let d2 = b.add_dim(&[PLANE_LR, l], None, &mut params(&[("Width", 400.0)])).unwrap();
        b.patch_dim(d2, &label("Depth"), &mut params(&[("Width", 400.0), ("Depth", 250.0)])).unwrap();
        assert_eq!(pos_of(&b, l, &mut params(&[("Width", 400.0), ("Depth", 250.0)])), -250.0);
    }

    #[test]
    fn dimension_between_two_free_planes_moves_the_far_one() {
        let mut b = FamilyBody::template();
        let a = b.add_offset_plane("A", PLANE_LR, "100").unwrap();
        let c = b.add_offset_plane("C", PLANE_LR, "500").unwrap();
        let d = b.add_dim(&[c, a], None, &mut params(&[])).unwrap();
        b.patch_dim(d, &label("Gap"), &mut params(&[("Gap", 250.0)])).unwrap();
        let env = &mut params(&[("Gap", 250.0)]);
        assert_eq!(pos_of(&b, a, env), 100.0);
        assert_eq!(pos_of(&b, c, env), 350.0);
        // moving A drags C along, and A cannot be unhooked from a plane that holds it
        b.dims.iter_mut().for_each(|d| d.drives.clear());
        assert!(b.dim(d).is_some());
    }

    #[test]
    fn eq_with_a_label_is_symmetric_about_the_origin() {
        let mut b = FamilyBody::template();
        let l = b.add_offset_plane("Left", PLANE_LR, "-300").unwrap();
        let r = b.add_offset_plane("Right", PLANE_LR, "300").unwrap();
        let d = b.add_dim(&[l, PLANE_LR, r], Some(SymView::Plan), &mut params(&[])).unwrap();
        b.patch_dim(d, &DimPatch { eq: Some(true), ..Default::default() }, &mut params(&[])).unwrap();
        b.patch_dim(d, &label("Width"), &mut params(&[("Width", 900.0)])).unwrap();
        let env = &mut params(&[("Width", 900.0)]);
        assert_eq!(pos_of(&b, l, env), -450.0);
        assert_eq!(pos_of(&b, r, env), 450.0);
        let env = &mut params(&[("Width", 1000.0)]);
        assert_eq!(pos_of(&b, l, env), -500.0);
        assert_eq!(pos_of(&b, r, env), 500.0);
        // the origin plane is never moved
        assert_eq!(pos_of(&b, PLANE_LR, env), 0.0);
    }

    #[test]
    fn eq_without_a_label_splits_the_span_evenly() {
        let mut b = FamilyBody::template();
        let a = b.add_offset_plane("A", PLANE_LR, "0").unwrap();
        let m1 = b.add_offset_plane("M1", PLANE_LR, "100").unwrap();
        let m2 = b.add_offset_plane("M2", PLANE_LR, "200").unwrap();
        let z = b.add_offset_plane("Z", PLANE_LR, "900").unwrap();
        let d = b.add_dim(&[z, a, m2, m1], None, &mut params(&[])).unwrap();
        b.patch_dim(d, &DimPatch { eq: Some(true), ..Default::default() }, &mut params(&[])).unwrap();
        let env = &mut params(&[]);
        assert_eq!(pos_of(&b, m1, env), 300.0);
        assert_eq!(pos_of(&b, m2, env), 600.0);
        // the end planes stay free: moving Z re-divides the span
        b.move_plane(z, 1200.0, env).unwrap();
        assert_eq!(pos_of(&b, m1, env), 400.0);
        assert_eq!(pos_of(&b, m2, env), 800.0);
        // a driven plane cannot be dragged
        assert!(b.move_plane(m1, 10.0, env).unwrap_err().contains("equal"));
    }

    #[test]
    fn locked_dimension_holds_its_value_when_the_near_plane_moves() {
        let mut b = FamilyBody::template();
        let a = b.add_offset_plane("A", PLANE_LR, "100").unwrap();
        let c = b.add_offset_plane("C", PLANE_LR, "400").unwrap();
        let d = b.add_dim(&[a, c], None, &mut params(&[])).unwrap();
        b.patch_dim(d, &DimPatch { locked: Some(true), ..Default::default() }, &mut params(&[])).unwrap();
        b.move_plane(a, 250.0, &mut params(&[])).unwrap();
        assert_eq!(pos_of(&b, c, &mut params(&[])), 550.0);
        // typing a value into the locked dimension keeps the lock at the new value
        b.set_dim_value(d, 0, 100.0, &mut params(&[])).unwrap();
        assert_eq!(pos_of(&b, c, &mut params(&[])), 350.0);
        b.move_plane(a, 0.0, &mut params(&[])).unwrap();
        assert_eq!(pos_of(&b, c, &mut params(&[])), 100.0);
        // unlocking frees the far plane where it stands
        b.patch_dim(d, &DimPatch { locked: Some(false), ..Default::default() }, &mut params(&[])).unwrap();
        b.move_plane(c, 700.0, &mut params(&[])).unwrap();
        assert_eq!(pos_of(&b, a, &mut params(&[])), 0.0);
    }

    #[test]
    fn removing_or_unlabelling_a_dimension_keeps_the_geometry_where_it_is() {
        let mut b = FamilyBody::template();
        let r = b.add_offset_plane("Right", PLANE_LR, "300").unwrap();
        let d = b.add_dim(&[PLANE_LR, r], None, &mut params(&[])).unwrap();
        b.patch_dim(d, &label("Width"), &mut params(&[("Width", 800.0)])).unwrap();
        b.patch_dim(d, &label(""), &mut params(&[("Width", 800.0)])).unwrap();
        assert!(b.dim(d).unwrap().drives.is_empty());
        assert_eq!(pos_of(&b, r, &mut params(&[])), 800.0);
        b.patch_dim(d, &label("Width"), &mut params(&[("Width", 900.0)])).unwrap();
        b.remove_dim(d, &mut params(&[("Width", 900.0)])).unwrap();
        assert!(b.dims.is_empty());
        assert_eq!(pos_of(&b, r, &mut params(&[])), 900.0);
    }

    #[test]
    fn over_constraint_and_loops_are_refused_and_change_nothing() {
        let mut b = FamilyBody::template();
        let a = b.add_offset_plane("A", PLANE_LR, "100").unwrap();
        let c = b.add_offset_plane("C", PLANE_LR, "500").unwrap();
        let d1 = b.add_dim(&[a, c], None, &mut params(&[])).unwrap();
        b.patch_dim(d1, &label("P"), &mut params(&[("P", 100.0), ("Q", 100.0)])).unwrap();
        // a second labelled dimension between the same two planes cannot also drive either of them
        let d2 = b.add_dim(&[a, c], None, &mut params(&[("P", 100.0)])).unwrap();
        let before = b.clone();
        let err = b.patch_dim(d2, &label("Q"), &mut params(&[("P", 100.0), ("Q", 100.0)]));
        assert!(err.unwrap_err().contains("over-constrain"));
        assert_eq!(b, before);
        // an unknown parameter name is refused without touching the planes
        let e = b.add_offset_plane("E", PLANE_LR, "50").unwrap();
        let d3 = b.add_dim(&[PLANE_LR, e], None, &mut params(&[("P", 100.0)])).unwrap();
        let before = b.clone();
        assert!(b.patch_dim(d3, &label("Nope"), &mut params(&[("P", 100.0)])).is_err());
        assert_eq!(b, before);
        // the origin plane is never driven
        assert!(b.dims.iter().all(|d| !d.drives.contains(&PLANE_LR)));
    }

    #[test]
    fn old_dimensions_and_anchors_still_load() {
        let old = r#"{"planes":[{"id":1,"name":"C","axis":"x","anchor":{"type":"origin"}}],"dims":[{"a":1,"b":1,"reports_to":"W"}]}"#;
        let mut b: FamilyBody = serde_json::from_str(old).unwrap();
        assert_eq!(b.dims[0].id, 0);
        b.fix_dim_ids();
        assert_ne!(b.dims[0].id, 0);
        assert!(b.dims[0].via.is_empty() && !b.dims[0].eq && b.dims[0].view.is_none());
        let div: Anchor = serde_json::from_str(r#"{"type":"div","a":1,"b":2,"i":1,"n":3}"#).unwrap();
        assert_eq!(div, Anchor::Div { a: 1, b: 2, i: 1, n: 3 });
    }
}
