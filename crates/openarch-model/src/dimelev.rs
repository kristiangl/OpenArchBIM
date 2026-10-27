//! Dimensions in elevation and section views.
//!
//! An elevation or section measures in its own frame: `u` across the view (along the frame's right axis from its
//! origin) and `z` up. A dimension placed there stores its references, picks, direction and line point in that
//! frame, so `measure` and the drawing work as they do in a plan. Its references are the lines the view names for
//! temporary dimensions (`ElevRef`: wall ends, tops and bottoms, the boundaries of walls seen end on, the edges, sills
//! and heads of openings, the edges of floors), held as `RefSpec::Elev`, plus levels and grids.
//!
//! Typing a value or holding a lock moves the element across the view (in the plan, along the view's right axis)
//! or up and down: a level changes its elevation, a door or window its sill, a wall its base, or only its top when
//! the top is what is measured, and the end of a wall seen across the view moves that end.

use std::collections::BTreeMap;

use crate::dimension::{DimCandidate, RefGeom};
use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::geom::Point;
use crate::site::{ElevFrame, ElevRef};
use crate::spot::{SpotSurface, SurfaceHit};
use crate::tempdim::RefSpec;

/// What an elevation or section view offers dimensions, in its own frame.
#[derive(Debug, Clone)]
pub struct ElevLines {
    pub frame: ElevFrame,
    pub refs: Vec<ElevRef>,
}

impl ElevLines {
    /// Where a reference is in the view, as a line.
    pub fn geom(&self, doc: &Document, spec: &RefSpec) -> Option<RefGeom> {
        let f = &self.frame;
        let (ul, ur) = (-f.left_mm, f.right_mm);
        match spec {
            RefSpec::Level { id } => {
                let z = doc.level(*id)?.elevation_mm;
                Some(RefGeom::Line(Point::new(ul, z), Point::new(ur, z)))
            }
            RefSpec::Grid { id } => {
                let (a, b) = doc.grid(*id)?.line()?;
                let u = self.u_of(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0));
                Some(RefGeom::Line(Point::new(u, f.z_min), Point::new(u, f.z_max)))
            }
            RefSpec::Elev { id, key } => {
                let r = self.refs.iter().find(|r| r.id == *id && r.key == *key)?;
                Some(if r.vertical { RefGeom::Line(Point::new(r.pos, r.lo), Point::new(r.pos, r.hi)) } else { RefGeom::Line(Point::new(r.lo, r.pos), Point::new(r.hi, r.pos)) })
            }
            _ => None,
        }
    }

    fn u_of(&self, p: Point) -> f64 {
        (p.x - self.frame.origin[0]) * self.frame.right[0] + (p.y - self.frame.origin[1]) * self.frame.right[1]
    }
}

impl Document {
    /// Whether dimensions in this view measure in an elevation or section frame.
    pub fn is_elevation_like(&self, view: Id) -> bool {
        self.elevation_frame(view).is_some()
    }

    /// The lines a dimension in an elevation or section view can measure to (levels and grids are added by
    /// `ElevLines::geom`). In a section every wall the view reaches counts, cut or seen.
    pub fn elevation_dim_lines(&self, view: Id) -> Option<ElevLines> {
        let frame = self.elevation_frame(view)?;
        let v = self.view(view)?;
        let hidden = |id: Id| v.hidden_elements.contains(&id);
        let pieces = self.wall_footprint_pieces(&hidden);
        let (o, look) = (Point::new(frame.origin[0], frame.origin[1]), Point::new(frame.look[0], frame.look[1]));
        let d_of = |p: &Point| (p.x - o.x) * look.x + (p.y - o.y) * look.y;
        let mut near: BTreeMap<Id, f64> = BTreeMap::new();
        for (id, ps) in &pieces {
            let ds: Vec<f64> = ps.iter().flatten().map(d_of).collect();
            let (lo, hi) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            if hi > 0.0 && lo < frame.depth_mm {
                near.insert(*id, lo.max(0.0));
            }
        }
        let refs = self.elevation_refs(&frame, &pieces, &near, &hidden);
        Some(ElevLines { frame, refs })
    }

