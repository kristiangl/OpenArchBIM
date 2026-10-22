//! Modify tools: Move, Copy, Rotate, Mirror, Offset, Trim/Extend and Align.
//!
//! Everything funnels through one rule set so each tool behaves the same way:
//! pinned elements refuse to move, walls that are joined to a moved wall follow
//! it (their joined end moves too) unless the move disjoins, doors and windows
//! travel with their host, and the whole command is one undo step that rolls
//! back if a hosted opening would no longer fit.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::Curve;
use crate::doc::Document;
use crate::element::*;
use crate::family::*;
use crate::geom::Point;
use crate::grid::Grid;

/// A rigid change of position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Xform {
    Move { dx: f64, dy: f64 },
    /// Counter-clockwise about a centre.
    Rotate { cx: f64, cy: f64, angle_deg: f64 },
    /// Reflect across the line through two points.
    Mirror { ax: f64, ay: f64, bx: f64, by: f64 },
}

impl Xform {
    pub fn apply(&self, p: Point) -> Point {
        match *self {
            Xform::Move { dx, dy } => Point::new(p.x + dx, p.y + dy),
            Xform::Rotate { cx, cy, angle_deg } => {
                let (s, c) = angle_deg.to_radians().sin_cos();
                let (x, y) = (p.x - cx, p.y - cy);
                Point::new(cx + x * c - y * s, cy + x * s + y * c)
            }
            Xform::Mirror { ax, ay, bx, by } => {
                let (dx, dy) = (bx - ax, by - ay);
                let l2 = dx * dx + dy * dy;
                let t = ((p.x - ax) * dx + (p.y - ay) * dy) / l2;
                let (fx, fy) = (ax + t * dx, ay + t * dy);
                Point::new(2.0 * fx - p.x, 2.0 * fy - p.y)
            }
        }
    }

    fn name(&self, copy: bool) -> &'static str {
        match (self, copy) {
            (Xform::Move { .. }, false) => "Move",
            (Xform::Move { .. }, true) => "Copy",
            (Xform::Rotate { .. }, false) => "Rotate",
            (Xform::Rotate { .. }, true) => "Rotate copy",
            (Xform::Mirror { .. }, false) => "Mirror",
            (Xform::Mirror { .. }, true) => "Mirror copy",
        }
    }

    fn verb(&self) -> &'static str {
        match self {
            Xform::Move { .. } => "moved",
            Xform::Rotate { .. } => "rotated",
            Xform::Mirror { .. } => "mirrored",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EditOptions {
    /// Leave the originals where they are and create transformed copies.
    pub copy: bool,
    /// Break joins first: walls joined to a moved wall stay where they are, and a
    /// door or window may land in a different wall.
    pub disjoin: bool,
}

fn line_intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = (Point::new(b.x - a.x, b.y - a.y), Point::new(d.x - c.x, d.y - c.y));
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 * (r.x.hypot(r.y) * s.x.hypot(s.y)).max(1e-12) {
        return None;
    }
    let t = ((c.x - a.x) * s.y - (c.y - a.y) * s.x) / den;
    Some(Point::new(a.x + t * r.x, a.y + t * r.y))
}

/// Where the line through `s` and `e` meets the line through `a` and `b`, unless they are within a degree of parallel.
fn slide_point(s: Point, e: Point, a: Point, b: Point) -> Option<Point> {
    let (r, t) = (Point::new(e.x - s.x, e.y - s.y), Point::new(b.x - a.x, b.y - a.y));
    let (lr, lt) = (r.x.hypot(r.y), t.x.hypot(t.y));
    if lr < 1e-9 || lt < 1e-9 || (r.x * t.y - r.y * t.x).abs() / (lr * lt) < 0.017 {
        return None;
    }
    line_intersection(s, e, a, b)
}

