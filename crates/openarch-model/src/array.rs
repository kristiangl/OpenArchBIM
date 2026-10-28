//! Array: the selection repeated along a vector (linear) or around a centre (radial), as plain copies in one undo
//! step. The number counts the original. With Move To 2nd the vector or angle is the step from one member to the
//! next; with Move To Last it is the whole span, and the members are spaced evenly within it.
//!
//! Group and Associate: the selection becomes a group and each member an instance of it, kept in a `GroupArray`
//! that remembers the number, the step and Move To. Changing the number adds or removes members at the end (2nd)
//! or spaces them all again within the same span (Last). Moving one member respaces the array: the first stays,
//! the moved one lands where it was put (along the array) and every member takes the new spacing. Moving the
//! first member alone carries the whole array. Deleting or ungrouping a member, or Remove Array, ends the array
//! and leaves the groups.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::geom::Point;

/// How the members are laid out. Plan views and the 3D view use `dz = 0`; an elevation adds the vertical part.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArrayKind {
    Linear { dx: f64, dy: f64, dz: f64 },
    /// Counter-clockwise about a centre, in plan.
    Radial { cx: f64, cy: f64, angle_deg: f64 },
}

/// The largest number of members.
pub const ARRAY_MAX: usize = 200;

/// An associated array: group instances, the first the original, laid out by `step` from one to the next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupArray {
    /// The step from one member to the next (a linear array's vector, a radial array's centre and angle).
    pub step: ArrayKind,
    /// Move To Last: a new number shares the span out again. Otherwise members come and go at the end.
    pub to_last: bool,
    /// A radial array to Last over a whole turn: the turn is shared by the number, not the gaps.
    #[serde(default)]
    pub full_circle: bool,
    pub members: Vec<Id>,
}

impl GroupArray {
    /// The change that carries a member at index `i` from where it would be with step `from` to where it is with `to`.
    fn shift(from: ArrayKind, to: ArrayKind, i: usize) -> Option<Xform> {
        let k = i as f64;
        match (from, to) {
            (ArrayKind::Linear { dx: a, dy: b, .. }, ArrayKind::Linear { dx: c, dy: d, .. }) => {
                let (dx, dy) = ((c - a) * k, (d - b) * k);
                (dx.hypot(dy) > 1e-9).then_some(Xform::Move { dx, dy })
            }
            (ArrayKind::Radial { cx, cy, angle_deg: a }, ArrayKind::Radial { angle_deg: b, .. }) => {
                let t = (b - a) * k;
                (t.abs() > 1e-9).then_some(Xform::Rotate { cx, cy, angle_deg: t })
            }
            _ => None,
        }
    }

    /// The change that places member `i` from the first one.
    fn place(step: ArrayKind, i: usize) -> Xform {
        let k = i as f64;
        match step {
            ArrayKind::Linear { dx, dy, .. } => Xform::Move { dx: dx * k, dy: dy * k },
            ArrayKind::Radial { cx, cy, angle_deg } => Xform::Rotate { cx, cy, angle_deg: angle_deg * k },
        }
    }
}

impl ArrayKind {
    /// The smallest number of members in a project.
    pub fn min_count(&self) -> usize {
        match self {
            ArrayKind::Linear { .. } => 2,
            ArrayKind::Radial { .. } => 3,
        }
    }
}

impl Document {
    /// Array `ids` into `count` members (the original is the first). `to_last`: the vector or angle reaches the last
    /// member, else the second. Returns the copies, member by member.
    pub fn array_elements(&mut self, ids: &[Id], kind: ArrayKind, count: usize, to_last: bool) -> Result<Vec<Id>, String> {
        self.array_with(ids, kind, count, to_last, false).map(|(ids, _)| ids)
    }

