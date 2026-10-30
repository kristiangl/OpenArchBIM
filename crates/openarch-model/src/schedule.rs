//! Schedules: a view that lists elements as a table. A schedule's definition says which category it lists, its
//! fields (the columns: an element's or its type's parameters, Count, formulas, percentages and combined
//! parameters), the filters a row must pass, up to four sort and group keys with their headers, footers and blank
//! lines, whether every element gets a row or identical rows are compressed into one, and the grand totals. The
//! table itself is worked out from the live model each time it is shown (openarch-api), and edits to its cells go
//! back to the elements.
//!
//! From Kristian's Schedules and Tabular Reporting specification (30 Oct). Besides a single category, a schedule
//! lists every model category (Multi-Category: the parameters they share), the sheets (a sheet list) or the views
//! (a view list); a material takeoff lists each element once for every material in its layers. Key schedules come
//! later.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::view::{View, ViewKind};

/// How many filters and sort keys a schedule takes.
pub const MAX_FILTERS: usize = 8;
pub const MAX_SORT: usize = 4;

/// The categories that aren't a model category.
pub const MULTI_CATEGORY: &str = "Multi-Category";
pub const SHEETS: &str = "Sheets";
pub const VIEWS: &str = "Views";

/// What a material takeoff adds to the element's fields: the material's own values and its share of the element.
pub const MATERIAL_FIELDS: [(&str, Unit); 12] = [
    ("Material: Name", Unit::Text),
    ("Material: Area", Unit::Area),
    ("Material: Volume", Unit::Volume),
    ("Material: Thickness", Unit::Length),
    ("Material: Class", Unit::Text),
    ("Material: Description", Unit::Text),
    ("Material: Comments", Unit::Text),
    ("Material: Manufacturer", Unit::Text),
    ("Material: Model", Unit::Text),
    ("Material: Cost", Unit::Currency),
    ("Material: Mark", Unit::Text),
    ("Material: Keynote", Unit::Text),
];

/// The categories a material takeoff can list: those built of layers.
pub const TAKEOFF_CATEGORIES: [&str; 4] = ["Walls", "Floors", "Roofs", MULTI_CATEGORY];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleDef {
    /// The category listed (Doors, Walls, ...), Multi-Category, Sheets or Views.
    pub category: String,
    /// A material takeoff: a row for each material of each element.
    #[serde(default)]
    pub takeoff: bool,
    #[serde(default)]
    pub fields: Vec<SchedField>,
    #[serde(default)]
    pub filters: Vec<SchedFilter>,
    #[serde(default)]
    pub sort: Vec<SortKey>,
    /// A row for every element; off, elements that agree on every sort key share one row.
    #[serde(default = "yes")]
    pub itemize: bool,
    #[serde(default)]
    pub grand_totals: bool,
    #[serde(default = "grand_title")]
    pub grand_title: String,
    /// Only the elements the sheet's views show, when the schedule is on a sheet (with schedules on sheets).
    #[serde(default)]
    pub filter_by_sheet: bool,
    #[serde(default = "yes")]
    pub show_title: bool,
    #[serde(default = "yes")]
    pub show_headers: bool,
}

fn yes() -> bool {
    true
}

fn grand_title() -> String {
    "Grand total".into()
}

/// A column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedField {
    /// The parameter's name, or the calculated field's own name.
    pub name: String,
    pub kind: FieldKind,
    /// The column's heading; empty for the name.
    #[serde(default)]
    pub heading: String,
    /// Used for sorting, filtering and formulas but not shown.
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub totals: Totals,
    #[serde(default)]
    pub align: Align,
    /// A formula field's expression, in the other fields' names ([Area] * [Unit Cost]).
    #[serde(default)]
    pub formula: String,
    /// What a formula gives.
    #[serde(default)]
    pub unit: Unit,
    /// A percentage field's field: each row's share of its total.
    #[serde(default)]
    pub of: String,
    /// A combined field's parts, joined in order.
    #[serde(default)]
    pub parts: Vec<CombinedPart>,
    /// Conditional formatting: the first rule a row passes colours this field's cell.
    #[serde(default)]
    pub highlights: Vec<Highlight>,
    /// The colours on sheets as well as in the schedule view.
    #[serde(default)]
    pub highlight_on_sheets: bool,
}

