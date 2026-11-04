//! Colour fill schemes: rooms, or one area scheme's areas, filled in a plan by the value of a parameter (or by
//! ranges of a number such as Area), and the colour fill legends that list a view's colours.
//!
//! A scheme belongs to a category (Rooms, or the areas of one area scheme); a plan shows one scheme through its
//! view settings (`ViewSettings::color_scheme`, which a view template can control). The entries a scheme stores are
//! the colours chosen so far; a value that turns up later gets the next colour of the palette until it is given one.

use serde::{Deserialize, Serialize};

use crate::geom::Region;
use crate::view::ViewKind;
use crate::{Document, Element, Id, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ColorCategory {
    Rooms,
    Areas { scheme: Id },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorEntry {
    /// By value: the parameter's value. By range: unused.
    #[serde(default)]
    pub value: String,
    /// By range: the lower bound (m² for areas, mm for lengths); the entry runs up to the next one.
    #[serde(default)]
    pub at_least: Option<f64>,
    pub color: [u8; 3],
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool { true }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorFillScheme {
    pub name: String,
    pub category: ColorCategory,
    /// The legend's heading.
    #[serde(default)]
    pub title: String,
    /// The parameter that picks the colour ("Name", "Department", "Area Type", "Area"...).
    pub parameter: String,
    #[serde(default)]
    pub by_range: bool,
    #[serde(default)]
    pub entries: Vec<ColorEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorFillLegend {
    pub view: Id,
    /// Top left corner of the legend, in model coordinates.
    pub at: Point,
    #[serde(default)]
    pub pinned: bool,
}

/// The colours given out in turn to values without one.
pub const PALETTE: [[u8; 3]; 16] = [
    [166, 206, 227], [178, 223, 138], [251, 154, 153], [253, 191, 111], [202, 178, 214], [255, 255, 153], [141, 211, 199], [190, 186, 218],
    [251, 128, 114], [128, 177, 211], [179, 222, 105], [252, 205, 229], [217, 217, 217], [188, 128, 189], [204, 235, 197], [255, 237, 111],
];

/// A parameter a scheme can colour by: its name, and whether it is a number (ranges allowed).
pub fn color_parameters(category: ColorCategory) -> &'static [(&'static str, bool)] {
    match category {
        ColorCategory::Rooms => &[("Name", false), ("Number", false), ("Department", false), ("Occupancy", false), ("Occupant", false), ("Base Finish", false), ("Floor Finish", false), ("Wall Finish", false), ("Ceiling Finish", false), ("Comments", false), ("Area", true), ("Perimeter", true)],
        ColorCategory::Areas { .. } => &[("Name", false), ("Number", false), ("Area Type", false), ("Comments", false), ("Area", true), ("Perimeter", true)],
    }
}

/// What a room or area has for a scheme's parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorValue { Text(String), Number(f64) }

/// One coloured room or area: where it is, and its entry's label and colour.
#[derive(Debug, Clone)]
pub struct ColorFill { pub element: Id, pub region: Region, pub label: String, pub color: [u8; 3] }

/// A legend row (or a row of the Edit Color Scheme table): label, colour, shown, and whether anything has it.
#[derive(Debug, Clone, Serialize)]
pub struct ColorRow { pub label: String, pub value: String, pub at_least: Option<f64>, pub color: [u8; 3], pub visible: bool, pub in_use: bool }

fn fmt_num(v: f64) -> String { let s = format!("{v:.2}"); s.trim_end_matches('0').trim_end_matches('.').to_string() }

impl ColorFillScheme {
    fn unit(&self) -> &'static str { if self.parameter == "Area" { " m\u{b2}" } else { "" } }
    /// The range entries in order, with a first one from nothing when none starts there.
    fn ranges(&self) -> Vec<ColorEntry> {
        let mut r: Vec<ColorEntry> = self.entries.iter().filter(|e| e.at_least.is_some()).cloned().collect();
        r.sort_by(|a, b| a.at_least.unwrap().total_cmp(&b.at_least.unwrap()));
        if r.is_empty() {
            let steps: &[f64] = if self.parameter == "Area" { &[0.0, 10.0, 20.0, 50.0, 100.0] } else { &[0.0, 10000.0, 20000.0, 40000.0] };
            r = steps.iter().enumerate().map(|(i, s)| ColorEntry { value: String::new(), at_least: Some(*s), color: PALETTE[i % PALETTE.len()], visible: true }).collect();
        }
        r
    }
    fn range_label(&self, r: &[ColorEntry], i: usize) -> String {
        let lo = r[i].at_least.unwrap();
        match r.get(i + 1) {
            Some(n) if i == 0 && lo <= 0.0 => format!("Less than {}{}", fmt_num(n.at_least.unwrap()), self.unit()),
            Some(n) => format!("{}{} to {}{}", fmt_num(lo), self.unit(), fmt_num(n.at_least.unwrap()), self.unit()),
            None => format!("{}{} or more", fmt_num(lo), self.unit()),
        }
    }
}

impl Document {
    pub fn color_fill_schemes(&self) -> Vec<(Id, &ColorFillScheme)> {
        self.iter().filter_map(|(i, e)| if let Element::ColorFillScheme(s) = e { Some((i, s)) } else { None }).collect()
    }
    pub fn color_fill_scheme(&self, id: Id) -> Option<&ColorFillScheme> { match self.get(id)? { Element::ColorFillScheme(s) => Some(s), _ => None } }
    pub fn color_fill_legends(&self, view: Id) -> Vec<(Id, &ColorFillLegend)> {
        self.iter().filter_map(|(i, e)| match e { Element::ColorFillLegend(l) if l.view == view => Some((i, l)), _ => None }).collect()
    }

    /// What a view can colour: rooms in a floor plan, its scheme's areas in an area plan.
    pub fn color_category_of_view(&self, view: Id) -> Option<ColorCategory> {
        let v = self.view(view)?;
        match v.view_kind {
            ViewKind::FloorPlan | ViewKind::StructuralPlan | ViewKind::Callout if v.level.is_some() => Some(ColorCategory::Rooms),
            ViewKind::AreaPlan => v.area_scheme.map(|scheme| ColorCategory::Areas { scheme }),
            _ => None,
        }
    }

    /// The scheme a view shows (its own, or its template's), when it suits the view.
    pub fn view_color_scheme(&self, view: Id) -> Option<Id> {
        let id = self.effective_settings(view)?.color_scheme?;
        let s = self.color_fill_scheme(id)?;
        (Some(s.category) == self.color_category_of_view(view)).then_some(id)
    }

    pub fn category_label(&self, c: ColorCategory) -> String {
        match c {
            ColorCategory::Rooms => "Rooms".into(),
            ColorCategory::Areas { scheme } => format!("Areas ({})", self.area_scheme(scheme).map_or("?", |s| s.name.as_str())),
        }
    }

    /// The colour schemes of a category, with a first one made when there are none. Returns its id.
    pub fn ensure_color_scheme(&mut self, category: ColorCategory) -> Result<Id, String> {
        if let Some((id, _)) = self.color_fill_schemes().into_iter().find(|(_, s)| s.category == category) { return Ok(id); }
        if let ColorCategory::Areas { scheme } = category { self.area_scheme(scheme).ok_or("Unknown area scheme.")?; }
        let title = match category { ColorCategory::Rooms => "Room Legend".to_string(), ColorCategory::Areas { .. } => "Area Legend".to_string() };
        self.transact("Color scheme", |tx| Ok(tx.add(Element::ColorFillScheme(ColorFillScheme { name: "Scheme 1".into(), category, title, parameter: "Name".into(), by_range: false, entries: vec![] }))))
    }

    /// A copy of a scheme under a new name.
    pub fn duplicate_color_scheme(&mut self, id: Id, name: &str) -> Result<Id, String> {
        let mut s = self.color_fill_scheme(id).ok_or("Unknown colour scheme.")?.clone();
        let name = name.trim();
        if name.is_empty() { return Err("A colour scheme needs a name.".into()); }
        if self.color_fill_schemes().iter().any(|(_, o)| o.category == s.category && o.name == name) { return Err(format!("There is already a colour scheme called {name}.")); }
        s.name = name.into();
        self.transact("Duplicate color scheme", |tx| Ok(tx.add(Element::ColorFillScheme(s))))
    }

    /// Change a scheme (name, title, parameter, by range and its entries). Changing the parameter starts the
    /// entries again.
    pub fn set_color_scheme(&mut self, id: Id, mut s: ColorFillScheme) -> Result<(), String> {
        let old = self.color_fill_scheme(id).ok_or("Unknown colour scheme.")?.clone();
        s.category = old.category;
        s.name = s.name.trim().to_string();
        if s.name.is_empty() { return Err("A colour scheme needs a name.".into()); }
        if self.color_fill_schemes().iter().any(|(o, x)| *o != id && x.category == s.category && x.name == s.name) { return Err(format!("There is already a colour scheme called {}.", s.name)); }
        let Some((_, numeric)) = color_parameters(s.category).iter().find(|(n, _)| *n == s.parameter) else { return Err(format!("{} cannot colour {}.", s.parameter, self.category_label(s.category))) };
        if s.by_range && !numeric { return Err(format!("{} is not a number: colour it by value.", s.parameter)); }
        if (s.parameter != old.parameter || s.by_range != old.by_range) && s.entries == old.entries { s.entries.clear(); }
        if s.entries.iter().any(|e| e.at_least.is_some_and(|v| !v.is_finite())) { return Err("Invalid range.".into()); }
        self.transact("Edit color scheme", |tx| tx.update(id, Element::ColorFillScheme(s)))
    }

    /// Delete a scheme: the views that showed it show none.
    pub fn delete_color_scheme(&mut self, id: Id) -> Result<(), String> {
        self.color_fill_scheme(id).ok_or("Unknown colour scheme.")?;
        let views: Vec<(Id, crate::view::View)> = self.views().into_iter().filter(|(_, v)| v.settings.color_scheme == Some(id)).map(|(i, v)| { let mut v = v.clone(); v.settings.color_scheme = None; (i, v) }).collect();
        let templates: Vec<(Id, crate::view::ViewTemplate)> = self.iter().filter_map(|(i, e)| match e { Element::ViewTemplate(t) if t.settings.color_scheme == Some(id) => { let mut t = t.clone(); t.settings.color_scheme = None; Some((i, t)) } _ => None }).collect();
        self.transact("Delete color scheme", |tx| {
            for (i, v) in views { tx.update(i, Element::View(v))?; }
            for (i, t) in templates { tx.update(i, Element::ViewTemplate(t))?; }
            tx.remove(id)
        })
    }

    /// Show a scheme in a view (None: no colours). Refused when the view's template controls it.
    pub fn set_view_color_scheme(&mut self, view: Id, scheme: Option<Id>) -> Result<(), String> {
        let mut v = self.view(view).ok_or("Unknown view.")?.clone();
        if v.template.and_then(|t| self.view_template(t)).is_some_and(|t| t.included.contains(&crate::view::TemplateProp::ColorScheme)) { return Err("The view template controls this view's colour scheme.".into()); }
        if let Some(s) = scheme {
            let cat = self.color_fill_scheme(s).ok_or("Unknown colour scheme.")?.category;
            if Some(cat) != self.color_category_of_view(view) { return Err(format!("This view cannot show a colour scheme for {}.", self.category_label(cat))); }
        }
        v.settings.color_scheme = scheme;
        self.transact("Color scheme", |tx| tx.update(view, Element::View(v)))
    }

    /// The rooms or areas a category colours, with their regions: all of them (`view` None), or those in a view.
    fn colored(&self, category: ColorCategory, view: Option<Id>) -> Vec<(Id, Option<Region>)> {
        let mut out = vec![];
        match category {
            ColorCategory::Rooms => {
                let v = view.and_then(|v| self.view(v));
                for (id, r) in self.rooms() {
                    if let Some(v) = v { if Some(r.level) != v.level || (r.phase.is_some() && v.phase.is_some() && r.phase != v.phase) { continue; } }
                    let Some(c) = self.room_calculation(id) else { continue };
                    out.push((id, c.region));
                }
            }
            ColorCategory::Areas { scheme } => {
                for (id, a) in self.areas() {
                    if self.scheme_of_area(a) != Some(scheme) || view.is_some_and(|v| v != a.view) { continue; }
                    let Some(c) = self.area_calculation(id) else { continue };
                    out.push((id, c.region));
                }
            }
        }
        out
    }

    /// What a room or area has for a parameter.
    pub fn color_value(&self, id: Id, parameter: &str) -> Option<ColorValue> {
        let t = |s: &str| Some(ColorValue::Text(s.to_string()));
        match self.get(id)? {
            Element::Room(r) => match parameter {
                "Name" => t(&r.name), "Number" => t(&r.number), "Department" => t(&r.department), "Occupancy" => t(&r.occupancy), "Occupant" => t(&r.occupant),
                "Base Finish" => t(&r.base_finish), "Floor Finish" => t(&r.floor_finish), "Wall Finish" => t(&r.wall_finish), "Ceiling Finish" => t(&r.ceiling_finish), "Comments" => t(&r.comments),
                "Area" => Some(ColorValue::Number(self.room_calculation(id)?.area_mm2 / 1e6)),
                "Perimeter" => Some(ColorValue::Number(self.room_calculation(id)?.perimeter_mm)),
                _ => None,
            },
            Element::Area(a) => match parameter {
                "Name" => t(&a.name), "Number" => t(&a.number), "Area Type" => t(a.area_type.label()), "Comments" => t(&a.comments),
                "Area" => Some(ColorValue::Number(self.area_calculation(id)?.area_mm2 / 1e6)),
                "Perimeter" => Some(ColorValue::Number(self.area_calculation(id)?.perimeter_mm)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A scheme's rows: by value, its stored entries and then any value in the project without one (the next
    /// palette colour); by range, its ranges. `in_use` says whether anything (in `view`, when given) has it.
    pub fn color_rows(&self, id: Id, view: Option<Id>) -> Vec<ColorRow> {
        let Some(s) = self.color_fill_scheme(id) else { return vec![] };
        let values: Vec<ColorValue> = self.colored(s.category, view).into_iter().filter(|(_, r)| r.is_some()).filter_map(|(e, _)| self.color_value(e, &s.parameter)).collect();
        let all: Vec<ColorValue> = if view.is_some() { self.colored(s.category, None).into_iter().filter_map(|(e, _)| self.color_value(e, &s.parameter)).collect() } else { values.clone() };
        if s.by_range {
            let r = s.ranges();
            return (0..r.len()).map(|i| {
                let (lo, hi) = (r[i].at_least.unwrap(), r.get(i + 1).and_then(|n| n.at_least).unwrap_or(f64::INFINITY));
                let in_use = values.iter().any(|v| matches!(v, ColorValue::Number(n) if *n >= lo - 1e-9 && *n < hi - 1e-9));
                ColorRow { label: s.range_label(&r, i), value: String::new(), at_least: Some(lo), color: r[i].color, visible: r[i].visible, in_use }
            }).collect();
        }
        let text = |v: &ColorValue| match v { ColorValue::Text(t) => t.clone(), ColorValue::Number(n) => fmt_num(*n) };
        let mut rows: Vec<ColorRow> = s.entries.iter().filter(|e| e.at_least.is_none()).map(|e| ColorRow { label: e.value.clone(), value: e.value.clone(), at_least: None, color: e.color, visible: e.visible, in_use: values.iter().any(|v| text(v) == e.value) }).collect();
        let mut extra: Vec<String> = vec![];
        for v in all.iter().map(text) { if !rows.iter().any(|r| r.value == v) && !extra.contains(&v) { extra.push(v); } }
        extra.sort();
        for v in extra {
            let color = PALETTE[rows.len() % PALETTE.len()];
            let in_use = values.iter().any(|x| text(x) == v);
            rows.push(ColorRow { label: if v.is_empty() { "(none)".into() } else { v.clone() }, value: v, at_least: None, color, visible: true, in_use });
        }
        rows
    }

    /// The coloured rooms or areas of a view, under its scheme.
    pub fn color_fills(&self, view: Id) -> Vec<ColorFill> {
        let Some(id) = self.view_color_scheme(view) else { return vec![] };
        let s = self.color_fill_scheme(id).unwrap();
        let rows = self.color_rows(id, None);
        let mut out = vec![];
        for (e, region) in self.colored(s.category, Some(view)) {
            let Some(region) = region else { continue };
            let Some(v) = self.color_value(e, &s.parameter) else { continue };
            let row = match (&v, s.by_range) {
                (ColorValue::Number(n), true) => rows.iter().rev().find(|r| r.at_least.is_some_and(|lo| *n >= lo - 1e-9)),
                (ColorValue::Number(n), false) => rows.iter().find(|r| r.value == fmt_num(*n)),
                (ColorValue::Text(t), _) => rows.iter().find(|r| &r.value == t),
            };
            if let Some(r) = row.filter(|r| r.visible) { out.push(ColorFill { element: e, region, label: r.label.clone(), color: r.color }); }
        }
        out
    }

    /// Place a colour fill legend in a plan. The view needs a colour scheme first.
    pub fn add_color_legend(&mut self, view: Id, at: Point) -> Result<Id, String> {
        if self.color_category_of_view(view).is_none() { return Err("A colour fill legend goes in a floor plan or an area plan.".into()); }
        if self.view_color_scheme(view).is_none() { return Err("Choose a colour scheme for the view first.".into()); }
        if !at.x.is_finite() || !at.y.is_finite() { return Err("Invalid location".into()); }
        self.transact("Color fill legend", |tx| Ok(tx.add(Element::ColorFillLegend(ColorFillLegend { view, at, pinned: false }))))
    }
}
