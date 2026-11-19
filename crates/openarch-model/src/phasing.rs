//! Phasing (private/docs/phasing.md): the project's timeline of phases, when each physical element is built and
//! demolished, and how a view shows an element by where it stands in time relative to the view's phase.
//!
//! One `Phasing` element holds it all, as project parameters keep their values: the phases in time order, the phase
//! filters, the graphic override of each status, and each element's Phase Created and Phase Demolished by element
//! id. An element with no Phase Created is in the default phase (New Construction in a new project), so a project
//! made before phasing draws as it did. Phase and filter ids are numbered inside the Phasing element.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::view::{CategoryOverride, LineOverride, TemplateProp, View};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Where an element stands in time, seen from a view's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    /// Built in the view's phase and still standing at its end.
    New,
    /// Built before the view's phase and still standing.
    Existing,
    /// Built before the view's phase and taken down in it.
    Demolished,
    /// Built and taken down in the view's phase.
    Temporary,
}

impl PhaseStatus {
    pub const ALL: [PhaseStatus; 4] = [PhaseStatus::New, PhaseStatus::Existing, PhaseStatus::Demolished, PhaseStatus::Temporary];
    pub fn label(self) -> &'static str {
        match self {
            PhaseStatus::New => "New",
            PhaseStatus::Existing => "Existing",
            PhaseStatus::Demolished => "Demolished",
            PhaseStatus::Temporary => "Temporary",
        }
    }
}

/// How a phase filter shows elements of one status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presentation {
    /// As the category draws in the view.
    #[default]
    ByCategory,
    /// With the status's graphic override.
    Overridden,
    /// Not shown at all.
    NotDisplayed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseFilter {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub new: Presentation,
    #[serde(default)]
    pub existing: Presentation,
    #[serde(default)]
    pub demolished: Presentation,
    #[serde(default)]
    pub temporary: Presentation,
}

impl PhaseFilter {
    pub fn get(&self, s: PhaseStatus) -> Presentation {
        match s {
            PhaseStatus::New => self.new,
            PhaseStatus::Existing => self.existing,
            PhaseStatus::Demolished => self.demolished,
            PhaseStatus::Temporary => self.temporary,
        }
    }
}

/// The graphic override each status takes where a filter says Overridden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PhaseOverrides {
    #[serde(default)]
    pub new: CategoryOverride,
    #[serde(default)]
    pub existing: CategoryOverride,
    #[serde(default)]
    pub demolished: CategoryOverride,
    #[serde(default)]
    pub temporary: CategoryOverride,
}

