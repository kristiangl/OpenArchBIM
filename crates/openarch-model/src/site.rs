//! Where the project sits and how it is looked at from the side: the project base point and
//! survey point, level datums and display elevations, elevation markers and elevation views
//! (Manage > Location, Level, View > Elevation).
//!
//! Heights are stored from the internal origin. What the person sees for a level is measured from
//! the datum the level names (`ElevationBase`): the project base point or the survey point, as in
//! a standard level type. Moving the base point up therefore changes every level's displayed elevation
//! and no geometry.

use serde::{Deserialize, Serialize};

use crate::doc::{apply_patch, Document};
use crate::element::*;
use crate::family::FamilyCategory;
use crate::geom::Point;
use crate::view::*;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// The base point, survey point and shared site origin of a project. One per project; its presence
/// also marks that the default Site plan and elevations have been made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectLocation {
    /// Project Base Point, in internal coordinates (mm). The default is the internal origin.
    #[serde(default)]
    pub base: P3,
    /// Angle between project north and true north, degrees (counter-clockwise positive).
    #[serde(default)]
    pub angle_true_north_deg: f64,
    #[serde(default)]
    pub base_pinned: bool,
    /// Survey Point, in internal coordinates.
    #[serde(default)]
    pub survey: P3,
    /// A clipped survey point travels with the shared site origin.
    #[serde(default = "yes_")]
    pub survey_clipped: bool,
    #[serde(default)]
    pub survey_pinned: bool,
    /// Shared Site Origin, in internal coordinates: the zero of shared coordinates.
    #[serde(default)]
    pub shared_origin: P3,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub latitude_deg: Option<f64>,
    #[serde(default)]
    pub longitude_deg: Option<f64>,
}

fn yes_() -> bool {
    true
}

impl Default for ProjectLocation {
    fn default() -> Self {
        ProjectLocation {
            base: P3::default(),
            angle_true_north_deg: 0.0,
            base_pinned: false,
            survey: P3::default(),
            survey_clipped: true,
            survey_pinned: false,
            shared_origin: P3::default(),
            site_name: String::new(),
            latitude_deg: None,
            longitude_deg: None,
        }
    }
}

/// Which of the two points an edit is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SitePoint {
    Base,
    Survey,
}

/// The direction each pointer of an elevation marker looks, in degrees from east when the marker
/// is not rotated: slot 0 looks south (the North elevation, seen from the north), 1 north (South),
/// 2 west (East), 3 east (West).
pub const SLOT_LOOK_DEG: [f64; 4] = [270.0, 90.0, 180.0, 0.0];
pub const SLOT_NAMES: [&str; 4] = ["North", "South", "East", "West"];

/// An elevation marker in plan: where it stands, how it is turned, and the views of its four
/// pointers. Each view it makes stores its own crop and depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElevationMarker {
    /// The level whose plan it was placed in.
    pub level: Id,
    pub position: Point,
    #[serde(default)]
    pub rotation_deg: f64,
    #[serde(default)]
    pub slots: [Option<Id>; 4],
    #[serde(default)]
    pub pinned: bool,
    /// The annotation family it is drawn with. `None` uses the first elevation marker family.
    #[serde(default)]
    pub family: Option<Id>,
}

impl ElevationMarker {
    /// Unit vector the view in `slot` looks along.
    pub fn look(&self, slot: usize) -> Point {
        let a = (self.rotation_deg + SLOT_LOOK_DEG[slot % 4]).to_radians();
        Point::new(a.cos(), a.sin())
    }

    /// Unit vector pointing to the right on screen of the view in `slot`.
    pub fn right(&self, slot: usize) -> Point {
        let d = self.look(slot);
        Point::new(d.y, -d.x)
    }

    /// The name views of this marker get when it is not a default one ("Elevation 1 - a").
    pub fn view_letter(slot: usize) -> char {
        (b'a' + slot as u8) as char
    }
}

/// The frame of an elevation view: where it looks from, which way, and how far it reaches.
#[derive(Debug, Clone, Serialize)]
pub struct ElevFrame {
    pub origin: [f64; 2],
    pub look: [f64; 2],
    pub right: [f64; 2],
    pub left_mm: f64,
    pub right_mm: f64,
    pub depth_mm: f64,
    pub z_min: f64,
    pub z_max: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelLine {
    pub id: Id,
    pub name: String,
    /// From the internal origin.
    pub z: f64,
    /// What the person sees (from the level's datum).
    pub elev: f64,
    pub bubbles: [bool; 2],
    pub story: bool,
    /// Where the line starts and ends across the view (mm along the view's right axis), when the
    /// level has its own extent; otherwise the view's width.
    pub u0: f64,
    pub u1: f64,
    pub custom_extent: bool,
    /// Which end of the stored extent (0 or 1) is at the left of this view.
    pub end_left: usize,
    /// The ends locked to each stored end of the extent (the alignment pin).
    pub locks: crate::datum::EndLocks,
    /// The head drawn at each end that shows one, with its labels worked out.
    pub head: Option<crate::annotation::DrawnHead>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElevItem {
    pub id: Id,
    pub category: &'static str,
    /// Distance in front of which nothing else is nearer: the painter's order.
    pub depth: f64,
    pub polys: Vec<Vec<[f64; 2]>>,
    pub lines: Vec<[[f64; 2]; 2]>,
    pub fill: [u8; 3],
    /// Whether the outline is drawn. A wall seen end on at a corner is filled only, so the corner
    /// reads as one continuous face.
    #[serde(default = "yes_stroke")]
    pub stroke: bool,
    /// Cut by a section plane: drawn over everything else, with the material's cut pattern.
    pub cut: bool,
    /// The material whose cut pattern fills a cut item.
    pub material: Option<String>,
    /// Openings in a cut item (doors and windows the plane passes through).
    pub holes: Vec<Vec<[f64; 2]>>,
    /// How strongly it is drawn: 1 in full, less as depth cueing fades it toward the background.
    pub fade: f64,
}

impl Default for ElevItem {
    fn default() -> Self {
        ElevItem { id: 0, category: "", depth: 0.0, polys: vec![], lines: vec![], fill: [200, 200, 200], stroke: true, cut: false, material: None, holes: vec![], fade: 1.0 }
    }
}

/// Depth cueing as a view asks for it (Show Depth): where the fade starts and ends, as fractions of the view depth
/// from the cut plane (or the elevation's own plane), and how faint things get at the end (Fade Limit).
#[derive(Debug, Clone, Copy)]
struct Cue {
    start: f64,
    end: f64,
    limit: f64,
    /// The depth the fractions measure, from the plane the view starts at.
    depth: f64,
}

impl Cue {
    /// How strongly to draw something `z` beyond the front plane, when the view reaches `span` beyond it.
    fn fade(&self, z: f64, span: f64) -> f64 {
        let f = if span > 1e-9 { z / span } else { 0.0 };
        let t = if self.end - self.start > 1e-9 { ((f - self.start) / (self.end - self.start)).clamp(0.0, 1.0) } else if f >= self.start { 1.0 } else { 0.0 };
        1.0 - t * (1.0 - self.limit)
    }
}

/// A jogged section draws each run from its own plane, so an element seen from two runs comes in two pieces that
/// meet where the line jogs. The same rectangle at the same depth either side of a jog is put back together, so
/// only the fold line marks the jog.
fn merge_at_seams(items: Vec<ElevItem>, seams: &[f64]) -> Vec<ElevItem> {
    if seams.is_empty() {
        return items;
    }
    // [u0, u1, z0, z1] of an item that is one upright rectangle
    let rect = |it: &ElevItem| -> Option<[f64; 4]> {
        if it.cut || it.polys.len() != 1 || !it.lines.is_empty() || !it.holes.is_empty() || it.polys[0].len() != 4 {
            return None;
        }
        let p = &it.polys[0];
        let (u0, u1) = (p.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min), p.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max));
        let (z0, z1) = (p.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min), p.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max));
        let corner = |q: &[f64; 2]| ((q[0] - u0).abs() < 1e-6 || (q[0] - u1).abs() < 1e-6) && ((q[1] - z0).abs() < 1e-6 || (q[1] - z1).abs() < 1e-6);
        p.iter().all(corner).then_some([u0, u1, z0, z1])
    };
    let mut items = items;
    let mut keep = vec![true; items.len()];
    let mut groups: std::collections::BTreeMap<(Id, u64, u64, u64), Vec<(usize, [f64; 4])>> = Default::default();
    for (k, it) in items.iter().enumerate() {
        if let Some(r) = rect(it) {
            groups.entry((it.id, it.depth.to_bits(), r[2].to_bits(), r[3].to_bits())).or_default().push((k, r));
        }
    }
    for mut g in groups.into_values() {
        g.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
        let (mut cur, mut cr) = g[0];
        for &(n, nr) in &g[1..] {
            let (a, b) = (&items[cur], &items[n]);
            let alike = a.fill == b.fill && a.stroke == b.stroke && a.category == b.category && (a.fade - b.fade).abs() < 1e-6;
            if alike && (cr[1] - nr[0]).abs() < 1e-6 && seams.iter().any(|s| (s - nr[0]).abs() < 1e-6) {
                cr[1] = nr[1];
                items[cur].polys = vec![vec![[cr[0], cr[2]], [cr[1], cr[2]], [cr[1], cr[3]], [cr[0], cr[3]]]];
                keep[n] = false;
            } else {
                (cur, cr) = (n, nr);
            }
        }
    }
    items.into_iter().zip(keep).filter_map(|(it, k)| k.then_some(it)).collect()
}

#[allow(dead_code)]
fn yes_stroke() -> bool {
    true
}

/// Convex hull of 2D points (monotone chain), counter-clockwise.
fn convex_hull_2d(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal).then(a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal)));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::new();
    for &p in &pts {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    let lower = h.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while h.len() >= lower && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
            h.pop();
        }
        h.push(p);
    }
    h.pop();
    h
}

