// Groups in the project: Create Group, Place Model Group and Place Detail Group, the Modify | Model Groups (or Detail
// Groups) panel and Edit Group. Members stay ordinary elements. A click on one picks its group (plan.js,
// elevation.js); Tab reaches the member itself. A detail group lives in one view and is edited there.

const groupById = (id) => (app.info.groups || []).find((g) => g.id === id);
const selectedGroups = () => app.selection.map(groupById).filter(Boolean);
// members picked on their own (Tab), outside Edit Group
const selectedMembers = () => (app.groupEdit ? [] : app.selection.filter((id) => Plan.groupOf(id) != null));

// after every refresh: the plan learns the groups, and an Edit Group whose group went (an undo) ends
function syncGroups() {
  if (app.groupEdit && !groupById(app.groupEdit.id)) { app.groupEdit = null; run("group_edit_state", { id: null }); }
  Plan.setGroups(app.info.groups || [], app.groupEdit);
  Elev.setGroups(app.info.groups || [], app.groupEdit);
}

async function selectOnly(ids) {
  app.selection = ids;
  Plan.setSelection(ids);
  Elev.setSelection(ids);
  Plan.callbacks.select(ids);
}

function nextGroupName(kind) {
  const names = new Set((app.info.group_types || []).map((t) => t.name)), base = kind === "detail" ? "Detail Group" : "Model Group";
  for (let n = 1; ; n++) if (!names.has(`${base} ${n}`)) return `${base} ${n}`;
}

// the current view's detail lines, regions, text notes, dimensions and spots
function viewDetailIds() {
  const d = app.view.kind === "plan" ? Plan.viewData() : app.view.kind === "elevation" ? Elev.data : null;
  return new Set(d ? [...(d.detail_lines || []), ...(d.regions || []), ...(d.text_notes || []), ...(d.dimensions || []), ...(d.spots || [])].map((x) => x.id) : []);
}

