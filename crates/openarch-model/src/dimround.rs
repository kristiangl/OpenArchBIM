//! Typed values and locks for the round dimensions: radial, diameter and angular.
//!
//! A radial or diameter dimension measures a curved wall face; typing a value changes the wall's radius about its
//! centre (the arc keeps its angles, so its ends move in or out). An angular dimension measures two straight lines;
//! typing a value turns the element of the reference that is not the anchor about the apex. An arc length dimension
//! cannot be typed into yet.
//!
//! A lock (or a driving label) holds the value: an edit that would change it is refused whole, and a typed value or a
//! new label value lifts the hold while it moves the geometry.

use crate::dimension::{DimKind, Dimension};
use crate::doc::Document;
use crate::edit::{EditOptions, Xform};
use crate::element::*;
use crate::geom::Point;
use crate::tempdim::RefSpec;

const TOL: f64 = 1e-3;

impl Document {
    /// What one segment of a dimension measures now: a length, or degrees for an angular dimension.
    pub fn segment_value(&self, d: &Dimension, segment: usize) -> Result<f64, String> {
        if d.kind.is_chain() {
            let m = self.measure(d)?;
            if segment + 1 >= m.pos.len() {
                return Err("no such segment".into());
            }
            Ok((m.pos[segment + 1] - m.pos[segment]).abs())
        } else {
            Ok(self.round_value(d)?)
        }
    }

    pub(crate) fn round_value(&self, d: &Dimension) -> Result<f64, String> {
        Ok(self.round_shape(d)?.value)
    }

    /// Move the geometry so a segment measures `value`, whatever kind of dimension it is.
    pub(crate) fn set_segment_value_raw(&mut self, id: Id, segment: usize, value: f64) -> Result<(), String> {
        let kind = self.dimension(id).ok_or("not a dimension")?.kind;
        if kind.is_chain() {
            self.set_dimension_value_raw(id, segment, value)
        } else {
            self.set_round_value_raw(id, value)
        }
    }

    /// Type a value into a radial, diameter or angular dimension.
    pub(crate) fn set_round_value_raw(&mut self, id: Id, value: f64) -> Result<(), String> {
        let Some(Element::Dimension(d)) = self.get(id).cloned() else { return Err("not a dimension".into()) };
        if !value.is_finite() || value <= 0.0 {
            return Err("a dimension must be greater than zero".into());
        }
        if d.kind == DimKind::Angular && value >= 360.0 {
            return Err("an angle must be less than 360 degrees".into());
        }
        if d.segments.first().is_some_and(|s| s.value_text.is_some()) {
            return Err("this dimension shows words in place of its value; clear them first".into());
        }
        let cur = self.round_value(&d)?;
        if (cur - value).abs() < TOL {
            return Ok(());
        }
        self.grouped("Change dimension", |doc| {
            // the typed value replaces the old one, so the hold is lifted while the geometry moves
            let held = d.segments.first().is_some_and(|s| s.locked || s.param.is_some());
            if held {
                let mut free = d.clone();
                free.segments[0].locked = false;
                free.segments[0].param = None;
                doc.transact("Change dimension", |tx| tx.update(id, Element::Dimension(free)))?;
            }
            match d.kind {
                DimKind::Radial | DimKind::Diameter => {
                    let r = if d.kind == DimKind::Diameter { value / 2.0 } else { value };
                    doc.set_face_radius(&d.refs[0].spec, r)?;
                }
                DimKind::Angular => {
                    let shape = doc.round_shape(&d)?;
                    let (r, _, sweep) = shape.arc.ok_or("no angle to change")?;
                    let _ = r;
                    // the sweep runs from the first reference's arm to the second's; turn the one that is not the anchor
                    let delta = (value - cur) * sweep.signum();
                    let anchor = d.segments.first().map_or(0, |s| s.anchor);
                    let (spec, deg) = if anchor == 0 { (&d.refs[1].spec, delta) } else { (&d.refs[0].spec, -delta) };
                    doc.turn_ref(spec, shape.centre, deg)?;
                }
                _ => return Err("an arc length dimension cannot be typed into; move the elements that bound it".into()),
            }
            let Some(Element::Dimension(mut now)) = doc.get(id).cloned() else { return Ok(()) };
            if held {
                now.segments[0].locked = d.segments[0].locked;
                now.segments[0].param = d.segments[0].param;
            }
            now.solved = vec![doc.round_value(&now)?];
            doc.transact("Change dimension", |tx| tx.update(id, Element::Dimension(now)))
        })
    }

