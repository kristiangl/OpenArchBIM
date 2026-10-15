//! Grids (Architecture > Datum > Grid): numbered or lettered lines that set out the
//! building. A grid is a straight line, an arc, or a chain of both (a multi-segment grid), with a
//! head (bubble) at each end that can be shown or hidden. How the line is drawn (weight, colour,
//! pattern) comes from the Grids entry of Object Styles, and the grid type holds the
//! head size and centre-segment settings.

use serde::{Deserialize, Serialize};

use crate::curve::{Curve, JOIN_TOL};
use crate::doc::{apply_patch, Document};
use crate::element::*;
use crate::geom::Point;

/// What the middle of a grid line looks like (Center Segment). With `None` the whole line
/// uses the line pattern. With `Continuous` the middle is solid and each end, for
/// `end_segment_mm`, uses the pattern. `Custom` is the same with the length of the end segment
/// set on the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CentreSegment {
    #[default]
    None,
    Continuous,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridType {
    pub name: String,
    /// Head diameter on paper, mm.
    #[serde(default = "bubble_dia")]
    pub bubble_dia_mm: f64,
    #[serde(default)]
    pub centre_segment: CentreSegment,
    /// Length on paper of each patterned end when the centre segment is not `None`.
    #[serde(default = "end_segment")]
    pub end_segment_mm: f64,
    /// Whether a new grid shows its head at its start and at its end (Plan View Symbols End 1 / End 2 Default).
    #[serde(default = "bubble_default")]
    pub bubble_default: [bool; 2],
}

fn bubble_dia() -> f64 {
    8.0
}
fn end_segment() -> f64 {
    20.0
}
fn bubble_default() -> [bool; 2] {
    [false, true]
}

impl GridType {
    pub fn standard() -> GridType {
        GridType { name: "Grid 8mm Bubble".into(), bubble_dia_mm: 8.0, centre_segment: CentreSegment::None, end_segment_mm: end_segment(), bubble_default: bubble_default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub name: String,
    /// One line or arc, or a head-to-tail chain of them (a multi-segment grid).
    pub curves: Vec<Curve>,
    pub grid_type: Id,
    /// Whether the head shows at the start and at the end of the line.
    #[serde(default = "bubble_default")]
    pub bubbles: [bool; 2],
    /// Scope box that limits its extent in views (kept for when scope boxes exist).
    #[serde(default)]
    pub scope_box: Option<Id>,
    #[serde(default)]
    pub pinned: bool,
    /// How far each head sits to the side of the line end (the bubble offset and elbow), mm.
    #[serde(default)]
    pub head_offsets: [Option<Point>; 2],
    /// The ends of other grids each end of this one is locked in line with (the alignment pin).
    #[serde(default)]
    pub end_locks: crate::datum::EndLocks,
}

impl Grid {
    pub fn start(&self) -> Point {
        self.curves.first().map_or(Point::default(), |c| c.start())
    }

    pub fn end(&self) -> Point {
        self.curves.last().map_or(Point::default(), |c| c.end())
    }

    /// The line as a polyline for drawing and picking.
    pub fn path(&self, tol: f64) -> Vec<Point> {
        let mut out: Vec<Point> = vec![];
        for c in &self.curves {
            let pts = c.tessellate(tol);
            if out.is_empty() {
                out.extend(pts);
            } else {
                out.extend(pts.into_iter().skip(1));
            }
        }
        out
    }

    pub fn length(&self) -> f64 {
        self.curves.iter().map(|c| c.length()).sum()
    }

    /// A single straight line.
    pub fn line(&self) -> Option<(Point, Point)> {
        match self.curves.as_slice() {
            [Curve::Line { a, b }] => Some((*a, *b)),
            _ => None,
        }
    }

    pub fn multi_segment(&self) -> bool {
        self.curves.len() > 1
    }

    pub fn moved(&self, x: crate::edit::Xform) -> Grid {
        Grid { curves: self.curves.iter().map(|c| c.transform(x)).collect(), head_offsets: self.head_offsets.map(|o| o.map(|p| rotate_vec(p, x))), ..self.clone() }
    }
}

/// Offsets are vectors, so a move does not change them and a rotation or mirror turns them.
fn rotate_vec(v: Point, x: crate::edit::Xform) -> Point {
    let z = x.apply(Point::new(0.0, 0.0));
    let p = x.apply(v);
    Point::new(p.x - z.x, p.y - z.y)
}

/// The name after `name`: digits count up (1, 2, 10), letters go A, B ... Z, AA.
pub fn next_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "1".into();
    }
    let digits: String = name.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    if !digits.is_empty() {
        let prefix = &name[..name.len() - digits.len()];
        let n: u64 = digits.parse().unwrap_or(0) + 1;
        return format!("{prefix}{n:0width$}", width = digits.len());
    }
    let mut chars: Vec<char> = name.chars().collect();
    let mut i = chars.len();
    loop {
        if i == 0 {
            chars.insert(0, if chars[0].is_ascii_uppercase() { 'A' } else { 'a' });
            break;
        }
        i -= 1;
        match chars[i] {
            'Z' => chars[i] = 'A',
            'z' => chars[i] = 'a',
            c if c.is_ascii_alphabetic() => {
                chars[i] = (c as u8 + 1) as char;
                break;
            }
            _ => {
                chars.push('1');
                break;
            }
        }
    }
    chars.into_iter().collect()
}

