//! IFC4 export: the model as an ISO 10303-21 (STEP) file in the IFC4 schema, the open exchange format other BIM
//! tools read. Levels that are building storeys become IfcBuildingStorey; walls are their joined plan outline swept
//! up (a curved wall, its faceted outline), with an IfcOpeningElement for each door or window that voids the wall
//! and the door or window filling it; flat floors are their outline less holes swept down from the top, sloped
//! floors, roofs, doors, windows and components closed triangle sets of what the 3D view draws. Layered wall and
//! floor types go out as material layer sets, materials with their shading colour. Groups become IfcGroup.
//!
//! Ported from BIMCraft (https://github.com/IPedrax/BIMCraft, crates/core/src/ifc.rs at commit c269c8c),
//! Copyright (c) 2026 IPedrax and the BIMCraft contributors, MIT OR Apache-2.0: the STEP writer (string escaping,
//! reals, GlobalId compression, entity numbering), the spatial tree, swept bodies, opening and fill relations,
//! material layer sets and the file header. Rewritten against OpenArch's model; see THIRD-PARTY-NOTICES.md.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::doc::Document;
use crate::element::*;
use crate::family::FamilyCategory;
use crate::geom::Point;
use crate::group::GroupKind;

// What a derived GlobalId is for, so each stays the same from one export to the next.
const PROJECT: u64 = 1;
const SITE: u64 = 2;
const BUILDING: u64 = 3;
const STOREY: u64 = 4;
const ELEMENT: u64 = 5;
const OPENING: u64 = 6;
const VOIDS: u64 = 7;
const FILLS: u64 = 8;
const AGGREGATES: u64 = 9;
const CONTAINS: u64 = 10;
const MATERIALS: u64 = 11;
const PSET: u64 = 12;
const PSET_DEFINES: u64 = 13;
const GROUP: u64 = 14;
const GROUP_MEMBERS: u64 = 15;

/// A STEP string: quotes doubled, backslashes escaped, anything outside printable ASCII as \X2\ or \X4\.
fn s(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str(r"\\"),
            ' '..='~' => out.push(c),
            _ if u32::from(c) > 0xFFFF => {
                let _ = write!(out, r"\X4\{:08X}\X0\", u32::from(c));
            }
            _ => {
                let _ = write!(out, r"\X2\{:04X}\X0\", u32::from(c));
            }
        }
    }
    out.push('\'');
    out
}

/// A STEP real: always with a decimal point.
fn real(x: f64) -> String {
    let x = if x.is_finite() && x != 0.0 { x } else { 0.0 };
    let t = format!("{x:.6}");
    let t = t.trim_end_matches('0');
    if t == "-0." { "0.".into() } else { t.to_string() }
}

/// A direction's ratio, to twelve places.
fn ratio(x: f64) -> String {
    let x = if x.is_finite() && x != 0.0 { x } else { 0.0 };
    format!("{x:.12}").trim_end_matches('0').to_string()
}

fn refs(ids: &[u64]) -> String {
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}

const DIGITS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// 128 bits as an IFC GlobalId: 22 base-64 digits, the first carrying the top 2 bits.
pub fn compress(v: u128) -> String {
    let digit = |shift: u32| char::from(DIGITS[((v >> shift) & 63) as usize]);
    std::iter::once(digit(126)).chain((0..21).rev().map(|i| digit(6 * i))).collect()
}

/// Whether `g` is a GlobalId as IFC writes them.
fn valid_guid(g: &str) -> bool {
    g.len() == 22 && g.bytes().all(|c| DIGITS.contains(&c)) && DIGITS.iter().position(|d| *d == g.as_bytes()[0]).is_some_and(|d| d < 4)
}

/// SplitMix64: a quick, well mixed hash step.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A GlobalId for what plays `role` for `id`, the same on every export.
fn derived(role: u64, id: u64) -> String {
    let hi = mix(role.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ id);
    let lo = mix(hi ^ id.rotate_left(17) ^ role);
    compress((u128::from(hi) << 64) | u128::from(lo))
}

/// UTC date and time of a Unix time, ISO 8601 (Howard Hinnant's days-to-civil).
fn utc(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}

/// The DATA section being written.
struct Step {
    out: String,
    next: u64,
    z: u64,
    world: u64,
    context: u64,
    body: u64,
    axis: u64,
    /// Each material written, and the surface style of its colour.
    materials: BTreeMap<String, (u64, Option<u64>)>,
}

impl Step {
    /// Write `#n=ENTITY;` and return n.
    fn add(&mut self, entity: impl AsRef<str>) -> u64 {
        self.next += 1;
        let _ = writeln!(self.out, "#{}={};", self.next, entity.as_ref());
        self.next
    }

    fn point(&mut self, p: Point) -> u64 {
        self.add(format!("IFCCARTESIANPOINT(({},{}))", real(p.x), real(p.y)))
    }

