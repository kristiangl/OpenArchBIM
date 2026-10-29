//! Placing family instances, hosting them in walls, and the openings they cut.

use crate::doc::{apply_patch, Document};
use crate::element::*;
use crate::frame::FrameKind;
use crate::family::*;
use crate::geom::Point;

/// A hosted instance placed in its wall's own coordinates, with everything the
/// plan symbol and the 3D mesh need.
#[derive(Debug, Clone, Copy)]
pub struct InstanceFrame {
    /// The host wall; 0 for a free standing instance.
    pub wall: Id,
    /// A free standing instance: no host, placed at a point on its level and turned by its rotation.
    pub free: bool,
    /// Centre of the instance on the wall's drawn path.
    pub origin: Point,
    /// Unit vector along the wall (start to end).
    pub dir: Point,
    /// Unit vector to the left of the wall (the exterior side).
    pub left: Point,
    /// How far the wall body reaches left and right of the path (mm).
    pub wall_left: f64,
    pub wall_right: f64,
    pub width: f64,
    pub height: f64,
    pub z0: f64,
    pub z1: f64,
    /// Elevation of the family's reference level (level plus Sill Height); family z is measured from it.
    pub z_ref: f64,
    /// Centre of the opening along the wall relative to the instance's position (mm); zero for most families.
    pub x_off: f64,
    pub flip_facing: bool,
    pub flip_hand: bool,
}

impl InstanceFrame {
    /// Local (along, across) to world.
    pub fn world(&self, x: f64, y: f64) -> Point {
        Point::new(self.origin.x + self.dir.x * x + self.left.x * y, self.origin.y + self.dir.y * x + self.left.y * y)
    }
    /// A point in the family's own axes (x along the host, y towards the side it faces) in the plan.
    pub fn fam_world(&self, x: f64, y: f64) -> Point {
        let hand = if self.flip_hand { -1.0 } else { 1.0 };
        let swing = if self.flip_facing { -1.0 } else { 1.0 };
        self.world(hand * x, self.y_mid() + swing * y)
    }
    /// A family point (x, y, z) in project coordinates.
    pub fn fam_point(&self, p: [f64; 3]) -> [f64; 3] {
        let w = self.fam_world(p[0], p[1]);
        [w.x, w.y, self.z_ref + p[2]]
    }
    /// Whether family to project coordinates reverse the handedness (triangles need turning round).
    pub fn fam_mirrors(&self) -> bool {
        self.flip_hand != self.flip_facing
    }
    /// Direction of the wall in degrees (counter-clockwise from +x).
    pub fn angle_deg(&self) -> f64 {
        self.dir.y.atan2(self.dir.x).to_degrees()
    }
    /// Centre of the wall body across its thickness.
    pub fn y_mid(&self) -> f64 {
        (self.wall_left - self.wall_right) / 2.0
    }
}

/// How much shallower or deeper than the wall a frame's throat may be before it is
/// reported (mm). Placeholder tolerances until Lawrence's supplier confirms.
const FRAME_FIT_SHALLOW: f64 = 5.0;
const FRAME_FIT_DEEP: f64 = 12.0;

