//! The family editor's Modify tools: Move, Copy, Rotate, Mirror and Offset of what is picked in one view.
//!
//! A change is given in the view's own coordinates: `u` along its horizontal axis and `v` along its vertical one,
//! each a family coordinate (mm). Every stored point of an item is worked out as a family point, changed, and written
//! back. A coordinate locked to a reference plane keeps its lock when the change leaves it where it was, and becomes
//! a plain distance when it moves. What a change cannot keep exact is refused with the reason, and nothing changes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::famgeom::{fmt_num, Anchor, Axis, Coord, Env, FamilyBody, FormShape, Loop, Node, PlaneId, SegShape, SymView};

/// A change in the view's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FamXform {
    Move { du: f64, dv: f64 },
    /// Counter-clockwise as the family's axes run (u to the right, v up), about (cu, cv).
    Rotate { cu: f64, cv: f64, deg: f64 },
    /// Across the line through (au, av) and (bu, bv).
    Mirror { au: f64, av: f64, bu: f64, bv: f64 },
}

/// Something picked in the family editor, by the editor's own names for the kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum FamItem {
    Form(u32),
    Sym(u32),
    Mline(u32),
    Mtext(u32),
    Ctrl(u32),
    Conn(u32),
    Comp(u32),
    Plane(u32),
}

const EPS: f64 = 1e-6;
const OFF_PLANE: &str = "only works in a view that looks straight at its work plane";

fn index(a: Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// The view's axes: (horizontal, vertical, looking into it). The view's u and v are family coordinates along the first
/// two, whichever way round the view shows them.
fn view_axes(view: SymView) -> (Axis, Axis, Axis) {
    let n = view.normal();
    let (h, v) = n.sketch_axes();
    (h, v, n)
}

/// The change as an affine map of the view's (u, v): a 2x2 matrix and a shift.
#[derive(Clone, Copy)]
struct Map2 {
    m: [[f64; 2]; 2],
    t: [f64; 2],
}

impl Map2 {
    fn of(x: FamXform) -> Map2 {
        match x {
            FamXform::Move { du, dv } => Map2 { m: [[1.0, 0.0], [0.0, 1.0]], t: [du, dv] },
            FamXform::Rotate { cu, cv, deg } => {
                let (s, c) = deg.to_radians().sin_cos();
                Map2 { m: [[c, -s], [s, c]], t: [cu - c * cu + s * cv, cv - s * cu - c * cv] }
            }
            FamXform::Mirror { au, av, bu, bv } => {
                let (dx, dy) = (bu - au, bv - av);
                let l2 = (dx * dx + dy * dy).max(1e-12);
                let (a, b) = ((dx * dx - dy * dy) / l2, 2.0 * dx * dy / l2);
                // reflect about the line through the origin with the same direction, then put the line back
                Map2 { m: [[a, b], [b, -a]], t: [au - a * au - b * av, av - b * au + a * av] }
            }
        }
    }
    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.t[0], self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.t[1]]
    }
    fn det(&self) -> f64 {
        self.m[0][0] * self.m[1][1] - self.m[0][1] * self.m[1][0]
    }
}

/// Works out and writes back points for one change in one view.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    pos: &'a BTreeMap<PlaneId, f64>,
    map: Map2,
    h: Axis,
    v: Axis,
    n: Axis,
}