/// A line in an elevation that a temporary dimension can measure from or to: a wall face, the edge of an
/// opening, a sill, a floor edge. It names the line on its element (`key`), so a dimension pinned to it
/// follows the element when it moves.
#[derive(Debug, Clone, Serialize)]
pub struct ElevRef {
    pub id: Id,
    /// Stable name of the line on its element ("w:-45" a wall boundary 45 mm right of its location line, "o:410" an
    /// opening edge, "top", "bottom", "sill", "head", "end:min").
    pub key: String,
    /// What it is called: Face, Core face, Core centre, Centreline, Wall end, Opening edge, Centre, Reveal, Frame edge,
    /// Top, Bottom, Sill, Head, Edge.
    pub label: String,
    /// A line at one place along the view (u), up and down; otherwise one at a height (z).
    pub vertical: bool,
    pub pos: f64,
    /// How far it runs: heights for a vertical line, places across the view for a horizontal one.
    pub lo: f64,
    pub hi: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElevationData {
    pub view_id: Id,
    pub name: String,
    pub frame: ElevFrame,
    /// The view scale denominator (1:100 is 100): annotation is drawn at its paper size at this scale.
    pub scale: f64,
    pub levels: Vec<LevelLine>,
    pub grids: Vec<GridLine>,
    pub items: Vec<ElevItem>,
    /// Reference lines for temporary dimensions.
    pub refs: Vec<ElevRef>,
    /// How the view is drawn (from its settings or its template).
    pub visual_style: VisualStyle,
    /// Where a jogged section's line jogs (across the view, mm): the view draws a fold line there.
    pub folds: Vec<f64>,
}

/// A grid seen end on in an elevation: a vertical line with its head at the top.
#[derive(Debug, Clone, Serialize)]
pub struct GridLine {
    pub id: Id,
    pub name: String,
    pub u: f64,
    pub z0: f64,
    pub z1: f64,
    pub head: bool,
    pub pinned: bool,
}

impl Document {
    pub fn project_location(&self) -> Option<(Id, &ProjectLocation)> {
        self.iter().find_map(|(id, e)| if let Element::ProjectLocation(l) = e { Some((id, l)) } else { None })
    }

    pub fn elevation_markers(&self) -> Vec<(Id, &ElevationMarker)> {
        self.iter().filter_map(|(id, e)| if let Element::ElevationMarker(m) = e { Some((id, m)) } else { None }).collect()
    }

    pub fn elevation_marker(&self, id: Id) -> Option<&ElevationMarker> {
        match self.get(id) {
            Some(Element::ElevationMarker(m)) => Some(m),
            _ => None,
        }
    }

    // ---- level datum ----------------------------------------------------------------

    /// Height of the datum a level's elevation is read from, from the internal origin.
    pub fn level_datum_z(&self, l: &Level) -> f64 {
        let Some((_, loc)) = self.project_location() else { return 0.0 };
        match l.elevation_base {
            ElevationBase::ProjectBasePoint => loc.base.z,
            ElevationBase::SurveyPoint => loc.shared_origin.z,
        }
    }

    /// The elevation shown for a level.
    pub fn level_display_elevation(&self, l: &Level) -> f64 {
        l.elevation_mm - self.level_datum_z(l)
    }

    // ---- levels ---------------------------------------------------------------------

    fn level_name_taken(&self, name: &str, except: Option<Id>) -> bool {
        self.levels().iter().any(|(id, l)| l.name == name && Some(*id) != except)
    }

    /// Add a level at `display_elevation` (as shown, from the base point) and by
    /// default, a floor plan for it.
    pub fn add_level(&mut self, name: Option<&str>, display_elevation: f64, make_plan: bool) -> Result<Id, String> {
        if !display_elevation.is_finite() {
            return Err("the elevation must be a number".into());
        }
        let mut l = Level::new("", 0.0);
        let datum = self.level_datum_z(&l);
        l.elevation_mm = display_elevation + datum;
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => {
                let mut k = self.levels().len() + 1;
                while self.level_name_taken(&format!("Level {k}"), None) {
                    k += 1;
                }
                format!("Level {k}")
            }
        };
        if self.level_name_taken(&name, None) {
            return Err(format!("a level named '{name}' already exists"));
        }
        l.name = name.clone();
        if let Some((tid, t)) = self.level_types().into_iter().next() {
            l.level_type = Some(tid);
            l.bubbles = t.end_default;
        }
        self.transact("Create level", |tx| {
            let id = tx.add(Element::Level(l));
            if make_plan {
                let mut v = View::floor_plan(&name, id);
                v.view_type = tx.doc_ref().view_types().into_iter().find(|(_, t)| t.view_kind == ViewKind::FloorPlan).map(|(t, _)| t);
                tx.add(Element::View(v));
            }
            Ok(id)
        })
    }

    /// Everything that stands on or hangs from a level: walls (base or top), floors, openings.
    pub fn level_users(&self, level: Id) -> Vec<Id> {
        let mut out = vec![];
        for (id, e) in self.iter() {
            let uses = match e {
                Element::Wall(w) => w.base_level == level || w.top_level == Some(level),
                Element::Floor(f) => f.level == level,
                Element::Roof(r) => r.level == level || r.cutoff_level == Some(level),
                Element::FamilyInstance(i) => i.level == level,
                Element::ElevationMarker(m) => m.level == level,
                Element::Section(s) => s.level == level,
                _ => false,
            };
            if uses {
                out.push(id);
            }
        }
        out
    }

    /// Change a level's properties with a JSON patch. `display_elevation_mm` is the elevation as
    /// shown (from the level's datum); the rest are the stored fields. Plans named after the level
    /// are renamed with it. One undo step; refused if a door or window would no longer fit.
    pub fn patch_level(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let Some(old) = self.level(id).cloned() else { return Err("not a level".into()) };
        let mut patch = patch.clone();
        let display = patch.as_object_mut().and_then(|o| o.remove("display_elevation_mm"));
        if let Some(o) = patch.as_object_mut() {
            o.remove("end_locks"); // locks change only through lock_datum_ends and unlock_datum_ends
        }
        let mut new: Level = apply_patch(&old, &patch)?;
        if let Some(v) = display.and_then(|v| v.as_f64()) {
            if !v.is_finite() {
                return Err("the elevation must be a number".into());
            }
            new.elevation_mm = v + self.level_datum_z(&new);
        } else if new.elevation_base != old.elevation_base {
            // switching the datum keeps the level where it is
        }
        if new == old {
            return Ok(());
        }
        if old.pinned && new.pinned && (new.elevation_mm != old.elevation_mm || new.name != old.name || new.extent != old.extent) {
            return Err("the level is pinned; unpin it first".into());
        }
        if new.level_type.is_some_and(|t| self.level_type(t).is_none()) {
            return Err("unknown level type".into());
        }
        if new.name.trim().is_empty() {
            return Err("a level needs a name".into());
        }
        if self.level_name_taken(&new.name, Some(id)) {
            return Err(format!("a level named '{}' already exists", new.name));
        }
        let plans: Vec<(Id, View)> = self
            .views()
            .into_iter()
            .filter(|(_, v)| matches!(v.view_kind, ViewKind::FloorPlan | ViewKind::CeilingPlan) && v.level == Some(id) && v.name == old.name)
            .map(|(i, v)| (i, v.clone()))
            .collect();
        let moved = (new.elevation_mm - old.elevation_mm).abs() > 1e-9;
        self.transact("Change level", |tx| {
            tx.update(id, Element::Level(new.clone()))?;
            if new.name != old.name {
                for (vid, v) in plans {
                    tx.update(vid, Element::View(View { name: new.name.clone(), ..v }))?;
                }
            }
            if moved {
                let doc = tx.doc_ref();
                for (iid, i) in doc.instances() {
                    doc.fits(i, Some(iid)).map_err(|e| format!("a door or window would not fit: {e}"))?;
                }
            }
            Ok(())
        })
    }

    /// Delete a level and its plans. Refused while anything stands on it or hangs from it.
    pub fn delete_level(&mut self, id: Id) -> Result<(), String> {
        let Some(l) = self.level(id).cloned() else { return Err("not a level".into()) };
        if l.pinned {
            return Err(format!("level '{}' is pinned; unpin it first", l.name));
        }
        if self.levels().len() <= 1 {
            return Err("a project needs at least one level".into());
        }
        let users = self.level_users(id);
        if !users.is_empty() {
            return Err(format!("{} element{} use level '{}'; move or delete them first", users.len(), if users.len() == 1 { "" } else { "s" }, l.name));
        }
        let plans: Vec<Id> = self.views().into_iter().filter(|(_, v)| v.level == Some(id)).map(|(i, _)| i).collect();
        self.transact("Delete level", |tx| {
            for p in plans {
                tx.remove(p)?;
            }
            tx.remove(id)
        })
    }

    // ---- project base point and survey point ------------------------------------------

    /// Move the base point or the survey point to `to` (internal coordinates). A clipped survey
    /// point takes the shared site origin with it. Pinned points refuse.
    pub fn move_site_point(&mut self, which: SitePoint, to: P3) -> Result<(), String> {
        let Some((id, loc)) = self.project_location().map(|(i, l)| (i, l.clone())) else { return Err("the project has no location".into()) };
        let mut n = loc.clone();
        match which {
            SitePoint::Base => {
                if loc.base_pinned {
                    return Err("the project base point is pinned; unpin it first".into());
                }
                n.base = to;
            }
            SitePoint::Survey => {
                if loc.survey_pinned {
                    return Err("the survey point is pinned; unpin it first".into());
                }
                n.survey = to;
                if loc.survey_clipped {
                    n.shared_origin = to;
                }
            }
        }
        if n == loc {
            return Ok(());
        }
        self.transact(if which == SitePoint::Base { "Move project base point" } else { "Move survey point" }, |tx| tx.update(id, Element::ProjectLocation(n)))
    }

    /// Patch the project location (angle to true north, pins, clip, site name, position).
    pub fn patch_project_location(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let Some((id, loc)) = self.project_location().map(|(i, l)| (i, l.clone())) else { return Err("the project has no location".into()) };
        let n: ProjectLocation = apply_patch(&loc, patch)?;
        if n == loc {
            return Ok(());
        }
        self.transact("Change project location", |tx| tx.update(id, Element::ProjectLocation(n)))
    }

