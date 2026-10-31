//! Project file: one SQLite database. Elements are stored as JSON rows, which
//! keeps the format easy to evolve while the model is young.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::doc::Document;
use crate::element::{Element, Id};

/// 2: elevation and 3D views draw in the Hidden Line style (before this the setting had no effect).
/// 3: plans no longer put a pale wash over floors.
const FORMAT_VERSION: i64 = 7;

impl Document {
    /// Write the project. The file is written beside the target and renamed into
    /// place, so a crash mid-save never leaves a half-written project.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("oab.tmp");
        let _ = std::fs::remove_file(&tmp);
        {
            let mut conn = Connection::open(&tmp).map_err(|e| e.to_string())?;
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE elements (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, data TEXT NOT NULL);",
            )
            .map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO meta VALUES ('format_version', ?1)", params![FORMAT_VERSION.to_string()])
                .map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO meta VALUES ('next_id', ?1)", params![self.next_id.to_string()])
                .map_err(|e| e.to_string())?;
            for (id, e) in self.iter() {
                let json = serde_json::to_string(e).map_err(|e| e.to_string())?;
                tx.execute("INSERT INTO elements VALUES (?1, ?2, ?3)", params![id as i64, e.kind(), json])
                    .map_err(|e| e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn open(path: &Path) -> Result<Document, String> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
        let meta = |k: &str| -> Result<String, String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", params![k], |r| r.get(0))
                .map_err(|_| format!("not an OpenArch BIM project (missing {k})"))
        };
        let v: i64 = meta("format_version")?.parse().map_err(|_| "bad format version".to_string())?;
        if v > FORMAT_VERSION {
            return Err(format!("project was saved by a newer version of OpenArch BIM (format {v})"));
        }
        let mut d = Document::default();
        d.next_id = meta("next_id")?.parse().map_err(|_| "bad next_id".to_string())?;
        let mut stmt = conn.prepare("SELECT id, data FROM elements").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, json) = row.map_err(|e| e.to_string())?;
            let e: Element = serde_json::from_str(&json).map_err(|e| format!("element {id}: {e}"))?;
            d.elements.insert(id as Id, e);
        }
        d.ensure_starter_families(); // projects saved before doors and windows
        d.ensure_roof_defaults(); // projects saved before roofs
        d.ensure_style_defaults(); // projects saved before object styles
        d.ensure_site_defaults(); // projects saved before the base point, Site plan and elevations
        d.ensure_site_point_overrides(); // projects saved before the base and survey points had categories
        d.ensure_annotation_defaults(); // projects saved before annotation families
        d.ensure_view_types(); // projects saved before view types
        d.ensure_detail_defaults(); // projects saved before detail items
        d.ensure_curtain_types(); // projects saved before curtain walls
        if v < 2 {
            d.use_hidden_line_in_elevations_and_3d(); // projects saved before the visual style was drawn
        }
        if v < 3 {
            d.clear_old_floor_wash(); // projects saved while new plans washed floors pale blue
        }
        if v < 7 {
            d.crop_elevations_and_sections(); // projects saved while elevations and sections were always cropped
        }
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    #[test]
    fn save_and_open_round_trip() {
        let mut d = Document::new_project();
        let (lvl, wt, ft) = (d.levels()[0].0, d.wall_types()[1].0, d.floor_types()[0].0);
        d.add_wall(wt, lvl, Point::new(0., 0.), Point::new(4000., 0.), 2700.).unwrap();
        d.add_floor(ft, lvl, vec![Point::new(0., 0.), Point::new(4000., 0.), Point::new(4000., 3000.)]).unwrap();
        let dir = std::env::temp_dir().join(format!("openarch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.oab");
        d.save(&path).unwrap();
        assert!(!path.with_extension("oab.tmp").exists());
        let o = Document::open(&path).unwrap();
        assert_eq!(o.iter().count(), d.iter().count());
        for (id, e) in d.iter() {
            assert_eq!(o.get(id), Some(e));
        }
        // ids keep counting from where the saved project left off
        let mut o = o;
        let new = o.add_wall(wt, lvl, Point::new(0., 0.), Point::new(1000., 0.), 2700.).unwrap();
        assert!(d.iter().all(|(id, _)| id != new));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn elevations_and_3d_start_in_hidden_line_and_older_projects_move_to_it_once() {
        use crate::view::{ViewKind, VisualStyle};
        let mut d = Document::new_project();
        let style_of = |d: &Document, k: ViewKind| d.views().into_iter().filter(|(_, v)| v.view_kind == k).map(|(_, v)| v.settings.visual_style).collect::<Vec<_>>();
        assert!(style_of(&d, ViewKind::Elevation).iter().all(|s| *s == VisualStyle::HiddenLine) && !style_of(&d, ViewKind::Elevation).is_empty());
        assert!(style_of(&d, ViewKind::ThreeD).iter().all(|s| *s == VisualStyle::HiddenLine));
        // a project saved before the style was drawn has Shaded everywhere (the default then)
        let ids: Vec<Id> = d.views().into_iter().filter(|(_, v)| matches!(v.view_kind, ViewKind::Elevation | ViewKind::ThreeD)).map(|(i, _)| i).collect();
        for id in &ids {
            let mut v = d.view(*id).unwrap().clone();
            v.settings.visual_style = VisualStyle::Shaded;
            d.transact("old", |tx| tx.update(*id, Element::View(v))).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("openarch-test-style-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.oab");
        d.save(&path).unwrap();
        let old = Connection::open(&path).unwrap();
        old.execute("UPDATE meta SET value = '1' WHERE key = 'format_version'", []).unwrap();
        drop(old);
        let o = Document::open(&path).unwrap();
        assert!(style_of(&o, ViewKind::Elevation).iter().chain(style_of(&o, ViewKind::ThreeD).iter()).all(|s| *s == VisualStyle::HiddenLine));
        // saved again in the current format, a style chosen afterwards stays
        let mut o = o;
        let id = ids[0];
        let mut v = o.view(id).unwrap().clone();
        v.settings.visual_style = VisualStyle::Shaded;
        o.transact("choose", |tx| tx.update(id, Element::View(v))).unwrap();
        o.save(&path).unwrap();
        let again = Document::open(&path).unwrap();
        assert_eq!(again.view(id).unwrap().settings.visual_style, VisualStyle::Shaded);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plans_no_longer_wash_floors_and_older_projects_lose_the_wash_once() {
        use crate::view::{View, ViewKind};
        let mut d = Document::new_project();
        let plans: Vec<Id> = d.views().into_iter().filter(|(_, v)| v.view_kind == ViewKind::FloorPlan).map(|(i, _)| i).collect();
        assert!(plans.iter().all(|p| !d.view(*p).unwrap().settings.overrides.model.contains_key("Floors")));
        // an older project: every plan has the wash, except one where the person changed it
        for (k, id) in plans.iter().enumerate() {
            let mut v = d.view(*id).unwrap().clone();
            let mut wash = View::old_floor_wash();
            if k == 0 {
                wash.transparency = 40;
            }
            v.settings.overrides.model.insert("Floors".into(), wash);
            d.transact("old", |tx| tx.update(*id, Element::View(v))).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("openarch-test-wash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.oab");
        d.save(&path).unwrap();
        let old = Connection::open(&path).unwrap();
        old.execute("UPDATE meta SET value = '2' WHERE key = 'format_version'", []).unwrap();
        drop(old);
        let o = Document::open(&path).unwrap();
        assert_eq!(o.view(plans[0]).unwrap().settings.overrides.model.get("Floors").map(|c| c.transparency), Some(40), "a changed override stays");
        assert!(plans[1..].iter().all(|p| !o.view(*p).unwrap().settings.overrides.model.contains_key("Floors")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_files_that_are_not_projects() {
        let dir = std::env::temp_dir().join(format!("openarch-test-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("junk.oab");
        std::fs::write(&path, b"this is not sqlite").unwrap();
        assert!(Document::open(&path).is_err());
        assert!(Document::open(&dir.join("missing.oab")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