impl Document {
    /// The wall (and distance along it) a point lands in, on a level. A wall the
    /// point is on is preferred over a nearer one it is not on; `prefer` wins ties.
    fn host_wall_at(&self, level: Id, p: Point, prefer: Option<Id>) -> Option<(Id, f64)> {
        let mut best: Option<(f64, Id, f64)> = None;
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            if w.base_level != level {
                continue;
            }
            let (l, r) = self.wall_extents(w);
            let (u, _, d) = w.path().project(p);
            if d > l.max(r) + 1.0 {
                continue;
            }
            let score = d - if Some(id) == prefer { 1e-3 } else { 0.0 };
            if best.map_or(true, |b| score < b.0) {
                best = Some((score, id, u));
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// New end points for walls, plus the knock-on changes: when `stretch` is set,
    /// walls joined to a moved end follow it, and walls butting into a moved wall
    /// run on to its new line. Returns every wall that changes.
    pub(crate) fn plan_wall_ends(&self, moves: &BTreeMap<Id, (Point, Point)>, stretch: bool) -> Result<BTreeMap<Id, Wall>, String> {
        let mut out: BTreeMap<Id, Wall> = BTreeMap::new();
        for (&id, &(s, e)) in moves {
            let Some(Element::Wall(w)) = self.get(id) else { return Err("not a wall".into()) };
            if s.dist(&e) < 1.0 {
                return Err("the wall would be too short".into());
            }
            let mut nw = w.clone();
            nw.retarget(s, e);
            out.insert(id, nw);
        }
        if !stretch {
            return Ok(out);
        }
        // coincident ends follow
        let nodes: Vec<(Id, u8, Point, Point, Id)> = moves
            .iter()
            .flat_map(|(&id, &(s, e))| {
                let Some(Element::Wall(w)) = self.get(id) else { return vec![] };
                let (a, b) = (w.start_join.allowed, w.end_join.allowed);
                let mut v = vec![];
                if a {
                    v.push((id, 0u8, w.start, s, w.base_level));
                }
                if b {
                    v.push((id, 1u8, w.end, e, w.base_level));
                }
                v
            })
            .collect();
        for (oid, el) in self.iter() {
            let Element::Wall(o) = el else { continue };
            if moves.contains_key(&oid) {
                continue;
            }
            let mut no = o.clone();
            let mut changed = false;
            for (which, p) in [(0u8, o.start), (1u8, o.end)] {
                let allowed = if which == 0 { o.start_join.allowed } else { o.end_join.allowed };
                if !allowed {
                    continue;
                }
                if let Some(n) = nodes.iter().find(|n| n.4 == o.base_level && n.2.dist(&p) <= 1.0) {
                    let (s, e) = if which == 0 { (n.3, no.end) } else { (no.start, n.3) };
                    no.retarget(s, e);
                    changed = true;
                }
            }
            if changed {
                if no.start.dist(&no.end) < 1.0 {
                    return Err("a joined wall would become too short".into());
                }
                out.insert(oid, no);
            }
        }
        self.run_stems_on(moves, &mut out)?;
        Ok(out)
    }

    /// Walls that butt into a moved wall (T joins) run on to its new location line.
    fn run_stems_on(&self, moves: &BTreeMap<Id, (Point, Point)>, out: &mut BTreeMap<Id, Wall>) -> Result<(), String> {
        for ((stem, end), host) in self.mid_join_hosts() {
            if !moves.contains_key(&host) || moves.contains_key(&stem) {
                continue;
            }
            let Some(&(hs, he)) = moves.get(&host) else { continue };
            // a curved wall has no straight line to run on to, and a curved stem keeps its shape
            if matches!(self.get(host), Some(Element::Wall(h)) if h.curve.is_some()) {
                continue;
            }
            let base = out.get(&stem).cloned().or_else(|| if let Some(Element::Wall(w)) = self.get(stem) { Some(w.clone()) } else { None });
            let Some(mut ns) = base else { continue };
            if ns.curve.is_some() {
                continue;
            }
            if let Some(x) = line_intersection(ns.start, ns.end, hs, he) {
                if end == 0 {
                    ns.start = x;
                } else {
                    ns.end = x;
                }
                if ns.start.dist(&ns.end) < 1.0 {
                    return Err("a wall butting into this one would become too short".into());
                }
                out.insert(stem, ns);
            }
        }
        Ok(())
    }

    /// New ends for walls that are moved, rotated or mirrored as a whole. A wall joined to a moved
    /// one keeps its direction: its joined end slides along its own line to meet the moved wall's new line, so
    /// only its length changes. The moved wall's own ends do the same against walls that stay, whether they meet
    /// end to end or butt into the middle of one. Lines that are parallel have nothing to meet, and those ends stay
    /// where the move put them.
    pub(crate) fn plan_wall_slide(&self, moves: &BTreeMap<Id, (Point, Point)>) -> Result<BTreeMap<Id, Wall>, String> {
        let mut out = self.plan_wall_ends(moves, false)?;
        let stems = self.mid_join_hosts();
        for (&id, &(s, e)) in moves {
            let Some(Element::Wall(w)) = self.get(id) else { continue };
            if w.curve.is_some() {
                continue;
            }
            let mut ends = [s, e];
            for which in 0..2u8 {
                if !(if which == 0 { w.start_join.allowed } else { w.end_join.allowed }) {
                    continue;
                }
                let old = if which == 0 { w.start } else { w.end };
                let mut hit: Option<Point> = None;
                for (oid, el) in self.iter() {
                    let Element::Wall(o) = el else { continue };
                    if moves.contains_key(&oid) || o.base_level != w.base_level || o.curve.is_some() {
                        continue;
                    }
                    for ow in 0..2u8 {
                        let (oallowed, op) = if ow == 0 { (o.start_join.allowed, o.start) } else { (o.end_join.allowed, o.end) };
                        if !oallowed || op.dist(&old) > 1.0 {
                            continue;
                        }
                        // the other wall's line is its own line before this change
                        if let Some(x) = slide_point(s, e, o.start, o.end) {
                            let mut no = out.get(&oid).cloned().unwrap_or_else(|| o.clone());
                            if ow == 0 {
                                no.retarget(x, no.end);
                            } else {
                                no.retarget(no.start, x);
                            }
                            if no.start.dist(&no.end) < 1.0 {
                                return Err("a joined wall would become too short".into());
                            }
                            out.insert(oid, no);
                            hit.get_or_insert(x);
                        }
                    }
                }
                if hit.is_none() {
                    if let Some(&host) = stems.get(&(id, which)) {
                        if !moves.contains_key(&host) {
                            if let Some(Element::Wall(h)) = self.get(host) {
                                if h.curve.is_none() {
                                    hit = slide_point(s, e, h.start, h.end);
                                }
                            }
                        }
                    }
                }
                if let Some(x) = hit {
                    ends[which as usize] = x;
                }
            }
            if ends[0].dist(&ends[1]) < 1.0 {
                return Err("the wall would be too short".into());
            }
            if let Some(nw) = out.get_mut(&id) {
                nw.retarget(ends[0], ends[1]);
            }
        }
        self.run_stems_on(moves, &mut out)?;
        Ok(out)
    }

    /// New ends for walls whose end grips are dragged to `to`: only those walls change, except that walls butting
    /// into them run on to their new line. A shared grip (two walls meeting at a corner) lists both walls.
    pub(crate) fn plan_wall_grip(&self, ends: &[(Id, u8)], to: Point) -> Result<BTreeMap<Id, Wall>, String> {
        let mut moves: BTreeMap<Id, (Point, Point)> = BTreeMap::new();
        for &(id, end) in ends {
            let Some(Element::Wall(w)) = self.get(id) else { return Err("not a wall".into()) };
            moves.insert(id, if end == 0 { (to, w.end) } else { (w.start, to) });
        }
        let mut out = self.plan_wall_ends(&moves, false)?;
        self.run_stems_on(&moves, &mut out)?;
        Ok(out)
    }

    /// Move, Rotate or Mirror elements, or copy them transformed. Returns the ids of
    /// the elements that now sit in the new place (the copies when `opt.copy`).
    pub fn transform_elements(&mut self, ids: &[Id], x: Xform, opt: EditOptions) -> Result<Vec<Id>, String> {
        // spot annotations standing on what moves go with it, in the same undo step
        let spots = if opt.copy { vec![] } else { self.spots_on(ids) };
        if spots.is_empty() {
            return self.transform_elements_inner(ids, x, opt);
        }
        let name = match x {
            Xform::Move { .. } => "Move",
            Xform::Rotate { .. } => "Rotate",
            Xform::Mirror { .. } => "Mirror",
        };
        self.grouped(name, |doc| {
            let out = doc.transform_elements_inner(ids, x, opt)?;
            doc.follow_spots(&spots, x)?;
            Ok(out)
        })
    }

    fn transform_elements_inner(&mut self, ids: &[Id], x: Xform, opt: EditOptions) -> Result<Vec<Id>, String> {
        if let Xform::Mirror { ax, ay, bx, by } = x {
            if (ax - bx).hypot(ay - by) < 1e-6 {
                return Err("the mirror axis needs two different points".into());
            }
        }
        if let Xform::Move { dx, dy } = x {
            if dx.hypot(dy) < 1e-9 {
                return Ok(ids.to_vec());
            }
        }
        let set: BTreeSet<Id> = ids.iter().copied().collect();
        let (mut walls, mut floors, mut insts, mut grids, mut roofs) = (vec![], vec![], vec![], vec![], vec![]);
        let mut details = vec![];
        for &id in &set {
            match self.get(id) {
                Some(Element::DetailLine(_)) | Some(Element::DetailRegion(_)) | Some(Element::TextNote(_)) => details.push(id),
                Some(Element::Wall(_)) => walls.push(id),
                Some(Element::Floor(_)) => floors.push(id),
                Some(Element::FamilyInstance(_)) => insts.push(id),
                Some(Element::Grid(_)) => grids.push(id),
                Some(Element::Roof(_)) => roofs.push(id),
                Some(_) => {}
                None => return Err(format!("no element {id}")),
            }
        }
        if walls.is_empty() && floors.is_empty() && insts.is_empty() && grids.is_empty() && roofs.is_empty() && details.is_empty() {
            return Err("select walls, floors, roofs, grids, doors, windows or detail items".into());
        }
        if !opt.copy {
            let all: Vec<Id> = walls.iter().chain(&floors).chain(&insts).chain(&grids).chain(&roofs).chain(&details).copied().collect();
            self.refuse_pinned(&all, x.verb())?;
        }
        let mirror = matches!(x, Xform::Mirror { .. });
        let axis_deg = if let Xform::Mirror { ax, ay, bx, by } = x { (by - ay).atan2(bx - ax).to_degrees() } else { 0.0 };
        let turn = if let Xform::Rotate { angle_deg, .. } = x { angle_deg } else { 0.0 };

        // instances whose wall travels with them follow it; the rest are handled one by one
        let walls_set: BTreeSet<Id> = walls.iter().copied().collect();
        let (followers, lone): (Vec<Id>, Vec<Id>) = insts.iter().partition(|i| {
            matches!(self.get(**i), Some(Element::FamilyInstance(f)) if f.host.is_some_and(|h| walls_set.contains(&h.wall)))
        });

        let mut moved_ends: BTreeMap<Id, (Point, Point)> = BTreeMap::new();
        for &id in &walls {
            if let Some(Element::Wall(w)) = self.get(id) {
                moved_ends.insert(id, (x.apply(w.start), x.apply(w.end)));
            }
        }

        // updates (moving) or additions (copying), planned before anything changes
        let mut updates: Vec<(Id, Element)> = vec![];
        let mut new_walls: Vec<(Id, Wall)> = vec![];
        let mut new_floors: Vec<Floor> = vec![];
        let mut new_grids: Vec<Grid> = vec![];
        let mut new_roofs: Vec<crate::roof::Roof> = vec![];
        let mut new_insts: Vec<(Id, FamilyInstance, bool)> = vec![]; // (original, copy, follows a copied wall)
        let mut new_details: Vec<Element> = vec![];

        // detail lines and regions are curves in their view: they carry the change as it is
        for &id in &details {
            let e = match self.get(id) {
                Some(Element::DetailLine(l)) => Element::DetailLine(crate::detail::DetailLine { curve: l.curve.transform(x), pinned: l.pinned && !opt.copy, ..l.clone() }),
                Some(Element::DetailRegion(r)) => {
                    let mut nr = r.clone();
                    for e in nr.loops.iter_mut().flatten() {
                        e.curve = e.curve.transform(x);
                    }
                    nr.pinned = r.pinned && !opt.copy;
                    Element::DetailRegion(nr)
                }
                // a text note's anchor and leaders move; the text stays upright
                Some(Element::TextNote(t)) => {
                    let mut nt = t.clone();
                    nt.at = x.apply(t.at);
                    for l in &mut nt.leaders {
                        l.end = x.apply(l.end);
                        l.elbow = l.elbow.map(|e| x.apply(e));
                    }
                    nt.pinned = t.pinned && !opt.copy;
                    Element::TextNote(nt)
                }
                _ => continue,
            };
            if opt.copy {
                new_details.push(e);
            } else {
                updates.push((id, e));
            }
        }

        if opt.copy {
            for &id in &walls {
                let Some(Element::Wall(w)) = self.get(id) else { continue };
                let (s, e) = moved_ends[&id];
                let mut nw = w.clone();
                nw.start = s;
                nw.end = e;
                nw.curve = w.curve.as_ref().map(|c| c.transform(x));
                if mirror {
                    nw.flipped = !nw.flipped;
                }
                nw.pinned = false;
                nw.mark.clear();
                nw.ifc.guid = new_ifc_guid();
                new_walls.push((id, nw));
            }
        } else {
            let plan = if opt.disjoin { self.plan_wall_ends(&moved_ends, false)? } else { self.plan_wall_slide(&moved_ends)? };
            for (id, mut nw) in plan {
                if moved_ends.contains_key(&id) {
                    // a rigid change carries the curve exactly (a mirror also reverses its bulge)
                    if let Some(Element::Wall(old)) = self.get(id) {
                        nw.curve = old.curve.as_ref().map(|c| c.transform(x));
                    }
                    if mirror {
                        nw.flipped = !nw.flipped;
                    }
                }
                updates.push((id, Element::Wall(nw)));
            }
        }

        for &id in &floors {
            let Some(Element::Floor(f)) = self.get(id) else { continue };
            let mut nf = f.clone();
            nf.boundary.clear();
            nf.loops = f.loops().iter().map(|l| l.iter().map(|c| c.transform(x)).collect()).collect();
            if let Some(s) = &mut nf.slope {
                s.tail = x.apply(s.tail);
                s.head = x.apply(s.head);
            }
            if let Some(s) = &mut nf.span {
                s.a = x.apply(s.a);
                s.b = x.apply(s.b);
            }
            if opt.copy {
                nf.pinned = false;
                new_floors.push(nf);
            } else {
                updates.push((id, Element::Floor(nf)));
            }
        }

        for &id in &roofs {
            let Some(Element::Roof(r)) = self.get(id) else { continue };
            let mut nr = r.clone();
            let crate::roof::RoofShape::Footprint { loops } = &mut nr.shape;
            for e in loops.iter_mut().flatten() {
                e.curve = e.curve.transform(x);
            }
            if opt.copy {
                nr.pinned = false;
                nr.mark.clear();
                nr.ifc.guid = new_ifc_guid();
                new_roofs.push(nr);
            } else {
                updates.push((id, Element::Roof(nr)));
            }
        }

        for &id in &grids {
            let Some(Element::Grid(g)) = self.get(id) else { continue };
            let ng = g.moved(x);
            if opt.copy {
                new_grids.push(Grid { pinned: false, ..ng });
            } else {
                updates.push((id, Element::Grid(ng)));
            }
        }

        for &id in &followers {
            if !opt.copy {
                // the instance is stored against its wall, so only a mirror changes it:
                // the wall's exterior side swaps with it, so the facing swaps too
                if mirror {
                    let Some(Element::FamilyInstance(i)) = self.get(id) else { continue };
                    let mut ni = i.clone();
                    ni.flip_facing = !ni.flip_facing;
                    updates.push((id, Element::FamilyInstance(ni)));
                }
                continue;
            }
            let Some(Element::FamilyInstance(i)) = self.get(id) else { continue };
            let mut ni = i.clone();
            ni.pinned = false;
            ni.mark.clear();
            ni.ifc.guid = new_ifc_guid();
            if mirror {
                ni.flip_facing = !ni.flip_facing;
            }
            new_insts.push((id, ni, true));
        }

        for &id in &lone {
            let Some(Element::FamilyInstance(i)) = self.get(id) else { continue };
            let mut ni = i.clone();
            if let Some(h) = i.host {
                let Some(frame) = self.instance_frame(id) else { return Err("the host wall no longer exists".into()) };
                let Xform::Move { dx, dy } = x else {
                    return Err("doors and windows rotate and mirror with their walls: select the walls too".into());
                };
                let target = Point::new(frame.origin.x + dx, frame.origin.y + dy);
                if opt.copy || opt.disjoin {
                    let (wall, u) = self.host_wall_at(i.level, target, Some(h.wall)).ok_or("there is no wall there to host it")?;
                    ni.host = Some(HostRef { wall, u_mm: u });
                    if let Some(Element::Wall(nw)) = self.get(wall) {
                        let (_, across, _) = nw.path().project(target);
                        // facing follows the side the instance lands on, as when placing
                        if wall != h.wall {
                            ni.flip_facing = across < 0.0;
                        }
                    }
                } else {
                    // moving along its own host: only the part of the move along the wall counts
                    let u = h.u_mm + dx * frame.dir.x + dy * frame.dir.y;
                    ni.host = Some(HostRef { wall: h.wall, u_mm: u });
                }
            } else {
                ni.location = x.apply(i.location);
                ni.rotation_deg = if mirror { 2.0 * axis_deg - i.rotation_deg } else { i.rotation_deg + turn }.rem_euclid(360.0);
                // a reflection is a turn plus a flip across the instance's own x axis
                if mirror {
                    ni.flip_facing = !ni.flip_facing;
                }
            }
            if opt.copy {
                ni.pinned = false;
                ni.mark.clear();
                ni.ifc.guid = new_ifc_guid();
                new_insts.push((id, ni, false));
            } else {
                updates.push((id, Element::FamilyInstance(ni)));
            }
        }

        let changed_walls: BTreeSet<Id> = updates.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(i, _)| *i).collect();
        let updated: BTreeSet<Id> = updates.iter().map(|(i, _)| *i).collect();
        let name = x.name(opt.copy);
        let requested: Vec<Id> = set.iter().copied().collect();
        self.transact(name, |tx| {
            let mut result = vec![];
            for (id, e) in updates {
                tx.update(id, e)?;
            }
            let mut wall_map: BTreeMap<Id, Id> = BTreeMap::new();
            for (orig, w) in new_walls {
                let id = tx.add(Element::Wall(w));
                wall_map.insert(orig, id);
                result.push(id);
            }
            for f in new_floors {
                result.push(tx.add(Element::Floor(f)));
            }
            for r in new_roofs {
                result.push(tx.add(Element::Roof(r)));
            }
            for e in new_details {
                result.push(tx.add(e));
            }
            for mut g in new_grids {
                g.name = tx.doc_ref().next_grid_name();
                result.push(tx.add(Element::Grid(g)));
            }
            let mut added: Vec<(Id, FamilyInstance)> = vec![];
            for (_orig, mut ni, follows) in new_insts {
                if follows {
                    if let Some(h) = ni.host {
                        if let Some(&nw) = wall_map.get(&h.wall) {
                            ni.host = Some(HostRef { wall: nw, u_mm: h.u_mm });
                        }
                    }
                }
                let id = tx.add(Element::FamilyInstance(ni.clone()));
                added.push((id, ni));
                result.push(id);
            }
            // every opening has to still fit where it ended up
            let doc = tx.doc_ref();
            for (id, i) in doc.instances() {
                let touched = added.iter().any(|(a, _)| *a == id) || i.host.is_some_and(|h| changed_walls.contains(&h.wall)) || updated.contains(&id);
                if touched {
                    doc.fits(i, Some(id)).map_err(|e| format!("a door or window would not fit: {e}"))?;
                }
            }
            Ok(if opt.copy { result } else { requested })
        })
    }

    /// Offset walls a set distance sideways. A single wall moves (or is copied)
    /// perpendicular to itself. Several walls that form a run are offset as a run, so
    /// the corners stay joined. `dist` is positive towards the left of the first
    /// wall listed, whichever way the run is traversed.
    pub fn offset_walls(&mut self, ids: &[Id], dist: f64, copy: bool) -> Result<Vec<Id>, String> {
        if dist.abs() < 1e-6 {
            return Err("the offset distance must not be zero".into());
        }
        let walls: Vec<Id> = ids.iter().copied().filter(|i| matches!(self.get(*i), Some(Element::Wall(_)))).collect();
        if walls.is_empty() {
            return Err("select a wall to offset".into());
        }
        if walls.iter().any(|i| matches!(self.get(*i), Some(Element::Wall(w)) if w.curve.is_some())) {
            if walls.len() > 1 {
                return Err("offset curved walls one at a time".into());
            }
            return self.offset_curved_wall(walls[0], dist, copy);
        }
        if walls.len() == 1 {
            let Some(Element::Wall(w)) = self.get(walls[0]) else { unreachable!() };
            let len = w.start.dist(&w.end);
            let n = Point::new(-(w.end.y - w.start.y) / len, (w.end.x - w.start.x) / len);
            let name = if copy { "Offset copy" } else { "Offset" };
            let r = self.transform_elements(&walls, Xform::Move { dx: n.x * dist, dy: n.y * dist }, EditOptions { copy, disjoin: false });
            // keep the undo name specific
            if r.is_ok() {
                self.rename_last(name);
            }
            return r;
        }
        let ordered = chain_order(self, &walls)?;
        // positive is left of the first wall listed, whichever way the run is traversed
        let first = ids.iter().find(|i| ordered.iter().any(|o| o.0 == **i)).copied();
        let reversed = first.and_then(|f| ordered.iter().find(|o| o.0 == f)).is_some_and(|o| matches!(self.get(o.0), Some(Element::Wall(w)) if w.start.dist(&o.1) > 1.0));
        let dist = if reversed { -dist } else { dist };
        let chain: Vec<(Point, Point)> = ordered.iter().map(|(_, a, b)| (*a, *b)).collect();
        // offset each wall's line along the run, then meet neighbours at the corners
        let lines: Vec<(Point, Point)> = chain
            .iter()
            .map(|(a, b)| {
                let len = a.dist(b);
                let n = Point::new(-(b.y - a.y) / len, (b.x - a.x) / len);
                (Point::new(a.x + n.x * dist, a.y + n.y * dist), Point::new(b.x + n.x * dist, b.y + n.y * dist))
            })
            .collect();
        let closed = chain.len() > 2 && chain[0].0.dist(&chain[chain.len() - 1].1) <= 1.0;
        let n = lines.len();
        let mut nodes: Vec<Point> = vec![];
        nodes.push(if closed { line_intersection(lines[n - 1].0, lines[n - 1].1, lines[0].0, lines[0].1).unwrap_or(lines[0].0) } else { lines[0].0 });
        for i in 1..n {
            nodes.push(line_intersection(lines[i - 1].0, lines[i - 1].1, lines[i].0, lines[i].1).unwrap_or(lines[i].0));
        }
        nodes.push(if closed { nodes[0] } else { lines[n - 1].1 });
        // map back to each wall's own drawing direction
        let mut moves: BTreeMap<Id, (Point, Point)> = BTreeMap::new();
        for (k, id) in walls_in_chain_order(self, &walls, &chain).iter().enumerate() {
            let Some(Element::Wall(w)) = self.get(*id) else { continue };
            let (a, b) = (nodes[k], nodes[k + 1]);
            let forward = w.start.dist(&chain[k].0) <= 1.0;
            moves.insert(*id, if forward { (a, b) } else { (b, a) });
        }
        if copy {
            let mut res = vec![];
            let mut new: Vec<Wall> = vec![];
            for (id, (s, e)) in &moves {
                let Some(Element::Wall(w)) = self.get(*id) else { continue };
                let mut nw = w.clone();
                nw.start = *s;
                nw.end = *e;
                nw.pinned = false;
                nw.mark.clear();
                nw.ifc.guid = new_ifc_guid();
                new.push(nw);
            }
            self.transact("Offset copy", |tx| {
                for w in new {
                    res.push(tx.add(Element::Wall(w)));
                }
                Ok(())
            })?;
            return Ok(res);
        }
        self.refuse_pinned(&walls, "offset")?;
        let plan = self.plan_wall_slide(&moves)?;
        let changed: BTreeSet<Id> = plan.keys().copied().collect();
        self.transact("Offset", |tx| {
            for (id, w) in plan {
                tx.update(id, Element::Wall(w))?;
            }
            let doc = tx.doc_ref();
            for (id, i) in doc.instances() {
                if i.host.is_some_and(|h| changed.contains(&h.wall)) {
                    doc.fits(i, Some(id)).map_err(|e| format!("a door or window would not fit: {e}"))?;
                }
            }
            Ok(())
        })?;
        Ok(walls)
    }

    /// Offset a curved wall: an arc becomes a concentric arc (towards the left of the wall for a
    /// positive distance), and the walls hosted in it keep their place along it.
    fn offset_curved_wall(&mut self, id: Id, dist: f64, copy: bool) -> Result<Vec<Id>, String> {
        let Some(Element::Wall(w)) = self.get(id).cloned() else { return Err("not a wall".into()) };
        let c = w.curve.clone().ok_or("not a curved wall")?;
        if !copy {
            self.refuse_pinned(&[id], "offset")?;
        }
        let path = w.path();
        let (mid, dir) = path.at(path.length() / 2.0);
        let side = if dist >= 0.0 { 1.0 } else { -1.0 };
        let towards = Point::new(mid.x - dir.y * side * 10.0, mid.y + dir.x * side * 10.0);
        let nc = crate::sketch::offset_curve(&c, towards, dist.abs())?;
        let mut nw = w.clone();
        nw.start = nc.start();
        nw.end = nc.end();
        nw.curve = Some(nc);
        self.check_wall(&nw)?;
        if copy {
            nw.pinned = false;
            nw.mark.clear();
            nw.ifc.guid = new_ifc_guid();
            return self.transact("Offset copy", |tx| Ok(vec![tx.add(Element::Wall(nw))]));
        }
        self.apply_wall_ends(&[(id, nw)], "Offset")?;
        Ok(vec![id])
    }

    /// Trim or extend a wall to meet another wall's location line. The part of the
    /// wall nearest `pick` is kept.
    pub fn trim_extend(&mut self, wall: Id, pick: Point, boundary: Id) -> Result<(), String> {
        self.refuse_pinned(&[wall], "trimmed or extended")?;
        let (Some(Element::Wall(w)), Some(Element::Wall(b))) = (self.get(wall).cloned(), self.get(boundary).cloned()) else {
            return Err("trim and extend work on walls".into());
        };
        if wall == boundary {
            return Err("pick a different wall as the boundary".into());
        }
        let nw = trimmed(&w, pick, &b)?;
        self.apply_wall_ends(&[(wall, nw)], "Trim/Extend")
    }

    /// Trim or extend two walls to meet at the corner of their location lines. Each
    /// keeps the part nearest its pick.
    pub fn trim_corner(&mut self, a: Id, pick_a: Point, b: Id, pick_b: Point) -> Result<(), String> {
        self.refuse_pinned(&[a, b], "trimmed or extended")?;
        let (Some(Element::Wall(wa)), Some(Element::Wall(wb))) = (self.get(a).cloned(), self.get(b).cloned()) else {
            return Err("trim and extend work on walls".into());
        };
        if a == b {
            return Err("pick two different walls".into());
        }
        let na = trimmed(&wa, pick_a, &wb)?;
        let nb = trimmed(&wb, pick_b, &wa)?;
        self.apply_wall_ends(&[(a, na), (b, nb)], "Trim/Extend to corner")
    }

    /// Move one end of a wall (0 start, 1 end), the way a drag of its end grip does. Only this wall changes; walls
    /// butting into it run on to its new line.
    pub fn move_wall_end(&mut self, id: Id, end: u8, to: Point) -> Result<(), String> {
        self.move_wall_ends(&[(id, end)], to)
    }

    /// Move the ends of one or more walls to one point: the grip of a single wall end, or the shared grip of walls
    /// that meet at a corner. Only the listed walls change; walls butting into them run on to their new line.
    pub fn move_wall_ends(&mut self, ends: &[(Id, u8)], to: Point) -> Result<(), String> {
        if ends.is_empty() || ends.iter().any(|e| e.1 > 1) {
            return Err("name walls and their ends (0 start, 1 end)".into());
        }
        let ids: Vec<Id> = ends.iter().map(|e| e.0).collect();
        self.refuse_pinned(&ids, "moved")?;
        let plan = self.plan_wall_grip(ends, to)?;
        let list: Vec<(Id, Wall)> = plan.into_iter().collect();
        self.apply_wall_ends(&list, "Move wall end")
    }

    /// The drag points of a curved wall besides its two ends: the middle of an arc (moving it
    /// changes the radius) and every fit point of a spline between the ends.
    pub fn wall_grips(w: &Wall) -> Vec<Point> {
        match &w.curve {
            Some(Curve::Arc { .. }) => vec![w.path().at(w.length() / 2.0).0],
            Some(Curve::Spline { points, closed: false }) if points.len() > 2 => points[1..points.len() - 1].to_vec(),
            _ => vec![],
        }
    }

    /// The wall as it would be with drag point `k` (see `wall_grips`) moved to `to`.
    pub fn reshaped_wall(&self, id: Id, k: usize, to: Point) -> Result<Wall, String> {
        let Some(Element::Wall(w)) = self.get(id) else { return Err("not a wall".into()) };
        let mut nw = w.clone();
        match &w.curve {
            Some(Curve::Arc { start, end, .. }) => {
                // the arc through the end points and the dragged point
                let (a, b, c) = (*start, to, *end);
                let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
                if d.abs() < 1e-6 {
                    return Err("those three points are in a line, so they make no arc".into());
                }
                let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
                let centre = Point::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
                let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
                nw.curve = Some(Curve::Arc { start: a, end: c, centre, ccw: cross > 0.0 });
            }
            Some(Curve::Spline { points, closed: false }) => {
                let n = points.len();
                if k + 2 >= n {
                    return Err("there is no such drag point".into());
                }
                let mut pts = points.clone();
                pts[k + 1] = to; // the grips are the fit points between the two ends
                nw.curve = Some(Curve::Spline { points: pts, closed: false });
            }
            _ => return Err("this wall has no drag points".into()),
        }
        Ok(nw)
    }

    /// Drag point `k` of a curved wall to `to` (the ends stay where they are).
    pub fn reshape_wall(&mut self, id: Id, k: usize, to: Point) -> Result<(), String> {
        self.refuse_pinned(&[id], "reshaped")?;
        let nw = self.reshaped_wall(id, k, to)?;
        self.check_wall(&nw)?;
        self.apply_wall_ends(&[(id, nw)], "Reshape wall")
    }

    pub(crate) fn apply_wall_ends(&mut self, list: &[(Id, Wall)], name: &str) -> Result<(), String> {
        for (_, w) in list {
            if w.start.dist(&w.end) < 1.0 {
                return Err("the wall would be too short".into());
            }
        }
        let changed: BTreeSet<Id> = list.iter().map(|(i, _)| *i).collect();
        let list = list.to_vec();
        self.transact(name, |tx| {
            for (id, w) in list {
                tx.update(id, Element::Wall(w))?;
            }
            let doc = tx.doc_ref();
            for (id, i) in doc.instances() {
                if i.host.is_some_and(|h| changed.contains(&h.wall)) {
                    doc.fits(i, Some(id)).map_err(|e| format!("a door or window would not fit: {e}"))?;
                }
            }
            Ok(())
        })
    }

    /// Move elements up or down (elevation views): walls, floors and roofs change their base offset,
    /// doors and windows their sill, levels their elevation. Anything else refuses.
    pub fn move_vertical(&mut self, ids: &[Id], dz: f64) -> Result<(), String> {
        if !dz.is_finite() || dz.abs() < 1e-9 {
            return Ok(());
        }
        self.refuse_pinned(ids, "moved")?;
        let ids = ids.to_vec();
        self.grouped("Move", |d| {
            for id in ids {
                match d.get(id).cloned() {
                    Some(Element::Wall(w)) => {
                        let mut patch = serde_json::json!({ "base_offset_mm": w.base_offset_mm + dz });
                        if w.top_level.is_some() {
                            patch["top_offset_mm"] = serde_json::json!(w.top_offset_mm + dz);
                        }
                        d.patch_wall(id, &patch)?;
                    }
                    Some(Element::Floor(mut f)) => {
                        f.offset_mm += dz;
                        d.transact("Move", |tx| tx.update(id, Element::Floor(f)))?;
                    }
                    Some(Element::Roof(r)) => d.patch_roof(id, &serde_json::json!({ "base_offset_mm": r.base_offset_mm + dz }))?,
                    Some(Element::FamilyInstance(i)) if i.host.is_none() => d.patch_instance(id, &serde_json::json!({ "offset_mm": i.offset_mm + dz }))?,
                    Some(Element::FamilyInstance(i)) => {
                        let (_, _, sill) = d.instance_opening_size(&i);
                        d.patch_instance(id, &serde_json::json!({ "values": { "Sill Height": sill + dz } }))?;
                    }
                    Some(Element::Level(l)) => {
                        let shown = l.elevation_mm - d.level_datum_z(&l);
                        d.patch_level(id, &serde_json::json!({ "display_elevation_mm": shown + dz }))?;
                    }
                    _ => return Err("that element cannot be moved up or down".into()),
                }
            }
            Ok(())
        })
    }

    /// Align: move elements sideways so an edge of theirs lies on a reference line.
    /// `reference` and `target` are each a pair of points on a line; the two lines
    /// must be parallel.
    pub fn align_elements(&mut self, ids: &[Id], reference: (Point, Point), target: (Point, Point)) -> Result<Vec<Id>, String> {
        let (rd, td) = (Point::new(reference.1.x - reference.0.x, reference.1.y - reference.0.y), Point::new(target.1.x - target.0.x, target.1.y - target.0.y));
        let (rl, tl) = (rd.x.hypot(rd.y), td.x.hypot(td.y));
        if rl < 1e-6 || tl < 1e-6 {
            return Err("pick two lines to align".into());
        }
        let sin = (rd.x * td.y - rd.y * td.x) / (rl * tl);
        if sin.abs() > 0.0087 {
            return Err("only parallel lines can be aligned".into());
        }
        // distance from the target line's midpoint to the reference line, along its normal
        let n = Point::new(-rd.y / rl, rd.x / rl);
        let mid = Point::new((target.0.x + target.1.x) / 2.0, (target.0.y + target.1.y) / 2.0);
        let d = (mid.x - reference.0.x) * n.x + (mid.y - reference.0.y) * n.y;
        if d.abs() < 1e-6 {
            return Err("those lines are already aligned".into());
        }
        let r = self.transform_elements(ids, Xform::Move { dx: -n.x * d, dy: -n.y * d }, EditOptions::default());
        if r.is_ok() {
            self.rename_last("Align");
        }
        r
    }
}

/// What a wall's location line is for trimming: a straight line or the full circle of an arc.
enum Reach {
    Line(Point, Point),
    Circle(Point, f64),
}

fn reach_of(w: &Wall) -> Result<Reach, String> {
    match &w.curve {
        None => Ok(Reach::Line(w.start, w.end)),
        Some(Curve::Arc { centre, start, .. }) => Ok(Reach::Circle(*centre, centre.dist(start))),
        Some(_) => Err("trim and extend work on straight and arc walls only".into()),
    }
}

/// Where two reaches meet (none, one or two points).
fn reach_hits(a: &Reach, b: &Reach) -> Vec<Point> {
    let line_circle = |p0: Point, p1: Point, c: Point, r: f64| -> Vec<Point> {
        let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
        let l2 = dx * dx + dy * dy;
        if l2 < 1e-12 {
            return vec![];
        }
        let t0 = ((c.x - p0.x) * dx + (c.y - p0.y) * dy) / l2;
        let foot = Point::new(p0.x + dx * t0, p0.y + dy * t0);
        let h2 = r * r - foot.dist(&c).powi(2);
        if h2 < 0.0 {
            return vec![];
        }
        let h = h2.sqrt() / l2.sqrt();
        vec![Point::new(foot.x - dx * h, foot.y - dy * h), Point::new(foot.x + dx * h, foot.y + dy * h)]
    };
    match (a, b) {
        (Reach::Line(a0, a1), Reach::Line(b0, b1)) => line_intersection(*a0, *a1, *b0, *b1).into_iter().collect(),
        (Reach::Line(a0, a1), Reach::Circle(c, r)) | (Reach::Circle(c, r), Reach::Line(a0, a1)) => line_circle(*a0, *a1, *c, *r),
        (Reach::Circle(c0, r0), Reach::Circle(c1, r1)) => {
            let d = c0.dist(c1);
            if d < 1e-9 || d > r0 + r1 || d < (r0 - r1).abs() {
                return vec![];
            }
            let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
            let h = (r0 * r0 - a * a).max(0.0).sqrt();
            let (ux, uy) = ((c1.x - c0.x) / d, (c1.y - c0.y) / d);
            let m = Point::new(c0.x + ux * a, c0.y + uy * a);
            vec![Point::new(m.x - uy * h, m.y + ux * h), Point::new(m.x + uy * h, m.y - ux * h)]
        }
    }
}

/// `w` with the end beyond the boundary wall cut back to it, or the nearer end run
/// out to it, keeping the part of the wall that holds `pick`. Works for straight walls
/// and arcs against straight and arc boundaries (an arc boundary reaches round its whole circle).
fn trimmed(w: &Wall, pick: Point, boundary: &Wall) -> Result<Wall, String> {
    let (mine, theirs) = (reach_of(w)?, reach_of(boundary)?);
    let hits = reach_hits(&mine, &theirs);
    if hits.is_empty() {
        return Err("those walls do not meet, so they cannot be trimmed to each other".into());
    }
    let path = w.path();
    let x = hits
        .into_iter()
        .min_by(|a, b| {
            let (da, db) = (path.project(*a).2, path.project(*b).2);
            if (da - db).abs() < 1e-6 { a.dist(&pick).total_cmp(&b.dist(&pick)) } else { da.total_cmp(&db) }
        })
        .unwrap();
    let mut nw = w.clone();
    match (&w.curve, &mine) {
        (Some(Curve::Arc { centre, ccw, .. }), Reach::Circle(..)) => {
            let tau = std::f64::consts::TAU;
            let sweep = crate::curve::arc_sweep(w.start, w.end, *centre, *ccw).abs();
            let dir = if *ccw { 1.0 } else { -1.0 };
            let a0 = (w.start.y - centre.y).atan2(w.start.x - centre.x);
            // position round the circle from the start in the direction of travel, extensions before the start negative
            let f = |p: Point| {
                let g = (dir * ((p.y - centre.y).atan2(p.x - centre.x) - a0)).rem_euclid(tau);
                if g > sweep + (tau - sweep) / 2.0 { g - tau } else { g }
            };
            if f(pick) < f(x) {
                nw.end = x;
            } else {
                nw.start = x;
            }
            nw.curve = Some(Curve::Arc { start: nw.start, end: nw.end, centre: *centre, ccw: *ccw });
        }
        _ => {
            let d = Point::new(w.end.x - w.start.x, w.end.y - w.start.y);
            let l2 = d.x * d.x + d.y * d.y;
            let t = |p: Point| ((p.x - w.start.x) * d.x + (p.y - w.start.y) * d.y) / l2;
            if t(pick) < t(x) {
                nw.end = x; // the pick is on the start side of the meeting point: keep it, move the end
            } else {
                nw.start = x;
            }
        }
    }
    Ok(nw)
}

/// Order selected walls into one run: (id, from, to) in the direction of travel.
pub(crate) fn chain_order(doc: &Document, ids: &[Id]) -> Result<Vec<(Id, Point, Point)>, String> {
    let mut walls: Vec<(Id, Point, Point)> = vec![];
    for &id in ids {
        if let Some(Element::Wall(w)) = doc.get(id) {
            walls.push((id, w.start, w.end));
        }
    }
    let near = |a: Point, b: Point| a.dist(&b) <= 1.0;
    let n = walls.len();
    let touches = |i: usize, p: Point| walls.iter().enumerate().filter(|(j, w)| *j != i && (near(w.1, p) || near(w.2, p))).count();
    // a start for the run: an end that nothing else touches, else any wall for a closed loop
    let mut start = None;
    for (i, w) in walls.iter().enumerate() {
        if touches(i, w.1) > 1 || touches(i, w.2) > 1 {
            return Err("the selected walls do not form a single run".into());
        }
        if start.is_none() && touches(i, w.1) == 0 {
            start = Some((i, false));
        } else if start.is_none() && touches(i, w.2) == 0 {
            start = Some((i, true));
        }
    }
    let (first, reversed) = start.unwrap_or((0, false));
    let mut out: Vec<(Id, Point, Point)> = vec![];
    let mut used = vec![false; n];
    let (mut cur, mut rev) = (first, reversed);
    loop {
        used[cur] = true;
        let (id, a, b) = walls[cur];
        let (from, to) = if rev { (b, a) } else { (a, b) };
        out.push((id, from, to));
        let next = (0..n).find(|&j| !used[j] && (near(walls[j].1, to) || near(walls[j].2, to)));
        match next {
            Some(j) => {
                rev = !near(walls[j].1, to);
                cur = j;
            }
            None => break,
        }
    }
    if out.len() != n {
        return Err("the selected walls do not form a single run".into());
    }
    Ok(out)
}

fn walls_in_chain_order(doc: &Document, ids: &[Id], _chain: &[(Point, Point)]) -> Vec<Id> {
    chain_order(doc, ids).map(|c| c.into_iter().map(|(i, _, _)| i).collect()).unwrap_or_default()
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
        let wt = d.wall_types().iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0;
        (d, lvl, wt)
    }

