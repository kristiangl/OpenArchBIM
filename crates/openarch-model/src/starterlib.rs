//! The starter library: plumbing fixtures, casework, appliances and furniture every new project has, so a house can
//! be fitted out without making families first. Each is a plain free-standing family drawn from boxes and polygons
//! on reference planes; the sizes that vary (width, depth, height) are type parameters that drive those planes, so
//! a new size is a new type. They open in the family editor like any other family.
//!
//! The insertion point is the middle of the back edge (the side that goes against a wall) on the floor; the
//! family runs forward along -y. Space turns it while placing. Sizes are common Australian ones.

use std::collections::BTreeMap;

use crate::famgeom::{Anchor, Axis, Coord, FamilyBody, Form, FormShape, Loop, Node, PlaneId, RefKind, RefPlane, Visibility, PLANE_FB, PLANE_LEVEL, PLANE_LR};
use crate::family::{Family, FamilyCategory, HostKind, ParamDef, ParamKind, ParamValue, PlanSymbol, StarterFamily};

/// One side of a box: a reference plane and a fixed distance from it.
type At = (PlaneId, f64);

struct Builder {
    body: FamilyBody,
    params: Vec<ParamDef>,
    /// The planes the width and depth drive: left, right, front (the back is the Center (Front/Back) plane).
    l: PlaneId,
    r: PlaneId,
    f: PlaneId,
}

impl Builder {
    /// A family whose width (centred) and depth (forward from the back) are the type's Width and Depth.
    fn new(width: f64, depth: f64, height: Option<f64>) -> Builder {
        let mut body = FamilyBody::template();
        let plane = |body: &mut FamilyBody, name: &str, axis: Axis, from: PlaneId, by: &str| -> PlaneId {
            let id = body.take_id();
            body.planes.push(RefPlane { id, name: name.into(), axis, anchor: Anchor::Offset { from, by: by.into() }, ref_kind: RefKind::Strong, wall_closure: false });
            id
        };
        let l = plane(&mut body, "Left", Axis::X, PLANE_LR, "-[Width] / 2");
        let r = plane(&mut body, "Right", Axis::X, PLANE_LR, "[Width] / 2");
        let f = plane(&mut body, "Front", Axis::Y, PLANE_FB, "-[Depth]");
        let mut params = vec![param("Width", width), param("Depth", depth)];
        if let Some(h) = height {
            params.push(param("Height", h));
        }
        Builder { body, params, l, r, f }
    }

    fn left(&self, off: f64) -> At {
        (self.l, off)
    }
    fn right(&self, off: f64) -> At {
        (self.r, off)
    }
    fn front(&self, off: f64) -> At {
        (self.f, off)
    }
    fn back(&self, off: f64) -> At {
        (PLANE_FB, off)
    }
    fn mid(&self, off: f64) -> At {
        (PLANE_LR, off)
    }

    fn form(&mut self, name: &str, profile: Loop, z0: &str, z1: &str, material: &str, void: bool) -> u32 {
        self.body.add_form(Form {
            id: 0,
            name: name.into(),
            shape: FormShape::Extrusion { plane: PLANE_LEVEL, start: z0.into(), end: z1.into(), start_lock: None, end_lock: None, profile: vec![profile] },
            void,
            cuts_host: false,
            cut_by: vec![],
            opening: false,
            material: material.into(),
            material_param: None,
            subcategory: String::new(),
            vis: Visibility::default(),
        })
    }

    /// A box between two x sides and two y sides, from z0 to z1 (numbers or formulas).
    fn boxed(&mut self, name: &str, x0: At, x1: At, y0: At, y1: At, z0: &str, z1: &str, material: &str) -> u32 {
        let c = |a: At| Coord::from(a.0, a.1);
        let nodes = vec![Node::new(c(x0), c(y0)), Node::new(c(x1), c(y0)), Node::new(c(x1), c(y1)), Node::new(c(x0), c(y1))];
        let profile = Loop { nodes, bulges: vec![], closed: true, shapes: vec![] };
        self.form(name, profile, z0, z1, material, false)
    }