/// A conditional format: a cell's background when a field of its row passes a test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    /// The field tested; empty for the field itself.
    #[serde(default)]
    pub field: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
    pub colour: crate::view::Color,
}

impl SchedField {
    pub fn heading(&self) -> &str {
        if self.heading.trim().is_empty() {
            &self.name
        } else {
            &self.heading
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// A parameter of the element.
    #[default]
    Instance,
    /// A parameter of the element's type.
    Type,
    /// How many elements the row stands for.
    Count,
    Formula,
    Percentage,
    Combined,
    /// A material takeoff's value of the row's material (MATERIAL_FIELDS).
    Material,
}

/// What a footer and the grand total show for a numeric field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Totals {
    #[default]
    None,
    Totals,
    Min,
    Max,
    MinMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Centre,
    Right,
}

/// What a value measures: lengths show in mm, areas in m², volumes in m³.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Length,
    Area,
    Volume,
    Angle,
    #[default]
    Number,
    Integer,
    Currency,
    Text,
    YesNo,
}

impl Unit {
    /// Its power of length: what multiplying and dividing do to it. None for what isn't a quantity.
    pub fn dims(self) -> Option<i32> {
        match self {
            Unit::Length => Some(1),
            Unit::Area => Some(2),
            Unit::Volume => Some(3),
            Unit::Angle | Unit::Number | Unit::Integer | Unit::Currency => Some(0),
            Unit::Text | Unit::YesNo => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Unit::Length => "Length",
            Unit::Area => "Area",
            Unit::Volume => "Volume",
            Unit::Angle => "Angle",
            Unit::Number => "Number",
            Unit::Integer => "Integer",
            Unit::Currency => "Currency",
            Unit::Text => "Text",
            Unit::YesNo => "Yes/No",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedPart {
    pub field: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    /// Put between this part and the next.
    #[serde(default)]
    pub separator: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedFilter {
    pub field: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Contains,
    NotContains,
    BeginsWith,
    NotBeginsWith,
    EndsWith,
    NotEndsWith,
    HasValue,
    HasNoValue,
    /// The element has the parameter at all (in a multi-category schedule, not every element does).
    HasParameter,
    /// The value is set by a global parameter: the one named, or any when no name is given.
    AssociatedWithGlobal,
    NotAssociatedWithGlobal,
}

impl FilterOp {
    pub const ALL: [FilterOp; 17] = [
        FilterOp::Equal,
        FilterOp::NotEqual,
        FilterOp::Greater,
        FilterOp::GreaterOrEqual,
        FilterOp::Less,
        FilterOp::LessOrEqual,
        FilterOp::Contains,
        FilterOp::NotContains,
        FilterOp::BeginsWith,
        FilterOp::NotBeginsWith,
        FilterOp::EndsWith,
        FilterOp::NotEndsWith,
        FilterOp::HasValue,
        FilterOp::HasNoValue,
        FilterOp::HasParameter,
        FilterOp::AssociatedWithGlobal,
        FilterOp::NotAssociatedWithGlobal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Equal => "equals",
            FilterOp::NotEqual => "does not equal",
            FilterOp::Greater => "is greater than",
            FilterOp::GreaterOrEqual => "is greater than or equal to",
            FilterOp::Less => "is less than",
            FilterOp::LessOrEqual => "is less than or equal to",
            FilterOp::Contains => "contains",
            FilterOp::NotContains => "does not contain",
            FilterOp::BeginsWith => "begins with",
            FilterOp::NotBeginsWith => "does not begin with",
            FilterOp::EndsWith => "ends with",
            FilterOp::NotEndsWith => "does not end with",
            FilterOp::HasValue => "has a value",
            FilterOp::HasNoValue => "has no value",
            FilterOp::HasParameter => "has parameter",
            FilterOp::AssociatedWithGlobal => "is associated with global parameter",
            FilterOp::NotAssociatedWithGlobal => "is not associated with global parameter",
        }
    }

    /// Whether a cell passes, knowing whether its element has the parameter and which global parameter, if any,
    /// sets it.
    pub fn test_cell(self, value: &str, num: Option<f64>, present: bool, global: Option<&str>, against: &str) -> bool {
        let named = |g: &str| against.trim().is_empty() || g.trim().eq_ignore_ascii_case(against.trim());
        match self {
            FilterOp::HasParameter => present,
            FilterOp::AssociatedWithGlobal => global.is_some_and(named),
            FilterOp::NotAssociatedWithGlobal => !global.is_some_and(named),
            _ => self.test(value, num, against),
        }
    }

    /// Whether a value passes: numbers compare as numbers (both in the field's display units), text as text,
    /// ignoring case.
    pub fn test(self, value: &str, num: Option<f64>, against: &str) -> bool {
        let a = value.trim().to_lowercase();
        let b = against.trim().to_lowercase();
        let both = num.zip(against.trim().parse::<f64>().ok());
        let cmp = |f: fn(f64, f64) -> bool, g: fn(&str, &str) -> bool| match both {
            Some((x, y)) => f(x, y),
            None => g(&a, &b),
        };
        match self {
            FilterOp::Equal => cmp(|x, y| (x - y).abs() < 1e-6, |x, y| x == y),
            FilterOp::NotEqual => !cmp(|x, y| (x - y).abs() < 1e-6, |x, y| x == y),
            FilterOp::Greater => cmp(|x, y| x > y, |x, y| x > y),
            FilterOp::GreaterOrEqual => cmp(|x, y| x >= y - 1e-9, |x, y| x >= y),
            FilterOp::Less => cmp(|x, y| x < y, |x, y| x < y),
            FilterOp::LessOrEqual => cmp(|x, y| x <= y + 1e-9, |x, y| x <= y),
            FilterOp::Contains => a.contains(&b),
            FilterOp::NotContains => !a.contains(&b),
            FilterOp::BeginsWith => a.starts_with(&b),
            FilterOp::NotBeginsWith => !a.starts_with(&b),
            FilterOp::EndsWith => a.ends_with(&b),
            FilterOp::NotEndsWith => !a.ends_with(&b),
            FilterOp::HasValue => !a.is_empty(),
            FilterOp::HasNoValue => a.is_empty(),
            FilterOp::HasParameter => true,
            FilterOp::AssociatedWithGlobal | FilterOp::NotAssociatedWithGlobal => false,
        }
    }
}

/// A sort and group key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    pub field: String,
    #[serde(default)]
    pub descending: bool,
    /// A band with the key's value where it changes.
    #[serde(default)]
    pub header: bool,
    /// A band after each group: its value, count and the fields' totals.
    #[serde(default)]
    pub footer: bool,
    #[serde(default)]
    pub blank_line: bool,
}

impl ScheduleDef {
    pub fn new(category: &str) -> ScheduleDef {
        ScheduleDef {
            category: category.into(),
            takeoff: false,
            fields: vec![],
            filters: vec![],
            sort: vec![],
            itemize: true,
            grand_totals: false,
            grand_title: grand_title(),
            filter_by_sheet: false,
            show_title: true,
            show_headers: true,
        }
    }

    pub fn field(&self, name: &str) -> Option<&SchedField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Whether the definition holds together: names are unique, filters and sort keys name fields, formulas parse
    /// and their units agree with what they say they give. `unit_of` is the unit of a parameter field.
    pub fn check(&self, unit_of: &dyn Fn(&SchedField) -> Unit) -> Result<(), String> {
        for (i, f) in self.fields.iter().enumerate() {
            if f.name.trim().is_empty() {
                return Err("A field needs a name.".into());
            }
            if self.fields[..i].iter().any(|g| g.name == f.name) {
                return Err(format!("{} is in the schedule twice.", f.name));
            }
        }
        if self.filters.len() > MAX_FILTERS {
            return Err(format!("A schedule takes up to {MAX_FILTERS} filters."));
        }
        if self.sort.len() > MAX_SORT {
            return Err(format!("A schedule sorts by up to {MAX_SORT} fields."));
        }
        if self.takeoff && !TAKEOFF_CATEGORIES.contains(&self.category.as_str()) {
            return Err(format!("A material takeoff lists walls, floors or roofs, not {}.", self.category.to_lowercase()));
        }
        let highlighted = self.fields.iter().flat_map(|f| f.highlights.iter().filter(|h| !h.field.is_empty()).map(|h| &h.field));
        for name in self.filters.iter().map(|f| &f.field).chain(self.sort.iter().map(|s| &s.field)).chain(highlighted) {
            if self.field(name).is_none() {
                return Err(format!("{name} is not a field of the schedule: add it (hidden if need be) to filter or sort by it."));
            }
        }
        let unit = |name: &str| -> Option<Unit> { self.field(name).map(|f| self.unit_of(f, unit_of)) };
        for f in &self.fields {
            match f.kind {
                FieldKind::Formula => {
                    let e = openarch_params::expr::parse(&f.formula).map_err(|e| format!("{}: {e}", f.name))?;
                    let mut names = vec![];
                    openarch_params::expr::names_in(&e, &mut names);
                    for n in &names {
                        if n == &f.name {
                            return Err(format!("{} can't use itself.", f.name));
                        }
                        if unit(n).is_none() {
                            return Err(format!("{}: {n} is not a field of the schedule.", f.name));
                        }
                    }
                    if let (Some(got), Some(want)) = (dims(&e, &unit)?, f.unit.dims()) {
                        if got != want && f.unit != Unit::Text {
                            return Err(format!("{} gives {}, not {}.", f.name, dims_label(got), f.unit.label()));
                        }
                    }
                }
                FieldKind::Percentage => match unit(&f.of) {
                    Some(u) if u.dims().is_some() => {}
                    _ => return Err(format!("{}: a percentage is of a numeric field of the schedule.", f.name)),
                },
                FieldKind::Combined => {
                    if f.parts.is_empty() {
                        return Err(format!("{} combines no fields.", f.name));
                    }
                    if let Some(p) = f.parts.iter().find(|p| self.field(&p.field).is_none()) {
                        return Err(format!("{}: {} is not a field of the schedule.", f.name, p.field));
                    }
                }
                FieldKind::Material if !self.takeoff || material_unit(&f.name).is_none() => {
                    return Err(format!("{} is a field of material takeoffs.", f.name));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// What a field measures.
    pub fn unit_of(&self, f: &SchedField, param_unit: &dyn Fn(&SchedField) -> Unit) -> Unit {
        match f.kind {
            FieldKind::Instance | FieldKind::Type => param_unit(f),
            FieldKind::Count => Unit::Integer,
            FieldKind::Formula => f.unit,
            FieldKind::Percentage => Unit::Number,
            FieldKind::Combined => Unit::Text,
            FieldKind::Material => material_unit(&f.name).unwrap_or(Unit::Text),
        }
    }
}

/// What a material field measures.
pub fn material_unit(name: &str) -> Option<Unit> {
    MATERIAL_FIELDS.iter().find(|(n, _)| *n == name).map(|(_, u)| *u)
}

fn dims_label(d: i32) -> String {
    match d {
        0 => "a Number".into(),
        1 => "a Length".into(),
        2 => "an Area".into(),
        3 => "a Volume".into(),
        n => format!("length to the power {n}"),
    }
}

/// The power of length a formula gives, None where it is a bare number that takes any; an error where it adds or
/// compares unlike things.
pub fn dims(e: &openarch_params::expr::Expr, unit: &dyn Fn(&str) -> Option<Unit>) -> Result<Option<i32>, String> {
    use openarch_params::expr::{BinOp, Expr};
    let same = |a: Option<i32>, b: Option<i32>, what: &str| -> Result<Option<i32>, String> {
        match (a, b) {
            (Some(x), Some(y)) if x != y => Err(format!("can't {what} {} and {}", dims_label(x), dims_label(y))),
            (Some(x), _) | (_, Some(x)) => Ok(Some(x)),
            _ => Ok(None),
        }
    };
    Ok(match e {
        Expr::Num(_) => None,
        Expr::Text(_) | Expr::Bool(_) => Some(0),
        Expr::Name(n) => unit(n).and_then(|u| u.dims()).or(Some(0)),
        Expr::Unary(_, a) => dims(a, unit)?,
        Expr::Binary(op, a, b) => {
            let (x, y) = (dims(a, unit)?, dims(b, unit)?);
            match op {
                BinOp::Mul => Some(x.unwrap_or(0) + y.unwrap_or(0)).filter(|_| x.is_some() || y.is_some()),
                BinOp::Div => Some(x.unwrap_or(0) - y.unwrap_or(0)).filter(|_| x.is_some() || y.is_some()),
                BinOp::Pow => match (x, &**b) {
                    (Some(d), Expr::Num(p)) => Some((d as f64 * p).round() as i32),
                    _ => x,
                },
                BinOp::Add | BinOp::Sub | BinOp::Rem => same(x, y, "add")?,
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    same(x, y, "compare")?;
                    Some(0)
                }
                BinOp::And | BinOp::Or => Some(0),
            }
        }
        Expr::Call(f, args) => match (f.as_str(), args.as_slice()) {
            ("if", [_, a, b]) => same(dims(a, unit)?, dims(b, unit)?, "choose between")?,
            ("sqrt", [a]) => dims(a, unit)?.map(|d| d / 2),
            ("min" | "max", _) => {
                let mut d = None;
                for a in args {
                    d = same(d, dims(a, unit)?, "compare")?;
                }
                d
            }
            (_, [a, ..]) => dims(a, unit)?,
            _ => None,
        },
    })
}

impl Document {
    pub fn schedules(&self) -> Vec<(Id, &View)> {
        self.views().into_iter().filter(|(_, v)| v.view_kind == ViewKind::Schedule).collect()
    }

    pub fn schedule(&self, id: Id) -> Option<&ScheduleDef> {
        self.view(id).and_then(|v| v.schedule.as_ref())
    }

    /// A new schedule of a category, or material takeoff (one undo step); the name must be new among schedules.
    pub fn add_schedule(&mut self, category: &str, name: &str, takeoff: bool) -> Result<Id, String> {
        let name = name.trim();
        if name.is_empty() || self.schedules().iter().any(|(_, v)| v.name == name) {
            return Err("A schedule needs a name of its own.".into());
        }
        if category.trim().is_empty() {
            return Err("A schedule lists a category.".into());
        }
        let mut v = View::floor_plan(name, 0);
        v.view_kind = ViewKind::Schedule;
        v.level = None;
        let def = ScheduleDef { takeoff, ..ScheduleDef::new(category) };
        def.check(&|_| Unit::Text)?;
        v.schedule = Some(def);
        self.transact("New Schedule", |tx| Ok(tx.add(Element::View(v))))
    }

    /// Changes a schedule's definition (one undo step), when it holds together.
    pub fn set_schedule(&mut self, id: Id, def: ScheduleDef, unit_of: &dyn Fn(&SchedField) -> Unit) -> Result<(), String> {
        let Some(v) = self.view(id).cloned() else { return Err("not a view".into()) };
        if v.view_kind != ViewKind::Schedule {
            return Err("not a schedule".into());
        }
        def.check(unit_of)?;
        self.transact("Schedule Properties", |tx| tx.update(id, Element::View(View { schedule: Some(def), ..v })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str, kind: FieldKind) -> SchedField {
        SchedField { name: name.into(), kind, heading: String::new(), hidden: false, totals: Totals::None, align: Align::Left, formula: String::new(), unit: Unit::Number, of: String::new(), parts: vec![], highlights: vec![], highlight_on_sheets: false }
    }

    #[test]
    fn formulas_are_checked_for_their_units() {
        let units = |x: &SchedField| match x.name.as_str() {
            "Width" | "Height" => Unit::Length,
            "Area" => Unit::Area,
            "Mark" => Unit::Text,
            _ => Unit::Number,
        };
        let mut s = ScheduleDef::new("Doors");
        s.fields = vec![f("Width", FieldKind::Instance), f("Height", FieldKind::Instance), f("Area", FieldKind::Instance), f("Unit Cost", FieldKind::Type)];
        let mut opening = SchedField { formula: "[Width] * [Height]".into(), unit: Unit::Area, ..f("Opening", FieldKind::Formula) };
        s.fields.push(opening.clone());
        assert!(s.check(&units).is_ok(), "a length times a length is an area");
        opening.unit = Unit::Length;
        s.fields[4] = opening.clone();
        assert!(s.check(&units).unwrap_err().contains("an Area, not Length"));
        opening.formula = "[Width] + [Area]".into();
        s.fields[4] = opening.clone();
        assert!(s.check(&units).unwrap_err().contains("can't add"));
        opening.formula = "[Area] * [Unit Cost] * 1.15".into();
        opening.unit = Unit::Area;
        s.fields[4] = opening;
        assert!(s.check(&units).is_ok(), "a number scales an area");
        // filters and sort keys name fields of the schedule
        s.sort.push(SortKey { field: "Mark".into(), descending: false, header: false, footer: false, blank_line: false });
        assert!(s.check(&units).unwrap_err().contains("not a field"));
        s.sort.clear();
        // percentages are of numbers
        s.fields.push(SchedField { of: "Area".into(), ..f("Share", FieldKind::Percentage) });
        assert!(s.check(&units).is_ok());
    }

    #[test]
    fn filters_compare_numbers_as_numbers_and_text_as_text() {
        assert!(FilterOp::Greater.test("900", Some(900.0), "820"));
        assert!(!FilterOp::Greater.test("900", Some(900.0), "1000"));
        assert!(FilterOp::Equal.test("Single-Flush", None, "single-flush"));
        assert!(FilterOp::BeginsWith.test("D01", None, "d"));
        assert!(FilterOp::HasNoValue.test("  ", None, ""));
        assert!(!FilterOp::HasParameter.test_cell("", None, false, None, ""));
        assert!(FilterOp::AssociatedWithGlobal.test_cell("900", Some(900.0), true, Some("Door Width"), "door width"));
        assert!(FilterOp::AssociatedWithGlobal.test_cell("900", Some(900.0), true, Some("Door Width"), ""), "no name: any global");
        assert!(FilterOp::NotAssociatedWithGlobal.test_cell("900", Some(900.0), true, Some("Door Width"), "Sill"));
        assert!(!FilterOp::NotAssociatedWithGlobal.test_cell("900", Some(900.0), true, Some("Door Width"), ""));
    }

    #[test]
    fn takeoffs_are_of_layered_categories_and_material_fields_are_theirs() {
        let units = |_: &SchedField| Unit::Text;
        let mut s = ScheduleDef { takeoff: true, ..ScheduleDef::new("Doors") };
        assert!(s.check(&units).unwrap_err().contains("walls, floors or roofs"));
        s.category = "Walls".into();
        s.fields = vec![f("Material: Area", FieldKind::Material), f("Type", FieldKind::Type)];
        assert!(s.check(&units).is_ok());
        assert_eq!(s.unit_of(&s.fields[0], &units), Unit::Area);
        s.takeoff = false;
        assert!(s.check(&units).unwrap_err().contains("material takeoffs"));
        // a highlight tests a field of the schedule
        s.takeoff = true;
        s.fields[1].highlights.push(Highlight { field: "Width".into(), op: FilterOp::Less, value: "15".into(), colour: [255, 200, 200] });
        assert!(s.check(&units).unwrap_err().contains("Width is not a field"));
    }
}