impl PhaseOverrides {
    pub fn get(&self, s: PhaseStatus) -> &CategoryOverride {
        match s {
            PhaseStatus::New => &self.new,
            PhaseStatus::Existing => &self.existing,
            PhaseStatus::Demolished => &self.demolished,
            PhaseStatus::Temporary => &self.temporary,
        }
    }
    pub fn get_mut(&mut self, s: PhaseStatus) -> &mut CategoryOverride {
        match s {
            PhaseStatus::New => &mut self.new,
            PhaseStatus::Existing => &mut self.existing,
            PhaseStatus::Demolished => &mut self.demolished,
            PhaseStatus::Temporary => &mut self.temporary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phasing {
    /// In time order, earliest first.
    pub phases: Vec<Phase>,
    pub filters: Vec<PhaseFilter>,
    #[serde(default)]
    pub overrides: PhaseOverrides,
    /// The phase of an element with no Phase Created, and of a view with no phase.
    pub default_phase: Id,
    /// Phase Created of each element that is not in the default phase.
    #[serde(default)]
    pub created: BTreeMap<Id, Id>,
    /// Phase Demolished of each element taken down.
    #[serde(default)]
    pub demolished: BTreeMap<Id, Id>,
    /// The next phase or filter id.
    pub next_id: Id,
}

/// Where an element stands in a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseState {
    /// Not a physical element (annotation, a datum, a view), or the project has no phasing: shown as ever.
    Unphased,
    Status(PhaseStatus),
    /// Not built yet, or taken down in an earlier phase: never shown.
    OutOfScope,
}

impl Default for Phasing {
    fn default() -> Self {
        use Presentation::*;
        let f = |id: Id, name: &str, new, existing, demolished, temporary| PhaseFilter { id, name: name.into(), new, existing, demolished, temporary };
        let mut overrides = PhaseOverrides::default();
        overrides.existing.halftone = true;
        let dashed = LineOverride { pattern: Some("Dash".into()), color: None, weight: None };
        overrides.demolished.projection_lines = dashed.clone();
        overrides.demolished.cut_lines = dashed.clone();
        overrides.temporary.projection_lines = dashed.clone();
        overrides.temporary.cut_lines = dashed;
        overrides.temporary.halftone = true;
        Phasing {
            phases: vec![
                Phase { id: 1, name: "Existing".into(), description: "What stands on the site now".into() },
                Phase { id: 2, name: "New Construction".into(), description: "The work this project builds".into() },
            ],
            filters: vec![
                f(3, "Show All", ByCategory, ByCategory, ByCategory, ByCategory),
                f(4, "Show Complete", ByCategory, ByCategory, NotDisplayed, NotDisplayed),
                f(5, "Show New", ByCategory, NotDisplayed, NotDisplayed, NotDisplayed),
                f(6, "Show Demo + New", ByCategory, NotDisplayed, Overridden, Overridden),
                f(7, "Show Previous + Demo", NotDisplayed, ByCategory, Overridden, NotDisplayed),
                f(8, "Show Previous + New", ByCategory, Overridden, NotDisplayed, NotDisplayed),
            ],
            overrides,
            default_phase: 2,
            created: BTreeMap::new(),
            demolished: BTreeMap::new(),
            next_id: 9,
        }
    }
}

impl Phasing {
    pub fn index(&self, phase: Id) -> Option<usize> {
        self.phases.iter().position(|p| p.id == phase)
    }
    pub fn phase(&self, id: Id) -> Option<&Phase> {
        self.phases.iter().find(|p| p.id == id)
    }
    pub fn filter(&self, id: Id) -> Option<&PhaseFilter> {
        self.filters.iter().find(|f| f.id == id)
    }
    /// The filter a view without one uses: Show All.
    pub fn default_filter(&self) -> Option<&PhaseFilter> {
        self.filters.iter().find(|f| f.name == "Show All").or(self.filters.first())
    }
    pub fn created_of(&self, id: Id) -> Id {
        self.created.get(&id).copied().unwrap_or(self.default_phase)
    }
    /// The status of an element built in `created` and taken down in `demolished`, seen from `view`.
    pub fn status(&self, created: Id, demolished: Option<Id>, view: Id) -> PhaseState {
        let (Some(c), Some(v)) = (self.index(created), self.index(view)) else { return PhaseState::Unphased };
        let d = demolished.and_then(|d| self.index(d));
        if c > v || d.is_some_and(|d| d < v) {
            return PhaseState::OutOfScope;
        }
        PhaseState::Status(match (c == v, d == Some(v)) {
            (true, true) => PhaseStatus::Temporary,
            (true, false) => PhaseStatus::New,
            (false, true) => PhaseStatus::Demolished,
            (false, false) => PhaseStatus::Existing,
        })
    }
}

/// Whether an element is built and taken down: the physical model elements.
pub fn is_phased(e: &Element) -> bool {
    matches!(
        e,
        Element::Wall(_)
            | Element::Floor(_)
            | Element::Roof(_)
            | Element::Ceiling(_)
            | Element::FamilyInstance(_)
            | Element::Stair(_)
            | Element::Ramp(_)
            | Element::Railing(_)
            | Element::StructuralColumn(_)
            | Element::Beam(_)
            | Element::BeamSystem(_)
            | Element::Truss(_)
            | Element::Toposolid(_)
            | Element::Kerb(_)
            | Element::KerbRamp(_)
            | Element::ParkingRow(_)
            | Element::LineMarking(_)
    )
}

impl Document {
    /// The project's phasing, if it has any yet (a project gets it the first time phases are looked at or set).
    pub fn phasing(&self) -> Option<(Id, &Phasing)> {
        let id = self.phasing_id()?;
        match self.get(id) {
            Some(Element::Phasing(p)) => Some((id, p)),
            _ => None,
        }
    }

    fn phasing_id(&self) -> Option<Id> {
        let rev = self.revision();
        let (at, id) = self.phasing_cache.get();
        if at == Some(rev) {
            return id;
        }
        let id = self.iter().find_map(|(id, e)| matches!(e, Element::Phasing(_)).then_some(id));
        self.phasing_cache.set((Some(rev), id));
        id
    }

    /// The phasing, made with the two starting phases and the standard filters when the project has none. One undo
    /// step when it is made.
    pub fn ensure_phasing(&mut self) -> Result<Id, String> {
        if let Some((id, _)) = self.phasing() {
            return Ok(id);
        }
        self.transact("Phases", |tx| Ok(tx.add(Element::Phasing(Phasing::default()))))
    }

    /// Change the phasing as one undo step.
    pub fn edit_phasing(&mut self, name: &str, f: impl FnOnce(&mut Phasing) -> Result<(), String>) -> Result<(), String> {
        let id = self.ensure_phasing()?;
        let Some(Element::Phasing(old)) = self.get(id).cloned() else { return Err("no phasing".into()) };
        let mut new = old.clone();
        f(&mut new)?;
        check(&new)?;
        if new == old {
            return Ok(());
        }
        self.transact(name, |tx| tx.update(id, Element::Phasing(new)))
    }

    /// A view's phase and phase filter (the filter from its template where the template controls it).
    pub fn view_phase(&self, view: &View) -> Option<(Id, Option<&PhaseFilter>)> {
        let (_, ph) = self.phasing()?;
        let phase = view.phase.filter(|p| ph.index(*p).is_some()).unwrap_or(ph.default_phase);
        let filter_id = match view.template.and_then(|t| self.view_template(t)) {
            Some(t) if t.included.contains(&TemplateProp::PhaseFilter) => t.settings.phase_filter,
            _ => view.settings.phase_filter,
        };
        let filter = filter_id.and_then(|f| ph.filter(f)).or_else(|| ph.default_filter());
        Some((phase, filter))
    }

    /// Where an element stands in a view.
    pub fn phase_state(&self, view: &View, id: Id) -> PhaseState {
        let Some(e) = self.get(id) else { return PhaseState::Unphased };
        if !is_phased(e) {
            return PhaseState::Unphased;
        }
        let Some((_, ph)) = self.phasing() else { return PhaseState::Unphased };
        let Some((phase, _)) = self.view_phase(view) else { return PhaseState::Unphased };
        ph.status(ph.created_of(id), ph.demolished.get(&id).copied(), phase)
    }

    /// Whether the view's phase hides the element: out of scope, or its status Not Displayed by the view's filter.
    pub fn phase_hides(&self, view: &View, id: Id) -> bool {
        match self.phase_state(view, id) {
            PhaseState::Unphased => false,
            PhaseState::OutOfScope => true,
            PhaseState::Status(s) => self.view_phase(view).and_then(|(_, f)| f).is_some_and(|f| f.get(s) == Presentation::NotDisplayed),
        }
    }

    /// The phase graphic override the view's filter gives the element, if any.
    pub fn phase_override(&self, view: &View, id: Id) -> Option<CategoryOverride> {
        let PhaseState::Status(s) = self.phase_state(view, id) else { return None };
        let (_, filter) = self.view_phase(view)?;
        if filter?.get(s) != Presentation::Overridden {
            return None;
        }
        Some(self.phasing()?.1.overrides.get(s).clone())
    }

    /// Phase Created and Phase Demolished of an element (None: not phased, or no phasing yet).
    pub fn element_phases(&self, id: Id) -> Option<(Id, Option<Id>)> {
        if !self.get(id).is_some_and(is_phased) {
            return None;
        }
        let (_, ph) = self.phasing()?;
        Some((ph.created_of(id), ph.demolished.get(&id).copied()))
    }

    /// Set Phase Created and/or Phase Demolished of elements, one undo step. `demolished: Some(None)` clears it.
    /// Refused where an element would be taken down before it is built.
    pub fn set_element_phases(&mut self, ids: &[Id], created: Option<Id>, demolished: Option<Option<Id>>) -> Result<(), String> {
        for &id in ids {
            if !self.get(id).is_some_and(is_phased) {
                return Err("only model elements have phases".into());
            }
        }
        let name = if demolished.is_some_and(|d| d.is_some()) && created.is_none() { "Demolish" } else { "Phase" };
        self.edit_phasing(name, |ph| {
            for &id in ids {
                if let Some(c) = created {
                    ph.index(c).ok_or("no such phase")?;
                    if c == ph.default_phase { ph.created.remove(&id); } else { ph.created.insert(id, c); }
                }
                match demolished {
                    Some(Some(d)) => {
                        ph.index(d).ok_or("no such phase")?;
                        ph.demolished.insert(id, d);
                    }
                    Some(None) => {
                        ph.demolished.remove(&id);
                    }
                    None => {}
                }
                if let Some(d) = ph.demolished.get(&id) {
                    if ph.index(*d) < ph.index(ph.created_of(id)) {
                        let (c, d) = (ph.created_of(id), *d);
                        let n = |p: Id| ph.phase(p).map_or(String::new(), |p| p.name.clone());
                        return Err(format!("an element cannot be demolished ({}) before it is built ({})", n(d), n(c)));
                    }
                }
            }
            Ok(())
        })
    }

    /// The Demolish tool: take the elements down in the view's phase.
    pub fn demolish_in_view(&mut self, view: Id, ids: &[Id]) -> Result<(), String> {
        self.ensure_phasing()?;
        let v = self.view(view).ok_or("no such view")?.clone();
        let (phase, _) = self.view_phase(&v).ok_or("no phasing")?;
        let ph = self.phasing().ok_or("no phasing")?.1;
        for &id in ids {
            if ph.demolished.contains_key(&id) {
                return Err("that element is already demolished".into());
            }
            if ph.index(ph.created_of(id)) > ph.index(phase) {
                return Err("that element is not built yet in this view's phase".into());
            }
        }
        self.set_element_phases(ids, None, Some(Some(phase)))
    }

    /// Set a view's phase; one undo step.
    pub fn set_view_phase(&mut self, view: Id, phase: Option<Id>) -> Result<(), String> {
        self.ensure_phasing()?;
        if let Some(p) = phase {
            self.phasing().and_then(|(_, ph)| ph.index(p)).ok_or("no such phase")?;
        }
        let Some(Element::View(v)) = self.get(view).cloned() else { return Err("no such view".into()) };
        let mut nv = v.clone();
        nv.phase = phase;
        if nv == v {
            return Ok(());
        }
        self.transact("View phase", |tx| tx.update(view, Element::View(nv)))
    }

    /// Set a view's phase filter; refused while its template controls it. One undo step.
    pub fn set_view_phase_filter(&mut self, view: Id, filter: Option<Id>) -> Result<(), String> {
        self.ensure_phasing()?;
        if let Some(f) = filter {
            self.phasing().and_then(|(_, ph)| ph.filter(f)).ok_or("no such phase filter")?;
        }
        let Some(Element::View(v)) = self.get(view).cloned() else { return Err("no such view".into()) };
        if v.template.and_then(|t| self.view_template(t)).is_some_and(|t| t.included.contains(&TemplateProp::PhaseFilter)) {
            return Err("the view's template sets its Phase Filter".into());
        }
        let mut nv = v.clone();
        nv.settings.phase_filter = filter;
        if nv == v {
            return Ok(());
        }
        self.transact("View phase filter", |tx| tx.update(view, Element::View(nv)))
    }

    /// New elements drawn while `phase` is the active view's phase are built in it (None: the default phase).
    pub fn set_creation_phase(&mut self, phase: Option<Id>) {
        self.creation_phase = phase;
    }

    /// What a transaction that added elements records about their phase (called by `transact`): the added
    /// physical elements are built in the creation phase when it is not the default.
    pub(crate) fn phase_new_elements(&self, added: &[Id]) -> Option<(Id, Element, Element)> {
        let phase = self.creation_phase?;
        let (pid, ph) = self.phasing()?;
        if phase == ph.default_phase || ph.index(phase).is_none() {
            return None;
        }
        let ids: Vec<Id> = added.iter().copied().filter(|id| self.get(*id).is_some_and(is_phased)).collect();
        if ids.is_empty() {
            return None;
        }
        let mut new = ph.clone();
        for id in ids {
            new.created.insert(id, phase);
        }
        Some((pid, Element::Phasing(ph.clone()), Element::Phasing(new)))
    }

    /// Insert a phase after `after` (None: at the start), one undo step; returns its id.
    pub fn insert_phase(&mut self, after: Option<Id>, name: &str) -> Result<Id, String> {
        let mut made = 0;
        self.edit_phasing("Add phase", |ph| {
            let at = match after {
                Some(a) => ph.index(a).ok_or("no such phase")? + 1,
                None => 0,
            };
            made = ph.next_id;
            ph.next_id += 1;
            ph.phases.insert(at, Phase { id: made, name: name.trim().to_string(), description: String::new() });
            Ok(())
        })?;
        Ok(made)
    }

    /// Combine a phase with the one before or after it (`into`): what was built or taken down in it is in `into`
    /// now, views showing it show `into`, and it is gone. One undo step.
    pub fn combine_phase(&mut self, phase: Id, into: Id) -> Result<(), String> {
        let views: Vec<Id> = self.views().iter().filter(|(_, v)| v.phase == Some(phase)).map(|(id, _)| *id).collect();
        self.grouped("Combine phases", |d| {
            d.edit_phasing("Combine phases", |ph| {
                let (a, b) = (ph.index(phase).ok_or("no such phase")?, ph.index(into).ok_or("no such phase")?);
                if a.abs_diff(b) != 1 {
                    return Err("a phase combines with the one just before or after it".into());
                }
                for m in [&mut ph.created, &mut ph.demolished] {
                    for v in m.values_mut() {
                        if *v == phase {
                            *v = into;
                        }
                    }
                }
                if ph.default_phase == phase {
                    ph.default_phase = into;
                }
                ph.created.retain(|_, v| *v != ph.default_phase);
                ph.phases.remove(a);
                Ok(())
            })?;
            for v in views {
                d.set_view_phase(v, Some(into))?;
            }
            Ok(())
        })
    }
}

/// While alive, the document draws `view`: openings follow its phase. Put back on drop.
pub struct DrawingView<'a> {
    doc: &'a Document,
    was: Option<Id>,
}

impl Drop for DrawingView<'_> {
    fn drop(&mut self) {
        self.doc.drawing_view.set(self.was);
    }
}

