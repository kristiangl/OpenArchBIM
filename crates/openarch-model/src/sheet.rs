//! Sheets: the drawings a project issues. A sheet is paper (millimetres, its origin at the title block's bottom left)
//! with a number unique in the project, a name, a title block and the views placed on it. A viewport places one
//! model view on one sheet, its centre on the paper and its size the view's crop at the view's scale; a model view
//! goes on one sheet only (a legend may go on many). The title block's labels read the sheet, the project information
//! and what the sheet works out (its scale: the views' common scale, or "As Indicated").
//!
//! From Kristian's Sheet Views and Document Composition specification (28 Oct). Revisions, schedules on sheets,
//! Activate View, sheet collections and placeholder sheets come later.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::*;
use crate::geom::Point;
use crate::view::ViewKind;

/// The project's own details, which every title block reads.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProjectInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub author: String,
}

/// A drawing sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub number: String,
    pub name: String,
    /// The title block family (an annotation family of the Title Block kind) that sets the paper.
    #[serde(default)]
    pub title_block: Option<Id>,
    #[serde(default)]
    pub drawn_by: String,
    #[serde(default)]
    pub checked_by: String,
    #[serde(default)]
    pub designed_by: String,
    #[serde(default)]
    pub approved_by: String,
    #[serde(default)]
    pub issue_date: String,
    #[serde(default)]
    pub current_revision: String,
    /// Listed in the drawing register (and offered by Export's "Show in sheet list only").
    #[serde(default = "yes")]
    pub appears_in_list: bool,
}

fn yes() -> bool {
    true
}

/// A view placed on a sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub sheet: Id,
    pub view: Id,
    /// Where the middle of the view's crop sits on the paper (mm).
    pub centre: [f64; 2],
    /// Turned on the sheet: 0, 90 (anticlockwise) or 270.
    #[serde(default)]
    pub rotation: i32,
    /// Numbered in order on the sheet: 1, 2, 3...
    #[serde(default)]
    pub detail_number: String,
}

/// A viewport as the sheet draws it: the view, its crop in the view's own terms, and its box on the paper.
#[derive(Debug, Clone, Serialize)]
pub struct PlacedView {
    pub id: Id,
    pub view: Id,
    /// "plan" (plans) or "elevation" (elevations and sections): which drawing the view takes.
    pub drawing: &'static str,
    pub title: String,
    pub detail_number: String,
    pub scale: u32,
    pub rotation: i32,
    pub centre: [f64; 2],
    /// The crop's middle and size in the view's terms (plan x and y, or along and up), mm.
    pub view_centre: [f64; 2],
    pub view_size: [f64; 2],
    /// The box on the paper, mm (turned when the viewport is).
    pub paper_size: [f64; 2],
}

impl Document {
    pub fn project_info(&self) -> ProjectInfo {
        self.iter().find_map(|(_, e)| if let Element::ProjectInfo(p) = e { Some(p.clone()) } else { None }).unwrap_or_default()
    }

    /// Changes the project information (one undo step).
    pub fn set_project_info(&mut self, info: ProjectInfo) -> Result<(), String> {
        let id = self.iter().find_map(|(id, e)| matches!(e, Element::ProjectInfo(_)).then_some(id));
        self.transact("Project Information", |tx| match id {
            Some(id) => tx.update(id, Element::ProjectInfo(info)),
            None => {
                tx.add(Element::ProjectInfo(info));
                Ok(())
            }
        })
    }

    /// The sheets, in number order.
    pub fn sheets(&self) -> Vec<(Id, &Sheet)> {
        let mut v: Vec<(Id, &Sheet)> = self.iter().filter_map(|(id, e)| if let Element::Sheet(s) = e { Some((id, s)) } else { None }).collect();
        v.sort_by(|a, b| natural(&a.1.number).cmp(&natural(&b.1.number)));
        v
    }

    pub fn sheet(&self, id: Id) -> Option<&Sheet> {
        match self.get(id) {
            Some(Element::Sheet(s)) => Some(s),
            _ => None,
        }
    }

    pub fn viewports(&self) -> Vec<(Id, &Viewport)> {
        self.iter().filter_map(|(id, e)| if let Element::Viewport(v) = e { Some((id, v)) } else { None }).collect()
    }

