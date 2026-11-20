//! The sun of 3D views (private/docs/sun.md): where it is now, its study frames, the sun path diagram, and the Sun
//! Settings dialog's changes.
use serde_json::{json, Value};

use openarch_model::sun::{days_from_civil, date_string, parse_date, sun_position, SunMode, SunSettings};
use openarch_model::view::TemplateProp;
use openarch_model::{Document, Id};

use crate::{id_of, Res, Session};

/// A view's sun for the 3D view and the Sun Settings dialog.
pub(crate) fn sun_json(d: &Document, view: Id) -> Result<Value, String> {
    let v = d.view(view).ok_or("no such view")?;
    let settings = d.effective_settings(view).ok_or("no such view")?;
    let g = &settings.graphic_display;
    let s = &g.sun;
    let st = d.sun_state(s);
    let cur = st.frames.get(st.frame);
    // the ground: the level chosen, else the lowest
    let levels = d.levels();
    let ground = s.ground_level.and_then(|l| d.level(l)).or_else(|| levels.iter().map(|(_, l)| *l).min_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm)));
    // where the sun can be dragged (spec 5.3): along the day (its time, every 5 minutes) and across the year (its date,
    // every 2 days, at the frame's time); only in a moment or a one-day study, with the path shown
    let (place, _) = d.sun_place();
    let draggable = settings.sun_path && !s.manual && matches!(s.mode, SunMode::Still | SunMode::SingleDay);
    let (track, year) = match cur.filter(|_| draggable) {
        Some(f) => {
            let days = parse_date(&f.date).map(|(y, m, dd)| days_from_civil(y, m, dd)).unwrap_or(0);
            let track: Vec<Value> = (0..288).filter_map(|k| {
                let p = sun_position(place, days, k as f64 * 5.0);
                (p.altitude_deg > 0.0).then(|| json!({ "min": k * 5, "dir": d.sun_vector(p.altitude_deg, p.azimuth_deg) }))
            }).collect();
            let (y, _, _) = parse_date(&f.date).unwrap_or((2026, 1, 1));
            let jan1 = days_from_civil(y, 1, 1);
            let year: Vec<Value> = (0..183).filter_map(|k| {
                let day = jan1 + k * 2;
                let p = sun_position(place, day, f.time_min);
                (p.altitude_deg > 0.0).then(|| json!({ "date": date_string(day), "dir": d.sun_vector(p.altitude_deg, p.azimuth_deg) }))
            }).collect();
            (json!(track), json!(year))
        }
        None => (json!([]), json!([])),
    };
    let locked = v.template.and_then(|t| d.view_template(t)).is_some_and(|t| t.included.contains(&TemplateProp::GraphicDisplay));
    Ok(json!({
        "view_id": view,
        "settings": s,
        "lighting": { "azimuth": g.sun_azimuth_deg, "altitude": g.sun_altitude_deg },
        "frames": st.frames,
        "frame": st.frame,
        "current": cur.map(|f| json!({
            "label": f.label, "altitude": f.altitude_deg, "azimuth": f.azimuth_deg,
            "dir": d.sun_vector(f.altitude_deg, f.azimuth_deg), "above": f.altitude_deg > 0.0,
        })),
        "place": { "latitude": st.latitude, "longitude": st.longitude, "utc_hours": st.utc_hours, "assumed": st.assumed_place, "true_north": st.true_north_deg, "dst": place.dst },
        "track": track,
        "year": year,
        "ambient_shadows": g.ambient_shadows,
        "sun_path": settings.sun_path,
        "cast_shadows": g.cast_shadows,
        "sun_intensity": g.sun_intensity,
        "ambient_intensity": g.ambient_intensity,
        "shadow_intensity": s.shadow_intensity,
        "ground_z_mm": ground.map_or(0.0, |l| l.elevation_mm),
        "ground_level": ground.map(|l| l.name.clone()),
        "levels": levels.iter().map(|(id, l)| json!({ "id": id, "name": l.name })).collect::<Vec<_>>(),
        "path_size_pct": s.path_size_pct,
        "path": if settings.sun_path && s.mode != SunMode::Lighting { json!(d.sun_path(s, &st)) } else { json!(null) },
        "locked": locked,
    }))
}