    /// Array, with Group and Associate when `associate`: the selection (or the one group selected) becomes the first
    /// member and the copies are instances of its group. Returns the new members and the array.
    pub fn array_with(&mut self, ids: &[Id], kind: ArrayKind, count: usize, to_last: bool, associate: bool) -> Result<(Vec<Id>, Option<Id>), String> {
        if ids.is_empty() {
            return Err("select the elements to array".into());
        }
        let min = kind.min_count();
        if count < min || count > ARRAY_MAX {
            return Err(format!("the number must be from {min} to {ARRAY_MAX}"));
        }
        self.refuse_pinned(ids, "arrayed")?;
        let gaps = (count - 1) as f64;
        // the step from one member to the next
        let step = match kind {
            ArrayKind::Linear { dx, dy, dz } => {
                if dx.hypot(dy).hypot(dz) < 1e-6 {
                    return Err("the array needs two different points".into());
                }
                let k = if to_last { 1.0 / gaps } else { 1.0 };
                ArrayKind::Linear { dx: dx * k, dy: dy * k, dz: dz * k }
            }
            ArrayKind::Radial { cx, cy, angle_deg } => {
                if !angle_deg.is_finite() || angle_deg.abs() < 1e-6 {
                    return Err("the angle cannot be zero".into());
                }
                // a whole turn to the last member would put it on the original, so a full circle shares the turn out
                let full = (angle_deg.abs() - 360.0).abs() < 1e-6;
                let a = if !to_last { angle_deg } else if full { angle_deg / count as f64 } else { angle_deg / gaps };
                ArrayKind::Radial { cx, cy, angle_deg: a }
            }
        };
        let levels: Vec<Id> = ids.iter().copied().filter(|i| self.level(*i).is_some()).collect();
        if !levels.is_empty() && !matches!(step, ArrayKind::Linear { dz, .. } if dz.abs() > 1e-6) {
            return Err("levels are arrayed up or down, in an elevation or section".into());
        }
        let rest: Vec<Id> = ids.iter().copied().filter(|i| !levels.contains(i)).collect();
        if associate {
            if !levels.is_empty() {
                return Err("levels are never grouped: turn Group and Associate off to array them".into());
            }
            if matches!(step, ArrayKind::Linear { dz, .. } if dz.abs() > 1e-6) {
                return Err("an array with Group and Associate goes across, not up: turn it off to array up or down".into());
            }
            let full = matches!(kind, ArrayKind::Radial { angle_deg, .. } if to_last && (angle_deg.abs() - 360.0).abs() < 1e-6);
            return self.grouped("Array", |d| {
                // one group selected is the first member as it is; anything else is grouped first
                let first = match ids {
                    [g] if d.group_instance(*g).is_some_and(|x| x.parent.is_none()) && d.group_of(*g).is_none() => *g,
                    _ => d.create_group(ids, None, None)?.0,
                };
                if d.array_of(first).is_some() {
                    return Err("that group is already in an array".into());
                }
                let mut members = vec![first];
                for i in 1..count {
                    members.push(d.transform_elements(&[first], GroupArray::place(step, i), EditOptions { copy: true, disjoin: false })?[0]);
                }
                let a = GroupArray { step, to_last, full_circle: full, members: members.clone() };
                let id = d.transact("Array", |tx| Ok(tx.add(Element::GroupArray(a))))?;
                Ok((members[1..].to_vec(), Some(id)))
            });
        }
        self.grouped("Array", |d| {
            let mut out = vec![];
            for i in 1..count {
                let k = i as f64;
                match step {
                    ArrayKind::Linear { dx, dy, dz } => {
                        for &l in &levels {
                            out.push(d.copy_level(l, dz * k)?);
                        }
                        if rest.is_empty() {
                            continue;
                        }
                        let copies = d.transform_elements(&rest, Xform::Move { dx: dx * k, dy: dy * k }, EditOptions { copy: true, disjoin: false })?;
                        // the copies go up or down too (grids only move sideways)
                        let lift: Vec<Id> = copies.iter().copied().filter(|c| d.grid(*c).is_none()).collect();
                        if dz.abs() > 1e-6 && !lift.is_empty() {
                            d.move_vertical(&lift, dz * k)?;
                        }
                        out.extend(copies);
                    }
                    ArrayKind::Radial { cx, cy, angle_deg } => {
                        out.extend(d.transform_elements(&rest, Xform::Rotate { cx, cy, angle_deg: angle_deg * k }, EditOptions { copy: true, disjoin: false })?);
                    }
                }
            }
            if out.is_empty() {
                return Err("nothing in the selection can be arrayed".into());
            }
            Ok((out, None))
        })
    }