impl Ctx<'_> {
    fn val(&self, c: &Coord) -> f64 {
        c.plane.and_then(|p| self.pos.get(&p)).copied().unwrap_or(0.0) + c.off
    }
    /// A coordinate after the change: as it was when the change leaves it in place, else a plain distance.
    fn coord(&self, c: &Coord, new: f64) -> Coord {
        if (self.val(c) - new).abs() < EPS {
            *c
        } else {
            Coord::at((new * 1e6).round() / 1e6)
        }
    }
    /// A family point changed: only its parts along the view's two axes move.
    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let (hi, vi) = (index(self.h), index(self.v));
        let q = self.map.apply([p[hi], p[vi]]);
        let mut out = p;
        out[hi] = q[0];
        out[vi] = q[1];
        out
    }
    /// The same Move without its part along `a` (a Move seen edge on: that part shifts an extrusion's ends instead).
    fn without(&self, a: Axis) -> Self {
        let mut c = *self;
        if a == self.h {
            c.map.t[0] = 0.0;
        } else if a == self.v {
            c.map.t[1] = 0.0;
        }
        c
    }
    /// The sketch plane's axis and position for an item drawn on plane `pl`.
    fn plane(&self, body: &FamilyBody, pl: PlaneId) -> Result<(Axis, f64), String> {
        let p = body.plane(pl).ok_or("the item's work plane is missing")?;
        Ok((p.axis, self.pos.get(&pl).copied().unwrap_or(0.0)))
    }
    /// A node of a sketch on a plane (axis `np` at `w0`) changed; None when the change would take it off the plane.
    fn node(&self, n: &Node, np: Axis, w0: f64) -> Option<Node> {
        let p = self.point(np.point(self.val(&n.u), self.val(&n.v), w0));
        if (p[index(np)] - w0).abs() > EPS {
            return None;
        }
        let (a1, a2) = np.sketch_axes();
        Some(Node { u: self.coord(&n.u, p[index(a1)]), v: self.coord(&n.v, p[index(a2)]) })
    }
    /// Whether the change turns a sketch over (arcs then run the other way).
    fn flips(&self) -> bool {
        self.map.det() < 0.0
    }
    fn lp(&self, l: &Loop, np: Axis, w0: f64) -> Option<Loop> {
        let nodes = l.nodes.iter().map(|n| self.node(n, np, w0)).collect::<Option<Vec<_>>>()?;
        let flip = self.flips();
        let bulges = l.bulges.iter().map(|b| if flip { -b } else { *b }).collect();
        let mut shapes = vec![];
        for s in &l.shapes {
            shapes.push(match s {
                None => None,
                Some(SegShape::Spline { through }) => Some(SegShape::Spline { through: through.iter().map(|n| self.node(n, np, w0)).collect::<Option<Vec<_>>>()? }),
                Some(SegShape::Ellipse { centre, rx, ry, rotation_deg, ccw }) => {
                    // the major axis turns as the change turns a direction in the sketch
                    let (a1, a2) = np.sketch_axes();
                    let r = rotation_deg.to_radians();
                    let c = np.point(self.val(&centre.u), self.val(&centre.v), w0);
                    let mut tip = c;
                    tip[index(a1)] += r.cos();
                    tip[index(a2)] += r.sin();
                    let (c2, t2) = (self.point(c), self.point(tip));
                    let ang = (t2[index(a2)] - c2[index(a2)]).atan2(t2[index(a1)] - c2[index(a1)]).to_degrees();
                    Some(SegShape::Ellipse { centre: self.node(centre, np, w0)?, rx: *rx, ry: *ry, rotation_deg: (ang * 1e6).round() / 1e6, ccw: if flip { !ccw } else { *ccw } })
                }
            });
        }
        Some(Loop { nodes, bulges, closed: l.closed, shapes })
    }
}

fn what(x: &FamXform) -> &'static str {
    match x {
        FamXform::Move { .. } => "Moving",
        FamXform::Rotate { .. } => "Rotating",
        FamXform::Mirror { .. } => "Mirroring",
    }
}

fn norm_deg(d: f64) -> f64 {
    let r = d.rem_euclid(360.0);
    if (r - 360.0).abs() < 1e-9 {
        0.0
    } else {
        (r * 1e6).round() / 1e6
    }
}

/// An extrusion end moved by `d`: a number stays a number, a formula gains the distance.
fn add_dist(e: &str, d: f64) -> String {
    match e.trim().parse::<f64>() {
        Ok(v) => fmt_num(v + d),
        Err(_) => format!("({}) + {}", e.trim(), fmt_num(d)),
    }
}

impl FamilyBody {
    /// Change `items` as the view `view` sees them; with `copy` the changed ones are new copies and the picked ones
    /// stay. Returns what is now selected (the copies, or the items). Nothing changes when it is refused.
    pub fn transform_items(&mut self, items: &[FamItem], view: SymView, x: FamXform, copy: bool, env: &mut Env) -> Result<Vec<FamItem>, String> {
        let saved = self.clone();
        let r = self.transform_inner(items, view, x, copy, env).and_then(|out| self.solve(env).map(|_| out));
        if r.is_err() {
            *self = saved;
        }
        r
    }

    fn transform_inner(&mut self, items: &[FamItem], view: SymView, x: FamXform, copy: bool, env: &mut Env) -> Result<Vec<FamItem>, String> {
        // copies first (where the originals are), then everything changes with the planes as they then stand
        let mut picked = items.to_vec();
        if copy {
            let pos0 = self.solve_planes(env)?;
            picked = picked.into_iter().map(|it| self.copy_item(it, &pos0)).collect::<Result<_, _>>()?;
        }
        let pos = self.solve_planes(env)?;
        let (h, v, n) = view_axes(view);
        let cx = Ctx { pos: &pos, map: Map2::of(x), h, v, n };
        for &it in &picked {
            self.change_item(it, &cx, x)?;
        }
        Ok(picked)
    }