    /// The same patch as the properties palette sends it. `base_shared_x/y/z` are the shared
    /// coordinates the base point should read: the base point stays put and the shared site origin
    /// moves (a clipped survey point goes with it). `survey_shared_x/y/z` move an unclipped survey
    /// point. Everything else is a plain field patch.
    pub fn patch_project_location_ui(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let Some(obj) = patch.as_object() else { return Err("the patch must be an object".into()) };
        let Some((id, loc)) = self.project_location().map(|(i, l)| (i, l.clone())) else { return Err("the project has no location".into()) };
        let mut n = loc.clone();
        let mut plain = serde_json::Map::new();
        for (k, v) in obj {
            let num = || v.as_f64().ok_or_else(|| format!("{k} must be a number"));
            match k.as_str() {
                "base_shared_x" | "base_shared_y" | "base_shared_z" => {
                    if loc.base_pinned {
                        return Err("the project base point is pinned; unpin it first".into());
                    }
                    let t = num()?;
                    match k.as_str() {
                        "base_shared_x" => n.shared_origin.x = n.base.x - t,
                        "base_shared_y" => n.shared_origin.y = n.base.y - t,
                        _ => n.shared_origin.z = n.base.z - t,
                    }
                }
                "survey_shared_x" | "survey_shared_y" | "survey_shared_z" => {
                    if loc.survey_clipped {
                        return Err("a clipped survey point cannot be moved; unclip it first".into());
                    }
                    if loc.survey_pinned {
                        return Err("the survey point is pinned; unpin it first".into());
                    }
                    let t = num()?;
                    match k.as_str() {
                        "survey_shared_x" => n.survey.x = n.shared_origin.x + t,
                        "survey_shared_y" => n.survey.y = n.shared_origin.y + t,
                        _ => n.survey.z = n.shared_origin.z + t,
                    }
                }
                _ => {
                    plain.insert(k.clone(), v.clone());
                }
            }
        }
        if !plain.is_empty() {
            n = apply_patch(&n, &serde_json::Value::Object(plain))?;
        }
        // Clipping a survey point snaps the shared origin to it.
        if n.survey_clipped && !loc.survey_clipped {
            n.shared_origin = n.survey;
        }
        if n == loc {
            return Ok(());
        }
        self.transact("Change project location", |tx| tx.update(id, Element::ProjectLocation(n)))
    }

    /// Shared coordinates of a point: east, north and elevation from the shared site origin.
    pub fn shared_coordinates(&self, p: P3) -> P3 {
        let o = self.project_location().map_or(P3::default(), |(_, l)| l.shared_origin);
        P3 { x: p.x - o.x, y: p.y - o.y, z: p.z - o.z }
    }

    // ---- defaults -------------------------------------------------------------------

