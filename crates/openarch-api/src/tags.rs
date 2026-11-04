//! Door and window tags: Properties, what a view draws for them, Tag by Category and Tag All.
use std::collections::BTreeMap;

use serde_json::{json, Value};

use openarch_model::annotation::AnnotationKind;
use openarch_model::tags::{ElementTag, LeaderEnd, TagOptions, TagOrientation};
use openarch_model::{Element, Id};

use crate::{id_of, Res, Session};

/// The tag kinds Tag All offers, by the name the UI sends.
const KINDS: [(&str, AnnotationKind); 2] = [("door_tag", AnnotationKind::DoorTag), ("window_tag", AnnotationKind::WindowTag)];

fn kind_named(name: &str) -> Option<AnnotationKind> {
    KINDS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

fn options_of(p: &Value) -> Result<TagOptions, String> {
    let mut o: TagOptions = serde_json::from_value(p.get("options").cloned().unwrap_or(json!({}))).map_err(|e| format!("bad tag options: {e}"))?;
    if o.leader_mm == 0.0 { o.leader_mm = 12.0; }
    Ok(o)
}

impl Session {
    pub(crate) fn handle_tags(&mut self, method: &str, p: &Value) -> Option<Res> {
        Some(match method {
            // Tag by Category and Tag on Placement: { view_id, element_id, options: { orientation, leader, leader_mm, family } }
            "add_tag" => (|| { let o = options_of(p)?; Ok(json!({ "id": self.doc.add_element_tag(id_of(p, "view_id")?, id_of(p, "element_id")?, &o)? })) })(),
            "patch_tag" => (|| { self.doc.patch_element_tag(id_of(p, "id")?, p.get("patch").ok_or("missing patch")?)?; Ok(json!({})) })(),
            // Spacebar on selected tags: horizontal and vertical swap, as one undo step
            "flip_tags" => (|| {
                let ids = crate::ids_of(p, "ids")?;
                self.doc.grouped("Tag orientation", |d| {
                    for id in &ids {
                        let Some(t) = d.element_tag(*id) else { continue };
                        let o = if t.orientation == TagOrientation::Horizontal { "vertical" } else { "horizontal" };
                        d.patch_element_tag(*id, &json!({ "orientation": o }))?;
                    }
                    Ok(())
                })?;
                Ok(json!({}))
            })(),
            "tag_all_options" => self.tag_all_options(p),
            "tag_all" => self.tag_all(p),
            _ => return None,
        })
    }

    /// For the Tag All dialog: each tag category, its loaded families and how many elements in the view have no tag.
    fn tag_all_options(&self, p: &Value) -> Res {
        let d = &self.doc;
        let view = id_of(p, "view_id")?;
        let only: Option<Vec<Id>> = p.get("only").filter(|v| v.is_array()).map(|_| crate::ids_of(p, "only")).transpose()?;
        let wanted = |id: Id| only.as_ref().is_none_or(|o| o.contains(&id));
        let mut out = vec![];
        for (name, kind) in KINDS {
            let untagged = d.untagged(view, &[kind], only.as_deref()).len();
            let families: Vec<Value> = d.annotation_families().into_iter().filter(|(_, f)| f.kind == kind).map(|(id, f)| json!({ "id": id, "name": f.name })).collect();
            out.push(json!({ "kind": name, "label": kind.category(), "families": families, "untagged": untagged }));
        }
        // rooms on a plan's level that have no tag in it
        if let Some(level) = d.view(view).filter(|v| v.view_kind.is_plan()).and_then(|v| v.level) {
            let untagged = d.rooms().into_iter().filter(|(id, r)| wanted(*id) && r.level == level && r.at.is_some() && !d.room_tags(view).iter().any(|(_, t)| t.room == *id)).count();
            let families: Vec<Value> = d.annotation_families().into_iter().filter(|(_, f)| f.kind == AnnotationKind::RoomTag).map(|(id, f)| json!({ "id": id, "name": f.name })).collect();
            out.push(json!({ "kind": "room_tag", "label": "Room Tags", "families": families, "untagged": untagged }));
        }
        Ok(json!(out))
    }

    /// Tag All Not Tagged: { view_id, kinds: [{ kind, family }], only?: [ids], options } as one undo step.
    fn tag_all(&mut self, p: &Value) -> Res {
        let view = id_of(p, "view_id")?;
        let o = options_of(p)?;
        let only: Option<Vec<Id>> = p.get("only").filter(|v| v.is_array()).map(|_| crate::ids_of(p, "only")).transpose()?;
        let mut kinds = vec![];
        let mut rooms = false;
        for k in p["kinds"].as_array().ok_or("missing kinds")? {
            let name = k["kind"].as_str().unwrap_or("");
            if name == "room_tag" { rooms = true; continue; }
            kinds.push((kind_named(name).ok_or_else(|| format!("unknown tag category {name}"))?, k["family"].as_u64()));
        }
        let room_ids: Vec<(Id, openarch_model::Point)> = match self.doc.view(view).filter(|v| rooms && v.view_kind.is_plan()).and_then(|v| v.level) {
            Some(level) => self.doc.rooms().into_iter()
                .filter(|(id, r)| only.as_ref().is_none_or(|s| s.contains(id)) && r.level == level && !self.doc.room_tags(view).iter().any(|(_, t)| t.room == *id))
                .filter_map(|(id, r)| Some((id, r.at?))).collect(),
            None => vec![],
        };
        let ids = self.doc.grouped("Tag All", |d| {
            let mut ids = d.tag_all(view, &kinds, only.as_deref(), &o)?;
            for (room, at) in &room_ids { ids.push(d.add_room_tag(view, *room, *at)?); }
            Ok(ids)
        })?;
        Ok(json!({ "ids": ids }))
    }

    /// What a view draws for its door and window tags: the head, the element's tag point, the leader's end and elbow
    /// (view mm), and the family drawing with its labels filled in from the element's parameters.
    pub(crate) fn tag_drawing(&self, view: Id) -> Value {
        let d = &self.doc;
        let Some(v) = d.view(view) else { return json!({}) };
        let settings = d.effective_settings(view).unwrap_or_default();
        let visible = |cat: &str, model: bool| (if model { settings.overrides.model.get(cat) } else { settings.overrides.annotation.get(cat) }).is_none_or(|o| o.visible);
        let mut types = BTreeMap::new();
        let tags: Vec<Value> = d.element_tags(view).into_iter().filter_map(|(id, t)| {
            if v.hidden_elements.contains(&id) || v.hidden_elements.contains(&t.element) { return None; }
            let kind = d.tag_kind(t.element)?;
            if !visible(kind.category(), false) || !visible(&self.schedule_category(t.element)?, true) { return None; }
            let p = d.tag_point(view, t.element)?;
            let fam = t.family.and_then(|f| d.annotation_family(f))?;
            let values = self.param_texts(t.element, &mut types);
            let labels = d.symbol_texts(fam, &|name| Some(values.get(name).cloned().unwrap_or_default()), false);
            let at = |o: [f64; 2]| json!({ "x": p.x + o[0], "y": p.y + o[1] });
            let end = match t.leader_end { LeaderEnd::Free => at(t.end_offset.unwrap_or([0.0, 0.0])), LeaderEnd::Attached => at([0.0, 0.0]) };
            Some(json!({
                "id": id, "element": t.element, "category": kind.category(), "at": at(t.offset), "point": at([0.0, 0.0]), "end": end,
                "elbow": t.elbow.map(at), "leader": t.leader, "vertical": t.orientation == TagOrientation::Vertical, "pinned": t.pinned,
                "family": d.symbol_drawing(fam, false), "labels": labels,
            }))
        }).collect();
        json!({ "element_tags": tags })
    }

    pub(crate) fn tag_props(&self, id: Id, t: &ElementTag) -> Value {
        let d = &self.doc;
        let kind = d.tag_kind(t.element);
        let what = match d.get(t.element) {
            Some(Element::FamilyInstance(i)) => {
                let name = d.instance_family(i).map(|(f, ty)| format!("{}: {}", f.name, ty.name)).unwrap_or_default();
                if i.mark.is_empty() { name } else { format!("{name} (Mark {})", i.mark) }
            }
            _ => String::new(),
        };
        let select = |label: &str, value: &str, opts: Vec<(String, Value, bool)>| json!({
            "label": label, "kind": "select", "value": value,
            "options": opts.into_iter().map(|(l, patch, selected)| json!({ "label": l, "patch": patch, "selected": selected })).collect::<Vec<_>>(),
        });
        let families: Vec<(String, Value, bool)> = d.annotation_families().into_iter().filter(|(_, f)| Some(f.kind) == kind)
            .map(|(fid, f)| (f.name.clone(), json!({ "family": fid }), t.family == Some(fid))).collect();
        let family_name = t.family.and_then(|f| d.annotation_family(f)).map_or(String::new(), |f| f.name.clone());
        let free = t.leader_end == LeaderEnd::Free;
        let vertical = t.orientation == TagOrientation::Vertical;
        json!({
            "id": id, "kind": "ElementTag", "category": kind.map_or("Tags", |k| k.category()),
            "title": kind.map_or("Tag", |k| k.label()), "type_label": family_name,
            "target": { "method": "patch_tag", "id": id },
            "groups": [
                { "name": "Graphics", "rows": [
                    { "label": "Leader Line", "kind": "bool", "path": "leader", "raw": t.leader, "value": if t.leader { "Yes" } else { "No" } },
                    select("Leader End", if free { "Free" } else { "Attached" }, vec![("Attached".into(), json!({ "leader_end": "attached" }), !free), ("Free".into(), json!({ "leader_end": "free" }), free)]),
                    select("Orientation", if vertical { "Vertical" } else { "Horizontal" }, vec![("Horizontal".into(), json!({ "orientation": "horizontal" }), !vertical), ("Vertical".into(), json!({ "orientation": "vertical" }), vertical)]),
                    select("Family", &family_name, families),
                    { "label": "Pinned", "kind": "bool", "path": "pinned", "raw": t.pinned, "value": if t.pinned { "Yes" } else { "No" } },
                ] },
                { "name": "Identity Data", "rows": [{ "label": "Tagged", "kind": "ro", "value": what }] },
            ],
        })
    }
}
