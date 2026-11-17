//! Plan symbols for hosted doors and windows: the lines, arcs and outlines a
//! plan view draws for an instance.

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;
use crate::view::DetailLevel;

#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub centre: Point,
    pub radius: f64,
    /// Degrees counter-clockwise from +x; the arc sweeps from `from` up to `to`.
    pub from: f64,
    pub to: f64,
}

/// A piece of a family body drawn in plan: the cut through a solid, a symbolic line or a filled region.
#[derive(Debug, Clone, PartialEq)]
pub struct SymPart {
    pub subcategory: String,
    /// Outline first, then holes (filled), or one polyline (lines).
    pub rings: Vec<Vec<Point>>,
    pub closed: bool,
    pub filled: bool,
    /// The material cut by the view's cut plane (drawn with cut weights) rather than seen below it.
    pub cut: bool,
    /// Hides what lies behind it.
    pub masking: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolGeom {
    /// What a family body draws, when the family has one; the fields below are for families drawn by code.
    pub parts: Vec<SymPart>,
    pub lines: Vec<[Point; 2]>,
    /// Glazing lines (windows), drawn in the Glass subcategory.
    pub glass: Vec<[Point; 2]>,
    pub arcs: Vec<Arc>,
    /// Closed outlines drawn filled (door leaves, glass).
    pub polys: Vec<Vec<Point>>,
    /// The instance's footprint in the wall, for selection.
    pub footprint: Vec<Point>,
    /// The door frame in section, drawn from Medium detail up: the frame's outline
    /// at Medium, the material itself at Fine.
    pub frame: Vec<Vec<Point>>,
}

/// The plan symbol of a hosted instance at a detail level, or `None` if it is not
/// hosted. Coarse draws the opening, leaf and swing; Medium adds the frame outline;
/// Fine draws the frame's section.
pub fn plan_symbol(doc: &Document, id: Id, detail: DetailLevel) -> Option<SymbolGeom> {
    plan_symbol_at(doc, id, detail, None)
}

/// Like `plan_symbol`, for a view that cuts at `cut_z` (an elevation in mm), or `None` when the instance is
/// below the cut plane and is only seen.
pub fn plan_symbol_at(doc: &Document, id: Id, detail: DetailLevel, cut_z: Option<f64>) -> Option<SymbolGeom> {
    plan_symbol_look(doc, id, detail, cut_z.map_or(PlanLook::Below, PlanLook::Cut))
}

/// Where a plan's cut plane is against an instance: through it (at this height), above it, or below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlanLook {
    Cut(f64),
    Below,
    Above,
}

/// `plan_symbol_at`, knowing whether the instance is above the cut plane: pieces set to show only if the instance
/// is cut, or only if it is above the cut plane, follow it.
pub fn plan_symbol_look(doc: &Document, id: Id, detail: DetailLevel, look: PlanLook) -> Option<SymbolGeom> {
    let Element::FamilyInstance(inst) = doc.get(id)? else { return None };
    let (fam, _) = doc.instance_family(inst)?;
    let f = doc.instance_frame(id)?;
    if !fam.body.is_empty() {
        return body_symbol(doc, id, &f, detail, look);
    }
    // a family without a body draws nothing; its footprint in the wall is kept for selection
    let (hw, yr, yl) = (f.width / 2.0, -f.wall_right, f.wall_left);
    Some(SymbolGeom { footprint: vec![f.world(-hw, yr), f.world(hw, yr), f.world(hw, yl), f.world(-hw, yl)], ..Default::default() })
}