    /// Make the project location, the Site plan and the four default elevations if the project has
    /// no location yet (new projects, and projects saved before these existed). The markers stand
    /// round whatever is already modelled.
    pub fn ensure_site_defaults(&mut self) {
        if self.project_location().is_some() {
            return;
        }
        let Some(level) = self.levels().first().map(|(id, _)| *id) else { return };
        let b = self.model_bounds();
        let (cx, cy, hx, hy) = match b {
            Some((lo, hi)) => ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, (hi.x - lo.x) / 2.0, (hi.y - lo.y) / 2.0),
            None => (0.0, 0.0, 6000.0, 6000.0),
        };
        let margin = 5000.0;
        let spots = [Point::new(cx, cy + hy + margin), Point::new(cx, cy - hy - margin), Point::new(cx + hx + margin, cy), Point::new(cx - hx - margin, cy)];
        let _ = self.transact("Project defaults", |tx| {
            tx.add(Element::ProjectLocation(ProjectLocation::default()));
            let mut site = View::floor_plan("Site", level);
            site.settings.scale = 500;
            site.settings.detail_level = DetailLevel::Coarse;
            site.settings.orientation = Orientation::TrueNorth;
            site.show_site_points();
            tx.add(Element::View(site));
            for (slot, pos) in spots.into_iter().enumerate() {
                let marker = tx.add(Element::ElevationMarker(ElevationMarker { level, position: pos, rotation_deg: 0.0, slots: [None; 4], pinned: false, family: None }));
                let view = tx.add(Element::View(View::elevation(SLOT_NAMES[slot], marker, slot as u8)));
                let mut m = ElevationMarker { level, position: pos, rotation_deg: 0.0, slots: [None; 4], pinned: false, family: None };
                m.slots[slot] = Some(view);
                tx.update(marker, Element::ElevationMarker(m))?;
            }
            Ok(())
        });
        self.clear_history();
    }

    /// Projects saved before the visual style was drawn: elevation and 3D views take Hidden Line (standard linework). Run once,
    /// when a project of an older format opens, so a style chosen afterwards is kept.
    /// Projects saved before format 3 gave every new plan a pale wash over floors. Where a plan or a template still
    /// has exactly that override it goes, so floors show their own surface pattern (or none).
    pub fn clear_old_floor_wash(&mut self) {
        let wash = View::old_floor_wash();
        let views: Vec<(Id, View)> = self.views().into_iter().filter(|(_, v)| v.settings.overrides.model.get("Floors") == Some(&wash)).map(|(id, v)| (id, v.clone())).collect();
        let templates: Vec<(Id, crate::view::ViewTemplate)> = self.view_templates().into_iter().filter(|(_, t)| t.settings.overrides.model.get("Floors") == Some(&wash)).map(|(id, t)| (id, t.clone())).collect();
        if views.is_empty() && templates.is_empty() {
            return;
        }
        let _ = self.transact("Clear the floor wash in plans", |tx| {
            for (id, mut v) in views {
                v.settings.overrides.model.remove("Floors");
                tx.update(id, Element::View(v))?;
            }
            for (id, mut t) in templates {
                t.settings.overrides.model.remove("Floors");
                tx.update(id, Element::ViewTemplate(t))?;
            }
            Ok(())
        });
    }

    pub fn use_hidden_line_in_elevations_and_3d(&mut self) {
        let todo: Vec<(Id, View)> = self
            .views()
            .into_iter()
            .filter(|(_, v)| matches!(v.view_kind, ViewKind::Elevation | ViewKind::ThreeD) && v.settings.visual_style != VisualStyle::HiddenLine)
            .map(|(id, v)| (id, v.clone()))
            .collect();
        if todo.is_empty() {
            return;
        }
        let _ = self.transact("Hidden Line in elevations and 3D", |tx| {
            for (id, mut v) in todo {
                v.settings.visual_style = VisualStyle::HiddenLine;
                tx.update(id, Element::View(v))?;
            }
            Ok(())
        });
        self.clear_history();
    }

    /// Projects saved before the base and survey points had categories: every view gets an explicit setting for
    /// each, hidden except in the Site plan. A view that already has a setting is left alone, so this runs once.
    pub fn ensure_site_point_overrides(&mut self) {
        let todo: Vec<(Id, View)> = self
            .views()
            .into_iter()
            .filter(|(_, v)| SITE_POINT_CATEGORIES.iter().any(|c| !v.settings.overrides.model.contains_key(*c)))
            .map(|(id, v)| (id, v.clone()))
            .collect();
        if todo.is_empty() {
            return;
        }
        let _ = self.transact("Site point visibility", |tx| {
            for (id, mut v) in todo {
                let site = v.name == "Site" && v.view_kind == ViewKind::FloorPlan;
                for cat in SITE_POINT_CATEGORIES {
                    if !v.settings.overrides.model.contains_key(cat) {
                        v.settings.overrides.model.insert(cat.into(), CategoryOverride { visible: site, ..CategoryOverride::default() });
                    }
                }
                tx.update(id, Element::View(v))?;
            }
            Ok(())
        });
        self.clear_history();
    }

    /// The box round the walls and floors of the model, if there are any.
    pub fn model_bounds(&self) -> Option<(Point, Point)> {
        let mut pts: Vec<Point> = vec![];
        for (_, e) in self.iter() {
            match e {
                Element::Wall(w) => pts.extend(w.path().pts.iter().copied()),
                Element::Floor(f) => {
                    for l in f.loops() {
                        pts.extend(crate::curve::loop_polygon(&l, 50.0));
                    }
                }
                Element::Roof(r) => {
                    for l in r.loops() {
                        let curves: Vec<crate::curve::Curve> = l.iter().map(|e| e.curve.clone()).collect();
                        pts.extend(crate::curve::loop_polygon(&curves, 50.0));
                    }
                }
                _ => {}
            }
        }
        if pts.is_empty() {
            return None;
        }
        let lo = Point::new(pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min));
        let hi = Point::new(pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max));
        Some((lo, hi))
    }

    // ---- elevation markers and views -------------------------------------------------------

    fn marker_number(&self) -> usize {
        // names like "Elevation 3 - a" count up from the markers that are not the defaults
        let mut k = 1;
        loop {
            let probe = format!("Elevation {k} - a");
            if !self.views().iter().any(|(_, v)| v.name.starts_with(&format!("Elevation {k} - "))) && !self.views().iter().any(|(_, v)| v.name == probe) {
                return k;
            }
            k += 1;
        }
    }

    /// Place an elevation marker in the plan of `level` with views for the pointers in `slots`.
    pub fn add_elevation_marker(&mut self, level: Id, position: Point, rotation_deg: f64, slots: [bool; 4]) -> Result<Id, String> {
        if self.level(level).is_none() {
            return Err("unknown level".into());
        }
        if !slots.iter().any(|s| *s) {
            return Err("an elevation marker needs at least one view".into());
        }
        let k = self.marker_number();
        self.transact("Create elevation marker", |tx| {
            let marker = tx.add(Element::ElevationMarker(ElevationMarker { level, position, rotation_deg, slots: [None; 4], pinned: false, family: None }));
            let mut m = ElevationMarker { level, position, rotation_deg, slots: [None; 4], pinned: false, family: None };
            for (slot, on) in slots.iter().enumerate() {
                if *on {
                    let name = format!("Elevation {k} - {}", ElevationMarker::view_letter(slot));
                    m.slots[slot] = Some(tx.add(Element::View(View::elevation(&name, marker, slot as u8))));
                }
            }
            tx.update(marker, Element::ElevationMarker(m))?;
            Ok(marker)
        })
    }

    /// Turn the pointer `slot` of a marker on (making its view) or off (deleting the view). A
    /// marker whose last view goes is removed with it.
    pub fn set_marker_slot(&mut self, marker: Id, slot: usize, on: bool) -> Result<(), String> {
        let Some(m) = self.elevation_marker(marker).cloned() else { return Err("not an elevation marker".into()) };
        if slot > 3 {
            return Err("a marker has four pointers".into());
        }
        if m.slots[slot].is_some() == on {
            return Ok(());
        }
        if m.pinned {
            return Err("the marker is pinned; unpin it first".into());
        }
        let existing_name = m.slots.iter().flatten().filter_map(|v| self.view(*v)).map(|v| v.name.clone()).next();
        let base = existing_name.as_deref().and_then(|n| n.rsplit_once(" - ").map(|(a, _)| a.to_string()));
        let default_names = SLOT_NAMES.contains(&existing_name.as_deref().unwrap_or(""));
        let k = self.marker_number();
        self.transact("Change elevation marker", |tx| {
            let mut nm = m.clone();
            if on {
                let name = if default_names && m.rotation_deg == 0.0 { SLOT_NAMES[slot].to_string() } else { format!("{} - {}", base.clone().unwrap_or(format!("Elevation {k}")), ElevationMarker::view_letter(slot)) };
                let name = {
                    let taken = |n: &str| tx.doc_ref().views().iter().any(|(_, v)| v.name == n);
                    let mut k = 1;
                    let mut cand = name.clone();
                    while taken(&cand) {
                        k += 1;
                        cand = format!("{name} ({k})");
                    }
                    cand
                };
                nm.slots[slot] = Some(tx.add(Element::View(View::elevation(&name, marker, slot as u8))));
                tx.update(marker, Element::ElevationMarker(nm))?;
            } else {
                let view = m.slots[slot].unwrap();
                nm.slots[slot] = None;
                tx.remove(view)?;
                if nm.slots.iter().all(|s| s.is_none()) {
                    tx.remove(marker)?;
                } else {
                    tx.update(marker, Element::ElevationMarker(nm))?;
                }
            }
            Ok(())
        })
    }

    /// Delete a marker with all its views.
    pub fn delete_marker(&mut self, marker: Id) -> Result<(), String> {
        let Some(m) = self.elevation_marker(marker).cloned() else { return Err("not an elevation marker".into()) };
        if m.pinned {
            return Err("the marker is pinned; unpin it first".into());
        }
        self.transact("Delete elevation marker", |tx| {
            for v in m.slots.iter().flatten() {
                tx.remove(*v)?;
            }
            tx.remove(marker)
        })
    }

    /// Delete anything the Delete command can be given: levels, markers and elevation views as well
    /// as the elements `delete` knows. One undo step.
    pub fn delete_items(&mut self, ids: &[Id]) -> Result<(), String> {
        self.refuse_pinned(ids, "deleted")?;
        self.grouped("Delete", |d| {
            let mut rest = vec![];
            for &id in ids {
                match d.get(id).cloned() {
                    Some(Element::Level(_)) => d.delete_level(id)?,
                    Some(Element::ElevationMarker(_)) => {
                        if d.elevation_marker(id).is_some() {
                            d.delete_marker(id)?
                        }
                    }
                    Some(Element::View(v)) if v.elevation.is_some() => {
                        let def = v.elevation.unwrap();
                        // the marker may already be gone with an earlier id in the same delete
                        if d.elevation_marker(def.marker).is_some() {
                            d.set_marker_slot(def.marker, def.slot as usize, false)?;
                        }
                    }
                    Some(Element::Section(_)) => d.delete_section(id)?,
                    Some(Element::View(v)) if v.section.is_some() => {
                        let sid = v.section.unwrap();
                        if d.section(sid).is_some() {
                            d.delete_section(sid)?;
                        }
                    }
                    Some(Element::ProjectLocation(_)) => return Err("the project location cannot be deleted".into()),
                    _ => rest.push(id),
                }
            }
            if !rest.is_empty() {
                d.delete(&rest)?;
            }
            Ok(())
        })
    }

    /// Move or turn an elevation marker. Its views follow, since they look from where it stands.
    pub fn set_marker_pose(&mut self, marker: Id, position: Point, rotation_deg: f64) -> Result<(), String> {
        let Some(m) = self.elevation_marker(marker).cloned() else { return Err("not an elevation marker".into()) };
        if m.pinned {
            return Err("the marker is pinned; unpin it first".into());
        }
        let n = ElevationMarker { position, rotation_deg: rotation_deg.rem_euclid(360.0), ..m.clone() };
        if n == m {
            return Ok(());
        }
        self.transact("Move elevation marker", |tx| tx.update(marker, Element::ElevationMarker(n)))
    }

    // ---- elevation drawing -----------------------------------------------------------------

    /// The lowest and highest heights of the levels, walls and roofs (mm): what elevations and sections fit their height to.
    pub fn model_z_extent(&self) -> (f64, f64) {
        let (mut z0, mut z1) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, lv) in self.levels() {
            z0 = z0.min(lv.elevation_mm);
            z1 = z1.max(lv.elevation_mm);
        }
        for (_, e) in self.iter() {
            if let Element::Wall(w) = e {
                let (a, t) = self.wall_extent(w);
                z0 = z0.min(a);
                z1 = z1.max(t);
            }
            if let Element::Roof(r) = e {
                if let Some(g) = self.roof_geometry(r) {
                    z1 = z1.max(g.top_z);
                }
            }
        }
        if !z0.is_finite() {
            return (0.0, 3000.0);
        }
        (z0, z1)
    }

    /// Frame of an elevation view, with auto-fitted crop and depth filled in.
    pub fn elevation_frame(&self, view: Id) -> Option<ElevFrame> {
        let v = self.view(view)?;
        if v.section.is_some() {
            return self.section_frame(view);
        }
        let def = v.elevation.as_ref()?;
        let m = self.elevation_marker(def.marker)?;
        let (look, right) = (m.look(def.slot as usize), m.right(def.slot as usize));
        let along = |p: Point, d: Point| (p.x - m.position.x) * d.x + (p.y - m.position.y) * d.y;
        let b = self.model_bounds();
        let corners = |lo: Point, hi: Point| [Point::new(lo.x, lo.y), Point::new(hi.x, lo.y), Point::new(hi.x, hi.y), Point::new(lo.x, hi.y)];
        let (mut l, mut r) = def.crop.unwrap_or_else(|| match b {
            Some((lo, hi)) => {
                let us: Vec<f64> = corners(lo, hi).iter().map(|p| along(*p, right)).collect();
                (-(us.iter().cloned().fold(f64::INFINITY, f64::min)) + 2000.0, us.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 2000.0)
            }
            None => (10000.0, 10000.0),
        });
        // `l` is how far it reaches to the left of the marker, `r` to the right
        l = l.max(1000.0);
        r = r.max(1000.0);
        let depth = def.far_clip_mm.unwrap_or_else(|| match b {
            Some((lo, hi)) => (corners(lo, hi).iter().map(|p| along(*p, look)).fold(f64::NEG_INFINITY, f64::max) + 2000.0).max(5000.0),
            None => 15000.0,
        });
        let (z0, z1) = self.model_z_extent();
        Some(ElevFrame { origin: [m.position.x, m.position.y], look: [look.x, look.y], right: [right.x, right.y], left_mm: l, right_mm: r, depth_mm: depth, z_min: z0 - 1500.0, z_max: z1 + 1500.0 })
    }

    /// The lines temporary dimensions in an elevation can use (see `ElevRef`): the boundaries of walls seen end on,
    /// the ends and top and bottom of every wall, the edges, sills and heads of openings and the edges of floors.
    fn elevation_refs(
        &self,
        frame: &ElevFrame,
        wall_pieces: &std::collections::BTreeMap<Id, Vec<Vec<Point>>>,
        host_near: &std::collections::BTreeMap<Id, f64>,
        hidden: &dyn Fn(Id) -> bool,
    ) -> Vec<ElevRef> {
        let o = Point::new(frame.origin[0], frame.origin[1]);
        let (look, right) = (Point::new(frame.look[0], frame.look[1]), Point::new(frame.right[0], frame.right[1]));
        let u_of = |p: Point| (p.x - o.x) * right.x + (p.y - o.y) * right.y;
        let d_of = |p: Point| (p.x - o.x) * look.x + (p.y - o.y) * look.y;
        let mut out: Vec<ElevRef> = vec![];
        let mut add = |id: Id, key: String, label: &str, vertical: bool, pos: f64, lo: f64, hi: f64| out.push(ElevRef { id, key, label: label.to_string(), vertical, pos, lo, hi });
        for (id, pieces) in wall_pieces {
            let Some(Element::Wall(w)) = self.get(*id) else { continue };
            if !host_near.contains_key(id) {
                continue;
            }
            let (z0, z1) = self.wall_extent(w);
            let us: Vec<f64> = pieces.iter().flatten().map(|p| u_of(*p)).collect();
            if us.is_empty() {
                continue;
            }
            let (u0, u1) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            let len = w.start.dist(&w.end);
            let edge_on = w.curve.is_none() && len > 1e-6 && ((w.end.x - w.start.x) * right.x + (w.end.y - w.start.y) * right.y).abs() / len < 0.2;
            if edge_on {
                // seen end on: its faces, layer boundaries and centre are lines up the view
                let (dx, dy) = ((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
                let mid = Point::new((w.start.x + w.end.x) / 2.0, (w.start.y + w.end.y) / 2.0);
                for (v, label) in self.wall_boundaries(w) {
                    let p = Point::new(mid.x - dy * v, mid.y + dx * v);
                    add(*id, format!("w:{:.1}", v), &label, true, u_of(p), z0, z1);
                }
            } else {
                add(*id, "end:min".into(), "Wall end", true, u0, z0, z1);
                add(*id, "end:max".into(), "Wall end", true, u1, z0, z1);
            }
            add(*id, "top".into(), "Top", false, z1, u0, u1);
            add(*id, "bottom".into(), "Bottom", false, z0, u0, u1);
        }
        for (iid, i) in self.instances() {
            if hidden(iid) {
                continue;
            }
            let Some(h) = i.host else { continue };
            if !wall_pieces.contains_key(&h.wall) {
                continue;
            }
            let Some(f) = self.instance_frame(iid) else { continue };
            let wproj = f.dir.x * right.x + f.dir.y * right.y;
            if (f.width * wproj.abs() / 2.0) < 15.0 {
                continue;
            }
            let dc = d_of(f.origin);
            if dc <= 0.0 || dc >= frame.depth_mm {
                continue;
            }
            let uc = u_of(f.origin) + wproj * f.x_off;
            for (v, label) in self.opening_offsets(iid) {
                add(iid, format!("o:{:.1}", v), &label, true, uc + wproj * v, f.z0, f.z1);
            }
            let (ow, _, _) = match self.get(iid) {
                Some(Element::FamilyInstance(fi)) => self.instance_opening_size(fi),
                _ => (f.width, f.height, 0.0),
            };
            let half = ow * wproj.abs() / 2.0;
            add(iid, "sill".into(), "Sill", false, f.z0, uc - half, uc + half);
            add(iid, "head".into(), "Head", false, f.z1, uc - half, uc + half);
        }
        for (id, e) in self.iter() {
            let Element::Floor(fl) = e else { continue };
            if hidden(id) {
                continue;
            }
            let Some(lv) = self.level(fl.level) else { continue };
            let thick = self.floor_type(fl.floor_type).map_or(200.0, |t| t.thickness_mm());
            let top = lv.elevation_mm + fl.offset_mm;
            let loops: Vec<Vec<Point>> = fl.loops().iter().map(|l| crate::curve::loop_polygon(l, 20.0)).collect();
            let depths = crate::curve::loop_depths(&loops);
            let mut ends: Option<(f64, f64)> = None;
            for (poly, depth) in loops.iter().zip(depths) {
                if depth != 0 || poly.len() < 3 {
                    continue;
                }
                let ds: Vec<f64> = poly.iter().map(|p| d_of(*p)).collect();
                if ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max) <= 0.0 || ds.iter().cloned().fold(f64::INFINITY, f64::min) >= frame.depth_mm {
                    continue;
                }
                let us: Vec<f64> = poly.iter().map(|p| u_of(*p)).collect();
                let (a, b) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                ends = Some(ends.map_or((a, b), |(x, y)| (x.min(a), y.max(b))));
            }
            if let Some((a, b)) = ends {
                add(id, "end:min".into(), "Edge", true, a, top - thick, top);
                add(id, "end:max".into(), "Edge", true, b, top - thick, top);
                add(id, "top".into(), "Top", false, top, a, b);
                add(id, "bottom".into(), "Bottom", false, top - thick, a, b);
            }
        }
        out
    }

    /// What an elevation view shows: level lines, and the walls, openings and floors in front of
    /// the clip plane, back to front.
    pub fn elevation_data(&self, view: Id) -> Result<ElevationData, String> {
        let v = self.view(view).ok_or("not a view")?;
        let frame = self.elevation_frame(view).ok_or("not an elevation view")?;
        let section = self.section_of_view(view).map(|(_, s)| s);
        let o = Point::new(frame.origin[0], frame.origin[1]);
        let (look, right) = (Point::new(frame.look[0], frame.look[1]), Point::new(frame.right[0], frame.right[1]));
        let u_of = |p: Point| (p.x - o.x) * right.x + (p.y - o.y) * right.y;
        let d_of = |p: Point| (p.x - o.x) * look.x + (p.y - o.y) * look.y;
        let levels: Vec<LevelLine> = self
            .levels()
            .into_iter()
            .map(|(id, l)| {
                let ext = l.extent.map(|[a, b]| (u_of(a), u_of(b)));
                let (u0, u1) = ext.map_or((-frame.left_mm, frame.right_mm), |(a, b)| (a.min(b), a.max(b)));
                LevelLine { id, name: l.name.clone(), z: l.elevation_mm, elev: self.level_display_elevation(l), bubbles: l.bubbles, story: l.building_story, u0, u1, custom_extent: ext.is_some(), end_left: ext.map_or(0, |(a, b)| if a <= b { 0 } else { 1 }), locks: l.end_locks.clone(), head: self.drawn_head(l) }
            })
            .collect();
        let settings = self.effective_settings_of(v);
        let gd = &settings.graphic_display;
        let cue = (gd.depth_cue && matches!(settings.discipline, crate::view::Discipline::Architectural | crate::view::Discipline::Coordination)).then(|| {
            // a section that does not clip fades over the depth of the model behind it
            let depth = match section {
                Some(s) if s.far_clip == crate::section::FarClip::NoClip => self.section_fit_depth(s),
                _ => frame.depth_mm,
            };
            Cue { start: gd.depth_cue_start as f64 / 100.0, end: gd.depth_cue_end as f64 / 100.0, limit: gd.depth_cue_fade as f64 / 100.0, depth }
        });
        let hidden = |id: Id| v.hidden_elements.contains(&id);
        // A section view is drawn run by run: each run of the line cuts at its own plane and shows what lies beyond
        // it across its own stretch of the view.
        let (mut items, wall_pieces, host_near, folds) = match self.section_slices(view) {
            Some(slices) => {
                let mut all = vec![];
                for s in &slices {
                    all.extend(self.elevation_items(v, &frame, Some(s), cue).0);
                }
                let seams: Vec<f64> = slices.iter().skip(1).map(|s| s.lo).collect();
                let folds = if section.is_some_and(|s| s.fold_lines) { seams.clone() } else { vec![] };
                (merge_at_seams(all, &seams), Default::default(), Default::default(), folds)
            }
            None => {
                let (i, w, h) = self.elevation_items(v, &frame, None, cue);
                (i, w, h, vec![])
            }
        };
        items.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        // grids that run away from the viewer show as vertical lines (those across the view are not drawn)
        let mut grids: Vec<GridLine> = vec![];
        for (gid, g) in self.grids() {
            if hidden(gid) {
                continue;
            }
            let Some((a, b)) = g.line() else { continue };
            let len = a.dist(&b);
            let across = ((b.x - a.x) * right.x + (b.y - a.y) * right.y).abs() / len.max(1e-9);
            if across > 0.15 {
                continue;
            }
            let (da, db) = (d_of(a), d_of(b));
            if da.max(db) <= 0.0 || da.min(db) >= frame.depth_mm {
                continue;
            }
            let u = u_of(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0));
            if section.is_some() && (u < 0.0 || u > frame.right_mm) {
                continue;
            }
            grids.push(GridLine { id: gid, name: g.name.clone(), u, z0: frame.z_min, z1: frame.z_max, head: g.bubbles[0] || g.bubbles[1], pinned: g.pinned });
        }
        let refs = if section.is_some() { vec![] } else { self.elevation_refs(&frame, &wall_pieces, &host_near, &hidden) };
        Ok(ElevationData { view_id: view, name: v.name.clone(), frame, scale: v.settings.scale as f64, levels, grids, items, refs, visual_style: settings.visual_style, folds })
    }

    /// The model drawn in an elevation, or in one run of a section (`sl`): walls, openings, families, floors and roofs
    /// as seen, then for a section what its plane cuts. Also the plan pieces of the walls and how near each wall
    /// comes, for the reference lines of temporary dimensions.
    #[allow(clippy::type_complexity)]
    fn elevation_items(&self, v: &View, frame: &ElevFrame, sl: Option<&crate::section::Slice>, cue: Option<Cue>) -> (Vec<ElevItem>, std::collections::BTreeMap<Id, Vec<Vec<Point>>>, std::collections::BTreeMap<Id, f64>) {
        let o = Point::new(frame.origin[0], frame.origin[1]);
        let (look, right) = (Point::new(frame.look[0], frame.look[1]), Point::new(frame.right[0], frame.right[1]));
        let u_of = |p: Point| (p.x - o.x) * right.x + (p.y - o.y) * right.y;
        let d_of = |p: Point| (p.x - o.x) * look.x + (p.y - o.y) * look.y;
        let mut items: Vec<ElevItem> = vec![];
        let rect = |u0: f64, u1: f64, z0: f64, z1: f64| vec![[u0, z0], [u1, z0], [u1, z1], [u0, z1]];
        let (near, far) = sl.map_or((0.0, frame.depth_mm), |s| (s.plane, s.depth));
        let visible = |dmin: f64, dmax: f64| dmax > near && dmin < far;
        let hidden = |id: Id| v.hidden_elements.contains(&id);
        // walls: one rectangle per footprint piece
        let mut wall_pieces: std::collections::BTreeMap<Id, Vec<Vec<Point>>> = Default::default();
        let polys = self.wall_polygons();
        for (id, e) in self.iter() {
            let Element::Wall(w) = e else { continue };
            if hidden(id) {
                continue;
            }
            let pieces: Vec<Vec<Point>> = if w.curve.is_none() {
                polys.get(&id).cloned().into_iter().collect()
            } else {
                let path = w.path();
                let (el, er) = self.wall_extents(w);
                (1..path.pts.len())
                    .filter_map(|i| {
                        let (a, b) = (path.pts[i - 1], path.pts[i]);
                        let l = a.dist(&b);
                        if l < 1e-6 {
                            return None;
                        }
                        let n = Point::new(-(b.y - a.y) / l, (b.x - a.x) / l);
                        Some(vec![Point::new(a.x + n.x * el, a.y + n.y * el), Point::new(b.x + n.x * el, b.y + n.y * el), Point::new(b.x - n.x * er, b.y - n.y * er), Point::new(a.x - n.x * er, a.y - n.y * er)])
                    })
                    .collect()
            };
            wall_pieces.insert(id, pieces);
        }
        // Walls are drawn layer by layer from the joined plan layers, so a layer that runs round a corner
        // is one continuous face. A wall running along the line of sight is filled without an outline.
        struct LayerItem {
            id: Id,
            depth: f64,
            u0: f64,
            u1: f64,
            z0: f64,
            z1: f64,
            fill: [u8; 3],
            edge_on: bool,
            stroke: bool,
        }
        let mut layer_items: Vec<LayerItem> = vec![];
        let layer_geo = self.wall_layer_geometry();
        let mut host_near: std::collections::BTreeMap<Id, f64> = Default::default();
        for (id, pieces) in &wall_pieces {
            let Some(Element::Wall(w)) = self.get(*id) else { continue };
            let (z0, z1) = self.wall_extent(w);
            let path = w.path();
            let (a, b) = (path.pts[0], *path.pts.last().unwrap());
            let wl = (b.x - a.x).hypot(b.y - a.y).max(1e-9);
            let edge_on = ((b.x - a.x) * right.x + (b.y - a.y) * right.y).abs() / wl < 0.2;
            if let Some(wlay) = layer_geo.get(id) {
                let stroke = !(edge_on && !(wlay.free[0] && wlay.free[1]));
                for layer in wlay.layers.iter().filter(|l| !l.is_void && l.thickness > 0.0) {
                    let fill = self.material(&layer.material).map_or([200, 200, 200], |m| [m.graphics.shading_color[0], m.graphics.shading_color[1], m.graphics.shading_color[2]]);
                    for shape in &layer.shapes {
                        let Some(ring) = shape.first() else { continue };
                        let clipped;
                        let ring = match &sl {
                            Some(s) => {
                                clipped = s.clip_ring(ring);
                                &clipped
                            }
                            None => ring,
                        };
                        if ring.len() < 3 {
                            continue;
                        }
                        let us: Vec<f64> = ring.iter().map(|p| u_of(*p)).collect();
                        let ds: Vec<f64> = ring.iter().map(|p| d_of(*p)).collect();
                        let (dmin, dmax) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                        if !visible(dmin, dmax) {
                            continue;
                        }
                        let (u0, u1) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                        if u1 - u0 < 0.5 {
                            continue;
                        }
                        // end-on faces go behind face-on ones that start at the same depth
                        let depth = if edge_on { dmin + 0.5 } else { dmin };
                        let near = host_near.entry(*id).or_insert(f64::INFINITY);
                        *near = near.min(dmin);
                        layer_items.push(LayerItem { id: *id, depth, u0, u1, z0, z1, fill, edge_on, stroke });
                    }
                }
                continue;
            }
            // no layer geometry (no type): the whole footprint in one colour
            for poly in pieces {
                let clipped;
                let poly = match &sl {
                    Some(s) => {
                        clipped = s.clip_ring(poly);
                        &clipped
                    }
                    None => poly,
                };
                if poly.len() < 3 {
                    continue;
                }
                let us: Vec<f64> = poly.iter().map(|p| u_of(*p)).collect();
                let ds: Vec<f64> = poly.iter().map(|p| d_of(*p)).collect();
                let (dmin, dmax) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                if !visible(dmin, dmax) {
                    continue;
                }
                let (u0, u1) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                let near = host_near.entry(*id).or_insert(f64::INFINITY);
                *near = near.min(dmin);
                items.push(ElevItem { id: *id, category: "Walls", depth: dmin, polys: vec![rect(u0, u1, z0, z1)], lines: vec![], fill: [200, 200, 200], stroke: true , ..Default::default() });
            }
        }
        // A layer seen end on at a corner continues the same layer of the wall in front of it: join them into one
        // face, so there is no line where the two walls meet.
        let mut merged = vec![false; layer_items.len()];
        for e in 0..layer_items.len() {
            if !layer_items[e].edge_on {
                continue;
            }
            for f in 0..layer_items.len() {
                if layer_items[f].edge_on {
                    continue;
                }
                let (a, b) = (&layer_items[e], &layer_items[f]);
                let same = a.fill == b.fill && (a.z0 - b.z0).abs() < 1.0 && (a.z1 - b.z1).abs() < 1.0 && (a.depth - b.depth).abs() < 300.0;
                if same && a.u1 >= b.u0 - 1.0 && a.u0 <= b.u1 + 1.0 {
                    let (u0, u1) = (a.u0.min(b.u0), a.u1.max(b.u1));
                    layer_items[f].u0 = u0;
                    layer_items[f].u1 = u1;
                    merged[e] = true;
                }
            }
        }
        for (k, l) in layer_items.iter().enumerate() {
            if !merged[k] {
                items.push(ElevItem { id: l.id, category: "Walls", depth: l.depth, polys: vec![rect(l.u0, l.u1, l.z0, l.z1)], lines: vec![], fill: l.fill, stroke: l.stroke , ..Default::default() });
            }
        }
        // doors and windows sit on the wall face
        for (iid, i) in self.instances() {
            if hidden(iid) {
                continue;
            }
            let Some(h) = i.host else { continue };
            if !wall_pieces.contains_key(&h.wall) {
                continue;
            }
            let Some(f) = self.instance_frame(iid) else { continue };
            let wproj = f.dir.x * right.x + f.dir.y * right.y;
            let half = f.width * wproj.abs() / 2.0;
            if half < 15.0 {
                continue;
            }
            let (uc, dc) = (u_of(f.origin), d_of(f.origin));
            if !visible(dc, dc) {
                continue;
            }
            let Some((fam, _)) = self.instance_family(i) else { continue };
            let door = fam.category == FamilyCategory::Doors;
            let (z0, z1) = (f.z0, f.z1);
            let inset = 60.0_f64.min(half / 2.0);
            let depth = host_near.get(&h.wall).map_or(dc - 1.0, |d| d - 1.0);
            // a family with a stored body is drawn from its own geometry, nearest form last
            if let Some(solved) = self.instance_solved(iid) {
                if let Ok(s) = solved.as_ref() {
                    use crate::famgeom::Axis;
                    let cat = if door { "Doors" } else { "Windows" };
                    let sl = look.x * f.left.x + look.y * f.left.y;
                    let swing = if f.flip_facing { -1.0 } else { 1.0 };
                    let mut shown: Vec<(f64, crate::famview::Piece2D)> = vec![];
                    for p in s.project(Axis::Y) {
                        let Some(form) = s.form(p.form) else { continue };
                        if p.void || !form.vis.front_back {
                            continue;
                        }
                        // how near the viewer this form lies: larger is nearer
                        let y = form.solid.bounds().map_or(0.0, |b| (b[0][1] + b[1][1]) / 2.0);
                        shown.push((-sl.signum() * swing * y, p));
                    }
                    shown.sort_by(|a, b| a.0.total_cmp(&b.0));
                    for (k, (_, p)) in shown.into_iter().enumerate() {
                        let polys: Vec<Vec<[f64; 2]>> = p
                            .shape
                            .iter()
                            .map(|ring| ring.iter().map(|q| [u_of(f.fam_world(q.x, 0.0)), f.z_ref + q.y]).collect())
                            .collect();
                        let fill = match p.subcategory.as_str() {
                            "Glass" => [190, 218, 236],
                            "Panel" => [232, 226, 214],
                            _ => [205, 196, 180],
                        };
                        items.push(ElevItem { id: iid, category: cat, depth: depth - 0.01 * k as f64, polys, lines: vec![], fill, stroke: true , ..Default::default() });
                    }
                    continue;
                }
            }
            if door {
                // the frame is a U: up both jambs and across the head, open at the floor
                let frame = vec![
                    [uc - half, z0], [uc - half, z1], [uc + half, z1], [uc + half, z0],
                    [uc + half - inset, z0], [uc + half - inset, z1 - inset], [uc - half + inset, z1 - inset], [uc - half + inset, z0],
                ];
                items.push(ElevItem { id: iid, category: "Doors", depth, polys: vec![frame], lines: vec![], fill: [205, 196, 180], stroke: true , ..Default::default() });
                let mut leaves = vec![rect(uc - half + inset, uc + half - inset, z0, z1 - inset)];
                if f.width > 1500.0 {
                    leaves = vec![rect(uc - half + inset, uc - inset / 2.0, z0, z1 - inset), rect(uc + inset / 2.0, uc + half - inset, z0, z1 - inset)];
                }
                items.push(ElevItem { id: iid, category: "Doors", depth: depth - 0.1, polys: leaves, lines: vec![], fill: [232, 226, 214], stroke: true , ..Default::default() });
            } else {
                let polys = vec![rect(uc - half, uc + half, z0, z1), rect(uc - half + inset, uc + half - inset, z0 + inset, z1 - inset)];
                items.push(ElevItem { id: iid, category: "Windows", depth, polys, lines: vec![[[uc, z0 + inset], [uc, z1 - inset]]], fill: [190, 218, 236], stroke: true , ..Default::default() });
            }
        }
        // free standing families (furniture, fixtures, generic models): their solids as seen from the view. Seen square
        // on, each solid is its exact projection; seen at an angle, the outline of its corners.
        for (iid, i) in self.instances() {
            if hidden(iid) || i.host.is_some() {
                continue;
            }
            let Some(f) = self.instance_frame(iid) else { continue };
            let Some(solved) = self.instance_solved(iid) else { continue };
            let Ok(s) = solved.as_ref() else { continue };
            let Some((fam, _)) = self.instance_family(i) else { continue };
            let cat = fam.category.label();
            let Some(b) = self.instance_body_bounds(iid) else { continue };
            let corners = [f.fam_world(b[0][0], b[0][1]), f.fam_world(b[1][0], b[0][1]), f.fam_world(b[1][0], b[1][1]), f.fam_world(b[0][0], b[1][1])];
            let ds: Vec<f64> = corners.iter().map(|p| d_of(*p)).collect();
            if !visible(ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max)) {
                continue;
            }
            if let Some(slc) = &sl {
                if slc.clip_ring(&corners).len() < 3 {
                    continue;
                }
            }
            let hand = if f.flip_hand { -1.0 } else { 1.0 };
            let swing = if f.flip_facing { -1.0 } else { 1.0 };
            let (ax, ay) = (Point::new(f.dir.x * hand, f.dir.y * hand), Point::new(f.left.x * swing, f.left.y * swing));
            let (lx, ly) = (look.x * ax.x + look.y * ax.y, look.x * ay.x + look.y * ay.y);
            let mut shown: Vec<(f64, Vec<Vec<[f64; 2]>>)> = vec![];
            let centre = |form: &crate::famgeom::SolvedForm| form.solid.bounds().map_or(f.origin, |bb| f.fam_world((bb[0][0] + bb[1][0]) / 2.0, (bb[0][1] + bb[1][1]) / 2.0));
            if lx.abs() > 0.999 || ly.abs() > 0.999 {
                use crate::famgeom::Axis;
                let along_y = ly.abs() > 0.999;
                for p in s.project(if along_y { Axis::Y } else { Axis::X }) {
                    let Some(form) = s.form(p.form) else { continue };
                    if p.void || !form.vis.present || !(if along_y { form.vis.front_back } else { form.vis.left_right }) {
                        continue;
                    }
                    let polys = p.shape.iter().map(|ring| ring.iter().map(|q| [u_of(if along_y { f.fam_world(q.x, 0.0) } else { f.fam_world(0.0, q.x) }), f.z_ref + q.y]).collect()).collect();
                    shown.push((d_of(centre(form)), polys));
                }
            } else {
                for form in s.forms.iter().filter(|x| !x.void && x.vis.present) {
                    let pts: Vec<[f64; 2]> = form.solid.tris.iter().flatten().map(|q| { let w = f.fam_point(*q); [u_of(Point::new(w[0], w[1])), w[2]] }).collect();
                    let hull = convex_hull_2d(pts);
                    if hull.len() >= 3 {
                        shown.push((d_of(centre(form)), vec![hull]));
                    }
                }
            }
            for (d, polys) in shown {
                items.push(ElevItem { id: iid, category: cat, depth: d, polys, lines: vec![], fill: [205, 196, 180], stroke: true, ..Default::default() });
            }
        }
        // floors seen edge on
        for (id, e) in self.iter() {
            let Element::Floor(fl) = e else { continue };
            if hidden(id) {
                continue;
            }
            let Some(lv) = self.level(fl.level) else { continue };
            let thick = self.floor_type(fl.floor_type).map_or(200.0, |t| t.thickness_mm());
            let top = lv.elevation_mm + fl.offset_mm;
            let loops: Vec<Vec<Point>> = fl.loops().iter().map(|l| crate::curve::loop_polygon(l, 20.0)).collect();
            let depths = crate::curve::loop_depths(&loops);
            for (poly, depth) in loops.iter().zip(depths) {
                if depth != 0 || poly.len() < 3 {
                    continue;
                }
                let clipped;
                let poly = match &sl {
                    Some(s) => {
                        clipped = s.clip_ring(poly);
                        &clipped
                    }
                    None => poly,
                };
                if poly.len() < 3 {
                    continue;
                }
                let us: Vec<f64> = poly.iter().map(|p| u_of(*p)).collect();
                let ds: Vec<f64> = poly.iter().map(|p| d_of(*p)).collect();
                let (dmin, dmax) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                if !visible(dmin, dmax) {
                    continue;
                }
                let (u0, u1) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                // a sloped floor is seen as its sloped edge: the hull of the top and bottom of every corner
                let sloped = self.floor_surface(fl).filter(|s| s.rise.abs() > 1e-9);
                let polys = match sloped {
                    Some(s) => {
                        let mut pts: Vec<[f64; 2]> = Vec::new();
                        for (p, u) in poly.iter().zip(&us) {
                            let z = s.z(*p);
                            pts.push([*u, z]);
                            pts.push([*u, z - thick]);
                        }
                        vec![convex_hull_2d(pts)]
                    }
                    None => vec![rect(u0, u1, top - thick, top)],
                };
                items.push(ElevItem { id, category: "Floors", depth: dmin - 0.5, polys, lines: vec![], fill: [170, 172, 176], stroke: true , ..Default::default() });
            }
        }
        // roofs: every plane as a polygon, so hips, gables and eaves show as they are
        for (id, e) in self.iter() {
            let Element::Roof(r) = e else { continue };
            if hidden(id) {
                continue;
            }
            let Some(g) = self.roof_geometry(r) else { continue };
            let t = self.roof_type(r.roof_type);
            let top = t.and_then(|t| t.layers.first()).and_then(|l| self.material(&l.material)).map_or([150, 120, 110], |m| [m.graphics.shading_color[0], m.graphics.shading_color[1], m.graphics.shading_color[2]]);
            let thick = t.map_or(0.0, |t| t.thickness_mm());
            let mut add = |poly: &[[f64; 3]], shade: f64, bias: f64| {
                let clipped;
                let poly = match &sl {
                    Some(s) => {
                        clipped = s.clip_ring3(poly);
                        &clipped[..]
                    }
                    None => poly,
                };
                let ds: Vec<f64> = poly.iter().map(|q| d_of(Point::new(q[0], q[1]))).collect();
                let (dmin, dmax) = (ds.iter().cloned().fold(f64::INFINITY, f64::min), ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                if poly.len() < 3 || !visible(dmin, dmax) {
                    return;
                }
                let uz: Vec<[f64; 2]> = poly.iter().map(|q| [u_of(Point::new(q[0], q[1])), q[2]]).collect();
                let n = uz.len();
                let area = (0..n).map(|i| uz[i][0] * uz[(i + 1) % n][1] - uz[(i + 1) % n][0] * uz[i][1]).sum::<f64>().abs() / 2.0;
                if area < 1.0 {
                    return;
                }
                let c = |v: u8| (v as f64 * shade).clamp(0.0, 255.0) as u8;
                items.push(ElevItem { id, category: "Roofs", depth: ds.iter().cloned().fold(f64::INFINITY, f64::min) + bias, polys: vec![uz], lines: vec![], fill: [c(top[0]), c(top[1]), c(top[2])], stroke: true , ..Default::default() });
            };
            for f in &g.faces {
                // faces turned toward the viewer are lit, the others a little darker
                let facing = f.outward[0] * look.x + f.outward[1] * look.y;
                let shade = if f.vertical { 0.92 } else { 1.0 - 0.18 * (facing).max(0.0) };
                add(&f.poly, shade, 0.0);
                // the thickness at the eave and along gable ends
                if thick > 0.0 {
                    let n = f.poly.len();
                    let drop = thick / f.slope_deg.min(80.0).to_radians().cos().max(0.2);
                    let edges: Vec<usize> = if f.vertical { (0..n.saturating_sub(1)).collect() } else { vec![n - 1] };
                    for i in edges {
                        let (a, b) = (f.poly[i], f.poly[(i + 1) % n]);
                        if f.vertical && (a[2] - g.base_z).abs() < 1e-6 && (b[2] - g.base_z).abs() < 1e-6 {
                            continue;
                        }
                        add(&[a, b, [b[0], b[1], b[2] - drop], [a[0], a[1], a[2] - drop]], 0.8, -0.5);
                    }
                }
            }
            for cap in &g.caps {
                add(cap, 1.0, 0.0);
            }
            for reg in &g.flat {
                let ring: Vec<[f64; 3]> = reg.outer.iter().map(|p| [p.x, p.y, g.base_z]).collect();
                let n = ring.len();
                for i in 0..n {
                    let (a, b) = (ring[i], ring[(i + 1) % n]);
                    add(&[a, b, [b[0], b[1], b[2] - thick], [a[0], a[1], a[2] - thick]], 0.85, 0.0);
                }
            }
        }
        if let Some(s) = sl {
            // keep to this run's stretch of the view, then add what its plane cuts (nearest, so drawn last)
            for it in items.iter_mut() {
                it.polys = it.polys.iter().map(|p| s.clip_width(p)).filter(|p| p.len() >= 3).collect();
                it.lines.retain(|l| l[0][0].max(l[1][0]) > s.lo && l[0][0].min(l[1][0]) < s.hi);
            }
            items.retain(|it| !it.polys.is_empty() || !it.lines.is_empty());
            let mut far = vec![];
            items.extend(self.section_cut_items(s, &hidden, &mut far));
            items.extend(far);
        }
        if let Some(c) = cue {
            // measured from the plane this run cuts at, as depth cueing measures from the front clip plane
            let plane = sl.map_or(0.0, |s| s.plane);
            for it in items.iter_mut().filter(|i| !i.cut) {
                it.fade = c.fade(it.depth - plane, c.depth - plane);
            }
        }
        (items, wall_pieces, host_near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn house() -> (Document, Id) {
        let mut d = Document::new_project();
        let (lvl, wt) = (d.levels()[0].0, d.wall_types().into_iter().find(|(_, t)| t.name == "Brick Veneer").unwrap().0);
        let ids: Vec<Id> = [((0., 0.), (8000., 0.)), ((8000., 0.), (8000., 6000.)), ((8000., 6000.), (0., 6000.)), ((0., 6000.), (0., 0.))]
            .iter()
            .map(|(a, b)| d.add_wall(wt, lvl, Point::new(a.0, a.1), Point::new(b.0, b.1), 2700.0).unwrap())
            .collect();
        (d, ids[0])
    }

    #[test]
    fn a_new_project_has_a_site_plan_a_base_point_and_four_elevations_round_the_model() {
        let d = Document::new_project();
        let (_, loc) = d.project_location().expect("location");
        assert_eq!(loc.base, P3::default());
        assert!(loc.survey_clipped);
        let names: Vec<String> = d.views().iter().map(|(_, v)| v.name.clone()).collect();
        for n in ["Site", "North", "South", "East", "West", "Ground", "Level 1"] {
            assert!(names.contains(&n.to_string()), "{n} in {names:?}");
        }
        let site = d.views().into_iter().find(|(_, v)| v.name == "Site").unwrap().1.clone();
        assert_eq!(site.settings.orientation, Orientation::TrueNorth);
        assert_eq!(site.settings.scale, 500);
        let ms = d.elevation_markers();
        assert_eq!(ms.len(), 4);
        // the North marker is above the model and looks south, at it
        let (_, north) = ms.iter().find(|(_, m)| m.slots[0].is_some()).unwrap();
        assert!(north.position.y > 0.0);
        let look = north.look(0);
        assert!(look.y < -0.99);
        // seen looking south, east is on the left
        assert!(north.right(0).x < -0.99);
        // defaults are not undoable
        assert!(Document::new_project().undo_name().is_none());
    }

    #[test]
    fn older_projects_get_the_defaults_round_what_they_hold_and_only_once() {
        let (mut d, _) = house();
        // pretend this is a file saved before locations existed
        let loc = d.project_location().unwrap().0;
        let markers: Vec<Id> = d.elevation_markers().iter().map(|(i, _)| *i).collect();
        let views: Vec<Id> = d.views().into_iter().filter(|(_, v)| v.view_kind == ViewKind::Elevation || v.name == "Site").map(|(i, _)| i).collect();
        for i in markers.into_iter().chain(views).chain([loc]) {
            d.elements.remove(&i);
        }
        assert!(d.project_location().is_none());
        d.ensure_site_defaults();
        let ms = d.elevation_markers();
        assert_eq!(ms.len(), 4);
        let north = ms.iter().find(|(_, m)| m.slots[0].is_some()).unwrap().1;
        assert!((north.position.x - 4000.0).abs() < 1e-6 && north.position.y > 6000.0, "{:?}", north.position);
        let n = d.views().len();
        d.ensure_site_defaults();
        assert_eq!(d.views().len(), n, "the second call changes nothing");
    }

    #[test]
    fn level_elevations_read_from_the_base_point() {
        let (mut d, _) = house();
        let l2 = d.levels()[1].0;
        assert_eq!(d.level_display_elevation(d.level(l2).unwrap()), 3000.0);
        d.move_site_point(SitePoint::Base, P3 { x: 0.0, y: 0.0, z: -10000.0 }).unwrap();
        assert_eq!(d.level_display_elevation(d.level(l2).unwrap()), 13000.0, "the datum moved, the level did not");
        assert_eq!(d.level(l2).unwrap().elevation_mm, 3000.0);
        // typing an elevation is read from the same datum
        d.patch_level(l2, &serde_json::json!({ "display_elevation_mm": 13500.0 })).unwrap();
        assert_eq!(d.level(l2).unwrap().elevation_mm, 3500.0);
        // from the survey point instead
        d.patch_level(l2, &serde_json::json!({ "elevation_base": "survey_point" })).unwrap();
        assert_eq!(d.level_display_elevation(d.level(l2).unwrap()), 3500.0);
        // pinned points refuse
        d.patch_project_location(&serde_json::json!({ "base_pinned": true })).unwrap();
        assert!(d.move_site_point(SitePoint::Base, P3::default()).unwrap_err().contains("pinned"));
        // a clipped survey point takes the shared origin with it, so shared coordinates of the base change
        d.move_site_point(SitePoint::Survey, P3 { x: 1000.0, y: 2000.0, z: 50.0 }).unwrap();
        let s = d.shared_coordinates(P3 { x: 0.0, y: 0.0, z: -10000.0 });
        assert_eq!((s.x, s.y, s.z), (-1000.0, -2000.0, -10050.0));
    }

    #[test]
    fn levels_are_added_renamed_moved_and_only_deleted_when_nothing_uses_them() {
        let (mut d, _) = house();
        let n = d.views().len();
        let l3 = d.add_level(None, 6000.0, true).unwrap();
        assert_eq!(d.level(l3).unwrap().name, "Level 3");
        assert_eq!(d.views().len(), n + 1, "its plan");
        assert!(d.add_level(Some("Level 3"), 9000.0, true).is_err());
        d.patch_level(l3, &serde_json::json!({ "name": "Roof" })).unwrap();
        assert!(d.views().iter().any(|(_, v)| v.name == "Roof" && v.level == Some(l3)), "the plan follows the name");
        // a wall that tops out at the level moves with it
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[0].0);
        let mut w = Wall::new(wt, lvl, Point::new(0., 9000.), Point::new(3000., 9000.), 2700.0);
        w.top_level = Some(l3);
        let wid = d.add_wall_with(w).unwrap();
        assert!(d.delete_level(l3).unwrap_err().contains("use level"));
        d.patch_level(l3, &serde_json::json!({ "display_elevation_mm": 6600.0 })).unwrap();
        let Some(Element::Wall(w)) = d.get(wid) else { panic!() };
        assert_eq!(d.wall_extent(w).1, 6600.0);
        d.delete(&[wid]).unwrap();
        d.delete_level(l3).unwrap();
        assert!(d.level(l3).is_none() && !d.views().iter().any(|(_, v)| v.level == Some(l3)));
        d.undo();
        assert!(d.level(l3).is_some() && d.views().iter().any(|(_, v)| v.level == Some(l3)), "one undo brings back the level and its plan");
    }

    #[test]
    fn markers_make_and_drop_views_and_the_last_one_takes_the_marker() {
        let (mut d, _) = house();
        let lvl = d.levels()[0].0;
        let m = d.add_elevation_marker(lvl, Point::new(2000.0, 3000.0), 0.0, [true, false, true, false]).unwrap();
        let mk = d.elevation_marker(m).unwrap().clone();
        assert!(mk.slots[0].is_some() && mk.slots[1].is_none() && mk.slots[2].is_some());
        assert_eq!(d.view(mk.slots[0].unwrap()).unwrap().name, "Elevation 1 - a");
        d.set_marker_slot(m, 1, true).unwrap();
        d.set_marker_slot(m, 0, false).unwrap();
        d.set_marker_slot(m, 2, false).unwrap();
        assert!(d.elevation_marker(m).is_some());
        d.set_marker_slot(m, 1, false).unwrap();
        assert!(d.elevation_marker(m).is_none(), "a marker without views is gone");
        assert!(!d.views().iter().any(|(_, v)| v.name.starts_with("Elevation 1")));
    }

    #[test]
    fn the_north_elevation_draws_the_far_and_near_walls_and_a_door_in_the_wall_facing_it() {
        let (mut d, _) = house();
        let lvl = d.levels()[0].0;
        let t = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let north_wall = d.iter().find_map(|(id, e)| match e { Element::Wall(w) if w.start.y == 6000.0 && w.end.y == 6000.0 => Some(id), _ => None }).unwrap();
        let door = d.place_on_wall(t, north_wall, Point::new(4000.0, 6300.0)).unwrap();
        let view = d.views().into_iter().find(|(_, v)| v.name == "North").unwrap().0;
        let e = d.elevation_data(view).unwrap();
        assert!(e.levels.iter().any(|l| l.name == "Level 1" && l.elev == 3000.0));
        assert!(e.items.iter().filter(|i| i.category == "Walls").count() >= 4);
        let its: Vec<&ElevItem> = e.items.iter().filter(|i| i.id == door).collect();
        assert!(its.len() >= 2, "the door is drawn as its frame and leaf");
        // seen from the north, the wall's x runs right to left: x = 4000 is centred about u = 4000 - the marker's own x offset
        let m = d.elevation_markers().into_iter().find(|(_, m)| m.slots[0].is_some()).unwrap().1.clone();
        let uc = -(4000.0 - m.position.x);
        let xs: Vec<f64> = its.iter().flat_map(|i| i.polys.iter().flatten().map(|p| p[0])).collect();
        assert!(((xs.iter().cloned().fold(f64::INFINITY, f64::min) + xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)) / 2.0 - uc).abs() < 1.0, "{xs:?} vs {uc}");
        // painter's order: far first, so the door (on the near wall) comes after the far south wall
        let south_wall = d.iter().find_map(|(id, e)| match e { Element::Wall(w) if w.start.y == 0.0 && w.end.y == 0.0 => Some(id), _ => None }).unwrap();
        let pos = |id: Id| e.items.iter().position(|i| i.id == id).unwrap();
        assert!(pos(south_wall) < pos(north_wall) && pos(north_wall) < pos(door));
        // the leaf is a closed panel 820 wide and 2040 high, off the floor by the bottom gap
        let leaf = its.iter().find(|i| i.fill == [232, 226, 214]).expect("a leaf");
        let (lx, lz): (Vec<f64>, Vec<f64>) = (leaf.polys[0].iter().map(|p| p[0]).collect(), leaf.polys[0].iter().map(|p| p[1]).collect());
        let span = |v: &[f64]| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - v.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!((span(&lx) - 820.0).abs() < 1e-6 && (span(&lz) - 2040.0).abs() < 1e-6, "{lx:?} {lz:?}");
        let _ = lvl;
    }

    #[test]
    fn elevation_reference_lines_name_wall_faces_opening_edges_sills_and_heads() {
        let (mut d, _) = house();
        let t = d.family_types().iter().find(|(_, t)| t.name.contains("820 x 2040")).unwrap().0;
        let north_wall = d.iter().find_map(|(id, e)| match e { Element::Wall(w) if w.start.y == 6000.0 && w.end.y == 6000.0 => Some(id), _ => None }).unwrap();
        let door = d.place_on_wall(t, north_wall, Point::new(4000.0, 6300.0)).unwrap();
        let view = d.views().into_iter().find(|(_, v)| v.name == "North").unwrap().0;
        let e = d.elevation_data(view).unwrap();
        let m = d.elevation_markers().into_iter().find(|(_, m)| m.slots[0].is_some()).unwrap().1.clone();
        let uc = -(4000.0 - m.position.x);
        // the opening's edges are the hole in the wall (a little wider than the 820 leaf), either side of the door's centre
        let edges: Vec<&ElevRef> = e.refs.iter().filter(|r| r.id == door && r.label == "Opening edge").collect();
        assert_eq!(edges.len(), 2);
        let mut ps: Vec<f64> = edges.iter().map(|r| r.pos).collect();
        ps.sort_by(|a, b| a.total_cmp(b));
        assert!(((ps[0] + ps[1]) / 2.0 - uc).abs() < 1.0 && ps[1] - ps[0] >= 820.0, "{ps:?} vs {uc}");
        let z = |label: &str| e.refs.iter().find(|r| r.id == door && r.label == label && !r.vertical).map(|r| r.pos).unwrap_or(f64::NAN);
        assert!(z("Head") - z("Sill") >= 2040.0 && z("Head") - z("Sill") < 2200.0, "{} {}", z("Sill"), z("Head"));
        // walls seen end on offer their faces and centreline up the view; the ones across it their ends
        let east = d.iter().find_map(|(id, el)| match el { Element::Wall(w) if w.start.x == 8000.0 && w.end.x == 8000.0 => Some(id), _ => None }).unwrap();
        let labels: Vec<&str> = e.refs.iter().filter(|r| r.id == east && r.vertical).map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Face") && labels.contains(&"Centreline"), "{labels:?}");
        assert!(e.refs.iter().any(|r| r.id == north_wall && r.label == "Wall end"));
        assert!(e.refs.iter().any(|r| r.id == north_wall && r.label == "Top" && !r.vertical));
    }
    #[test]
    fn plans_are_made_on_demand_and_numbered_when_duplicated() {
        let mut d = Document::new_project();
        let l3 = d.add_level(Some("Level 2"), 6000.0, false).unwrap();
        assert!(!d.views().iter().any(|(_, v)| v.level == Some(l3)), "a new level makes no plan unless asked");
        let fp = d.view_types().into_iter().find(|(_, t)| t.view_kind == ViewKind::FloorPlan).unwrap().0;
        let cp = d.view_types().into_iter().find(|(_, t)| t.view_kind == ViewKind::CeilingPlan).unwrap().0;
        let made = d.create_plan_views(fp, &[l3], true, Some(50)).unwrap();
        let v = d.view(made[0]).unwrap();
        assert_eq!((v.name.as_str(), v.settings.scale, v.view_type, v.level), ("Level 2", 50, Some(fp), Some(l3)));
        assert!(d.create_plan_views(fp, &[l3], true, None).is_err(), "nothing new to make");
        let again = d.create_plan_views(fp, &[l3], false, None).unwrap();
        assert_eq!(d.view(again[0]).unwrap().name, "Level 2(1)");
        // a ceiling plan can share the level's name, since it is another kind of view
        let rcp = d.create_plan_views(cp, &[l3], true, None).unwrap();
        assert_eq!(d.view(rcp[0]).unwrap().name, "Level 2");
        assert_eq!(d.view(rcp[0]).unwrap().view_kind, ViewKind::CeilingPlan);
        // renaming the level renames the plans that carry its name
        d.patch_level(l3, &serde_json::json!({ "name": "Roof" })).unwrap();
        assert_eq!(d.view(made[0]).unwrap().name, "Roof");
        assert_eq!(d.view(rcp[0]).unwrap().name, "Roof");
        // every plan in a new project has a type
        assert!(d.views().iter().filter(|(_, v)| v.view_kind.is_plan()).all(|(_, v)| v.view_type.is_some()));
    }

}
