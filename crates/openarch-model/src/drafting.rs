//! Drafting views (View > Drafting View): views with no model in them, only what is drawn in them (detail lines,
//! filled and masking regions, text notes, revision clouds). They have a name and a scale, open in the plan canvas,
//! duplicate with their detailing, and go on sheets sized by what is drawn in them.

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;
use crate::view::{View, ViewKind};

impl View {
    pub fn drafting(name: &str, scale: u32) -> View {
        let mut v = View { view_kind: ViewKind::Drafting, level: None, ..View::floor_plan(name, 0) };
        v.settings.scale = scale;
        v
    }
}

impl Document {
    /// A new drafting view ("Drafting 1" and on when no name is given), one undo step.
    pub fn add_drafting_view(&mut self, name: Option<&str>, scale: u32) -> Result<Id, String> {
        if !(1..=5000).contains(&scale) {
            return Err("The scale is 1 : 1 to 1 : 5000.".into());
        }
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                if self.view_name_taken(n, ViewKind::Drafting, None) {
                    return Err(format!("There is already a drafting view called {n}."));
                }
                n.to_string()
            }
            None => (1..).map(|n| format!("Drafting {n}")).find(|c| !self.view_name_taken(c, ViewKind::Drafting, None)).unwrap(),
        };
        self.transact("New drafting view", |tx| Ok(tx.add(Element::View(View::drafting(&name, scale)))))
    }

    /// The extent of what is drawn in a view (detail lines, regions, text notes), mm; None when nothing is.
    pub fn view_detail_bounds(&self, view: Id) -> Option<(Point, Point)> {
        let mut pts: Vec<Point> = vec![];
        for (_, l) in self.detail_lines(view) {
            pts.extend(l.curve.tessellate(5.0));
        }
        for (_, r) in self.detail_regions(view) {
            for edge in r.loops.iter().flatten() {
                pts.extend(edge.curve.tessellate(5.0));
            }
        }
        for (_, t) in self.text_notes(view) {
            pts.push(t.at);
        }
        // legend components: their strips and symbols
        if !self.legend_components(view).is_empty() {
            for (_, ring) in self.legend_paths(view, 1.0, crate::view::DetailLevel::Coarse).1 {
                pts.extend(ring);
            }
        }
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))))
    }

    /// The detailing of a view copied into another (Duplicate of a drafting view), inside a transaction.
    pub(crate) fn detail_copies(&self, from: Id, to: Id) -> Vec<Element> {
        let mut out = vec![];
        for (_, l) in self.detail_lines(from) {
            out.push(Element::DetailLine(crate::detail::DetailLine { view: to, pinned: false, ..l.clone() }));
        }
        for (_, r) in self.detail_regions(from) {
            out.push(Element::DetailRegion(crate::detail::DetailRegion { view: to, pinned: false, ..r.clone() }));
        }
        for (_, t) in self.text_notes(from) {
            out.push(Element::TextNote(crate::detail::TextNote { view: to, pinned: false, ..t.clone() }));
        }
        for (_, c) in self.legend_components(from) {
            out.push(Element::LegendComponent(crate::legend::LegendComponent { view: to, pinned: false, ..c.clone() }));
        }
        out
    }
}
