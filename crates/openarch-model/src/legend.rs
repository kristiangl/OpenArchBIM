//! Legends (View > Legend): views like drafting views (only what is drawn in them) that can go on any number of
//! sheets, and Legend Components in them: a type drawn on its own. A wall type is a strip in plan with its layers
//! cut and hatched; a floor, roof or ceiling type is a strip in section, its layers stacked; a door, window or other
//! family type is its plan symbol (a door or window in a short stub of wall). Nothing of the model is drawn.

use serde::{Deserialize, Serialize};

use crate::annotation::{PathFill, PathStroke, SymbolPaths};
use crate::doc::Document;
use crate::element::{Element, Id, Layer};
use crate::geom::Point;
use crate::view::{DetailLevel, View, ViewKind};

/// How a legend component shows its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegendDir {
    /// From above: a wall type's layers as a plan strip, a family's plan symbol.
    #[default]
    Plan,
    /// Cut across: a floor, roof or ceiling type's layers stacked.
    Section,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendComponent {
    pub view: Id,
    /// A wall, floor, roof or ceiling type, or a family type.
    pub type_id: Id,
    /// View mm: the left end of a strip (its top for a section), a symbol's origin.
    pub at: Point,
    #[serde(default)]
    pub dir: LegendDir,
    /// How long a strip is drawn (Host Length), mm.
    #[serde(default = "len")]
    pub length_mm: f64,
    /// None follows the view.
    #[serde(default)]
    pub detail: Option<DetailLevel>,
    #[serde(default)]
    pub pinned: bool,
}

fn len() -> f64 {
    1000.0
}

/// What kind of type a legend component shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendKind {
    Wall,
    Slab,
    Family,
}

impl View {
    pub fn legend(name: &str, scale: u32) -> View {
        let mut v = View { view_kind: ViewKind::Legend, level: None, ..View::floor_plan(name, 0) };
        v.settings.scale = scale;
        v
    }
}