    /// A copy of an item, with a new id, added to the family.
    fn copy_item(&mut self, it: FamItem, pos: &BTreeMap<PlaneId, f64>) -> Result<FamItem, String> {
        let id = self.take_id();
        let gone = || "something picked is no longer there".to_string();
        Ok(match it {
            FamItem::Form(i) => {
                let mut f = self.forms.iter().find(|f| f.id == i).cloned().ok_or_else(gone)?;
                f.id = id;
                f.cut_by.clear();
                self.forms.push(f);
                FamItem::Form(id)
            }
            FamItem::Sym(i) => {
                let mut s = self.syms.iter().find(|s| s.id == i).cloned().ok_or_else(gone)?;
                s.id = id;
                self.syms.push(s);
                FamItem::Sym(id)
            }
            FamItem::Mline(i) => {
                let mut m = self.model_lines.iter().find(|m| m.id == i).cloned().ok_or_else(gone)?;
                m.id = id;
                self.model_lines.push(m);
                FamItem::Mline(id)
            }
            FamItem::Mtext(i) => {
                let mut t = self.model_texts.iter().find(|t| t.id == i).cloned().ok_or_else(gone)?;
                t.id = id;
                self.model_texts.push(t);
                FamItem::Mtext(id)
            }
            FamItem::Ctrl(i) => {
                let mut c = self.controls.iter().find(|c| c.id == i).cloned().ok_or_else(gone)?;
                c.id = id;
                self.controls.push(c);
                FamItem::Ctrl(id)
            }
            FamItem::Conn(i) => {
                let mut c = self.connectors.iter().find(|c| c.id == i).cloned().ok_or_else(gone)?;
                c.id = id;
                self.connectors.push(c);
                FamItem::Conn(id)
            }
            FamItem::Comp(i) => {
                let mut c = self.nested.iter().find(|c| c.id == i).cloned().ok_or_else(gone)?;
                c.id = id;
                self.nested.push(c);
                FamItem::Comp(id)
            }
            FamItem::Plane(i) => {
                // a copy of a plane stands on its own: an offset from the origin plane of its axis
                let p = self.plane(i).cloned().ok_or_else(gone)?;
                let origin = self.planes.iter().find(|q| q.axis == p.axis && matches!(q.anchor, Anchor::Origin)).map(|q| q.id).ok_or("no origin plane on that axis")?;
                let mut k = 2;
                let mut name = format!("{} {k}", p.name);
                while self.planes.iter().any(|q| q.name == name) {
                    k += 1;
                    name = format!("{} {k}", p.name);
                }
                let mut np = p.clone();
                np.id = id;
                np.name = name;
                np.anchor = Anchor::Offset { from: origin, by: fmt_num(pos.get(&i).copied().unwrap_or(0.0)) };
                self.planes.push(np);
                FamItem::Plane(id)
            }
        })
    }

