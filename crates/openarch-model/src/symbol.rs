//! Plan symbols for hosted doors and windows: the lines, arcs and outlines a
//! plan view draws for an instance.

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::family::PlanSymbol;
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
    let Element::FamilyInstance(inst) = doc.get(id)? else { return None };
    let (fam, _) = doc.instance_family(inst)?;
    let f = doc.instance_frame(id)?;
    if !fam.body.is_empty() {
        return body_symbol(doc, id, &f, detail, cut_z);
    }
    let num = |n: &str, d: f64| doc.instance_param(inst, n).and_then(|v| v.as_f64()).unwrap_or(d);
    let (hw, ym) = (f.width / 2.0, f.y_mid());
    let (yr, yl) = (-f.wall_right, f.wall_left);
    let mut g = SymbolGeom::default();
    g.footprint = vec![f.world(-hw, yr), f.world(hw, yr), f.world(hw, yl), f.world(-hw, yl)];
    let line = |g: &mut SymbolGeom, a: (f64, f64), b: (f64, f64)| g.lines.push([f.world(a.0, a.1), f.world(b.0, b.1)]);
    let rect = |g: &mut SymbolGeom, x: (f64, f64), y: (f64, f64)| g.polys.push(vec![f.world(x.0, y.0), f.world(x.1, y.0), f.world(x.1, y.1), f.world(x.0, y.1)]);
    // the jambs close the opening across the wall's thickness
    line(&mut g, (-hw, yr), (-hw, yl));
    line(&mut g, (hw, yr), (hw, yl));
    let swing = if f.flip_facing { -1.0 } else { 1.0 };
    let wa = f.angle_deg();
    match fam.plan_symbol {
        PlanSymbol::None => {}
        PlanSymbol::SingleSwing | PlanSymbol::DoubleSwing => {
            let steel = doc.door_frame(id);
            let t = steel.as_ref().map_or_else(|| num("Thickness", 40.0), |s| s.leaf_t);
            // the leaf pivots where it closes: against the rib or stop of a frame, else mid wall
            let ypiv = steel.as_ref().map_or(ym, |s| ym + s.shift + swing * (s.leaf_b0() + s.leaf_t / 2.0));
            let leaves: Vec<(f64, f64)> = match &steel {
                Some(s) => s.hinges(f.flip_hand).into_iter().map(|h| (h, s.leaf_w)).collect(),
                None if fam.plan_symbol == PlanSymbol::DoubleSwing => vec![(-hw, f.width / 2.0), (hw, f.width / 2.0)], // hinge x, leaf length
                None => vec![(if f.flip_hand { hw } else { -hw }, f.width)],
            };
            if let Some(s) = &steel {
                if detail != DetailLevel::Coarse {
                    let r2 = s.reveal_w / 2.0;
                    let pieces = if detail == DetailLevel::Fine { s.section() } else { vec![s.envelope()] };
                    for side in [-1.0, 1.0] {
                        for [a0, a1, b0, b1] in &pieces {
                            let (xa, xb) = (side * (r2 + a0), side * (r2 + a1));
                            let (ya, yb) = (ym + s.shift + swing * b0, ym + s.shift + swing * b1);
                            g.frame.push(vec![f.world(xa, ya), f.world(xb, ya), f.world(xb, yb), f.world(xa, yb)]);
                        }
                    }
                }
            }
            for (hx, l) in leaves {
                // open leaf at 90 degrees to the wall, inside the opening
                let inward = if hx < 0.0 { 1.0 } else { -1.0 };
                let (xa, xb) = if inward > 0.0 { (hx, hx + t) } else { (hx - t, hx) };
                let (ya, yb) = if swing > 0.0 { (ypiv, ypiv + l) } else { (ypiv - l, ypiv) };
                rect(&mut g, (xa, xb), (ya, yb));
                // swing arc from the open leaf to the closed position (towards the other jamb)
                let (from, to) = match (swing > 0.0, hx < 0.0) {
                    (true, true) => (0.0, 90.0),
                    (false, true) => (-90.0, 0.0),
                    (true, false) => (90.0, 180.0),
                    (false, false) => (180.0, 270.0),
                };
                g.arcs.push(Arc { centre: f.world(hx, ypiv), radius: l, from: from + wa, to: to + wa });
            }
        }
        PlanSymbol::Sliding => {
            let t = 30.0;
            let off = 25.0;
            rect(&mut g, (-hw, hw * 0.2), (ym - off - t / 2.0, ym - off + t / 2.0));
            rect(&mut g, (-hw * 0.2, hw), (ym + off - t / 2.0, ym + off + t / 2.0));
        }
        PlanSymbol::Window => {
            // wall faces across the opening and two glazing lines
            line(&mut g, (-hw, yr), (hw, yr));
            line(&mut g, (-hw, yl), (hw, yl));
            let gt = num("Glazing Thickness", 6.0);
            for y in [ym - gt, ym + gt] {
                g.glass.push([f.world(-hw, y), f.world(hw, y)]);
            }
        }
    }
    Some(g)
}

/// The plan symbol of a family with a stored body: the jambs of the opening it cuts, then
/// the cut (or projection) of every solid and the plan symbolic geometry that shows at this detail level.
fn body_symbol(doc: &Document, id: Id, f: &crate::hosting::InstanceFrame, detail: DetailLevel, cut_z: Option<f64>) -> Option<SymbolGeom> {
    use crate::famgeom::{Axis, SolvedVis};
    let solved = doc.instance_solved(id)?;
    let s = solved.as_ref().as_ref().ok()?;
    let (xa, xb) = (f.x_off - f.width / 2.0, f.x_off + f.width / 2.0);
    let (yr, yl) = (-f.wall_right, f.wall_left);
    let mut g = SymbolGeom::default();
    g.footprint = vec![f.world(xa, yr), f.world(xb, yr), f.world(xb, yl), f.world(xa, yl)];
    g.lines.push([f.world(xa, yr), f.world(xa, yl)]);
    g.lines.push([f.world(xb, yr), f.world(xb, yl)]);
    let shows = |v: &SolvedVis| {
        v.present
            && v.plan
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
    Some(g)
}