    /// Axes at `xyz` with the x axis along `dir` (in plan).
    fn axes(&mut self, (x, y, z): (f64, f64, f64), dir: Option<Point>) -> u64 {
        let o = self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(x), real(y), real(z)));
        match dir {
            Some(d) => {
                let d = self.add(format!("IFCDIRECTION(({},{},0.))", ratio(d.x), ratio(d.y)));
                self.add(format!("IFCAXIS2PLACEMENT3D(#{o},#{},#{d})", self.z))
            }
            None => self.add(format!("IFCAXIS2PLACEMENT3D(#{o},$,$)")),
        }
    }

    /// A placement at `xyz`, x along `dir`, relative to `to` (none: the world).
    fn place(&mut self, to: Option<u64>, xyz: (f64, f64, f64), dir: Option<Point>) -> u64 {
        let a = self.axes(xyz, dir);
        let to = to.map_or_else(|| "$".to_string(), |t| format!("#{t}"));
        self.add(format!("IFCLOCALPLACEMENT({to},#{a})"))
    }

    fn polyline(&mut self, ring: &[Point]) -> u64 {
        let mut pts: Vec<u64> = ring.iter().map(|p| self.point(*p)).collect();
        pts.extend(pts.first().copied());
        self.add(format!("IFCPOLYLINE(({}))", refs(&pts)))
    }

    /// A profile around `outer`, less `holes`.
    fn profile(&mut self, outer: &[Point], holes: &[Vec<Point>]) -> u64 {
        let o = self.polyline(outer);
        if holes.is_empty() {
            return self.add(format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{o})"));
        }
        let inner: Vec<u64> = holes.iter().map(|h| self.polyline(h)).collect();
        self.add(format!("IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#{o},({}))", refs(&inner)))
    }

    /// A rectangle `w` by `d` centred on `c`, as a profile.
    fn rectangle(&mut self, c: Point, w: f64, d: f64) -> u64 {
        let c = self.point(c);
        let at = self.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
        self.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{at},{},{})", real(w), real(d)))
    }

    /// Solids, each a profile swept `depth` up from `bottom`, as a body, plus any other representations.
    fn swept(&mut self, solids: &[(u64, f64, f64)], extra: &[u64]) -> u64 {
        let items: Vec<u64> = solids
            .iter()
            .map(|&(profile, bottom, depth)| {
                let at = if bottom == 0.0 { self.world } else { self.axes((0.0, 0.0, bottom), None) };
                self.add(format!("IFCEXTRUDEDAREASOLID(#{profile},#{at},#{},{})", self.z, real(depth)))
            })
            .collect();
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','SweptSolid',({}))", self.body, refs(&items)));
        // the body first: readers that take only the first representation get the solid
        let reps: Vec<u64> = std::iter::once(rep).chain(extra.iter().copied()).collect();
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({}))", refs(&reps)))
    }

    /// Triangles from a 3D view mesh (position and normal per corner, metres), as a face set in mm with `dz` added,
    /// each corner written once.
    fn faceset(&mut self, verts: &[f32], dz: f64) -> Option<u64> {
        let mut index: BTreeMap<[i64; 3], usize> = BTreeMap::new();
        let mut pts: Vec<[f64; 3]> = vec![];
        let mut faces = vec![];
        for t in verts.chunks_exact(18) {
            let mut c = [0usize; 3];
            for (k, v) in [0, 6, 12].iter().enumerate() {
                let p = [f64::from(t[*v]) * 1000.0, f64::from(t[v + 1]) * 1000.0, f64::from(t[v + 2]) * 1000.0 + dz];
                c[k] = *index.entry(p.map(|x| (x * 10.0).round() as i64)).or_insert_with(|| {
                    pts.push(p);
                    pts.len()
                });
            }
            if c[0] != c[1] && c[1] != c[2] && c[0] != c[2] {
                faces.push(format!("({},{},{})", c[0], c[1], c[2]));
            }
        }
        if faces.is_empty() {
            return None;
        }
        let coords: Vec<String> = pts.iter().map(|p| format!("({},{},{})", real(p[0]), real(p[1]), real(p[2]))).collect();
        let list = self.add(format!("IFCCARTESIANPOINTLIST3D(({}))", coords.join(",")));
        Some(self.add(format!("IFCTRIANGULATEDFACESET(#{list},$,.F.,({}),$)", faces.join(","))))
    }

    fn tessellated(&mut self, items: &[u64]) -> u64 {
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Tessellation',({}))", self.body, refs(items)));
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"))
    }

    /// A surface style of a colour, `transparency` 0 to 1.
    fn style(&mut self, name: &str, rgb: [u8; 3], transparency: f64) -> u64 {
        let [r, g, b] = rgb.map(|c| real(f64::from(c) / 255.0));
        let colour = self.add(format!("IFCCOLOURRGB($,{r},{g},{b})"));
        let t = if transparency > 0.0 { real(transparency) } else { "$".into() };
        let shading = self.add(format!("IFCSURFACESTYLESHADING(#{colour},{t})"));
        self.add(format!("IFCSURFACESTYLE({},.BOTH.,(#{shading}))", s(name)))
    }

    /// A material by name, written once, with its shading colour for viewers.
    fn material(&mut self, doc: &Document, name: &str) -> u64 {
        if let Some(&(id, _)) = self.materials.get(name) {
            return id;
        }
        let id = self.add(format!("IFCMATERIAL({},$,$)", s(name)));
        let style = doc.material(name).map(|m| {
            let style = self.style(name, m.graphics.shading_color, f64::from(m.graphics.shading_transparency) / 100.0);
            let item = self.add(format!("IFCSTYLEDITEM($,(#{style}),$)"));
            let rep = self.add(format!("IFCSTYLEDREPRESENTATION(#{},'Style','Material',(#{item}))", self.context));
            self.add(format!("IFCMATERIALDEFINITIONREPRESENTATION($,$,(#{rep}),#{id})"));
            style
        });
        self.materials.insert(name.to_string(), (id, style));
        id
    }

    /// A material layer set of `layers`, outermost first.
    fn layer_set(&mut self, doc: &Document, name: &str, layers: &[Layer]) -> u64 {
        let mut ids = vec![];
        for l in layers {
            let material = if l.material.trim().is_empty() { "$".to_string() } else { format!("#{}", self.material(doc, l.material.trim())) };
            let category = match l.function {
                LayerFunction::Structure => "'LoadBearing'",
                LayerFunction::ThermalAir => "'Insulation'",
                LayerFunction::Membrane => "'Membrane'",
                LayerFunction::Substrate => "'Substrate'",
                LayerFunction::Finish1 | LayerFunction::Finish2 => "'Finish'",
            };
            let vent = if l.is_void { ".T." } else { "$" };
            ids.push(self.add(format!("IFCMATERIALLAYER({material},{},{vent},$,$,{category},$)", real(l.thickness_mm))));
        }
        self.add(format!("IFCMATERIALLAYERSET(({}),{},$)", refs(&ids), s(name)))
    }

    /// A property set `name` of `props` for `ent`.
    fn pset(&mut self, ent: u64, key: u64, name: &str, props: &[u64]) {
        let set = self.add(format!("IFCPROPERTYSET({},$,{},$,({}))", s(&derived(PSET, key)), s(name), refs(props)));
        self.add(format!("IFCRELDEFINESBYPROPERTIES({},$,$,$,(#{ent}),#{set})", s(&derived(PSET_DEFINES, key))));
    }

    fn boolean(&mut self, name: &str, v: bool) -> u64 {
        self.add(format!("IFCPROPERTYSINGLEVALUE({},$,IFCBOOLEAN({}),$)", s(name), if v { ".T." } else { ".F." }))
    }
}

