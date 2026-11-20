//! The sun (private/docs/sun.md; spec Specifications/bim_sun_paths_shadow_rendering_architecture_specification.md):
//! where it is from the project's latitude, longitude and time zone and a date and time (NOAA's solar position
//! equations), each 3D view's sun settings (kept in its Graphic Display, so a view template carries them), solar
//! studies as frames, and the sun path diagram as lines on a sphere of radius 1 that the 3D view scales round the
//! model.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::Id;

/// How a view's sun is set (spec 4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SunMode {
    /// The light comes from over the viewer's shoulder (Graphic Display's azimuth and altitude, relative to the
    /// camera), whatever the location and time.
    #[default]
    Lighting,
    /// One moment: a date and time at the project's location, or a fixed azimuth and altitude.
    Still,
    /// One date, from a start to an end time (or sunrise to sunset) in steps.
    SingleDay,
    /// One time of day across a run of dates in steps.
    MultiDay,
}

/// A view's sun settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SunSettings {
    pub mode: SunMode,
    /// The date of Still and Single-Day, "YYYY-MM-DD".
    pub date: String,
    /// The time of Still and Multi-Day, minutes after local midnight.
    pub time_min: u32,
    /// Still: the azimuth (from true north, clockwise) and altitude below, not a date and time.
    pub manual: bool,
    pub azimuth_deg: f64,
    pub altitude_deg: f64,
    /// Single-Day: from and to (minutes), or sunrise to sunset.
    pub start_min: u32,
    pub end_min: u32,
    pub sunrise_to_sunset: bool,
    /// Single-Day: minutes between frames (15, 30 or 60).
    pub interval_min: u32,
    /// Multi-Day: from and to dates, and days between frames (1, 7 or 30).
    pub start_date: String,
    pub end_date: String,
    pub step_days: u32,
    /// The study frame shown.
    pub frame: u32,
    /// The level the ground plane (and the sun path's compass) sits on; none: the lowest level.
    pub ground_level: Option<Id>,
    /// How dark cast shadows are (0 none, 100 black).
    pub shadow_intensity: u8,
    /// The sun path's size against the model (100 to 300 %).
    pub path_size_pct: u16,
}

impl Default for SunSettings {
    fn default() -> Self {
        SunSettings {
            mode: SunMode::Lighting,
            date: "2026-06-21".into(),
            time_min: 12 * 60,
            manual: false,
            azimuth_deg: 0.0,
            altitude_deg: 35.0,
            start_min: 9 * 60,
            end_min: 15 * 60,
            sunrise_to_sunset: false,
            interval_min: 60,
            start_date: "2026-01-01".into(),
            end_date: "2026-12-31".into(),
            step_days: 30,
            frame: 0,
            ground_level: None,
            shadow_intensity: 50,
            path_size_pct: 100,
        }
    }
}

/// Where the sun is: degrees above the horizon and clockwise from true north.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPos {
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
}

/// When clocks go forward an hour. Taken a day at a time: the hour of the change itself is not modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DstRule {
    /// No daylight saving (Western Australia, Queensland, the Northern Territory).
    #[default]
    None,
    /// The first Sunday in October to the first Sunday in April (NSW, Victoria, the ACT, SA, Tasmania).
    Australia,
    /// The last Sunday in September to the first Sunday in April.
    NewZealand,
    /// The last Sunday in March to the last Sunday in October.
    Europe,
    /// The second Sunday in March to the first Sunday in November.
    NorthAmerica,
}

/// Sunday 0 .. Saturday 6 of a day since 1970 (a Thursday).
fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// The `n`th Sunday of a month (n = 1 first), or with n = 0 the last.
fn sunday(y: i32, m: u32, n: i64) -> i64 {
    if n == 0 {
        let last = days_from_civil(y, m, days_in_month(y, m));
        return last - weekday(last);
    }
    let first = days_from_civil(y, m, 1);
    first + (7 - weekday(first)) % 7 + 7 * (n - 1)
}

