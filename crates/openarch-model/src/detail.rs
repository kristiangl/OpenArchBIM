//! Detail items: drawn in one view only, on the view's own plane (model millimetres in a plan view). A detail line
//! takes a line style (a subcategory of Lines in Object Styles). A filled region is closed loops of edges, each edge
//! with its own line style, filled as its filled region type says; a masking region hides what is behind it.
//! Line weights are model line weights, so they follow the view's scale as other lines do; patterns and dashes are
//! paper sizes.

use serde::{Deserialize, Serialize};

use crate::annotation::{edge_loops, PathFill, PathStroke, RegionEdge, RegionType, SymbolPaths, INVISIBLE_LINES};
use crate::curve::{self, Curve};
use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;
use crate::styles::{ObjectStyle, PatternKind, SOLID_FILL};

/// The line style a new detail line or region edge gets.
pub const DEFAULT_LINE_STYLE: &str = "Thin Lines";
/// The category whose subcategories are the line styles.
pub const LINES: &str = "Lines";

fn thin() -> String {
    DEFAULT_LINE_STYLE.into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailLine {
    pub view: Id,
    pub curve: Curve,
    #[serde(default = "thin")]
    pub style: String,
    #[serde(default)]
    pub pinned: bool,
}

/// A filled region (with a type) or a masking region (without), in one view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailRegion {
    pub view: Id,
    /// Closed loops, each head to tail; a loop inside another is a hole. An edge's `subcategory` is its line style.
    pub loops: Vec<Vec<RegionEdge>>,
    #[serde(default)]
    pub masking: bool,
    #[serde(default)]
    pub region_type: Option<Id>,
    #[serde(default)]
    pub pinned: bool,
}

/// One line of a detail line sketch: the element it was (none for a new line), its curve and its line style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailLineEdit {
    #[serde(default)]
    pub id: Option<Id>,
    pub curve: Curve,
    #[serde(default = "thin")]
    pub style: String,
}

impl Document {
    pub fn detail_lines(&self, view: Id) -> Vec<(Id, &DetailLine)> {
        self.iter().filter_map(|(id, e)| if let Element::DetailLine(l) = e { Some((id, l)).filter(|(_, l)| l.view == view) } else { None }).collect()
    }

    pub fn detail_regions(&self, view: Id) -> Vec<(Id, &DetailRegion)> {
        self.iter().filter_map(|(id, e)| if let Element::DetailRegion(r) = e { Some((id, r)).filter(|(_, r)| r.view == view) } else { None }).collect()
    }

    pub fn filled_region_types(&self) -> Vec<(Id, &RegionType)> {
        self.iter().filter_map(|(id, e)| if let Element::FilledRegionType(t) = e { Some((id, t)) } else { None }).collect()
    }

    pub fn filled_region_type(&self, id: Id) -> Option<&RegionType> {
        match self.get(id) {
            Some(Element::FilledRegionType(t)) => Some(t),
            _ => None,
        }
    }

    /// The line styles: the subcategories of Lines in Object Styles.
    pub fn line_styles(&self) -> Vec<String> {
        self.object_styles().and_then(|(_, s)| s.annotation.get(LINES)).map(|c| c.subcategories.keys().cloned().collect()).unwrap_or_default()
    }

    /// The filled region types a project starts with (and an older project gets).
    pub fn ensure_detail_defaults(&mut self) {
        if !self.filled_region_types().is_empty() {
            return;
        }
        let _ = self.transact("Filled region types", |tx| {
            for (name, pattern) in [("Solid Black", SOLID_FILL), ("Diagonal Up", "Diagonal up"), ("Diagonal Crosshatch", "Diagonal crosshatch"), ("Crosshatch", "Crosshatch")] {
                let mut t = RegionType::solid(name);
                t.foreground = Some(pattern.into());
                tx.add(Element::FilledRegionType(t));
            }
            Ok(())
        });
        self.clear_history();
    }

    fn check_detail_view(&self, view: Id) -> Result<(), String> {
        match self.view(view) {
            Some(v) if v.view_kind.is_plan() => Ok(()),
            Some(_) => Err("detail lines and regions can only be drawn in plan views for now".into()),
            None => Err("no such view".into()),
        }
    }