fn check_chain(curves: &[Curve]) -> Result<(), String> {
    if curves.is_empty() {
        return Err("a grid needs a line or an arc".into());
    }
    for c in curves {
        match c {
            Curve::Line { .. } | Curve::Arc { .. } => {}
            _ => return Err("a grid is made of straight lines and arcs".into()),
        }
        if c.length() < 10.0 {
            return Err("that grid line is too short".into());
        }
    }
    for w in curves.windows(2) {
        if w[0].end().dist(&w[1].start()) > JOIN_TOL {
            return Err("the segments of a multi-segment grid must join end to end".into());
        }
    }
    if curves.len() > 2 && curves[curves.len() - 1].end().dist(&curves[0].start()) < JOIN_TOL {
        return Err("a grid cannot close on itself".into());
    }
    Ok(())
}

impl Document {
    pub fn grids(&self) -> Vec<(Id, &Grid)> {
        self.iter().filter_map(|(id, e)| if let Element::Grid(g) = e { Some((id, g)) } else { None }).collect()
    }

    pub fn grid(&self, id: Id) -> Option<&Grid> {
        match self.get(id) {
            Some(Element::Grid(g)) => Some(g),
            _ => None,
        }
    }

    pub fn grid_types(&self) -> Vec<(Id, &GridType)> {
        self.iter().filter_map(|(id, e)| if let Element::GridType(t) = e { Some((id, t)) } else { None }).collect()
    }

    pub fn grid_type(&self, id: Id) -> Option<&GridType> {
        match self.get(id) {
            Some(Element::GridType(t)) => Some(t),
            _ => None,
        }
    }

    /// The name the next grid gets: one on from the grid made last.
    pub fn next_grid_name(&self) -> String {
        let latest = self.grids().into_iter().max_by_key(|(id, _)| *id).map(|(_, g)| g.name.clone());
        let mut name = latest.map_or("1".to_string(), |n| next_name(&n));
        while self.grids().iter().any(|(_, g)| g.name == name) {
            name = next_name(&name);
        }
        name
    }

    fn grid_name_taken(&self, name: &str, except: Option<Id>) -> bool {
        self.grids().iter().any(|(id, g)| g.name == name && Some(*id) != except)
    }

