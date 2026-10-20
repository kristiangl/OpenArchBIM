//! Shared parameter files: a list of parameters, each with a GUID that stays the same in every
//! family and project that uses it, so schedules and tags can pick them up across all of them.
//!
//! The file is tab separated text. Lines starting with `*` name the columns of the lines that
//! follow, `#` lines are comments:
//!
//! ```text
//! *META   VERSION MINVERSION
//! META    2       1
//! *GROUP  ID      NAME
//! GROUP   1       Dimensions
//! *PARAM  GUID NAME DATATYPE DATACATEGORY GROUP VISIBLE DESCRIPTION USERMODIFIABLE HIDEWHENNOVALUE
//! PARAM   <guid> Width LENGTH  1 1 The width 1 0
//! ```

use crate::family::ParamKind;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedGroup {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedParam {
    pub guid: String,
    pub name: String,
    /// The data type as the file spells it (LENGTH, TEXT, YESNO, ...). Kept as written so a type the
    /// editor has no parameter kind for survives a round trip.
    pub datatype: String,
    #[serde(default)]
    pub data_category: String,
    pub group: u32,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default = "yes")]
    pub user_modifiable: bool,
    #[serde(default)]
    pub hide_when_no_value: bool,
}

impl SharedParam {
    /// The family parameter kind this data type becomes, or `None` for a type families do not have yet.
    pub fn kind(&self) -> Option<ParamKind> {
        Some(match self.datatype.to_ascii_uppercase().as_str() {
            "TEXT" | "MULTILINETEXT" | "URL" => ParamKind::Text,
            "LENGTH" => ParamKind::Length,
            "NUMBER" => ParamKind::Number,
            "INTEGER" => ParamKind::Integer,
            "ANGLE" => ParamKind::Angle,
            "AREA" => ParamKind::Area,
            "VOLUME" => ParamKind::Volume,
            "YESNO" => ParamKind::YesNo,
            "MATERIAL" => ParamKind::Material,
            _ => return None,
        })
    }
}

pub fn datatype_of(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Text => "TEXT",
        ParamKind::Length => "LENGTH",
        ParamKind::Number => "NUMBER",
        ParamKind::Integer => "INTEGER",
        ParamKind::Angle => "ANGLE",
        ParamKind::Area => "AREA",
        ParamKind::Volume => "VOLUME",
        ParamKind::YesNo => "YESNO",
        ParamKind::Material => "MATERIAL",
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SharedParamFile {
    pub groups: Vec<SharedGroup>,
    pub params: Vec<SharedParam>,
}

/// File bytes as text: UTF-8, or UTF-16 when the file starts with a byte order mark.
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |b: &[u8], le: bool| -> Result<String, String> {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16(&units).map_err(|_| "the file is not valid text".to_string())
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16(&bytes[2..], false);
    }
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(b.to_vec()).map_err(|_| "the file is not valid text".to_string())
}

impl SharedParamFile {
    pub fn parse(text: &str) -> Result<SharedParamFile, String> {
        let mut f = SharedParamFile::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with('*') {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            let at = |i: usize| c.get(i).copied().unwrap_or("");
            match c[0] {
                "META" => {}
                "GROUP" => {
                    let id: u32 = at(1).trim().parse().map_err(|_| format!("line {}: a group needs a number", n + 1))?;
                    if f.groups.iter().any(|g| g.id == id) {
                        return Err(format!("line {}: group {id} appears twice", n + 1));
                    }
                    f.groups.push(SharedGroup { id, name: at(2).trim().to_string() });
                }
                "PARAM" => {
                    let group: u32 = at(5).trim().parse().map_err(|_| format!("line {}: a parameter needs a group number", n + 1))?;
                    if at(1).trim().is_empty() || at(2).trim().is_empty() {
                        return Err(format!("line {}: a parameter needs a GUID and a name", n + 1));
                    }
                    f.params.push(SharedParam {
                        guid: at(1).trim().to_ascii_lowercase(),
                        name: at(2).trim().to_string(),
                        datatype: at(3).trim().to_string(),
                        data_category: at(4).trim().to_string(),
                        group,
                        visible: at(6).trim() != "0",
                        description: at(7).to_string(),
                        user_modifiable: at(8).trim() != "0",
                        hide_when_no_value: at(9).trim() == "1",
                    });
                }
                other => return Err(format!("line {}: unknown entry '{other}'", n + 1)),
            }
        }
        for p in &f.params {
            if !f.groups.iter().any(|g| g.id == p.group) {
                return Err(format!("parameter '{}' is in group {}, which the file does not define", p.name, p.group));
            }
        }
        Ok(f)
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("# This is a shared parameter file.\n# Do not edit manually.\n*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\n");
        for g in &self.groups {
            s.push_str(&format!("GROUP\t{}\t{}\n", g.id, g.name));
        }
        s.push_str("*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\n");
        for p in &self.params {
            let clean = |t: &str| t.replace(['\t', '\n', '\r'], " ");
            s.push_str(&format!("PARAM\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", p.guid, clean(&p.name), p.datatype, p.data_category, p.group, p.visible as u8, clean(&p.description), p.user_modifiable as u8, p.hide_when_no_value as u8));
        }
        s
    }