    /// Lines near `point` (across the view, up) a dimension in an elevation or section can measure to, nearest first:
    /// the view's reference lines, then levels and grids.
    pub fn elevation_dim_candidates(&self, view: Id, point: Point, tol: f64) -> Vec<DimCandidate> {
        let Some(lines) = self.elevation_dim_lines(view) else { return vec![] };
        let hidden = |id: Id| self.view(view).is_some_and(|v| v.hidden_elements.contains(&id));
        let mut specs: Vec<(RefSpec, String)> = lines.refs.iter().map(|r| (RefSpec::Elev { id: r.id, key: r.key.clone() }, r.label.clone())).collect();
        specs.extend(self.levels().into_iter().filter(|(id, _)| !hidden(*id)).map(|(id, l)| (RefSpec::Level { id }, format!("Level: {}", l.name))));
        specs.extend(self.grids().into_iter().filter(|(id, _)| !hidden(*id)).map(|(id, g)| (RefSpec::Grid { id }, format!("Grid {}", g.name))));
        let mut out: Vec<DimCandidate> = vec![];
        for (spec, label) in specs {
            let Some(RefGeom::Line(a, b)) = lines.geom(self, &spec) else { continue };
            // a grid that runs across the view is not seen in it
            if matches!(spec, RefSpec::Grid { .. }) && !self.grid_seen_end_on(&lines.frame, &spec) {
                continue;
            }
            let dist = seg_dist(point, a, b);
            if dist <= tol {
                out.push(DimCandidate { spec, label, a: [a.x, a.y], b: [b.x, b.y], dist, arc: None });
            }
        }
        // at the same distance a face, edge or level comes before the end of a wall seen along the view
        let rank = |c: &DimCandidate| if c.label == "Wall end" { 1 } else { 0 };
        out.sort_by(|x, y| x.dist.total_cmp(&y.dist).then(rank(x).cmp(&rank(y))));
        out
    }

    /// The horizontal lines a spot elevation in an elevation or section can stand on near `p`, nearest first: the tops
    /// and bottoms of walls and floors, the sills and heads of openings, levels. `Top` names a top, a head or a level,
    /// `Bottom` a bottom or a sill.
    pub fn elevation_spot_hits(&self, view: Id, p: Point, tol: f64) -> Vec<SurfaceHit> {
        let Some(lines) = self.elevation_dim_lines(view) else { return vec![] };
        let mut out: Vec<SurfaceHit> = vec![];
        let up = [0.0, 0.0, 1.0];
        for r in lines.refs.iter().filter(|r| !r.vertical) {
            let Some(surface) = spot_surface_of(&r.key) else { continue };
            if p.x < r.lo - tol || p.x > r.hi + tol || (p.y - r.pos).abs() > tol {
                continue;
            }
            out.push(SurfaceHit { host: r.id, surface, label: format!("{} ({})", r.label, self.element_noun(r.id)), z: r.pos, normal: up, singular: false });
        }
        let hidden = |id: Id| self.view(view).is_some_and(|v| v.hidden_elements.contains(&id));
        for (id, l) in self.levels() {
            if !hidden(id) && (p.y - l.elevation_mm).abs() <= tol {
                out.push(SurfaceHit { host: id, surface: SpotSurface::Top, label: format!("Level: {}", l.name), z: l.elevation_mm, normal: up, singular: false });
            }
        }
        out.sort_by(|a, b| (a.z - p.y).abs().total_cmp(&(b.z - p.y).abs()));
        out
    }

    /// The line a spot elevation in an elevation or section stands on, wherever it is now.
    pub(crate) fn elevation_spot_hit(&self, view: Id, host: Id, surface: SpotSurface) -> Option<SurfaceHit> {
        let up = [0.0, 0.0, 1.0];
        if let Some(l) = self.level(host) {
            return Some(SurfaceHit { host, surface, label: format!("Level: {}", l.name), z: l.elevation_mm, normal: up, singular: false });
        }
        let lines = self.elevation_dim_lines(view)?;
        let r = lines.refs.iter().find(|r| r.id == host && !r.vertical && spot_surface_of(&r.key) == Some(surface))?;
        Some(SurfaceHit { host, surface, label: r.label.clone(), z: r.pos, normal: up, singular: false })
    }