impl DstRule {
    /// Whether clocks are an hour forward on a day.
    pub fn in_effect(self, days: i64) -> bool {
        let (y, _, _) = civil_from_days(days);
        let within = |a: i64, b: i64| days >= a && days < b;
        match self {
            DstRule::None => false,
            // southern: from spring to the next autumn, across the new year
            DstRule::Australia => !within(sunday(y, 4, 1), sunday(y, 10, 1)),
            DstRule::NewZealand => !within(sunday(y, 4, 1), sunday(y, 9, 0)),
            DstRule::Europe => within(sunday(y, 3, 0), sunday(y, 10, 0)),
            DstRule::NorthAmerica => within(sunday(y, 3, 2), sunday(y, 11, 1)),
        }
    }
}

/// A place on the earth: degrees (south and west negative), hours from UTC, and its daylight saving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
    pub utc_hours: f64,
    pub dst: DstRule,
}

impl Place {
    /// The place as its clocks read on a day: an hour further from UTC while daylight saving is in effect.
    pub fn on(self, days: i64) -> Place {
        Place { utc_hours: self.utc_hours + if self.dst.in_effect(days) { 1.0 } else { 0.0 }, dst: DstRule::None, ..self }
    }
}

/// Perth, for a project whose location has not been set.
pub const PERTH: Place = Place { latitude: -31.9523, longitude: 115.8613, utc_hours: 8.0, dst: DstRule::None };

/// "YYYY-MM-DD" as (year, month, day), checked.
pub fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut it = s.trim().split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some((y, m, d))
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 },
    }
}

/// Days since 1970-01-01 (Howard Hinnant's civil calendar algorithm).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32, m, d)
}