    fn check_line_style(&self, style: &str, invisible_ok: bool) -> Result<(), String> {
        if (invisible_ok && style == INVISIBLE_LINES) || self.line_styles().iter().any(|s| s == style) {
            Ok(())
        } else {
            Err(format!("'{style}' is not a line style"))
        }
    }

    /// Make a view's detail lines what a Detail Line sketch holds: lines with an id change, lines without one are
    /// added, and the view's lines missing from the list are deleted, all in one undo step. A line listed twice
    /// with one id (split, or copied) keeps the id the first time and is new after that. Returns each line's id.
    pub fn set_detail_lines(&mut self, view: Id, lines: Vec<DetailLineEdit>) -> Result<Vec<Id>, String> {
        self.check_detail_view(view)?;
        for l in &lines {
            self.check_line_style(&l.style, false)?;
            if l.curve.length().is_nan() || l.curve.length() <= 0.5 {
                return Err("a detail line needs some length".into());
            }
        }
        let have: Vec<Id> = self.detail_lines(view).iter().map(|(id, _)| *id).collect();
        let mut used = std::collections::BTreeSet::new();
        let plan: Vec<(Option<Id>, DetailLine)> = lines
            .into_iter()
            .map(|l| {
                let id = l.id.filter(|i| have.contains(i) && used.insert(*i));
                let pinned = id.and_then(|i| match self.get(i) { Some(Element::DetailLine(d)) => Some(d.pinned), _ => None }).unwrap_or(false);
                (id, DetailLine { view, curve: l.curve, style: l.style, pinned })
            })
            .collect();
        let gone: Vec<Id> = have.iter().copied().filter(|i| !used.contains(i)).collect();
        let changed = !gone.is_empty() || plan.iter().any(|(id, l)| id.is_none_or(|i| self.get(i) != Some(&Element::DetailLine(l.clone()))));
        if !changed {
            return Ok(plan.iter().filter_map(|(id, _)| *id).collect());
        }
        self.transact("Detail lines", |tx| {
            for i in &gone {
                tx.remove(*i)?;
            }
            let mut out = vec![];
            for (id, l) in plan {
                match id {
                    Some(i) => {
                        if tx.doc_ref().get(i) != Some(&Element::DetailLine(l.clone())) {
                            tx.update(i, Element::DetailLine(l))?;
                        }
                        out.push(i);
                    }
                    None => out.push(tx.add(Element::DetailLine(l))),
                }
            }
            Ok(out)
        })
    }