/// An element's GlobalId: its own, or one made from its id.
fn own_guid(id: Id, ifc: &IfcParams) -> String {
    s(&if valid_guid(&ifc.guid) { ifc.guid.clone() } else { derived(ELEMENT, id) })
}

/// The IFC class and predefined type a family instance of `cat` goes out as.
fn component_class(cat: FamilyCategory) -> &'static str {
    match cat {
        FamilyCategory::Furniture | FamilyCategory::FurnitureSystems | FamilyCategory::Casework => "IFCFURNITURE",
        FamilyCategory::PlumbingFixtures => "IFCSANITARYTERMINAL",
        FamilyCategory::LightingFixtures => "IFCLIGHTFIXTURE",
        _ => "IFCBUILDINGELEMENTPROXY",
    }
}

impl Document {
    /// The model as an IFC4 file (ISO 10303-21 text), named `name` in its header and on the project.
    pub fn export_ifc(&self, name: &str) -> String {
        let mut w = Step { out: String::new(), next: 0, z: 0, world: 0, context: 0, body: 0, axis: 0, materials: BTreeMap::new() };
        let origin = w.add("IFCCARTESIANPOINT((0.,0.,0.))");
        w.z = w.add("IFCDIRECTION((0.,0.,1.))");
        w.world = w.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"));
        let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$)", w.world));
        w.context = ctx;
        w.body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$)"));
        w.axis = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,#{ctx},$,.GRAPH_VIEW.,$)"));
        let units = [
            "IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)",
            "IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)",
            "IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)",
            "IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)",
        ]
        .map(|u| w.add(u));
        let units = w.add(format!("IFCUNITASSIGNMENT(({}))", refs(&units)));
        let project = w.add(format!("IFCPROJECT({},$,{},$,$,$,$,(#{ctx}),#{units})", s(&derived(PROJECT, 0)), s(name)));
        let site_at = w.place(None, (0.0, 0.0, 0.0), None);
        let site = w.add(format!("IFCSITE({},$,'Site',$,$,#{site_at},$,$,.ELEMENT.,$,$,$,$,$)", s(&derived(SITE, 0))));
        let building_at = w.place(Some(site_at), (0.0, 0.0, 0.0), None);
        let building = w.add(format!("IFCBUILDING({},$,{},$,$,#{building_at},$,$,.ELEMENT.,$,$,$)", s(&derived(BUILDING, 0)), s(name)));
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{project},(#{site}))", s(&derived(AGGREGATES, PROJECT))));
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{site},(#{building}))", s(&derived(AGGREGATES, SITE))));

        // storeys: the building storey levels; an element on another level goes in the storey at or below it
        let levels = self.levels();
        let mut storey_levels: Vec<(Id, f64)> = levels.iter().filter(|(_, l)| l.building_story).map(|(i, l)| (*i, l.elevation_mm)).collect();
        if storey_levels.is_empty() {
            storey_levels = levels.iter().map(|(i, l)| (*i, l.elevation_mm)).collect();
        }
        let mut storeys: BTreeMap<Id, (u64, u64, f64)> = BTreeMap::new(); // level: (storey, placement, elevation)
        for (id, l) in &levels {
            if !storey_levels.iter().any(|(s, _)| s == id) {
                continue;
            }
            let at = w.place(Some(building_at), (0.0, 0.0, l.elevation_mm), None);
            let st = w.add(format!("IFCBUILDINGSTOREY({},$,{},$,$,#{at},$,$,.ELEMENT.,{})", s(&derived(STOREY, *id)), s(&l.name), real(l.elevation_mm)));
            storeys.insert(*id, (st, at, l.elevation_mm));
        }
        if storeys.is_empty() {
            // a project with no levels still has one storey to hold its elements
            let at = w.place(Some(building_at), (0.0, 0.0, 0.0), None);
            let st = w.add(format!("IFCBUILDINGSTOREY({},$,'Level 0',$,$,#{at},$,$,.ELEMENT.,0.)", s(&derived(STOREY, 0))));
            storeys.insert(0, (st, at, 0.0));
            storey_levels = vec![(0, 0.0)];
        }
        let all: Vec<u64> = storeys.values().map(|v| v.0).collect();
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{building},({}))", s(&derived(AGGREGATES, BUILDING)), refs(&all)));
        let storey_of = |level: Id| -> Id {
            if storeys.contains_key(&level) {
                return level;
            }
            let e = self.level(level).map_or(0.0, |l| l.elevation_mm);
            storey_levels.iter().rev().find(|(_, z)| *z <= e + 1e-6).or(storey_levels.first()).map_or(0, |(i, _)| *i)
        };

        let mut contained: BTreeMap<Id, Vec<u64>> = BTreeMap::new();
        let mut products: BTreeMap<Id, u64> = BTreeMap::new();
        let mut openings: BTreeMap<Id, (u64, f64, f64)> = BTreeMap::new(); // instance: (opening, width, height)
        let mut wall_uses: BTreeMap<(Id, String, bool), Vec<u64>> = BTreeMap::new(); // (type, offset, flipped): walls
        let mut floor_uses: BTreeMap<(Id, String), Vec<u64>> = BTreeMap::new();
        let skip = |ifc: &IfcParams| ifc.export == IfcExport::No;

        // walls: swept from their joined outline, with an opening element for each door or window in them
        let outlines = self.wall_outlines();
        let polygons = self.wall_polygons();
        for (id, e) in self.iter() {
            let Element::Wall(wl) = e else { continue };
            if skip(&wl.ifc) {
                continue;
            }
            let lv = storey_of(wl.base_level);
            let (_, storey_at, sz) = storeys[&lv];
            let (z0, z1) = self.wall_extent(wl);
            let ring: Vec<Point> = match outlines.get(&id) {
                Some(o) => o.to_vec(),
                None => match polygons.get(&id) {
                    Some(p) if p.len() >= 3 => p.clone(),
                    _ => continue,
                },
            };
            if z1 - z0 < 1.0 {
                continue;
            }
            // placed at its start, along the way it sets off: its outline and openings in that frame
            let path = wl.path();
            let (start, u) = path.at(0.0);
            let left = Point::new(-u.y, u.x);
            let local = |p: Point| Point::new((p.x - start.x) * u.x + (p.y - start.y) * u.y, (p.x - start.x) * left.x + (p.y - start.y) * left.y);
            let at = w.place(Some(storey_at), (start.x, start.y, z0 - sz), Some(u));
            let pts: Vec<u64> = path.pts.iter().map(|p| w.point(local(*p))).collect();
            let line = w.add(format!("IFCPOLYLINE(({}))", refs(&pts)));
            let axis = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Axis','Curve2D',(#{line}))", w.axis));
            let mut outline: Vec<Point> = ring.iter().map(|p| local(*p)).collect();
            if crate::geom::signed_area(&outline) < 0.0 {
                outline.reverse();
            }
            let profile = w.profile(&outline, &[]);
            let shape = w.swept(&[(profile, 0.0, z1 - z0)], &[axis]);
            let ty = self.wall_type(wl.wall_type);
            let tname = ty.map_or(String::new(), |t| t.name.clone());
            let class = if wl.ifc.predefined_type.is_empty() { ".STANDARD.".to_string() } else { format!(".{}.", wl.ifc.predefined_type.to_uppercase()) };
            let desc = if wl.comments.is_empty() { "$".into() } else { s(&wl.comments) };
            let tag = if wl.mark.is_empty() { "$".into() } else { s(&wl.mark) };
            let ent = w.add(format!("IFCWALL({},$,{},{desc},{},#{at},#{shape},{tag},{class})", own_guid(id, &wl.ifc), s(&tname), s(&tname)));
            contained.entry(lv).or_default().push(ent);
            products.insert(id, ent);
            let ext = ty.is_some_and(|t| t.function == WallFunction::Exterior);
            let (p1, p2) = (w.boolean("IsExternal", ext), w.boolean("LoadBearing", wl.structural));
            w.pset(ent, id, "Pset_WallCommon", &[p1, p2]);
            let (l, r) = self.wall_extents(wl);
            if ty.is_some_and(|t| !t.layers.is_empty()) {
                wall_uses.entry((wl.wall_type, real(l), wl.flipped)).or_default().push(ent);
            }
            // each opening: a box through the wall (10 mm past both faces, more across a curve) at its place along it
            for op in self.wall_openings(id) {
                let (mid, t) = path.at((op.u0 + op.u1) / 2.0);
                let width = op.u1 - op.u0;
                let bow = if path.is_straight() { 0.0 } else { width * 0.25 };
                let c = local(Point::new(mid.x + left.x * (l - r) / 2.0, mid.y + left.y * (l - r) / 2.0));
                let tl = Point::new(t.x * u.x + t.y * u.y, t.x * left.x + t.y * left.y);
                let o_at = w.place(Some(at), (c.x, c.y, op.z0 - z0), Some(tl));
                let rect = w.rectangle(Point::new(0.0, 0.0), width, l + r + 20.0 + 2.0 * bow);
                let o_shape = w.swept(&[(rect, 0.0, op.z1 - op.z0)], &[]);
                let opening = w.add(format!("IFCOPENINGELEMENT({},$,'Opening',$,$,#{o_at},#{o_shape},$,.OPENING.)", s(&derived(OPENING, op.instance))));
                w.add(format!("IFCRELVOIDSELEMENT({},$,$,$,#{ent},#{opening})", s(&derived(VOIDS, op.instance))));
                openings.insert(op.instance, (opening, width, op.z1 - op.z0));
            }
        }

        // floors: flat ones their regions swept down from the top, sloped ones their 3D mesh
        for (id, e) in self.iter() {
            let Element::Floor(f) = e else { continue };
            let lv = storey_of(f.level);
            let (_, storey_at, sz) = storeys[&lv];
            let Some(ft) = self.floor_type(f.floor_type) else { continue };
            let Some(surf) = self.floor_surface(f) else { continue };
            let t = ft.thickness_mm();
            let at = w.place(Some(storey_at), (0.0, 0.0, 0.0), None);
            let shape = if surf.rise.abs() < 1e-9 {
                let solids: Vec<(u64, f64, f64)> = f
                    .regions(0.5)
                    .iter()
                    .map(|r| {
                        let mut outer = r.outer.clone();
                        if crate::geom::signed_area(&outer) < 0.0 {
                            outer.reverse();
                        }
                        (w.profile(&outer, &r.holes), surf.base - t - sz, t)
                    })
                    .collect();
                if solids.is_empty() {
                    continue;
                }
                w.swept(&solids, &[])
            } else {
                let Some(set) = crate::mesh::floor_mesh(self, f).and_then(|v| w.faceset(&v, -sz)) else { continue };
                w.tessellated(&[set])
            };
            let ent = w.add(format!("IFCSLAB({},$,{},$,{},#{at},#{shape},$,.FLOOR.)", s(&derived(ELEMENT, id)), s(&ft.name), s(&ft.name)));
            contained.entry(lv).or_default().push(ent);
            products.insert(id, ent);
            let ext = ft.function == FloorFunction::Exterior;
            let (p1, p2) = (w.boolean("IsExternal", ext), w.boolean("LoadBearing", f.structural));
            w.pset(ent, id, "Pset_SlabCommon", &[p1, p2]);
            if !ft.layers.is_empty() {
                floor_uses.entry((f.floor_type, real(surf.base - sz))).or_default().push(ent);
            }
        }

        // roofs: their 3D mesh
        for (id, e) in self.iter() {
            let Element::Roof(r) = e else { continue };
            if skip(&r.ifc) {
                continue;
            }
            let lv = storey_of(r.level);
            let (_, storey_at, sz) = storeys[&lv];
            let Some(set) = crate::mesh::roof_mesh(self, r).and_then(|v| w.faceset(&v, -sz)) else { continue };
            let at = w.place(Some(storey_at), (0.0, 0.0, 0.0), None);
            let shape = w.tessellated(&[set]);
            let tname = self.roof_type(r.roof_type).map_or(String::new(), |t| t.name.clone());
            let tag = if r.mark.is_empty() { "$".into() } else { s(&r.mark) };
            let ent = w.add(format!("IFCROOF({},$,{},$,{},#{at},#{shape},{tag},.NOTDEFINED.)", own_guid(id, &r.ifc), s(&tname), s(&tname)));
            contained.entry(lv).or_default().push(ent);
            products.insert(id, ent);
        }

        // doors, windows and components: what the 3D view draws, glass in a see-through style
        let mut glass_style: Option<u64> = None;
        for (id, i) in self.instances() {
            if skip(&i.ifc) {
                continue;
            }
            let Some((cat, solid, glass)) = crate::mesh::instance_meshes(self, id) else { continue };
            let lv = storey_of(i.level);
            let (_, storey_at, sz) = storeys[&lv];
            let mut items = vec![];
            items.extend(w.faceset(&solid, -sz));
            if let Some(g) = w.faceset(&glass, -sz) {
                let style = *glass_style.get_or_insert_with(|| w.style("Glass", [190, 220, 235], 0.6));
                w.add(format!("IFCSTYLEDITEM(#{g},(#{style}),$)"));
                items.push(g);
            }
            if items.is_empty() {
                continue;
            }
            let at = w.place(Some(storey_at), (0.0, 0.0, 0.0), None);
            let shape = w.tessellated(&items);
            let name = self.instance_family(i).map(|(f, t)| format!("{}:{}", f.name, t.name)).unwrap_or_default();
            let (name, tag) = (s(&name), if i.mark.is_empty() { "$".into() } else { s(&i.mark) });
            let desc = if i.comments.is_empty() { "$".into() } else { s(&i.comments) };
            let g = own_guid(id, &i.ifc);
            // the door's or window's own size where its type has one (the opening is the rough size, frame and all)
            let own = self.instance_family(i).and_then(|(_, t)| Some((self.type_param(t, "Height")?.as_f64()?, self.type_param(t, "Width")?.as_f64()?)));
            let size = openings.get(&id).map(|(_, wd, h)| own.map_or((real(*h), real(*wd)), |(oh, ow)| (real(oh), real(ow))));
            let (h, wd) = size.clone().unwrap_or(("$".into(), "$".into()));
            let ent = match cat {
                FamilyCategory::Doors => w.add(format!("IFCDOOR({g},$,{name},{desc},{name},#{at},#{shape},{tag},{h},{wd},.DOOR.,.NOTDEFINED.,$)")),
                FamilyCategory::Windows => w.add(format!("IFCWINDOW({g},$,{name},{desc},{name},#{at},#{shape},{tag},{h},{wd},.WINDOW.,.NOTDEFINED.,$)")),
                c => w.add(format!("{}({g},$,{name},{desc},{name},#{at},#{shape},{tag},.NOTDEFINED.)", component_class(c))),
            };
            if let Some((opening, ..)) = openings.get(&id) {
                w.add(format!("IFCRELFILLSELEMENT({},$,$,$,#{opening},#{ent})", s(&derived(FILLS, id))));
            }
            contained.entry(lv).or_default().push(ent);
            products.insert(id, ent);
        }

        // layered types: a layer set per type, used by its walls from their left face (the exterior unless flipped)
        let mut sets: BTreeMap<Id, u64> = BTreeMap::new();
        for (k, ((tid, offset, flipped), ents)) in wall_uses.into_iter().enumerate() {
            let Some(t) = self.wall_type(tid) else { continue };
            let set = match (sets.get(&tid), flipped) {
                (Some(s), false) => *s,
                _ => {
                    let mut layers = t.layers.clone();
                    if flipped {
                        layers.reverse();
                    }
                    let set = w.layer_set(self, &t.name, &layers);
                    if !flipped {
                        sets.insert(tid, set);
                    }
                    set
                }
            };
            let usage = w.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS2.,.NEGATIVE.,{offset},$)"));
            w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),#{usage})", s(&derived(MATERIALS, k as u64)), refs(&ents)));
        }
        for (k, ((tid, top), ents)) in floor_uses.into_iter().enumerate() {
            let Some(t) = self.floor_type(tid) else { continue };
            let set = w.layer_set(self, &t.name, &t.layers);
            let usage = w.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS3.,.NEGATIVE.,{top},$)"));
            w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),#{usage})", s(&derived(MATERIALS, 1_000_000 + k as u64)), refs(&ents)));
        }

        // groups: an IfcGroup each, holding the products (and groups) in it
        let mut group_ents: BTreeMap<Id, u64> = BTreeMap::new();
        for (gid, g) in self.group_instances() {
            let tname = self.group_type(g.group_type).map_or(String::new(), |t| t.name.clone());
            let kind = if self.group_type(g.group_type).is_some_and(|t| t.kind == GroupKind::Detail) { "Detail Group" } else { "Model Group" };
            group_ents.insert(gid, w.add(format!("IFCGROUP({},$,{},$,{})", s(&derived(GROUP, gid)), s(&tname), s(kind))));
        }
        for (gid, g) in self.group_instances() {
            let members: Vec<u64> = g.live(self).into_iter().filter_map(|(_, m)| products.get(&m).or(group_ents.get(&m)).copied()).collect();
            if !members.is_empty() {
                w.add(format!("IFCRELASSIGNSTOGROUP({},$,$,$,({}),$,#{})", s(&derived(GROUP_MEMBERS, gid)), refs(&members), group_ents[&gid]));
            }
        }

        for (level, (storey, ..)) in &storeys {
            if let Some(items) = contained.get(level) {
                w.add(format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,({}),#{storey})", s(&derived(CONTAINS, *level)), refs(items)));
            }
        }

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let app = s(&format!("OpenArch BIM {}", env!("CARGO_PKG_VERSION")));
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');\n\
             FILE_NAME({},'{}',(''),(''),{app},{app},'');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
            s(&format!("{name}.ifc")),
            utc(now),
            w.out
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(f: &str, kind: &str) -> usize {
        f.lines().filter(|l| l.split_once('=').is_some_and(|(_, r)| r.starts_with(&format!("{kind}(")))).count()
    }

    /// Every `#n` a line refers to is an entity written in the file, and each entity once.
    fn check_references(f: &str) {
        let mut defined = std::collections::BTreeSet::new();
        for l in f.lines().filter(|l| l.starts_with('#')) {
            let n: u64 = l[1..l.find('=').unwrap()].parse().unwrap();
            assert!(defined.insert(n), "#{n} written twice");
        }
        for l in f.lines().filter(|l| l.starts_with('#')) {
            let body = &l[l.find('=').unwrap()..];
            let mut rest = body;
            while let Some(i) = rest.find('#') {
                let digits: String = rest[i + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let n: u64 = digits.parse().unwrap();
                    assert!(defined.contains(&n), "{l} refers to #{n}, which is not written");
                }
                rest = &rest[i + 1..];
            }
        }
    }

    /// Every entity written has as many attributes as the IFC4 schema gives it.
    fn check_attribute_counts(f: &str) {
        let schema: BTreeMap<&str, usize> = [
            ("IFCCARTESIANPOINT", 1), ("IFCDIRECTION", 1), ("IFCAXIS2PLACEMENT3D", 3), ("IFCAXIS2PLACEMENT2D", 2),
            ("IFCLOCALPLACEMENT", 2), ("IFCGEOMETRICREPRESENTATIONCONTEXT", 6), ("IFCGEOMETRICREPRESENTATIONSUBCONTEXT", 10),
            ("IFCSIUNIT", 4), ("IFCUNITASSIGNMENT", 1), ("IFCPROJECT", 9), ("IFCSITE", 14), ("IFCBUILDING", 12),
            ("IFCBUILDINGSTOREY", 10), ("IFCRELAGGREGATES", 6), ("IFCPOLYLINE", 1), ("IFCSHAPEREPRESENTATION", 4),
            ("IFCARBITRARYCLOSEDPROFILEDEF", 3), ("IFCARBITRARYPROFILEDEFWITHVOIDS", 4), ("IFCRECTANGLEPROFILEDEF", 5),
            ("IFCEXTRUDEDAREASOLID", 4), ("IFCPRODUCTDEFINITIONSHAPE", 3), ("IFCWALL", 9), ("IFCPROPERTYSINGLEVALUE", 4),
            ("IFCPROPERTYSET", 5), ("IFCRELDEFINESBYPROPERTIES", 6), ("IFCOPENINGELEMENT", 9), ("IFCRELVOIDSELEMENT", 6),
            ("IFCSLAB", 9), ("IFCCARTESIANPOINTLIST3D", 1), ("IFCTRIANGULATEDFACESET", 5), ("IFCROOF", 9), ("IFCCOLOURRGB", 4),
            ("IFCSURFACESTYLESHADING", 2), ("IFCSURFACESTYLE", 3), ("IFCSTYLEDITEM", 3), ("IFCDOOR", 13), ("IFCWINDOW", 13),
            ("IFCFURNITURE", 9), ("IFCSANITARYTERMINAL", 9), ("IFCLIGHTFIXTURE", 9), ("IFCBUILDINGELEMENTPROXY", 9),
            ("IFCRELFILLSELEMENT", 6), ("IFCMATERIAL", 3), ("IFCSTYLEDREPRESENTATION", 4), ("IFCMATERIALDEFINITIONREPRESENTATION", 4),
            ("IFCMATERIALLAYER", 7), ("IFCMATERIALLAYERSET", 3), ("IFCMATERIALLAYERSETUSAGE", 5), ("IFCRELASSOCIATESMATERIAL", 6),
            ("IFCGROUP", 5), ("IFCRELASSIGNSTOGROUP", 7), ("IFCRELCONTAINEDINSPATIALSTRUCTURE", 6),
        ]
        .into_iter()
        .collect();
        for l in f.lines().filter(|l| l.starts_with('#')) {
            let body = &l[l.find('=').unwrap() + 1..l.len() - 1];
            let (name, args) = body.split_at(body.find('(').unwrap());
            let want = *schema.get(name).unwrap_or_else(|| panic!("{name} is not in the schema table"));
            // top-level commas, outside strings and nested brackets
            let (mut depth, mut quoted, mut n) = (0, false, 1);
            for c in args[1..args.len() - 1].chars() {
                match c {
                    '\'' => quoted = !quoted,
                    '(' if !quoted => depth += 1,
                    ')' if !quoted => depth -= 1,
                    ',' if !quoted && depth == 0 => n += 1,
                    _ => {}
                }
            }
            assert_eq!(n, want, "{l}");
        }
    }

    #[test]
    fn doors_and_components_export_as_their_classes() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[0].0;
        let a = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), 2700.0).unwrap();
        let types = d.family_types().into_iter().map(|(i, t)| (i, t.family)).collect::<Vec<_>>();
        let cat = |d: &Document, f: Id| d.family(f).map(|f| f.category);
        let door = types.iter().find(|(_, f)| cat(&d, *f) == Some(FamilyCategory::Doors)).unwrap().0;
        d.place_on_wall(door, a, Point::new(1500.0, 0.0)).unwrap();
        let comp = types.iter().find(|(_, f)| !matches!(cat(&d, *f), Some(FamilyCategory::Doors | FamilyCategory::Windows) | None)).map(|t| t.0);
        if let Some(c) = comp {
            d.place_free(c, lvl, Point::new(2000.0, 2000.0), 0.0, 0.0).unwrap();
        }
        let f = d.export_ifc("Doors");
        assert_eq!(count(&f, "IFCDOOR"), 1);
        assert_eq!(count(&f, "IFCRELFILLSELEMENT"), 1);
        if comp.is_some() {
            let n = ["IFCFURNITURE", "IFCSANITARYTERMINAL", "IFCLIGHTFIXTURE", "IFCBUILDINGELEMENTPROXY"].iter().map(|k| count(&f, k)).sum::<usize>();
            assert_eq!(n, 1);
        }
        check_references(&f);
        check_attribute_counts(&f);
    }

    #[test]
    fn strings_reals_and_global_ids() {
        assert_eq!(s("It's a \\ path"), r"'It''s a \\ path'");
        assert_eq!(s("Café"), r"'Caf\X2\00E9\X0\'");
        assert_eq!(real(3.0), "3.");
        assert_eq!(real(-0.0), "0.");
        assert_eq!(real(2.5), "2.5");
        let g = derived(ELEMENT, 42);
        assert!(valid_guid(&g) && g == derived(ELEMENT, 42) && g != derived(ELEMENT, 43));
        assert!(valid_guid(&new_ifc_guid()));
        assert_eq!(utc(0), "1970-01-01T00:00:00");
    }

    #[test]
    fn a_project_exports_its_storeys_walls_openings_and_floors() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types().iter().find(|(_, t)| t.layers.len() > 1).unwrap().0;
        let a = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), 2700.0).unwrap();
        d.add_wall(wt, lvl, Point::new(5000.0, 0.0), Point::new(5000.0, 4000.0), 2700.0).unwrap();
        let ft = d.floor_types()[0].0;
        let sq = vec![Point::new(0.0, 0.0), Point::new(5000.0, 0.0), Point::new(5000.0, 4000.0), Point::new(0.0, 4000.0)];
        d.add_floor(ft, lvl, sq).unwrap();
        // a window in the first wall
        let win = d.family_types().into_iter().find(|(_, t)| t.name.contains("1200")).map(|(i, _)| i).expect("a window type");
        d.place_on_wall(win, a, Point::new(2500.0, 0.0)).unwrap();
        let (g, _) = d.create_group(&[a], Some("Bay"), None).unwrap();
        let f = d.export_ifc("Test");
        assert!(f.starts_with("ISO-10303-21;") && f.contains("FILE_SCHEMA(('IFC4'));") && f.trim_end().ends_with("END-ISO-10303-21;"));
        assert_eq!(count(&f, "IFCPROJECT"), 1);
        assert_eq!(count(&f, "IFCBUILDINGSTOREY"), d.levels().iter().filter(|(_, l)| l.building_story).count());
        assert_eq!(count(&f, "IFCWALL"), 2);
        assert_eq!(count(&f, "IFCSLAB"), 1);
        let floor_layers = usize::from(!d.floor_type(ft).unwrap().layers.is_empty());
        assert_eq!(count(&f, "IFCMATERIALLAYERSETUSAGE"), 1 + floor_layers, "one per wall type and side, one per floor type");
        assert_eq!(count(&f, "IFCWINDOW"), 1);
        assert_eq!(count(&f, "IFCOPENINGELEMENT"), 1);
        assert_eq!(count(&f, "IFCOPENINGELEMENT"), count(&f, "IFCRELVOIDSELEMENT"));
        assert_eq!(count(&f, "IFCOPENINGELEMENT"), count(&f, "IFCRELFILLSELEMENT"));
        assert_eq!(count(&f, "IFCGROUP"), 1);
        // the wall keeps its own GlobalId
        let guid = match d.get(a) {
            Some(Element::Wall(w)) => w.ifc.guid.clone(),
            _ => unreachable!(),
        };
        assert!(f.contains(&format!("IFCWALL('{guid}'")));
        check_references(&f);
        check_attribute_counts(&f);
        let _ = g;
    }
}