impl Document {
    pub fn families(&self) -> Vec<(Id, &Family)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::Family(f) = e { Some((*id, f)) } else { None }).collect()
    }

    pub fn family(&self, id: Id) -> Option<&Family> {
        match self.get(id) {
            Some(Element::Family(f)) => Some(f),
            _ => None,
        }
    }

    pub fn family_types(&self) -> Vec<(Id, &FamilyType)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::FamilyType(t) = e { Some((*id, t)) } else { None }).collect()
    }

    pub fn family_type(&self, id: Id) -> Option<&FamilyType> {
        match self.get(id) {
            Some(Element::FamilyType(t)) => Some(t),
            _ => None,
        }
    }

    pub fn instances(&self) -> Vec<(Id, &FamilyInstance)> {
        self.elements.iter().filter_map(|(id, e)| if let Element::FamilyInstance(i) = e { Some((*id, i)) } else { None }).collect()
    }

    /// The family and type of an instance.
    pub fn instance_family(&self, i: &FamilyInstance) -> Option<(&Family, &FamilyType)> {
        let t = self.family_type(i.type_id)?;
        Some((self.family(t.family)?, t))
    }

    /// A type parameter: worked out by its formula if it has one, else the type's
    /// value, else the family's default.
    pub fn type_param(&self, t: &FamilyType, name: &str) -> Option<ParamValue> {
        self.param_at(t, None, name, 0)
    }

    /// Worked out like `type_param`, but a formula can also read the instance's own
    /// values (a door's Frame Type changes the hole it cuts).
    fn param_at(&self, t: &FamilyType, inst: Option<&FamilyInstance>, name: &str, depth: u8) -> Option<ParamValue> {
        self.param_try(t, inst, name, depth).ok().flatten()
    }

    /// `param_at` with the reason a formula did not work out.
    pub fn param_try(&self, t: &FamilyType, inst: Option<&FamilyInstance>, name: &str, depth: u8) -> Result<Option<ParamValue>, String> {
        let Some(fam) = self.family(t.family) else { return Ok(None) };
        let def = fam.param(name);
        if let Some(src) = def.and_then(|p| p.formula.as_deref()) {
            if depth > 8 {
                return Err(format!("'{name}' refers back to itself"));
            }
            let expr = openarch_params::expr::parse(src).map_err(|e| format!("'{name}': {e:?}"))?;
            let _tables = (depth == 0 && !fam.body.lookup_tables.is_empty()).then(|| openarch_params::lookup::use_tables(&fam.body.lookup_tables));
            let mut resolve = |n: &str| -> Result<openarch_params::Value, openarch_params::EvalError> {
                use openarch_params::Value as Val;
                let own = inst.and_then(|i| i.values.get(n)).filter(|_| fam.param(n).map_or(false, |p| p.instance && p.formula.is_none()));
                match own.cloned().or_else(|| self.param_at(t, inst, n, depth + 1)) {
                    Some(ParamValue::Number(x)) => Ok(Val::Num(x)),
                    Some(ParamValue::Bool(b)) => Ok(Val::Bool(b)),
                    Some(ParamValue::Text(s)) => Ok(Val::Text(s)),
                    None => Err(openarch_params::EvalError::UnknownName(n.to_string())),
                }
            };
            return match openarch_params::expr::eval(&expr, &mut resolve) {
                Ok(openarch_params::Value::Num(x)) => Ok(Some(ParamValue::Number(x))),
                Ok(openarch_params::Value::Bool(b)) => Ok(Some(ParamValue::Bool(b))),
                Ok(openarch_params::Value::Text(s)) => Ok(Some(ParamValue::Text(s))),
                Err(e) => Err(format!("'{name}': {}", match e {
                    openarch_params::EvalError::BadCall(m) => m,
                    other => format!("{other:?}"),
                })),
            };
        }
        if let Some(v) = t.values.get(name) {
            return Ok(Some(v.clone()));
        }
        Ok(def.map(|p| p.default.clone()))
    }

    /// An instance's value for a parameter: its own, else the type's, else the
    /// family default. Formula parameters are always worked out from the type.
    pub fn instance_param(&self, i: &FamilyInstance, name: &str) -> Option<ParamValue> {
        let t = self.family_type(i.type_id)?;
        if let Some(v) = self.reported_value(i, name) {
            return Some(ParamValue::Number(v));
        }
        let calculated = self.family(t.family).and_then(|f| f.param(name)).map_or(false, |p| p.formula.is_some());
        if !calculated {
            if let Some(v) = i.values.get(name) {
                return Some(v.clone());
            }
        }
        self.param_at(t, Some(i), name, 0)
    }

    fn dim(&self, i: &FamilyInstance, names: &[&str], fallback: f64) -> f64 {
        names.iter().find_map(|n| self.instance_param(i, n).and_then(|v| v.as_f64())).unwrap_or(fallback)
    }

    /// Width of the hole the instance cuts, height of the hole, and the sill.
    pub fn instance_opening_size(&self, i: &FamilyInstance) -> (f64, f64, f64) {
        if let Some(b) = self.instance_cut_box(i) {
            return (b[1][0] - b[0][0], b[1][2] - b[0][2], self.dim(i, &["Sill Height"], 0.0) + b[0][2]);
        }
        (
            self.dim(i, &["Rough Width", "Width"], 800.0),
            self.dim(i, &["Rough Height", "Height"], 2000.0),
            self.dim(i, &["Sill Height"], 0.0),
        )
    }

    /// Where a hosted instance sits. `None` for free standing instances or when
    /// the host has gone.
    pub fn instance_frame(&self, id: Id) -> Option<InstanceFrame> {
        let Some(Element::FamilyInstance(i)) = self.get(id) else { return None };
        let Some(host) = i.host else { return self.free_frame(id, i) };
        let Some(Element::Wall(w)) = self.get(host.wall) else { return None };
        let path = w.path();
        if path.length() < 1e-6 {
            return None;
        }
        // on a curved wall the door sits square to the wall where it is
        let (origin, dir) = path.at(host.u_mm);
        let left = Point::new(-dir.y, dir.x);
        let (wl, wr) = self.wall_extents(w);
        let (width, height, sill) = self.instance_opening_size(i);
        let (base, _) = self.wall_extent(w);
        let level_z = self.level(i.level).map_or(base, |l| l.elevation_mm);
        let z0 = level_z + sill;
        let cut = self.instance_cut_box(i);
        let z_ref = z0 - cut.map_or(0.0, |b| b[0][2]);
        let hand = if i.flip_hand { -1.0 } else { 1.0 };
        let x_off = cut.map_or(0.0, |b| hand * (b[0][0] + b[1][0]) / 2.0);
        Some(InstanceFrame {
            wall: host.wall,
            free: false,
            origin,
            dir,
            left,
            wall_left: wl,
            wall_right: wr,
            width,
            height,
            z0,
            z1: z0 + height,
            z_ref,
            x_off,
            flip_facing: i.flip_facing,
            flip_hand: i.flip_hand,
        })
    }

    /// Where a free standing instance sits: at its location on its level (plus Elevation from Level), turned by
    /// its rotation. Width and height are the extent of its solved body, so views can tell what the cut plane crosses.
    fn free_frame(&self, id: Id, i: &FamilyInstance) -> Option<InstanceFrame> {
        let (fam, _) = self.instance_family(i)?;
        if fam.body.is_empty() {
            return None;
        }
        let level_z = self.level(i.level)?.elevation_mm;
        let z_ref = level_z + i.offset_mm;
        let a = i.rotation_deg.to_radians();
        let dir = Point::new(a.cos(), a.sin());
        let left = Point::new(-dir.y, dir.x);
        let b = self.instance_body_bounds(id).unwrap_or([[0.0; 3]; 2]);
        Some(InstanceFrame {
            wall: 0,
            free: true,
            origin: i.location,
            dir,
            left,
            wall_left: 0.0,
            wall_right: 0.0,
            width: b[1][0] - b[0][0],
            height: b[1][2] - b[0][2],
            z0: z_ref + b[0][2],
            z1: z_ref + b[1][2],
            z_ref,
            x_off: 0.0,
            flip_facing: i.flip_facing,
            flip_hand: i.flip_hand,
        })
    }

    /// The box the visible solids of an instance's body take up, in the family's own axes.
    pub fn instance_body_bounds(&self, id: Id) -> Option<[[f64; 3]; 2]> {
        let solved = self.instance_solved(id)?;
        let s = solved.as_ref().as_ref().ok()?;
        let mut out: Option<[[f64; 3]; 2]> = None;
        let mut grow = |p: [f64; 3]| {
            let b = out.get_or_insert([p, p]);
            for k in 0..3 {
                b[0][k] = b[0][k].min(p[k]);
                b[1][k] = b[1][k].max(p[k]);
            }
        };
        for f in s.forms.iter().filter(|f| !f.void && f.vis.present) {
            for t in &f.solid.tris {
                for p in t {
                    grow(*p);
                }
            }
        }
        for m in s.model_lines.iter().filter(|m| m.vis.present) {
            for p in &m.points {
                grow(*p);
            }
        }
        out
    }

    /// Place a free standing family (furniture, fixtures, generic models and the like) at `point` on `level`,
    /// turned `rotation_deg` anticlockwise. A work-plane-based family is placed on the level's plane. One undo step.
    pub fn place_free(&mut self, type_id: Id, level: Id, point: Point, rotation_deg: f64, offset_mm: f64) -> Result<Id, String> {
        let ft = self.family_type(type_id).ok_or("unknown family type")?.clone();
        let fam = self.family(ft.family).ok_or("type has no family")?;
        match fam.host {
            HostKind::None | HostKind::Face => {}
            HostKind::Wall => return Err(format!("{} is hosted in walls: click a wall to place it", fam.name)),
            HostKind::Floor => return Err(format!("{} is hosted in floors, and placing in floors is not built yet", fam.name)),
            HostKind::Ceiling => return Err(format!("{} is hosted in ceilings, and placing in ceilings is not built yet", fam.name)),
            HostKind::Roof => return Err(format!("{} is hosted in roofs, and placing in roofs is not built yet", fam.name)),
        }
        if fam.body.is_empty() {
            return Err(format!("{} has no geometry of its own to place", fam.name));
        }
        if self.level(level).is_none() {
            return Err("pick a level to place it on".into());
        }
        if !(point.x.is_finite() && point.y.is_finite() && rotation_deg.is_finite() && offset_mm.is_finite()) {
            return Err("the position must be numbers".into());
        }
        let mut inst = FamilyInstance::new(type_id, level);
        inst.location = point;
        inst.rotation_deg = rotation_deg.rem_euclid(360.0);
        inst.offset_mm = offset_mm;
        inst.ifc.export_as = fam.category.ifc_class().to_string();
        inst.ifc.predefined_type = fam.category.ifc_predefined().to_string();
        self.transact("Place component", |tx| Ok(tx.add(Element::FamilyInstance(inst))))
    }

    /// Openings cut in a wall by the doors and windows hosted in it, in order
    /// along the wall, clipped to the wall's length and height.
    pub fn wall_openings(&self, wall: Id) -> Vec<Opening> {
        let Some(Element::Wall(w)) = self.get(wall) else { return vec![] };
        let len = w.length();
        let (wz0, wz1) = self.wall_extent(w);
        let mut out = vec![];
        for (id, i) in self.instances() {
            let Some(h) = i.host else { continue };
            if h.wall != wall {
                continue;
            }
            let Some(f) = self.instance_frame(id) else { continue };
            let o = Opening { instance: id, u0: (h.u_mm + f.x_off - f.width / 2.0).max(0.0), u1: (h.u_mm + f.x_off + f.width / 2.0).min(len), z0: f.z0.max(wz0), z1: f.z1.min(wz1) };
            if o.u1 - o.u0 > 1e-6 && o.z1 - o.z0 > 1e-6 {
                out.push(o);
            }
        }
        // curtain walls standing in this one with Automatically Embed cut their openings too
        out.extend(self.embed_cuts(wall));
        out.sort_by(|a, b| a.u0.total_cmp(&b.u0));
        out
    }

    /// Area (mm2) of the wall face taken up by openings.
    pub fn wall_opening_area(&self, wall: Id) -> f64 {
        self.wall_openings(wall).iter().map(|o| (o.u1 - o.u0) * (o.z1 - o.z0)).sum()
    }

    /// Plan outline of a wall as separate pieces, broken where openings that
    /// span `cut_z` interrupt it. Each piece is `[start-right, end-right,
    /// end-left, start-left]` like the wall outline. Without a cut plane (the
    /// wall is not cut) the whole outline comes back.
    pub fn wall_plan_pieces(&self, wall: Id, outline: &[Point; 4], cut_z: Option<f64>) -> Vec<[Point; 4]> {
        let Some(cut) = cut_z else { return vec![*outline] };
        if self.get(wall).is_some_and(|e| matches!(e, Element::Wall(w) if w.curve.is_some())) {
            return vec![*outline];
        }
        let blocked: Vec<(f64, f64)> = self.wall_openings(wall).into_iter().filter(|o| o.z0 < cut && cut < o.z1).map(|o| (o.u0, o.u1)).collect();
        if blocked.is_empty() {
            return vec![*outline];
        }
        let Some(Element::Wall(w)) = self.get(wall) else { return vec![*outline] };
        let len = w.start.dist(&w.end);
        let mut cuts = vec![0.0];
        for (a, b) in &blocked {
            cuts.push(*a);
            cuts.push(*b);
        }
        cuts.push(len);
        // pairs (0..a1), (b1..a2), ... (bn..len)
        cuts.chunks(2).filter(|c| c[1] - c[0] > 1.0).map(|c| self.wall_slice(w, outline, c[0], c[1], len)).collect()
    }

    /// The part of a wall between `ua` and `ub` along its path, using the joined
    /// outline at the wall's own ends.
    pub fn wall_slice(&self, w: &Wall, o: &[Point; 4], ua: f64, ub: f64, len: f64) -> [Point; 4] {
        let (wl, wr) = self.wall_extents(w);
        let dir = Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        let n = Point::new(-dir.y, dir.x);
        let at = |u: f64, side: f64| Point::new(w.start.x + dir.x * u + n.x * side, w.start.y + dir.y * u + n.y * side);
        let ra = if ua <= 1e-6 { o[0] } else { at(ua, -wr) };
        let la = if ua <= 1e-6 { o[3] } else { at(ua, wl) };
        let rb = if ub >= len - 1e-6 { o[1] } else { at(ub, -wr) };
        let lb = if ub >= len - 1e-6 { o[2] } else { at(ub, wl) };
        [ra, rb, lb, la]
    }

    // ---- placing and editing ---------------------------------------------

    /// Add the starter door and window families if the project has none (older
    /// project files). Not an undo step.
    pub fn ensure_starter_families(&mut self) {
        let have: Vec<String> = self.families().iter().map(|(_, f)| f.name.clone()).collect();
        // built-in families saved before they had frame parameters or a stored body take
        // the current definition (their types and placed doors stay)
        let upgrades: Vec<(Id, crate::family::Family)> = starter_families()
            .into_iter()
            .filter_map(|sf| {
                let (id, old) = self.families().into_iter().find(|(_, f)| f.name == sf.family.name)?;
                let frame_params_missing = sf.family.param("Frame Type").is_some() && old.param("Frame Type").is_none();
                let body_missing = old.body.is_empty() && !sf.family.body.is_empty();
                // the first stored bodies (14 Oct) built frames from separate extrusions, the head apart from the jambs
                let has_sweep = |b: &crate::famgeom::FamilyBody| b.forms.iter().any(|f| matches!(f.shape, crate::famgeom::FormShape::Sweep { .. }));
                let extruded_frame = |b: &crate::famgeom::FamilyBody| {
                    b.forms.iter().any(|f| f.name.ends_with("Jambs") || (f.name == "Frame" && matches!(f.shape, crate::famgeom::FormShape::Extrusion { .. })))
                };
                let pieces_frame = has_sweep(&sf.family.body) && !old.body.is_empty() && !has_sweep(&old.body) && extruded_frame(&old.body);
                (old.built_in && (frame_params_missing || body_missing || pieces_frame)).then(|| (id, sf.family))
            })
            .collect();
        if upgrades.is_empty() && starter_families().iter().all(|sf| have.contains(&sf.family.name)) {
            return;
        }
        let _ = self.transact("Load starter families", |tx| {
            for (id, fam) in upgrades {
                let _ = tx.update(id, Element::Family(fam));
            }
            tx.add_starter_families(&have);
            Ok(())
        });
        self.clear_history();
    }

    /// Add a type from the family's catalogue (Edit Type > Add from catalogue).
    pub fn add_catalog_type(&mut self, family: Id, row_name: &str) -> Result<Id, String> {
        let fam = self.family(family).ok_or("not a family")?;
        let row = fam.catalog.iter().find(|r| r.name == row_name).ok_or("that size is not in the catalogue")?.clone();
        if self.family_types().iter().any(|(_, t)| t.family == family && t.name == row.name) {
            return Err(format!("the family already has a type named '{}'", row.name));
        }
        self.transact("Add type from catalogue", |tx| Ok(tx.add(Element::FamilyType(FamilyType { family, name: row.name, values: row.values, globals: Default::default() }))))
    }

    /// How a door's frame suits the wall it sits in, or `None` if it does. A metal
    /// frame's back opening must match the wall's thickness; a timber jamb must at
    /// least cover the core it is built into.
    pub fn frame_fit(&self, id: Id) -> Option<String> {
        let frame = self.door_frame(id)?;
        let Some(Element::FamilyInstance(inst)) = self.get(id) else { return None };
        let Some(Element::Wall(w)) = self.get(inst.host?.wall) else { return None };
        let (l, r) = self.wall_extents(w);
        let (thick, (core, _)) = (l + r, self.wall_core(w));
        let (kind, suits) = frame_for_wall(thick, core);
        match frame.kind {
            FrameKind::PressedMetal => {
                if frame.depth + FRAME_FIT_SHALLOW < thick {
                    Some(format!("The {:.0} mm back opening is shallower than the {thick:.0} mm wall.{}", frame.depth, if kind == "Timber" { " A timber frame suits a wall this thick.".to_string() } else { format!(" Use {suits:.0}.") }))
                } else if frame.depth - thick > FRAME_FIT_DEEP {
                    Some(format!("The {:.0} mm back opening is much deeper than the {thick:.0} mm wall. {suits:.0} suits this wall.", frame.depth))
                } else {
                    None
                }
            }
            FrameKind::Timber => (frame.depth + FRAME_FIT_SHALLOW < core).then(|| format!("The {:.0} mm jamb is narrower than the {core:.0} mm wall core.", frame.depth)),
        }
    }

    /// Place a door or window in a wall. `point` is where the user clicked: the
    /// instance is centred at its projection on the wall and faces the side the
    /// point is on.
    pub fn place_on_wall(&mut self, type_id: Id, wall: Id, point: Point) -> Result<Id, String> {
        self.place_on_wall_flipped(type_id, wall, point, false, false)
    }

    /// Place with the swing flipped from what the click side gives (Spacebar while placing).
    pub fn place_on_wall_flipped(&mut self, type_id: Id, wall: Id, point: Point, flip_hand: bool, flip_facing: bool) -> Result<Id, String> {
        self.place_on_wall_at(type_id, wall, point, flip_hand, flip_facing, None)
    }

    /// Place on a wall with the sill at `sill` above the wall's base level (an import knows where the opening is).
    pub fn place_on_wall_at(&mut self, type_id: Id, wall: Id, point: Point, flip_hand: bool, flip_facing: bool, sill: Option<f64>) -> Result<Id, String> {
        let ft = self.family_type(type_id).ok_or("unknown family type")?.clone();
        let fam = self.family(ft.family).ok_or("type has no family")?;
        if fam.host != HostKind::Wall {
            return Err(format!("{} does not host in walls", fam.name));
        }
        let Some(Element::Wall(w)) = self.get(wall).cloned() else { return Err("the host must be a wall".into()) };
        if self.curtain_of(&w).is_some() {
            return Err("doors and windows go into a curtain wall as panels, which come later".into());
        }
        let len = w.length();
        let (along, across, _) = w.path().project(point); // across is positive to the left of the wall
        let mut inst = FamilyInstance::new(type_id, w.base_level);
        // windows take their type's default sill; doors keep the family default
        if let Some(s) = self.type_param(&ft, "Default Sill Height") {
            inst.values.insert("Sill Height".into(), s);
        }
        if let Some(s) = sill {
            inst.values.insert("Sill Height".into(), ParamValue::Number(s));
        }
        // built-in doors take the frame that suits the wall
        if fam.param("Frame Type").is_some() && fam.param("Jamb Depth").is_some() {
            let (l, r) = self.wall_extents(&w);
            let (kind, depth) = frame_for_wall(l + r, self.wall_core(&w).0);
            inst.values.insert("Frame Type".into(), ParamValue::Text(kind.into()));
            inst.values.insert("Jamb Depth".into(), ParamValue::Number(depth));
        }
        let (width, _, _) = self.instance_opening_size(&inst);
        if width > len {
            return Err("the wall is shorter than the opening".into());
        }
        let u = along.clamp(width / 2.0, len - width / 2.0);
        inst.host = Some(HostRef { wall, u_mm: u });
        inst.flip_facing = (across < 0.0) != flip_facing;
        inst.flip_hand = flip_hand;
        inst.ifc.export_as = fam.category.ifc_class().to_string();
        inst.ifc.predefined_type = fam.category.ifc_predefined().to_string();
        self.fits(&inst, None)?;
        let name = match fam.category {
            FamilyCategory::Doors => "Place door",
            FamilyCategory::Windows => "Place window",
            _ => "Place family",
        };
        self.transact(name, |tx| Ok(tx.add(Element::FamilyInstance(inst))))
    }

    /// Check an instance fits its host: inside the wall, within its height, and
    /// not overlapping another opening. `ignore` is the instance being edited.
    pub fn fits(&self, inst: &FamilyInstance, ignore: Option<Id>) -> Result<(), String> {
        let Some(h) = inst.host else { return Ok(()) };
        let Some(Element::Wall(w)) = self.get(h.wall) else { return Err("the host wall no longer exists".into()) };
        let len = w.length();
        let (width, height, sill) = self.instance_opening_size(inst);
        if width <= 0.0 || height <= 0.0 {
            return Err("width and height must be positive".into());
        }
        if sill < 0.0 {
            return Err("sill height cannot be negative".into());
        }
        let (u0, u1) = (h.u_mm - width / 2.0, h.u_mm + width / 2.0);
        if u0 < -1e-6 || u1 > len + 1e-6 {
            return Err("the opening does not fit along the wall".into());
        }
        let (wz0, wz1) = self.wall_extent(w);
        let z0 = self.level(inst.level).map_or(wz0, |l| l.elevation_mm) + sill;
        if z0 + height > wz1 + 1e-6 || z0 < wz0 - 1e-6 {
            return Err("the opening does not fit the height of the wall".into());
        }
        for (oid, o) in self.instances() {
            if Some(oid) == ignore {
                continue;
            }
            let Some(oh) = o.host else { continue };
            if oh.wall != h.wall {
                continue;
            }
            let (ow, _, _) = self.instance_opening_size(o);
            if u0 < oh.u_mm + ow / 2.0 - 1e-6 && u1 > oh.u_mm - ow / 2.0 + 1e-6 {
                return Err("that overlaps another door or window".into());
            }
        }
        Ok(())
    }

    /// Check every instance hosted by `wall` still fits after the wall is
    /// replaced by `new_wall`. Used before wall edits that change its size.
    pub fn check_hosted_fit(&self, wall: Id, new_wall: &Wall) -> Result<(), String> {
        let mut probe = self.clone();
        probe.elements.insert(wall, Element::Wall(new_wall.clone()));
        for (id, i) in probe.instances() {
            if i.host.is_some_and(|h| h.wall == wall) {
                probe.fits(i, Some(id)).map_err(|e| format!("a door or window in this wall would no longer fit: {e}"))?;
            }
        }
        Ok(())
    }

    /// Ids of the instances hosted by any of `walls`.
    pub fn hosted_in(&self, walls: &[Id]) -> Vec<Id> {
        self.instances().into_iter().filter(|(_, i)| i.host.is_some_and(|h| walls.contains(&h.wall))).map(|(id, _)| id).collect()
    }

    /// Generic property edit of an instance (JSON merge patch), one undo step.
    /// Spacebar on selected doors and windows: step the swing through the four
    /// combinations of facing and hand (as the two flip arrows do), as one undo step.
    pub fn flip_instances(&mut self, ids: &[Id]) -> Result<(), String> {
        self.refuse_pinned(ids, "flipped")?;
        let mut edits = vec![];
        for &id in ids {
            let Some(Element::FamilyInstance(i)) = self.get(id) else { continue };
            if i.host.is_none() {
                continue;
            }
            let mut ni = i.clone();
            // (facing, hand): 00 -> 01 -> 11 -> 10 -> 00
            let next = match (ni.flip_facing, ni.flip_hand) {
                (false, false) => (false, true),
                (false, true) => (true, true),
                (true, true) => (true, false),
                (true, false) => (false, false),
            };
            ni.flip_facing = next.0;
            ni.flip_hand = next.1;
            edits.push((id, ni));
        }
        if edits.is_empty() {
            return Err("select doors or windows to flip".into());
        }
        self.transact("Flip", |tx| {
            for (id, ni) in edits {
                tx.update(id, Element::FamilyInstance(ni))?;
            }
            Ok(())
        })
    }

    pub fn patch_instance(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::FamilyInstance(i)) = self.get(id).cloned() else { return Err("not a family instance".into()) };
        let ni: FamilyInstance = apply_patch(&i, patch)?;
        if ni == i {
            return Ok(());
        }
        if self.family_type(ni.type_id).is_none() {
            return Err("unknown family type".into());
        }
        if self.level(ni.level).is_none() {
            return Err("unknown level".into());
        }
        if let Some(h) = ni.host {
            if !matches!(self.get(h.wall), Some(Element::Wall(_))) {
                return Err("the host must be a wall".into());
            }
        }
        // a type swap keeps to families that host the same way
        if ni.type_id != i.type_id {
            if let Some((fam, _)) = self.instance_family(&ni) {
                let free = matches!(fam.host, HostKind::None | HostKind::Face) && !fam.body.is_empty();
                if ni.host.is_some() && fam.host != HostKind::Wall {
                    return Err(format!("{} is not hosted in walls", fam.name));
                }
                if ni.host.is_none() && !free {
                    return Err(format!("{} cannot stand free", fam.name));
                }
            }
        }
        // only instance parameters can be set on an instance
        if let Some((fam, _)) = self.instance_family(&ni) {
            for k in ni.values.keys() {
                match fam.param(k) {
                    Some(p) if p.instance && !p.reporting => {}
                    Some(_) => return Err(format!("'{k}' is a type parameter")),
                    None => return Err(format!("the family has no parameter '{k}'")),
                }
            }
        }
        self.fits(&ni, Some(id))?;
        self.transact("Change properties", |tx| tx.update(id, Element::FamilyInstance(ni)))
    }

    /// Swap an instance to another type of the same family category.
    pub fn set_instance_type(&mut self, id: Id, type_id: Id) -> Result<(), String> {
        self.patch_instance(id, &serde_json::json!({ "type_id": type_id }))
    }

    /// Edit a family type's parameter values (JSON merge patch on the type).
    pub fn patch_family_type(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(Element::FamilyType(t)) = self.get(id).cloned() else { return Err("not a family type".into()) };
        let nt: FamilyType = apply_patch(&t, patch)?;
        if nt == t {
            return Ok(());
        }
        if nt.family != t.family {
            return Err("a type cannot move to another family".into());
        }
        if nt.name.trim().is_empty() {
            return Err("a type needs a name".into());
        }
        if self.family_types().iter().any(|(oid, o)| *oid != id && o.family == nt.family && o.name == nt.name) {
            return Err(format!("a type named '{}' already exists in this family", nt.name));
        }
        let fam = self.family(nt.family).ok_or("type has no family")?.clone();
        for (k, v) in &nt.values {
            let Some(p) = fam.param(k) else { return Err(format!("the family has no parameter '{k}'")) };
            if p.instance {
                return Err(format!("'{k}' is an instance parameter"));
            }
            if p.formula.is_some() || p.reporting {
                return Err(format!("'{k}' is worked out by a formula or a dimension and cannot be set"));
            }
            let ok = match p.param_kind {
                ParamKind::YesNo => v.as_bool().is_some(),
                ParamKind::Text | ParamKind::Material => v.as_text().is_some(),
                _ => v.as_f64().is_some(),
            };
            if !ok {
                return Err(format!("'{k}' has the wrong kind of value"));
            }
        }
        // every placed instance must still fit with the new sizes
        let mut probe = self.clone();
        probe.elements.insert(id, Element::FamilyType(nt.clone()));
        for (iid, i) in probe.instances() {
            if i.type_id == id {
                probe.fits(i, Some(iid)).map_err(|e| format!("a placed instance would no longer fit: {e}"))?;
            }
        }
        self.transact("Change type properties", |tx| tx.update(id, Element::FamilyType(nt)))
    }

    /// Delete a family type. Refused while an instance uses it and for a family's last type.
    pub fn delete_family_type(&mut self, id: Id) -> Result<(), String> {
        let t = self.family_type(id).ok_or("not a family type")?.clone();
        if self.family_types().iter().filter(|(_, o)| o.family == t.family).count() < 2 {
            return Err("a family needs at least one type".into());
        }
        let used = self.instances().iter().filter(|(_, i)| i.type_id == id).count();
        if used > 0 {
            return Err(format!("{used} placed instance{} use this type", if used == 1 { "" } else { "s" }));
        }
        self.transact("Delete type", |tx| tx.remove(id))
    }

    /// Change a family's category or host. Refused while instances are placed, since a door
    /// cannot become a chair under the people who placed it.
    pub fn set_family_meta(&mut self, id: Id, category: Option<crate::family::FamilyCategory>, host: Option<crate::family::HostKind>) -> Result<(), String> {
        let Some(Element::Family(f)) = self.get(id) else { return Err("no such family".into()) };
        let mut f = f.clone();
        let (c, h) = (category.unwrap_or(f.category), host.unwrap_or(f.host));
        if (c, h) == (f.category, f.host) {
            return Ok(());
        }
        let used = self.instances().iter().filter(|(_, i)| self.family_type(i.type_id).map_or(false, |t| t.family == id)).count();
        if used > 0 {
            return Err(format!("{used} placed instance{} use this family, so its category and host cannot change", if used == 1 { "" } else { "s" }));
        }
        f.category = c;
        f.host = h;
        self.transact("Family category and host", |tx| tx.update(id, Element::Family(f)))
    }

    /// Duplicate a family type under a new name (Edit Type > Duplicate).
    pub fn duplicate_family_type(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let t = self.family_type(id).ok_or("not a family type")?.clone();
        let name = name.trim();
        if name.is_empty() {
            return Err("a type needs a name".into());
        }
        if self.family_types().iter().any(|(_, o)| o.family == t.family && o.name == name) {
            return Err(format!("a type named '{name}' already exists in this family"));
        }
        self.transact("Duplicate type", |tx| Ok(tx.add(Element::FamilyType(FamilyType { name: name.into(), ..t }))))
    }
}