    pub fn patch_detail_line(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::DetailLine(old)) = self.get(id).cloned() else { return Err("not a detail line".into()) };
        let new: DetailLine = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        if new.view != old.view {
            return Err("a detail line stays in its view".into());
        }
        self.check_line_style(&new.style, false)?;
        self.transact("Change detail line", |tx| tx.update(id, Element::DetailLine(new)))
    }

    /// Check a region's edges and group them into loops, as Finish does.
    fn region_loops_checked(&self, edges: &[RegionEdge]) -> Result<Vec<Vec<RegionEdge>>, String> {
        for e in edges {
            self.check_line_style(&e.subcategory, true)?;
        }
        edge_loops(edges, 1.0).map_err(|e| e.message)
    }

    /// A filled region (with `region_type`) or a masking region (without) from a sketch's edges.
    pub fn add_detail_region(&mut self, view: Id, edges: Vec<RegionEdge>, region_type: Option<Id>) -> Result<Id, String> {
        self.check_detail_view(view)?;
        if let Some(t) = region_type {
            if self.filled_region_type(t).is_none() {
                return Err("unknown filled region type".into());
            }
        }
        let loops = self.region_loops_checked(&edges)?;
        let masking = region_type.is_none();
        self.transact(if masking { "Masking region" } else { "Filled region" }, |tx| Ok(tx.add(Element::DetailRegion(DetailRegion { view, loops, masking, region_type, pinned: false }))))
    }

    /// Edit Boundary: new edges for a region.
    pub fn set_detail_region_edges(&mut self, id: Id, edges: Vec<RegionEdge>) -> Result<(), String> {
        let Some(Element::DetailRegion(old)) = self.get(id).cloned() else { return Err("not a region".into()) };
        let loops = self.region_loops_checked(&edges)?;
        if loops == old.loops {
            return Ok(());
        }
        self.transact("Edit boundary", |tx| tx.update(id, Element::DetailRegion(DetailRegion { loops, ..old })))
    }

    pub fn patch_detail_region(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::DetailRegion(old)) = self.get(id).cloned() else { return Err("not a region".into()) };
        let new: DetailRegion = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        if new.view != old.view || new.loops != old.loops || new.masking != old.masking {
            return Err("change a region's boundary with Edit Boundary".into());
        }
        match new.region_type {
            Some(t) if self.filled_region_type(t).is_none() => return Err("unknown filled region type".into()),
            None if !new.masking => return Err("a filled region needs a type".into()),
            _ => {}
        }
        self.transact("Change region", |tx| tx.update(id, Element::DetailRegion(new)))
    }

    fn check_region_type(&self, id: Option<Id>, t: &RegionType) -> Result<(), String> {
        if t.name.trim().is_empty() {
            return Err("a type needs a name".into());
        }
        if self.filled_region_types().iter().any(|(i, o)| Some(*i) != id && o.name == t.name) {
            return Err(format!("a filled region type named '{}' already exists", t.name));
        }
        if !(1..=crate::styles::MAX_WEIGHT).contains(&t.line_weight) {
            return Err(format!("line weight must be 1 to {}", crate::styles::MAX_WEIGHT));
        }
        if let Some(p) = &t.foreground {
            if self.fill_pattern(p).is_none() {
                return Err(format!("no fill pattern named '{p}'"));
            }
        }
        if let Some(p) = &t.background {
            match self.fill_pattern(p) {
                None => return Err(format!("no fill pattern named '{p}'")),
                Some(fp) if fp.pattern_type != PatternKind::Drafting => return Err("the background pattern must be a drafting pattern".into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn patch_filled_region_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.filled_region_type(id).cloned() else { return Err("not a filled region type".into()) };
        let new: RegionType = crate::doc::apply_patch(&old, patch)?;
        if new == old {
            return Ok(());
        }
        self.check_region_type(Some(id), &new)?;
        self.transact("Change filled region type", |tx| tx.update(id, Element::FilledRegionType(new)))
    }

    pub fn duplicate_filled_region_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let Some(t) = self.filled_region_type(id).cloned() else { return Err("not a filled region type".into()) };
        let new = RegionType { name: name.trim().into(), ..t };
        self.check_region_type(None, &new)?;
        self.transact("Duplicate filled region type", |tx| Ok(tx.add(Element::FilledRegionType(new))))
    }

    fn line_style(&self, style: &str) -> ObjectStyle {
        let os = self.object_styles().map(|(_, s)| s);
        os.and_then(|s| s.resolve(LINES, Some(style))).cloned().unwrap_or_else(|| ObjectStyle::new(1, None))
    }

    /// A view's detail lines and regions as paths, in paper mm (model mm over the view `scale`) so patterns, dashes
    /// and line widths keep their printed size. A stroke's `line` or `region` and a fill's `region` are element ids.
    /// `lines` and `regions` say whether the view shows them (its Lines and Detail Items categories).
    pub fn detail_paths(&self, view: Id, scale: f64, lines: bool, regions: bool) -> SymbolPaths {
        const TOL_MM: f64 = 1.0;
        let weights = self.line_weights().map(|(_, w)| w.clone()).unwrap_or_default();
        let s = scale.max(1.0);
        let paper = |pts: Vec<Point>| -> Vec<[f64; 2]> { pts.iter().map(|q| [q.x / s, q.y / s]).collect() };
        let stroke = |c: &Curve, style: &str, line: Option<Id>, region: Option<Id>| -> Option<PathStroke> {
            if style == INVISIBLE_LINES {
                return None;
            }
            let st = self.line_style(style);
            Some(PathStroke {
                pts: paper(c.tessellate(TOL_MM.min(s * 0.02))),
                closed: c.is_closed(),
                width_mm: weights.model_width(st.projection_weight, scale as u32),
                color: st.color,
                dash: self.line_pattern(&st.pattern).map(|p| p.dash_array()).unwrap_or_default(),
                line: line.map(|i| i as usize),
                region: region.map(|i| i as usize),
                hidden: false,
            })
        };
        let mut out = SymbolPaths::default();
        if regions {
            for (id, r) in self.detail_regions(view) {
                let polys: Vec<Vec<Point>> = r.loops.iter().map(|l| curve::loop_polygon(&l.iter().map(|e| e.curve.clone()).collect::<Vec<_>>(), TOL_MM)).collect();
                let depth = curve::loop_depths(&polys);
                let area: f64 = polys.iter().zip(&depth).map(|(p, d)| if d % 2 == 0 { 1.0 } else { -1.0 } * crate::geom::polygon_area_abs(p)).sum();
                let ty = r.region_type.and_then(|t| self.filled_region_type(t));
                out.fills.push(PathFill {
                    rings: polys.into_iter().map(paper).collect(),
                    masking: r.masking || ty.is_some_and(|t| t.masking),
                    foreground: if r.masking { None } else { ty.and_then(|t| t.foreground.clone()) },
                    foreground_color: ty.map_or([0, 0, 0], |t| t.foreground_color),
                    background: if r.masking { None } else { ty.and_then(|t| t.background.clone()) },
                    background_color: ty.map_or([255, 255, 255], |t| t.background_color),
                    pen_mm: weights.model_width(ty.map_or(1, |t| t.line_weight), scale as u32),
                    region: id as usize,
                    area_mm2: area,
                    hidden: false,
                });
                for e in r.loops.iter().flatten() {
                    out.strokes.extend(stroke(&e.curve, &e.subcategory, None, Some(id)));
                }
            }
        }
        if lines {
            for (id, l) in self.detail_lines(view) {
                out.strokes.extend(stroke(&l.curve, &l.style, Some(id), None));
            }
        }
        out
    }

    /// The area a region covers (holes taken off), mm².
    pub fn detail_region_area(&self, r: &DetailRegion) -> f64 {
        let polys: Vec<Vec<Point>> = r.loops.iter().map(|l| curve::loop_polygon(&l.iter().map(|e| e.curve.clone()).collect::<Vec<_>>(), 1.0)).collect();
        let depth = curve::loop_depths(&polys);
        polys.iter().zip(&depth).map(|(p, d)| if d % 2 == 0 { 1.0 } else { -1.0 } * crate::geom::polygon_area_abs(p)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn plan(d: &Document) -> Id {
        d.views().into_iter().find(|(_, v)| v.view_kind.is_plan()).unwrap().0
    }

    #[test]
    fn a_detail_line_sketch_adds_changes_and_deletes_lines_in_one_step_each() {
        let mut d = Document::new_project();
        let v = plan(&d);
        let line = |id: Option<Id>, a: Point, b: Point, style: &str| DetailLineEdit { id, curve: Curve::line(a, b), style: style.into() };
        let ids = d.set_detail_lines(v, vec![line(None, p(0.0, 0.0), p(3000.0, 0.0), "Thin Lines"), line(None, p(0.0, 0.0), p(0.0, 2000.0), "Wide Lines")]).unwrap();
        assert_eq!(d.detail_lines(v).len(), 2);
        // a split keeps the id on the first half; the second half is new; the other line goes
        let ids2 = d.set_detail_lines(v, vec![line(Some(ids[0]), p(0.0, 0.0), p(1000.0, 0.0), "Thin Lines"), line(Some(ids[0]), p(1000.0, 0.0), p(3000.0, 0.0), "Thin Lines")]).unwrap();
        assert_eq!(ids2[0], ids[0]);
        assert_ne!(ids2[1], ids[0]);
        assert!(d.get(ids[1]).is_none());
        d.undo().unwrap();
        assert_eq!(d.detail_lines(v).len(), 2, "one undo step takes the change back");
        assert!(d.set_detail_lines(v, vec![line(None, p(0.0, 0.0), p(10.0, 0.0), "No Such Style")]).unwrap_err().contains("line style"));
        let elev = d.views().into_iter().find(|(_, v)| !v.view_kind.is_plan()).unwrap().0;
        assert!(d.set_detail_lines(elev, vec![]).is_err());
        // nothing changed: no undo step
        let rev = d.revision();
        let same: Vec<DetailLineEdit> = d.detail_lines(v).iter().map(|(i, l)| line(Some(*i), l.curve.start(), l.curve.end(), &l.style)).collect();
        d.set_detail_lines(v, same).unwrap();
        assert_eq!(d.revision(), rev);
    }

    #[test]
    fn regions_close_take_a_type_and_draw_at_paper_size() {
        let mut d = Document::new_project();
        let v = plan(&d);
        let e = |a: Point, b: Point, s: &str| RegionEdge { curve: Curve::line(a, b), subcategory: s.into() };
        let sq = vec![e(p(0.0, 0.0), p(2000.0, 0.0), "Thin Lines"), e(p(2000.0, 0.0), p(2000.0, 1000.0), INVISIBLE_LINES), e(p(2000.0, 1000.0), p(0.0, 1000.0), "Thin Lines"), e(p(0.0, 1000.0), p(0.0, 0.0), "Thin Lines")];
        let solid = d.filled_region_types().into_iter().find(|(_, t)| t.name == "Solid Black").unwrap().0;
        let r = d.add_detail_region(v, sq.clone(), Some(solid)).unwrap();
        assert!(d.add_detail_region(v, sq[..3].to_vec(), Some(solid)).is_err(), "an open boundary is refused");
        let m = d.add_detail_region(v, sq.iter().map(|x| RegionEdge { curve: x.curve.transform(crate::edit::Xform::Move { dx: 5000.0, dy: 0.0 }), ..x.clone() }).collect(), None).unwrap();
        let paths = d.detail_paths(v, 100.0, true, true);
        let fill = paths.fills.iter().find(|f| f.region == r as usize).unwrap();
        assert!((fill.area_mm2 - 2_000_000.0).abs() < 1.0);
        assert_eq!(fill.rings[0].iter().map(|q| q[0]).fold(0.0, f64::max), 20.0, "paper mm at 1:100");
        assert_eq!(fill.foreground.as_deref(), Some(SOLID_FILL));
        assert_eq!(paths.strokes.iter().filter(|s| s.region == Some(r as usize)).count(), 3, "the invisible edge is not drawn");
        assert!(paths.fills.iter().find(|f| f.region == m as usize).unwrap().masking);
        assert!(d.detail_paths(v, 100.0, true, false).fills.is_empty(), "Detail Items hidden in the view");
        // types: a background pattern must be a drafting one; a duplicate needs a name of its own
        assert!(d.duplicate_filled_region_type(solid, "Solid Black").is_err());
        let t2 = d.duplicate_filled_region_type(solid, "Grey").unwrap();
        d.patch_filled_region_type(t2, &serde_json::json!({ "foreground_color": [128, 128, 128] })).unwrap();
        d.patch_detail_region(r, &serde_json::json!({ "region_type": t2 })).unwrap();
        assert_eq!(d.detail_paths(v, 100.0, true, true).fills.iter().find(|f| f.region == r as usize).unwrap().foreground_color, [128, 128, 128]);
        if let Some(model) = d.fill_patterns().into_iter().find(|(_, p)| p.pattern_type == PatternKind::Model).map(|(_, p)| p.name.clone()) {
            assert!(d.patch_filled_region_type(t2, &serde_json::json!({ "background": model })).unwrap_err().contains("drafting"));
        }
    }

    #[test]
    fn detail_items_move_copy_and_go_with_their_view() {
        let mut d = Document::new_project();
        let v = plan(&d);
        let ids = d.set_detail_lines(v, vec![DetailLineEdit { id: None, curve: Curve::line(p(0.0, 0.0), p(1000.0, 0.0)), style: "Thin Lines".into() }]).unwrap();
        let copies = d.transform_elements(&ids, crate::edit::Xform::Move { dx: 0.0, dy: 500.0 }, crate::edit::EditOptions { copy: true, disjoin: false }).unwrap();
        assert_eq!(d.detail_lines(v).len(), 2);
        let Some(Element::DetailLine(c)) = d.get(copies[0]) else { panic!() };
        assert_eq!(c.curve.start(), p(0.0, 500.0));
        d.transform_elements(&ids, crate::edit::Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, crate::edit::EditOptions::default()).unwrap();
        let Some(Element::DetailLine(l)) = d.get(ids[0]) else { panic!() };
        assert!(l.curve.end().dist(&p(0.0, 1000.0)) < 1e-6);
        d.delete(&[v]).unwrap();
        assert!(d.get(ids[0]).is_none() && d.get(copies[0]).is_none(), "the view's detail lines go with it");
    }
}
