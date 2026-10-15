//! Alignment locks on the ends of datum lines (the alignment pin). When the end of a grid or level
//! is brought in line with the same end of another one, the two can be locked: moving one end then
//! moves every end locked to it, so a run of levels or grids keeps its heads in a tidy line. A lock can
//! be taken off and put back, and an end is only ever locked to the ends of lines of its own kind
//! (grids with grids, levels with levels) that run parallel to it.
//!
//! The locks are stored on both datums (`end_locks`, one list for each end), so deleting either one
//! leaves nothing dangling that matters: a missing id is skipped.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::geom::Point;

/// One end of a datum: `end` is 0 for the start and 1 for the finish of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndRef {
    pub id: Id,
    pub end: usize,
}

/// For each end of a datum, the ends it is locked to.
pub type EndLocks = [Vec<EndRef>; 2];

fn unit(a: Point, b: Point) -> Option<Point> {
    let l = a.dist(&b);
    if l < 1e-9 {
        None
    } else {
        Some(Point::new((b.x - a.x) / l, (b.y - a.y) / l))
    }
}

fn dot(p: Point, d: Point) -> f64 {
    p.x * d.x + p.y * d.y
}

fn element_line(e: &Element) -> Option<(Point, Point)> {
    match e {
        Element::Grid(g) => g.line(),
        Element::Level(l) => l.extent.map(|[a, b]| (a, b)),
        _ => None,
    }
}

fn with_lock(mut e: Element, end: usize, r: EndRef, add: bool) -> Element {
    let list = match &mut e {
        Element::Grid(g) => &mut g.end_locks[end],
        Element::Level(l) => &mut l.end_locks[end],
        _ => return e,
    };
    list.retain(|x| *x != r);
    if add {
        list.push(r);
    }
    e
}

impl Document {
    /// The locks on a grid or level, empty for anything else.
    pub fn datum_locks(&self, id: Id) -> EndLocks {
        match self.get(id) {
            Some(Element::Grid(g)) => g.end_locks.clone(),
            Some(Element::Level(l)) => l.end_locks.clone(),
            _ => Default::default(),
        }
    }

    /// Every end locked to `(id, end)`, directly or through others, not counting itself.
    pub fn locked_group(&self, id: Id, end: usize) -> Vec<EndRef> {
        let mut seen = vec![EndRef { id, end }];
        let mut i = 0;
        while i < seen.len() {
            let cur = seen[i];
            i += 1;
            let locks = self.datum_locks(cur.id);
            for r in locks.get(cur.end).into_iter().flatten() {
                if r.end < 2 && self.get(r.id).is_some() && !seen.contains(r) {
                    seen.push(*r);
                }
            }
        }
        seen.into_iter().skip(1).collect()
    }

    fn level_with_end(&self, id: Id, end: usize, to: Point) -> Result<Level, String> {
        let Some(Element::Level(l)) = self.get(id) else { return Err("not a level".into()) };
        if l.pinned {
            return Err("the level is pinned; unpin it first".into());
        }
        let Some([a, b]) = l.extent else { return Err("that level has no extent of its own yet".into()) };
        let (fixed, moving) = if end == 0 { (b, a) } else { (a, b) };
        let u = unit(fixed, moving).ok_or("the level line has no length")?;
        let t = dot(Point::new(to.x - fixed.x, to.y - fixed.y), u);
        if t < 100.0 {
            return Err("that would make the level too short".into());
        }
        let np = Point::new(fixed.x + u.x * t, fixed.y + u.y * t);
        let extent = if end == 0 { [np, b] } else { [a, np] };
        Ok(Level { extent: Some(extent), ..l.clone() })
    }

    fn datum_with_end(&self, id: Id, end: usize, to: Point) -> Result<Element, String> {
        match self.get(id) {
            Some(Element::Grid(_)) => Ok(Element::Grid(self.grid_with_end(id, end, to)?)),
            Some(Element::Level(_)) => Ok(Element::Level(self.level_with_end(id, end, to)?)),
            _ => Err("not a grid or a level".into()),
        }
    }