    /// A polygon of `sides` round an ellipse `w` by `d` across, centred `cx` from the middle and `cy` from the back.
    fn oval(&mut self, name: &str, cx: f64, cy: f64, w: f64, d: f64, sides: usize, z0: &str, z1: &str, material: &str, void: bool) -> u32 {
        let r = |v: f64| (v * 100.0).round() / 100.0;
        let nodes = (0..sides)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / sides as f64;
                Node::new(Coord::from(PLANE_LR, r(cx + w / 2.0 * a.cos())), Coord::from(PLANE_FB, r(cy + d / 2.0 * a.sin())))
            })
            .collect();
        self.form(name, Loop { nodes, bulges: vec![], closed: true, shapes: vec![] }, z0, z1, material, void)
    }

    /// A void box that cuts the solids listed.
    fn cut(&mut self, name: &str, x0: At, x1: At, y0: At, y1: At, z0: &str, z1: &str, solids: &[u32]) -> u32 {
        let id = self.boxed(name, x0, x1, y0, y1, z0, z1, "");
        self.voids(id, solids)
    }

    /// Mark form `id` as a void cutting `solids`.
    fn voids(&mut self, id: u32, solids: &[u32]) -> u32 {
        for f in &mut self.body.forms {
            if f.id == id {
                f.void = true;
                f.material.clear();
            }
            if solids.contains(&f.id) {
                f.cut_by.push(id);
            }
        }
        id
    }

    fn finish(self, name: &str, category: FamilyCategory, types: &[(&str, &[(&str, f64)])]) -> StarterFamily {
        StarterFamily {
            family: Family {
                name: name.into(),
                category,
                host: HostKind::None,
                plan_symbol: PlanSymbol::None,
                params: self.params,
                built_in: false,
                catalog: vec![],
                settings: Default::default(),
                source_path: None,
                loaded_at: None,
                body: self.body,
            },
            types: types.iter().map(|(t, vals)| (t.to_string(), vals.iter().map(|(k, v)| (k.to_string(), ParamValue::Number(*v))).collect::<BTreeMap<_, _>>())).collect(),
        }
    }
}

fn param(name: &str, default: f64) -> ParamDef {
    ParamDef { name: name.into(), param_kind: ParamKind::Length, instance: false, group: "Dimensions".into(), formula: None, reporting: false, shared_guid: None, choices: vec![], discipline: "common".into(), tooltip: String::new(), locked: false, family_category: None, default: ParamValue::Number(default) }
}