    fn wall(d: &Document, id: Id) -> Wall {
        match d.get(id) {
            Some(Element::Wall(w)) => w.clone(),
            _ => panic!("not a wall"),
        }
    }

    fn mv(dx: f64, dy: f64) -> Xform {
        Xform::Move { dx, dy }
    }

    fn arc_wall(d: &mut Document, lvl: Id, wt: Id) -> Id {
        use crate::curve::Curve;
        let mut w = Wall::new(wt, lvl, p(4000., 0.), p(0., 4000.), 2700.);
        w.curve = Some(Curve::Arc { start: p(4000., 0.), end: p(0., 4000.), centre: p(0., 0.), ccw: true });
        d.add_wall_with(w).unwrap()
    }

    fn centre_of(w: &Wall) -> (Point, f64) {
        match &w.curve {
            Some(crate::curve::Curve::Arc { centre, start, .. }) => (*centre, centre.dist(start)),
            _ => panic!("not an arc"),
        }
    }

    #[test]
    fn moving_rotating_and_mirroring_a_curved_wall_carries_the_curve() {
        let (mut d, lvl, wt) = setup();
        let a = arc_wall(&mut d, lvl, wt);
        d.transform_elements(&[a], mv(1000., 500.), EditOptions::default()).unwrap();
        let w = wall(&d, a);
        let (c, r) = centre_of(&w);
        assert!(c.dist(&p(1000., 500.)) < 1e-6 && (r - 4000.0).abs() < 1e-6);
        assert!(w.start.dist(&p(5000., 500.)) < 1e-6);
        // quarter turn about the origin: the start now sits on the y axis
        d.transform_elements(&[a], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, EditOptions::default()).unwrap();
        let w = wall(&d, a);
        assert!(w.start.dist(&p(-500., 5000.)) < 1e-6);
        assert!(w.curve.as_ref().unwrap().start().dist(&w.start) < 1e-6 && w.curve.as_ref().unwrap().end().dist(&w.end) < 1e-6);
        // mirror across the y axis as a copy: the arc bulges the other way round and the exterior stays outside
        let ids = d.transform_elements(&[a], Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, EditOptions { copy: true, disjoin: false }).unwrap();
        let m = wall(&d, ids[0]);
        let (mc, _) = centre_of(&m);
        let (oc, _) = centre_of(&wall(&d, a));
        assert!((mc.x + oc.x).abs() < 1e-6 && (mc.y - oc.y).abs() < 1e-6);
        assert!(m.flipped);
        assert!(d.check_wall(&m).is_ok());
        assert!(m.curve.as_ref().unwrap().start().dist(&m.start) < 1e-6);
    }

