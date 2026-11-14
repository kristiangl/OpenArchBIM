//! Project file: one SQLite database. Elements are stored as JSON rows, which
//! keeps the format easy to evolve while the model is young.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::doc::Document;
use crate::element::{Element, Id};

/// 2: elevation and 3D views draw in the Hidden Line style (before this the setting had no effect).
/// 3: plans no longer put a pale wash over floors.
/// 11: door and window tags (a new element kind).
/// 12: ceilings and ceiling types.
/// 13: area schemes, area boundaries, areas and area tags.
/// 14: toposolids and toposolid types.
/// 15: contour labels, and split lines on toposolids.
/// 16: line markings, parking rows, kerbs, kerb ramps and CAD imports (new element kinds), shape points on roofs,
///     and the number of backups to keep.
/// 17: view filters (a new element kind), and the Enable switch of a filter in a view.
/// 18: structural types, structural columns and beams (new element kinds).
/// 19: beam systems and trusses (new element kinds); joins, extensions and cutback on beams, slanted and
///     attached columns.
///
/// Raise this whenever a build starts to store something an earlier build would not know: a new element kind
/// always (the test `the_element_kinds_match_the_format_version` holds the list), and a new field whose loss would
/// matter. An earlier build then says the file is from a newer version instead of failing on it or, worse, opening
/// it and saving it back without the new data.
const FORMAT_VERSION: i64 = 19;

/// The backups a new project keeps, and the most that can be asked for.
pub const DEFAULT_BACKUPS: u32 = 5;
pub const MOST_BACKUPS: u32 = 99;

/// The numbered backups of `path` that are beside it, oldest first: `House.0001.oab`, `House.0002.oab` for
/// `House.oab`.
pub fn backups_of(path: &Path) -> Vec<(u32, PathBuf)> {
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else { return vec![] };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut found: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let rest = name.strip_prefix(stem)?.strip_prefix('.')?;
            let digits = if ext.is_empty() { rest } else { rest.strip_suffix(ext)?.strip_suffix('.')? };
            (digits.len() >= 4 && digits.chars().all(|c| c.is_ascii_digit())).then(|| digits.parse().ok()).flatten().map(|n| (n, e.path()))
        })
        .collect();
    found.sort();
    found
}

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
            tx.execute("INSERT INTO meta VALUES ('max_backups', ?1)", params![self.max_backups.to_string()])
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

    /// Save over the project's file, first keeping the file as it was as a numbered backup beside it
    /// (`House.0001.oab`, then `.0002`, and so on), and dropping the oldest backups past the number the project
    /// keeps. Gives the backup made, if any. The file as it was is copied, never moved, so it is there the whole
    /// time; a backup that cannot be made (a full disk, a folder that cannot be written) does not stop the save,
    /// and is reported instead.
    pub fn save_keeping_backups(&self, path: &Path) -> Result<Backup, String> {
        let keep = self.max_backups as usize;
        let mut made = Backup::None;
        if keep > 0 && path.is_file() {
            let next = backups_of(path).last().map_or(1, |(n, _)| n + 1);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("project");
            let name = match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => format!("{stem}.{next:04}.{ext}"),
                None => format!("{stem}.{next:04}"),
            };
            let to = path.with_file_name(name);
            made = match std::fs::copy(path, &to) {
                Ok(_) => Backup::Made(to),
                Err(e) => {
                    let _ = std::fs::remove_file(&to);
                    Backup::Failed(format!("{}: {e}", to.display()))
                }
            };
        }
        self.save(path)?;
        let all = backups_of(path);
        for (_, old) in all.iter().take(all.len().saturating_sub(keep)) {
            let _ = std::fs::remove_file(old);
        }
        Ok(made)
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
        // projects saved before backups were kept have no number: they take the default
        d.max_backups = meta("max_backups").ok().and_then(|v| v.parse().ok()).map_or(DEFAULT_BACKUPS, |n: u32| n.min(MOST_BACKUPS));
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
        d.ensure_view_marker_types();
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