// Create Group (GP): name it, and the selection becomes its first instance
function createGroup() {
  // groups in the selection go inside the new one (an attached detail group stays with its model group)
  const ids = app.selection.filter((id) => !groupById(id) || groupById(id).parent == null);
  if (!ids.length) { setPrompt("Select the elements to group first.", true); return; }
  // the view's items alone make a detail group; with model elements, a model group and its attached detail group
  const det = viewDetailIds(), kind = ids.every((i) => det.has(i) || (groupById(i) && groupById(i).kind === "detail")) ? "detail" : "model";
  openModal(kind === "detail" ? "Create Detail Group" : "Create Model Group", (body, foot, close) => {
    const inp = el("input", { type: "text", id: "group-name", value: nextGroupName(kind), style: "width:100%" });
    const msg = el("div", { className: "modal-msg" });
    body.append(el("label", { textContent: "Name" }), inp, msg);
    const ok = async () => {
      let r;
      try { r = await Backend.call("create_group", { ids, name: inp.value }); } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
      await refresh();
      await selectOnly([r.id]);
      const att = (app.info.groups || []).find((g) => g.parent === r.id);
      const made = `Created ${inp.value.trim()}${att ? ` and its attached detail group ${att.name}` : ""}.`;
      setPrompt(r.notes.length ? `${made} ${r.notes.map((n) => n[0].toUpperCase() + n.slice(1)).join(". ")}.` : made);
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") ok(); };
    foot.append(button("OK", ok, true), button("Cancel", close));
    inp.focus(); inp.select();
  });
}

// Architecture > Model Group and Annotate > Detail Group: pick a group (when there are several), then click to place
// it by its origin. A model group goes on the plan's level; a detail group in the plan, elevation or section open.
async function placeGroupCommand(kind = "model") {
  const types = (app.info.group_types || []).filter((t) => t.kind === kind);
  const what = kind === "detail" ? "detail groups" : "model groups";
  if (!types.length) { setPrompt(`There are no ${what} yet. Select ${kind === "detail" ? "detail items" : "elements"} and use Create Group.`, true); return; }
  if (kind === "model" ? !(await ensurePlan("A group")) : app.view.kind !== "plan" && app.view.kind !== "elevation") {
    if (kind === "detail") setPrompt("Detail groups are placed in a plan, elevation or section view.", true);
    return;
  }
  const V = app.view.kind === "elevation" ? Elev : Plan;
  const go = (t) => { V.placeGroupType({ typeId: t.id, name: t.name, kind }); setPrompt(`Click to place ${t.name} by its origin. Esc to stop.`); };
  if (types.length === 1) { go(types[0]); return; }
  openModal(kind === "detail" ? "Place Detail Group" : "Place Model Group", (body, foot, close) => {
    const sel = el("select", { size: Math.min(8, types.length), style: "width:100%" }, types.map((t, i) => new Option(t.name, t.id, false, i === 0)));
    const ok = () => { const t = types.find((x) => x.id === Number(sel.value)); close(); if (t) go(t); };
    sel.ondblclick = ok;
    body.append(sel);
    foot.append(button("Place", ok, true), button("Cancel", close));
    sel.focus();
  });
}

Plan.callbacks.placeGroup = Elev.callbacks.placeGroup = async (typeId, p) => {
  const v = viewById(app.view.viewId), t = (app.info.group_types || []).find((x) => x.id === typeId);
  const at = [Math.round(p.x * 1000) / 1000, Math.round(p.y * 1000) / 1000];
  if (t && t.kind !== "detail" && (!v || v.level_id == null)) { setPrompt("Place model groups in a floor plan.", true); return; }
  const r = await run("place_group", t && t.kind === "detail" ? { type_id: typeId, at, view_id: app.view.viewId } : { type_id: typeId, at, level_id: v.level_id });
  if (!r) return;
  await refresh();
  setPrompt(r.notes.length ? `Placed. ${r.notes.join(". ")}.` : "Placed. Click to place another, or Esc to stop.");
};

// ---- Edit Group: the rest of the model fades and cannot be picked; Finish carries the change to every instance ----
async function startGroupEdit(id) {
  const g = groupById(id);
  if (!g) return;
  if (g.kind === "detail" ? g.view_id !== app.view.viewId : app.view.kind !== "plan") {
    setPrompt(g.kind === "detail" ? "A detail group is edited in its own view: open that view first." : "Edit Group works in floor plans for now.", true);
    return;
  }
  const m = await run("edit_begin", {});
  if (!m) return;
  app.groupEdit = { id, mode: null, mark: m.mark };
  await run("group_edit_state", { id });
  syncGroups();
  await selectOnly([]);
  if (Plan.tool !== "select") Plan.setTool("select");
  if (Elev.tool !== "select") Elev.setTool("select");
  setPrompt(`Editing ${g.name}. Change its members as usual; Add and Remove change what is in it. Finish applies the change to every ${g.name}.`);
  updateRibbonContext(true);
}

function setGroupMode(mode) {
  if (!app.groupEdit) return;
  app.groupEdit.mode = app.groupEdit.mode === mode ? null : mode;
  syncGroups();
  setPrompt(app.groupEdit.mode === "add" ? "Click elements to add to the group." : app.groupEdit.mode === "remove" ? "Click members to take out of the group." : "Select members to change them.");
  updateRibbonContext();
}

Plan.callbacks.groupPick = async (ids) => {
  const ge = app.groupEdit;
  if (!ge || !ge.mode) return;
  if (await run(ge.mode === "add" ? "add_to_group" : "remove_from_group", { id: ge.id, ids })) await refresh();
};
Plan.callbacks.editGroup = Elev.callbacks.editGroup = (id) => startGroupEdit(id);
Elev.callbacks.groupPick = (ids) => Plan.callbacks.groupPick(ids);

async function finishGroupEdit(resolve) {
  const ge = app.groupEdit;
  if (!ge) return;
  const r = await run("finish_group", resolve ? { id: ge.id, resolve } : { id: ge.id });
  if (!r) return; // the reason is in the status bar; the edit stays open
  // instances that cannot take the change: Fix Groups, Ungroup them, or go back to editing
  if (!r.applied) { inconsistentDialog(ge, r.conflicts); return; }
  await run("edit_commit", { mark: ge.mark, name: "Edit group" });
  app.groupEdit = null;
  await run("group_edit_state", { id: null });
  await refresh();
  await selectOnly([ge.id]);
  const n = (groupById(ge.id) && (app.info.group_types.find((t) => t.id === groupById(ge.id).type_id) || {}).count) || 1;
  setPrompt(`${r.notes.length ? r.notes.join(". ") + ". " : ""}${n > 1 ? `All ${n} instances updated.` : "Group updated."}`);
}

async function cancelGroupEdit() {
  const ge = app.groupEdit;
  if (!ge) return;
  await run("edit_cancel", { mark: ge.mark });
  app.groupEdit = null;
  await run("group_edit_state", { id: null });
  await refresh();
  await selectOnly([ge.id]);
  setPrompt("Edit Group cancelled.");
}

// Groups Have Become Inconsistent: the change cannot be built in some instances
function inconsistentDialog(ge, conflicts) {
  const g = groupById(ge.id), name = g ? g.name : "the group";
  openModal("Groups Have Become Inconsistent", (body, foot, close) => {
    body.append(el("div", { className: "modal-msg", id: "inconsistent-msg", textContent: `${conflicts.length} instance${conflicts.length === 1 ? "" : "s"} of ${name} cannot take this change: ${conflicts[0].reason}.` }));
    body.append(el("div", { textContent: "Fix Groups excludes what does not fit from those instances. Ungroup turns them into plain elements as they were. Cancel goes back to editing." }));
    foot.append(
      button("Fix Groups", () => { close(); finishGroupEdit("fix"); }, true),
      button("Ungroup", () => { close(); finishGroupEdit("ungroup"); }),
      button("Cancel", () => { close(); setPrompt(`Still editing ${name}.`); }));
  });
}

// Delete on a member picked with Tab excludes it from that instance; anything else deletes as usual
async function excludeSelected() {
  const ids = selectedMembers();
  if (!ids.length) return false;
  if (await run("exclude_members", { ids })) { await refresh(); await selectOnly([]); setPrompt("Excluded from this instance. Restore All Excluded brings it back."); }
  return true;
}

function greyButton(label, icon, title) {
  const b = sketchButton({ label, icon }, true, () => {}, false, "icon-only");
  b.disabled = true; b.title = title;
  return { node: b };
}

// the panel a group selection or Edit Group adds to the Modify tab
function groupContext() {
  const ge = app.groupEdit;
  if (ge) {
    const g = groupById(ge.id);
    return { auto: true, key: `groupedit:${ge.id}:${ge.mode}`, title: "Edit Group", groups: [{ label: `Edit Group${g ? ": " + g.name : ""}`, items: [
      { label: "Add", icon: "groupAdd", onclick: () => setGroupMode("add"), on: ge.mode === "add" },
      { label: "Remove", icon: "groupRemove", onclick: () => setGroupMode("remove"), on: ge.mode === "remove" },
      greyButton("Attach", "modelGroup", "Attach: not yet. Select the attached detail group and use Edit Group on it"),
      { label: "Finish", ico: "✓", big: true, onclick: () => finishGroupEdit(), cls: "finish" },
      { label: "Cancel", ico: "✕", big: true, onclick: () => cancelGroupEdit(), cls: "cancel" },
    ] }] };
  }
  const gs = selectedGroups();
  // a group inside another, picked with Tab: edit it (every copy follows) or exclude it from this one
  if (gs.length === 1 && app.selection.length === 1 && Plan.groupOf(gs[0].id) != null && !ge) {
    return { auto: true, key: `nested:${gs[0].id}`, title: "Model Groups", groups: [{ label: "Group in a Group", items: [
      { label: "Edit Group", icon: "editGroup", big: true, onclick: () => startGroupEdit(gs[0].id) },
      { label: "Exclude", icon: "exclude", big: true, onclick: () => excludeSelected() },
    ] }] };
  }
  if (gs.length && gs.length === app.selection.length) {
    const ids = gs.map((g) => g.id), detail = gs.every((g) => g.kind === "detail");
    const arr = gs.length === 1 ? (app.info.arrays || []).find((a) => a.members.includes(gs[0].id)) : null;
    return { auto: true, key: `groups:${ids.join(",")}`, title: detail ? "Detail Groups" : "Model Groups", groups: [{ label: "Group", items: [
      { label: "Edit Group", icon: "editGroup", big: true, onclick: () => (gs.length === 1 ? startGroupEdit(gs[0].id) : setPrompt("Select one group to edit.", true)) },
      { label: "Ungroup", icon: "ungroup", onclick: async () => { const r = await run("ungroup", { ids }); if (r) { await refresh(); await selectOnly(r.ids); setPrompt("Ungrouped. The group stays in the project and can be placed again."); } } },
      { label: "Restore All Excluded", icon: "restoreExcluded", onclick: async () => { const r = await run("restore_excluded", { ids }); if (r) { await refresh(); setPrompt(r.ids.length ? `Restored ${r.ids.length} member${r.ids.length === 1 ? "" : "s"}.` : "Nothing is excluded."); } } },
      gs.length === 1 && gs[0].kind !== "detail" && app.view.kind === "plan"
        ? { label: "Attached Detail Groups", icon: "detailGroup", onclick: () => attachedDialog(gs[0]) }
        : greyButton("Attached Detail Groups", "detailGroup", "Attached Detail Groups: select one model group in a plan view"),
    ] }, ...(arr ? [arrayPanel(arr)] : [])] };
  }
  const ms = selectedMembers();
  if (ms.length && ms.length === app.selection.length) {
    return { auto: true, key: `members:${ms.join(",")}`, title: "Groups", groups: [{ label: "Group Member", items: [
      { label: "Exclude", icon: "exclude", big: true, onclick: () => excludeSelected() },
    ] }] };
  }
  return null;
}

// Attached Detail Groups: tick the attached detail groups this model group shows in the open plan view
async function attachedDialog(g) {
  const view = app.view.viewId;
  const choices = await run("attached_choices", { id: g.id, view_id: view });
  if (!choices) return;
  if (!choices.length) { setPrompt(`${g.name} has no attached detail groups for this kind of view. Create Group with its elements and the view's dimensions or notes makes one.`, true); return; }
  openModal("Attached Detail Group Placement", (body, foot, close) => {
    body.append(el("div", { className: "modal-msg", textContent: `Attached detail groups of ${g.name} shown in this view:` }));
    const boxes = choices.map((c) => {
      const cb = el("input", { type: "checkbox", checked: c.shown });
      cb.dataset.type = c.type_id;
      body.append(el("label", { className: "attached-choice", style: "display:block;margin:4px 0" }, [cb, ` ${c.name}`]));
      return [c, cb];
    });
    const ok = async () => {
      close();
      for (const [c, cb] of boxes) if (cb.checked !== c.shown) await run("show_attached", { id: g.id, view_id: view, type_id: c.type_id, show: cb.checked });
      await refresh();
      await selectOnly([g.id]);
      setPrompt("Attached detail groups updated.");
    };
    foot.append(button("OK", ok, true), button("Cancel", close));
  });
}

// a member of an associated array: its number (members come or go at the end with Move To 2nd, all are spaced
// again within the span with Last) and Remove Array, which keeps the groups
function arrayPanel(arr) {
  const min = arr.kind === "radial" ? 3 : 2;
  const num = el("input", { type: "number", min: String(min), max: "200", step: "1", value: arr.count, id: "rb-array-count" });
  num.onchange = async () => {
    const v = Math.round(parseFloat(num.value));
    if (!(v >= min && v <= 200)) { setPrompt(`The number must be from ${min} to 200.`, true); num.value = arr.count; return; }
    if (await run("set_array_count", { id: arr.id, count: v })) {
      await refresh();
      const now = (app.info.arrays || []).find((a) => a.id === arr.id);
      // a member removed from the end may have been the one selected
      if (now && !now.members.includes(app.selection[0])) await selectOnly([now.members[0]]);
      setPrompt(`The array has ${v} members.`);
    } else num.value = arr.count;
  };
  const field = el("label", { className: "rb-field", title: arr.move_to === "last" ? "Number: the members share the same span" : "Number: members come and go at the end" }, [el("span", { textContent: "Number" }), num]);
  return { label: `Array (Move To ${arr.move_to === "last" ? "Last" : "2nd"})`, items: [
    { node: field },
    { label: "Remove Array", icon: "ungroup", onclick: async () => { if (await run("remove_array", { id: arr.id })) { await refresh(); setPrompt("The array is gone; its members stay as groups."); } } },
  ] };
}