    /// Make a grid from a line, an arc or a chain of them. The first grid of a project also makes
    /// the standard grid type.
    pub fn add_grid(&mut self, curves: Vec<Curve>, type_id: Option<Id>, name: Option<&str>) -> Result<Id, String> {
        check_chain(&curves)?;
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                if self.grid_name_taken(n, None) {
                    return Err(format!("a grid named '{n}' already exists"));
                }
                n.to_string()
            }
            None => self.next_grid_name(),
        };
        if let Some(t) = type_id {
            if self.grid_type(t).is_none() {
                return Err("unknown grid type".into());
            }
        }
        let existing = type_id.or_else(|| self.grid_types().first().map(|(i, _)| *i));
        self.transact("Create grid", |tx| {
            let t = match existing {
                Some(t) => t,
                None => tx.add(Element::GridType(GridType::standard())),
            };
            let bubbles = tx.doc_ref().grid_type(t).map_or(bubble_default(), |g| g.bubble_default);
            Ok(tx.add(Element::Grid(Grid { name, curves, grid_type: t, bubbles, scope_box: None, pinned: false, head_offsets: [None, None], end_locks: Default::default() })))
        })
    }

    /// Change a grid with a JSON patch: name, type, bubbles, pinned, head offsets.
    pub fn patch_grid(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.grid(id).cloned() else { return Err("not a grid".into()) };
        let mut patch = patch.clone();
        if let Some(o) = patch.as_object_mut() {
            // geometry changes go through the grid's own commands
            o.remove("curves");
            // locks change only through lock_datum_ends and unlock_datum_ends
            o.remove("end_locks");
        }
        let new: Grid = apply_patch(&old, &patch)?;
        if new == old {
            return Ok(());
        }
        if old.pinned && new.pinned && (new.name != old.name || new.head_offsets != old.head_offsets) {
            return Err("the grid is pinned; unpin it first".into());
        }
        if new.name.trim().is_empty() {
            return Err("a grid needs a name".into());
        }
        if self.grid_name_taken(&new.name, Some(id)) {
            return Err(format!("a grid named '{}' already exists", new.name));
        }
        if self.grid_type(new.grid_type).is_none() {
            return Err("unknown grid type".into());
        }
        self.transact("Change grid", |tx| tx.update(id, Element::Grid(new)))
    }

    /// Change a grid type (head size, centre segment, default heads) with a JSON patch.
    pub fn patch_grid_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.grid_type(id).cloned() else { return Err("not a grid type".into()) };
        let mut patch = patch.clone();
        // the palette patches one default head at a time: { "bubble_default": { "1": true } }
        if let Some(o) = patch.get("bubble_default").and_then(|v| v.as_object()).cloned() {
            let mut arr = old.bubble_default;
            for (k, v) in o {
                if let (Ok(i), Some(b)) = (k.parse::<usize>(), v.as_bool()) {
                    if i < 2 {
                        arr[i] = b;
                    }
                }
            }
            patch["bubble_default"] = serde_json::json!(arr);
        }
        let new: GridType = apply_patch(&old, &patch)?;
        if new == old {
            return Ok(());
        }
        if new.name.trim().is_empty() {
            return Err("a type needs a name".into());
        }
        if !(2.0..=30.0).contains(&new.bubble_dia_mm) {
            return Err("the head diameter must be between 2 and 30 mm".into());
        }
        if self.grid_types().iter().any(|(i, t)| *i != id && t.name == new.name) {
            return Err(format!("a grid type named '{}' already exists", new.name));
        }
        self.transact("Change grid type", |tx| tx.update(id, Element::GridType(new)))
    }

    /// A copy of a grid type under a new name (Duplicate in the Type Properties dialog).
    pub fn duplicate_grid_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let Some(t) = self.grid_type(id).cloned() else { return Err("not a grid type".into()) };
        let name = name.trim();
        if name.is_empty() {
            return Err("a type needs a name".into());
        }
        if self.grid_types().iter().any(|(_, o)| o.name == name) {
            return Err(format!("a grid type named '{name}' already exists"));
        }
        self.transact("Duplicate grid type", |tx| Ok(tx.add(Element::GridType(GridType { name: name.to_string(), ..t }))))
    }

    /// Slide an end of a grid along its line (a straight grid) or round its arc (an arc grid), which
    /// changes how far it reaches and nothing else. `end` is 0 for the start, 1 for the end.
    /// Ends locked to this one (see `datum`) move with it, in the same undo step.
    pub fn set_grid_end(&mut self, id: Id, end: usize, to: Point) -> Result<(), String> {
        if self.grid(id).is_none() {
            return Err("not a grid".into());
        }
        self.move_datum_end(id, end, to)
    }

    /// The grid as it would be with one end moved to `to`, without changing the document.
    pub(crate) fn grid_with_end(&self, id: Id, end: usize, to: Point) -> Result<Grid, String> {
        let Some(g) = self.grid(id).cloned() else { return Err("not a grid".into()) };
        if g.pinned {
            return Err("the grid is pinned; unpin it first".into());
        }
        if end > 1 {
            return Err("a grid has two ends".into());
        }
        let mut curves = g.curves.clone();
        let k = if end == 0 { 0 } else { curves.len() - 1 };
        curves[k] = match &curves[k] {
            Curve::Line { a, b } => {
                let (fixed, moving) = if end == 0 { (*b, *a) } else { (*a, *b) };
                let d = (moving.x - fixed.x, moving.y - fixed.y);
                let l = (d.0 * d.0 + d.1 * d.1).sqrt();
                if l < 1e-9 {
                    return Err("the grid line has no length".into());
                }
                let u = (d.0 / l, d.1 / l);
                let t = (to.x - fixed.x) * u.0 + (to.y - fixed.y) * u.1;
                if t < 10.0 {
                    return Err("that would make the grid too short".into());
                }
                let np = Point::new(fixed.x + u.0 * t, fixed.y + u.1 * t);
                if end == 0 { Curve::line(np, fixed) } else { Curve::line(fixed, np) }
            }
            Curve::Arc { start, end: e, centre, ccw } => {
                let r = centre.dist(start);
                let a = (to.y - centre.y).atan2(to.x - centre.x);
                let np = Point::new(centre.x + r * a.cos(), centre.y + r * a.sin());
                let (ns, ne) = if end == 0 { (np, *e) } else { (*start, np) };
                let nc = Curve::Arc { start: ns, end: ne, centre: *centre, ccw: *ccw };
                if nc.length() < 10.0 {
                    return Err("that would make the grid too short".into());
                }
                nc
            }
            _ => return Err("a grid is made of straight lines and arcs".into()),
        };
        // joins inside a multi-segment grid stay joined
        if curves.len() > 1 {
            check_chain(&curves)?;
        }
        Ok(Grid { curves, ..g })
    }

    /// Make a grid along an existing wall, or along another grid, `offset` mm to the side of the
    /// picked line `towards` the point (Pick Lines).
    pub fn grid_from_line(&mut self, source: Id, offset: f64, towards: Point) -> Result<Id, String> {
        let base: Vec<Curve> = match self.get(source) {
            Some(Element::Wall(w)) => vec![w.curve.clone().unwrap_or(Curve::line(w.start, w.end))],
            Some(Element::Grid(g)) => g.curves.clone(),
            _ => return Err("pick a wall or a grid line".into()),
        };
        let curves = if offset.abs() < 1e-9 {
            base
        } else if base.len() == 1 {
            vec![crate::sketch::offset_curve(&base[0], towards, offset.abs())?]
        } else {
            crate::sketch::offset_chain(&base, towards, offset.abs())?
        };
        self.add_grid(curves, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
        Curve::line(Point::new(a.0, a.1), Point::new(b.0, b.1))
    }

    #[test]
    fn names_count_up_in_numbers_and_letters() {
        assert_eq!(next_name(""), "1");
        assert_eq!(next_name("1"), "2");
        assert_eq!(next_name("9"), "10");
        assert_eq!(next_name("A"), "B");
        assert_eq!(next_name("Z"), "AA");
        assert_eq!(next_name("AZ"), "BA");
        assert_eq!(next_name("A9"), "A10");
        assert_eq!(next_name("a"), "b");
    }

    #[test]
    fn grids_are_made_named_in_sequence_and_edited() {
        let mut d = Document::new_project();
        let a = d.add_grid(vec![line((0.0, 0.0), (0.0, 10000.0))], None, None).unwrap();
        let b = d.add_grid(vec![line((6000.0, 0.0), (6000.0, 10000.0))], None, None).unwrap();
        assert_eq!(d.grid(a).unwrap().name, "1");
        assert_eq!(d.grid(b).unwrap().name, "2");
        assert_eq!(d.grid_types().len(), 1, "the type is made once");
        // renaming to a letter carries on from it, and a taken name is refused
        d.patch_grid(b, &json!({ "name": "A" })).unwrap();
        let c = d.add_grid(vec![line((9000.0, 0.0), (9000.0, 10000.0))], None, None).unwrap();
        assert_eq!(d.grid(c).unwrap().name, "B");
        assert!(d.patch_grid(c, &json!({ "name": "1" })).is_err());
        // bubbles come from the type and are editable
        assert_eq!(d.grid(a).unwrap().bubbles, [false, true]);
        d.patch_grid(a, &json!({ "bubbles": [true, true] })).unwrap();
        assert_eq!(d.grid(a).unwrap().bubbles, [true, true]);
        // an end slides along the line and cannot cross the other end
        d.set_grid_end(a, 1, Point::new(500.0, 12000.0)).unwrap();
        assert_eq!(d.grid(a).unwrap().end(), Point::new(0.0, 12000.0));
        assert!(d.set_grid_end(a, 1, Point::new(0.0, -50.0)).is_err());
        d.patch_grid(a, &json!({ "pinned": true })).unwrap();
        assert!(d.set_grid_end(a, 0, Point::new(0.0, 100.0)).is_err());
        assert!(d.delete(&[a]).is_err());
        // undo walks back each step
        d.patch_grid(a, &json!({ "pinned": false })).unwrap();
        let before = d.grid(a).unwrap().clone();
        d.set_grid_end(a, 0, Point::new(0.0, -2000.0)).unwrap();
        assert_eq!(d.grid(a).unwrap().start(), Point::new(0.0, -2000.0));
        d.undo();
        assert_eq!(d.grid(a).unwrap(), &before);
    }

    #[test]
    fn arcs_and_chains_are_grids_but_closed_and_broken_ones_are_not() {
        let mut d = Document::new_project();
        let arc = Curve::Arc { start: Point::new(5000.0, 0.0), end: Point::new(0.0, 5000.0), centre: Point::new(0.0, 0.0), ccw: true };
        let g = d.add_grid(vec![arc.clone()], None, None).unwrap();
        // dragging the end of an arc grid keeps its radius
        d.set_grid_end(g, 1, Point::new(-4000.0, 4000.0)).unwrap();
        let e = d.grid(g).unwrap().end();
        assert!((e.x.hypot(e.y) - 5000.0).abs() < 1e-6 && e.x < 0.0);
        // multi-segment: line, then arc, joined
        let chain = vec![line((0.0, 0.0), (0.0, 4000.0)), Curve::Arc { start: Point::new(0.0, 4000.0), end: Point::new(3000.0, 7000.0), centre: Point::new(3000.0, 4000.0), ccw: false }];
        let m = d.add_grid(chain, None, None).unwrap();
        assert!(d.grid(m).unwrap().multi_segment());
        assert!(d.grid(m).unwrap().length() > 4000.0);
        // a broken chain, a closed one and a circle are refused
        assert!(d.add_grid(vec![line((0.0, 0.0), (0.0, 4000.0)), line((100.0, 4000.0), (3000.0, 4000.0))], None, None).is_err());
        assert!(d.add_grid(vec![line((0.0, 0.0), (4000.0, 0.0)), line((4000.0, 0.0), (4000.0, 4000.0)), line((4000.0, 4000.0), (0.0, 0.0))], None, None).is_err());
        assert!(d.add_grid(vec![Curve::Circle { centre: Point::new(0.0, 0.0), radius: 1000.0 }], None, None).is_err());
        assert!(d.add_grid(vec![line((0.0, 0.0), (3.0, 0.0))], None, None).is_err());
    }

    #[test]
    fn a_grid_picks_up_a_wall_line_with_an_offset() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[0].0;
        let w = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(8000.0, 0.0), 2700.0).unwrap();
        let g = d.grid_from_line(w, 0.0, Point::new(0.0, 100.0)).unwrap();
        assert_eq!(d.grid(g).unwrap().line(), Some((Point::new(0.0, 0.0), Point::new(8000.0, 0.0))));
        let h = d.grid_from_line(w, 1500.0, Point::new(4000.0, 900.0)).unwrap();
        assert_eq!(d.grid(h).unwrap().line(), Some((Point::new(0.0, 1500.0), Point::new(8000.0, 1500.0))));
        let k = d.grid_from_line(h, 6000.0, Point::new(4000.0, 9000.0)).unwrap();
        assert_eq!(d.grid(k).unwrap().start(), Point::new(0.0, 7500.0));
        assert!(d.grid_from_line(lvl, 0.0, Point::default()).is_err());
    }

    #[test]
    fn older_files_without_grids_open_and_grids_survive_save_and_load() {
        let mut d = Document::new_project();
        let g = d.add_grid(vec![line((0.0, 0.0), (0.0, 5000.0))], None, Some("C")).unwrap();
        let dir = std::env::temp_dir().join(format!("openarch-grid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("g.oab");
        d.save(&path).unwrap();
        let back = Document::open(&path).unwrap();
        assert_eq!(back.grid(g).unwrap().name, "C");
        assert_eq!(back.grid(g).unwrap().bubbles, [false, true]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn grids_move_copy_rotate_and_mirror_with_the_other_elements() {
        use crate::edit::{EditOptions, Xform};
        let mut d = Document::new_project();
        let a = d.add_grid(vec![line((0.0, 0.0), (0.0, 10000.0))], None, None).unwrap();
        d.transform_elements(&[a], Xform::Move { dx: 500.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert_eq!(d.grid(a).unwrap().start(), Point::new(500.0, 0.0));
        // a copy is a new grid with the next name; the original stays
        let out = d.transform_elements(&[a], Xform::Move { dx: 6000.0, dy: 0.0 }, EditOptions { copy: true, disjoin: false }).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(d.grid(out[0]).unwrap().name, "2");
        assert_eq!(d.grid(a).unwrap().start(), Point::new(500.0, 0.0));
        // mirror about the y axis and rotate a quarter turn
        d.transform_elements(&[a], Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, EditOptions::default()).unwrap();
        assert_eq!(d.grid(a).unwrap().start(), Point::new(-500.0, 0.0));
        d.transform_elements(&[a], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, EditOptions::default()).unwrap();
        let g = d.grid(a).unwrap();
        assert!((g.end().x + 10000.0).abs() < 1e-6 && (g.end().y + 500.0).abs() < 1e-6, "{:?}", g.end());
        // a pinned grid refuses, and a copy of it is not pinned
        d.patch_grid(a, &json!({ "pinned": true })).unwrap();
        assert!(d.transform_elements(&[a], Xform::Move { dx: 1.0, dy: 0.0 }, EditOptions::default()).is_err());
        let c = d.transform_elements(&[a], Xform::Move { dx: 3000.0, dy: 0.0 }, EditOptions { copy: true, disjoin: false }).unwrap();
        assert!(!d.grid(c[0]).unwrap().pinned);
    }

    #[test]
    fn grids_that_run_away_from_the_viewer_show_in_elevations() {
        let mut d = Document::new_project();
        // north-south grid at x = 3000 and an east-west one at y = 4000
        d.add_grid(vec![line((3000.0, -2000.0), (3000.0, 12000.0))], None, None).unwrap();
        d.add_grid(vec![line((-2000.0, 4000.0), (12000.0, 4000.0))], None, None).unwrap();
        let north = d.views().into_iter().find(|(_, v)| v.name == "North").unwrap().0;
        let e = d.elevation_data(north).unwrap();
        // the North elevation looks south: only the north-south grid shows, once
        assert_eq!(e.grids.len(), 1);
        assert_eq!(e.grids[0].name, "1");
    }
}