/// The plan symbol of a family with a stored body: the jambs of the opening it cuts, then
/// the cut (or projection) of every solid and the plan symbolic geometry that shows at this detail level.
fn body_symbol(doc: &Document, id: Id, f: &crate::hosting::InstanceFrame, detail: DetailLevel, look: PlanLook) -> Option<SymbolGeom> {
    let cut_z = match look {
        PlanLook::Cut(z) => Some(z),
        _ => None,
    };
    use crate::famgeom::{Axis, SolvedVis};
    let solved = doc.instance_solved(id)?;
    let s = solved.as_ref().as_ref().ok()?;
    let (xa, xb) = (f.x_off - f.width / 2.0, f.x_off + f.width / 2.0);
    let (yr, yl) = (-f.wall_right, f.wall_left);
    let mut g = SymbolGeom::default();
    if f.free {
        // a free standing instance is picked by the box its plan takes up, turned with it
        let mut b: Option<[f64; 4]> = None;
        let mut grow = |x: f64, y: f64| {
            let r = b.get_or_insert([x, y, x, y]);
            r[0] = r[0].min(x);
            r[1] = r[1].min(y);
            r[2] = r[2].max(x);
            r[3] = r[3].max(y);
        };
        for form in s.forms.iter().filter(|x| !x.void && x.vis.present) {
            for t in &form.solid.tris {
                for p in t {
                    grow(p[0], p[1]);
                }
            }
        }
        for sym in s.syms.iter().filter(|y| y.view == crate::famgeom::SymView::Plan && y.vis.present) {
            for p in &sym.points {
                grow(p.x, p.y);
            }
        }
        for m in s.model_lines.iter().filter(|m| m.vis.present) {
            for p in &m.points {
                grow(p[0], p[1]);
            }
        }
        let [x0, y0, x1, y1] = b.unwrap_or([-150.0, -150.0, 150.0, 150.0]);
        g.footprint = vec![f.fam_world(x0, y0), f.fam_world(x1, y0), f.fam_world(x1, y1), f.fam_world(x0, y1)];
    } else {
        g.footprint = vec![f.world(xa, yr), f.world(xb, yr), f.world(xb, yl), f.world(xa, yl)];
        g.lines.push([f.world(xa, yr), f.world(xa, yl)]);
        g.lines.push([f.world(xb, yr), f.world(xb, yl)]);
    }
    let shows = |v: &SolvedVis| {
        v.present
            && v.plan
            && (!v.only_if_cut || cut_z.is_some())
            && (!v.only_above_cut || look == PlanLook::Above)
            && match detail {
                DetailLevel::Coarse => v.coarse,
                DetailLevel::Medium => v.medium,
                DetailLevel::Fine => v.fine,
            }
    };
    let world = |ring: &[Point]| -> Vec<Point> { ring.iter().map(|p| f.fam_world(p.x, p.y)).collect() };
    let pieces = match cut_z {
        Some(z) => s.section(Axis::Z, z - f.z_ref),
        None => s.project(Axis::Z),
    };
    for p in pieces {
        let Some(form) = s.form(p.form) else { continue };
        if p.void || !shows(&form.vis) {
            continue;
        }
        g.parts.push(SymPart { subcategory: p.subcategory, rings: p.shape.iter().map(|r| world(r)).collect(), closed: true, filled: true, cut: cut_z.is_some(), masking: false });
    }
    for sym in &s.syms {
        if sym.view != crate::famgeom::SymView::Plan || !shows(&sym.vis) || sym.points.len() < 2 {
            continue;
        }
        g.parts.push(SymPart { subcategory: sym.subcategory.clone(), rings: vec![world(&sym.points)], closed: sym.closed, filled: sym.filled, cut: false, masking: sym.masking });
    }
    // Model lines are 3D, so the plan sees them flattened onto the floor.
    for m in &s.model_lines {
        if !shows(&m.vis) || m.points.len() < 2 {
            continue;
        }
        let flat: Vec<Point> = m.points.iter().map(|p| Point::new(p[0], p[1])).collect();
        let span = flat.iter().map(|q| (q.x - flat[0].x).abs().max((q.y - flat[0].y).abs())).fold(0.0, f64::max);
        if span < 1e-6 {
            continue;
        }
        g.parts.push(SymPart { subcategory: m.subcategory.clone(), rings: vec![world(&flat)], closed: m.closed, filled: false, cut: false, masking: false });
    }
    Some(g)
}