    /// Move an end of a grid or level to the point `to` (the end slides along its own line) and, with
    /// it, every end locked to it so they stay in line. One undo step.
    pub fn move_datum_end(&mut self, id: Id, end: usize, to: Point) -> Result<(), String> {
        if end > 1 {
            return Err("a line has two ends".into());
        }
        let first = self.datum_with_end(id, end, to)?;
        let mut updates: Vec<(Id, Element)> = vec![];
        let group = self.locked_group(id, end);
        if !group.is_empty() {
            let (a, b) = element_line(&first).ok_or("the line must be straight to hold a lock")?;
            let dir = unit(a, b).ok_or("the line has no length")?;
            let np = if end == 0 { a } else { b };
            for r in group {
                let Some((pa, pb)) = self.get(r.id).and_then(element_line) else { continue };
                let Some(ub) = unit(pa, pb) else { continue };
                let c = dot(ub, dir);
                if c.abs() < 0.1 {
                    return Err("the locked lines are no longer parallel; unlock them first".into());
                }
                let cur = if r.end == 0 { pa } else { pb };
                let s = (dot(np, dir) - dot(cur, dir)) / c;
                let target = Point::new(cur.x + ub.x * s, cur.y + ub.y * s);
                if s.abs() > 1e-9 {
                    updates.push((r.id, self.datum_with_end(r.id, r.end, target)?));
                }
            }
        }
        updates.insert(0, (id, first));
        self.transact("Move aligned ends", |tx| {
            for (i, e) in updates {
                tx.update(i, e)?;
            }
            Ok(())
        })
    }

    /// Lock an end of one grid or level to an end of another of the same kind. The second end is brought
    /// in line with the first, and from then on they move together.
    pub fn lock_datum_ends(&mut self, a: EndRef, b: EndRef) -> Result<(), String> {
        if a.end > 1 || b.end > 1 {
            return Err("a line has two ends".into());
        }
        if a.id == b.id {
            return Err("pick the ends of two different lines".into());
        }
        let same_kind = matches!((self.get(a.id), self.get(b.id)), (Some(Element::Grid(_)), Some(Element::Grid(_))) | (Some(Element::Level(_)), Some(Element::Level(_))));
        if !same_kind {
            return Err("grids lock to grids and levels to levels".into());
        }
        let (Some((a0, a1)), Some((b0, b1))) = (self.get(a.id).and_then(element_line), self.get(b.id).and_then(element_line)) else {
            return Err("only straight lines can be locked, and a level needs its own extent".into());
        };
        let (Some(ua), Some(ub)) = (unit(a0, a1), unit(b0, b1)) else { return Err("a line has no length".into()) };
        if (ua.x * ub.y - ua.y * ub.x).abs() > 1e-3 {
            return Err("only parallel lines can be locked".into());
        }
        if self.datum_locks(a.id)[a.end].contains(&b) {
            return Ok(());
        }
        let pa = if a.end == 0 { a0 } else { a1 };
        let cur_b = if b.end == 0 { b0 } else { b1 };
        let s = (dot(pa, ua) - dot(cur_b, ua)) / dot(ub, ua);
        let mut eb = self.get(b.id).cloned().ok_or("not a line")?;
        if s.abs() > 1e-6 {
            let target = Point::new(cur_b.x + ub.x * s, cur_b.y + ub.y * s);
            eb = self.datum_with_end(b.id, b.end, target)?;
        }
        let eb = with_lock(eb, b.end, a, true);
        let ea = with_lock(self.get(a.id).cloned().ok_or("not a line")?, a.end, b, true);
        self.transact("Lock aligned ends", |tx| {
            tx.update(a.id, ea)?;
            tx.update(b.id, eb)
        })
    }