    fn change_item(&mut self, it: FamItem, cx: &Ctx, x: FamXform) -> Result<(), String> {
        let gone = || "something picked is no longer there".to_string();
        // how far a Move goes along a family axis
        let along = |a: Axis| -> f64 {
            match x {
                FamXform::Move { du, .. } if a == cx.h => du,
                FamXform::Move { dv, .. } if a == cx.v => dv,
                _ => 0.0,
            }
        };
        match it {
            FamItem::Form(i) => {
                let k = self.forms.iter().position(|f| f.id == i).ok_or_else(gone)?;
                let mut f = self.forms[k].clone();
                let (np, w0) = cx.plane(self, f.shape.plane())?;
                let name = if f.name.is_empty() { "the form".to_string() } else { format!("'{}'", f.name) };
                // seen edge on, a Move toward or away from the work plane shifts an extrusion's or blend's ends; the
                // rest of the Move goes to its sketch
                let d = if np != cx.n { along(np) } else { 0.0 };
                if d.abs() > EPS {
                    match &mut f.shape {
                        FormShape::Extrusion { start, end, start_lock, end_lock, .. } | FormShape::Blend { start, end, start_lock, end_lock, .. } => {
                            if start_lock.is_some() || end_lock.is_some() {
                                return Err(format!("{name} has an end locked to a reference plane: unlock it to move it this way"));
                            }
                            *start = add_dist(start, d);
                            *end = add_dist(end, d);
                        }
                        _ => return Err(format!("{name} is drawn on its work plane: move it in a view that looks straight at the plane")),
                    }
                }
                let c = if d.abs() > EPS { cx.without(np) } else { *cx };
                let off = || format!("{} {name} {OFF_PLANE}", what(&x));
                let lp = |l: &Loop| c.lp(l, np, w0).ok_or_else(off);
                match &mut f.shape {
                    FormShape::Extrusion { profile, .. } => *profile = profile.iter().map(&lp).collect::<Result<_, _>>()?,
                    FormShape::Blend { base, top, .. } => {
                        *base = base.iter().map(&lp).collect::<Result<_, _>>()?;
                        *top = top.iter().map(&lp).collect::<Result<_, _>>()?;
                    }
                    FormShape::Revolve { axis_a, axis_b, profile, .. } => {
                        *profile = profile.iter().map(&lp).collect::<Result<_, _>>()?;
                        *axis_a = c.node(axis_a, np, w0).ok_or_else(off)?;
                        *axis_b = c.node(axis_b, np, w0).ok_or_else(off)?;
                    }
                    FormShape::Sweep { path, flip, .. } | FormShape::SweptBlend { path, flip, .. } => {
                        *path = lp(path)?;
                        // a mirrored path carries its profile mirrored too
                        if c.flips() {
                            *flip = !*flip;
                        }
                    }
                }
                self.forms[k] = f;
            }
            FamItem::Mline(i) => {
                let k = self.model_lines.iter().position(|m| m.id == i).ok_or_else(gone)?;
                let (np, w0) = cx.plane(self, self.model_lines[k].plane)?;
                self.model_lines[k].path = cx.lp(&self.model_lines[k].path, np, w0).ok_or_else(|| format!("{} a model line {OFF_PLANE}", what(&x)))?;
            }
            FamItem::Sym(i) => {
                let k = self.syms.iter().position(|s| s.id == i).ok_or_else(gone)?;
                if self.syms[k].view.normal() != cx.n {
                    return Err("symbolic lines and regions change in the view they belong to".into());
                }
                // a symbolic item's points are the view's own coordinates: as if drawn on a plane facing the view
                self.syms[k].path = cx.lp(&self.syms[k].path, cx.n, 0.0).ok_or("a symbolic line could not be changed")?;
            }
            FamItem::Ctrl(i) => {
                let k = self.controls.iter().position(|c| c.id == i).ok_or_else(gone)?;
                if self.controls[k].view.normal() != cx.n {
                    return Err("a control changes in the view it belongs to".into());
                }
                let c = &self.controls[k];
                let nd = cx.node(&Node { u: c.u, v: c.v }, cx.n, 0.0).ok_or("a control could not be changed")?;
                self.controls[k].u = nd.u;
                self.controls[k].v = nd.v;
            }
            FamItem::Conn(i) => {
                let k = self.connectors.iter().position(|c| c.id == i).ok_or_else(gone)?;
                let (np, w0) = cx.plane(self, self.connectors[k].plane)?;
                let c = &self.connectors[k];
                let nd = cx.node(&Node { u: c.u, v: c.v }, np, w0).ok_or_else(|| format!("{} a connector {OFF_PLANE}", what(&x)))?;
                self.connectors[k].u = nd.u;
                self.connectors[k].v = nd.v;
            }
            FamItem::Mtext(i) => {
                if !matches!(x, FamXform::Move { .. }) {
                    return Err("Model Text can be moved and copied; it does not turn or mirror yet".into());
                }
                let k = self.model_texts.iter().position(|t| t.id == i).ok_or_else(gone)?;
                let (np, w0) = cx.plane(self, self.model_texts[k].plane)?;
                let t = &self.model_texts[k];
                let nd = cx.node(&Node { u: t.u, v: t.v }, np, w0).ok_or_else(|| format!("{} Model Text {OFF_PLANE}", what(&x)))?;
                self.model_texts[k].u = nd.u;
                self.model_texts[k].v = nd.v;
            }
            FamItem::Comp(i) => {
                let k = self.nested.iter().position(|c| c.id == i).ok_or_else(gone)?;
                let c = self.nested[k].clone();
                let p = cx.point([cx.val(&c.origin[0]), cx.val(&c.origin[1]), cx.val(&c.origin[2])]);
                let mut nc = c.clone();
                for (a, o) in nc.origin.iter_mut().enumerate() {
                    *o = cx.coord(&c.origin[a], p[a]);
                }
                match x {
                    FamXform::Move { .. } => {}
                    FamXform::Rotate { deg, .. } => {
                        if cx.n != Axis::Z {
                            return Err("a component turns in a plan view".into());
                        }
                        nc.rotation_deg = norm_deg(c.rotation_deg + deg);
                    }
                    FamXform::Mirror { au, av, bu, bv } => {
                        if cx.n != Axis::Z {
                            return Err("a component mirrors in a plan view".into());
                        }
                        // the mirror across a line at angle phi of a turn r is the turn 2 phi - r + 180 of a left-right mirror
                        let phi = (bv - av).atan2(bu - au).to_degrees();
                        nc.rotation_deg = norm_deg(2.0 * phi - c.rotation_deg + 180.0);
                        nc.mirror = !c.mirror;
                    }
                }
                self.nested[k] = nc;
            }
            FamItem::Plane(i) => {
                let p = self.plane(i).cloned().ok_or_else(gone)?;
                if matches!(p.anchor, Anchor::Origin) {
                    return Err(format!("'{}' is an origin plane and stays where it is", p.name));
                }
                let at = cx.pos.get(&i).copied().unwrap_or(0.0);
                if p.axis == cx.n {
                    return Err(format!("'{}' faces this view: change it in a view that sees it edge on", p.name));
                }
                // two points on the plane, changed: it goes there if it is still square to its axis
                let other = if p.axis == cx.h { cx.v } else { cx.h };
                let mut q = [0.0; 3];
                q[index(p.axis)] = at;
                let mut q2 = q;
                q2[index(other)] = 1000.0;
                let (r, r2) = (cx.point(q), cx.point(q2));
                if (r[index(p.axis)] - r2[index(p.axis)]).abs() > EPS {
                    return Err(format!("reference planes stay square to the family's axes: '{}' cannot turn that way", p.name));
                }
                let to = r[index(p.axis)];
                if (to - at).abs() > EPS {
                    self.set_plane_at(i, to, cx.pos)?;
                }
            }
        }
        Ok(())
    }

