//! Ramps: runs drawn like a stair's, sloping evenly from the base to the top over their total length, with automatic
//! landings between them. The ramp type sets the steepest slope, how long a run may be before a landing (by slope:
//! at most 9 m at 1:14, 15 m at 1:20, in proportion between, as AS 1428.1 has it), the shortest landing, and the
//! cross-section: Thick (a slab under the slope) or Solid (filled down to the base). The layout is a stair layout with
//! no risers, so landings, plans, walklines and railings work as they do on stairs.

use serde::{Deserialize, Serialize};

use crate::element::{IfcParams, TypeInfo};
use crate::stairs::{landing_between, Convex, LandingLayout, RunJustification, RunLayout, StairLayout};
use crate::{Document, Element, Id, Point};

fn d_ratio() -> f64 { 14.0 }
fn d_len() -> f64 { 9000.0 }
fn d_len20() -> f64 { 15000.0 }
fn d_landing() -> f64 { 1200.0 }
fn d_thick() -> f64 { 150.0 }
fn d_width() -> f64 { 1200.0 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RampShape {
    /// A slab of the type's thickness under the sloping surface.
    #[default]
    Thick,
    /// Filled down to the ramp's base.
    Solid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RampType {
    pub name: String,
    /// The steepest slope, as 1 in this.
    #[serde(default = "d_ratio")]
    pub max_slope_ratio: f64,
    /// How long a run may be at the steepest slope before it needs a landing.
    #[serde(default = "d_len")]
    pub max_run_length_mm: f64,
    /// How long at 1 in 20 (between the two in proportion; flatter than 1 in 20 is not limited).
    #[serde(default = "d_len20")]
    pub max_run_length_at_1_in_20_mm: f64,
    #[serde(default = "d_landing")]
    pub min_landing_mm: f64,
    #[serde(default)]
    pub shape: RampShape,
    #[serde(default = "d_thick")]
    pub thickness_mm: f64,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub function: crate::stairs::StairFunction,
    #[serde(default)]
    pub info: TypeInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RampRun {
    pub start: Point,
    pub end: Point,
    #[serde(default)]
    pub justification: RunJustification,
    #[serde(default = "d_width")]
    pub width_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ramp {
    pub ramp_type: Id,
    pub base_level: Id,
    #[serde(default)]
    pub base_offset_mm: f64,
    #[serde(default)]
    pub top_level: Option<Id>,
    #[serde(default)]
    pub top_offset_mm: f64,
    #[serde(default)]
    pub unconnected_height_mm: f64,
    #[serde(default)]
    pub runs: Vec<RampRun>,
    /// Runs followed by a landing (bridging to the next run).
    #[serde(default)]
    pub landings: Vec<usize>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub comments: String,
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub ifc: IfcParams,
}

/// A step ramp (AS 1428.1-2009 as summarised by Sunshine Coast Council and Moddex, 9 Nov 2026): as steep as 1 in
/// [0], no longer than [1] and rising no more than [2] (mm).
pub const STEP_RAMP: [f64; 3] = [10.0, 1900.0, 190.0];

/// How long a run sloping at 1 in `ratio` may be before a landing.
pub fn allowed_run_length(t: &RampType, ratio: f64) -> f64 {
    if ratio >= 20.0 { return f64::INFINITY; }
    let (a, b) = (t.max_slope_ratio.min(20.0), 20.0);
    if b - a < 1e-9 { return t.max_run_length_mm; }
    let k = ((ratio - a) / (b - a)).clamp(0.0, 1.0);
    t.max_run_length_mm + (t.max_run_length_at_1_in_20_mm - t.max_run_length_mm) * k
}

impl Document {
    pub fn ramp_types(&self) -> Vec<(Id, &RampType)> {
        self.iter().filter_map(|(i, e)| if let Element::RampType(t) = e { Some((i, t)) } else { None }).collect()
    }

    pub fn ramp_type(&self, id: Id) -> Option<&RampType> {
        match self.get(id)? { Element::RampType(t) => Some(t), _ => None }
    }

    pub fn ramps(&self) -> Vec<(Id, &Ramp)> {
        self.iter().filter_map(|(i, e)| if let Element::Ramp(r) = e { Some((i, r)) } else { None }).collect()
    }

    pub fn ramp(&self, id: Id) -> Option<&Ramp> {
        match self.get(id)? { Element::Ramp(r) => Some(r), _ => None }
    }

    pub fn ensure_ramp_types(&mut self) -> Result<(), String> {
        if !self.ramp_types().is_empty() { return Ok(()); }
        self.transact("Ramp types", |tx| {
            for t in starter_ramp_types() { tx.add(Element::RampType(t)); }
            Ok(())
        })
    }

    pub fn ramp_heights(&self, r: &Ramp) -> Option<(f64, f64)> {
        let base = self.level(r.base_level)?.elevation_mm + r.base_offset_mm;
        let top = match r.top_level {
            Some(t) => self.level(t)?.elevation_mm + r.top_offset_mm,
            None => base + r.unconnected_height_mm,
        };
        Some((base, top))
    }

    pub fn new_ramp(&self, ramp_type: Id, base_level: Id, top_level: Option<Id>) -> Result<Ramp, String> {
        self.ramp_type(ramp_type).ok_or("unknown ramp type")?;
        let r = Ramp { ramp_type, base_level, base_offset_mm: 0.0, top_level, top_offset_mm: 0.0, unconnected_height_mm: 600.0, runs: vec![], landings: vec![], pinned: false, comments: String::new(), mark: String::new(), ifc: IfcParams { guid: crate::element::new_ifc_guid(), ..Default::default() } };
        let (b, t) = self.ramp_heights(&r).ok_or("unknown level")?;
        if t - b <= 0.0 { return Err("The top of the ramp must be above its base.".into()); }
        Ok(r)
    }

    /// The runs laid out: frames, lengths and heights (the slope is the height over the length drawn), the landings
    /// between them, and what breaks the type's rules.
    pub fn ramp_layout(&self, r: &Ramp) -> Option<StairLayout> {
        let t = self.ramp_type(r.ramp_type)?;
        let (base_z, top_z) = self.ramp_heights(r)?;
        let height = top_z - base_z;
        let total: f64 = r.runs.iter().map(|x| x.start.dist(&x.end)).sum();
        let k = if total > 1e-9 { height / total } else { 0.0 };
        let mut runs = vec![];
        let mut z = base_z;
        for (i, run) in r.runs.iter().enumerate() {
            let len = run.start.dist(&run.end);
            let dir = if len > 1e-9 { Point::new((run.end.x - run.start.x) / len, (run.end.y - run.start.y) / len) } else { Point::new(1.0, 0.0) };
            let left = Point::new(-dir.y, dir.x);
            let off = match run.justification { RunJustification::Left | RunJustification::LeftExterior => -0.5, RunJustification::Center => 0.0, _ => 0.5 } * run.width_mm;
            let origin = Point::new(run.start.x + left.x * off, run.start.y + left.y * off);
            let z1 = z + k * len;
            runs.push(RunLayout { index: i, origin, dir, left, width: run.width_mm, length: len, z0: z, z1, risers: vec![], treads: vec![], path: None });
            z = z1;
        }
        let mut landings = vec![];
        for (n, after) in r.landings.iter().enumerate() {
            let (Some(a), Some(b)) = (runs.get(*after), runs.get(after + 1)) else { continue };
            if let Some(outline) = landing_between(a, b) {
                landings.push(LandingLayout { index: n, after_run: *after, outline, z: a.z1, thickness: t.thickness_mm });
            }
        }
        let mut warnings = vec![];
        let ratio = if k > 1e-9 { 1.0 / k } else { f64::INFINITY };
        if r.runs.is_empty() {
            warnings.push("The ramp has no runs.".into());
        } else if ratio < t.max_slope_ratio - 1e-6 {
            // a step ramp (AS 1428.1: no longer than 1900 and rising no more than 190) may be as steep as 1:10
            let step = total <= STEP_RAMP[1] + 1e-6 && height <= STEP_RAMP[2] + 1e-6;
            if !step || ratio < STEP_RAMP[0] - 1e-6 {
                warnings.push(if step {
                    format!("The step ramp slopes at 1:{ratio:.1}, steeper than 1:{}: draw {:.0} more of it.", STEP_RAMP[0], height * STEP_RAMP[0] - total)
                } else {
                    format!("The ramp slopes at 1:{ratio:.1}, steeper than 1:{}: draw {:.0} more of it.", t.max_slope_ratio, height * t.max_slope_ratio - total)
                });
            }
        }
        let allowed = allowed_run_length(t, ratio);
        for run in &runs {
            if run.length > allowed + 1e-6 {
                warnings.push(format!("Run {} is {:.0} long; at 1:{ratio:.1} a run may be at most {allowed:.0} before a landing.", run.index + 1, run.length));
            }
        }
        for g in &landings {
            // how far the landing reaches along the run that arrives at it
            let a = &runs[g.after_run];
            let ds: Vec<f64> = g.outline.iter().map(|p| (p.x - a.origin.x) * a.dir.x + (p.y - a.origin.y) * a.dir.y - a.length).collect();
            let depth = ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - ds.iter().cloned().fold(f64::INFINITY, f64::min).max(0.0);
            if depth < t.min_landing_mm - 1e-6 {
                warnings.push(format!("Landing {} is {depth:.0} long, under {}.", g.index + 1, t.min_landing_mm));
            }
        }
        Some(StairLayout { base_z, top_z, height, desired_risers: 0, actual_risers: 0, riser_mm: 0.0, tread_mm: 0.0, runs, landings, warnings })
    }

    fn check_ramp(&self, r: &Ramp) -> Result<(), String> {
        if self.ramp_type(r.ramp_type).is_none() { return Err("unknown ramp type".into()); }
        let (b, t) = self.ramp_heights(r).ok_or("unknown level")?;
        if !(b.is_finite() && t.is_finite()) { return Err("the heights must be numbers".into()); }
        if t - b <= 0.0 { return Err("The top of the ramp must be above its base.".into()); }
        for run in &r.runs {
            if !(run.width_mm.is_finite() && run.width_mm > 0.0) { return Err("A run's width must be more than zero.".into()); }
            if run.start.dist(&run.end) < 1.0 { return Err("A run needs a length.".into()); }
        }
        Ok(())
    }

    fn ramp_landings(&self, r: &Ramp) -> Vec<usize> {
        let mut probe = r.clone();
        probe.landings.clear();
        let Some(l) = self.ramp_layout(&probe) else { return vec![] };
        (0..l.runs.len().saturating_sub(1)).filter(|i| landing_between(&l.runs[*i], &l.runs[i + 1]).is_some()).collect()
    }

    pub fn add_ramp(&mut self, mut r: Ramp) -> Result<Id, String> {
        if r.runs.is_empty() { return Err("Draw at least one run.".into()); }
        r.landings = self.ramp_landings(&r);
        self.check_ramp(&r)?;
        self.transact("Create ramp", |tx| Ok(tx.add(Element::Ramp(r))))
    }

    pub fn set_ramp_runs(&mut self, id: Id, runs: Vec<RampRun>) -> Result<(), String> {
        self.refuse_pinned(&[id], "edited")?;
        let mut r = self.ramp(id).cloned().ok_or("not a ramp")?;
        if runs.is_empty() { return Err("A ramp needs at least one run.".into()); }
        r.runs = runs;
        r.landings = self.ramp_landings(&r);
        self.check_ramp(&r)?;
        self.transact("Edit ramp", |tx| tx.update(id, Element::Ramp(r)))
    }

    pub fn patch_ramp(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.ramp(id).cloned().ok_or("not a ramp")?;
        let mut patch = patch.clone();
        let width = patch.as_object_mut().and_then(|o| o.remove("run_width_mm")).and_then(|v| v.as_f64());
        let mut r: Ramp = crate::doc::apply_patch(&old, &patch)?;
        if let Some(w) = width { for x in &mut r.runs { x.width_mm = w; } }
        let moved = r.base_level != old.base_level || r.top_level != old.top_level || r.base_offset_mm != old.base_offset_mm || r.top_offset_mm != old.top_offset_mm || r.runs != old.runs;
        if old.pinned && r.pinned && moved { return Err("Unpin the ramp before changing it.".into()); }
        r.landings = self.ramp_landings(&r);
        self.check_ramp(&r)?;
        self.transact("Ramp properties", |tx| tx.update(id, Element::Ramp(r)))
    }

    pub fn patch_ramp_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.ramp_type(id).cloned().ok_or("not a ramp type")?;
        let t: RampType = crate::doc::apply_patch(&old, patch)?;
        if t.name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.ramp_types().iter().any(|(i, o)| *i != id && o.name == t.name) { return Err(format!("a ramp type named '{}' already exists", t.name)); }
        let nums = [t.max_slope_ratio, t.max_run_length_mm, t.max_run_length_at_1_in_20_mm, t.min_landing_mm, t.thickness_mm];
        if nums.iter().any(|v| !v.is_finite() || *v < 0.0) { return Err("the type's sizes must be numbers, zero or more".into()); }
        if t.max_slope_ratio < 1.0 { return Err("the steepest slope must be 1 in 1 or flatter".into()); }
        if t.thickness_mm <= 0.0 { return Err("a ramp must have some thickness".into()); }
        self.transact("Change ramp type", |tx| tx.update(id, Element::RampType(t)))
    }

    pub fn duplicate_ramp_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let mut t = self.ramp_type(id).cloned().ok_or("not a ramp type")?;
        if name.trim().is_empty() { return Err("a type needs a name".into()); }
        if self.ramp_types().iter().any(|(_, o)| o.name == name) { return Err(format!("a ramp type named '{name}' already exists")); }
        t.name = name.into();
        self.transact("Duplicate type", |tx| Ok(tx.add(Element::RampType(t))))
    }

    /// The ramp's runs and landings as convex solids (each with the run or landing it belongs to: runs first).
    pub fn ramp_pieces(&self, r: &Ramp) -> Vec<(crate::stairs::StairPart, Option<String>, Convex)> {
        use crate::stairs::StairPart;
        let (Some(t), Some(l)) = (self.ramp_type(r.ramp_type), self.ramp_layout(r)) else { return vec![] };
        let mat = if t.material.is_empty() { None } else { Some(t.material.clone()) };
        let mut out = vec![];
        for run in &l.runs {
            let w = run.width / 2.0;
            let k = if run.length > 1e-9 { (run.z1 - run.z0) / run.length } else { 0.0 };
            let drop = t.thickness_mm * (1.0 + k * k).sqrt();
            let profile = match t.shape {
                RampShape::Thick => vec![[0.0, run.z0 - drop], [run.length, run.z1 - drop], [run.length, run.z1], [0.0, run.z0]],
                RampShape::Solid => vec![[0.0, l.base_z - t.thickness_mm], [run.length, l.base_z - t.thickness_mm], [run.length, run.z1], [0.0, run.z0]],
            };
            out.push((StairPart::Run(run.index), mat.clone(), Convex::across(run, &profile, -w, w)));
        }
        for g in &l.landings {
            let bottom = match t.shape { RampShape::Thick => g.z - t.thickness_mm, RampShape::Solid => l.base_z - t.thickness_mm };
            out.push((StairPart::Landing(g.index), mat.clone(), Convex::prism(&g.outline, bottom, g.z)));
        }
        out
    }

    pub fn ramp_meshes(&self, r: &Ramp) -> Vec<(Option<String>, Vec<f32>)> {
        let mut by: Vec<(Option<String>, Vec<f32>)> = vec![];
        for (_, m, solid) in self.ramp_pieces(r) {
            let k = match by.iter().position(|(x, _)| *x == m) { Some(k) => k, None => { by.push((m.clone(), vec![])); by.len() - 1 } };
            solid.mesh(&mut by[k].1);
        }
        by
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ramp_view_items(&self, id: Id, r: &Ramp, u: &dyn Fn(Point) -> f64, d: &dyn Fn(Point) -> f64, near: f64, far: f64, cut: Option<f64>) -> Vec<crate::site::ElevItem> {
        let pieces: Vec<(Option<String>, Convex)> = self.ramp_pieces(r).into_iter().map(|(_, m, c)| (m, c)).collect();
        self.convex_view_items(id, "Ramps", &pieces, u, d, near, far, cut)
    }

    pub fn ramp_outlines(&self, r: &Ramp) -> Vec<Vec<Point>> {
        let Some(l) = self.ramp_layout(r) else { return vec![] };
        l.runs.iter().map(|x| x.footprint()).chain(l.landings.iter().map(|g| g.outline.clone())).collect()
    }

    /// A stair's or a ramp's layout, for what stands on it (railings).
    pub fn host_layout(&self, id: Id) -> Option<StairLayout> {
        match self.get(id)? {
            Element::Stair(s) => self.stair_layout(s),
            Element::Ramp(r) => self.ramp_layout(r),
            _ => None,
        }
    }

    pub fn host_base_level(&self, id: Id) -> Option<Id> {
        match self.get(id)? {
            Element::Stair(s) => Some(s.base_level),
            Element::Ramp(r) => Some(r.base_level),
            _ => None,
        }
    }
}

pub fn starter_ramp_types() -> Vec<RampType> {
    vec![
        RampType { name: "Accessible Ramp 1:14".into(), max_slope_ratio: 14.0, max_run_length_mm: 9000.0, max_run_length_at_1_in_20_mm: 15000.0, min_landing_mm: 1200.0, shape: RampShape::Thick, thickness_mm: 150.0, material: "Concrete".into(), function: Default::default(), info: TypeInfo::default() },
        RampType { name: "Ramp 1:8".into(), max_slope_ratio: 8.0, max_run_length_mm: 1520.0, max_run_length_at_1_in_20_mm: 15000.0, min_landing_mm: 1200.0, shape: RampShape::Solid, thickness_mm: 150.0, material: "Concrete".into(), function: Default::default(), info: TypeInfo::default() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> (Document, Id, Id) {
        let d = Document::new_project();
        let lv = d.levels().iter().min_by(|a, b| a.1.elevation_mm.total_cmp(&b.1.elevation_mm)).unwrap().0;
        let ty = d.ramp_types().iter().find(|(_, t)| t.name == "Accessible Ramp 1:14").unwrap().0;
        (d, lv, ty)
    }

    #[test]
    fn a_short_low_ramp_is_a_step_ramp_and_may_be_one_in_ten() {
        let (mut d, lv, ty) = doc();
        let mut r = d.new_ramp(ty, lv, None).unwrap();
        r.unconnected_height_mm = 150.0;
        // 150 over 1600 is about 1:10.7: a step ramp, so no warning although it is steeper than 1:14
        r.runs = vec![RampRun { start: Point::new(0.0, 0.0), end: Point::new(1600.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 }];
        let id = d.add_ramp(r).unwrap();
        assert!(d.ramp_layout(d.ramp(id).unwrap()).unwrap().warnings.is_empty());
        // 150 over 1200 is 1:8: too steep even for a step ramp
        d.set_ramp_runs(id, vec![RampRun { start: Point::new(0.0, 0.0), end: Point::new(1200.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 }]).unwrap();
        let w = d.ramp_layout(d.ramp(id).unwrap()).unwrap().warnings;
        assert!(w.iter().any(|w| w.contains("step ramp") && w.contains("1:10")), "{w:?}");
    }

    #[test]
    fn a_ramp_slopes_over_its_runs_and_checks_its_slope_lengths_and_landings() {
        let (mut d, lv, ty) = doc();
        let mut r = d.new_ramp(ty, lv, None).unwrap();
        r.unconnected_height_mm = 600.0;
        // 1:14 for 600 needs 8400: two runs of 4200 with a 1500 landing between, straight on
        r.runs = vec![
            RampRun { start: Point::new(0.0, 0.0), end: Point::new(4200.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 },
            RampRun { start: Point::new(5700.0, 0.0), end: Point::new(9900.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 },
        ];
        let id = d.add_ramp(r).unwrap();
        let r = d.ramp(id).unwrap().clone();
        assert_eq!(r.landings, vec![0]);
        let l = d.ramp_layout(&r).unwrap();
        assert!((l.runs[0].z1 - l.base_z - 300.0).abs() < 1e-6 && (l.runs[1].z1 - l.base_z - 600.0).abs() < 1e-6);
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(d.ramp_pieces(&r).len(), 3);
        // shorter: too steep, and a short landing
        let short = vec![
            RampRun { start: Point::new(0.0, 0.0), end: Point::new(3000.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 },
            RampRun { start: Point::new(3800.0, 0.0), end: Point::new(6800.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 },
        ];
        d.set_ramp_runs(id, short).unwrap();
        let l = d.ramp_layout(d.ramp(id).unwrap()).unwrap();
        assert!(l.warnings.iter().any(|w| w.contains("steeper than 1:14")), "{:?}", l.warnings);
        assert!(l.warnings.iter().any(|w| w.contains("Landing 1 is 800 long")), "{:?}", l.warnings);
        // one long run: too long before a landing
        let long = vec![RampRun { start: Point::new(0.0, 0.0), end: Point::new(12000.0, 0.0), justification: RunJustification::Center, width_mm: 1200.0 }];
        d.set_ramp_runs(id, long).unwrap();
        d.patch_ramp(id, &serde_json::json!({ "unconnected_height_mm": 800.0 })).unwrap();
        let l = d.ramp_layout(d.ramp(id).unwrap()).unwrap();
        assert!(l.warnings.iter().any(|w| w.contains("before a landing")), "{:?}", l.warnings);
        assert!((allowed_run_length(d.ramp_type(ty).unwrap(), 17.0) - 12000.0).abs() < 1e-6);
    }
}