    /// Give the curved wall face a reference names a new radius, about the same centre and through the same angles.
    fn set_face_radius(&mut self, spec: &RefSpec, face_r: f64) -> Result<(), String> {
        let RefSpec::Wall { id, offset } = spec else { return Err("only a curved wall face can be given a radius".into()) };
        let Some(Element::Wall(w)) = self.get(*id).cloned() else { return Err("not a wall".into()) };
        let (arc, ccw) = crate::dimension::wall_arc(&w).ok_or("the wall is not curved")?;
        self.refuse_pinned(&[*id], "changed")?;
        // the face is `offset` to the left of the location line: inside a counter-clockwise arc, outside a clockwise one
        let loc = if ccw { face_r + offset } else { face_r - offset };
        if loc <= 1.0 {
            return Err("the wall would have no radius left".into());
        }
        let k = loc / arc.radius;
        let c = arc.centre;
        let scale = |p: Point| Point::new(c.x + (p.x - c.x) * k, c.y + (p.y - c.y) * k);
        let mut nw = w.clone();
        nw.start = scale(w.start);
        nw.end = scale(w.end);
        if let Some(crate::curve::Curve::Arc { start, end, centre, ccw }) = &w.curve {
            nw.curve = Some(crate::curve::Curve::Arc { start: scale(*start), end: scale(*end), centre: *centre, ccw: *ccw });
        }
        self.transact_solving("Change radius", |tx| tx.update(*id, Element::Wall(nw)))
    }

    /// Turn what a reference belongs to about `centre` by `deg` (counter-clockwise).
    fn turn_ref(&mut self, spec: &RefSpec, centre: Point, deg: f64) -> Result<(), String> {
        if deg.abs() < 1e-9 {
            return Ok(());
        }
        match spec {
            RefSpec::Wall { id, .. } | RefSpec::Grid { id } | RefSpec::WallEnd { id, .. } => {
                self.transform_elements(&[*id], Xform::Rotate { cx: centre.x, cy: centre.y, angle_deg: deg }, EditOptions::default()).map(|_| ())
            }
            _ => Err("that line cannot be turned".into()),
        }
    }

    /// The round dimensions a lock or a driving label holds must keep their value. Returns an error when an edit
    /// changed one, so the edit is refused whole.
    pub(crate) fn check_round_holds(&mut self) -> Result<(), String> {
        let ids: Vec<Id> = self.dimensions().into_iter().filter(|(_, d)| !d.kind.is_chain()).map(|(i, _)| i).collect();
        for id in ids {
            let Some(Element::Dimension(d)) = self.get(id).cloned() else { continue };
            let Some(seg) = d.segments.first() else { continue };
            let driving = seg.param.and_then(|g| self.global_param(g)).filter(|g| !g.reporting).and_then(|g| g.value.as_f64());
            if !seg.locked && driving.is_none() {
                continue;
            }
            let Ok(v) = self.round_value(&d) else { continue };
            let want = driving.or_else(|| d.solved.first().copied());
            match want {
                Some(w) if (w - v).abs() > TOL => {
                    let what = match d.kind {
                        DimKind::Angular => "angle",
                        DimKind::Diameter => "diameter",
                        DimKind::ArcLength => "arc length",
                        _ => "radius",
                    };
                    return Err(format!("a locked {what} would change; unlock its dimension first"));
                }
                Some(_) => {}
                None => {
                    let mut nd = d.clone();
                    nd.solved = vec![v];
                    self.transact("Update dimension", |tx| tx.update(id, Element::Dimension(nd)))?;
                }
            }
        }
        Ok(())
    }
}
