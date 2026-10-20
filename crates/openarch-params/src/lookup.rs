//! Lookup tables: sizes read from a CSV file and kept in the family that uses them, read by
//! `size_lookup(table, column, default, value1, value2, ...)` in a formula.
//!
//! The first row of the file names the columns. A name may carry its kind and unit after `##`,
//! as in `Nominal Diameter##length##millimeters`. Lengths are stored in millimetres whatever unit
//! the file used. The first column labels the row and is not matched; `value1` is matched against
//! the second column, `value2` against the third, and so on. The column named in the call gives the
//! value returned. When no row matches, the default is returned.

use crate::value::{EvalError, Value};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookupColumn {
    pub name: String,
    /// "length", "number", "integer", "text" and the other data types a file can name; empty for plain text or numbers.
    #[serde(default)]
    pub kind: String,
    /// The unit the file gave (lengths are converted to millimetres on import and exported back in this unit).
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookupTable {
    pub name: String,
    pub columns: Vec<LookupColumn>,
    pub rows: Vec<Vec<Cell>>,
}

fn mm_per(unit: &str) -> Option<f64> {
    Some(match unit.trim().to_ascii_lowercase().as_str() {
        "millimeters" | "millimetres" | "millimeter" | "millimetre" | "mm" | "" => 1.0,
        "centimeters" | "centimetres" | "centimeter" | "centimetre" | "cm" => 10.0,
        "meters" | "metres" | "meter" | "metre" | "m" => 1000.0,
        "inches" | "inch" | "in" => 25.4,
        "feet" | "foot" | "ft" => 304.8,
        _ => return None,
    })
}

/// Cells of a CSV line, with quoted cells that may hold commas and doubled quotes.
fn split_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) => {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            ('"', false) if cell.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut cell)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    rows
}