/// What the dialog may set, checked.
fn check(s: &SunSettings) -> Result<(), String> {
    for (what, date) in [("date", &s.date), ("start date", &s.start_date), ("end date", &s.end_date)] {
        if parse_date(date).is_none() {
            return Err(format!("The {what} must be a date like 2026-06-21."));
        }
    }
    if parse_date(&s.start_date) > parse_date(&s.end_date) {
        return Err("The end date must be after the start date.".into());
    }
    if s.time_min >= 1440 || s.start_min >= 1440 || s.end_min >= 1440 {
        return Err("Times are between 00:00 and 23:59.".into());
    }
    if s.start_min > s.end_min {
        return Err("The end time must be after the start time.".into());
    }
    if ![15, 30, 60].contains(&s.interval_min) {
        return Err("The time interval is 15, 30 or 60 minutes.".into());
    }
    if ![1, 7, 30].contains(&s.step_days) {
        return Err("The date interval is a day, a week or a month.".into());
    }
    if !(-90.0..=90.0).contains(&s.altitude_deg) || !s.azimuth_deg.is_finite() {
        return Err("The altitude is between -90 and 90 degrees.".into());
    }
    if s.shadow_intensity > 100 || !(100..=300).contains(&s.path_size_pct) {
        return Err("Shadow intensity is 0 to 100, the sun path size 100 to 300 %.".into());
    }
    Ok(())
}

impl Session {
    pub(crate) fn handle_sun(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            "sun" => sun_json(&self.doc, id_of(p, "view_id").ok().or_else(|| self.doc.views().iter().find(|(_, v)| v.view_kind == openarch_model::view::ViewKind::ThreeD).map(|(id, _)| *id)).unwrap_or(0)),
            // the Sun Settings dialog and the 3D view's sun bar: { view_id, patch: { sun fields, sun_path?, cast_shadows?,
            // sun_intensity?, ambient_intensity? } }, one undo step
            "set_sun" => (|| {
                let view = id_of(p, "view_id")?;
                let v = self.doc.view(view).ok_or("no such view")?.clone();
                if v.template.and_then(|t| self.doc.view_template(t)).is_some_and(|t| t.included.contains(&TemplateProp::GraphicDisplay)) {
                    return Err("The view's template sets its graphic display (and sun). Change it in the template, or take Graphic Display out of it.".into());
                }
                let patch = p.get("patch").ok_or("missing patch")?.as_object().ok_or("the patch must be an object")?;
                let mut n = v.clone();
                let g = &mut n.settings.graphic_display;
                let mut sun = serde_json::to_value(&g.sun).map_err(|e| e.to_string())?;
                for (k, val) in patch {
                    match k.as_str() {
                        "sun_path" => n.settings.sun_path = val.as_bool().ok_or("sun_path is true or false")?,
                        "cast_shadows" => n.settings.graphic_display.cast_shadows = val.as_bool().ok_or("cast_shadows is true or false")?,
                        "ambient_shadows" => n.settings.graphic_display.ambient_shadows = val.as_bool().ok_or("ambient_shadows is true or false")?,
                        "sun_intensity" | "ambient_intensity" => {
                            let x = val.as_u64().filter(|x| *x <= 100).ok_or("intensities are 0 to 100")? as u8;
                            if k == "sun_intensity" { n.settings.graphic_display.sun_intensity = x } else { n.settings.graphic_display.ambient_intensity = x }
                        }
                        "lighting_azimuth" => n.settings.graphic_display.sun_azimuth_deg = val.as_f64().ok_or("a number")?,
                        "lighting_altitude" => n.settings.graphic_display.sun_altitude_deg = val.as_f64().filter(|a| (0.0..=90.0).contains(a)).ok_or("the altitude is 0 to 90 degrees")?,
                        _ => sun[k] = val.clone(),
                    }
                }
                let s: SunSettings = serde_json::from_value(sun).map_err(|e| format!("bad sun settings: {e}"))?;
                check(&s)?;
                if s.ground_level.is_some_and(|l| self.doc.level(l).is_none()) {
                    return Err("no such level".into());
                }
                n.settings.graphic_display.sun = s;
                if n != v {
                    self.doc.patch_view(view, &json!({ "settings": serde_json::to_value(&n.settings).map_err(|e| e.to_string())? }))?;
                }
                sun_json(&self.doc, view)
            })(),
            _ => return None,
        })
    }
}