pub fn date_string(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The sun's declination, the equation of time (minutes) and Julian century of a moment (NOAA).
fn solar_terms(days: i64, minutes_utc: f64) -> (f64, f64) {
    let jd = days as f64 + 2440587.5 + minutes_utc / 1440.0;
    let jc = (jd - 2451545.0) / 36525.0;
    let l0 = (280.46646 + jc * (36000.76983 + jc * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + jc * (35999.05029 - 0.0001537 * jc);
    let e = 0.016708634 - jc * (0.000042037 + 0.0000001267 * jc);
    let r = |x: f64| x.to_radians();
    let c = r(m).sin() * (1.914602 - jc * (0.004817 + 0.000014 * jc)) + r(2.0 * m).sin() * (0.019993 - 0.000101 * jc) + r(3.0 * m).sin() * 0.000289;
    let true_long = l0 + c;
    let omega = 125.04 - 1934.136 * jc;
    let app_long = true_long - 0.00569 - 0.00478 * r(omega).sin();
    let mean_obliq = 23.0 + (26.0 + (21.448 - jc * (46.815 + jc * (0.00059 - jc * 0.001813))) / 60.0) / 60.0;
    let obliq = mean_obliq + 0.00256 * r(omega).cos();
    let decl = (r(obliq).sin() * r(app_long).sin()).asin().to_degrees();
    let y = (r(obliq / 2.0)).tan().powi(2);
    let eq_time = 4.0
        * (y * (2.0 * r(l0)).sin() - 2.0 * e * r(m).sin() + 4.0 * e * y * r(m).sin() * (2.0 * r(l0)).cos()
            - 0.5 * y * y * (4.0 * r(l0)).sin()
            - 1.25 * e * e * (2.0 * r(m)).sin())
        .to_degrees();
    (decl, eq_time)
}

/// Where the sun is at a place on a date (days since 1970) at a local clock time (minutes after midnight).
pub fn sun_position(place: Place, days: i64, local_min: f64) -> SunPos {
    let place = place.on(days);
    let utc = local_min - place.utc_hours * 60.0;
    let (decl, eq_time) = solar_terms(days, utc);
    let tst = (local_min + eq_time + 4.0 * place.longitude - 60.0 * place.utc_hours).rem_euclid(1440.0);
    let ha = if tst / 4.0 < 0.0 { tst / 4.0 + 180.0 } else { tst / 4.0 - 180.0 };
    let (lat, dec) = (place.latitude.to_radians(), decl.to_radians());
    let cos_zen = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.to_radians().cos()).clamp(-1.0, 1.0);
    let zen = cos_zen.acos();
    let az = if zen.sin().abs() < 1e-9 || lat.cos().abs() < 1e-9 {
        180.0
    } else {
        let a = (((lat.sin() * cos_zen) - dec.sin()) / (lat.cos() * zen.sin())).clamp(-1.0, 1.0).acos().to_degrees();
        if ha > 0.0 { (a + 180.0).rem_euclid(360.0) } else { (540.0 - a).rem_euclid(360.0) }
    };
    SunPos { altitude_deg: 90.0 - zen.to_degrees(), azimuth_deg: az }
}

/// Sunrise and sunset (local minutes) on a date, or None where the sun does not rise or set that day.
pub fn sunrise_sunset(place: Place, days: i64) -> Option<(f64, f64)> {
    let place = place.on(days);
    let (decl, eq_time) = solar_terms(days, 720.0 - place.utc_hours * 60.0);
    let (lat, dec) = (place.latitude.to_radians(), decl.to_radians());
    let c = 90.833f64.to_radians().cos() / (lat.cos() * dec.cos()) - lat.tan() * dec.tan();
    if !(-1.0..=1.0).contains(&c) {
        return None;
    }
    let ha = c.acos().to_degrees();
    let noon = 720.0 - 4.0 * place.longitude - eq_time + place.utc_hours * 60.0;
    Some((noon - ha * 4.0, noon + ha * 4.0))
}

/// "09:30" from minutes.
pub fn clock(min: f64) -> String {
    let m = min.round().rem_euclid(1440.0) as u32;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// One frame of a view's sun: when (a label) and where.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SunFrame {
    pub label: String,
    pub date: String,
    pub time_min: f64,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
}

/// What a 3D view's sun is now: its frames (one for Still), the frame shown, and the place used.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SunState {
    pub mode: SunMode,
    pub frames: Vec<SunFrame>,
    pub frame: usize,
    pub latitude: f64,
    pub longitude: f64,
    pub utc_hours: f64,
    /// The project has no latitude and longitude yet: Perth is assumed.
    pub assumed_place: bool,
    pub true_north_deg: f64,
}

impl Document {
    /// The project's place on the earth, or Perth (and true) when its location has no latitude and longitude.
    pub fn sun_place(&self) -> (Place, bool) {
        let Some((_, l)) = self.project_location() else { return (PERTH, true) };
        match (l.latitude_deg, l.longitude_deg) {
            (Some(latitude), Some(longitude)) => {
                // without a time zone: the nearest whole hour to the longitude's solar time
                let utc_hours = l.time_zone_hours.unwrap_or((longitude / 15.0).round());
                (Place { latitude, longitude, utc_hours, dst: l.daylight_saving }, false)
            }
            _ => (PERTH, true),
        }
    }

    /// A view's sun now (spec 4.1): Still gives one frame, a study one per step.
    pub fn sun_state(&self, s: &SunSettings) -> SunState {
        let (place, assumed) = self.sun_place();
        let north = self.project_location().map_or(0.0, |(_, l)| l.angle_true_north_deg);
        let date = parse_date(&s.date).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or_else(|| days_from_civil(2026, 6, 21));
        let fmt_date = |days: i64| {
            let (y, m, d) = civil_from_days(days);
            const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            format!("{d} {} {y}", M[(m - 1) as usize])
        };
        let at = |days: i64, min: f64| {
            let p = sun_position(place, days, min);
            SunFrame { label: format!("{} {}", fmt_date(days), clock(min)), date: date_string(days), time_min: min, altitude_deg: p.altitude_deg, azimuth_deg: p.azimuth_deg }
        };
        let frames = match s.mode {
            SunMode::Lighting => vec![],
            SunMode::Still if s.manual => vec![SunFrame { label: format!("Azimuth {:.0}°, altitude {:.0}°", s.azimuth_deg, s.altitude_deg), date: date_string(date), time_min: s.time_min as f64, altitude_deg: s.altitude_deg, azimuth_deg: s.azimuth_deg }],
            SunMode::Still => vec![at(date, s.time_min as f64)],
            SunMode::SingleDay => {
                let (a, b) = if s.sunrise_to_sunset { sunrise_sunset(place, date).unwrap_or((s.start_min as f64, s.end_min as f64)) } else { (s.start_min as f64, s.end_min as f64) };
                let step = s.interval_min.clamp(1, 240) as f64;
                // from sunrise the frames fall on the interval's clock times, with sunrise and sunset themselves
                let mut t = a;
                let mut out = vec![];
                while t <= b + 1e-6 && out.len() < 200 {
                    out.push(at(date, t));
                    let next = if s.sunrise_to_sunset { ((t / step).floor() + 1.0) * step } else { t + step };
                    if s.sunrise_to_sunset && next > b && t < b - 1e-6 {
                        t = b;
                    } else {
                        t = next;
                    }
                }
                out
            }
            SunMode::MultiDay => {
                let d0 = parse_date(&s.start_date).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or(date);
                let d1 = parse_date(&s.end_date).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or(d0);
                let step = s.step_days.clamp(1, 366) as i64;
                let mut out = vec![];
                let mut d = d0;
                while d <= d1 && out.len() < 400 {
                    out.push(at(d, s.time_min as f64));
                    // a month is a calendar month, not 30 days
                    d = if step == 30 {
                        let (y, m, dd) = civil_from_days(d);
                        let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                        days_from_civil(y, m, dd.min(days_in_month(y, m)))
                    } else {
                        d + step
                    };
                }
                out
            }
        };
        let frame = (s.frame as usize).min(frames.len().saturating_sub(1));
        SunState { mode: s.mode, frames, frame, latitude: place.latitude, longitude: place.longitude, utc_hours: place.utc_hours, assumed_place: assumed, true_north_deg: north }
    }

    /// A compass bearing (degrees clockwise from true north) as a plan direction (spec 3.2: turned by true north).
    pub fn bearing_to_plan(&self, bearing: f64) -> (f64, f64) {
        let north = self.project_location().map_or(0.0, |(_, l)| l.angle_true_north_deg);
        let (e, n) = (bearing.to_radians().sin(), bearing.to_radians().cos());
        let (sn, cs) = north.to_radians().sin_cos();
        (e * cs - n * sn, e * sn + n * cs)
    }

    /// The unit vector toward the sun in project coordinates (x east-ish, y north-ish, z up).
    pub fn sun_vector(&self, altitude_deg: f64, azimuth_deg: f64) -> [f64; 3] {
        let (x, y) = self.bearing_to_plan(azimuth_deg);
        let c = altitude_deg.to_radians().cos();
        [x * c, y * c, altitude_deg.to_radians().sin()]
    }

    /// The sun path diagram (spec 5.1) on a sphere of radius 1 round the origin, in project directions: the compass
    /// ring with its ticks and labels, the day's arc, the two solstice arcs, an analemma for each hour, and the sun.
    pub fn sun_path(&self, s: &SunSettings, st: &SunState) -> SunPath {
        let (place, _) = self.sun_place();
        let pt = |alt: f64, az: f64| self.sun_vector(alt, az);
        let year = parse_date(&s.date).map_or(2026, |(y, _, _)| y);
        let mut out = SunPath::default();
        // the compass: a ring at the horizon, ticks every 15 degrees (longer at the eight points), their names
        out.ring = (0..=96).map(|k| pt(0.0, k as f64 * 360.0 / 96.0)).collect();
        for k in 0..24 {
            let b = k as f64 * 15.0;
            let (x, y) = self.bearing_to_plan(b);
            let len = if k % 6 == 0 { 0.1 } else if k % 3 == 0 { 0.07 } else { 0.04 };
            out.ticks.push([[x, y, 0.0], [x * (1.0 + len), y * (1.0 + len), 0.0]]);
        }
        for (k, name) in ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().enumerate() {
            let (x, y) = self.bearing_to_plan(k as f64 * 45.0);
            out.labels.push(SunLabel { text: (*name).into(), at: [x * 1.18, y * 1.18, 0.0], kind: "compass".into() });
        }
        // an arc across the sky on a date, above the horizon, every 10 minutes
        let arc = |days: i64| -> Vec<Vec<[f64; 3]>> {
            let mut lines = vec![];
            let mut cur = vec![];
            for k in 0..=144 {
                let p = sun_position(place, days, k as f64 * 10.0);
                if p.altitude_deg > 0.0 {
                    cur.push(pt(p.altitude_deg, p.azimuth_deg));
                } else if cur.len() > 1 {
                    lines.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
            if cur.len() > 1 {
                lines.push(cur);
            }
            lines
        };
        let date = st.frames.get(st.frame).and_then(|f| parse_date(&f.date)).or_else(|| parse_date(&s.date)).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or_else(|| days_from_civil(year, 6, 21));
        out.day = arc(date);
        out.summer = arc(days_from_civil(year, if place.latitude < 0.0 { 12 } else { 6 }, 21));
        out.winter = arc(days_from_civil(year, if place.latitude < 0.0 { 6 } else { 12 }, 21));
        // an analemma for each hour the sun is up some of the year: where it stands at that clock time, day by day
        let jan1 = days_from_civil(year, 1, 1);
        for h in 0..24 {
            let mut lines = vec![];
            let mut cur = vec![];
            for k in 0..=92 {
                let p = sun_position(place, jan1 + k * 4, h as f64 * 60.0);
                if p.altitude_deg > 0.0 {
                    cur.push(pt(p.altitude_deg, p.azimuth_deg));
                } else if cur.len() > 1 {
                    lines.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
            if cur.len() > 1 {
                lines.push(cur);
            }
            if lines.is_empty() {
                continue;
            }
            out.analemmas.extend(lines);
            // the hour's label where the day's arc crosses it
            let p = sun_position(place, date, h as f64 * 60.0);
            if p.altitude_deg > 2.0 {
                let v = pt(p.altitude_deg, p.azimuth_deg);
                out.labels.push(SunLabel { text: format!("{h:02}"), at: [v[0] * 1.06, v[1] * 1.06, v[2] * 1.06], kind: "hour".into() });
            }
        }
        if let Some(f) = st.frames.get(st.frame) {
            out.sun = Some(pt(f.altitude_deg, f.azimuth_deg));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SunLabel {
    pub text: String,
    pub at: [f64; 3],
    /// "compass" or "hour"
    pub kind: String,
}

/// The sun path diagram on a unit sphere (see `Document::sun_path`).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SunPath {
    pub ring: Vec<[f64; 3]>,
    pub ticks: Vec<[[f64; 3]; 2]>,
    pub labels: Vec<SunLabel>,
    pub day: Vec<Vec<[f64; 3]>>,
    pub summer: Vec<Vec<[f64; 3]>>,
    pub winter: Vec<Vec<[f64; 3]>>,
    pub analemmas: Vec<Vec<[f64; 3]>>,
    pub sun: Option<[f64; 3]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perth_sun_at_noon_in_midwinter_and_midsummer() {
        // 21 June 2026, 12:00 AWST: the sun is low in the north (about 34.6 degrees up)
        let p = sun_position(PERTH, days_from_civil(2026, 6, 21), 12.0 * 60.0);
        assert!((p.altitude_deg - 34.5).abs() < 1.0, "{p:?}");
        assert!(p.azimuth_deg < 15.0 || p.azimuth_deg > 345.0, "{p:?}");
        // 21 December: high, just north of overhead
        let q = sun_position(PERTH, days_from_civil(2026, 12, 21), 12.0 * 60.0);
        assert!(q.altitude_deg > 78.0 && q.altitude_deg < 82.0, "{q:?}");
        // mid-morning the sun is in the north-east, mid-afternoon the north-west
        let am = sun_position(PERTH, days_from_civil(2026, 6, 21), 9.0 * 60.0);
        let pm = sun_position(PERTH, days_from_civil(2026, 6, 21), 15.0 * 60.0);
        assert!(am.azimuth_deg > 30.0 && am.azimuth_deg < 70.0, "{am:?}");
        assert!(pm.azimuth_deg > 290.0 && pm.azimuth_deg < 330.0, "{pm:?}");
        // midnight: below the horizon
        assert!(sun_position(PERTH, days_from_civil(2026, 6, 21), 0.0).altitude_deg < 0.0);
    }

    #[test]
    fn perth_sunrise_and_sunset_in_june() {
        // about 07:16 and 17:20 AWST on 21 June
        let (r, s) = sunrise_sunset(PERTH, days_from_civil(2026, 6, 21)).unwrap();
        assert!((r - (7.0 * 60.0 + 16.0)).abs() < 5.0, "{}", clock(r));
        assert!((s - (17.0 * 60.0 + 20.0)).abs() < 5.0, "{}", clock(s));
    }

    #[test]
    fn daylight_saving_moves_the_clock_an_hour() {
        let d = |y, m, dd| days_from_civil(y, m, dd);
        // Sydney 2026: from 5 October to 5 April (the first Sundays)
        assert!(DstRule::Australia.in_effect(d(2026, 1, 15)) && DstRule::Australia.in_effect(d(2026, 10, 4)) && DstRule::Australia.in_effect(d(2026, 4, 4)));
        assert!(!DstRule::Australia.in_effect(d(2026, 4, 5)) && !DstRule::Australia.in_effect(d(2026, 7, 1)) && !DstRule::Australia.in_effect(d(2026, 10, 3)));
        assert!(DstRule::Europe.in_effect(d(2026, 3, 29)) && !DstRule::Europe.in_effect(d(2026, 3, 28)) && !DstRule::Europe.in_effect(d(2026, 10, 25)));
        assert!(DstRule::NorthAmerica.in_effect(d(2026, 3, 8)) && !DstRule::NorthAmerica.in_effect(d(2026, 11, 1)));
        // the same sun is an hour later on the clock: Sydney 13:00 in January is 12:00 standard time
        let syd = Place { latitude: -33.87, longitude: 151.21, utc_hours: 10.0, dst: DstRule::None };
        let summer = Place { dst: DstRule::Australia, ..syd };
        let a = sun_position(syd, d(2026, 1, 15), 12.0 * 60.0);
        let b = sun_position(summer, d(2026, 1, 15), 13.0 * 60.0);
        assert!((a.altitude_deg - b.altitude_deg).abs() < 1e-9 && (a.azimuth_deg - b.azimuth_deg).abs() < 1e-9);
        let (r0, _) = sunrise_sunset(syd, d(2026, 1, 15)).unwrap();
        let (r1, _) = sunrise_sunset(summer, d(2026, 1, 15)).unwrap();
        assert!((r1 - r0 - 60.0).abs() < 0.1, "{}", r1 - r0);
    }

    #[test]
    fn dates_round_trip_and_bad_ones_are_refused() {
        for (y, m, d) in [(1970, 1, 1), (2026, 2, 28), (2028, 2, 29), (2026, 12, 31)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert!(parse_date("2026-02-30").is_none() && parse_date("2026-13-01").is_none() && parse_date("x").is_none());
        assert_eq!(parse_date(" 2026-06-21 "), Some((2026, 6, 21)));
    }

    #[test]
    fn studies_give_frames_and_true_north_turns_the_sun() {
        let mut d = Document::new_project();
        let mut s = SunSettings { mode: SunMode::SingleDay, ..Default::default() };
        // 09:00 to 15:00 hourly: seven frames
        let st = d.sun_state(&s);
        assert_eq!(st.frames.len(), 7);
        assert!(st.assumed_place);
        s.sunrise_to_sunset = true;
        let st = d.sun_state(&s);
        assert!(st.frames.first().unwrap().time_min > 7.0 * 60.0 && st.frames.last().unwrap().time_min < 17.5 * 60.0, "{:?}", st.frames);
        assert_eq!(st.frames[1].time_min, 8.0 * 60.0, "after sunrise the frames are on the hour");
        s.mode = SunMode::MultiDay;
        assert_eq!(d.sun_state(&s).frames.len(), 12, "monthly through 2026");
        // north noon sun: straight up the plan, then turned with true north
        let v = d.sun_vector(30.0, 0.0);
        assert!(v[0].abs() < 1e-9 && v[1] > 0.8);
        d.patch_project_location(&serde_json::json!({ "angle_true_north_deg": 90.0 })).unwrap();
        let w = d.sun_vector(30.0, 0.0);
        assert!(w[0] < -0.8 && w[1].abs() < 1e-9, "{w:?}");
        // the path: a ring, labels, arcs, analemmas and the sun
        s.mode = SunMode::Still;
        let st = d.sun_state(&s);
        let p = d.sun_path(&s, &st);
        assert!(p.ring.len() > 90 && p.labels.iter().filter(|l| l.kind == "compass").count() == 8);
        assert!(!p.day.is_empty() && !p.summer.is_empty() && !p.winter.is_empty() && p.analemmas.len() >= 10 && p.sun.is_some());
    }
}