    /// The sheet a view is placed on, and its viewport.
    pub fn sheet_of_view(&self, view: Id) -> Option<(Id, Id)> {
        self.viewports().into_iter().find(|(_, v)| v.view == view).map(|(id, v)| (v.sheet, id))
    }

    /// The next free sheet number after the highest one: its last run of digits one up, as wide as it was.
    pub fn next_sheet_number(&self) -> String {
        let Some(last) = self.sheets().last().map(|(_, s)| s.number.clone()) else { return "A1.01".into() };
        let digits: String = last.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        let stem = &last[..last.len() - digits.len()];
        let n: u64 = digits.parse().unwrap_or(0) + 1;
        let mut out = format!("{stem}{n:0width$}", width = digits.len().max(1));
        while self.sheets().iter().any(|(_, s)| s.number == out) {
            out.push('a');
        }
        out
    }

    /// A new sheet with a title block (one undo step). The number must be new.
    pub fn add_sheet(&mut self, number: &str, name: &str, title_block: Option<Id>) -> Result<Id, String> {
        let number = number.trim();
        if number.is_empty() {
            return Err("A sheet needs a number.".into());
        }
        if self.sheets().iter().any(|(_, s)| s.number == number) {
            return Err(format!("Sheet number {number} is already used: each sheet needs its own."));
        }
        if let Some(t) = title_block {
            if !matches!(self.get(t), Some(Element::AnnotationFamily(f)) if f.kind == crate::annotation::AnnotationKind::TitleBlock) {
                return Err("not a title block".into());
            }
        }
        let s = Sheet {
            number: number.into(),
            name: name.trim().into(),
            title_block,
            drawn_by: String::new(),
            checked_by: String::new(),
            designed_by: String::new(),
            approved_by: String::new(),
            issue_date: String::new(),
            current_revision: String::new(),
            appears_in_list: true,
        };
        self.transact("New Sheet", |tx| Ok(tx.add(Element::Sheet(s))))
    }

    /// Changes a sheet (one undo step); a number already used is refused.
    pub fn set_sheet(&mut self, id: Id, s: Sheet) -> Result<(), String> {
        if self.sheet(id).is_none() {
            return Err("not a sheet".into());
        }
        if s.number.trim().is_empty() || self.sheets().iter().any(|(i, x)| *i != id && x.number == s.number.trim()) {
            return Err(format!("Sheet number {} is already used: each sheet needs its own.", s.number.trim()));
        }
        self.transact("Sheet", |tx| tx.update(id, Element::Sheet(s)))
    }

    /// Places a view on a sheet with its centre at `centre` on the paper (one undo step). A model view goes on one
    /// sheet only; sheets, schedules and 3D views are refused for now.
    pub fn place_view(&mut self, sheet: Id, view: Id, centre: [f64; 2]) -> Result<Id, String> {
        if self.sheet(sheet).is_none() {
            return Err("not a sheet".into());
        }
        let v = self.view(view).ok_or("not a view")?;
        if v.view_kind == ViewKind::ThreeD {
            return Err("3D views go on sheets in a later step.".into());
        }
        if v.view_kind != ViewKind::Legend {
            if let Some((other, _)) = self.sheet_of_view(view) {
                let s = self.sheet(other).map_or(String::new(), |s| format!("{} - {}", s.number, s.name));
                return Err(format!("{} is already on sheet {s}. A view goes on one sheet: duplicate it to place it again.", v.name));
            }
        }
        let n = self.viewports().iter().filter(|(_, p)| p.sheet == sheet).filter_map(|(_, p)| p.detail_number.parse::<u32>().ok()).max().unwrap_or(0) + 1;
        let vp = Viewport { sheet, view, centre, rotation: 0, detail_number: n.to_string() };
        self.transact("Place View", |tx| Ok(tx.add(Element::Viewport(vp))))
    }

    /// Moves or changes a viewport (one undo step).
    pub fn set_viewport(&mut self, id: Id, vp: Viewport) -> Result<(), String> {
        if !matches!(self.get(id), Some(Element::Viewport(_))) {
            return Err("not a viewport".into());
        }
        if ![0, 90, 270].contains(&vp.rotation) {
            return Err("A viewport turns 0, 90 or 270 degrees.".into());
        }
        if vp.detail_number.trim().is_empty() {
            return Err("A viewport needs a detail number.".into());
        }
        self.transact("Viewport", |tx| tx.update(id, Element::Viewport(vp)))
    }

