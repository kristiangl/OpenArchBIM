//! Array: the selection repeated along a vector (linear) or around a centre (radial), as plain copies in one undo
//! step. The number counts the original. With Move To 2nd the vector or angle is the step from one member to the
//! next; with Move To Last it is the whole span, and the members are spaced evenly within it.
//!
//! Group and Associate (the members kept as groups under an array that remembers its number and spacing) waits for
//! groups; this module makes the unassociated kind.

use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;

/// How the members are laid out. Plan views and the 3D view use `dz = 0`; an elevation adds the vertical part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrayKind {
    Linear { dx: f64, dy: f64, dz: f64 },
    /// Counter-clockwise about a centre, in plan.
    Radial { cx: f64, cy: f64, angle_deg: f64 },
}

/// The largest number of members.
pub const ARRAY_MAX: usize = 200;

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
            Ok(out)
        })
    }
}