impl Document {
    /// Draw `view` until the guard goes: a door or window its phase leaves out does not open its wall.
    pub fn drawing(&self, view: Id) -> DrawingView<'_> {
        let was = self.drawing_view.replace(Some(view));
        DrawingView { doc: self, was }
    }

    /// How far into the timeline an element is built (0 for the first phase, and without phasing).
    pub fn phase_rank(&self, id: Id) -> u8 {
        let Some((_, ph)) = self.phasing() else { return 0 };
        ph.index(ph.created_of(id)).unwrap_or(0).min(20) as u8
    }

    /// Where a straight wall shown in `view` is filled in at the cut height `cut`: the openings of doors and
    /// windows demolished in the view's phase that the view does not show (in later phases the infill is simply
    /// part of the wall). Each as a ring across the wall, so
    /// the plan can draw the infill as new work.
    pub fn phase_infills(&self, view: &View, wall: Id, cut: f64) -> Vec<[crate::geom::Point; 4]> {
        let Some((_, ph)) = self.phasing() else { return vec![] };
        let Some((vphase, _)) = self.view_phase(view) else { return vec![] };
        let Some(Element::Wall(w)) = self.get(wall) else { return vec![] };
        if w.curve.is_some() || self.phase_hides(view, wall) {
            return vec![];
        }
        let was = self.drawing_view.replace(None);
        let all = self.wall_openings(wall);
        self.drawing_view.set(was);
        let (wl, wr) = self.wall_extents(w);
        let len = w.start.dist(&w.end);
        if len < 1e-6 {
            return vec![];
        }
        let d = crate::geom::Point::new((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
        let n = crate::geom::Point::new(-d.y, d.x);
        let at = |u: f64, o: f64| crate::geom::Point::new(w.start.x + d.x * u + n.x * o, w.start.y + d.y * u + n.y * o);
        all.into_iter()
            .filter(|o| o.z0 < cut && cut < o.z1)
            .filter(|o| ph.demolished.get(&o.instance) == Some(&vphase) && self.phase_hides(view, o.instance))
            .map(|o| [at(o.u0, -wr), at(o.u1, -wr), at(o.u1, wl), at(o.u0, wl)])
            .collect()
    }
}

/// What a phasing must hold to.
fn check(ph: &Phasing) -> Result<(), String> {
    if ph.phases.is_empty() {
        return Err("a project needs at least one phase".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for p in &ph.phases {
        if p.name.trim().is_empty() {
            return Err("a phase needs a name".into());
        }
        if !names.insert(p.name.trim().to_lowercase()) {
            return Err(format!("there is already a phase called '{}'", p.name.trim()));
        }
    }
    if ph.index(ph.default_phase).is_none() {
        return Err("the default phase is missing".into());
    }
    let mut fnames = std::collections::BTreeSet::new();
    for f in &ph.filters {
        if f.name.trim().is_empty() || !fnames.insert(f.name.trim().to_lowercase()) {
            return Err(format!("phase filter names must be given and different ('{}')", f.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    fn setup() -> (Document, Id, Id, Id) {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[0].0;
        let w = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), 2700.0).unwrap();
        let view = d.views().iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan).unwrap().0;
        (d, lvl, w, view)
    }

    #[test]
    fn the_status_of_an_element_follows_the_views_phase() {
        let ph = Phasing::default();
        let s = |c, d, v| ph.status(c, d, v);
        assert_eq!(s(1, None, 1), PhaseState::Status(PhaseStatus::New));
        assert_eq!(s(1, None, 2), PhaseState::Status(PhaseStatus::Existing));
        assert_eq!(s(1, Some(2), 2), PhaseState::Status(PhaseStatus::Demolished));
        assert_eq!(s(2, Some(2), 2), PhaseState::Status(PhaseStatus::Temporary));
        assert_eq!(s(2, None, 1), PhaseState::OutOfScope);
        assert_eq!(s(1, Some(1), 2), PhaseState::OutOfScope);
    }

    #[test]
    fn a_project_without_phasing_draws_as_before_and_an_existing_wall_demolished_shows_by_filter() {
        let (mut d, _, w, view) = setup();
        let v = d.view(view).unwrap().clone();
        assert_eq!(d.phase_state(&v, w), PhaseState::Unphased);
        assert!(!d.phase_hides(&v, w));
        // the wall was there before: Existing, then taken down in New Construction
        d.set_element_phases(&[w], Some(1), None).unwrap();
        let v = d.view(view).unwrap().clone();
        assert_eq!(d.phase_state(&v, w), PhaseState::Status(PhaseStatus::Existing));
        d.demolish_in_view(view, &[w]).unwrap();
        let v = d.view(view).unwrap().clone();
        assert_eq!(d.phase_state(&v, w), PhaseState::Status(PhaseStatus::Demolished));
        // Show All draws it as its category; Show Complete leaves it out; Show Previous + Demo dashes it
        assert!(!d.phase_hides(&v, w) && d.phase_override(&v, w).is_none());
        let filter = |d: &Document, n: &str| d.phasing().unwrap().1.filters.iter().find(|f| f.name == n).unwrap().id;
        d.set_view_phase_filter(view, Some(filter(&d, "Show Complete"))).unwrap();
        assert!(d.phase_hides(d.view(view).unwrap(), w));
        d.set_view_phase_filter(view, Some(filter(&d, "Show Previous + Demo"))).unwrap();
        let o = d.phase_override(d.view(view).unwrap(), w).unwrap();
        assert_eq!(o.projection_lines.pattern.as_deref(), Some("Dash"));
        // the Existing phase view sees it standing
        d.set_view_phase(view, Some(1)).unwrap();
        assert_eq!(d.phase_state(d.view(view).unwrap(), w), PhaseState::Status(PhaseStatus::New));
        // it cannot be demolished before it is built
        assert!(d.set_element_phases(&[w], Some(2), Some(Some(1))).is_err());
    }

    #[test]
    fn a_wall_drawn_in_an_existing_view_is_existing_in_the_same_undo_step() {
        let (mut d, lvl, _, view) = setup();
        d.ensure_phasing().unwrap();
        d.set_view_phase(view, Some(1)).unwrap();
        d.set_creation_phase(Some(1));
        let wt = d.wall_types()[0].0;
        let w2 = d.add_wall(wt, lvl, Point::new(0.0, 3000.0), Point::new(5000.0, 3000.0), 2700.0).unwrap();
        assert_eq!(d.element_phases(w2), Some((1, None)));
        d.undo().unwrap();
        assert!(d.get(w2).is_none());
        assert!(!d.phasing().unwrap().1.created.contains_key(&w2), "the phase went with the wall");
    }

    #[test]
    fn combining_a_phase_moves_its_elements_and_views() {
        let (mut d, _, w, view) = setup();
        let stage = d.insert_phase(Some(2), "Stage 2").unwrap();
        d.set_element_phases(&[w], Some(stage), None).unwrap();
        d.set_view_phase(view, Some(stage)).unwrap();
        d.combine_phase(stage, 2).unwrap();
        assert_eq!(d.element_phases(w), Some((2, None)));
        assert_eq!(d.view(view).unwrap().phase, Some(2));
        assert!(d.phasing().unwrap().1.phase(stage).is_none());
        assert!(d.insert_phase(None, "existing").is_err(), "names are unique");
    }
}