impl LookupTable {
    /// Read a table from the text of a CSV file.
    pub fn from_csv(name: &str, text: &str) -> Result<LookupTable, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a lookup table needs a name".into());
        }
        let mut lines = split_csv(text).into_iter();
        let head = lines.next().ok_or("the file is empty")?;
        if head.len() < 2 {
            return Err("a lookup table needs a label column and at least one more".into());
        }
        let mut columns = Vec::new();
        for h in &head {
            let mut it = h.split("##");
            let cname = it.next().unwrap_or("").trim().to_string();
            if cname.is_empty() {
                return Err("every column needs a name in the first row".into());
            }
            let kind = it.next().unwrap_or("").trim().to_ascii_lowercase();
            let unit = it.next().unwrap_or("").trim().to_ascii_lowercase();
            if kind == "length" && mm_per(&unit).is_none() {
                return Err(format!("column '{cname}': unknown length unit '{unit}'"));
            }
            if columns.iter().any(|c: &LookupColumn| c.name.eq_ignore_ascii_case(&cname)) {
                return Err(format!("two columns are called '{cname}'"));
            }
            columns.push(LookupColumn { name: cname, kind, unit });
        }
        let mut rows = Vec::new();
        for (i, line) in lines.enumerate() {
            if line.len() != columns.len() {
                return Err(format!("row {} has {} cells, the header has {}", i + 2, line.len(), columns.len()));
            }
            let mut out = Vec::new();
            for (c, raw) in columns.iter().zip(&line) {
                let raw = raw.trim();
                let numeric = !matches!(c.kind.as_str(), "text" | "multilinetext" | "url" | "material");
                if numeric {
                    if let Ok(x) = raw.parse::<f64>() {
                        let f = if c.kind == "length" { mm_per(&c.unit).unwrap_or(1.0) } else { 1.0 };
                        out.push(Cell::Num(x * f));
                        continue;
                    }
                    // a column that must hold numbers refuses text, but may be left blank
                    if matches!(c.kind.as_str(), "length" | "number" | "integer" | "angle") {
                        if raw.is_empty() {
                            out.push(Cell::Text(String::new()));
                            continue;
                        }
                        return Err(format!("row {}, column '{}': '{raw}' is not a number", i + 2, c.name));
                    }
                }
                out.push(Cell::Text(raw.to_string()));
            }
            rows.push(out);
        }
        Ok(LookupTable { name: name.to_string(), columns, rows })
    }

    /// The table as CSV text, lengths back in the unit the file used.
    pub fn to_csv(&self) -> String {
        let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
        let mut out = String::new();
        let head: Vec<String> = self
            .columns
            .iter()
            .map(|c| {
                if c.kind.is_empty() { q(&c.name) } else { q(&format!("{}##{}##{}", c.name, c.kind, c.unit)) }
            })
            .collect();
        out.push_str(&head.join(","));
        out.push('\n');
        for r in &self.rows {
            let cells: Vec<String> = r
                .iter()
                .zip(&self.columns)
                .map(|(cell, c)| match cell {
                    Cell::Num(x) => {
                        let v = if c.kind == "length" { x / mm_per(&c.unit).unwrap_or(1.0) } else { *x };
                        format!("{}", (v * 1e9).round() / 1e9)
                    }
                    Cell::Text(t) => q(t),
                })
                .collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name.trim()))
    }

    fn matches(cell: &Cell, key: &Value) -> bool {
        match (cell, key) {
            (Cell::Num(a), Value::Num(b)) => (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0),
            (Cell::Num(a), Value::Bool(b)) => *a == if *b { 1.0 } else { 0.0 },
            (Cell::Text(a), Value::Text(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
            (Cell::Text(a), Value::Num(b)) => a.trim().parse::<f64>().map_or(false, |x| (x - b).abs() < 1e-9),
            _ => false,
        }
    }

    /// The row whose second, third, ... cells equal `keys`.
    pub fn find(&self, keys: &[Value]) -> Option<&Vec<Cell>> {
        self.rows.iter().find(|r| keys.iter().enumerate().all(|(i, k)| r.get(i + 1).map_or(false, |c| Self::matches(c, k))))
    }
}

thread_local! {
    static TABLES: RefCell<Rc<Vec<LookupTable>>> = RefCell::new(Rc::new(Vec::new()));
}

/// Keeps a family's tables in reach of `size_lookup` until it is dropped (the previous tables come back after).
pub struct TablesGuard(Rc<Vec<LookupTable>>);

pub fn use_tables(tables: &[LookupTable]) -> TablesGuard {
    let old = TABLES.with(|c| c.replace(Rc::new(tables.to_vec())));
    TablesGuard(old)
}

impl Drop for TablesGuard {
    fn drop(&mut self) {
        TABLES.with(|c| *c.borrow_mut() = self.0.clone());
    }
}

/// `size_lookup(table, column, default, value1, ...)`.
pub fn size_lookup(table: &str, column: &str, default: Value, keys: &[Value]) -> Result<Value, EvalError> {
    let tables = TABLES.with(|c| c.borrow().clone());
    let t = tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(table.trim()))
        .ok_or_else(|| EvalError::BadCall(format!("size_lookup: there is no lookup table called '{table}'")))?;
    let col = t.column(column).ok_or_else(|| EvalError::BadCall(format!("size_lookup: table '{table}' has no column '{column}'")))?;
    if keys.len() + 1 > t.columns.len() {
        return Err(EvalError::BadCall(format!("size_lookup: table '{table}' has only {} columns to match against", t.columns.len() - 1)));
    }
    Ok(match t.find(keys).and_then(|r| r.get(col)) {
        Some(Cell::Num(x)) => Value::Num(*x),
        Some(Cell::Text(s)) => Value::Text(s.clone()),
        None => default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{eval, parse};

    const CSV: &str = "Name,Size##length##inches,OD##length##millimeters,Rating##text\nNP,1,33.4,\"Class 150, std\"\nNP2,2,60.3,Class 300\nNP3,4,114.3,Class 300\n";

    fn run(src: &str) -> Result<Value, EvalError> {
        eval(&parse(src).unwrap(), &mut |n| if n == "Size" { Ok(Value::Num(25.4 * 2.0)) } else { Err(EvalError::UnknownName(n.to_string())) })
    }

    #[test]
    fn a_csv_with_units_reads_in_millimetres_and_writes_back() {
        let t = LookupTable::from_csv("Pipes", CSV).unwrap();
        assert_eq!(t.columns.len(), 4);
        assert_eq!(t.rows[1][1], Cell::Num(50.8));
        assert_eq!(t.rows[0][3], Cell::Text("Class 150, std".into()));
        let again = LookupTable::from_csv("Pipes", &t.to_csv()).unwrap();
        assert_eq!(again, t);
    }

    #[test]
    fn size_lookup_matches_columns_after_the_first_and_falls_back_to_the_default() {
        let t = LookupTable::from_csv("Pipes", CSV).unwrap();
        let _g = use_tables(&[t]);
        assert_eq!(run("size_lookup(Pipes, \"OD\", 0, Size)").unwrap(), Value::Num(60.3));
        assert_eq!(run("size_lookup(\"Pipes\", \"rating\", \"none\", 101.6)").unwrap(), Value::Text("Class 300".into()));
        assert_eq!(run("size_lookup(Pipes, OD, 7, 999)").unwrap(), Value::Num(7.0));
        // two values match two columns
        assert_eq!(run("size_lookup(Pipes, \"Name\", \"?\", 50.8, 60.3)").unwrap(), Value::Text("NP2".into()));
        assert!(matches!(run("size_lookup(Nope, OD, 0, 1)"), Err(EvalError::BadCall(m)) if m.contains("no lookup table")));
        assert!(matches!(run("size_lookup(Pipes, Nope, 0, 1)"), Err(EvalError::BadCall(m)) if m.contains("no column")));
    }

    #[test]
    fn bad_files_are_explained() {
        assert!(LookupTable::from_csv("T", "").is_err());
        assert!(LookupTable::from_csv("T", "A").unwrap_err().contains("label column"));
        assert!(LookupTable::from_csv("T", "A,B##length##furlongs\n1,2").unwrap_err().contains("unit"));
        assert!(LookupTable::from_csv("T", "A,B##number\nx,oops").unwrap_err().contains("not a number"));
        assert!(LookupTable::from_csv("T", "A,B\n1").unwrap_err().contains("cells"));
    }

    #[test]
    fn the_tables_in_reach_are_put_back() {
        let t = LookupTable::from_csv("Pipes", CSV).unwrap();
        {
            let _g = use_tables(&[t]);
        }
        assert!(run("size_lookup(Pipes, OD, 0, 1)").is_err());
    }
}
