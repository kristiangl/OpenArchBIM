//! Project Units (Manage > Project Units): how lengths, areas, volumes and angles are written across the project, and
//! the decimal symbol and digit grouping. Dimension types follow the length format unless set to their own; room areas
//! and volumes, tags and schedules write through these.

use serde::{Deserialize, Serialize};

use crate::dimension::{format_length, num, LengthFormat};
use crate::doc::Document;
use crate::element::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaUnit {
    #[default]
    SquareMetres,
    SquareMillimetres,
    SquareFeet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeUnit {
    #[default]
    CubicMetres,
    CubicFeet,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AreaFormat {
    #[serde(default)]
    pub unit: AreaUnit,
    #[serde(default = "two")]
    pub decimals: u8,
    #[serde(default = "yes")]
    pub symbol: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VolumeFormat {
    #[serde(default)]
    pub unit: VolumeUnit,
    #[serde(default = "two")]
    pub decimals: u8,
    #[serde(default = "yes")]
    pub symbol: bool,
}

fn two() -> u8 {
    2
}
fn yes() -> bool {
    true
}

impl Default for AreaFormat {
    fn default() -> Self {
        AreaFormat { unit: AreaUnit::SquareMetres, decimals: 2, symbol: true }
    }
}
impl Default for VolumeFormat {
    fn default() -> Self {
        VolumeFormat { unit: VolumeUnit::CubicMetres, decimals: 2, symbol: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectUnits {
    #[serde(default)]
    pub length: LengthFormat,
    #[serde(default)]
    pub area: AreaFormat,
    #[serde(default)]
    pub volume: VolumeFormat,
    /// Decimal places of an angle, in degrees.
    #[serde(default = "two")]
    pub angle_decimals: u8,
    /// Write 2,5 for 2.5 (grouping then uses full stops).
    #[serde(default)]
    pub decimal_comma: bool,
    /// Write 12,000 for 12000.
    #[serde(default)]
    pub digit_grouping: bool,
}

impl Default for ProjectUnits {
    fn default() -> Self {
        ProjectUnits { length: LengthFormat::default(), area: AreaFormat::default(), volume: VolumeFormat::default(), angle_decimals: 2, decimal_comma: false, digit_grouping: false }
    }
}

impl ProjectUnits {
    /// A number as written with a full stop, given this project's decimal symbol and grouping. Feet and inches pass
    /// through untouched except for the decimal symbol.
    pub fn local(&self, s: &str) -> String {
        if !self.decimal_comma && !self.digit_grouping {
            return s.to_string();
        }
        let (point, group) = if self.decimal_comma { (',', '.') } else { ('.', ',') };
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len() + 4);
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                // a run of digits, perhaps with a decimal part
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let whole: String = chars[start..i].iter().collect();
                let after_point = start > 0 && chars[start - 1] == '.';
                if self.digit_grouping && !after_point && whole.len() > 3 {
                    let n = whole.len();
                    for (k, c) in whole.chars().enumerate() {
                        if k > 0 && (n - k) % 3 == 0 {
                            out.push(group);
                        }
                        out.push(c);
                    }
                } else {
                    out.push_str(&whole);
                }
                if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                    out.push(point);
                    i += 1;
                }
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    /// A length (mm) in `f`, written in this project's way.
    pub fn write_length(&self, mm: f64, f: &LengthFormat) -> String {
        self.local(&format_length(mm, f))
    }
    pub fn length(&self, mm: f64) -> String {
        self.write_length(mm, &self.length)
    }
    /// An area given in mm².
    pub fn area(&self, mm2: f64) -> String {
        let (v, sym) = match self.area.unit {
            AreaUnit::SquareMetres => (mm2 / 1e6, " m\u{b2}"),
            AreaUnit::SquareMillimetres => (mm2, " mm\u{b2}"),
            AreaUnit::SquareFeet => (mm2 / (304.8 * 304.8), " ft\u{b2}"),
        };
        self.local(&format!("{}{}", num(v, self.area.decimals, false), if self.area.symbol { sym } else { "" }))
    }
    /// A volume given in mm³.
    pub fn volume(&self, mm3: f64) -> String {
        let (v, sym) = match self.volume.unit {
            VolumeUnit::CubicMetres => (mm3 / 1e9, " m\u{b3}"),
            VolumeUnit::CubicFeet => (mm3 / (304.8 * 304.8 * 304.8), " ft\u{b3}"),
        };
        self.local(&format!("{}{}", num(v, self.volume.decimals, false), if self.volume.symbol { sym } else { "" }))
    }
    pub fn angle(&self, deg: f64) -> String {
        self.local(&format!("{}\u{b0}", num(deg, self.angle_decimals, true)))
    }
}

impl Document {
    pub fn project_units(&self) -> ProjectUnits {
        self.iter().find_map(|(_, e)| match e { Element::ProjectInfo(p) => Some(p.units), _ => None }).unwrap_or_default()
    }

    /// The Project Units dialog's OK: one undo step.
    pub fn set_project_units(&mut self, u: ProjectUnits) -> Result<(), String> {
        if u.length.decimals > 6 || u.area.decimals > 6 || u.volume.decimals > 6 || u.angle_decimals > 6 {
            return Err("Up to 6 decimal places.".into());
        }
        if !matches!(u.length.fraction, 1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 | 256) {
            return Err("Inches round to 1/2, 1/4 ... 1/256.".into());
        }
        if u == self.project_units() {
            return Ok(());
        }
        let found = self.iter().find_map(|(id, e)| match e { Element::ProjectInfo(p) => Some((id, p.clone())), _ => None });
        self.transact("Project Units", |tx| {
            match found {
                Some((id, mut p)) => {
                    p.units = u;
                    tx.update(id, Element::ProjectInfo(p))?;
                }
                None => {
                    tx.add(Element::ProjectInfo(crate::sheet::ProjectInfo { units: u, ..Default::default() }));
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::LengthUnit;

    #[test]
    fn project_units_write_lengths_areas_and_volumes() {
        let u = ProjectUnits::default();
        assert_eq!((u.length(2500.0), u.area(12.5e6), u.volume(30e9), u.angle(45.0)), ("2500".into(), "12.50 m\u{b2}".into(), "30.00 m\u{b3}".into(), "45\u{b0}".into()));
        let eu = ProjectUnits { decimal_comma: true, digit_grouping: true, length: LengthFormat { unit: LengthUnit::Metres, decimals: 3, suppress_trailing_zeros: false, ..LengthFormat::default() }, ..u };
        assert_eq!((eu.length(12500.0), eu.area(1234.5e6), eu.length(1234567.0)), ("12,500".into(), "1.234,50 m\u{b2}".into(), "1.234,567".into()));
        let au = ProjectUnits { digit_grouping: true, ..u };
        assert_eq!((au.length(12000.0), au.length(950.0), au.area(1500.25e6)), ("12,000".into(), "950".into(), "1,500.25 m\u{b2}".into()));
        let ft = ProjectUnits { area: AreaFormat { unit: AreaUnit::SquareFeet, decimals: 0, symbol: true }, ..u };
        assert_eq!(ft.area(304.8 * 304.8 * 100.0), "100 ft\u{b2}");

        let mut d = Document::new_project();
        assert_eq!(d.project_units(), ProjectUnits::default());
        d.set_project_units(au).unwrap();
        assert!(d.project_units().digit_grouping);
        assert!(d.set_project_units(ProjectUnits { angle_decimals: 9, ..au }).is_err());
        d.undo().unwrap();
        assert!(!d.project_units().digit_grouping);
    }
}
