// Model groups in the project: Create Group, Place Model Group, the Modify | Model Groups panel and Edit Group.
// Members stay ordinary elements. A click on one picks its group (plan.js); Tab reaches the member itself.

const groupById = (id) => (app.info.groups || []).find((g) => g.id === id);
const selectedGroups = () => app.selection.map(groupById).filter(Boolean);
// members picked on their own (Tab), outside Edit Group
const selectedMembers = () => (app.groupEdit ? [] : app.selection.filter((id) => Plan.groupOf(id) != null));

// after every refresh: the plan learns the groups, and an Edit Group whose group went (an undo) ends
function syncGroups() {
  if (app.groupEdit && !groupById(app.groupEdit.id)) app.groupEdit = null;
  Plan.setGroups(app.info.groups || [], app.groupEdit);
}

async function selectOnly(ids) {
  app.selection = ids;
  Plan.setSelection(ids);
  Plan.callbacks.select(ids);
}

function nextGroupName() {
  const names = new Set((app.info.group_types || []).map((t) => t.name));
  for (let n = 1; ; n++) if (!names.has(`Model Group ${n}`)) return `Model Group ${n}`;
}

// Create Group (GP): name it, and the selection becomes its first instance
function createGroup() {
  const ids = app.selection.filter((id) => !groupById(id));
  if (!ids.length) { setPrompt("Select the elements to group first.", true); return; }
  openModal("Create Model Group", (body, foot, close) => {
    const inp = el("input", { type: "text", id: "group-name", value: nextGroupName(), style: "width:100%" });
    const msg = el("div", { className: "modal-msg" });
    body.append(el("label", { textContent: "Name" }), inp, msg);
    const ok = async () => {
      let r;
      try { r = await Backend.call("create_group", { ids, name: inp.value }); } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
      await refresh();
      await selectOnly([r.id]);
      setPrompt(r.notes.length ? `Created ${inp.value.trim()}. ${r.notes.map((n) => n[0].toUpperCase() + n.slice(1)).join(". ")}.` : `Created ${inp.value.trim()}.`);
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") ok(); };
    foot.append(button("OK", ok, true), button("Cancel", close));
    inp.focus(); inp.select();
  });
}

// Architecture > Model Group: pick a group (when there are several), then click to place it by its origin
async function placeGroupCommand() {
  const types = app.info.group_types || [];
  if (!types.length) { setPrompt("There are no groups yet. Select elements and use Create Group.", true); return; }
  if (!(await ensurePlan("A group"))) return;
  const go = (t) => { Plan.placeGroupType({ typeId: t.id, name: t.name }); setPrompt(`Click to place ${t.name} by its origin. Esc to stop.`); };
  if (types.length === 1) { go(types[0]); return; }
  openModal("Place Model Group", (body, foot, close) => {
    const sel = el("select", { size: Math.min(8, types.length), style: "width:100%" }, types.map((t, i) => new Option(t.name, t.id, false, i === 0)));
    const ok = () => { const t = types.find((x) => x.id === Number(sel.value)); close(); if (t) go(t); };
    sel.ondblclick = ok;
    body.append(sel);
    foot.append(button("Place", ok, true), button("Cancel", close));
    sel.focus();
  });
}

Plan.callbacks.placeGroup = async (typeId, p) => {
  const v = viewById(app.view.viewId);
  if (!v || v.level_id == null) { setPrompt("Place groups in a floor plan.", true); return; }
  const r = await run("place_group", { type_id: typeId, at: [Math.round(p.x * 1000) / 1000, Math.round(p.y * 1000) / 1000], level_id: v.level_id });
  if (!r) return;
  await refresh();
  setPrompt(r.notes.length ? `Placed. ${r.notes.join(". ")}.` : "Placed. Click to place another, or Esc to stop.");
};

// ---- Edit Group: the rest of the model fades and cannot be picked; Finish carries the change to every instance ----
async function startGroupEdit(id) {
  const g = groupById(id);
  if (!g) return;
  if (app.view.kind !== "plan") { setPrompt("Edit Group works in floor plans for now.", true); return; }
  const m = await run("edit_begin", {});
  if (!m) return;
  app.groupEdit = { id, mode: null, mark: m.mark };
  syncGroups();
  await selectOnly([]);
  if (Plan.tool !== "select") Plan.setTool("select");
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
Plan.callbacks.editGroup = (id) => startGroupEdit(id);

async function finishGroupEdit() {
  const ge = app.groupEdit;
  if (!ge) return;
  const r = await run("finish_group", { id: ge.id });
  if (!r) return; // the reason is in the status bar; the edit stays open
  await run("edit_commit", { mark: ge.mark, name: "Edit group" });
  app.groupEdit = null;
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
  await refresh();
  await selectOnly([ge.id]);
  setPrompt("Edit Group cancelled.");
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
      greyButton("Attach", "modelGroup", "Attach: attached detail groups come later"),
      { label: "Finish", ico: "✓", big: true, onclick: () => finishGroupEdit(), cls: "finish" },
      { label: "Cancel", ico: "✕", big: true, onclick: () => cancelGroupEdit(), cls: "cancel" },
    ] }] };
  }
  if (app.view.kind !== "plan" && app.view.kind !== "3d") return null;
  const gs = selectedGroups();
  if (gs.length && gs.length === app.selection.length) {
    const ids = gs.map((g) => g.id);
    return { auto: true, key: `groups:${ids.join(",")}`, title: "Model Groups", groups: [{ label: "Group", items: [
      { label: "Edit Group", icon: "editGroup", big: true, onclick: () => (gs.length === 1 ? startGroupEdit(gs[0].id) : setPrompt("Select one group to edit.", true)) },
      { label: "Ungroup", icon: "ungroup", onclick: async () => { const r = await run("ungroup", { ids }); if (r) { await refresh(); await selectOnly(r.ids); setPrompt("Ungrouped. The group stays in the project and can be placed again."); } } },
      { label: "Restore All Excluded", icon: "restoreExcluded", onclick: async () => { const r = await run("restore_excluded", { ids }); if (r) { await refresh(); setPrompt(r.ids.length ? `Restored ${r.ids.length} member${r.ids.length === 1 ? "" : "s"}.` : "Nothing is excluded."); } } },
      greyButton("Attached Detail Groups", "modelGroup", "Attached Detail Groups: come later"),
    ] }] };
  }
  const ms = selectedMembers();
  if (ms.length && ms.length === app.selection.length) {
    return { auto: true, key: `members:${ms.join(",")}`, title: "Model Groups", groups: [{ label: "Group Member", items: [
      { label: "Exclude", icon: "exclude", big: true, onclick: () => excludeSelected() },
    ] }] };
  }
  return null;
}
