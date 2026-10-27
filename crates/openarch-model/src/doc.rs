//! The document: all elements, plus transactions with undo/redo.

use std::collections::BTreeMap;

use crate::curve::Curve;
use crate::element::*;
use crate::geom::{polygon_area_abs, Point};
use crate::view::*;

#[derive(Debug, Clone)]
struct Change {
    id: Id,
    before: Option<Element>,
    after: Option<Element>,
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    changes: Vec<Change>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) elements: BTreeMap<Id, Element>,
    pub(crate) next_id: Id,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    revision: u64,
    /// Counts every write through a transaction, rollback, undo and redo; keys the solved-family cache.
    pub(crate) edit_count: u64,
    pub(crate) solve_cache: crate::faminst::SolveCache,
    /// Depth of `grouped` calls open; dimension constraints settle when the outermost one ends.
    defer_settle: u32,
    /// Set while the constraint solver itself is moving elements.
    settling: bool,
}

/// Handle passed to a transaction closure. Every change is recorded so the
/// whole transaction can be undone as one step.
pub struct Tx<'a> {
    doc: &'a mut Document,
    changes: Vec<Change>,
}

impl Tx<'_> {
    pub fn add(&mut self, e: Element) -> Id {
        let id = self.doc.next_id;
        self.doc.next_id += 1;
        self.doc.edit_count += 1;
        self.doc.elements.insert(id, e.clone());
        self.changes.push(Change { id, before: None, after: Some(e) });
        id
    }

    pub fn update(&mut self, id: Id, e: Element) -> Result<(), String> {
        let before = self.doc.elements.get(&id).cloned().ok_or_else(|| format!("no element {id}"))?;
        self.doc.edit_count += 1;
        self.doc.elements.insert(id, e.clone());
        self.changes.push(Change { id, before: Some(before), after: Some(e) });
        Ok(())
    }

    pub fn remove(&mut self, id: Id) -> Result<(), String> {
        self.doc.edit_count += 1;
        let before = self.doc.elements.remove(&id).ok_or_else(|| format!("no element {id}"))?;
        let is_annotation = matches!(before, Element::Dimension(_) | Element::Spot(_));
        self.changes.push(Change { id, before: Some(before), after: None });
        if !is_annotation {
            // spots standing on this element go with it
            for sid in self.doc.spots_on(&[id]) {
                if self.doc.elements.contains_key(&sid) {
                    self.remove(sid)?;
                }
            }
            // dimensions that measured this element drop that reference and close up around the gap
            for (did, healed) in self.doc.healed_dimensions(id) {
                match healed {
                    Some(d) => self.update(did, Element::Dimension(d))?,
                    None => self.remove(did)?,
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, id: Id) -> Option<&Element> {
        self.doc.elements.get(&id)
    }

    pub(crate) fn doc_ref(&self) -> &Document {
        self.doc
    }
}

impl Default for Document {
    fn default() -> Self {
        Document { elements: BTreeMap::new(), next_id: 1, undo: vec![], redo: vec![], revision: 0, edit_count: 0, solve_cache: Default::default(), defer_settle: 0, settling: false }
    }
}

fn layer(material: &str, t: f64, function: LayerFunction, core: bool) -> Layer {
    Layer::new(material, t, function, core)
}

fn void(name: &str, t: f64, core: bool) -> Layer {
    Layer { is_void: true, ..Layer::new(name, t, LayerFunction::ThermalAir, core) }
}

/// Floor layers carry a function too, for the same reasons.
fn floor_layer(material: &str, t: f64, function: LayerFunction) -> Layer {
    layer(material, t, function, function == LayerFunction::Structure)
}

/// What to change in `Document::set_wall_join`. `None` leaves a setting alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct JoinChange {
    pub allowed: Option<bool>,
    pub kind: Option<JoinKind>,
    pub through: Option<Id>,
    pub cleanup: Option<JoinCleanup>,
}

impl Document {
    /// A new project with two levels and a starter set of Australian types.
    pub fn new_project() -> Self {
        let mut d = Document::default();
        use LayerFunction::{Finish1 as F1, Finish2 as F2, Structure as St};
        d.transact("New project", |tx| {
            tx.add(Element::Level(Level::new("Ground", 0.0)));
            tx.add(Element::Level(Level::new("Level 1", 3000.0)));
            for (name, layers) in [
                (
                    "Internal Stud 90",
                    vec![
                        layer("Plasterboard", 13.0, F1, false),
                        layer("Timber Frame", 90.0, St, true),
                        layer("Plasterboard", 13.0, F2, false),
                    ],
                ),
                (
                    "Brick Veneer",
                    vec![
                        layer("Face Brick", 110.0, F1, false),
                        void("Cavity", 40.0, false),
                        layer("Timber Frame", 90.0, St, true),
                        layer("Plasterboard", 13.0, F2, false),
                    ],
                ),
                (
                    "Double Brick Cavity",
                    vec![layer("Face Brick", 110.0, St, true), void("Cavity", 50.0, true), layer("Face Brick", 110.0, St, true)],
                ),
                ("Concrete 200", vec![layer("Concrete", 200.0, St, true)]),
                ("Blockwork 190", vec![layer("Concrete Block", 190.0, St, true)]),
            ] {
                tx.add(Element::WallType(WallType { name: name.into(), layers, wrap_ends: Wrap::None, wrap_inserts: Wrap::None, coarse_fill: default_coarse_fill(), function: WallFunction::default(), info: TypeInfo::default(), sample_height_mm: 6000.0 }));
            }
            for (name, layers) in [
                ("Concrete Slab 200", vec![floor_layer("Concrete", 200.0, St)]),
                ("Timber Floor 190", vec![floor_layer("Timber Flooring", 19.0, F1), floor_layer("Timber Joist", 170.0, St)]),
            ] {
                tx.add(Element::FloorType(FloorType { name: name.into(), layers, coarse_fill: default_coarse_fill(), function: FloorFunction::default(), info: TypeInfo::default() }));
            }
            tx.add_roof_types();
            let levels: Vec<(Id, String)> = tx
                .doc
                .elements
                .iter()
                .filter_map(|(id, e)| if let Element::Level(l) = e { Some((*id, l.name.clone())) } else { None })
                .collect();
            for (id, name) in levels {
                tx.add(Element::View(View::floor_plan(&name, id)));
            }
            tx.add(Element::View(View::three_d("{3D}")));
            tx.add_starter_families(&[]);
            tx.add_style_defaults();
            Ok(())
        })
        .expect("default project");
        d.ensure_site_defaults();
        d.ensure_annotation_defaults();
        d.ensure_view_types();
        d.ensure_detail_defaults();
        d.undo.clear(); // starting state is not undoable
        d
    }

    /// Run `f` as one undoable step. On error everything it did is rolled back.
    pub fn transact<T>(&mut self, name: &str, f: impl FnOnce(&mut Tx) -> Result<T, String>) -> Result<T, String> {
        let mut tx = Tx { doc: self, changes: Vec::new() };
        let out = f(&mut tx);
        let changes = std::mem::take(&mut tx.changes);
        match out {
            Ok(v) => {
                if !changes.is_empty() {
                    let model = changes.iter().any(|c| !matches!(c.after.as_ref().or(c.before.as_ref()), Some(Element::Dimension(_) | Element::DimensionType(_) | Element::Spot(_) | Element::SpotType(_) | Element::DetailLine(_) | Element::DetailRegion(_) | Element::FilledRegionType(_) | Element::TextNote(_) | Element::TextNoteType(_))));
                    let redo = if model && self.defer_settle == 0 && !self.settling { Some(self.redo.clone()) } else { None };
                    self.undo.push(Entry { name: name.to_string(), changes });
                    self.redo.clear();
                    self.revision += 1;
                    if let Some(redo) = redo {
                        let from = self.undo.len() - 1;
                        if let Err(e) = self.settle(from) {
                            self.redo = redo;
                            return Err(e);
                        }
                    }
                }
                Ok(v)
            }
            Err(e) => {
                Self::apply(&mut self.elements, &changes, true);
                self.edit_count += 1;
                Err(e)
            }
        }
    }

    /// Bring locked and equal dimensions back to what they say after the edits from undo step `from`
    /// on. The solver's own moves are folded into that step. When the constraints cannot all hold
    /// the whole step is undone and the reason returned.
    /// Like `transact`, then runs the dimension solver even when only a dimension changed (switching
    /// Equality on moves the elements now). A refusal undoes the whole change.
    pub(crate) fn transact_solving<T>(&mut self, name: &str, f: impl FnOnce(&mut Tx) -> Result<T, String>) -> Result<T, String> {
        let (n, redo) = (self.undo.len(), self.redo.clone());
        let out = self.transact(name, f)?;
        if let Err(e) = self.settle(n) {
            self.redo = redo;
            return Err(e);
        }
        Ok(out)
    }

    fn settle(&mut self, from: usize) -> Result<(), String> {
        if self.settling || !(self.has_dimension_constraints() || self.has_global_links()) {
            return Ok(());
        }
        self.settling = true;
        // the locks and labels hold, then reporting parameters read what their dimensions measure
        let r = self.solve_dimension_constraints().and_then(|_| self.check_round_holds()).and_then(|_| self.read_reporting());
        self.settling = false;
        match r {
            Ok(()) => {
                if self.undo.len() > from + 1 {
                    let name = self.undo[from].name.clone();
                    let merged: Vec<Change> = self.undo.drain(from..).flat_map(|e| e.changes).collect();
                    self.undo.push(Entry { name, changes: merged });
                }
                Ok(())
            }
            Err(e) => {
                while self.undo.len() > from {
                    let en = self.undo.pop().unwrap();
                    Self::apply(&mut self.elements, &en.changes, true);
                }
                self.edit_count += 1;
                Err(e)
            }
        }
    }

    /// Run `f`, which may make several edits, and fold them into one undo step called `name`.
    /// If `f` fails everything it did is undone and the redo history is left as it was.
    pub fn grouped<T>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        let (n, redo) = (self.undo.len(), self.redo.clone());
        self.defer_settle += 1;
        let out = f(self);
        self.defer_settle -= 1;
        match out {
            Ok(v) => {
                if self.undo.len() > n + 1 {
                    let merged: Vec<Change> = self.undo.drain(n..).flat_map(|e| e.changes).collect();
                    self.undo.push(Entry { name: name.to_string(), changes: merged });
                } else if self.undo.len() == n + 1 {
                    self.undo[n].name = name.to_string();
                }
                if self.defer_settle == 0 && self.undo.len() > n {
                    if let Err(e) = self.settle(n) {
                        self.redo = redo;
                        return Err(e);
                    }
                }
                Ok(v)
            }
            Err(e) => {
                while self.undo.len() > n {
                    let en = self.undo.pop().unwrap();
                    Self::apply(&mut self.elements, &en.changes, true);
                }
                self.edit_count += 1;
                self.redo = redo;
                Err(e)
            }
        }
    }

    /// Where the undo history stands, for a dialog that edits live but may still be cancelled
    /// (Type Properties): `fold_since` keeps its edits as one step, `roll_back_to` drops them.
    pub fn edit_mark(&self) -> usize {
        self.undo.len()
    }

    /// Fold the undo steps made since `mark` into one step called `name` (OK or Apply).
    pub fn fold_since(&mut self, mark: usize, name: &str) {
        let n = mark.min(self.undo.len());
        if self.undo.len() > n + 1 {
            let merged: Vec<Change> = self.undo.drain(n..).flat_map(|e| e.changes).collect();
            self.undo.push(Entry {
                name: name.to_string(),
                changes: merged,
            });
        } else if self.undo.len() == n + 1 {
            self.undo[n].name = name.to_string();
        }
    }

    /// Undo every step made since `mark` without leaving them to redo (Cancel).
    pub fn roll_back_to(&mut self, mark: usize) {
        let mut changed = false;
        while self.undo.len() > mark {
            let en = self.undo.pop().unwrap();
            Self::apply(&mut self.elements, &en.changes, true);
            changed = true;
        }
        if changed {
            self.revision += 1;
            self.edit_count += 1;
        }
    }

    /// Give the latest undo step a more specific name.
    pub(crate) fn rename_last(&mut self, name: &str) {
        if let Some(e) = self.undo.last_mut() {
            e.name = name.to_string();
        }
    }

    fn apply(elements: &mut BTreeMap<Id, Element>, changes: &[Change], backwards: bool) {
        let iter: Box<dyn Iterator<Item = &Change>> = if backwards { Box::new(changes.iter().rev()) } else { Box::new(changes.iter()) };
        for c in iter {
            let target = if backwards { &c.before } else { &c.after };
            match target {
                Some(e) => {
                    elements.insert(c.id, e.clone());
                }
                None => {
                    elements.remove(&c.id);
                }
            }
        }
    }

    pub fn undo(&mut self) -> Option<String> {
        let e = self.undo.pop()?;
        Self::apply(&mut self.elements, &e.changes, true);
        let name = e.name.clone();
        self.redo.push(e);
        self.revision += 1;
        self.edit_count += 1;
        Some(name)
    }

    pub fn redo(&mut self) -> Option<String> {
        let e = self.redo.pop()?;
        Self::apply(&mut self.elements, &e.changes, false);
        let name = e.name.clone();
        self.undo.push(e);
        self.revision += 1;
        self.edit_count += 1;
        Some(name)
    }

    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|e| e.name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|e| e.name.as_str())
    }

    /// Increases on every change, undo and redo. The UI uses it to know when to redraw.
    /// Forget undo history (used after setup steps that are not the user's edits).
    pub(crate) fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn get(&self, id: Id) -> Option<&Element> {
        self.elements.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id, &Element)> {
        self.elements.iter().map(|(k, v)| (*k, v))
    }

    pub fn levels(&self) -> Vec<(Id, &Level)> {
        let mut v: Vec<_> = self
            .iter()
            .filter_map(|(i, e)| if let Element::Level(l) = e { Some((i, l)) } else { None })
            .collect();
        v.sort_by(|a, b| a.1.elevation_mm.partial_cmp(&b.1.elevation_mm).unwrap());
        v
    }

    pub fn wall_types(&self) -> Vec<(Id, &WallType)> {
        self.iter().filter_map(|(i, e)| if let Element::WallType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn floor_types(&self) -> Vec<(Id, &FloorType)> {
        self.iter().filter_map(|(i, e)| if let Element::FloorType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn level(&self, id: Id) -> Option<&Level> {
        match self.get(id)? {
            Element::Level(l) => Some(l),
            _ => None,
        }
    }

    pub fn wall_type(&self, id: Id) -> Option<&WallType> {
        match self.get(id)? {
            Element::WallType(t) => Some(t),
            _ => None,
        }
    }

    pub fn floor_type(&self, id: Id) -> Option<&FloorType> {
        match self.get(id)? {
            Element::FloorType(t) => Some(t),
            _ => None,
        }
    }

    // ---- authoring commands (each is one undo step) ----------------------

    pub fn add_wall(&mut self, wall_type: Id, level: Id, start: Point, end: Point, height_mm: f64) -> Result<Id, String> {
        if self.wall_type(wall_type).is_none() {
            return Err("unknown wall type".into());
        }
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        if start.dist(&end) < 1.0 {
            return Err("wall is too short".into());
        }
        if height_mm <= 0.0 {
            return Err("wall height must be positive".into());
        }
        self.add_wall_with(Wall::new(wall_type, level, start, end, height_mm))
    }

    /// Check a wall is one that can exist: known type and level, some length and height, and
    /// for a curved wall an open curve that runs between its ends and bends no tighter than
    /// its thickness allows.
    pub fn check_wall(&self, w: &Wall) -> Result<(), String> {
        if self.wall_type(w.wall_type).is_none() {
            return Err("unknown wall type".into());
        }
        if self.level(w.base_level).is_none() {
            return Err("unknown level".into());
        }
        if w.start.dist(&w.end) < 1.0 && w.curve.is_none() {
            return Err("wall is too short".into());
        }
        if w.top_level.is_none() && w.height_mm <= 0.0 {
            return Err("wall height must be positive".into());
        }
        if let Some(c) = &w.curve {
            if c.is_closed() {
                return Err("a curved wall must be an open curve (a circle is two arc walls)".into());
            }
            if c.start().dist(&w.start) > 1.0 || c.end().dist(&w.end) > 1.0 {
                return Err("the curve must start and end at the ends of the wall".into());
            }
            let path = w.path();
            if path.length() < 1.0 {
                return Err("wall is too short".into());
            }
            // radius of curvature at each bend of the path, against how far the wall reaches to each side
            let (l, r) = self.wall_extents(w);
            let reach = l.max(r);
            for i in 1..path.pts.len() - 1 {
                let (a, b, c2) = (path.pts[i - 1], path.pts[i], path.pts[i + 1]);
                let (d1, d2) = (Point::new(b.x - a.x, b.y - a.y), Point::new(c2.x - b.x, c2.y - b.y));
                let turn = (d1.x * d2.y - d1.y * d2.x).atan2(d1.x * d2.x + d1.y * d2.y).abs();
                if turn > 1e-6 {
                    let radius = (a.dist(&b) + b.dist(&c2)) / 2.0 / turn;
                    if radius < reach + 1.0 {
                        return Err("the curve is too tight for this wall's thickness".into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Add a fully specified wall (location line, constraints and so on).
    pub fn add_wall_with(&mut self, w: Wall) -> Result<Id, String> {
        self.check_wall(&w)?;
        self.transact("Create wall", |tx| Ok(tx.add(Element::Wall(w))))
    }

    /// Add several walls as one undo step (a rectangle or polygon of walls).
    pub fn add_walls_with(&mut self, walls: Vec<Wall>) -> Result<Vec<Id>, String> {
        for w in &walls {
            self.check_wall(w)?;
        }
        if walls.is_empty() {
            return Err("no walls to add".into());
        }
        self.transact(if walls.len() == 1 { "Create wall" } else { "Create walls" }, |tx| Ok(walls.into_iter().map(|w| tx.add(Element::Wall(w))).collect()))
    }

    pub fn add_floor(&mut self, floor_type: Id, level: Id, boundary: Vec<Point>) -> Result<Id, String> {
        if boundary.len() < 3 || polygon_area_abs(&boundary) < 1.0 {
            return Err("floor boundary needs at least three points enclosing an area".into());
        }
        let n = boundary.len();
        let curves: Vec<Curve> = (0..n).map(|i| Curve::line(boundary[i], boundary[(i + 1) % n])).collect();
        self.add_floor_sketch(floor_type, level, curves, None, None)
    }

    /// Check a sketch the way Finish Edit Mode does and return its loops.
    pub fn check_floor_sketch(&self, curves: &[Curve]) -> Result<Vec<Vec<Curve>>, String> {
        let loops = crate::curve::sketch_loops(curves).map_err(|e| e.to_string())?;
        let polys: Vec<Vec<Point>> = loops.iter().map(|l| crate::curve::loop_polygon(l, 0.5)).collect();
        for r in crate::geom::regions(&polys) {
            if r.area() < 1.0 {
                return Err("the boundary encloses no area".into());
            }
            if crate::geom::triangulate_region(&r).is_none() {
                return Err("the boundary could not be filled: check it for crossing lines".into());
            }
        }
        Ok(loops)
    }

    /// Create a floor from sketch curves (the loose lines of a sketch), with an optional slope
    /// arrow and span direction. The sketch is checked and refused with a message if invalid.
    pub fn add_floor_sketch(&mut self, floor_type: Id, level: Id, curves: Vec<Curve>, slope: Option<SlopeArrow>, span: Option<SpanDirection>) -> Result<Id, String> {
        if self.floor_type(floor_type).is_none() {
            return Err("unknown floor type".into());
        }
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        let loops = self.check_floor_sketch(&curves)?;
        self.check_slope(slope.as_ref(), level)?;
        let mut f = Floor::new(floor_type, level, loops);
        f.slope = slope;
        f.span = span;
        self.transact("Create floor", |tx| Ok(tx.add(Element::Floor(f))))
    }

    /// Replace a floor's sketch (Edit Boundary, Finish Edit Mode), one undo step.
    pub fn set_floor_sketch(&mut self, id: Id, curves: Vec<Curve>, slope: Option<SlopeArrow>, span: Option<SpanDirection>) -> Result<(), String> {
        let Some(Element::Floor(f)) = self.get(id).cloned() else { return Err("not a floor".into()) };
        self.refuse_pinned(&[id], "edited")?;
        let loops = self.check_floor_sketch(&curves)?;
        self.check_slope(slope.as_ref(), f.level)?;
        let mut nf = f;
        nf.boundary.clear();
        nf.loops = loops;
        nf.slope = slope;
        nf.span = span;
        self.transact("Edit floor boundary", |tx| tx.update(id, Element::Floor(nf)))
    }

    fn check_slope(&self, s: Option<&SlopeArrow>, level: Id) -> Result<(), String> {
        let Some(s) = s else { return Ok(()) };
        if s.tail.dist(&s.head) < 1.0 {
            return Err("the slope arrow needs a tail and a head that are apart".into());
        }
        for l in [s.tail_level, s.head_level].into_iter().flatten() {
            if self.level(l).is_none() {
                return Err("the slope arrow refers to a level that does not exist".into());
            }
        }
        let _ = level;
        Ok(())
    }

    /// The surface a floor's top lies on: flat at its level plus offset, or tilted by its slope arrow.
    pub fn floor_surface(&self, f: &Floor) -> Option<FloorSurface> {
        let elev = |l: Option<Id>| -> Option<f64> { self.level(l.unwrap_or(f.level)).map(|l| l.elevation_mm) };
        let flat = elev(None)? + f.offset_mm;
        let Some(s) = &f.slope else {
            return Some(FloorSurface { base: flat, origin: Point::new(0.0, 0.0), dir: Point::new(1.0, 0.0), rise: 0.0 });
        };
        let len = s.tail.dist(&s.head);
        if len < 1e-6 {
            return Some(FloorSurface { base: flat, origin: s.tail, dir: Point::new(1.0, 0.0), rise: 0.0 });
        }
        let dir = Point::new((s.head.x - s.tail.x) / len, (s.head.y - s.tail.y) / len);
        let (tail_z, rise) = match s.spec {
            SlopeSpec::HeightAtTail { tail_offset_mm, head_offset_mm } => {
                let tz = elev(s.tail_level)? + tail_offset_mm + f.offset_mm;
                let hz = elev(s.head_level)? + head_offset_mm + f.offset_mm;
                (tz, (hz - tz) / len)
            }
            SlopeSpec::Slope { tail_offset_mm, percent } => (elev(s.tail_level)? + tail_offset_mm + f.offset_mm, percent / 100.0),
        };
        Some(FloorSurface { base: tail_z, origin: s.tail, dir, rise })
    }

    /// Whether an element is pinned (walls, floors, doors and windows can be).
    pub fn is_pinned(&self, id: Id) -> bool {
        match self.get(id) {
            Some(Element::Wall(w)) => w.pinned,
            Some(Element::Floor(f)) => f.pinned,
            Some(Element::Roof(r)) => r.pinned,
            Some(Element::FamilyInstance(i)) => i.pinned,
            Some(Element::ElevationMarker(m)) => m.pinned,
            Some(Element::Section(s)) => s.pinned,
            Some(Element::Level(l)) => l.pinned,
            Some(Element::Grid(g)) => g.pinned,
            _ => false,
        }
    }

    /// Error out when any of `ids` is pinned. `what` completes "cannot be ...".
    pub fn refuse_pinned(&self, ids: &[Id], what: &str) -> Result<(), String> {
        let n = ids.iter().filter(|i| self.is_pinned(**i)).count();
        match n {
            0 => Ok(()),
            1 => Err(format!("a pinned element cannot be {what}; unpin it first")),
            n => Err(format!("{n} pinned elements cannot be {what}; unpin them first")),
        }
    }

    /// Pin or unpin elements (Modify > Pin / Unpin), one undo step. Elements that
    /// cannot be pinned are skipped.
    pub fn set_pinned(&mut self, ids: &[Id], pinned: bool) -> Result<(), String> {
        let mut edits = vec![];
        for &id in ids {
            let mut e = match self.get(id) {
                Some(e @ (Element::Wall(_) | Element::Floor(_) | Element::Roof(_) | Element::FamilyInstance(_) | Element::ElevationMarker(_) | Element::Section(_) | Element::Level(_) | Element::Grid(_))) => e.clone(),
                _ => continue,
            };
            match &mut e {
                Element::Wall(w) => w.pinned = pinned,
                Element::Floor(f) => f.pinned = pinned,
                Element::Roof(r) => r.pinned = pinned,
                Element::FamilyInstance(i) => i.pinned = pinned,
                Element::ElevationMarker(m) => m.pinned = pinned,
                Element::Section(s) => s.pinned = pinned,
                Element::Level(l) => l.pinned = pinned,
                Element::Grid(g) => g.pinned = pinned,
                _ => {}
            }
            if self.get(id) != Some(&e) {
                edits.push((id, e));
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.transact(if pinned { "Pin" } else { "Unpin" }, |tx| {
            for (id, e) in edits {
                tx.update(id, e)?;
            }
            Ok(())
        })
    }

    pub fn delete(&mut self, ids: &[Id]) -> Result<(), String> {
        self.refuse_pinned(ids, "deleted")?;
        for id in ids {
            match self.get(*id) {
                Some(Element::Wall(_)) | Some(Element::Floor(_)) | Some(Element::Roof(_)) | Some(Element::View(_)) => {}
                Some(Element::ViewTemplate(_)) | Some(Element::FamilyInstance(_)) | Some(Element::Grid(_)) | Some(Element::Dimension(_)) | Some(Element::Spot(_)) => {}
                Some(Element::DetailLine(_)) | Some(Element::DetailRegion(_)) | Some(Element::TextNote(_)) => {}
                Some(_) => return Err("only walls, floors, roofs, doors, windows, grids, dimensions, spot annotations, detail lines, regions, text notes, views and view templates can be deleted for now".into()),
                None => return Err(format!("no element {id}")),
            }
        }
        // doors and windows go with the wall they are hosted in; detail items go with their view
        let mut ids: Vec<Id> = ids.to_vec();
        let views: Vec<Id> = ids.iter().copied().filter(|i| matches!(self.get(*i), Some(Element::View(_)))).collect();
        for (i, e) in self.iter() {
            let view = match e {
                Element::DetailLine(l) => Some(l.view),
                Element::DetailRegion(r) => Some(r.view),
                Element::TextNote(t) => Some(t.view),
                _ => None,
            };
            if view.is_some_and(|v| views.contains(&v)) && !ids.contains(&i) {
                ids.push(i);
            }
        }
        for h in self.hosted_in(&ids.clone()) {
            if !ids.contains(&h) {
                ids.push(h);
            }
        }
        let ids = &ids[..];
        let name = if ids.len() == 1 { "Delete element" } else { "Delete elements" };
        // views that used a deleted template stop pointing at it
        let orphans: Vec<(Id, View)> = self
            .views()
            .into_iter()
            .filter(|(_, v)| v.template.is_some_and(|t| ids.contains(&t)))
            .map(|(i, v)| (i, v.clone()))
            .collect();
        self.transact(name, |tx| {
            for (vid, v) in orphans {
                if !ids.contains(&vid) {
                    tx.update(vid, Element::View(View { template: None, ..v }))?;
                }
            }
            for id in ids {
                tx.remove(*id)?;
            }
            Ok(())
        })
    }

    pub fn set_wall_type(&mut self, id: Id, wall_type: Id) -> Result<(), String> {
        let Some(Element::Wall(w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        if self.wall_type(wall_type).is_none() {
            return Err("unknown wall type".into());
        }
        if w.wall_type == wall_type {
            return Ok(()); // nothing to do, and no empty undo step
        }
        self.transact("Change wall type", |tx| tx.update(id, Element::Wall(Wall { wall_type, ..w })))
    }

    pub fn set_floor_type(&mut self, id: Id, floor_type: Id) -> Result<(), String> {
        let Some(Element::Floor(f)) = self.get(id).cloned() else { return Err("not a floor".into()) };
        if self.floor_type(floor_type).is_none() {
            return Err("unknown floor type".into());
        }
        if f.floor_type == floor_type {
            return Ok(());
        }
        self.transact("Change floor type", |tx| tx.update(id, Element::Floor(Floor { floor_type, ..f })))
    }

    /// Change a floor's instance properties (Height Offset From Level, Structural, Pinned) with a JSON patch.
    pub fn patch_floor(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::Floor(f)) = self.get(id).cloned() else { return Err("not a floor".into()) };
        let nf: Floor = apply_patch(&f, patch)?;
        if nf == f {
            return Ok(());
        }
        if self.floor_type(nf.floor_type).is_none() {
            return Err("unknown floor type".into());
        }
        if self.level(nf.level).is_none() {
            return Err("unknown level".into());
        }
        if !nf.offset_mm.is_finite() {
            return Err("the offset must be a number".into());
        }
        self.transact("Change floor properties", |tx| tx.update(id, Element::Floor(nf)))
    }

    pub fn set_wall_height(&mut self, id: Id, height_mm: f64) -> Result<(), String> {
        let Some(Element::Wall(w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        if height_mm <= 0.0 {
            return Err("wall height must be positive".into());
        }
        if w.top_level.is_none() && w.height_mm == height_mm {
            return Ok(());
        }
        let nw = Wall { top_level: None, height_mm, ..w };
        self.check_hosted_fit(id, &nw)?;
        self.transact("Change wall height", |tx| tx.update(id, Element::Wall(nw)))
    }

    // ---- generic property edits (the Properties palette) -------------------

    /// Apply a JSON merge patch to a wall's properties as one undo step. Any
    /// stored property can be changed this way, so new properties need no new
    /// command. `null` resets a property to its default.
    pub fn patch_wall(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::Wall(w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        let nw: Wall = apply_patch(&w, patch)?;
        if nw == w {
            return Ok(());
        }
        if self.wall_type(nw.wall_type).is_none() {
            return Err("unknown wall type".into());
        }
        if self.level(nw.base_level).is_none() {
            return Err("unknown base level".into());
        }
        match nw.top_level {
            Some(t) if self.level(t).is_none() => return Err("unknown top level".into()),
            None if nw.height_mm <= 0.0 => return Err("wall height must be positive".into()),
            _ => {}
        }
        if nw.start.dist(&nw.end) < 1.0 {
            return Err("wall is too short".into());
        }
        let (z0, z1) = self.wall_extent(&nw);
        if z1 <= z0 {
            return Err("wall top must be above its base".into());
        }
        self.check_hosted_fit(id, &nw)?;
        self.transact("Change wall properties", |tx| tx.update(id, Element::Wall(nw)))
    }

    pub fn views(&self) -> Vec<(Id, &View)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::View(v) = e { Some((*id, v)) } else { None }).collect()
    }

    pub fn view(&self, id: Id) -> Option<&View> {
        match self.get(id) {
            Some(Element::View(v)) => Some(v),
            _ => None,
        }
    }

    pub fn view_templates(&self) -> Vec<(Id, &ViewTemplate)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::ViewTemplate(v) = e { Some((*id, v)) } else { None }).collect()
    }

    pub fn view_template(&self, id: Id) -> Option<&ViewTemplate> {
        match self.get(id) {
            Some(Element::ViewTemplate(v)) => Some(v),
            _ => None,
        }
    }

    /// View names are unique within a kind of view: a floor plan and a ceiling plan can both be called "Level 1".
    fn view_name_taken(&self, name: &str, kind: ViewKind, except: Option<Id>) -> bool {
        self.views().iter().any(|(id, v)| v.name == name && v.view_kind == kind && Some(*id) != except)
    }

    pub fn view_types(&self) -> Vec<(Id, &crate::view::ViewType)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::ViewType(t) = e { Some((*id, t)) } else { None }).collect()
    }

    pub fn view_type(&self, id: Id) -> Option<&crate::view::ViewType> {
        match self.get(id) {
            Some(Element::ViewType(t)) => Some(t),
            _ => None,
        }
    }

    /// Give a project the view types it needs (new projects, and projects saved before view types), and point
    /// the plans that have none at the type of their kind. Not an undo step.
    pub fn ensure_view_types(&mut self) {
        let have = |d: &Document, k: ViewKind| d.view_types().iter().any(|(_, t)| t.view_kind == k);
        if have(self, ViewKind::FloorPlan) && have(self, ViewKind::CeilingPlan) {
            return;
        }
        let _ = self.transact("View types", |tx| {
            let mut ids = Vec::new();
            for (name, kind) in [("Floor Plan", ViewKind::FloorPlan), ("Ceiling Plan", ViewKind::CeilingPlan)] {
                if !have(tx.doc_ref(), kind) {
                    ids.push((kind, tx.add(Element::ViewType(crate::view::ViewType { name: name.into(), view_kind: kind, template_for_new: None, new_views_dependent: true }))));
                }
            }
            let plans: Vec<(Id, View)> = tx.doc_ref().views().into_iter().filter(|(_, v)| v.view_type.is_none()).map(|(i, v)| (i, v.clone())).collect();
            for (vid, mut v) in plans {
                if let Some((_, t)) = ids.iter().find(|(k, _)| *k == v.view_kind) {
                    v.view_type = Some(*t);
                    tx.update(vid, Element::View(v))?;
                }
            }
            Ok(())
        });
        self.clear_history();
    }

    /// New Plan: a plan of `view_type`'s kind for each of `levels`. With `skip_existing` a level that already has a
    /// plan of that kind is left alone; otherwise a further one is made and numbered, "Level 1(1)". `scale` is 1:n.
    /// One undo step; the new views in level order.
    pub fn create_plan_views(&mut self, view_type: Id, levels: &[Id], skip_existing: bool, scale: Option<u32>) -> Result<Vec<Id>, String> {
        let vt = self.view_type(view_type).ok_or("unknown view type")?.clone();
        if !matches!(vt.view_kind, ViewKind::FloorPlan | ViewKind::CeilingPlan) {
            return Err("only floor plans and ceiling plans can be made here so far".into());
        }
        if levels.is_empty() {
            return Err("pick at least one level".into());
        }
        if scale == Some(0) {
            return Err("scale must be positive".into());
        }
        let mut todo: Vec<(Id, String)> = Vec::new();
        for &l in levels {
            let lv = self.level(l).ok_or("unknown level")?;
            let has = self.views().iter().any(|(_, v)| v.view_kind == vt.view_kind && v.level == Some(l));
            if has && skip_existing {
                continue;
            }
            if !todo.iter().any(|(x, _)| *x == l) {
                todo.push((l, lv.name.clone()));
            }
        }
        todo.sort_by(|a, b| {
            let z = |id: Id| self.level(id).map_or(0.0, |l| l.elevation_mm);
            z(a.0).total_cmp(&z(b.0))
        });
        let template = vt.template_for_new.filter(|t| self.view_template(*t).is_some_and(|x| x.view_kind == vt.view_kind));
        let mut made: Vec<(Id, View)> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for (l, base) in todo {
            let taken = |d: &Document, n: &str, names: &[String]| d.view_name_taken(n, vt.view_kind, None) || names.iter().any(|x| x == n);
            let mut name = base.clone();
            let mut k = 1;
            while taken(self, &name, &names) {
                name = format!("{base}({k})");
                k += 1;
            }
            let mut v = if vt.view_kind == ViewKind::CeilingPlan { View::ceiling_plan(&name, l) } else { View::floor_plan(&name, l) };
            v.view_type = Some(view_type);
            if let Some(s) = scale {
                v.settings.scale = s;
            }
            if let Some(t) = template {
                if vt.new_views_dependent {
                    v.template = Some(t);
                } else if let Some(tt) = self.view_template(t) {
                    v.settings = tt.settings.clone();
                }
            }
            names.push(name);
            made.push((0, v));
        }
        if made.is_empty() {
            return Err("every level picked already has a plan of this kind (clear Do not duplicate existing views to make another)".into());
        }
        let label = if vt.view_kind == ViewKind::CeilingPlan { "Create ceiling plan" } else { "Create floor plan" };
        self.transact(label, |tx| Ok(made.into_iter().map(|(_, v)| tx.add(Element::View(v))).collect()))
    }

    /// Apply a JSON merge patch to a view (name, scale, view range, crop,
    /// graphics overrides, template link, and so on).
    pub fn patch_view(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(v) = self.view(id).cloned() else { return Err("not a view".into()) };
        let nv: View = apply_patch(&v, patch)?;
        if nv == v {
            return Ok(());
        }
        if nv.name.trim().is_empty() {
            return Err("a view needs a name".into());
        }
        if self.view_name_taken(&nv.name, nv.view_kind, Some(id)) {
            return Err(format!("a view named '{}' already exists", nv.name));
        }
        if let Some(l) = nv.level {
            if self.level(l).is_none() {
                return Err("unknown level".into());
            }
        }
        if nv.view_kind.is_plan() && nv.level.is_none() {
            return Err("a plan view needs a level".into());
        }
        if nv.settings.scale == 0 {
            return Err("scale must be positive".into());
        }
        check_depth_cue(&nv.settings)?;
        if let Some(t) = nv.template {
            match self.view_template(t) {
                None => return Err("unknown view template".into()),
                Some(t) if t.view_kind != nv.view_kind => return Err("that template is for a different kind of view".into()),
                _ => {}
            }
        }
        if nv.view_kind.is_plan() {
            let eff = if nv.template.is_some() { self.effective_settings_of(&nv) } else { nv.settings.clone() };
            if let Some(r) = self.resolve_view_range(&nv, &eff) {
                if !(r.top >= r.cut && r.cut >= r.bottom && r.bottom >= r.depth) {
                    return Err("view range must run from the top plane down through the cut plane and bottom plane to the view depth".into());
                }
            }
        }
        for l in [nv.underlay.base_level, nv.underlay.top_level].into_iter().flatten() {
            if self.level(l).is_none() {
                return Err("unknown underlay level".into());
            }
        }
        self.transact("Change view properties", |tx| tx.update(id, Element::View(nv)))
    }

    pub fn patch_view_template(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(t) = self.view_template(id).cloned() else { return Err("not a view template".into()) };
        let nt: ViewTemplate = apply_patch(&t, patch)?;
        if nt == t {
            return Ok(());
        }
        if nt.name.trim().is_empty() {
            return Err("a view template needs a name".into());
        }
        if self.view_templates().iter().any(|(i, o)| *i != id && o.name == nt.name) {
            return Err(format!("a view template named '{}' already exists", nt.name));
        }
        check_depth_cue(&nt.settings)?;
        self.transact("Change view template", |tx| tx.update(id, Element::ViewTemplate(nt)))
    }

    /// Duplicate a view ("Level 1 Copy 1"). Detailing and dependent copies come later.
    pub fn duplicate_view(&mut self, id: Id) -> Result<Id, String> {
        let Some(v) = self.view(id).cloned() else { return Err("not a view".into()) };
        let mut n = 1;
        let name = loop {
            let c = format!("{} Copy {n}", v.name);
            if !self.view_name_taken(&c, v.view_kind, None) {
                break c;
            }
            n += 1;
        };
        self.transact("Duplicate view", |tx| Ok(tx.add(Element::View(View { name, ..v }))))
    }

    pub fn add_floor_plan(&mut self, level: Id, name: &str) -> Result<Id, String> {
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        if name.trim().is_empty() || self.view_name_taken(name, ViewKind::FloorPlan, None) {
            return Err("view name is empty or already used".into());
        }
        self.transact("Create floor plan", |tx| Ok(tx.add(Element::View(View::floor_plan(name, level)))))
    }

    /// New view template from a view's current settings (all properties included).
    pub fn create_view_template(&mut self, name: &str, from_view: Id) -> Result<Id, String> {
        let Some(v) = self.view(from_view).cloned() else { return Err("not a view".into()) };
        if name.trim().is_empty() || self.view_templates().iter().any(|(_, t)| t.name == name) {
            return Err("template name is empty or already used".into());
        }
        let t = ViewTemplate {
            name: name.into(),
            view_kind: v.view_kind,
            included: TemplateProp::ALL.into_iter().collect(),
            settings: self.effective_settings(from_view).unwrap_or(v.settings),
        };
        self.transact("Create view template", |tx| Ok(tx.add(Element::ViewTemplate(t))))
    }

    /// The settings a view actually uses: its own, with every property its
    /// template includes taken from the template.
    pub fn effective_settings(&self, id: Id) -> Option<ViewSettings> {
        Some(self.effective_settings_of(self.view(id)?))
    }

    /// Like `effective_settings` for a view value that may not be stored yet.
    pub fn effective_settings_of(&self, v: &View) -> ViewSettings {
        let mut s = v.settings.clone();
        if let Some(t) = v.template.and_then(|t| self.view_template(t)) {
            for p in &t.included {
                s.take(&t.settings, *p);
            }
        }
        s
    }

    /// Elevations of a plan view's range planes.
    pub fn resolve_view_range(&self, view: &View, settings: &ViewSettings) -> Option<ResolvedRange> {
        let assoc = self.level(view.level?)?.elevation_mm;
        let mut elevs: Vec<f64> = self.levels().iter().map(|(_, l)| l.elevation_mm).collect();
        elevs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let plane = |p: &RangePlane, unlimited: f64| -> f64 {
            let base = match p.level {
                RangeLevel::Associated => Some(assoc),
                RangeLevel::Above => elevs.iter().copied().find(|e| *e > assoc + 1e-6),
                RangeLevel::Below => elevs.iter().copied().rev().find(|e| *e < assoc - 1e-6),
                RangeLevel::Level(id) => self.level(id).map(|l| l.elevation_mm),
                RangeLevel::Unlimited => None,
            };
            base.map_or(unlimited, |b| b + p.offset_mm)
        };
        let r = &settings.view_range;
        Some(ResolvedRange {
            top: plane(&r.top, f64::INFINITY),
            cut: assoc + r.cut_offset_mm,
            bottom: plane(&r.bottom, f64::NEG_INFINITY),
            depth: plane(&r.depth, f64::NEG_INFINITY),
        })
    }

    // ---- derived values ---------------------------------------------------

    /// Base and top elevation of a wall, mm.
    pub fn wall_extent(&self, w: &Wall) -> (f64, f64) {
        let base = self.level(w.base_level).map_or(0.0, |l| l.elevation_mm) + w.base_offset_mm;
        let top = match w.top_level.and_then(|t| self.level(t)) {
            Some(l) => l.elevation_mm + w.top_offset_mm,
            None => base + w.height_mm,
        };
        (base, top)
    }

    /// How far the wall body reaches to the left and right of its location line
    /// (mm), looking from start to end. The exterior face is on the left.
    pub fn wall_extents(&self, w: &Wall) -> (f64, f64) {
        // the location line names a face or the core relative to the exterior, so a
        // flipped wall measures the same line and then mirrors across it
        let (l, r) = self.wall_type(w.wall_type).map_or((50.0, 50.0), |t| t.extents(w.location_line));
        if w.flipped { (r, l) } else { (l, r) }
    }

    /// The wall's core: its thickness, and how far its centre lies to the left of
    /// the centre of the whole wall body (mm), looking from start to end.
    pub fn wall_core(&self, w: &Wall) -> (f64, f64) {
        let Some(t) = self.wall_type_for(w) else { return (0.0, 0.0) };
        let (cs, ce) = t.core_span();
        let (wl, wr) = self.wall_extents(w);
        ((ce - cs).max(0.0), (wl - (cs + ce) / 2.0) - (wl - wr) / 2.0)
    }

    /// The wall's type as it sits in this wall: layers reversed (and exterior and
    /// interior wrapping swapped) when the wall is flipped. Everything that lays
    /// layers out across the wall goes through this.
    pub fn wall_type_for(&self, w: &Wall) -> Option<crate::element::WallType> {
        use crate::element::Wrap;
        let mut t = self.wall_type(w.wall_type)?.clone();
        if w.flipped {
            t.layers.reverse();
            let swap = |x: Wrap| match x {
                Wrap::Exterior => Wrap::Interior,
                Wrap::Interior => Wrap::Exterior,
                o => o,
            };
            t.wrap_ends = swap(t.wrap_ends);
            t.wrap_inserts = swap(t.wrap_inserts);
        }
        Some(t)
    }

    /// Flip walls (Spacebar or the flip arrows) as one undo step.
    pub fn flip_walls(&mut self, ids: &[Id]) -> Result<(), String> {
        self.refuse_pinned(ids, "flipped")?;
        let mut edits = vec![];
        for &id in ids {
            let Some(Element::Wall(w)) = self.get(id) else { continue };
            let mut nw = w.clone();
            nw.flipped = !nw.flipped;
            edits.push((id, nw));
        }
        if edits.is_empty() {
            return Err("select walls to flip".into());
        }
        self.transact("Flip wall", |tx| {
            for (id, nw) in edits {
                tx.update(id, Element::Wall(nw))?;
            }
            Ok(())
        })
    }

    /// Walls whose end `which` (0 = start, 1 = end) sits on the same node as this
    /// wall's, on the same level, including the wall itself.
    pub fn walls_at_end(&self, id: Id, which: u8) -> Vec<(Id, u8)> {
        let Some(Element::Wall(w)) = self.get(id) else { return vec![] };
        let p = if which == 0 { w.start } else { w.end };
        let mut v = vec![];
        for (oid, e) in self.iter() {
            if let Element::Wall(o) = e {
                if o.base_level != w.base_level {
                    continue;
                }
                for (ow, op) in [(0u8, o.start), (1u8, o.end)] {
                    if op.dist(&p) <= 1.0 {
                        v.push((oid, ow));
                    }
                }
            }
        }
        v
    }

    /// Change how a wall end joins (what the Wall Joins tool does).
    ///
    /// `allowed` applies to this end only (Disallow Join). `kind`, `through` and
    /// `cleanup` apply to every wall end at the same node so the join stays
    /// consistent. `through` names the wall that should run through; the others
    /// butt into it.
    pub fn set_wall_join(&mut self, id: Id, which: u8, c: JoinChange) -> Result<(), String> {
        if which > 1 {
            return Err("end must be 0 (start) or 1 (end)".into());
        }
        let Some(Element::Wall(_)) = self.get(id) else { return Err("not a wall".into()) };
        let at = self.walls_at_end(id, which);
        if let Some(t) = c.through {
            if !at.iter().any(|(w, _)| *w == t) {
                return Err("that wall is not part of this join".into());
            }
        }
        let mut edits: Vec<(Id, Wall)> = vec![];
        for (wid, end) in at {
            let Some(Element::Wall(w)) = self.get(wid).cloned() else { continue };
            let mut nw = w.clone();
            let j = if end == 0 { &mut nw.start_join } else { &mut nw.end_join };
            if wid == id && end == which {
                if let Some(a) = c.allowed {
                    j.allowed = a;
                }
            }
            if let Some(k) = c.kind {
                j.kind = k;
            }
            if let Some(cl) = c.cleanup {
                j.cleanup = cl;
            }
            if let Some(t) = c.through {
                j.through = Some(wid == t);
            }
            if nw != w {
                edits.push((wid, nw));
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.transact("Change wall join", |tx| {
            for (wid, nw) in edits {
                tx.update(wid, Element::Wall(nw))?;
            }
            Ok(())
        })
    }

    /// The walls as join input. A curved wall comes as one wall per facet of its path, mitred
    /// to the next, so joins and layers work on it the way they do on straight walls; ids carry
    /// the wall and the facet number (see `wallpath`).
    pub(crate) fn join_walls_list(&self) -> Vec<crate::join::JoinWall> {
        self.iter().filter_map(|(id, e)| if let Element::Wall(w) = e { Some(self.wall_facets(id, w)) } else { None }).flatten().collect()
    }

    pub(crate) fn wall_facets(&self, id: Id, w: &Wall) -> Vec<crate::join::JoinWall> {
        use crate::wallpath::facet_id;
        let (left, right) = self.wall_extents(w);
        let facets = w.path().facets();
        let n = facets.len();
        let inner = EndJoin { kind: JoinKind::Miter, ..EndJoin::default() };
        facets
            .into_iter()
            .enumerate()
            .map(|(k, (a, b))| crate::join::JoinWall {
                id: facet_id(id, k),
                level: w.base_level,
                start: a,
                end: b,
                left,
                right,
                start_join: if k == 0 { w.start_join } else { inner },
                end_join: if k + 1 == n { w.end_join } else { inner },
            })
            .collect()
    }

    /// Joined plan outline of every straight wall (corners mitred, T junctions butted).
    /// Curved walls are left out; see `wall_polygons`.
    pub fn wall_outlines(&self) -> BTreeMap<Id, [Point; 4]> {
        let mut by_facet = crate::join::join_walls(&self.join_walls_list());
        self.iter()
            .filter_map(|(id, e)| match e {
                Element::Wall(w) if w.curve.is_none() => by_facet.remove(&crate::wallpath::facet_id(id, 0)).map(|o| (id, o)),
                _ => None,
            })
            .collect()
    }

    /// Joined plan outline of every wall as a polygon: the quad of a straight wall, and for a
    /// curved wall the left side from start to end then the right side back.
    pub fn wall_polygons(&self) -> BTreeMap<Id, Vec<Point>> {
        use crate::wallpath::{facet_id};
        let by_facet = crate::join::join_walls(&self.join_walls_list());
        let mut out = BTreeMap::new();
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            let n = w.path().pts.len() - 1;
            let quads: Vec<&[Point; 4]> = (0..n).filter_map(|k| by_facet.get(&facet_id(id, k))).collect();
            if quads.len() != n || n == 0 {
                continue;
            }
            // each quad is [start-right, end-right, end-left, start-left]
            let mut poly: Vec<Point> = quads.iter().map(|q| q[3]).collect();
            poly.push(quads[n - 1][2]);
            poly.push(quads[n - 1][1]);
            poly.extend(quads.iter().rev().map(|q| q[0]));
            out.insert(id, poly);
        }
        out
    }

    /// Wall ends that land on another wall (T joins): `(wall, end)` to the host wall.
    /// Ends whose join is disallowed are left out.
    pub fn mid_join_hosts(&self) -> BTreeMap<(Id, u8), Id> {
        use crate::wallpath::{facet_no, wall_of};
        let walls = self.join_walls_list();
        let mut out = BTreeMap::new();
        for n in crate::join::classify(&walls) {
            if let crate::join::Node::Mid { e, other } = n {
                let (wall, host) = (wall_of(e.id), wall_of(walls[other].id));
                // only the ends of the wall itself, not the joints between its facets
                let last = self.get(wall).map_or(0, |w| if let Element::Wall(w) = w { w.path().pts.len() - 2 } else { 0 });
                let k = facet_no(e.id);
                if (e.which == 0 && k == 0) || (e.which == 1 && k == last) {
                    out.insert((wall, e.which), host);
                }
            }
        }
        out
    }

    pub fn wall_length_mm(w: &Wall) -> f64 {
        w.length()
    }
}

/// Depth cueing fades from a start to an end along the view depth (the start no further than the end) down to a
/// fade limit, all percentages.
fn check_depth_cue(s: &ViewSettings) -> Result<(), String> {
    let g = &s.graphic_display;
    if g.depth_cue_start > 100 || g.depth_cue_end > 100 || g.depth_cue_fade > 100 {
        return Err("depth cueing values are percentages from 0 to 100".into());
    }
    if g.depth_cue_start > g.depth_cue_end {
        return Err("depth cueing must start no further back than it ends".into());
    }
    Ok(())
}

/// Serialise `value`, merge `patch` into it (RFC 7396), and read it back.
pub(crate) fn apply_patch<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T, patch: &serde_json::Value) -> Result<T, String> {
    fn merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
        use serde_json::Value;
        if let Value::Object(p) = patch {
            if !target.is_object() {
                *target = Value::Object(Default::default());
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        } else {
            *target = patch.clone();
        }
    }
    let mut v = serde_json::to_value(value).map_err(|e| e.to_string())?;
    merge(&mut v, patch);
    serde_json::from_value(v).map_err(|e| format!("bad value: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn setup() -> (Document, Id, Id) {
        let d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[0].0;
        (d, lvl, wt)
    }

    #[test]
    fn new_project_has_defaults_and_is_clean() {
        let (d, _, _) = setup();
        assert_eq!(d.levels().len(), 2);
        assert!(d.wall_types().len() >= 4);
        assert!(d.floor_types().len() >= 2);
        assert_eq!(d.undo_name(), None);
        let brick = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().1;
        assert_eq!(brick.thickness_mm(), 253.0);
    }

    #[test]
    fn flipping_a_wall_mirrors_its_layers_across_the_location_line() {
        let (mut d, lvl, _) = setup();
        let brick = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let w = d.add_wall(brick, lvl, p(0., 0.), p(5000., 0.), 2700.).unwrap();
        let first_layer_y = |d: &Document| {
            let g = d.wall_layer_geometry();
            let l = &g[&w].layers[0];
            let ring = &l.shapes[0][0];
            (l.material.clone(), ring.iter().map(|q| q.y).sum::<f64>() / ring.len() as f64)
        };
        let (m0, y0) = first_layer_y(&d);
        let (el, er) = {
            let Some(Element::Wall(x)) = d.get(w) else { panic!() };
            d.wall_extents(x)
        };
        d.flip_walls(&[w]).unwrap();
        assert_eq!(d.undo_name(), Some("Flip wall"));
        let (m1, _) = first_layer_y(&d);
        // the exterior layer is now on the right of the line, and the first layer left is the old interior one
        assert!(y0 > 0.0 || el > er, "exterior starts on the left");
        assert_ne!(m0, m1);
        let Some(Element::Wall(x)) = d.get(w) else { panic!() };
        assert_eq!(d.wall_extents(x), (er, el));
        let g = d.wall_layer_geometry();
        let last = g[&w].layers.last().unwrap();
        assert_eq!(last.material, m0, "old first layer is now last");
        let ring = &last.shapes[0][0];
        assert!(ring.iter().map(|q| q.y).sum::<f64>() < 0.0, "and sits on the right");
        d.undo();
        assert_eq!(first_layer_y(&d).0, m0);
    }

    #[test]
    fn undo_redo_walls() {
        let (mut d, lvl, wt) = setup();
        let w = d.add_wall(wt, lvl, p(0., 0.), p(5000., 0.), 2700.).unwrap();
        assert!(d.get(w).is_some());
        assert_eq!(d.undo_name(), Some("Create wall"));
        assert_eq!(d.undo(), Some("Create wall".to_string()));
        assert!(d.get(w).is_none());
        assert_eq!(d.redo_name(), Some("Create wall"));
        d.redo();
        assert!(d.get(w).is_some());
        // a new change clears the redo stack
        d.undo();
        d.add_wall(wt, lvl, p(0., 0.), p(1000., 0.), 2700.).unwrap();
        assert_eq!(d.redo_name(), None);
    }

    #[test]
    fn failed_transaction_rolls_back() {
        let (mut d, lvl, wt) = setup();
        let before = d.iter().count();
        let r: Result<(), String> = d.transact("bad", |tx| {
            tx.add(Element::Level(Level::new("x", 1.0)));
            tx.remove(9999)
        });
        assert!(r.is_err());
        assert_eq!(d.iter().count(), before);
        assert!(d.add_wall(wt, lvl, p(0., 0.), p(0., 0.5), 2700.).is_err());
        assert!(d.add_wall(wt, 9999, p(0., 0.), p(1000., 0.), 2700.).is_err());
    }

    #[test]
    fn change_type_and_height_undo() {
        let (mut d, lvl, wt) = setup();
        let other = d.wall_types()[1].0;
        let w = d.add_wall(wt, lvl, p(0., 0.), p(5000., 0.), 2700.).unwrap();
        d.set_wall_type(w, other).unwrap();
        d.set_wall_type(w, other).unwrap(); // repeating a change adds no undo step
        d.set_wall_height(w, 3300.).unwrap();
        d.set_wall_height(w, 3300.).unwrap();
        let Some(Element::Wall(x)) = d.get(w) else { panic!() };
        assert_eq!((x.wall_type, x.height_mm), (other, 3300.0));
        d.undo();
        d.undo();
        let Some(Element::Wall(x)) = d.get(w) else { panic!() };
        assert_eq!((x.wall_type, x.height_mm), (wt, 2700.0));
    }

    #[test]
    fn floors_validate_boundaries() {
        let (mut d, lvl, _) = setup();
        let ft = d.floor_types()[0].0;
        assert!(d.add_floor(ft, lvl, vec![p(0., 0.), p(1., 1.)]).is_err());
        // bow-tie crosses itself
        assert!(d.add_floor(ft, lvl, vec![p(0., 0.), p(4000., 4000.), p(4000., 0.), p(0., 4000.)]).is_err());
        let f = d.add_floor(ft, lvl, vec![p(0., 0.), p(4000., 0.), p(4000., 3000.), p(0., 3000.)]).unwrap();
        d.delete(&[f]).unwrap();
        assert!(d.get(f).is_none());
        d.undo();
        assert!(d.get(f).is_some());
    }

    fn quarter_wall(d: &mut Document, lvl: Id, wt: Id, r: f64) -> Id {
        let mut w = Wall::new(wt, lvl, p(r, 0.), p(0., r), 2700.);
        w.curve = Some(Curve::Arc { start: p(r, 0.), end: p(0., r), centre: p(0., 0.), ccw: true });
        d.add_wall_with(w).unwrap()
    }

    #[test]
    fn a_curved_wall_gets_one_smooth_shape_per_layer_with_the_right_area() {
        let (mut d, lvl, _) = setup();
        let brick = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let w = quarter_wall(&mut d, lvl, brick, 5000.0);
        let g = d.wall_layer_geometry();
        let layers = &g[&w].layers;
        assert!(layers.len() >= 3);
        let thick: f64 = layers.iter().map(|l| l.thickness).sum();
        let mut area = 0.0;
        for l in layers {
            // each layer is one piece (the facets merge) following the curve
            assert_eq!(l.shapes.len(), 1, "{} should be one piece", l.material);
            assert!(l.shapes[0][0].len() > 20);
            assert_eq!(l.shapes[0].len(), 1, "no holes or slivers");
            area += polygon_area_abs(&l.shapes[0][0]);
        }
        // the band between two arcs: thickness x arc length at the middle of the wall
        let (left, right) = d.wall_extents(match d.get(w) { Some(Element::Wall(w)) => w, _ => panic!() });
        let mid = 5000.0 + (right - left) / 2.0; // left of an anticlockwise path is towards the centre
        let want = thick * mid * std::f64::consts::FRAC_PI_2;
        assert!((area - want).abs() / want < 0.01, "area {area} vs {want}");
        assert_eq!(g[&w].free, [true, true]);
        let poly = &d.wall_polygons()[&w];
        assert!(poly.len() > 40);
        assert!((polygon_area_abs(poly) - want).abs() / want < 0.01);
        assert!(d.wall_outlines().get(&w).is_none());
    }

    #[test]
    fn a_straight_wall_butts_into_the_end_of_a_curved_wall_like_any_other_join() {
        let (mut d, lvl, wt) = setup();
        let c = quarter_wall(&mut d, lvl, wt, 4000.0);
        let s = d.add_wall(wt, lvl, p(0., 4000.), p(-3000., 4000.), 2700.).unwrap();
        // the straight wall starts where the arc ends: they join, so neither end is free
        let g = d.wall_layer_geometry();
        assert!(!g[&c].free[1] && !g[&s].free[0]);
        assert!(g[&c].free[0] && g[&s].free[1]);
        // the join shows in the outlines: the straight wall's start corners sit on the arc wall's end faces
        let o = d.wall_outlines()[&s];
        assert!(o[0].dist(&p(0., 4000.)) < 200.0 && o[3].dist(&p(0., 4000.)) < 200.0);
        // and the host of an end that lands on a curved wall is found by wall, not by facet
        let t = d.add_wall(wt, lvl, p(4000. * 0.7071, 4000. * 0.7071), p(2000., 2000.), 2700.).unwrap();
        let hosts = d.mid_join_hosts();
        assert_eq!(hosts.get(&(t, 0)), Some(&c));
    }

    #[test]
    fn a_curve_that_bends_tighter_than_the_wall_is_thick_is_refused() {
        let (mut d, lvl, _) = setup();
        let brick = d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        let mut w = Wall::new(brick, lvl, p(100., 0.), p(0., 100.), 2700.);
        w.curve = Some(Curve::Arc { start: p(100., 0.), end: p(0., 100.), centre: p(0., 0.), ccw: true });
        assert!(d.add_wall_with(w).unwrap_err().contains("too tight"));
        // a closed curve is not a wall either
        let mut c = Wall::new(brick, lvl, p(5000., 0.), p(5000., 0.), 2700.);
        c.curve = Some(Curve::Circle { centre: p(0., 0.), radius: 5000.0 });
        assert!(d.add_wall_with(c).is_err());
        // an arc whose ends are not the wall's ends
        let mut m = Wall::new(brick, lvl, p(1., 0.), p(0., 5000.), 2700.);
        m.curve = Some(Curve::Arc { start: p(5000., 0.), end: p(0., 5000.), centre: p(0., 0.), ccw: true });
        assert!(d.add_wall_with(m).unwrap_err().contains("ends"));
    }

    #[test]
    fn wall_extent_follows_levels() {
        let (mut d, lvl, wt) = setup();
        let w = d.add_wall(wt, lvl, p(0., 0.), p(5000., 0.), 2700.).unwrap();
        let Some(Element::Wall(mut wall)) = d.get(w).cloned() else { panic!() };
        assert_eq!(d.wall_extent(&wall), (0.0, 2700.0));
        wall.top_level = Some(d.levels()[1].0);
        wall.top_offset_mm = -200.0;
        assert_eq!(d.wall_extent(&wall), (0.0, 2800.0));
    }

    #[test]
    fn wall_join_settings_apply_to_the_whole_node_and_undo() {
        let (mut d, lvl, wt) = setup();
        let a = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        let b = d.add_wall(wt, lvl, p(4000., 0.), p(4000., 3000.), 2700.).unwrap();
        let before = d.wall_outlines();
        d.set_wall_join(a, 1, JoinChange { kind: Some(JoinKind::Miter), ..Default::default() }).unwrap();
        // both ends at the node changed together
        let Some(Element::Wall(wa)) = d.get(a) else { panic!() };
        let Some(Element::Wall(wb)) = d.get(b) else { panic!() };
        assert_eq!(wa.end_join.kind, JoinKind::Miter);
        assert_eq!(wb.start_join.kind, JoinKind::Miter);
        assert_ne!(d.wall_outlines()[&a], before[&a]);
        // making b run through flips the roles
        d.set_wall_join(b, 0, JoinChange { kind: Some(JoinKind::Butt), through: Some(b), ..Default::default() }).unwrap();
        let Some(Element::Wall(wb)) = d.get(b) else { panic!() };
        assert_eq!(wb.start_join.through, Some(true));
        // repeating the same change is a no-op with no undo step
        let depth = d.undo_name().map(|s| s.to_string());
        d.set_wall_join(b, 0, JoinChange { through: Some(b), ..Default::default() }).unwrap();
        assert_eq!(d.undo_name().map(|s| s.to_string()), depth);
        d.undo();
        d.undo();
        assert_eq!(d.wall_outlines(), before);
        assert!(d.set_wall_join(a, 1, JoinChange { through: Some(999), ..Default::default() }).is_err());
    }

    #[test]
    fn disallow_join_affects_only_that_end() {
        let (mut d, lvl, wt) = setup();
        let a = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        let b = d.add_wall(wt, lvl, p(4000., 0.), p(4000., 3000.), 2700.).unwrap();
        d.set_wall_join(a, 1, JoinChange { allowed: Some(false), ..Default::default() }).unwrap();
        let Some(Element::Wall(wa)) = d.get(a) else { panic!() };
        let Some(Element::Wall(wb)) = d.get(b) else { panic!() };
        assert!(!wa.end_join.allowed);
        assert!(wb.start_join.allowed);
    }

    #[test]
    fn walls_without_join_fields_still_load() {
        // a wall saved before join settings existed
        let json = r#"{"kind":"Wall","wall_type":1,"start":{"x":0,"y":0},"end":{"x":1000,"y":0},"base_level":1,"top_level":null,"height_mm":2700}"#;
        let e: Element = serde_json::from_str(json).unwrap();
        let Element::Wall(w) = e else { panic!() };
        assert!(w.start_join.allowed && w.end_join.kind == JoinKind::Butt);
    }

    #[test]
    fn location_line_moves_the_wall_body_across_its_layers() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let brick_veneer = d.wall_types()[1].0; // 110 brick, 40 cavity, 90 frame (core), 13 board = 253
        let id = d.add_wall(brick_veneer, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        let ext = |d: &Document| {
            let Some(Element::Wall(w)) = d.get(id) else { panic!() };
            d.wall_extents(w)
        };
        assert_eq!(ext(&d), (126.5, 126.5));
        let set = |d: &mut Document, ll| d.patch_wall(id, &serde_json::json!({ "location_line": ll })).unwrap();
        set(&mut d, "finish_face_exterior");
        assert_eq!(ext(&d), (0.0, 253.0));
        set(&mut d, "finish_face_interior");
        assert_eq!(ext(&d), (253.0, 0.0));
        set(&mut d, "core_face_exterior"); // core starts 150 in
        assert_eq!(ext(&d), (150.0, 103.0));
        set(&mut d, "core_centreline"); // core 150..240
        assert_eq!(ext(&d), (195.0, 58.0));
        d.undo();
        assert_eq!(ext(&d), (150.0, 103.0));
    }

    #[test]
    fn new_project_has_a_plan_per_level_and_a_3d_view() {
        let d = Document::new_project();
        let names: Vec<&str> = d.views().iter().map(|(_, v)| v.name.as_str()).collect();
        assert_eq!(names, ["Ground", "Level 1", "{3D}", "Site", "North", "South", "East", "West"]);
        let (id, v) = d.views()[1];
        let r = d.resolve_view_range(v, &d.effective_settings(id).unwrap()).unwrap();
        assert_eq!((r.top, r.cut, r.bottom, r.depth), (5300.0, 4200.0, 3000.0, 3000.0));
    }

    #[test]
    fn view_range_levels_resolve() {
        let mut d = Document::new_project();
        let (id, _) = d.views()[0];
        d.patch_view(id, &serde_json::json!({ "settings": { "view_range": {
            "top": { "level": "above", "offset_mm": 0.0 },
            "bottom": { "level": "unlimited", "offset_mm": 0.0 },
            "depth": { "level": "below", "offset_mm": 0.0 }
        }}})).unwrap();
        let v = d.view(id).unwrap();
        let r = d.resolve_view_range(v, &d.effective_settings(id).unwrap()).unwrap();
        // Ground is the lowest level: nothing below it
        assert_eq!((r.top, r.cut, r.bottom, r.depth), (3000.0, 1200.0, f64::NEG_INFINITY, f64::NEG_INFINITY));
    }

    #[test]
    fn view_edits_are_validated_and_undoable() {
        let mut d = Document::new_project();
        let (a, _) = d.views()[0];
        d.patch_view(a, &serde_json::json!({ "settings": { "scale": 50, "detail_level": "fine" } })).unwrap();
        assert_eq!(d.view(a).unwrap().settings.scale, 50);
        assert!(d.patch_view(a, &serde_json::json!({ "name": "Level 1" })).is_err()); // taken
        assert!(d.patch_view(a, &serde_json::json!({ "settings": { "scale": 0 } })).is_err());
        assert!(d.patch_view(a, &serde_json::json!({ "level": 9999 })).is_err());
        let depth = d.undo_name().map(str::to_string);
        d.patch_view(a, &serde_json::json!({ "settings": { "scale": 50 } })).unwrap(); // no change, no undo step
        assert_eq!(d.undo_name().map(str::to_string), depth);
        d.undo();
        assert_eq!(d.view(a).unwrap().settings.scale, 100);
    }

    #[test]
    fn templates_control_only_their_included_properties() {
        let mut d = Document::new_project();
        let (a, b) = (d.views()[0].0, d.views()[1].0);
        d.patch_view(a, &serde_json::json!({ "settings": { "scale": 50, "detail_level": "fine" } })).unwrap();
        let t = d.create_view_template("Working Plan", a).unwrap();
        // only scale is included
        d.patch_view_template(t, &serde_json::json!({ "included": ["scale"] })).unwrap();
        d.patch_view(b, &serde_json::json!({ "template": t, "settings": { "detail_level": "coarse" } })).unwrap();
        let eff = d.effective_settings(b).unwrap();
        assert_eq!(eff.scale, 50); // from the template
        assert_eq!(eff.detail_level, DetailLevel::Coarse); // the view's own
        // changing the template flows through to the linked view
        d.patch_view_template(t, &serde_json::json!({ "settings": { "scale": 200 } })).unwrap();
        assert_eq!(d.effective_settings(b).unwrap().scale, 200);
        // deleting the template unlinks the view in the same undo step
        d.delete(&[t]).unwrap();
        assert_eq!(d.view(b).unwrap().template, None);
        d.undo();
        assert_eq!(d.view(b).unwrap().template, Some(t));
        // a template for another kind of view is refused
        let three_d = d.views()[2].0;
        assert!(d.patch_view(three_d, &serde_json::json!({ "template": t })).is_err());
    }

    #[test]
    fn category_overrides_round_trip_through_a_patch() {
        let mut d = Document::new_project();
        let (a, _) = d.views()[0];
        d.patch_view(a, &serde_json::json!({ "settings": { "overrides": { "model": { "Walls": {
            "halftone": true,
            "cut_lines": { "color": [200, 0, 0], "weight": 5 },
            "cut_foreground": { "color": [250, 220, 220], "pattern": "Solid fill" }
        }}}}})).unwrap();
        let v = d.view(a).unwrap();
        let w = &v.settings.overrides.model["Walls"];
        assert!(w.halftone && w.visible);
        assert_eq!(w.cut_lines.weight, Some(5));
        assert_eq!(w.cut_foreground.color, Some([250, 220, 220]));
        // hiding a single element in the view
        d.patch_view(a, &serde_json::json!({ "hidden_elements": [5, 6] })).unwrap();
        assert!(d.view(a).unwrap().hidden_elements.contains(&6));
    }

    #[test]
    fn wall_properties_patch_validates() {
        let (mut d, lvl, wt) = setup();
        let id = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        d.patch_wall(id, &serde_json::json!({ "mark": "W01", "comments": "party wall", "room_bounding": false, "structural": true, "structural_usage": "bearing" })).unwrap();
        let Some(Element::Wall(w)) = d.get(id) else { panic!() };
        assert_eq!((w.mark.as_str(), w.room_bounding, w.structural_usage), ("W01", false, StructuralUsage::Bearing));
        assert_eq!(w.ifc.guid.len(), 22);
        assert!(d.patch_wall(id, &serde_json::json!({ "wall_type": 9999 })).is_err());
        assert!(d.patch_wall(id, &serde_json::json!({ "height_mm": -5 })).is_err());
        assert!(d.patch_wall(id, &serde_json::json!({ "location_line": "nonsense" })).is_err());
    }

    #[test]
    fn ifc_guids_are_22_characters_and_unique() {
        let a = new_ifc_guid();
        let b = new_ifc_guid();
        assert_eq!(a.len(), 22);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$'));
        assert!("0123".contains(a.chars().next().unwrap())); // first character carries only 2 bits
    }

    #[test]
    fn view_range_planes_must_stay_in_order() {
        let mut d = Document::new_project();
        let (a, _) = d.views()[0];
        // cut plane above the top plane
        assert!(d.patch_view(a, &serde_json::json!({ "settings": { "view_range": { "cut_offset_mm": 2500.0 } } })).is_err());
        // bottom above the cut plane
        assert!(d.patch_view(a, &serde_json::json!({ "settings": { "view_range": { "bottom": { "offset_mm": 1500.0 } } } })).is_err());
        assert!(d.patch_view(a, &serde_json::json!({ "settings": { "view_range": { "cut_offset_mm": 1500.0 } } })).is_ok());
    }
}

/// A plane: height `base` at `origin`, rising `rise` mm per mm along `dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloorSurface {
    pub base: f64,
    pub origin: Point,
    pub dir: Point,
    pub rise: f64,
}

impl FloorSurface {
    pub fn z(&self, p: Point) -> f64 {
        self.base + self.rise * ((p.x - self.origin.x) * self.dir.x + (p.y - self.origin.y) * self.dir.y)
    }

    /// Slope as a percentage (rise over run).
    pub fn percent(&self) -> f64 {
        self.rise * 100.0
    }
}