    pub fn find(&self, guid: &str) -> Option<&SharedParam> {
        self.params.iter().find(|p| p.guid.eq_ignore_ascii_case(guid))
    }

    fn check_name(name: &str) -> Result<String, String> {
        let n = name.trim();
        if n.is_empty() {
            return Err("give it a name".into());
        }
        if n.contains(['[', ']', '\t']) {
            return Err("a name cannot contain [, ] or a tab".into());
        }
        Ok(n.to_string())
    }

    pub fn add_group(&mut self, name: &str) -> Result<u32, String> {
        let n = Self::check_name(name)?;
        if self.groups.iter().any(|g| g.name.eq_ignore_ascii_case(&n)) {
            return Err(format!("there is already a group called '{n}'"));
        }
        let id = self.groups.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        self.groups.push(SharedGroup { id, name: n });
        Ok(id)
    }

    pub fn rename_group(&mut self, id: u32, name: &str) -> Result<(), String> {
        let n = Self::check_name(name)?;
        if self.groups.iter().any(|g| g.id != id && g.name.eq_ignore_ascii_case(&n)) {
            return Err(format!("there is already a group called '{n}'"));
        }
        self.groups.iter_mut().find(|g| g.id == id).ok_or("no such group")?.name = n;
        Ok(())
    }

    pub fn remove_group(&mut self, id: u32) -> Result<(), String> {
        if self.params.iter().any(|p| p.group == id) {
            return Err("the group still has parameters: move or delete them first".into());
        }
        let before = self.groups.len();
        self.groups.retain(|g| g.id != id);
        if self.groups.len() == before {
            return Err("no such group".into());
        }
        Ok(())
    }

    /// A new parameter with a fresh GUID. Names are unique within a group.
    pub fn add_param(&mut self, group: u32, name: &str, datatype: &str, description: &str) -> Result<String, String> {
        let n = Self::check_name(name)?;
        if !self.groups.iter().any(|g| g.id == group) {
            return Err("no such group".into());
        }
        if datatype.trim().is_empty() {
            return Err("choose a type".into());
        }
        if self.params.iter().any(|p| p.group == group && p.name.eq_ignore_ascii_case(&n)) {
            return Err(format!("the group already has a parameter called '{n}'"));
        }
        let guid = uuid::Uuid::new_v4().to_string();
        self.params.push(SharedParam { guid: guid.clone(), name: n, datatype: datatype.trim().to_ascii_uppercase(), data_category: String::new(), group, visible: true, description: description.to_string(), user_modifiable: true, hide_when_no_value: false });
        Ok(guid)
    }

    /// Change a parameter's name, group or description. The data type stays: families already hold values of it.
    pub fn edit_param(&mut self, guid: &str, name: Option<&str>, group: Option<u32>, description: Option<&str>) -> Result<(), String> {
        let i = self.params.iter().position(|p| p.guid.eq_ignore_ascii_case(guid)).ok_or("no such parameter")?;
        let new_name = match name {
            Some(n) => Self::check_name(n)?,
            None => self.params[i].name.clone(),
        };
        let new_group = group.unwrap_or(self.params[i].group);
        if !self.groups.iter().any(|g| g.id == new_group) {
            return Err("no such group".into());
        }
        if self.params.iter().enumerate().any(|(j, p)| j != i && p.group == new_group && p.name.eq_ignore_ascii_case(&new_name)) {
            return Err(format!("the group already has a parameter called '{new_name}'"));
        }
        let p = &mut self.params[i];
        p.name = new_name;
        p.group = new_group;
        if let Some(d) = description {
            p.description = d.to_string();
        }
        Ok(())
    }