    /// Put a plane at `to` by its own offset (refused while a dimension holds it or a formula places it).
    fn set_plane_at(&mut self, id: PlaneId, to: f64, pos: &BTreeMap<PlaneId, f64>) -> Result<(), String> {
        let i = self.planes.iter().position(|q| q.id == id).ok_or("no such plane")?;
        let name = self.planes[i].name.clone();
        if self.dims.iter().any(|d| d.drives.contains(&id)) {
            return Err(format!("'{name}' is held by a dimension; change or unlock the dimension"));
        }
        let Anchor::Offset { from, by } = self.planes[i].anchor.clone() else {
            return Err(format!("'{name}' cannot be moved on its own"));
        };
        if by.trim().parse::<f64>().is_err() {
            return Err(format!("'{name}' is positioned by a formula ({by})"));
        }
        self.planes[i].anchor = Anchor::Offset { from, by: fmt_num(to - pos.get(&from).copied().unwrap_or(0.0)) };
        Ok(())
    }

    /// Offset a model line or symbolic line by `dist` to the side of it `toward` lies on (the view's u, v), in a view
    /// that looks straight at it: the line moves there, or with `copy` a copy goes there. Lines and arcs; where two
    /// straight pieces meet they are trimmed or extended to meet again, and arcs keep their centres.
    pub fn offset_item(&mut self, it: FamItem, view: SymView, dist: f64, toward: [f64; 2], copy: bool, env: &mut Env) -> Result<FamItem, String> {
        let saved = self.clone();
        let r = self.offset_inner(it, view, dist, toward, copy, env).and_then(|out| self.solve(env).map(|_| out));
        if r.is_err() {
            *self = saved;
        }
        r
    }