    /// How a viewport draws: the view's crop (or what the model covers) at its scale.
    pub fn placed_view(&self, id: Id) -> Option<PlacedView> {
        let Some(Element::Viewport(vp)) = self.get(id) else { return None };
        self.view_on_paper(vp.view, id, vp.centre, vp.rotation, &vp.detail_number)
    }

    /// A view printed on its own (Export's Views): on a page as big as it is at its scale and 10 mm round it.
    pub fn print_view(&self, view: Id) -> Option<(PlacedView, [f64; 2])> {
        let p = self.view_on_paper(view, 0, [0.0, 0.0], 0, "")?;
        let paper = [p.paper_size[0] + 20.0, p.paper_size[1] + 20.0];
        Some((PlacedView { centre: [paper[0] / 2.0, paper[1] / 2.0], ..p }, paper))
    }

    fn view_on_paper(&self, view: Id, id: Id, centre: [f64; 2], rotation: i32, detail_number: &str) -> Option<PlacedView> {
        let v = self.view(view)?;
        let scale = v.settings.scale.max(1);
        let (drawing, c, size) = match v.view_kind {
            ViewKind::Elevation | ViewKind::Section => {
                let f = self.elevation_frame(view)?;
                // the view runs from -left_mm to right_mm along it
                ("elevation", [(f.right_mm - f.left_mm) / 2.0, (f.z_min + f.z_max) / 2.0], [f.left_mm + f.right_mm, f.z_max - f.z_min])
            }
            _ => {
                let (lo, hi) = match (v.crop.enabled, v.crop.region) {
                    (true, Some(r)) => r,
                    _ => {
                        let (lo, hi) = self.model_bounds().unwrap_or((Point::new(0.0, 0.0), Point::new(10000.0, 10000.0)));
                        (Point::new(lo.x - 1500.0, lo.y - 1500.0), Point::new(hi.x + 1500.0, hi.y + 1500.0))
                    }
                };
                ("plan", [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0], [hi.x - lo.x, hi.y - lo.y])
            }
        };
        let paper = [size[0] / scale as f64, size[1] / scale as f64];
        let paper = if rotation == 0 { paper } else { [paper[1], paper[0]] };
        let title = if v.title_on_sheet.trim().is_empty() { v.name.clone() } else { v.title_on_sheet.clone() };
        Some(PlacedView { id, view, drawing, title, detail_number: detail_number.into(), scale, rotation, centre, view_centre: c, view_size: size, paper_size: paper })
    }

    /// The views placed on a sheet.
    pub fn sheet_views(&self, sheet: Id) -> Vec<PlacedView> {
        let mut out: Vec<PlacedView> = self.viewports().into_iter().filter(|(_, v)| v.sheet == sheet).filter_map(|(id, _)| self.placed_view(id)).collect();
        out.sort_by(|a, b| natural(&a.detail_number).cmp(&natural(&b.detail_number)));
        out
    }

    /// The sheet's scale for its title block: the views' common scale, "As Indicated" when they differ, empty with
    /// no views.
    pub fn sheet_scale(&self, sheet: Id) -> String {
        let mut scales: Vec<u32> = self.sheet_views(sheet).iter().map(|v| v.scale).collect();
        scales.sort();
        scales.dedup();
        match scales.as_slice() {
            [] => String::new(),
            [s] => format!("1 : {s}"),
            _ => "As Indicated".into(),
        }
    }

    /// What a title block label reads on a sheet.
    pub fn sheet_param(&self, sheet: Id, param: &str) -> Option<String> {
        let s = self.sheet(sheet)?;
        let p = self.project_info();
        let v = match param {
            "Sheet Number" => s.number.clone(),
            "Sheet Name" => s.name.clone(),
            "Drawn By" => s.drawn_by.clone(),
            "Checked By" => s.checked_by.clone(),
            "Designed By" => s.designed_by.clone(),
            "Approved By" => s.approved_by.clone(),
            "Sheet Issue Date" => s.issue_date.clone(),
            "Current Revision" => s.current_revision.clone(),
            "Scale" => self.sheet_scale(sheet),
            "Project Name" => p.name,
            "Project Number" => p.number,
            "Client Name" => p.client,
            "Project Address" => p.address,
            "Project Status" => p.status,
            "Author" => p.author,
            _ => return None,
        };
        Some(v)
    }