fn wd(w: f64, d: f64) -> [(&'static str, f64); 2] {
    [("Width", w), ("Depth", d)]
}

/// Every family of the starter library.
pub fn starter_library() -> Vec<StarterFamily> {
    vec![
        base_cabinet(),
        wall_cabinet(),
        tall_cabinet(),
        island_bench(),
        robe_shelf(),
        toilet(),
        vanity(),
        bath(),
        shower(),
        kitchen_sink(),
        laundry_trough(),
        refrigerator(),
        dishwasher(),
        cooktop(),
        washing_machine(),
        hot_water_unit(),
        bed(),
        bedside_table(),
        sofa(),
        dining_table(),
        dining_chair(),
        coffee_table(),
        desk(),
    ]
}

// ---- casework ---------------------------------------------------------------------------------------------------

fn base_cabinet() -> StarterFamily {
    // kick 150 set back 50, carcass to 870, a 30 stone top running 20 past the doors
    let mut b = Builder::new(600.0, 600.0, None);
    b.boxed("Kick", b.left(0.0), b.right(0.0), b.front(50.0), b.back(0.0), "0", "150", "Laminate");
    b.boxed("Carcass", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "150", "870", "Laminate");
    b.boxed("Benchtop", b.left(0.0), b.right(0.0), b.front(-20.0), b.back(0.0), "870", "900", "Stone");
    let t: Vec<(String, [(&str, f64); 2])> = [300.0, 450.0, 600.0, 900.0, 1200.0].iter().map(|w| (format!("{w:.0} x 600"), wd(*w, 600.0))).collect();
    let types: Vec<(&str, &[(&str, f64)])> = t.iter().map(|(n, v)| (n.as_str(), &v[..])).collect();
    b.finish("Base Cabinet", FamilyCategory::Casework, &types)
}

fn wall_cabinet() -> StarterFamily {
    // overhead cupboards from 1500 above the floor
    let mut b = Builder::new(600.0, 350.0, Some(700.0));
    b.boxed("Cabinet", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "1500", "1500 + [Height]", "Laminate");
    b.finish("Wall Cabinet", FamilyCategory::Casework, &[("600 x 350", &[("Width", 600.0), ("Depth", 350.0), ("Height", 700.0)]), ("900 x 350", &[("Width", 900.0), ("Depth", 350.0), ("Height", 700.0)]), ("1200 x 350", &[("Width", 1200.0), ("Depth", 350.0), ("Height", 700.0)])])
}

fn tall_cabinet() -> StarterFamily {
    let mut b = Builder::new(600.0, 600.0, Some(2100.0));
    b.boxed("Kick", b.left(0.0), b.right(0.0), b.front(50.0), b.back(0.0), "0", "150", "Laminate");
    b.boxed("Cabinet", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "150", "[Height]", "Laminate");
    b.finish("Tall Cabinet", FamilyCategory::Casework, &[("Pantry 600", &[("Width", 600.0), ("Depth", 600.0), ("Height", 2100.0)]), ("Pantry 900", &[("Width", 900.0), ("Depth", 600.0), ("Height", 2100.0)]), ("Linen 900 x 450", &[("Width", 900.0), ("Depth", 450.0), ("Height", 2100.0)])])
}

fn island_bench() -> StarterFamily {
    // a bank of cupboards with a top overhanging 300 on the seating side (the back)
    let mut b = Builder::new(2400.0, 1000.0, None);
    b.boxed("Kick", b.left(50.0), b.right(-50.0), b.front(50.0), b.back(-350.0), "0", "150", "Laminate");
    b.boxed("Carcass", b.left(0.0), b.right(0.0), b.front(0.0), b.back(-300.0), "150", "870", "Laminate");
    b.boxed("Benchtop", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "870", "900", "Stone");
    b.finish("Island Bench", FamilyCategory::Casework, &[("2400 x 1000", &wd(2400.0, 1000.0)), ("2550 x 1100", &wd(2550.0, 1100.0)), ("3000 x 1200", &wd(3000.0, 1200.0))])
}

fn robe_shelf() -> StarterFamily {
    // a robe's shelf at 1700 with the hanging rail below it
    let mut b = Builder::new(1200.0, 450.0, None);
    b.boxed("Shelf", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "1700", "1725", "Laminate");
    b.boxed("Rail", b.left(0.0), b.right(0.0), b.back(-240.0), b.back(-210.0), "1630", "1660", "Stainless Steel");
    b.finish("Robe Shelf and Rail", FamilyCategory::Casework, &[("900", &wd(900.0, 450.0)), ("1200", &wd(1200.0, 450.0)), ("1800", &wd(1800.0, 450.0)), ("2400", &wd(2400.0, 450.0))])
}

// ---- plumbing fixtures ------------------------------------------------------------------------------------------

fn toilet() -> StarterFamily {
    let mut b = Builder::new(400.0, 680.0, None);
    b.boxed("Cistern", b.mid(-200.0), b.mid(200.0), b.back(-180.0), b.back(0.0), "350", "800", "Vitreous China");
    b.oval("Pan", 0.0, -420.0, 360.0, 480.0, 24, "0", "380", "Vitreous China", false);
    b.oval("Seat", 0.0, -430.0, 370.0, 460.0, 24, "380", "400", "Vitreous China", false);
    b.finish("Toilet Suite", FamilyCategory::PlumbingFixtures, &[("Close Coupled", &wd(400.0, 680.0))])
}

fn vanity() -> StarterFamily {
    let mut b = Builder::new(750.0, 460.0, None);
    let carcass = b.boxed("Carcass", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "150", "850", "Laminate");
    b.boxed("Kick", b.left(0.0), b.right(0.0), b.front(50.0), b.back(0.0), "0", "150", "Laminate");
    let bowl = b.oval("Basin", 0.0, -250.0, 480.0, 340.0, 24, "720", "860", "", false);
    b.voids(bowl, &[carcass]);
    b.finish("Vanity with Basin", FamilyCategory::PlumbingFixtures, &[("600", &wd(600.0, 460.0)), ("750", &wd(750.0, 460.0)), ("900", &wd(900.0, 460.0)), ("1200", &wd(1200.0, 460.0))])
}

fn bath() -> StarterFamily {
    let mut b = Builder::new(1675.0, 760.0, None);
    let tub = b.boxed("Bath", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "550", "Vitreous China");
    b.cut("Tub", b.left(70.0), b.right(-70.0), b.front(70.0), b.back(-70.0), "150", "560", &[tub]);
    b.finish("Bath", FamilyCategory::PlumbingFixtures, &[("1525 x 760", &wd(1525.0, 760.0)), ("1675 x 760", &wd(1675.0, 760.0)), ("1800 x 800", &wd(1800.0, 800.0))])
}

fn shower() -> StarterFamily {
    // a tray with frameless glass on the front and the left side, open where it meets the walls
    let mut b = Builder::new(900.0, 900.0, None);
    b.boxed("Tray", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "40", "Vitreous China");
    b.boxed("Screen Front", b.left(0.0), b.right(0.0), b.front(0.0), b.front(10.0), "40", "2000", "Glass");
    b.boxed("Screen Side", b.left(0.0), b.left(10.0), b.front(0.0), b.back(0.0), "40", "2000", "Glass");
    b.finish("Shower", FamilyCategory::PlumbingFixtures, &[("900 x 900", &wd(900.0, 900.0)), ("1000 x 1000", &wd(1000.0, 1000.0)), ("1200 x 900", &wd(1200.0, 900.0))])
}

fn kitchen_sink() -> StarterFamily {
    // an inset sink: its rim on the benchtop and the bowls cut through it, set 50 in from the back of the bench
    let mut b = Builder::new(1020.0, 500.0, None);
    let rim = b.boxed("Rim", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "900", "905", "Stainless Steel");
    b.cut("Bowl", b.left(40.0), b.mid(40.0), b.front(40.0), b.back(-60.0), "880", "910", &[rim]);
    b.cut("Half Bowl", b.mid(70.0), b.right(-200.0), b.front(40.0), b.back(-60.0), "880", "910", &[rim]);
    b.finish("Kitchen Sink", FamilyCategory::PlumbingFixtures, &[("1 3/4 Bowl 1020", &wd(1020.0, 500.0)), ("Double Bowl 1080", &wd(1080.0, 500.0))])
}

fn laundry_trough() -> StarterFamily {
    let mut b = Builder::new(600.0, 500.0, None);
    let cab = b.boxed("Cabinet", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "900", "Stainless Steel");
    b.cut("Trough", b.left(50.0), b.right(-50.0), b.front(50.0), b.back(-70.0), "600", "910", &[cab]);
    b.finish("Laundry Trough", FamilyCategory::PlumbingFixtures, &[("45 L 600", &wd(600.0, 500.0)), ("45 L 700", &wd(700.0, 500.0))])
}

// ---- appliances ---------------------------------------------------------------------------------------------------

fn refrigerator() -> StarterFamily {
    let mut b = Builder::new(900.0, 700.0, Some(1800.0));
    b.boxed("Body", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "[Height]", "Stainless Steel");
    b.finish("Refrigerator", FamilyCategory::SpecialtyEquipment, &[("Single Door 700", &[("Width", 700.0), ("Depth", 700.0), ("Height", 1750.0)]), ("French Door 900", &[("Width", 900.0), ("Depth", 700.0), ("Height", 1800.0)])])
}

fn dishwasher() -> StarterFamily {
    let mut b = Builder::new(600.0, 580.0, None);
    b.boxed("Body", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "850", "Stainless Steel");
    b.finish("Dishwasher", FamilyCategory::SpecialtyEquipment, &[("600", &wd(600.0, 580.0))])
}

fn cooktop() -> StarterFamily {
    // sits on the benchtop, set 50 in from the back of the bench
    let mut b = Builder::new(600.0, 510.0, None);
    b.boxed("Glass", b.left(0.0), b.right(0.0), b.front(0.0), b.back(-50.0), "900", "906", "Glass");
    for (x, y, d) in [(-150.0, -170.0, 180.0), (150.0, -170.0, 160.0), (-150.0, -390.0, 160.0), (150.0, -390.0, 220.0)] {
        b.oval("Burner", x, y, d, d, 16, "906", "916", "Steel", false);
    }
    b.finish("Cooktop", FamilyCategory::SpecialtyEquipment, &[("600 Gas", &wd(600.0, 560.0)), ("900 Gas", &wd(900.0, 560.0))])
}

fn washing_machine() -> StarterFamily {
    let mut b = Builder::new(600.0, 600.0, None);
    b.boxed("Body", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "850", "Laminate");
    b.oval("Door", 0.0, -605.0, 320.0, 10.0, 16, "350", "670", "Glass", false);
    b.finish("Washing Machine", FamilyCategory::SpecialtyEquipment, &[("Front Loader 600", &wd(600.0, 600.0))])
}

fn hot_water_unit() -> StarterFamily {
    // an instantaneous gas unit on an outside wall
    let mut b = Builder::new(350.0, 180.0, None);
    b.boxed("Unit", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "1100", "1700", "Laminate");
    b.finish("Hot Water Unit", FamilyCategory::MechanicalEquipment, &[("Instantaneous Gas", &wd(350.0, 180.0))])
}

// ---- furniture ----------------------------------------------------------------------------------------------------

fn bed() -> StarterFamily {
    // the head against the wall at the back
    let mut b = Builder::new(1530.0, 2030.0, None);
    b.boxed("Headboard", b.left(0.0), b.right(0.0), b.back(-60.0), b.back(0.0), "0", "1000", "Timber");
    b.boxed("Base", b.left(0.0), b.right(0.0), b.front(0.0), b.back(-60.0), "0", "300", "Timber");
    b.boxed("Mattress", b.left(10.0), b.right(-10.0), b.front(10.0), b.back(-60.0), "300", "550", "Fabric");
    b.boxed("Pillows", b.left(60.0), b.right(-60.0), b.back(-500.0), b.back(-100.0), "550", "650", "Fabric");
    b.finish("Bed", FamilyCategory::Furniture, &[("Single", &wd(920.0, 1940.0)), ("King Single", &wd(1070.0, 2090.0)), ("Double", &wd(1380.0, 1940.0)), ("Queen", &wd(1530.0, 2090.0)), ("King", &wd(1830.0, 2090.0))])
}

fn bedside_table() -> StarterFamily {
    let mut b = Builder::new(450.0, 400.0, None);
    b.boxed("Table", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "550", "Timber");
    b.finish("Bedside Table", FamilyCategory::Furniture, &[("450 x 400", &wd(450.0, 400.0))])
}

fn sofa() -> StarterFamily {
    let mut b = Builder::new(2100.0, 900.0, None);
    b.boxed("Seat", b.left(150.0), b.right(-150.0), b.front(0.0), b.back(-200.0), "0", "450", "Fabric");
    b.boxed("Back", b.left(0.0), b.right(0.0), b.back(-200.0), b.back(0.0), "0", "800", "Fabric");
    b.boxed("Arm Left", b.left(0.0), b.left(150.0), b.front(0.0), b.back(-200.0), "0", "620", "Fabric");
    b.boxed("Arm Right", b.right(-150.0), b.right(0.0), b.front(0.0), b.back(-200.0), "0", "620", "Fabric");
    b.finish("Sofa", FamilyCategory::Furniture, &[("2 Seater", &wd(1600.0, 900.0)), ("3 Seater", &wd(2100.0, 900.0))])
}

fn dining_table() -> StarterFamily {
    // its middle at the insertion point: the back plane is the table's middle line
    let mut b = Builder::new(1800.0, 900.0, None);
    b.body.planes.iter_mut().filter(|p| p.name == "Front").for_each(|p| p.anchor = Anchor::Offset { from: PLANE_FB, by: "-[Depth] / 2".into() });
    let back = b.body.take_id();
    b.body.planes.push(RefPlane { id: back, name: "Back".into(), axis: Axis::Y, anchor: Anchor::Offset { from: PLANE_FB, by: "[Depth] / 2".into() }, ref_kind: RefKind::Strong, wall_closure: false });
    b.boxed("Top", b.left(0.0), b.right(0.0), b.front(0.0), (back, 0.0), "720", "750", "Timber");
    for (x, y) in [(b.left(50.0), b.front(50.0)), (b.right(-100.0), b.front(50.0)), (b.left(50.0), (back, -100.0)), (b.right(-100.0), (back, -100.0))] {
        b.boxed("Leg", x, (x.0, x.1 + 50.0), y, (y.0, y.1 + 50.0), "0", "720", "Timber");
    }
    b.finish("Dining Table", FamilyCategory::Furniture, &[("1500 x 900", &wd(1500.0, 900.0)), ("1800 x 900", &wd(1800.0, 900.0)), ("2100 x 1000", &wd(2100.0, 1000.0))])
}

fn dining_chair() -> StarterFamily {
    let mut b = Builder::new(450.0, 500.0, None);
    b.boxed("Seat", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "430", "460", "Timber");
    b.boxed("Back", b.left(0.0), b.right(0.0), b.back(-30.0), b.back(0.0), "460", "850", "Timber");
    for (x, y) in [(b.left(0.0), b.front(0.0)), (b.right(-30.0), b.front(0.0)), (b.left(0.0), b.back(-30.0)), (b.right(-30.0), b.back(-30.0))] {
        b.boxed("Leg", x, (x.0, x.1 + 30.0), y, (y.0, y.1 + 30.0), "0", "430", "Timber");
    }
    b.finish("Dining Chair", FamilyCategory::Furniture, &[("450 x 500", &wd(450.0, 500.0))])
}

fn coffee_table() -> StarterFamily {
    let mut b = Builder::new(1200.0, 600.0, None);
    b.boxed("Table", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "400", "Timber");
    b.finish("Coffee Table", FamilyCategory::Furniture, &[("1200 x 600", &wd(1200.0, 600.0))])
}

fn desk() -> StarterFamily {
    let mut b = Builder::new(1500.0, 700.0, None);
    b.boxed("Top", b.left(0.0), b.right(0.0), b.front(0.0), b.back(0.0), "695", "720", "Timber");
    b.boxed("Side Left", b.left(0.0), b.left(25.0), b.front(0.0), b.back(0.0), "0", "695", "Timber");
    b.boxed("Side Right", b.right(-25.0), b.right(0.0), b.front(0.0), b.back(0.0), "0", "695", "Timber");
    b.finish("Study Desk", FamilyCategory::Furniture, &[("1200 x 600", &wd(1200.0, 600.0)), ("1500 x 700", &wd(1500.0, 700.0))])
}

#[cfg(test)]
mod tests {
    use crate::doc::Document;
    use crate::geom::Point;

    #[test]
    fn every_starter_type_places_at_its_own_size() {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let names: Vec<String> = super::starter_library().into_iter().map(|f| f.family.name).collect();
        let mut placed = 0;
        for (tid, t) in d.family_types().into_iter().map(|(id, t)| (id, t.clone())).collect::<Vec<_>>() {
            let fam = d.family(t.family).unwrap().clone();
            if !names.contains(&fam.name) {
                continue;
            }
            let id = d.place_free(tid, lvl, Point::new(0.0, 0.0), 0.0, 0.0).unwrap_or_else(|e| panic!("{} {}: {e}", fam.name, t.name));
            let solved = d.instance_solved(id).unwrap();
            let s = solved.as_ref().as_ref().unwrap_or_else(|e| panic!("{} {}: {e}", fam.name, t.name));
            assert!(s.warnings.is_empty(), "{} {}: {:?}", fam.name, t.name, s.warnings);
            assert!(s.forms.iter().any(|f| !f.void && !f.solid.tris.is_empty()), "{} {} has no solid", fam.name, t.name);
            // the solids span the type's width
            let w = t.values.get("Width").and_then(|v| v.as_f64()).unwrap();
            let xs: Vec<f64> = s.forms.iter().filter(|f| !f.void).flat_map(|f| f.solid.tris.iter().flat_map(|tr| tr.iter().map(|p| p[0]))).collect();
            let span = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
            assert!((span - w).abs() < 1.0, "{} {}: {span} wide, not {w}", fam.name, t.name);
            placed += 1;
        }
        assert!(placed >= 50, "{placed}");
    }
}