impl crate::doc::Tx<'_> {
    /// Add the starter door and window families and their types.
    pub fn add_starter_families(&mut self, have: &[String]) {
        for sf in starter_families() {
            if have.contains(&sf.family.name) {
                continue;
            }
            let fid = self.add(Element::Family(sf.family));
            for (name, values) in sf.types {
                self.add(Element::FamilyType(FamilyType { family: fid, name, values, globals: Default::default() }));
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn type_named(d: &Document, family: &str, name: &str) -> Id {
        d.family_types().into_iter().find(|(_, t)| d.family(t.family).unwrap().name == family && t.name == name).unwrap().0
    }

    /// A 6 m wall on Level 1 and its id.
    fn wall(d: &mut Document) -> Id {
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        d.add_wall(wt, lvl, p(0., 0.), p(6000., 0.), 2700.).unwrap()
    }

    #[test]
    fn new_projects_have_door_and_window_families() {
        let d = Document::new_project();
        let cats: Vec<_> = d.families().iter().map(|(_, f)| f.category).collect();
        assert!(cats.contains(&FamilyCategory::Doors) && cats.contains(&FamilyCategory::Windows));
        assert!(!d.family_types().is_empty());
    }

    #[test]
    fn a_door_in_a_curved_wall_sits_square_to_the_wall_where_it_is_and_cuts_the_layers() {
        use crate::curve::Curve;
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        let mut cw = Wall::new(wt, lvl, p(5000., 0.), p(0., 5000.), 2700.);
        cw.curve = Some(Curve::Arc { start: p(5000., 0.), end: p(0., 5000.), centre: p(0., 0.), ccw: true });
        let w = d.add_wall_with(cw).unwrap();
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        // click on the outside of the arc half way round
        let c = 5000.0 * std::f64::consts::FRAC_1_SQRT_2;
        let id = d.place_on_wall(t, w, p(c * 1.05, c * 1.05)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() };
        let len = 5000.0 * std::f64::consts::FRAC_PI_2;
        assert!((i.host.unwrap().u_mm - len / 2.0).abs() < 30.0, "centred where it was clicked");
        let f = d.instance_frame(id).unwrap();
        // half way round the quarter the wall runs at 135 degrees, so the door faces along the diagonal
        assert!((f.dir.x + f.dir.y).abs() < 0.02 && f.dir.x < 0.0);
        assert!(f.origin.dist(&p(c, c)) < 30.0);
        // the opening cuts through every layer in plan
        let g = d.wall_layer_geometry();
        let layers = d.wall_plan_layers(w, &g[&w], Some(1200.0));
        for l in &layers {
            assert!(l.shapes.len() >= 2, "{} is split by the opening", l.material);
        }
        // and the doorway is clear of material
        assert!(!layers.iter().any(|l| crate::curve::point_in_polygon(f.origin, &l.shapes.iter().flat_map(|s| s[0].clone()).collect::<Vec<_>>())));
        // nothing hosts past the ends
        assert!(d.place_on_wall(t, w, p(5000., 1.)).is_ok());
        assert!(d.wall_openings(w).len() == 2);
    }

    #[test]
    fn a_door_cuts_an_opening_and_faces_the_cursor() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        let id = d.place_on_wall(t, w, p(2000., -300.)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() };
        assert!(i.flip_facing, "cursor on the right of the wall");
        assert_eq!(i.host.unwrap().u_mm, 2000.0);
        assert!(!i.ifc.guid.is_empty());
        let ops = d.wall_openings(w);
        assert_eq!(ops.len(), 1);
        // a 200 mm wall gets a timber frame: the hole is the 824 x 2055 reveal plus a 42 mm jamb each side and above
        assert_eq!((ops[0].u0, ops[0].u1, ops[0].z0, ops[0].z1), (1546.0, 2454.0, 0.0, 2097.0));
        assert_eq!(d.wall_opening_area(w), 908.0 * 2097.0);
    }

    #[test]
    fn windows_take_the_type_sill_and_door_position_is_clamped_into_the_wall() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Fixed", "1200 x 1200");
        let id = d.place_on_wall(t, w, p(-500., 100.)).unwrap();
        let Some(Element::FamilyInstance(i)) = d.get(id) else { panic!() };
        assert_eq!(i.host.unwrap().u_mm, 600.0, "clamped so the window stays on the wall");
        let o = d.wall_openings(w)[0];
        assert_eq!((o.z0, o.z1), (900.0, 2100.0));
    }

    #[test]
    fn overlaps_and_oversize_are_refused() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(t, w, p(2000., 0.)).unwrap();
        assert!(d.place_on_wall(t, w, p(2300., 0.)).unwrap_err().contains("overlaps"));
        assert!(d.place_on_wall(t, w, p(3000., 0.)).is_ok());
        // a door taller than a low wall
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[3].0);
        let low = d.add_wall(wt, lvl, p(0., 5000.), p(4000., 5000.), 1800.).unwrap();
        assert!(d.place_on_wall(t, low, p(2000., 5000.)).unwrap_err().contains("height"));
    }

    #[test]
    fn plan_pieces_break_at_the_cut_plane_only_when_the_opening_spans_it() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let win = type_named(&d, "Fixed", "1200 x 1200"); // 900..2100
        let door = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(win, w, p(1500., 0.)).unwrap();
        d.place_on_wall(door, w, p(4000., 0.)).unwrap();
        let o = d.wall_outlines()[&w];
        assert_eq!(d.wall_plan_pieces(w, &o, None).len(), 1);
        assert_eq!(d.wall_plan_pieces(w, &o, Some(1200.0)).len(), 3, "both cut at 1200");
        assert_eq!(d.wall_plan_pieces(w, &o, Some(600.0)).len(), 2, "only the door spans 600");
        assert_eq!(d.wall_plan_pieces(w, &o, Some(2200.0)).len(), 1, "both below 2200");
        let pieces = d.wall_plan_pieces(w, &o, Some(1200.0));
        // the first piece ends where the window starts
        let xs: Vec<f64> = pieces[0].iter().map(|q| q.x).collect();
        assert!((xs.iter().cloned().fold(f64::MIN, f64::max) - 900.0).abs() < 1e-6);
    }

    #[test]
    fn wall_mesh_with_openings_has_every_face_pointing_out() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let win = type_named(&d, "Fixed", "1200 x 1200");
        let door = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(win, w, p(1500., 0.)).unwrap();
        d.place_on_wall(door, w, p(4000., 0.)).unwrap();
        let Some(Element::Wall(wall)) = d.get(w).cloned() else { panic!() };
        let o = d.wall_outlines()[&w];
        let m = crate::mesh::wall_mesh(&d, &wall, &o, &d.wall_openings(w)).unwrap();
        assert!(m.len() > 12 * 3 * 6);
        for t in m.chunks(18) {
            let v = |i: usize| [t[i * 6] as f64, t[i * 6 + 1] as f64, t[i * 6 + 2] as f64];
            let (a, b, c) = (v(0), v(1), v(2));
            let (u, w2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * w2[2] - u[2] * w2[1], u[2] * w2[0] - u[0] * w2[2], u[0] * w2[1] - u[1] * w2[0]];
            let s = [t[3] as f64, t[4] as f64, t[5] as f64];
            assert!(n[0] * s[0] + n[1] * s[1] + n[2] * s[2] > 0.0);
        }
        let (cat, frame, glass) = crate::mesh::instance_meshes(&d, d.instances()[0].0).unwrap();
        assert_eq!(cat, FamilyCategory::Windows);
        assert!(!frame.is_empty() && !glass.is_empty());
    }

    #[test]
    fn deleting_a_wall_deletes_what_it_hosts_in_one_undo_step() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(t, w, p(2000., 0.)).unwrap();
        d.delete(&[w]).unwrap();
        assert!(d.instances().is_empty());
        d.undo();
        assert_eq!(d.instances().len(), 1);
        assert_eq!(d.wall_openings(w).len(), 1);
    }

    #[test]
    fn a_wall_edit_that_would_orphan_an_opening_is_refused() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(t, w, p(2000., 0.)).unwrap();
        assert!(d.set_wall_height(w, 1800.).unwrap_err().contains("no longer fit"));
        assert!(d.patch_wall(w, &json!({ "end": { "x": 2200.0, "y": 0.0 } })).is_err());
        assert!(d.set_wall_height(w, 2400.).is_ok());
    }

    #[test]
    fn instance_and_type_edits_go_through_patches() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let win = type_named(&d, "Awning", "900 x 600"); // sill 1500
        let id = d.place_on_wall(win, w, p(3000., 0.)).unwrap();
        d.patch_instance(id, &json!({ "mark": "W01", "flip_hand": true, "values": { "Sill Height": 1200.0 } })).unwrap();
        let o = d.wall_openings(w)[0];
        assert_eq!((o.z0, o.z1), (1200.0, 1800.0));
        assert!(d.patch_instance(id, &json!({ "values": { "Width": 1000.0 } })).unwrap_err().contains("type parameter"));
        assert!(d.patch_instance(id, &json!({ "values": { "Sill Height": 2400.0 } })).is_err(), "head above the wall");
        // widen the type: placed instances follow
        d.patch_family_type(win, &json!({ "values": { "Width": 1500.0, "Rough Width": 1500.0 } })).unwrap();
        let o = d.wall_openings(w)[0];
        assert_eq!(o.u1 - o.u0, 1500.0);
        // a type too wide for the wall is refused
        assert!(d.patch_family_type(win, &json!({ "values": { "Rough Width": 9000.0 } })).is_err());
        // duplicate keeps the values under a new name
        let dup = d.duplicate_family_type(win, "1500 x 600 custom").unwrap();
        assert_eq!(d.family_type(dup).unwrap().values.get("Width"), Some(&ParamValue::Number(1500.0)));
        assert!(d.duplicate_family_type(win, "1500 x 600 custom").is_err());
        d.undo();
        d.undo();
        assert_eq!(d.wall_openings(w)[0].u1 - d.wall_openings(w)[0].u0, 900.0);
    }

    #[test]
    fn plan_symbol_swings_towards_the_cursor_side() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        let up = d.place_on_wall(t, w, p(1000., 300.)).unwrap();
        let down = d.place_on_wall(t, w, p(3000., -300.)).unwrap();
        let (a, b) = (crate::symbol::plan_symbol(&d, up, crate::view::DetailLevel::Medium).unwrap(), crate::symbol::plan_symbol(&d, down, crate::view::DetailLevel::Medium).unwrap());
        // the swing is a Plan Swing part from the closed end round to the open tip; its radius is the leaf
        let swing = |g: &crate::symbol::SymbolGeom| g.parts.iter().filter(|p| p.subcategory == "Plan Swing").map(|p| p.rings[0].clone()).collect::<Vec<_>>();
        let (sa, sb) = (swing(&a), swing(&b));
        assert_eq!((sa.len(), sb.len()), (1, 1));
        let (ra, rb) = (&sa[0], &sb[0]);
        // the wall runs along x, so the swing side is the sign of y away from the pivot (the arc's first point)
        assert!(ra.iter().all(|q| q.y >= ra[0].y - 1e-6) && ra.iter().any(|q| q.y > ra[0].y + 800.0));
        assert!(rb.iter().all(|q| q.y <= rb[0].y + 1e-6) && rb.iter().any(|q| q.y < rb[0].y - 800.0));
        let (first, last) = (ra[0], ra[ra.len() - 1]);
        assert!((first.dist(&last) - 820.0 * 2f64.sqrt()).abs() < 1.0, "a quarter circle of radius 820");
    }

    #[test]
    fn families_survive_save_and_old_files_get_the_starter_set() {
        let mut d = Document::new_project();
        let w = wall(&mut d);
        let t = type_named(&d, "Single-Flush", "820 x 2040");
        d.place_on_wall(t, w, p(2000., 0.)).unwrap();
        let dir = std::env::temp_dir().join(format!("openarch-fam-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.oab");
        d.save(&path).unwrap();
        let o = Document::open(&path).unwrap();
        assert_eq!(o.instances().len(), 1);
        assert_eq!(o.wall_openings(w).len(), 1);
        // an older project: no families at all
        let mut old = Document::new_project();
        let ids: Vec<Id> = old.elements.iter().filter(|(_, e)| matches!(e, Element::Family(_) | Element::FamilyType(_))).map(|(i, _)| *i).collect();
        for i in ids {
            old.elements.remove(&i);
        }
        let p2 = dir.join("old.oab");
        old.save(&p2).unwrap();
        let reopened = Document::open(&p2).unwrap();
        assert!(!reopened.families().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