    #[test]
    fn dragging_the_end_of_an_arc_keeps_the_angle_it_sweeps_and_a_joined_wall_stays() {
        let (mut d, lvl, wt) = setup();
        let a = arc_wall(&mut d, lvl, wt);
        let s = d.add_wall(wt, lvl, p(0., 4000.), p(-3000., 4000.), 2700.).unwrap();
        d.move_wall_end(a, 1, p(0., 5000.)).unwrap();
        let w = wall(&d, a);
        let sweep = |w: &Wall| {
            let (c, _) = centre_of(w);
            crate::curve::arc_sweep(w.start, w.end, c, true).to_degrees()
        };
        assert!((sweep(&w) - 90.0).abs() < 1e-6, "swept {}", sweep(&w));
        assert!(w.end.dist(&p(0., 5000.)) < 1e-6 && w.curve.as_ref().unwrap().end().dist(&p(0., 5000.)) < 1e-6);
        assert!(wall(&d, s).start.dist(&p(0., 4000.)) < 1e-6, "the straight wall stays: a grip changes only its own wall");
        d.undo();
        assert!(wall(&d, a).end.dist(&p(0., 4000.)) < 1e-6 && wall(&d, a).curve.is_some());
    }

    #[test]
    fn moving_a_wall_slides_joined_walls_along_their_own_lines() {
        let (mut d, lvl, wt) = setup();
        let a = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        let b = d.add_wall(wt, lvl, p(4000., 0.), p(4000., 3000.), 2700.).unwrap();
        // up: the joined wall keeps its direction and only its start (its length) changes
        d.transform_elements(&[a], Xform::Move { dx: 0.0, dy: 500.0 }, EditOptions::default()).unwrap();
        assert!(wall(&d, a).start.dist(&p(0., 500.)) < 1e-6 && wall(&d, a).end.dist(&p(4000., 500.)) < 1e-6);
        assert!(wall(&d, b).start.dist(&p(4000., 500.)) < 1e-6 && wall(&d, b).end.dist(&p(4000., 3000.)) < 1e-6);
        // along its own line: the corner stays and the free end moves, nothing else changes
        d.transform_elements(&[a], Xform::Move { dx: -500.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert!(wall(&d, a).start.dist(&p(-500., 500.)) < 1e-6 && wall(&d, a).end.dist(&p(4000., 500.)) < 1e-6);
        assert!(wall(&d, b).start.dist(&p(4000., 500.)) < 1e-6);
    }

    #[test]
    fn a_wall_butting_into_others_moves_without_changing_them() {
        let (mut d, lvl, wt) = setup();
        let h1 = d.add_wall(wt, lvl, p(0., 0.), p(4000., 0.), 2700.).unwrap();
        let h2 = d.add_wall(wt, lvl, p(0., 3000.), p(4000., 3000.), 2700.).unwrap();
        let s = d.add_wall(wt, lvl, p(2000., 0.), p(2000., 3000.), 2700.).unwrap();
        d.transform_elements(&[s], Xform::Move { dx: 500.0, dy: 0.0 }, EditOptions::default()).unwrap();
        assert!(wall(&d, s).start.dist(&p(2500., 0.)) < 1e-6 && wall(&d, s).end.dist(&p(2500., 3000.)) < 1e-6, "the wall stays on the other walls' lines");
        for (id, a, b) in [(h1, p(0., 0.), p(4000., 0.)), (h2, p(0., 3000.), p(4000., 3000.))] {
            assert!(wall(&d, id).start.dist(&a) < 1e-6 && wall(&d, id).end.dist(&b) < 1e-6, "the walls it runs into do not change");
        }
    }

    #[test]
    fn offsetting_an_arc_wall_makes_a_concentric_arc() {
        let (mut d, lvl, wt) = setup();
        let a = arc_wall(&mut d, lvl, wt);
        // left of an anticlockwise arc is towards the centre
        let ids = d.offset_walls(&[a], 500.0, true).unwrap();
        let (c, r) = centre_of(&wall(&d, ids[0]));
        assert!(c.dist(&p(0., 0.)) < 1e-6 && (r - 3500.0).abs() < 1e-6);
        d.offset_walls(&[a], -300.0, false).unwrap();
        assert!((centre_of(&wall(&d, a)).1 - 4300.0).abs() < 1e-6);
    }

    #[test]
    fn moving_a_wall_drags_the_walls_joined_to_it() {
        let (mut d, l, t) = setup();
        let a = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let b = d.add_wall(t, l, p(6000.0, 0.0), p(6000.0, 4000.0), 2700.0).unwrap();
        d.transform_elements(&[a], mv(0.0, 500.0), EditOptions::default()).unwrap();
        assert_eq!((wall(&d, a).start, wall(&d, a).end), (p(0.0, 500.0), p(6000.0, 500.0)));
        // b was joined at (6000, 0): its start follows, its far end stays
        assert_eq!((wall(&d, b).start, wall(&d, b).end), (p(6000.0, 500.0), p(6000.0, 4000.0)));
        // one undo step puts both back
        d.undo();
        assert_eq!(wall(&d, b).start, p(6000.0, 0.0));
        assert_eq!(wall(&d, a).start, p(0.0, 0.0));
    }

    #[test]
    fn disjoin_leaves_joined_walls_where_they_were() {
        let (mut d, l, t) = setup();
        let a = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let b = d.add_wall(t, l, p(6000.0, 0.0), p(6000.0, 4000.0), 2700.0).unwrap();
        d.transform_elements(&[a], mv(0.0, 500.0), EditOptions { copy: false, disjoin: true }).unwrap();
        assert_eq!(wall(&d, b).start, p(6000.0, 0.0));
    }

    #[test]
    fn a_wall_butting_into_a_moved_wall_runs_on_to_its_new_line() {
        let (mut d, l, t) = setup();
        let host = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let stem = d.add_wall(t, l, p(3000.0, 4000.0), p(3000.0, 30.0), 2700.0).unwrap();
        d.transform_elements(&[host], mv(0.0, -800.0), EditOptions::default()).unwrap();
        // the stem was in the host's body, so it now ends on the host's new centreline
        let s = wall(&d, stem);
        assert!((s.end.x - 3000.0).abs() < 1e-6 && (s.end.y + 800.0).abs() < 1e-6, "{:?}", s.end);
    }

    #[test]
    fn pinned_elements_refuse_to_move_but_can_be_copied() {
        let (mut d, l, t) = setup();
        let a = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        d.set_pinned(&[a], true).unwrap();
        assert!(d.transform_elements(&[a], mv(0.0, 500.0), EditOptions::default()).unwrap_err().contains("pinned"));
        let copies = d.transform_elements(&[a], mv(0.0, 500.0), EditOptions { copy: true, disjoin: false }).unwrap();
        assert_eq!(copies.len(), 1);
        assert!(!wall(&d, copies[0]).pinned, "a copy of a pinned wall is not pinned");
        assert!(wall(&d, a).pinned);
        assert_ne!(wall(&d, copies[0]).ifc.guid, wall(&d, a).ifc.guid);
    }

    #[test]
    fn copying_walls_copies_the_doors_hosted_in_them() {
        let (mut d, l, t) = setup();
        let w = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let door = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let inst = d.place_on_wall(door, w, p(1500.0, 200.0)).unwrap();
        let copies = d.transform_elements(&[w, inst], mv(0.0, 4000.0), EditOptions { copy: true, disjoin: false }).unwrap();
        assert_eq!(copies.len(), 2);
        let hosts: Vec<Id> = d.instances().iter().filter(|(i, _)| *i != inst).filter_map(|(_, i)| i.host.map(|h| h.wall)).collect();
        assert_eq!(hosts.len(), 1);
        assert_ne!(hosts[0], w, "the copied door is hosted by the copied wall");
        assert_eq!(wall(&d, hosts[0]).start, p(0.0, 4000.0));
        // the original door is untouched
        assert_eq!(d.instances().iter().find(|(i, _)| *i == inst).unwrap().1.host.unwrap().wall, w);
    }

    #[test]
    fn a_door_moves_along_its_wall_and_a_move_that_does_not_fit_is_refused() {
        let (mut d, l, t) = setup();
        let w = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let door = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let inst = d.place_on_wall(door, w, p(1500.0, 200.0)).unwrap();
        // sideways part of the move is ignored, the part along the wall counts
        d.transform_elements(&[inst], mv(500.0, 700.0), EditOptions::default()).unwrap();
        let u = |d: &Document| d.instances().iter().find(|(i, _)| *i == inst).unwrap().1.host.unwrap().u_mm;
        assert!((u(&d) - 2000.0).abs() < 1e-6);
        assert!(d.transform_elements(&[inst], mv(9000.0, 0.0), EditOptions::default()).is_err());
        assert!((u(&d) - 2000.0).abs() < 1e-6, "a refused move changes nothing");
        // Disjoin lets it land in another wall
        let w2 = d.add_wall(t, l, p(0.0, 3000.0), p(6000.0, 3000.0), 2700.0).unwrap();
        d.transform_elements(&[inst], mv(0.0, 3000.0), EditOptions { copy: false, disjoin: true }).unwrap();
        assert_eq!(d.instances().iter().find(|(i, _)| *i == inst).unwrap().1.host.unwrap().wall, w2);
        // and with nothing there it refuses
        assert!(d.transform_elements(&[inst], mv(0.0, 1500.0), EditOptions { copy: false, disjoin: true }).is_err());
    }

    #[test]
    fn rotating_turns_walls_floors_and_the_doors_hosted_in_them() {
        let (mut d, l, t) = setup();
        let w = d.add_wall(t, l, p(0.0, 0.0), p(6000.0, 0.0), 2700.0).unwrap();
        let ft = d.floor_types()[0].0;
        let f = d.add_floor(ft, l, vec![p(0.0, 0.0), p(2000.0, 0.0), p(2000.0, 1000.0), p(0.0, 1000.0)]).unwrap();
        d.transform_elements(&[w, f], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 90.0 }, EditOptions::default()).unwrap();
        let nw = wall(&d, w);
        assert!(nw.end.x.abs() < 1e-6 && (nw.end.y - 6000.0).abs() < 1e-6);
        match d.get(f) {
            Some(Element::Floor(fl)) => assert!({ let o = fl.outline(); o.iter().any(|q| q.x.abs() < 1e-6 && (q.y - 2000.0).abs() < 1e-6) }),
            _ => panic!(),
        }
        // a door cannot be rotated without its wall
        let door = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let inst = d.place_on_wall(door, w, p(100.0, 3000.0)).unwrap();
        assert!(d.transform_elements(&[inst], Xform::Rotate { cx: 0.0, cy: 0.0, angle_deg: 10.0 }, EditOptions::default()).is_err());
    }

    #[test]
    fn mirroring_keeps_the_exterior_on_the_mirrored_side() {
        let (mut d, l, t) = setup();
        let w = d.add_wall(t, l, p(1000.0, 0.0), p(5000.0, 0.0), 2700.0).unwrap();
        let door = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let inst = d.place_on_wall(door, w, p(2000.0, 200.0)).unwrap();
        let facing = d.instances().iter().find(|(i, _)| *i == inst).unwrap().1.flip_facing;
        // mirror about the vertical line x = 0
        let copies = d.transform_elements(&[w, inst], Xform::Mirror { ax: 0.0, ay: 0.0, bx: 0.0, by: 1.0 }, EditOptions { copy: true, disjoin: false }).unwrap();
        let nw = copies.iter().find_map(|i| if let Some(Element::Wall(w)) = d.get(*i) { Some(w.clone()) } else { None }).unwrap();
        assert_eq!((nw.start, nw.end), (p(-1000.0, 0.0), p(-5000.0, 0.0)));
        assert!(nw.flipped, "mirroring flips the layer order so the exterior stays on the mirrored exterior side");
        // the exterior face of the original is on +y (left of +x); in the mirror image it is
        // still on +y, which is the right of the new direction (-x), hence flipped
        let ni = d.instances().iter().find(|(i, _)| copies.contains(i)).unwrap().1.clone();
        assert_eq!(ni.flip_facing, !facing);
        assert_eq!(ni.host.unwrap().u_mm, d.instances().iter().find(|(i, _)| *i == inst).unwrap().1.host.unwrap().u_mm);
    }

    #[test]
    fn offsetting_a_run_of_walls_keeps_its_corners() {
        let (mut d, l, t) = setup();
        let a = d.add_wall(t, l, p(0.0, 0.0), p(4000.0, 0.0), 2700.0).unwrap();
        let b = d.add_wall(t, l, p(4000.0, 0.0), p(4000.0, 3000.0), 2700.0).unwrap();
        // left of the run (a runs +x, so left is +y): the corner offsets to (3900, 100)
        let copies = d.offset_walls(&[a, b], 100.0, true).unwrap();
        assert_eq!(copies.len(), 2);
        let ws: Vec<Wall> = copies.iter().map(|i| wall(&d, *i)).collect();
        let pts: Vec<Point> = ws.iter().flat_map(|w| [w.start, w.end]).collect();
        assert!(pts.iter().any(|q| q.dist(&p(0.0, 100.0)) < 1e-6));
        assert!(pts.iter().filter(|q| q.dist(&p(3900.0, 100.0)) < 1e-6).count() == 2, "{pts:?}");
        assert!(pts.iter().any(|q| q.dist(&p(3900.0, 3000.0)) < 1e-6));
        // a single wall offsets sideways
        let c = d.add_wall(t, l, p(0.0, 9000.0), p(4000.0, 9000.0), 2700.0).unwrap();
        d.offset_walls(&[c], -300.0, false).unwrap();
        assert_eq!(wall(&d, c).start, p(0.0, 8700.0));
        assert!(d.offset_walls(&[a], 0.0, false).is_err());
    }

    #[test]
    fn trim_and_extend_meet_at_the_corner() {
        let (mut d, l, t) = setup();
        // a crosses b, so trimming to the corner cuts its overshoot back
        let a = d.add_wall(t, l, p(0.0, 0.0), p(5000.0, 0.0), 2700.0).unwrap();
        let b = d.add_wall(t, l, p(4000.0, -1000.0), p(4000.0, 2000.0), 2700.0).unwrap();
        d.trim_corner(a, p(1000.0, 0.0), b, p(4000.0, 1500.0)).unwrap();
        assert_eq!(wall(&d, a).end, p(4000.0, 0.0));
        assert_eq!((wall(&d, b).start, wall(&d, b).end), (p(4000.0, 0.0), p(4000.0, 2000.0)));
        // extend one wall to another that it falls short of
        let c = d.add_wall(t, l, p(0.0, 5000.0), p(3000.0, 5000.0), 2700.0).unwrap();
        let e = d.add_wall(t, l, p(6000.0, 4000.0), p(6000.0, 8000.0), 2700.0).unwrap();
        d.trim_extend(c, p(500.0, 5000.0), e).unwrap();
        assert_eq!(wall(&d, c).end, p(6000.0, 5000.0));
        // parallel walls never meet
        let f = d.add_wall(t, l, p(0.0, 7000.0), p(1000.0, 7000.0), 2700.0).unwrap();
        assert!(d.trim_extend(f, p(0.0, 7000.0), c).is_err());
    }

    #[test]
    fn align_moves_a_wall_until_its_face_is_on_the_reference() {
        let (mut d, l, t) = setup();
        let a = d.add_wall(t, l, p(0.0, 0.0), p(4000.0, 0.0), 2700.0).unwrap();
        // move the wall until its (location line) is on y = 250
        d.align_elements(&[a], (p(0.0, 250.0), p(100.0, 250.0)), (p(0.0, 0.0), p(4000.0, 0.0))).unwrap();
        assert_eq!(wall(&d, a).start, p(0.0, 250.0));
        // not parallel
        assert!(d.align_elements(&[a], (p(0.0, 0.0), p(0.0, 100.0)), (p(0.0, 250.0), p(4000.0, 250.0))).is_err());
        // already aligned
        assert!(d.align_elements(&[a], (p(0.0, 250.0), p(100.0, 250.0)), (p(0.0, 250.0), p(4000.0, 250.0))).is_err());
    }

    #[test]
    fn an_arc_wall_trims_and_extends_to_a_straight_wall_and_to_another_arc() {
        let (mut d, lvl, wt) = setup();
        let a = arc_wall(&mut d, lvl, wt);
        let (s0, e0) = (wall(&d, a).start, wall(&d, a).end);
        // a vertical cutter across the quarter arc: keeps the part the pick is on
        let b = d.add_wall(wt, lvl, p(2000., -500.), p(2000., 6000.), 2700.).unwrap();
        d.trim_extend(a, s0, b).unwrap();
        let w = wall(&d, a);
        assert!(w.start.dist(&s0) < 1e-6, "the picked end stays");
        assert!((w.end.x - 2000.0).abs() < 1e-6, "{:?}", w.end);
        let (c, r) = centre_of(&w);
        assert!(c.dist(&p(0., 0.)) < 1e-6 && (r - w.end.dist(&p(0., 0.))).abs() < 1e-6, "still the same circle");
        // run it back out to a line beyond the original end
        let b2 = d.add_wall(wt, lvl, p(-500., 4000.), p(1000., 4000.), 2700.).unwrap();
        let before = wall(&d, a).end;
        d.trim_extend(a, before, b2).unwrap();
        let w = wall(&d, a);
        assert!((w.end.y - w.end.y.abs()).abs() < 1e-9 && w.end.y > before.y, "extended up the circle: {:?}", w.end);
        let _ = e0;
        // a straight wall extends to an arc, up to the circle
        let f = d.add_wall(wt, lvl, p(1000., 0.), p(1000., 1000.), 2700.).unwrap();
        d.trim_extend(f, p(1000., 1000.), a).unwrap();
        assert!((wall(&d, f).end.y - (16e6f64 - 1e6).sqrt()).abs() < 1e-6);
    }
}
