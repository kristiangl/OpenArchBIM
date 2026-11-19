//! Design options (private/docs/design-options.md; spec Specifications/bim_design_options_architecture_specification.md):
//! option sets, each a decision with its design options of which one is primary; the option each element belongs to
//! (none: the main model); the option of each set a view shows; and the option being edited.
//!
//! One `DesignOptions` element holds the sets and each element's option by element id, as `Phasing` holds phases, so
//! every change is one undo step and a project made before design options has none and is all main model. Set and
//! option ids are numbered inside it. Views keep `design_options` (set id to option id; a set left out is
//! Automatic: the option being edited when it is in that set, else the primary).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::view::View;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignOption {
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSet {
    pub id: Id,
    pub name: String,
    /// In the order the dialog lists them.
    pub options: Vec<DesignOption>,
    /// The option every view shows unless set otherwise, and the one Accept Primary keeps.
    pub primary: Id,
}

impl OptionSet {
    pub fn has(&self, option: Id) -> bool {
        self.options.iter().any(|o| o.id == option)
    }
    pub fn option(&self, option: Id) -> Option<&DesignOption> {
        self.options.iter().find(|o| o.id == option)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignOptions {
    pub sets: Vec<OptionSet>,
    /// The option each element in one belongs to.
    #[serde(default)]
    pub member: BTreeMap<Id, Id>,
    /// The next set or option id.
    pub next_id: Id,
}

impl Default for DesignOptions {
    fn default() -> Self {
        DesignOptions { sets: vec![], member: BTreeMap::new(), next_id: 1 }
    }
}

impl DesignOptions {
    pub fn set(&self, id: Id) -> Option<&OptionSet> {
        self.sets.iter().find(|s| s.id == id)
    }
    /// The set an option is in.
    pub fn set_of(&self, option: Id) -> Option<&OptionSet> {
        self.sets.iter().find(|s| s.has(option))
    }
    /// "Set: Option", and "(primary)" after the primary.
    pub fn label(&self, option: Id) -> Option<String> {
        let s = self.set_of(option)?;
        let o = s.option(option)?;
        Some(format!("{}: {}{}", s.name, o.name, if s.primary == option { " (primary)" } else { "" }))
    }
    fn take_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Elements that can belong to a design option: model elements, rooms and areas, and view-specific annotation.
/// Datums, views, sheets, types and project settings stay in the main model.
pub fn can_be_in_option(e: &Element) -> bool {
    crate::phasing::is_phased(e)
        || matches!(
            e,
            Element::Room(_)
                | Element::RoomSeparation(_)
                | Element::RoomTag(_)
                | Element::Area(_)
                | Element::AreaBoundary(_)
                | Element::AreaTag(_)
                | Element::ElementTag(_)
                | Element::Dimension(_)
                | Element::Spot(_)
                | Element::DetailLine(_)
                | Element::DetailRegion(_)
                | Element::TextNote(_)
        )
}

/// What a set, an option, or its elements must hold to.
fn check(o: &DesignOptions) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    for s in &o.sets {
        if s.name.trim().is_empty() {
            return Err("an option set needs a name".into());
        }
        if !names.insert(s.name.trim().to_lowercase()) {
            return Err(format!("there is already an option set called '{}'", s.name.trim()));
        }
        if s.options.is_empty() {
            return Err(format!("option set '{}' needs at least one option", s.name));
        }
        if !s.has(s.primary) {
            return Err(format!("option set '{}' has no primary option", s.name));
        }
        let mut on = std::collections::BTreeSet::new();
        for x in &s.options {
            if x.name.trim().is_empty() || !on.insert(x.name.trim().to_lowercase()) {
                return Err(format!("the options of '{}' need names, each different", s.name));
            }
        }
    }
    Ok(())
}

impl Document {
    /// The project's design options, if it has any yet.
    pub fn design_options(&self) -> Option<(Id, &DesignOptions)> {
        let rev = self.revision();
        let id = match self.options_cache.get() {
            (Some(at), id) if at == rev => id,
            _ => {
                let id = self.iter().find_map(|(id, e)| matches!(e, Element::DesignOptions(_)).then_some(id));
                self.options_cache.set((Some(rev), id));
                id
            }
        }?;
        match self.get(id) {
            Some(Element::DesignOptions(o)) => Some((id, o)),
            _ => None,
        }
    }

    /// The option an element belongs to (None: the main model).
    pub fn option_of(&self, id: Id) -> Option<Id> {
        let (_, o) = self.design_options()?;
        o.member.get(&id).copied().filter(|x| o.set_of(*x).is_some())
    }

    /// The option being edited (None: the main model).
    pub fn active_option(&self) -> Option<Id> {
        self.active_option.filter(|a| self.design_options().is_some_and(|(_, o)| o.set_of(*a).is_some()))
    }

    /// Edit `option` (None: the main model): new elements go into it, and views on Automatic show it.
    pub fn set_active_option(&mut self, option: Option<Id>) -> Result<(), String> {
        if let Some(o) = option {
            if !self.design_options().is_some_and(|(_, d)| d.set_of(o).is_some()) {
                return Err("no such design option".into());
            }
        }
        self.active_option = option;
        // views on Automatic now show it: what was worked out for them is stale (not an edit of the project)
        self.revision += 1;
        Ok(())
    }

    /// The option of `set` a view shows: the one it is set to, else (Automatic) the option being edited when it is
    /// in that set, else the primary.
    pub fn view_option(&self, view: Option<&View>, set: &OptionSet) -> Id {
        if let Some(o) = view.and_then(|v| v.design_options.get(&set.id)).filter(|o| set.has(**o)) {
            return *o;
        }
        match self.active_option() {
            Some(a) if set.has(a) => a,
            _ => set.primary,
        }
    }

    /// Whether the view leaves the element out because it shows another option of the element's set (spec 4.2).
    /// With no view: what Automatic shows.
    pub fn option_hides(&self, view: Option<&View>, id: Id) -> bool {
        let Some((_, d)) = self.design_options() else { return false };
        let Some(&o) = d.member.get(&id) else { return false };
        let Some(s) = d.set_of(o) else { return false };
        self.view_option(view, s) != o
    }

    /// Whether the element is in a secondary option: left out of exports and of what the main model's rooms see.
    pub fn in_secondary_option(&self, id: Id) -> bool {
        let Some((_, d)) = self.design_options() else { return false };
        let Some(&o) = d.member.get(&id) else { return false };
        d.set_of(o).is_some_and(|s| s.primary != o)
    }

    /// Whether the element cannot be edited now: while an option is edited, everything outside it (spec 3.1).
    pub fn option_locked(&self, id: Id) -> bool {
        match self.active_option() {
            Some(a) => self.get(id).is_some_and(can_be_in_option) && self.option_of(id) != Some(a),
            None => false,
        }
    }

    /// The design options, made empty when the project has none. One undo step when made.
    pub fn ensure_design_options(&mut self) -> Result<Id, String> {
        if let Some((id, _)) = self.design_options() {
            return Ok(id);
        }
        self.transact("Design options", |tx| Ok(tx.add(Element::DesignOptions(DesignOptions::default()))))
    }

    /// Change the design options as one undo step.
    pub fn edit_design_options(&mut self, name: &str, f: impl FnOnce(&mut DesignOptions) -> Result<(), String>) -> Result<(), String> {
        let id = self.ensure_design_options()?;
        let Some(Element::DesignOptions(old)) = self.get(id).cloned() else { return Err("no design options".into()) };
        let mut new = old.clone();
        f(&mut new)?;
        check(&new)?;
        if new == old {
            return Ok(());
        }
        self.transact(name, |tx| tx.update(id, Element::DesignOptions(new)))
    }

    /// A new option set with one option, its primary ("Option 1"). Returns (set, option).
    pub fn add_option_set(&mut self, name: &str) -> Result<(Id, Id), String> {
        let mut made = (0, 0);
        self.edit_design_options("New option set", |d| {
            let (s, o) = (d.take_id(), d.take_id());
            d.sets.push(OptionSet { id: s, name: name.trim().to_string(), options: vec![DesignOption { id: o, name: "Option 1".into() }], primary: o });
            made = (s, o);
            Ok(())
        })?;
        Ok(made)
    }

    /// A new secondary option in `set`, named "Option n" unless given. Returns its id.
    pub fn add_design_option(&mut self, set: Id, name: Option<&str>) -> Result<Id, String> {
        let mut made = 0;
        self.edit_design_options("New design option", |d| {
            let id = d.take_id();
            let s = d.sets.iter_mut().find(|s| s.id == set).ok_or("no such option set")?;
            let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => n.to_string(),
                None => (1..).map(|k| format!("Option {k}")).find(|n| !s.options.iter().any(|o| &o.name == n)).unwrap(),
            };
            s.options.push(DesignOption { id, name });
            made = id;
            Ok(())
        })?;
        Ok(made)
    }

    /// Rename a set or an option.
    pub fn rename_design_option(&mut self, id: Id, name: &str) -> Result<(), String> {
        let name = name.trim().to_string();
        self.edit_design_options("Rename", |d| {
            if let Some(s) = d.sets.iter_mut().find(|s| s.id == id) {
                s.name = name;
                return Ok(());
            }
            let o = d.sets.iter_mut().flat_map(|s| s.options.iter_mut()).find(|o| o.id == id).ok_or("no such set or option")?;
            o.name = name;
            Ok(())
        })
    }

    /// Make an option its set's primary.
    pub fn make_primary(&mut self, option: Id) -> Result<(), String> {
        self.edit_design_options("Make primary", |d| {
            let s = d.sets.iter_mut().find(|s| s.has(option)).ok_or("no such design option")?;
            s.primary = option;
            Ok(())
        })
    }

    /// The elements in an option.
    pub fn option_elements(&self, option: Id) -> Vec<Id> {
        let Some((_, d)) = self.design_options() else { return vec![] };
        d.member.iter().filter(|(e, o)| **o == option && self.get(**e).is_some()).map(|(e, _)| *e).collect()
    }

    /// Delete a secondary option and everything in it (the primary cannot go: make another primary first).
    pub fn delete_design_option(&mut self, option: Id) -> Result<(), String> {
        let (_, d) = self.design_options().ok_or("no such design option")?;
        let s = d.set_of(option).ok_or("no such design option")?;
        if s.primary == option {
            return Err("The primary option cannot be deleted. Make another option primary first.".into());
        }
        let elements = self.option_elements(option);
        self.grouped("Delete design option", |doc| {
            doc.delete_quietly(&elements)?;
            doc.forget_option(option)
        })
    }

    /// Delete an option set, all its options and everything in them.
    pub fn delete_option_set(&mut self, set: Id) -> Result<(), String> {
        let (_, d) = self.design_options().ok_or("no such option set")?;
        let s = d.set(set).ok_or("no such option set")?.clone();
        let elements: Vec<Id> = s.options.iter().flat_map(|o| self.option_elements(o.id)).collect();
        self.grouped("Delete option set", |doc| {
            doc.delete_quietly(&elements)?;
            doc.forget_set(set)
        })
    }

    /// Accept Primary (spec 3.2): the primary's elements join the main model, the other options and everything in
    /// them are deleted, and the set goes. Views set to one of its options go back to Automatic.
    pub fn accept_primary(&mut self, set: Id) -> Result<(), String> {
        let (_, d) = self.design_options().ok_or("no such option set")?;
        let s = d.set(set).ok_or("no such option set")?.clone();
        let doomed: Vec<Id> = s.options.iter().filter(|o| o.id != s.primary).flat_map(|o| self.option_elements(o.id)).collect();
        self.grouped("Accept primary", |doc| {
            doc.delete_quietly(&doomed)?;
            doc.forget_set(set)
        })
    }

    /// Delete elements that may be pinned (their option is going).
    fn delete_quietly(&mut self, ids: &[Id]) -> Result<(), String> {
        let ids: Vec<Id> = ids.iter().copied().filter(|i| self.get(*i).is_some()).collect();
        if ids.is_empty() {
            return Ok(());
        }
        // pinned elements in a discarded option go with it; an infill goes with its door
        let pinned: Vec<(Id, Element)> = ids
            .iter()
            .filter_map(|i| match self.get(*i)? {
                Element::Wall(w) if w.pinned && w.infill.is_none() => Some((*i, Element::Wall(crate::element::Wall { pinned: false, ..w.clone() }))),
                Element::FamilyInstance(f) if f.pinned => Some((*i, Element::FamilyInstance(crate::FamilyInstance { pinned: false, ..f.clone() }))),
                _ => None,
            })
            .collect();
        if !pinned.is_empty() {
            self.transact("Unpin", |tx| {
                for (i, e) in pinned {
                    tx.update(i, e)?;
                }
                Ok(())
            })?;
        }
        let ids: Vec<Id> = ids.into_iter().filter(|i| !matches!(self.get(*i), Some(Element::Wall(w)) if w.infill.is_some())).collect();
        self.delete(&ids)
    }

    /// Take an option out of its set: its elements (if any are left) join the main model, views set to it go to
    /// Automatic.
    fn forget_option(&mut self, option: Id) -> Result<(), String> {
        let views: Vec<(Id, View)> = self.views().iter().filter(|(_, v)| v.design_options.values().any(|o| *o == option)).map(|(id, v)| (*id, (*v).clone())).collect();
        let Some((did, d)) = self.design_options() else { return Ok(()) };
        let mut n = d.clone();
        n.member.retain(|_, o| *o != option);
        for s in n.sets.iter_mut() {
            s.options.retain(|o| o.id != option);
        }
        self.transact("Design options", |tx| {
            for (id, mut v) in views {
                v.design_options.retain(|_, o| *o != option);
                tx.update(id, Element::View(v))?;
            }
            tx.update(did, Element::DesignOptions(n))
        })?;
        if self.active_option == Some(option) {
            self.active_option = None;
        }
        Ok(())
    }

    fn forget_set(&mut self, set: Id) -> Result<(), String> {
        let views: Vec<(Id, View)> = self.views().iter().filter(|(_, v)| v.design_options.contains_key(&set)).map(|(id, v)| (*id, (*v).clone())).collect();
        let Some((did, d)) = self.design_options() else { return Ok(()) };
        let mut n = d.clone();
        let options: Vec<Id> = n.set(set).map(|s| s.options.iter().map(|o| o.id).collect()).unwrap_or_default();
        n.member.retain(|_, o| !options.contains(o));
        n.sets.retain(|s| s.id != set);
        self.transact("Design options", |tx| {
            for (id, mut v) in views {
                v.design_options.remove(&set);
                tx.update(id, Element::View(v))?;
            }
            tx.update(did, Element::DesignOptions(n))
        })?;
        if self.active_option.is_some_and(|a| options.contains(&a)) {
            self.active_option = None;
        }
        Ok(())
    }

    /// Add to Set (spec 3.2): main-model elements go into the first of `options` (all in one set); each further
    /// option gets a copy of them in place, so the options can grow apart. One undo step; the elements in each
    /// option, in the order given.
    pub fn add_to_set(&mut self, ids: &[Id], options: &[Id]) -> Result<Vec<Vec<Id>>, String> {
        let (_, d) = self.design_options().ok_or("Make an option set first (Manage > Design Options).")?;
        let set = d.set_of(*options.first().ok_or("Choose at least one option.")?).ok_or("no such design option")?.id;
        if options.iter().any(|o| d.set_of(*o).map(|s| s.id) != Some(set)) {
            return Err("The options must all be in one set.".into());
        }
        let ids: Vec<Id> = ids.iter().copied().filter(|i| self.get(*i).is_some_and(can_be_in_option)).collect();
        if ids.is_empty() {
            return Err("Select model elements or annotation to add to the set.".into());
        }
        if ids.iter().any(|i| self.option_of(*i).is_some()) {
            return Err("Some of these are in a design option already; only main-model elements can be added to a set.".into());
        }
        // a wall's doors and windows go with it
        let mut ids = ids;
        let hosted: Vec<Id> = self.instances().into_iter().filter(|(_, i)| i.host.is_some_and(|h| ids.contains(&h.wall))).map(|(id, _)| id).collect();
        for h in hosted {
            if !ids.contains(&h) {
                ids.push(h);
            }
        }
        let options = options.to_vec();
        self.grouped("Add to set", |doc| {
            let mut out = vec![ids.clone()];
            for &o in &options[1..] {
                // copied aside while the option is edited, so the copies are made in it and join only its walls,
                // then put back in place
                let was = doc.active_option.replace(o);
                let before = doc.next_id;
                let made = doc.transform_elements(&ids, crate::edit::Xform::Move { dx: 1000.0, dy: 0.0 }, crate::edit::EditOptions { copy: true, disjoin: false }).and_then(|copies| {
                    let made: Vec<Id> = (before..doc.next_id).filter(|i| doc.get(*i).is_some_and(can_be_in_option)).collect();
                    let walls: Vec<Id> = made.iter().copied().filter(|i| matches!(doc.get(*i), Some(Element::Wall(_)))).collect();
                    let movers: Vec<Id> = made.iter().copied().filter(|i| !matches!(doc.get(*i), Some(Element::FamilyInstance(f)) if f.host.is_some_and(|h| walls.contains(&h.wall)))).collect();
                    doc.transform_elements(&movers, crate::edit::Xform::Move { dx: -1000.0, dy: 0.0 }, crate::edit::EditOptions { copy: false, disjoin: false })?;
                    Ok((copies, made))
                });
                doc.active_option = was;
                let (copies, made) = made?;
                // a copy keeps the phases of what it copies
                if copies.len() == ids.len() && doc.phasing().is_some() {
                    let pairs: Vec<(Id, Id)> = ids.iter().copied().zip(copies.iter().copied()).collect();
                    doc.edit_phasing("Add to set", |ph| {
                        for (a, b) in pairs {
                            match ph.created.get(&a).copied() {
                                Some(c) => ph.created.insert(b, c),
                                None => ph.created.remove(&b),
                            };
                            match ph.demolished.get(&a).copied() {
                                Some(c) => ph.demolished.insert(b, c),
                                None => ph.demolished.remove(&b),
                            };
                        }
                        Ok(())
                    })?;
                }
                out.push(made);
            }
            doc.edit_design_options("Add to set", |d| {
                for (o, els) in options.iter().zip(&out) {
                    for e in els {
                        d.member.insert(*e, *o);
                    }
                }
                Ok(())
            })?;
            Ok(out)
        })
    }

    /// Move elements to the main model or into an option (Properties' Design Option row; no copies).
    pub fn set_element_option(&mut self, ids: &[Id], option: Option<Id>) -> Result<(), String> {
        let ids: Vec<Id> = ids.iter().copied().filter(|i| self.get(*i).is_some_and(can_be_in_option)).collect();
        self.edit_design_options("Design option", |d| {
            if option.is_some_and(|o| d.set_of(o).is_none()) {
                return Err("no such design option".into());
            }
            for i in &ids {
                match option {
                    Some(o) => d.member.insert(*i, o),
                    None => d.member.remove(i),
                };
            }
            Ok(())
        })
    }

    /// A view's Design Options setting for one set: an option, or None for Automatic.
    pub fn set_view_option(&mut self, view: Id, set: Id, option: Option<Id>) -> Result<(), String> {
        let Some(Element::View(v)) = self.get(view).cloned() else { return Err("no such view".into()) };
        if let Some(o) = option {
            if !self.design_options().and_then(|(_, d)| d.set(set)).is_some_and(|s| s.has(o)) {
                return Err("that option is not in that set".into());
            }
        }
        let mut n = v.clone();
        match option {
            Some(o) => n.design_options.insert(set, o),
            None => n.design_options.remove(&set),
        };
        if n == v {
            return Ok(());
        }
        self.transact("View design options", |tx| tx.update(view, Element::View(n)))
    }

    /// Elements added while an option is edited go into it (spec 3.1, creation routing): `(id, before, after)` of
    /// the design options, or None.
    pub(crate) fn option_new_elements(&self, added: &[Id]) -> Option<(Id, Element, Element)> {
        let option = self.active_option()?;
        let (id, d) = self.design_options()?;
        let ids: Vec<Id> = added.iter().copied().filter(|i| self.get(*i).is_some_and(can_be_in_option) && !d.member.contains_key(i)).collect();
        if ids.is_empty() {
            return None;
        }
        let mut n = d.clone();
        for i in ids {
            n.member.insert(i, option);
        }
        Some((id, Element::DesignOptions(d.clone()), Element::DesignOptions(n)))
    }

    /// Why an element's host is not allowed (spec 5.1): a main-model element cannot sit in an option's element,
    /// nor an element of one option in another's.
    pub(crate) fn option_host_problem(&self, ids: &[Id]) -> Option<String> {
        self.design_options()?;
        for &i in ids {
            let Some(Element::FamilyInstance(f)) = self.get(i) else { continue };
            let Some(h) = f.host else { continue };
            let host = self.option_of(h.wall);
            if host.is_some() && host != self.option_of(i) {
                return Some(if self.option_of(i).is_none() {
                    "A main-model door or window cannot go in a wall that is in a design option. Edit that option, or add the door to its set.".into()
                } else {
                    "A door or window cannot go in a wall of another design option.".into()
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    fn setup() -> (Document, Id, Id) {
        let mut d = Document::new_project();
        let lvl = d.levels()[0].0;
        let wt = d.wall_types()[0].0;
        (d, lvl, wt)
    }

    #[test]
    fn views_show_the_primary_unless_set_or_editing_and_accept_primary_keeps_it() {
        let (mut d, lvl, wt) = setup();
        let main = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), 2700.0).unwrap();
        let (set, a) = d.add_option_set("Entry").unwrap();
        let b = d.add_design_option(set, None).unwrap();
        assert_eq!(d.design_options().unwrap().1.set(set).unwrap().options[1].name, "Option 2");
        // a wall drawn while editing B goes into B
        d.set_active_option(Some(b)).unwrap();
        let wb = d.add_wall(wt, lvl, Point::new(0.0, 3000.0), Point::new(5000.0, 3000.0), 2700.0).unwrap();
        assert_eq!(d.option_of(wb), Some(b));
        assert!(d.option_locked(main) && !d.option_locked(wb));
        let view = d.views().iter().find(|(_, v)| v.view_kind == crate::view::ViewKind::FloorPlan).unwrap().0;
        let v = |d: &Document| d.view(view).unwrap().clone();
        // Automatic shows the option being edited
        assert!(!d.option_hides(Some(&v(&d)), wb));
        d.set_active_option(None).unwrap();
        let w3 = d.add_wall(wt, lvl, Point::new(0.0, 6000.0), Point::new(5000.0, 6000.0), 2700.0).unwrap();
        assert_eq!(d.option_of(w3), None);
        // back in the main model, Automatic shows the primary (A): B's wall is hidden, the main wall is not
        assert!(d.option_hides(Some(&v(&d)), wb) && !d.option_hides(Some(&v(&d)), main));
        d.set_view_option(view, set, Some(b)).unwrap();
        assert!(!d.option_hides(Some(&v(&d)), wb));
        // Add to Set: the main wall goes into A, a copy into B
        let got = d.add_to_set(&[main], &[a, b]).unwrap();
        assert_eq!(got[0], vec![main]);
        assert_eq!(got[1].len(), 1);
        assert_eq!((d.option_of(main), d.option_of(got[1][0])), (Some(a), Some(b)));
        assert!(d.in_secondary_option(got[1][0]) && !d.in_secondary_option(main));
        // Accept Primary: A's walls join the main model, B's are gone, the set goes, the view is Automatic again
        d.accept_primary(set).unwrap();
        assert!(d.get(wb).is_none() && d.get(got[1][0]).is_none());
        assert_eq!(d.option_of(main), None);
        assert!(d.design_options().unwrap().1.sets.is_empty());
        assert!(d.view(view).unwrap().design_options.is_empty());
        // all of it is one undo step each way
        d.undo();
        assert!(d.get(wb).is_some() && d.option_of(main) == Some(a));
    }

    #[test]
    fn the_primary_cannot_be_deleted_and_a_main_door_cannot_go_in_an_option_wall() {
        let (mut d, lvl, wt) = setup();
        let (set, a) = d.add_option_set("Facade").unwrap();
        assert!(d.delete_design_option(a).is_err());
        let b = d.add_design_option(set, Some("Brick")).unwrap();
        d.make_primary(b).unwrap();
        d.set_active_option(Some(a)).unwrap();
        let w = d.add_wall(wt, lvl, Point::new(0.0, 0.0), Point::new(5000.0, 0.0), 2700.0).unwrap();
        d.set_active_option(None).unwrap();
        let door = d.family_types().into_iter().find(|(_, t)| t.name.contains("820")).map(|(id, _)| id).unwrap();
        assert!(d.place_on_wall(door, w, Point::new(1500.0, -200.0)).is_err());
        d.delete_design_option(a).unwrap();
        assert!(d.get(w).is_none());
    }
}
