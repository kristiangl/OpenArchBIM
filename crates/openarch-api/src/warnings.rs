//! Review Warnings (Manage > Inquiry > Warnings): the problems the project has now, each with the elements it is
//! about, gathered from the checks that already exist (overlapping walls, rooms, stairs, ramps, railings).
use serde_json::{json, Value};

use openarch_model::rooms::RoomStatus;
use openarch_model::{Document, Element, Id};

use crate::{Res, Session};

/// A short name for an element in the list: its kind and what tells it apart (a room's number and name, a wall's type).
fn label(d: &Document, id: Id) -> String {
    match d.get(id) {
        Some(Element::Room(r)) => format!("Room: {} {}", r.number, r.name).trim_end().to_string(),
        Some(Element::Wall(w)) => format!("Wall: {}", d.wall_type(w.wall_type).map_or("?", |t| t.name.as_str())),
        Some(e) => e.kind().to_string(),
        None => "?".to_string(),
    }
}

impl Session {
    pub(crate) fn handle_warnings(&mut self, method: &str, _p: &Value) -> Option<Res> {
        Some(match method {
            "review_warnings" => Ok(self.review_warnings()),
            _ => return None,
        })
    }

    /// Every warning: { message, ids, labels }, grouped by message in the order the checks run.
    fn review_warnings(&self) -> Value {
        let d = &self.doc;
        let mut out: Vec<(String, Vec<Id>)> = vec![];
        // overlapping walls, each pair once
        let mut seen = std::collections::BTreeSet::new();
        for (id, e) in d.iter() {
            if !matches!(e, Element::Wall(_)) {
                continue;
            }
            if let Some(other) = d.wall_overlap_with(id) {
                if seen.insert((id.min(other), id.max(other))) {
                    out.push(("Highlighted walls overlap. One of them may need to be deleted.".into(), vec![id, other]));
                }
            }
        }
        for (id, _) in d.rooms() {
            let Some(c) = d.room_calculation(id) else { continue };
            match c.status {
                RoomStatus::NotEnclosed => out.push(("Room is not in a properly enclosed region.".into(), vec![id])),
                RoomStatus::Redundant => out.push(("Multiple rooms are in the same enclosed region. The extra ones are Redundant.".into(), vec![id])),
                _ => {}
            }
            if let Some(other) = c.option_conflict {
                if seen.insert((id.min(other), id.max(other))) {
                    out.push(("A main model room and a design option room are in the same space.".into(), vec![id, other]));
                }
            }
        }
        // stairs, ramps and railings say what rule they break in their own Properties
        let checked: Vec<Id> = d.iter().filter(|(_, e)| matches!(e, Element::Stair(_) | Element::Ramp(_) | Element::Railing(_))).map(|(id, _)| id).collect();
        for id in checked {
            let Ok(p) = self.describe(id) else { continue };
            for m in p["warnings"].as_array().into_iter().flatten().filter_map(|m| m.as_str()) {
                out.push((m.to_string(), vec![id]));
            }
        }
        json!({
            "count": out.len(),
            "warnings": out.iter().map(|(m, ids)| json!({
                "message": m, "ids": ids, "labels": ids.iter().map(|i| label(d, *i)).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}