    pub fn group_array(&self, id: Id) -> Option<&GroupArray> {
        match self.get(id)? {
            Element::GroupArray(a) => Some(a),
            _ => None,
        }
    }

    pub fn group_arrays(&self) -> Vec<(Id, &GroupArray)> {
        self.iter().filter_map(|(i, e)| if let Element::GroupArray(a) = e { Some((i, a)) } else { None }).collect()
    }

    /// The array a group instance is a member of.
    pub fn array_of(&self, member: Id) -> Option<Id> {
        self.group_arrays().into_iter().find(|(_, a)| a.members.contains(&member)).map(|(i, _)| i)
    }

    /// Change an associated array's number: with Move To 2nd members are added or removed at the end; with Last the
    /// span stays and every member is spaced again.
    pub fn set_array_count(&mut self, id: Id, count: usize) -> Result<(), String> {
        let mut a = self.group_array(id).cloned().ok_or("not an array")?;
        let min = a.step.min_count();
        if count < min || count > ARRAY_MAX {
            return Err(format!("the number must be from {min} to {ARRAY_MAX}"));
        }
        let cur = a.members.len();
        if count == cur {
            return Ok(());
        }
        let step = if !a.to_last {
            a.step
        } else {
            match a.step {
                ArrayKind::Linear { dx, dy, dz } => {
                    let k = (cur - 1) as f64 / (count - 1) as f64;
                    ArrayKind::Linear { dx: dx * k, dy: dy * k, dz: dz * k }
                }
                ArrayKind::Radial { cx, cy, angle_deg } => {
                    let a2 = if a.full_circle { angle_deg * cur as f64 / count as f64 } else { angle_deg * (cur - 1) as f64 / (count - 1) as f64 };
                    ArrayKind::Radial { cx, cy, angle_deg: a2 }
                }
            }
        };
        self.grouped("Array number", |d| {
            if count < cur {
                // the array lets go of the tail first, so deleting it does not end the array
                let tail = a.members.split_off(count);
                let kept = GroupArray { members: a.members.clone(), ..a.clone() };
                d.transact("Array number", |tx| tx.update(id, Element::GroupArray(kept)))?;
                d.delete_groups(&tail)?;
            }
            d.respace(&a.members, a.step, step)?;
            let first = a.members[0];
            for i in cur..count {
                a.members.push(d.transform_elements(&[first], GroupArray::place(step, i), EditOptions { copy: true, disjoin: false })?[0]);
            }
            a.step = step;
            d.transact("Array number", |tx| tx.update(id, Element::GroupArray(a)))
        })
    }

    /// Move each member from where step `from` put it to where step `to` puts it.
    fn respace(&mut self, members: &[Id], from: ArrayKind, to: ArrayKind) -> Result<(), String> {
        for (i, m) in members.iter().enumerate().skip(1) {
            if let Some(x) = GroupArray::shift(from, to, i) {
                self.move_group(*m, x)?;
            }
        }
        Ok(())
    }

    /// Remove Array: the members stay as groups of the same type.
    pub fn remove_array(&mut self, id: Id) -> Result<(), String> {
        self.group_array(id).ok_or("not an array")?;
        self.transact("Remove array", |tx| tx.remove(id))
    }