    fn element_noun(&self, id: Id) -> &'static str {
        match self.get(id) {
            Some(Element::Wall(_)) => "Wall",
            Some(Element::Floor(_)) => "Floor",
            Some(Element::FamilyInstance(_)) => "Opening",
            _ => "Element",
        }
    }

    fn grid_seen_end_on(&self, f: &ElevFrame, spec: &RefSpec) -> bool {
        let RefSpec::Grid { id } = spec else { return false };
        let Some((a, b)) = self.grid(*id).and_then(|g| g.line()) else { return false };
        let len = a.dist(&b).max(1e-9);
        ((b.x - a.x) * f.right[0] + (b.y - a.y) * f.right[1]).abs() / len <= 0.15
    }

    /// Move what a reference in an elevation or section belongs to by `v` (across the view, up).
    pub(crate) fn move_elev_ref(&mut self, view: Id, spec: &RefSpec, v: Point) -> Result<(), String> {
        let frame = self.elevation_frame(view).ok_or("not an elevation or section view")?;
        let (du, dz) = (if v.x.abs() > 1e-9 { v.x } else { 0.0 }, if v.y.abs() > 1e-9 { v.y } else { 0.0 });
        if du == 0.0 && dz == 0.0 {
            return Ok(());
        }
        let id = spec.element();
        let side = Point::new(frame.right[0] * du, frame.right[1] * du);
        self.grouped("Move", |doc| {
            match spec {
                RefSpec::Level { .. } => {
                    if du != 0.0 {
                        return Err("a level only moves up and down".into());
                    }
                }
                RefSpec::Grid { .. } => {
                    if dz != 0.0 {
                        return Err("a grid only moves across the view".into());
                    }
                }
                RefSpec::Elev { .. } => {}
                _ => return Err("that line is not seen in this view".into()),
            }
            let key = if let RefSpec::Elev { key, .. } = spec { key.as_str() } else { "" };
            match doc.get(id).cloned() {
                Some(Element::Wall(w)) if du != 0.0 && key.starts_with("end:") && w.curve.is_none() => {
                    // the end of the wall that shows at that side of the view
                    let u = |p: Point| (p.x - frame.origin[0]) * frame.right[0] + (p.y - frame.origin[1]) * frame.right[1];
                    let start_is_min = u(w.start) <= u(w.end);
                    let end = if (key == "end:min") == start_is_min { 0 } else { 1 };
                    doc.move_ref_element(&RefSpec::WallEnd { id, end }, side)?;
                }
                Some(Element::Wall(w)) if dz != 0.0 && (key == "top" || key == "bottom") => {
                    doc.refuse_pinned(&[id], "moved")?;
                    // only the measured edge moves: the top changes the height, the bottom the base
                    let patch = if key == "top" {
                        if w.top_level.is_some() { serde_json::json!({ "top_offset_mm": w.top_offset_mm + dz }) } else { serde_json::json!({ "height_mm": w.height_mm + dz }) }
                    } else if w.top_level.is_some() {
                        serde_json::json!({ "base_offset_mm": w.base_offset_mm + dz })
                    } else {
                        serde_json::json!({ "base_offset_mm": w.base_offset_mm + dz, "height_mm": w.height_mm - dz })
                    };
                    let (z0, z1) = doc.wall_extent(&w);
                    if (key == "top" && z1 + dz <= z0 + 1.0) || (key == "bottom" && z0 + dz >= z1 - 1.0) {
                        return Err("the wall would have no height".into());
                    }
                    doc.patch_wall(id, &patch)?;
                }
                Some(_) => {
                    if du != 0.0 {
                        doc.transform_elements(&[id], Xform::Move { dx: side.x, dy: side.y }, EditOptions::default())?;
                    }
                    if dz != 0.0 {
                        doc.move_vertical(&[id], dz)?;
                    }
                }
                None => return Err("the measured element is gone".into()),
            }
            Ok(())
        })
    }
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-12 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    (p.x - a.x - dx * t).hypot(p.y - a.y - dy * t)
}

/// Which way a horizontal line in an elevation faces, for a spot standing on it.
fn spot_surface_of(key: &str) -> Option<SpotSurface> {
    match key {
        "top" | "head" => Some(SpotSurface::Top),
        "bottom" | "sill" => Some(SpotSurface::Bottom),
        _ => None,
    }
}