    /// The paper a sheet's title block sets (A1 when it has none), width and height in mm.
    pub fn sheet_paper(&self, sheet: Id) -> [f64; 2] {
        self.sheet(sheet)
            .and_then(|s| s.title_block)
            .and_then(|t| match self.get(t) {
                Some(Element::AnnotationFamily(f)) => f.paper_mm,
                _ => None,
            })
            .unwrap_or([841.0, 594.0])
    }
}

/// A sort key that puts A1.2 before A1.10: runs of digits compare by value.
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = vec![];
    let mut text = String::new();
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                num.clear();
            }
            text.push(c);
        }
    }
    out.push((text, num.parse().unwrap_or(0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_hold_views_once_and_read_their_title_block_values() {
        let mut d = Document::new_project();
        let tb = d.annotation_families().into_iter().find(|(_, f)| f.kind == crate::annotation::AnnotationKind::TitleBlock && f.name.contains("A1")).map(|(id, _)| id);
        assert!(tb.is_some(), "a new project has title blocks");
        let a = d.add_sheet("A1.01", "Site Plan", tb).unwrap();
        assert_eq!(d.next_sheet_number(), "A1.02");
        assert!(d.add_sheet("A1.01", "Again", tb).is_err(), "numbers are unique");
        let b = d.add_sheet(&d.next_sheet_number(), "Floor Plan", tb).unwrap();
        d.add_sheet("A1.10", "Later", tb).unwrap();
        assert_eq!(d.sheets().iter().map(|(_, s)| s.number.as_str()).collect::<Vec<_>>(), vec!["A1.01", "A1.02", "A1.10"]);
        assert_eq!(d.sheet_paper(a), [841.0, 594.0]);
        // a plan goes on one sheet only
        let plan = d.views().into_iter().find(|(_, v)| v.view_kind == ViewKind::FloorPlan).unwrap().0;
        let vp = d.place_view(a, plan, [400.0, 300.0]).unwrap();
        assert!(d.place_view(b, plan, [400.0, 300.0]).unwrap_err().contains("duplicate"));
        assert_eq!(d.sheet_of_view(plan), Some((a, vp)));
        let south = d.views().into_iter().find(|(_, v)| v.name == "South").unwrap().0;
        d.place_view(a, south, [600.0, 200.0]).unwrap();
        let placed = d.sheet_views(a);
        assert_eq!(placed.iter().map(|p| (p.drawing, p.detail_number.as_str())).collect::<Vec<_>>(), vec![("plan", "1"), ("elevation", "2")]);
        // the views share 1 : 100; one at 1 : 50 makes it As Indicated
        assert_eq!(d.sheet_param(a, "Scale").as_deref(), Some("1 : 100"));
        let mut v = d.view(south).unwrap().clone();
        v.settings.scale = 50;
        d.transact("scale", |tx| tx.update(south, Element::View(v))).unwrap();
        assert_eq!(d.sheet_param(a, "Scale").as_deref(), Some("As Indicated"));
        // the project information reaches every sheet
        d.set_project_info(ProjectInfo { name: "Cornish House".into(), number: "367".into(), ..Default::default() }).unwrap();
        assert_eq!((d.sheet_param(b, "Project Number").as_deref(), d.sheet_param(b, "Sheet Name").as_deref()), (Some("367"), Some("Floor Plan")));
        // a viewport's box is the view's crop at its scale
        let mut pv = d.view(plan).unwrap().clone();
        pv.crop = crate::view::Crop { enabled: true, region: Some((Point::new(0.0, 0.0), Point::new(20000.0, 10000.0))), ..pv.crop };
        d.transact("crop", |tx| tx.update(plan, Element::View(pv))).unwrap();
        assert_eq!(d.placed_view(vp).unwrap().paper_size, [200.0, 100.0]);
    }
}