    /// End the arrays any of `groups` is in (a member deleted or ungrouped).
    pub(crate) fn end_arrays_of(&mut self, groups: &[Id]) -> Result<(), String> {
        let ended: Vec<Id> = self.group_arrays().into_iter().filter(|(_, a)| a.members.iter().any(|m| groups.contains(m))).map(|(i, _)| i).collect();
        if ended.is_empty() {
            return Ok(());
        }
        self.transact("Remove array", |tx| {
            for a in ended {
                tx.remove(a)?;
            }
            Ok(())
        })
    }

    /// A transform of group instances that touches associated arrays. Returns the transform still to do on the rest,
    /// having done what the arrays need: one member moved alone respaces its array (the first moves it all); every
    /// member together carries the array's step with them; some members but not all is refused.
    pub(crate) fn arrays_for_transform(&mut self, groups: &[Id], x: Xform, copy: bool) -> Result<Vec<Id>, String> {
        if copy {
            return Ok(groups.to_vec());
        }
        let mut rest: Vec<Id> = groups.to_vec();
        for (aid, a) in self.group_arrays().into_iter().map(|(i, a)| (i, a.clone())).collect::<Vec<_>>() {
            let picked: Vec<usize> = a.members.iter().enumerate().filter(|(_, m)| groups.contains(m)).map(|(i, _)| i).collect();
            if picked.is_empty() {
                continue;
            }
            let mut na = a.clone();
            if picked.len() == a.members.len() {
                // the whole array: its step turns and mirrors with it (a radial array's centre moves too)
                na.step = match a.step {
                    ArrayKind::Linear { dx, dy, dz } => {
                        let (o, p) = (x.apply(Point::new(0.0, 0.0)), x.apply(Point::new(dx, dy)));
                        ArrayKind::Linear { dx: p.x - o.x, dy: p.y - o.y, dz }
                    }
                    ArrayKind::Radial { cx, cy, angle_deg } => {
                        let c = x.apply(Point::new(cx, cy));
                        let a2 = if matches!(x, Xform::Mirror { .. }) { -angle_deg } else { angle_deg };
                        ArrayKind::Radial { cx: c.x, cy: c.y, angle_deg: a2 }
                    }
                };
                self.transact("Move", |tx| tx.update(aid, Element::GroupArray(na)))?;
                continue;
            }
            let Xform::Move { dx, dy } = x else {
                return Err("an array member turns or mirrors only with the whole array: select all its members".into());
            };
            if picked.len() > 1 {
                return Err("move one member of an array to space it again, or all of them to move it".into());
            }
            let k = picked[0];
            rest.retain(|g| *g != a.members[k]);
            if k == 0 {
                // the first member carries the whole array
                for m in &a.members {
                    self.move_group(*m, x)?;
                }
                if let ArrayKind::Radial { cx, cy, angle_deg } = a.step {
                    na.step = ArrayKind::Radial { cx: cx + dx, cy: cy + dy, angle_deg };
                    self.transact("Move", |tx| tx.update(aid, Element::GroupArray(na)))?;
                }
                continue;
            }
            // a member further along: the spacing changes so that it lands where it was put
            let kf = k as f64;
            na.step = match a.step {
                ArrayKind::Linear { dx: sx, dy: sy, dz } => ArrayKind::Linear { dx: sx + dx / kf, dy: sy + dy / kf, dz },
                ArrayKind::Radial { cx, cy, angle_deg } => {
                    let o = self.group_instance(a.members[k]).map(|g| g.frame.origin).ok_or("an array member has gone")?;
                    let (a0, a1) = ((o.y - cy).atan2(o.x - cx), (o.y + dy - cy).atan2(o.x + dx - cx));
                    let turn = (a1 - a0).to_degrees();
                    let turn = (turn + 540.0).rem_euclid(360.0) - 180.0;
                    ArrayKind::Radial { cx, cy, angle_deg: angle_deg + turn / kf }
                }
            };
            self.respace(&a.members, a.step, na.step)?;
            self.transact("Move", |tx| tx.update(aid, Element::GroupArray(na)))?;
        }
        Ok(rest)
    }
}
