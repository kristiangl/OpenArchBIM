//! Project parameters and schedules, evaluated as one dependency graph.
//!
//! Nodes are project parameters, schedule cells and schedule totals. A formula
//! can refer to any of them, so a schedule total can feed a project parameter,
//! another schedule's calculated column, or its own schedule's other rows.
//! Evaluation is lazy and memoised; reference cycles are detected and reported
//! on every node involved instead of hanging or crashing.

use std::collections::{BTreeMap, HashMap};

use crate::expr::{eval, names_in, parse};
use crate::value::{EvalError, Value};

#[derive(Debug, Clone)]
pub enum Param {
    Value(Value),
    Formula(String),
}

#[derive(Debug, Clone)]
pub enum ColumnKind {
    /// Read straight from each row's stored field of the same name.
    Field,
    /// Formula evaluated per row. May refer to other columns in the row,
    /// project parameters, and totals such as `[Rooms.Area.Sum]`.
    Calculated(String),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct Schedule {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agg {
    Sum,
    Count,
    Min,
    Max,
    Avg,
}

impl Agg {
    fn parse(s: &str) -> Option<Agg> {
        match s.to_ascii_lowercase().as_str() {
            "sum" => Some(Agg::Sum),
            "count" => Some(Agg::Count),
            "min" => Some(Agg::Min),
            "max" => Some(Agg::Max),
            "avg" | "average" => Some(Agg::Avg),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Global(String),
    Cell(usize, usize, usize),
    Total(usize, usize, Agg),
    Count(usize),
}

#[derive(Debug, Default, Clone)]
pub struct Project {
    pub globals: BTreeMap<String, Param>,
    pub schedules: Vec<Schedule>,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_value(&mut self, name: &str, v: Value) {
        self.globals.insert(name.to_string(), Param::Value(v));
    }

    pub fn set_formula(&mut self, name: &str, src: &str) {
        self.globals.insert(name.to_string(), Param::Formula(src.to_string()));
    }

    pub fn add_schedule(&mut self, s: Schedule) -> usize {
        self.schedules.push(s);
        self.schedules.len() - 1
    }

    /// Evaluate everything in the project.
    pub fn evaluate(&self) -> Evaluation {
        let mut ev = Evaluator { project: self, cache: HashMap::new(), stack: Vec::new() };
        let mut out = Evaluation::default();
        for name in self.globals.keys() {
            let r = ev.get(&Key::Global(name.clone()));
            out.globals.insert(name.clone(), r);
        }
        for (si, s) in self.schedules.iter().enumerate() {
            let mut cells = Vec::new();
            for ri in 0..s.rows.len() {
                let mut row = Vec::new();
                for ci in 0..s.columns.len() {
                    row.push(ev.get(&Key::Cell(si, ri, ci)));
                }
                cells.push(row);
            }
            out.cells.insert(s.name.clone(), cells);
            for (ci, c) in s.columns.iter().enumerate() {
                let r = ev.get(&Key::Total(si, ci, Agg::Sum));
                out.sums.insert((s.name.clone(), c.name.clone()), r);
            }
        }
        out
    }

    /// Names a formula depends on (display only, no evaluation).
    pub fn dependencies(formula: &str) -> Result<Vec<String>, EvalError> {
        let e = parse(formula)?;
        let mut v = Vec::new();
        names_in(&e, &mut v);
        v.sort();
        v.dedup();
        Ok(v)
    }
}

#[derive(Debug, Default)]
pub struct Evaluation {
    pub globals: BTreeMap<String, Result<Value, EvalError>>,
    /// schedule name -> rows -> columns
    pub cells: BTreeMap<String, Vec<Vec<Result<Value, EvalError>>>>,
    /// (schedule, column) -> sum, for quick display of totals rows
    pub sums: BTreeMap<(String, String), Result<Value, EvalError>>,
}

impl Evaluation {
    pub fn global(&self, name: &str) -> Option<&Result<Value, EvalError>> {
        self.globals.get(name)
    }

    pub fn cell(&self, schedule: &str, row: usize, col: usize) -> Option<&Result<Value, EvalError>> {
        self.cells.get(schedule)?.get(row)?.get(col)
    }

    pub fn sum(&self, schedule: &str, column: &str) -> Option<&Result<Value, EvalError>> {
        self.sums.get(&(schedule.to_string(), column.to_string()))
    }

    pub fn error_count(&self) -> usize {
        self.globals.values().filter(|r| r.is_err()).count()
            + self.cells.values().flatten().flatten().filter(|r| r.is_err()).count()
    }
}

struct Evaluator<'a> {
    project: &'a Project,
    cache: HashMap<Key, Result<Value, EvalError>>,
    stack: Vec<Key>,
}

impl<'a> Evaluator<'a> {
    fn label(&self, k: &Key) -> String {
        match k {
            Key::Global(n) => n.clone(),
            Key::Cell(s, r, c) => {
                let sch = &self.project.schedules[*s];
                format!("{}[{}].{}", sch.name, sch.rows[*r].id, sch.columns[*c].name)
            }
            Key::Total(s, c, a) => {
                let sch = &self.project.schedules[*s];
                format!("{}.{}.{:?}", sch.name, sch.columns[*c].name, a)
            }
            Key::Count(s) => format!("{}.Count", self.project.schedules[*s].name),
        }
    }

    fn get(&mut self, key: &Key) -> Result<Value, EvalError> {
        if let Some(r) = self.cache.get(key) {
            return r.clone();
        }
        if let Some(pos) = self.stack.iter().position(|k| k == key) {
            let mut path: Vec<String> = self.stack[pos..].iter().map(|k| self.label(k)).collect();
            path.push(self.label(key));
            return Err(EvalError::Cycle(path));
        }
        self.stack.push(key.clone());
        let r = self.compute(key);
        self.stack.pop();
        self.cache.insert(key.clone(), r.clone());
        r
    }

    fn compute(&mut self, key: &Key) -> Result<Value, EvalError> {
        let project = self.project;
        match key {
            Key::Global(name) => match project.globals.get(name) {
                Some(Param::Value(v)) => Ok(v.clone()),
                Some(Param::Formula(src)) => {
                    let e = parse(src)?;
                    eval(&e, &mut |n| self.resolve(n, None))
                }
                None => Err(EvalError::UnknownName(name.clone())),
            },
            Key::Cell(s, r, c) => {
                let sch = &project.schedules[*s];
                let col = &sch.columns[*c];
                match &col.kind {
                    ColumnKind::Field => sch.rows[*r].fields.get(&col.name).cloned().ok_or_else(|| {
                        EvalError::MissingField(format!("{} on row {}", col.name, sch.rows[*r].id))
                    }),
                    ColumnKind::Calculated(src) => {
                        let e = parse(src)?;
                        let scope = Some((*s, *r));
                        eval(&e, &mut |n| self.resolve(n, scope))
                    }
                }
            }
            Key::Count(s) => Ok(Value::Num(project.schedules[*s].rows.len() as f64)),
            Key::Total(s, c, agg) => {
                let n = project.schedules[*s].rows.len();
                let mut vals = Vec::with_capacity(n);
                for r in 0..n {
                    vals.push(self.get(&Key::Cell(*s, r, *c))?.as_num()?);
                }
                match agg {
                    Agg::Sum => Ok(Value::Num(vals.iter().sum())),
                    Agg::Count => Ok(Value::Num(n as f64)),
                    Agg::Min => vals
                        .iter()
                        .cloned()
                        .reduce(f64::min)
                        .map(Value::Num)
                        .ok_or_else(|| EvalError::MissingField("no rows to take min of".into())),
                    Agg::Max => vals
                        .iter()
                        .cloned()
                        .reduce(f64::max)
                        .map(Value::Num)
                        .ok_or_else(|| EvalError::MissingField("no rows to take max of".into())),
                    Agg::Avg => {
                        if n == 0 {
                            Err(EvalError::DivideByZero)
                        } else {
                            Ok(Value::Num(vals.iter().sum::<f64>() / n as f64))
                        }
                    }
                }
            }
        }
    }

    /// Resolve a name: column in this row, then project parameter, then a
    /// `Schedule.Column.Agg` / `Schedule.Count` total.
    fn resolve(&mut self, name: &str, scope: Option<(usize, usize)>) -> Result<Value, EvalError> {
        let project = self.project;
        if let Some((s, r)) = scope {
            if let Some(c) = project.schedules[s]
                .columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name))
            {
                return self.get(&Key::Cell(s, r, c));
            }
        }
        if let Some(g) = project.globals.keys().find(|k| k.eq_ignore_ascii_case(name)) {
            let k = Key::Global(g.clone());
            return self.get(&k);
        }
        if let Some((left, agg_name)) = name.rsplit_once('.') {
            if let Some(agg) = Agg::parse(agg_name) {
                // Schedule.Count
                if agg == Agg::Count {
                    if let Some(s) = project.schedules.iter().position(|s| s.name.eq_ignore_ascii_case(left)) {
                        return self.get(&Key::Count(s));
                    }
                }
                // Schedule.Column.Agg
                if let Some((sched, col)) = left.rsplit_once('.') {
                    if let Some(s) = project.schedules.iter().position(|s| s.name.eq_ignore_ascii_case(sched)) {
                        if let Some(c) = project.schedules[s]
                            .columns
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case(col))
                        {
                            return self.get(&Key::Total(s, c, agg));
                        }
                    }
                }
            }
        }
        Err(EvalError::UnknownName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::Num(x)
    }

    fn field(name: &str) -> Column {
        Column { name: name.into(), kind: ColumnKind::Field }
    }

    fn calc(name: &str, f: &str) -> Column {
        Column { name: name.into(), kind: ColumnKind::Calculated(f.into()) }
    }

    fn row(id: &str, fields: &[(&str, Value)]) -> Row {
        Row { id: id.into(), fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
    }

    fn rooms() -> Schedule {
        Schedule {
            name: "Rooms".into(),
            columns: vec![field("Name"), field("Area"), calc("Cost", "[Area] * [Area Rate]")],
            rows: vec![
                row("r1", &[("Name", Value::Text("Shop".into())), ("Area", n(120.0))]),
                row("r2", &[("Name", Value::Text("Store".into())), ("Area", n(30.0))]),
                row("r3", &[("Name", Value::Text("Amenities".into())), ("Area", n(15.5))]),
            ],
        }
    }

    /// The headline use case: turn an area schedule into a rough pricing
    /// schedule with a calculated rate column, then use its total elsewhere.
    #[test]
    fn area_schedule_becomes_pricing_schedule() {
        let mut p = Project::new();
        p.set_value("Area Rate", n(3200.0));
        p.set_formula("Build Cost", "[Rooms.Cost.Sum]");
        p.set_formula("Contingency", "round([Build Cost] * 0.05, 0)");
        p.set_formula("Total Inc GST", "round(([Build Cost] + Contingency) * 1.1, 2)");
        p.add_schedule(rooms());
        let ev = p.evaluate();
        assert_eq!(ev.error_count(), 0);
        assert_eq!(ev.cell("Rooms", 0, 2), Some(&Ok(n(384_000.0))));
        assert_eq!(ev.global("Build Cost"), Some(&Ok(n(529_600.0))));
        assert_eq!(ev.global("Contingency"), Some(&Ok(n(26_480.0))));
        assert_eq!(ev.global("Total Inc GST"), Some(&Ok(n(611_688.0))));
    }

    #[test]
    fn totals_feed_other_schedules() {
        let mut p = Project::new();
        p.set_value("Area Rate", n(100.0));
        p.add_schedule(rooms());
        // Share of the building each level represents, from another schedule's total
        p.add_schedule(Schedule {
            name: "Levels".into(),
            columns: vec![field("Level"), field("Area"), calc("Share", "round([Area] / [Rooms.Area.Sum] * 100, 1)")],
            rows: vec![
                row("l1", &[("Level", Value::Text("Ground".into())), ("Area", n(83.0))]),
                row("l2", &[("Level", Value::Text("First".into())), ("Area", n(82.5))]),
            ],
        });
        let ev = p.evaluate();
        assert_eq!(ev.error_count(), 0, "{:?}", ev);
        assert_eq!(ev.cell("Levels", 0, 2), Some(&Ok(n(50.2))));
        assert_eq!(ev.cell("Levels", 1, 2), Some(&Ok(n(49.8))));
    }

    #[test]
    fn counts_min_max_avg() {
        let mut p = Project::new();
        p.set_value("Area Rate", n(1.0));
        p.add_schedule(rooms());
        p.set_formula("N", "Rooms.Count");
        p.set_formula("Biggest", "[Rooms.Area.Max]");
        p.set_formula("Smallest", "[Rooms.Area.Min]");
        p.set_formula("Mean", "round([Rooms.Area.Avg], 2)");
        let ev = p.evaluate();
        assert_eq!(ev.global("N"), Some(&Ok(n(3.0))));
        assert_eq!(ev.global("Biggest"), Some(&Ok(n(120.0))));
        assert_eq!(ev.global("Smallest"), Some(&Ok(n(15.5))));
        assert_eq!(ev.global("Mean"), Some(&Ok(n(55.17))));
    }

    #[test]
    fn cycles_are_reported_not_fatal() {
        let mut p = Project::new();
        p.set_formula("A", "B + 1");
        p.set_formula("B", "A + 1");
        p.set_value("Fine", n(5.0));
        p.set_formula("Uses Fine", "Fine * 2");
        let ev = p.evaluate();
        assert!(matches!(ev.global("A"), Some(Err(EvalError::Cycle(_)))));
        assert!(matches!(ev.global("B"), Some(Err(EvalError::Cycle(_)))));
        assert_eq!(ev.global("Uses Fine"), Some(&Ok(n(10.0))));
    }

    #[test]
    fn schedule_total_cycle_is_caught() {
        let mut p = Project::new();
        p.add_schedule(Schedule {
            name: "S".into(),
            columns: vec![field("X"), calc("Y", "[X] + [S.Y.Sum]")],
            rows: vec![row("a", &[("X", n(1.0))])],
        });
        let ev = p.evaluate();
        assert!(matches!(ev.cell("S", 0, 1), Some(Err(EvalError::Cycle(_)))));
    }

    #[test]
    fn errors_stay_local() {
        let mut p = Project::new();
        p.set_formula("Bad", "1 / 0");
        p.set_formula("Dependent", "Bad + 1");
        p.set_value("Good", n(2.0));
        let ev = p.evaluate();
        assert_eq!(ev.global("Bad"), Some(&Err(EvalError::DivideByZero)));
        assert_eq!(ev.global("Dependent"), Some(&Err(EvalError::DivideByZero)));
        assert_eq!(ev.global("Good"), Some(&Ok(n(2.0))));
    }

    #[test]
    fn dependencies_listed() {
        let d = Project::dependencies("[Area] * [Area Rate] + Rooms.Count").unwrap();
        assert_eq!(d, vec!["Area", "Area Rate", "Rooms.Count"]);
    }
}