impl Document {
    /// A new legend view ("Legend 1" and on when no name is given), one undo step.
    pub fn add_legend_view(&mut self, name: Option<&str>, scale: u32) -> Result<Id, String> {
        if !(1..=5000).contains(&scale) {
            return Err("The scale is 1 : 1 to 1 : 5000.".into());
        }
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                if self.view_name_taken(n, ViewKind::Legend, None) {
                    return Err(format!("There is already a legend called {n}."));
                }
                n.to_string()
            }
            None => (1..).map(|n| format!("Legend {n}")).find(|c| !self.view_name_taken(c, ViewKind::Legend, None)).unwrap(),
        };
        self.transact("New legend", |tx| Ok(tx.add(Element::View(View::legend(&name, scale)))))
    }

    pub fn legend_component(&self, id: Id) -> Option<&LegendComponent> {
        match self.get(id) {
            Some(Element::LegendComponent(c)) => Some(c),
            _ => None,
        }
    }

    pub fn legend_components(&self, view: Id) -> Vec<(Id, &LegendComponent)> {
        self.iter().filter_map(|(id, e)| match e { Element::LegendComponent(c) if c.view == view => Some((id, c)), _ => None }).collect()
    }

    /// What a type is to a legend: a wall type, a floor, roof or ceiling type, or a family type; None for others.
    pub fn legend_kind(&self, type_id: Id) -> Option<LegendKind> {
        match self.get(type_id)? {
            Element::WallType(t) if t.curtain.is_none() => Some(LegendKind::Wall),
            Element::FloorType(_) | Element::RoofType(_) | Element::CeilingType(_) => Some(LegendKind::Slab),
            // profiles are only used inside other families, so there is nothing to show
            Element::FamilyType(t) if self.family(t.family).is_some_and(|f| f.category != crate::family::FamilyCategory::Profiles) => Some(LegendKind::Family),
            _ => None,
        }
    }

    fn check_legend_component(&self, c: &LegendComponent) -> Result<(), String> {
        if self.view(c.view).map(|v| v.view_kind) != Some(ViewKind::Legend) {
            return Err("Legend components go in legend views.".into());
        }
        let kind = self.legend_kind(c.type_id).ok_or("A legend component shows a wall, floor, roof or ceiling type, or a family type.")?;
        match (kind, c.dir) {
            (LegendKind::Slab, LegendDir::Plan) => return Err("Floor, roof and ceiling types show in section.".into()),
            (LegendKind::Wall | LegendKind::Family, LegendDir::Section) => return Err("Walls, doors, windows and components show in plan.".into()),
            _ => {}
        }
        if !(c.length_mm.is_finite() && (10.0..=100_000.0).contains(&c.length_mm)) {
            return Err("Host Length is 10 to 100000 mm.".into());
        }
        Ok(())
    }

    /// Place a legend component: the direction follows the type (section for floors, roofs and ceilings).
    pub fn add_legend_component(&mut self, view: Id, type_id: Id, at: Point, length_mm: Option<f64>) -> Result<Id, String> {
        let dir = if self.legend_kind(type_id) == Some(LegendKind::Slab) { LegendDir::Section } else { LegendDir::Plan };
        let c = LegendComponent { view, type_id, at, dir, length_mm: length_mm.unwrap_or(1000.0), detail: None, pinned: false };
        self.check_legend_component(&c)?;
        self.transact("Legend component", |tx| Ok(tx.add(Element::LegendComponent(c))))
    }

    pub fn patch_legend_component(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.legend_component(id).cloned().ok_or("not a legend component")?;
        let mut new: LegendComponent = crate::doc::apply_patch(&old, patch)?;
        new.view = old.view;
        // a new type of another kind takes that kind's direction
        if new.type_id != old.type_id {
            new.dir = if self.legend_kind(new.type_id) == Some(LegendKind::Slab) { LegendDir::Section } else { LegendDir::Plan };
        }
        if new == old {
            return Ok(());
        }
        self.check_legend_component(&new)?;
        self.transact("Change legend component", |tx| tx.update(id, Element::LegendComponent(new)))
    }

    /// The name a legend component shows its type by ("Basic Wall : Brick Veneer", "Single Flush : 820 x 2040").
    pub fn legend_type_label(&self, type_id: Id) -> String {
        match self.get(type_id) {
            Some(Element::WallType(t)) => format!("Basic Wall : {}", t.name),
            Some(Element::FloorType(t)) => format!("Floor : {}", t.name),
            Some(Element::RoofType(t)) => format!("Roof : {}", t.name),
            Some(Element::CeilingType(t)) => format!("Ceiling : {}", t.name),
            Some(Element::FamilyType(t)) => format!("{} : {}", self.family(t.family).map_or("?", |f| f.name.as_str()), t.name),
            _ => "?".into(),
        }
    }

    /// The legend components of a view drawn, on paper (mm, the view's scale): strips of layers in their cut
    /// patterns and family symbols. Each piece carries its component's id in `line` (strokes) or `region` (fills).
    /// Also each component's outline in view mm, for picking.
    pub fn legend_paths(&self, view: Id, scale: f64, detail: DetailLevel) -> (SymbolPaths, Vec<(Id, Vec<Point>)>) {
        let s = scale.max(1.0);
        let weights = self.line_weights().map(|(_, w)| w.clone()).unwrap_or_default();
        let (thin, thick) = (weights.model_width(1, s as u32), weights.model_width(3, s as u32));
        let paper = |pts: &[Point]| -> Vec<[f64; 2]> { pts.iter().map(|q| [q.x / s, q.y / s]).collect() };
        let mut out = SymbolPaths::default();
        let mut boxes = vec![];
        let stroke = |pts: &[Point], closed: bool, width: f64, id: Id| PathStroke { pts: paper(pts), closed, width_mm: width, color: [0, 0, 0], dash: vec![], line: Some(id as usize), region: None, style: String::new(), hidden: false };
        let fill_of = |layer_material: &str, rings: &[Vec<Point>], id: Id| -> PathFill {
            let g = self.material(layer_material).map(|m| m.graphics.clone());
            PathFill {
                rings: rings.iter().map(|r| paper(r)).collect(),
                masking: true,
                foreground: g.as_ref().and_then(|g| g.cut_foreground.pattern.clone()),
                foreground_color: g.as_ref().and_then(|g| g.cut_foreground.color).unwrap_or([0, 0, 0]),
                background: g.as_ref().and_then(|g| g.cut_background.pattern.clone()),
                background_color: g.as_ref().and_then(|g| g.cut_background.color).unwrap_or([255, 255, 255]),
                pen_mm: thin,
                region: id as usize,
                area_mm2: 0.0,
                hidden: false,
            }
        };
        let comps = self.legend_components(view);
        // families are drawn from one copy of the project with each placed in it, well apart
        let fam: Vec<(Id, &LegendComponent)> = comps.iter().filter(|(_, c)| self.legend_kind(c.type_id) == Some(LegendKind::Family)).map(|(i, c)| (*i, *c)).collect();
        let symbols = if fam.is_empty() { vec![] } else { self.legend_family_symbols(&fam, c_detail(detail)) };
        for (id, c) in &comps {
            let id = *id;
            let mut pts_all: Vec<Point> = vec![];
            match self.legend_kind(c.type_id) {
                Some(LegendKind::Wall) => {
                    let Some(Element::WallType(t)) = self.get(c.type_id) else { continue };
                    for l in crate::layers::sample_plan_layers(t, c.length_mm, None) {
                        for shape in &l.shapes {
                            let rings: Vec<Vec<Point>> = shape.iter().map(|r| r.iter().map(|p| Point::new(c.at.x + p.x, c.at.y + p.y)).collect()).collect();
                            if !l.is_void { out.fills.push(fill_of(&l.material, &rings, id)); }
                            for r in &rings { out.strokes.push(stroke(r, true, thick, id)); pts_all.extend(r); }
                        }
                    }
                }
                Some(LegendKind::Slab) => {
                    let layers: Vec<Layer> = match self.get(c.type_id) {
                        Some(Element::FloorType(t)) => t.layers.clone(),
                        Some(Element::RoofType(t)) => t.layers.clone(),
                        Some(Element::CeilingType(t)) => t.layers.clone(),
                        _ => vec![],
                    };
                    let mut y = c.at.y;
                    for l in layers.iter().filter(|l| l.thickness_mm > 0.0) {
                        let r = vec![Point::new(c.at.x, y), Point::new(c.at.x + c.length_mm, y), Point::new(c.at.x + c.length_mm, y - l.thickness_mm), Point::new(c.at.x, y - l.thickness_mm)];
                        if !l.is_void { out.fills.push(fill_of(&l.material, std::slice::from_ref(&r), id)); }
                        out.strokes.push(stroke(&r, true, thick, id));
                        pts_all.extend(&r);
                        y -= l.thickness_mm;
                    }
                }
                Some(LegendKind::Family) => {
                    if let Some((_, parts)) = symbols.iter().find(|(i, _)| *i == id) {
                        for (rings, closed, filled, cut) in parts {
                            let moved: Vec<Vec<Point>> = rings.iter().map(|r| r.iter().map(|p| Point::new(c.at.x + p.x, c.at.y + p.y)).collect()).collect();
                            if *filled {
                                out.fills.push(PathFill { rings: moved.iter().map(|r| paper(r)).collect(), masking: true, foreground: None, foreground_color: [0, 0, 0], background: None, background_color: [255, 255, 255], pen_mm: thin, region: id as usize, area_mm2: 0.0, hidden: false });
                            }
                            for r in &moved { out.strokes.push(stroke(r, *closed, if *cut { thick } else { thin }, id)); pts_all.extend(r); }
                        }
                    }
                }
                None => {}
            }
            if let Some(first) = pts_all.first().copied() {
                let (lo, hi) = pts_all.iter().fold((first, first), |(lo, hi), p| (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y))));
                boxes.push((id, vec![lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)]));
            } else {
                // nothing to draw (a family with no plan symbol): a small box so it can still be picked
                let (a, b) = (Point::new(c.at.x - 150.0, c.at.y - 150.0), Point::new(c.at.x + 150.0, c.at.y + 150.0));
                boxes.push((id, vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]));
            }
        }
        (out, boxes)
    }

    /// Each family component's plan symbol about its origin: (rings, closed, filled, cut) for every piece. A door or
    /// window sits in the middle of a stub of basic wall 600 mm longer than it each side, drawn with it.
    fn legend_family_symbols(&self, comps: &[(Id, &LegendComponent)], detail: DetailLevel) -> Vec<(Id, Vec<(Vec<Vec<Point>>, bool, bool, bool)>)> {
        let mut d = self.clone();
        let Some(lvl) = d.levels().first().map(|(id, _)| *id) else { return vec![] };
        let wall_type = d.iter().find_map(|(id, e)| match e { Element::WallType(t) if t.curtain.is_none() => Some(id), _ => None });
        let mut out = vec![];
        for (k, (cid, c)) in comps.iter().enumerate() {
            // each one far from the model and the others
            let origin = Point::new(1.0e7 + k as f64 * 50_000.0, 1.0e7);
            let Some(ft) = d.family_type(c.type_id).cloned() else { continue };
            let Some(fam) = d.family(ft.family).cloned() else { continue };
            let mut pieces: Vec<(Vec<Vec<Point>>, bool, bool, bool)> = vec![];
            let placed = match fam.host {
                crate::family::HostKind::Wall => {
                    let Some(wt) = wall_type else { continue };
                    let Ok(w) = d.add_wall(wt, lvl, Point::new(origin.x - 3000.0, origin.y), Point::new(origin.x + 3000.0, origin.y), 2700.0) else { continue };
                    let Ok(i) = d.place_on_wall(c.type_id, w, origin) else { continue };
                    // the wall either side of the opening, out 600 past it
                    if let (Some(o), Some(Element::Wall(wall))) = (d.wall_openings(w).into_iter().find(|o| o.instance == i), d.get(w).cloned()) {
                        let (l, r) = d.wall_extents(&wall);
                        let (u0, u1) = (o.u0 - 3000.0, o.u1 - 3000.0);
                        for (a, b) in [(u0 - 600.0, u0), (u1, u1 + 600.0)] {
                            pieces.push((vec![vec![Point::new(a, l), Point::new(b, l), Point::new(b, -r), Point::new(a, -r)]], true, false, true));
                        }
                    }
                    Some(i)
                }
                crate::family::HostKind::None | crate::family::HostKind::Ceiling => {
                    if fam.host == crate::family::HostKind::Ceiling {
                        let _ = d.transact("Legend", |tx| tx.update(ft.family, Element::Family(crate::family::Family { host: crate::family::HostKind::None, ..fam.clone() })));
                    }
                    d.place_free(c.type_id, lvl, origin, 0.0, 0.0).ok()
                }
                _ => None,
            };
            let Some(i) = placed else { continue };
            if let Some(sym) = crate::symbol::plan_symbol_at(&d, i, detail, None) {
                let rel = |p: &Point| Point::new(p.x - origin.x, p.y - origin.y);
                for p in &sym.parts {
                    pieces.push((p.rings.iter().map(|r| r.iter().map(rel).collect()).collect(), p.closed, p.filled, p.cut));
                }
                for l in &sym.lines {
                    pieces.push((vec![vec![rel(&l[0]), rel(&l[1])]], false, false, false));
                }
                for poly in &sym.polys {
                    pieces.push((vec![poly.iter().map(rel).collect()], true, false, false));
                }
            }
            out.push((*cid, pieces));
        }
        out
    }
}

fn c_detail(d: DetailLevel) -> DetailLevel {
    d
}