    pub fn remove_param(&mut self, guid: &str) -> Result<(), String> {
        let before = self.params.len();
        self.params.retain(|p| !p.guid.eq_ignore_ascii_case(guid));
        if self.params.len() == before {
            return Err("no such parameter".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# This is a shared parameter file.\n*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tDimensions\nGROUP\t2\tIdentity\n*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\nPARAM\t11111111-2222-3333-4444-555555555555\tRough Width\tLENGTH\t\t1\t1\tOpening width\t1\t0\nPARAM\t66666666-2222-3333-4444-555555555555\tSupplier\tTEXT\t\t2\t1\t\t0\t1\nPARAM\t77777777-2222-3333-4444-555555555555\tPlan\tFAMILYTYPE\t\t2\t1\t\t1\t0\n";

    #[test]
    fn a_file_reads_and_writes_back_the_same() {
        let f = SharedParamFile::parse(FILE).unwrap();
        assert_eq!(f.groups.len(), 2);
        assert_eq!(f.params.len(), 3);
        assert_eq!(f.params[0].kind(), Some(ParamKind::Length));
        assert_eq!(f.params[1].kind(), Some(ParamKind::Text));
        assert!(!f.params[1].user_modifiable && f.params[1].hide_when_no_value);
        assert_eq!(f.params[2].kind(), None, "a type families do not have yet is kept, not dropped");
        let again = SharedParamFile::parse(&f.to_text()).unwrap();
        assert_eq!(again, f);
        assert!(f.to_text().contains("FAMILYTYPE"));
    }

    #[test]
    fn utf16_files_read() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in FILE.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(SharedParamFile::parse(&decode(&bytes).unwrap()).unwrap().params.len(), 3);
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(FILE.as_bytes());
        assert_eq!(decode(&with_bom).unwrap(), FILE);
    }

    #[test]
    fn groups_and_parameters_are_edited_with_their_rules() {
        let mut f = SharedParamFile::default();
        let g = f.add_group("Dimensions").unwrap();
        assert!(f.add_group("dimensions").is_err(), "group names are unique");
        let a = f.add_param(g, "Width", "length", "").unwrap();
        assert_eq!(f.find(&a).unwrap().datatype, "LENGTH");
        assert_eq!(a.len(), 36, "a GUID");
        assert!(f.add_param(g, "WIDTH", "length", "").is_err(), "names are unique within a group");
        let g2 = f.add_group("Other").unwrap();
        let b = f.add_param(g2, "Width", "text", "").unwrap();
        assert_ne!(a, b, "the same name in another group is a different parameter");
        assert!(f.remove_group(g).unwrap_err().contains("still has parameters"));
        f.edit_param(&a, Some("Overall Width"), None, Some("across the frame")).unwrap();
        assert_eq!(f.find(&a).unwrap().name, "Overall Width");
        assert!(f.edit_param(&a, None, Some(g2), None).is_ok());
        assert!(f.edit_param(&a, Some("Width"), None, None).unwrap_err().contains("already"), "it would clash with the other Width in that group");
        assert!(f.add_param(g, "Bad[Name]", "length", "").is_err());
        f.remove_param(&a).unwrap();
        f.remove_param(&b).unwrap();
        assert!(f.remove_param(&b).is_err());
        f.remove_group(g).unwrap();
    }

    #[test]
    fn broken_files_are_explained() {
        assert!(SharedParamFile::parse("GROUP\tx\tA\n").unwrap_err().contains("line 1"));
        assert!(SharedParamFile::parse("PARAM\tg\tN\tLENGTH\t\t9\t1\t\t1\t0\n").unwrap_err().contains("group 9"));
        assert!(SharedParamFile::parse("WHAT\t1\n").unwrap_err().contains("unknown entry"));
    }
}