/// What became of the backup when a project was saved over its file.
#[derive(Debug, Clone, PartialEq)]
pub enum Backup {
    /// None was wanted, or there was no earlier file.
    None,
    Made(PathBuf),
    /// The reason it could not be made. The project itself was saved.
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openarch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every element kind the project file can hold, as of FORMAT_VERSION. A kind added to `Element` without this
    /// list (and so the version) being touched fails here: an earlier build could not read such a file and has to
    /// be told it is from a newer version.
    const KINDS: (i64, &str) = (19, "Level WallType FloorType Wall Floor RoofType Roof View ViewTemplate Family FamilyType FamilyInstance ObjectStyles LineWeights LinePattern FillPattern Material ProjectLocation ElevationMarker Grid GridType AnnotationFamily LevelType Dimension DimensionType Spot SpotType Section ViewType ViewMarkerType DetailLine DetailRegion FilledRegionType TextNote TextNoteType GlobalParam GroupType GroupInstance GroupArray ProjectInfo Sheet Viewport SheetCollection SheetSchedule ProjectParameter Room RoomSeparation RoomTag RoomSettings Revision RevisionSettings RevisionCloud RevisionTag ElementTag CeilingType Ceiling AreaScheme AreaBoundary Area AreaTag ColorFillScheme ColorFillLegend StairType Stair RailingType Railing RampType Ramp ToposolidType Toposolid ContourLabel PropertyLine LineMarking ParkingRow Kerb KerbRamp CadImport ViewFilter StructuralType StructuralColumn Beam BeamSystem Truss");

    #[test]
    fn the_element_kinds_match_the_format_version() {
        let source = include_str!("element.rs");
        let body = source.split("pub enum Element {").nth(1).expect("the Element enum").split("\n}").next().unwrap();
        let kinds: Vec<&str> = body.lines().filter_map(|l| {
            let l = l.trim();
            let name = l.split('(').next()?;
            (l.contains('(') && !l.starts_with("//") && !l.starts_with('#') && name.chars().all(|c| c.is_ascii_alphanumeric()) && name.starts_with(|c: char| c.is_ascii_uppercase())).then_some(name)
        }).collect();
        assert!(kinds.len() > 60, "the Element enum was not read: {kinds:?}");
        assert_eq!(kinds.join(" "), KINDS.1, "an element kind was added, removed or renamed: raise FORMAT_VERSION in store.rs, say what changed in the list above it, and bring KINDS here up to date");
        assert_eq!(KINDS.0, FORMAT_VERSION, "FORMAT_VERSION changed: note the version beside KINDS");
    }

    #[test]
    fn a_file_from_a_newer_version_says_so() {
        let dir = folder("newer");
        let path = dir.join("Newer.oab");
        Document::new_project().save(&path).unwrap();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("UPDATE meta SET value = ?1 WHERE key = 'format_version'", params![(FORMAT_VERSION + 1).to_string()]).unwrap();
            // something this build has never heard of, as a newer build would write
            conn.execute("INSERT INTO elements VALUES (999999, 'HoverCar', '{\"HoverCar\":{\"lanes\":2}}')", []).unwrap();
        }
        let err = Document::open(&path).err().expect("refused");
        assert!(err.contains("newer version"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn saving_keeps_numbered_backups_and_drops_the_oldest() {
        let dir = folder("backups");
        let path = dir.join("House.oab");
        let mut d = Document::new_project();
        assert_eq!(d.max_backups(), DEFAULT_BACKUPS);
        d.set_max_backups(3).unwrap();
        // the first save has nothing to back up
        assert_eq!(d.save_keeping_backups(&path).unwrap(), Backup::None);
        assert!(backups_of(&path).is_empty());
        let (lvl, wt) = (d.levels()[0].0, d.wall_types()[1].0);
        let walls = |p: &Path| Document::open(p).unwrap().iter().filter(|(_, e)| matches!(e, Element::Wall(_))).count();
        for n in 1..=5u32 {
            d.add_wall(wt, lvl, Point::new(0.0, n as f64 * 1000.0), Point::new(4000.0, n as f64 * 1000.0), 2700.0).unwrap();
            let made = d.save_keeping_backups(&path).unwrap();
            assert_eq!(made, Backup::Made(dir.join(format!("House.{n:04}.oab"))));
            // the backup is the file as it was before this save
            assert_eq!(walls(&dir.join(format!("House.{n:04}.oab"))), n as usize - 1);
            assert_eq!(walls(&path), n as usize);
        }
        // three are kept, the newest ones, and the numbers go on from the last
        assert_eq!(backups_of(&path).iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![3, 4, 5]);
        // a file with a like name is not one of its backups
        std::fs::write(dir.join("House.notes.oab"), "x").unwrap();
        std::fs::write(dir.join("House2.0001.oab"), "x").unwrap();
        assert_eq!(backups_of(&path).len(), 3);
        // the number kept is in the file
        assert_eq!(Document::open(&path).unwrap().max_backups(), 3);
        // keeping none makes none, and clears those there
        d.set_max_backups(0).unwrap();
        assert_eq!(d.save_keeping_backups(&path).unwrap(), Backup::None);
        assert!(backups_of(&path).is_empty());
        assert!(d.set_max_backups(MOST_BACKUPS + 1).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

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