    /// Take the lock between two ends off. They stay where they are.
    pub fn unlock_datum_ends(&mut self, a: EndRef, b: EndRef) -> Result<(), String> {
        if a.end > 1 || b.end > 1 {
            return Err("a line has two ends".into());
        }
        let ea = self.get(a.id).cloned();
        let eb = self.get(b.id).cloned();
        if ea.is_none() && eb.is_none() {
            return Ok(());
        }
        self.transact("Unlock aligned ends", |tx| {
            if let Some(e) = ea {
                tx.update(a.id, with_lock(e, a.end, b, false))?;
            }
            if let Some(e) = eb {
                tx.update(b.id, with_lock(e, b.end, a, false))?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;

    fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
        Curve::line(Point::new(a.0, a.1), Point::new(b.0, b.1))
    }

    fn three_grids() -> (Document, Id, Id, Id) {
        let mut d = Document::new_project();
        let a = d.add_grid(vec![line((0.0, 0.0), (0.0, 10000.0))], None, None).unwrap();
        let b = d.add_grid(vec![line((6000.0, 0.0), (6000.0, 10000.0))], None, None).unwrap();
        let c = d.add_grid(vec![line((9000.0, 0.0), (9000.0, 10000.0))], None, None).unwrap();
        (d, a, b, c)
    }

    #[test]
    fn locked_grid_ends_move_together_and_unlocked_ones_do_not() {
        let (mut d, a, b, c) = three_grids();
        d.lock_datum_ends(EndRef { id: a, end: 1 }, EndRef { id: b, end: 1 }).unwrap();
        d.lock_datum_ends(EndRef { id: b, end: 1 }, EndRef { id: c, end: 1 }).unwrap();
        // moving one end carries the whole run, through the middle one
        d.set_grid_end(a, 1, Point::new(0.0, 12000.0)).unwrap();
        for g in [a, b, c] {
            assert_eq!(d.grid(g).unwrap().end().y, 12000.0);
            assert_eq!(d.grid(g).unwrap().start().y, 0.0, "the other end is not locked");
        }
        // one undo step puts all three back
        d.undo();
        for g in [a, b, c] {
            assert_eq!(d.grid(g).unwrap().end().y, 10000.0);
        }
        d.redo();
        // unlocking the pair b-c leaves c where it is when a moves again
        d.unlock_datum_ends(EndRef { id: b, end: 1 }, EndRef { id: c, end: 1 }).unwrap();
        d.set_grid_end(a, 1, Point::new(0.0, 8000.0)).unwrap();
        assert_eq!(d.grid(b).unwrap().end().y, 8000.0);
        assert_eq!(d.grid(c).unwrap().end().y, 12000.0);
    }

    #[test]
    fn locking_aligns_the_second_end_and_refuses_what_cannot_be_locked() {
        let (mut d, a, b, _c) = three_grids();
        d.set_grid_end(b, 0, Point::new(6000.0, 1500.0)).unwrap();
        d.lock_datum_ends(EndRef { id: a, end: 0 }, EndRef { id: b, end: 0 }).unwrap();
        assert_eq!(d.grid(b).unwrap().start().y, 0.0, "b's start was brought in line with a's");
        // locking twice is harmless; a grid does not lock to itself or to a level
        d.lock_datum_ends(EndRef { id: a, end: 0 }, EndRef { id: b, end: 0 }).unwrap();
        assert!(d.lock_datum_ends(EndRef { id: a, end: 0 }, EndRef { id: a, end: 1 }).is_err());
        let l = d.levels()[0].0;
        assert!(d.lock_datum_ends(EndRef { id: a, end: 0 }, EndRef { id: l, end: 0 }).is_err());
        // grids that are not parallel cannot be locked
        let cross = d.add_grid(vec![line((-2000.0, 5000.0), (8000.0, 5000.0))], None, None).unwrap();
        assert!(d.lock_datum_ends(EndRef { id: a, end: 0 }, EndRef { id: cross, end: 0 }).is_err());
        // a pinned grid in the group stops the move
        d.patch_grid(b, &serde_json::json!({ "pinned": true })).unwrap();
        assert!(d.set_grid_end(a, 0, Point::new(0.0, 500.0)).is_err());
        assert_eq!(d.grid(a).unwrap().start().y, 0.0);
    }

    #[test]
    fn level_ends_lock_too_and_old_files_open_without_locks() {
        let mut d = Document::new_project();
        let l1 = d.levels()[0].0;
        let l2 = d.add_level(Some("Roof"), 6000.0, false).unwrap();
        for l in [l1, l2] {
            d.patch_level(l, &serde_json::json!({ "extent": [{"x": -2000.0, "y": 0.0}, {"x": 9000.0, "y": 0.0}] })).unwrap();
        }
        d.lock_datum_ends(EndRef { id: l1, end: 1 }, EndRef { id: l2, end: 1 }).unwrap();
        d.move_datum_end(l1, 1, Point::new(11000.0, 0.0)).unwrap();
        assert_eq!(d.level(l2).unwrap().extent.unwrap()[1].x, 11000.0);
        assert_eq!(d.level(l2).unwrap().extent.unwrap()[0].x, -2000.0);
        // a level without its own extent cannot hold a lock
        let l3 = d.add_level(Some("Attic"), 9000.0, false).unwrap();
        assert!(d.lock_datum_ends(EndRef { id: l1, end: 0 }, EndRef { id: l3, end: 0 }).is_err());
        // a level saved before locks existed has none
        let old: Level = serde_json::from_value(serde_json::json!({ "name": "Old", "elevation_mm": 0.0 })).unwrap();
        assert!(old.end_locks[0].is_empty() && old.end_locks[1].is_empty());
        // the locks survive a save and load
        let dir = std::env::temp_dir().join(format!("openarch-datum-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("d.oab");
        d.save(&path).unwrap();
        let back = Document::open(&path).unwrap();
        assert_eq!(back.level(l1).unwrap().end_locks[1], vec![EndRef { id: l2, end: 1 }]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