    fn offset_inner(&mut self, it: FamItem, view: SymView, dist: f64, toward: [f64; 2], copy: bool, env: &mut Env) -> Result<FamItem, String> {
        if !(dist.is_finite() && dist > 0.0) {
            return Err("the offset must be a distance above zero".into());
        }
        let pos = self.solve_planes(env)?;
        let (h, v, n) = view_axes(view);
        let cx = Ctx { pos: &pos, map: Map2::of(FamXform::Move { du: 0.0, dv: 0.0 }), h, v, n };
        // the path in the view's coordinates
        let (path, np) = match it {
            FamItem::Mline(i) => {
                let m = self.model_lines.iter().find(|m| m.id == i).ok_or("something picked is no longer there")?;
                let (np, _) = cx.plane(self, m.plane)?;
                (m.path.clone(), np)
            }
            FamItem::Sym(i) => {
                let sy = self.syms.iter().find(|s| s.id == i).ok_or("something picked is no longer there")?;
                if sy.view.normal() != n {
                    return Err("symbolic lines and regions change in the view they belong to".into());
                }
                (sy.path.clone(), n)
            }
            _ => return Err("Offset works on model lines and symbolic lines".into()),
        };
        if np != n {
            return Err(format!("Offsetting a line {OFF_PLANE}"));
        }
        if path.shapes.iter().any(|s| s.is_some()) {
            return Err("Offset works on lines and arcs, not splines or ellipses".into());
        }
        let pts: Vec<[f64; 2]> = path.nodes.iter().map(|q| [cx.val(&q.u), cx.val(&q.v)]).collect();
        let out = offset_polyline(&pts, &path.bulges, path.closed, dist, toward)?;
        let new_path = Loop { nodes: out.iter().map(|p| Node { u: Coord::at(round6(p[0])), v: Coord::at(round6(p[1])) }).collect(), bulges: path.bulges.clone(), closed: path.closed, shapes: vec![] };
        let it = if copy { self.copy_item(it, &pos)? } else { it };
        match it {
            FamItem::Mline(i) => self.model_lines.iter_mut().find(|m| m.id == i).unwrap().path = new_path,
            FamItem::Sym(i) => self.syms.iter_mut().find(|s| s.id == i).unwrap().path = new_path,
            _ => {}
        }
        Ok(it)
    }
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// A chain of straight pieces and arcs (bulge per piece: tan of a quarter of the arc's angle, positive
/// counter-clockwise) offset by `d` to the side `toward` is on. Straight pieces meeting at an angle are cut back or run
/// on to meet; an arc keeps its centre and angle (so its bulge), its radius growing or shrinking.
fn offset_polyline(p: &[[f64; 2]], bulges: &[f64], closed: bool, d: f64, toward: [f64; 2]) -> Result<Vec<[f64; 2]>, String> {
    let n = p.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    if segs == 0 {
        return Err("there is no line to offset".into());
    }
    // each piece: its ends, and for an arc its centre, radius and turn (+1 counter-clockwise)
    struct Seg {
        a: [f64; 2],
        c: [f64; 2],
        arc: Option<([f64; 2], f64, f64)>,
    }
    let mut segv = vec![];
    for i in 0..segs {
        let (a, c) = (p[i], p[(i + 1) % n]);
        let (dx, dy) = (c[0] - a[0], c[1] - a[1]);
        let len = dx.hypot(dy);
        if len < 1e-9 {
            return Err("a line of the chain has no length".into());
        }
        let bu = bulges.get(i).copied().unwrap_or(0.0);
        let arc = if bu.abs() < 1e-12 {
            None
        } else {
            let ang = 4.0 * bu.atan();
            let r = len / (2.0 * (ang / 2.0).sin().abs());
            let mid = [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0];
            // the centre sits off the chord's middle, to the left for a counter-clockwise arc of less than half a turn
            let h = r * (ang / 2.0).cos();
            let left = [-dy / len, dx / len];
            let k = if bu > 0.0 { h } else { -h };
            Some(([mid[0] + left[0] * k, mid[1] + left[1] * k], r, bu.signum()))
        };
        segv.push(Seg { a, c, arc });
    }
    // the side: of the piece nearest `toward`, whether it is on the left going along the chain
    let mut best = (f64::INFINITY, true);
    for sg in &segv {
        let (dist, left) = match sg.arc {
            None => {
                let (dx, dy) = (sg.c[0] - sg.a[0], sg.c[1] - sg.a[1]);
                let t = (((toward[0] - sg.a[0]) * dx + (toward[1] - sg.a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((toward[0] - sg.a[0] - dx * t).hypot(toward[1] - sg.a[1] - dy * t), dx * (toward[1] - sg.a[1]) - dy * (toward[0] - sg.a[0]) >= 0.0)
            }
            Some((cen, r, turn)) => {
                let dc = (toward[0] - cen[0]).hypot(toward[1] - cen[1]);
                // the left of a counter-clockwise arc is its inside
                ((dc - r).abs(), (dc < r) == (turn > 0.0))
            }
        };
        if dist < best.0 {
            best = (dist, left);
        }
    }
    let s = if best.1 { d } else { -d };
    // each piece offset: its two ends, and its direction at each end
    struct Piece {
        a: [f64; 2],
        c: [f64; 2],
        ta: [f64; 2],
        tc: [f64; 2],
        arc: bool,
    }
    let mut pieces = vec![];
    for sg in &segv {
        match sg.arc {
            None => {
                let (dx, dy) = (sg.c[0] - sg.a[0], sg.c[1] - sg.a[1]);
                let len = dx.hypot(dy);
                let (nx, ny) = (-dy / len * s, dx / len * s);
                let t = [dx / len, dy / len];
                pieces.push(Piece { a: [sg.a[0] + nx, sg.a[1] + ny], c: [sg.c[0] + nx, sg.c[1] + ny], ta: t, tc: t, arc: false });
            }
            Some((cen, r, turn)) => {
                let nr = if turn > 0.0 { r - s } else { r + s };
                if nr <= 1e-6 {
                    return Err("an arc would shrink to nothing at that offset".into());
                }
                let at = |q: [f64; 2]| {
                    let (ux, uy) = (q[0] - cen[0], q[1] - cen[1]);
                    let l = ux.hypot(uy);
                    [cen[0] + ux / l * nr, cen[1] + uy / l * nr]
                };
                let tan = |q: [f64; 2]| {
                    let (ux, uy) = ((q[0] - cen[0]) / r, (q[1] - cen[1]) / r);
                    if turn > 0.0 { [-uy, ux] } else { [uy, -ux] }
                };
                pieces.push(Piece { a: at(sg.a), c: at(sg.c), ta: tan(sg.a), tc: tan(sg.c), arc: true });
            }
        }
    }
    // where consecutive pieces meet: the crossing of two straight ones, the shared point of ones that run on smoothly
    let meet = |x: &Piece, y: &Piece| -> Result<[f64; 2], String> {
        let cross = x.tc[0] * y.ta[1] - x.tc[1] * y.ta[0];
        if cross.abs() < 1e-6 {
            return Ok([(x.c[0] + y.a[0]) / 2.0, (x.c[1] + y.a[1]) / 2.0]);
        }
        if x.arc || y.arc {
            return Err("Offset works where an arc meets the next piece smoothly; this chain has a corner at an arc".into());
        }
        let t = ((y.a[0] - x.c[0]) * y.ta[1] - (y.a[1] - x.c[1]) * y.ta[0]) / cross;
        Ok([x.c[0] + x.tc[0] * t, x.c[1] + x.tc[1] * t])
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let q = if closed {
            meet(&pieces[(i + segs - 1) % segs], &pieces[i % segs])?
        } else if i == 0 {
            pieces[0].a
        } else if i == n - 1 {
            pieces[segs - 1].c
        } else {
            meet(&pieces[i - 1], &pieces[i])?
        };
        out.push(q);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::famgeom::{Form, ModelLine, SymElement, Visibility, PLANE_LEVEL, PLANE_LR};

    fn env() -> impl FnMut(&str) -> Result<openarch_params::Value, openarch_params::EvalError> {
        |n: &str| match n {
            "Width" => Ok(openarch_params::Value::Num(600.0)),
            other => Err(openarch_params::EvalError::UnknownName(other.into())),
        }
    }
    fn line(pts: &[[f64; 2]], closed: bool) -> Loop {
        Loop { nodes: pts.iter().map(|p| Node { u: Coord::at(p[0]), v: Coord::at(p[1]) }).collect(), bulges: vec![0.0; pts.len()], closed, shapes: vec![] }
    }
    fn mline(b: &mut FamilyBody, pts: &[[f64; 2]]) -> u32 {
        let id = b.take_id();
        b.model_lines.push(ModelLine { id, name: String::new(), plane: PLANE_LEVEL, path: line(pts, false), subcategory: String::new(), vis: Visibility::default() });
        id
    }
    fn pts(b: &FamilyBody, id: u32) -> Vec<[f64; 2]> {
        let pos = b.solve_planes(&mut env()).unwrap();
        let m = b.model_lines.iter().find(|m| m.id == id).unwrap();
        let v = |c: &Coord| c.plane.map(|p| pos[&p]).unwrap_or(0.0) + c.off;
        m.path.nodes.iter().map(|n| [round6(v(&n.u)), round6(v(&n.v))]).collect()
    }

    #[test]
    fn move_copy_rotate_and_mirror_a_model_line_in_plan() {
        let mut b = FamilyBody::template();
        let id = mline(&mut b, &[[0.0, 0.0], [1000.0, 0.0]]);
        b.transform_items(&[FamItem::Mline(id)], SymView::Plan, FamXform::Move { du: 100.0, dv: 50.0 }, false, &mut env()).unwrap();
        assert_eq!(pts(&b, id), vec![[100.0, 50.0], [1100.0, 50.0]]);
        let c = b.transform_items(&[FamItem::Mline(id)], SymView::Plan, FamXform::Move { du: 0.0, dv: 500.0 }, true, &mut env()).unwrap();
        let FamItem::Mline(cid) = c[0] else { panic!() };
        assert_ne!(cid, id);
        assert_eq!(pts(&b, id), vec![[100.0, 50.0], [1100.0, 50.0]], "the original stays");
        assert_eq!(pts(&b, cid), vec![[100.0, 550.0], [1100.0, 550.0]]);
        b.transform_items(&[FamItem::Mline(cid)], SymView::Plan, FamXform::Rotate { cu: 100.0, cv: 550.0, deg: 90.0 }, false, &mut env()).unwrap();
        assert_eq!(pts(&b, cid), vec![[100.0, 550.0], [100.0, 1550.0]]);
        b.transform_items(&[FamItem::Mline(cid)], SymView::Plan, FamXform::Mirror { au: 0.0, av: 0.0, bu: 0.0, bv: 1.0 }, false, &mut env()).unwrap();
        assert_eq!(pts(&b, cid), vec![[-100.0, 550.0], [-100.0, 1550.0]]);
        // a plan line seen edge on in the front view moves across, not up off its plane
        assert!(b.transform_items(&[FamItem::Mline(cid)], SymView::Front, FamXform::Move { du: 0.0, dv: 100.0 }, false, &mut env()).is_err());
        b.transform_items(&[FamItem::Mline(cid)], SymView::Front, FamXform::Move { du: 50.0, dv: 0.0 }, false, &mut env()).unwrap();
        assert_eq!(pts(&b, cid), vec![[-50.0, 550.0], [-50.0, 1550.0]]);
    }

    #[test]
    fn a_lock_to_a_plane_stays_when_the_change_leaves_it_in_place() {
        let mut b = FamilyBody::template();
        let r = b.add_offset_plane("Right", PLANE_LR, "300").unwrap();
        let id = b.take_id();
        let path = Loop { nodes: vec![Node { u: Coord::on(r), v: Coord::at(0.0) }, Node { u: Coord::on(r), v: Coord::at(500.0) }], bulges: vec![0.0, 0.0], closed: false, shapes: vec![] };
        b.model_lines.push(ModelLine { id, name: String::new(), plane: PLANE_LEVEL, path, subcategory: String::new(), vis: Visibility::default() });
        // up and down: still on the Right plane
        b.transform_items(&[FamItem::Mline(id)], SymView::Plan, FamXform::Move { du: 0.0, dv: 100.0 }, false, &mut env()).unwrap();
        assert_eq!(b.model_lines[0].path.nodes[0].u.plane, Some(r));
        // across: off it, a plain distance
        b.transform_items(&[FamItem::Mline(id)], SymView::Plan, FamXform::Move { du: 20.0, dv: 0.0 }, false, &mut env()).unwrap();
        assert_eq!(b.model_lines[0].path.nodes[0].u, Coord::at(320.0));
    }

    #[test]
    fn planes_move_and_mirror_square_and_refuse_to_turn() {
        let mut b = FamilyBody::template();
        let r = b.add_offset_plane("Right", PLANE_LR, "300").unwrap();
        b.transform_items(&[FamItem::Plane(r)], SymView::Plan, FamXform::Move { du: 100.0, dv: 40.0 }, false, &mut env()).unwrap();
        assert_eq!(b.solve_planes(&mut env()).unwrap()[&r], 400.0);
        b.transform_items(&[FamItem::Plane(r)], SymView::Plan, FamXform::Mirror { au: 0.0, av: 0.0, bu: 0.0, bv: 10.0 }, false, &mut env()).unwrap();
        assert_eq!(b.solve_planes(&mut env()).unwrap()[&r], -400.0);
        assert!(b.transform_items(&[FamItem::Plane(r)], SymView::Plan, FamXform::Rotate { cu: 0.0, cv: 0.0, deg: 30.0 }, false, &mut env()).is_err());
        let c = b.transform_items(&[FamItem::Plane(r)], SymView::Plan, FamXform::Move { du: -100.0, dv: 0.0 }, true, &mut env()).unwrap();
        let FamItem::Plane(cp) = c[0] else { panic!() };
        assert_eq!(b.solve_planes(&mut env()).unwrap()[&cp], -500.0);
        assert!(b.transform_items(&[FamItem::Plane(PLANE_LR)], SymView::Plan, FamXform::Move { du: 10.0, dv: 0.0 }, false, &mut env()).is_err());
    }

    #[test]
    fn an_extrusion_moved_up_in_the_front_view_shifts_its_ends() {
        let mut b = FamilyBody::template();
        let shape = FormShape::Extrusion { plane: PLANE_LEVEL, start: "0".into(), end: "Width".into(), start_lock: None, end_lock: None, profile: vec![line(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], true)] };
        let f = b.add_form(Form { id: 0, name: "Box".into(), shape, void: false, cuts_host: true, cut_by: vec![], opening: false, material: String::new(), material_param: None, subcategory: String::new(), vis: Visibility::default() });
        b.transform_items(&[FamItem::Form(f)], SymView::Front, FamXform::Move { du: 10.0, dv: 250.0 }, false, &mut env()).unwrap();
        let FormShape::Extrusion { start, end, profile, .. } = &b.forms[0].shape else { panic!() };
        assert_eq!((start.as_str(), end.as_str()), ("250", "(Width) + 250"));
        assert_eq!(profile[0].nodes[0].u, Coord::at(10.0));
        // a turn seen edge on is refused, and nothing changes
        let before = b.clone();
        assert!(b.transform_items(&[FamItem::Form(f)], SymView::Front, FamXform::Rotate { cu: 0.0, cv: 0.0, deg: 30.0 }, false, &mut env()).is_err());
        assert_eq!(b, before);
    }

    #[test]
    fn offset_runs_straight_lines_on_to_meet_and_keeps_arcs_round() {
        let sq = offset_polyline(&[[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0]], &[0.0; 4], true, 100.0, [500.0, 500.0]).unwrap();
        assert_eq!(sq.iter().map(|p| [round6(p[0]), round6(p[1])]).collect::<Vec<_>>(), vec![[100.0, 100.0], [900.0, 100.0], [900.0, 900.0], [100.0, 900.0]]);
        let out = offset_polyline(&[[0.0, 0.0], [1000.0, 0.0]], &[0.0], false, 50.0, [500.0, -10.0]).unwrap();
        assert_eq!(out, vec![[0.0, -50.0], [1000.0, -50.0]]);
        // a half circle (bulge 1) from (-500, 0) to (500, 0) counter-clockwise below the chord: inside shrinks
        let arc = offset_polyline(&[[-500.0, 0.0], [500.0, 0.0]], &[1.0], false, 100.0, [0.0, -100.0]).unwrap();
        assert!((arc[0][0] + 400.0).abs() < 1e-6 && (arc[1][0] - 400.0).abs() < 1e-6, "{arc:?}");
        let mut b = FamilyBody::template();
        let id = b.take_id();
        b.syms.push(SymElement { id, view: SymView::Plan, path: line(&[[0.0, 0.0], [1000.0, 0.0]], false), filled: false, masking: false, subcategory: String::new(), vis: Visibility::default() });
        let c = b.offset_item(FamItem::Sym(id), SymView::Plan, 200.0, [0.0, 300.0], true, &mut env()).unwrap();
        let FamItem::Sym(cid) = c else { panic!() };
        assert_eq!(b.syms.iter().find(|s| s.id == cid).unwrap().path.nodes[1].v, Coord::at(200.0));
        assert!(b.offset_item(FamItem::Sym(id), SymView::Front, 200.0, [0.0, 300.0], false, &mut env()).is_err());
    }
}
