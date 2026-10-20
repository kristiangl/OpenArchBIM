// Properties palette and the dialogs (Visibility/Graphics, View Range). All
// content comes from the Rust core: rows say how they are edited and which JSON
// patch an edit makes, so this file has no per-property code.

// ---- Properties palette ---------------------------------------------------------
async function applyPatch(target, patch) {
  if (target.apply) return target.apply(patch); // dialogs handle their own refresh and errors
  if (await run(target.method, { id: target.id, patch, ...(target.extra || {}) })) await refresh();
}

function rowCells(r, target) {
  const label = document.createElement("td");
  label.textContent = r.label;
  const val = document.createElement("td");
  const lock = !!r.locked;
  if (lock) { label.classList.add("locked"); val.classList.add("locked"); val.title = "Controlled by the view template"; }
  const dis = (el) => { if (lock) el.disabled = true; return el; };
  if (r.kind === "select") {
    const sel = document.createElement("select");
    // several elements that disagree: a blank first choice stands for "leave each as it is"
    if (r.varies) { sel.add(new Option("*Varies*", -1, false, true)); }
    r.options.forEach((o, i) => sel.add(new Option(o.label, i, false, o.selected)));
    sel.onchange = () => { if (Number(sel.value) >= 0) applyPatch(target, r.options[Number(sel.value)].patch); };
    val.append(dis(sel));
  } else if (r.kind === "number" || r.kind === "text") {
    const inp = document.createElement("input");
    inp.type = r.kind === "number" ? "number" : "text";
    inp.value = r.varies ? "" : r.kind === "number" ? r.raw : r.value;
    if (r.varies) inp.placeholder = "*Varies*";
    if (r.kind === "number") inp.step = "any";
    inp.onchange = () => {
      const value = r.kind === "number" ? parseFloat(inp.value) : inp.value;
      if (r.kind === "number" && Number.isNaN(value)) return;
      applyPatch(target, nestPatch(r.path, value));
    };
    val.append(dis(inp));
  } else if (r.kind === "bool") {
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !!r.raw;
    if (r.varies) cb.indeterminate = true;
    cb.onchange = async () => {
      const patch = nestPatch(r.path, cb.checked);
      // switching a crop on gives it a region around the model
      if (r.path === "crop.enabled" && cb.checked) {
        const b = Plan.bounds();
        patch.crop.region = b ? [[b[0][0] - 1000, b[0][1] - 1000], [b[1][0] + 1000, b[1][1] + 1000]] : [[0, 0], [10000, 8000]];
      }
      applyPatch(target, patch);
    };
    val.append(dis(cb));
  } else if (r.kind === "color") {
    const inp = document.createElement("input");
    inp.type = "color";
    inp.value = r.value;
    inp.onchange = () => {
      const h = inp.value;
      applyPatch(target, nestPatch(r.path, [1, 3, 5].map((k) => parseInt(h.slice(k, k + 2), 16))));
    };
    val.append(dis(inp));
  } else if (r.kind === "dialog") {
    const b = document.createElement("button");
    b.textContent = r.value;
    b.className = "edit-btn";
    b.onclick = () => (target && target.dialogs && target.dialogs[r.dialog] ? target.dialogs[r.dialog]() : ({ vg: openVG, view_range: openViewRange })[r.dialog]?.());
    val.append(dis(b));
  } else {
    val.textContent = r.value;
    val.classList.add("ro");
  }
  return [label, val];
}

function groupTable(name, rows, target) {
  const t = document.createElement("table");
  t.className = "props";
  const head = t.insertRow();
  head.className = "pgroup";
  const hc = head.insertCell();
  hc.colSpan = 2;
  hc.textContent = name;
  for (const r of rows) {
    const tr = t.insertRow();
    tr.append(...rowCells(r, target));
  }
  return t;
}

let propsRender = 0;
async function renderProperties() {
  renderFilterCount();
  if (typeof updateRibbonContext === "function") updateRibbonContext();
  const sel = $("#type-select"), body = $("#prop-body");
  const token = ++propsRender;
  let d = null;
  if (app.selection.length === 1) d = await run("describe", { id: app.selection[0] });
  else if (app.selection.length > 1) d = await run("describe_multi", { ids: app.selection });
  else if (!app.selection.length && app.browserSel != null && viewById(app.browserSel)) d = await run("describe", { id: app.browserSel });
  else if (!app.selection.length && Plan.tool === "select" && app.view.kind !== "schedule" && curView()) d = await run("describe", { id: curView().id });
  if (token !== propsRender) return; // a newer render started while this one waited
  const tool = Plan.tool;

  let types = null, current = null, onType = null, title = "No selection";
  const groups = [];
  if (d && d.kind === "Multi") {
    title = d.title;
    const ids = d.ids;
    const onMulti = async (id) => { await run("set_type_multi", { ids, type_id: id }); await refresh(); };
    if (d.type_kind === "wall") { types = app.info.wall_types; current = d.type_id; onType = onMulti; }
    else if (d.type_kind === "floor") { types = app.info.floor_types; current = d.type_id; onType = onMulti; }
    else if (d.type_kind === "roof") { types = app.info.roof_types; current = d.type_id; onType = onMulti; }
    else if (d.type_kind === "instance") { types = familyTypes(d.category); current = d.type_id; onType = onMulti; }
    else if (d.type_kind === "grid") { types = app.info.grid_types; current = d.type_id; onType = onMulti; }
  } else if (d && d.kind === "Floor") {
    types = app.info.floor_types; current = d.type_id;
    onType = async (id) => { await run("set_floor_type", { id: d.id, type_id: id }); await refresh(); };
    title = d.title;
  } else if (d && d.kind === "Roof") {
    types = app.info.roof_types; current = d.type_id;
    onType = async (id) => { await run("set_roof_type", { id: d.id, type_id: id }); await refresh(); };
    title = d.title;
  } else if (d && d.kind === "Grid") {
    types = app.info.grid_types; current = d.type_id;
    onType = async (id) => { await run("patch_grid", { id: d.id, patch: { grid_type: id } }); await refresh(); };
    title = d.title;
  } else if (d && d.kind === "Level") {
    types = app.info.level_types; current = d.type_id;
    onType = async (id) => { await run("patch_level", { id: d.id, patch: { level_type: id } }); await refresh(); };
    title = d.title;
  } else if (d && d.kind === "Wall") {
    types = app.info.wall_types; current = d.type_id;
    onType = async (id) => { await run("set_wall_type", { id: d.id, type_id: id }); await refresh(); };
    title = d.title;
  } else if (d && d.kind === "FamilyInstance") {
    types = familyTypes(d.category); current = d.type_id;
    onType = async (id) => { await run("patch_instance", { id: d.id, patch: { type_id: id } }); await refresh(); };
    title = d.title;
  } else if (d) {
    title = d.title;
  } else if (app.selection.length > 1) {
    title = `${app.selection.length} elements`;
    groups.push({ name: "Selection", rows: [{ label: "Elements", value: String(app.selection.length), kind: "ro" }] });
  } else if (tool === "wall") {
    types = app.info.wall_types; current = app.wallTypeId;
    onType = (id) => { app.wallTypeId = id; updatePlanExtents(); renderProperties(); };
    const locOpts = app.info.location_lines.map((l) => ({ label: l.label, patch: l.value, selected: l.value === app.locationLine }));
    groups.push({ name: "Constraints", rows: [
      { label: "Location Line", kind: "select", options: locOpts, value: "" },
      { label: "Base Constraint", value: currentLevel().name, kind: "ro" },
      { label: "Top Constraint", kind: "select", options: wallTopOptions(), value: "" },
      { label: "Unconnected Height", kind: "number", raw: app.height, value: String(app.height), path: "h" },
    ] });
  } else if (tool === "component" || tool === "hosted") {
    types = componentTypes(); current = app.componentTypeId;
    onType = async (id) => { await useComponentType(id); renderProperties(); };
    title = "Place Component";
    if (tool === "hosted") groups.push({ name: "Constraints", rows: [{ label: "Host", value: "Wall (click to place)", kind: "ro" }] });
    else groups.push({ name: "Constraints", target: { apply: (patch) => { if (Number.isFinite(patch.offset)) app.componentOffset = patch.offset; renderProperties(); } }, rows: [
      { label: "Level", value: currentLevel().name, kind: "ro" },
      { label: "Host", value: `Level : ${currentLevel().name}`, kind: "ro" },
      { label: "Elevation from Level", kind: "number", raw: app.componentOffset || 0, value: String(app.componentOffset || 0), path: "offset" },
    ] });
  } else if (tool === "door" || tool === "window") {
    types = familyTypes(placeCategory(tool)); current = placeTypeId(tool);
    onType = (id) => { if (tool === "door") app.doorTypeId = id; else app.windowTypeId = id; syncPlacement(tool); renderProperties(); };
    title = tool === "door" ? "Place Door" : "Place Window";
    groups.push({ name: "Constraints", rows: [{ label: "Host", value: "Wall (click to place)", kind: "ro" }] });
  } else if (tool === "sketch" && Sketch.kind === "floor" && !Sketch.editingId) {
    types = app.info.floor_types; current = app.floorTypeId;
    onType = (id) => { app.floorTypeId = id; renderProperties(); };
    groups.push({ name: "Constraints", rows: [{ label: "Level", value: currentLevel().name, kind: "ro" }] });
    if (tool === "sketch") title = "Create Floor Boundary";
  } else if (tool === "sketch" && Sketch.kind === "roof" && !Sketch.editingId) {
    types = app.info.roof_types; current = app.roofTypeId;
    onType = (id) => { app.roofTypeId = id; renderProperties(); };
    title = "Create Roof Footprint";
    groups.push({ name: "Constraints", rows: [{ label: "Base Level", value: currentLevel().name, kind: "ro" }] });
  } else {
    groups.push({ name: "Identity Data", rows: [{ label: "View Name", value: "Wall and Floor Quantities", kind: "ro" }, { label: "View Type", value: "Schedule", kind: "ro" }] });
  }
  if (d) groups.push(...d.groups);

  sel.replaceChildren();
  if (types) {
    sel.disabled = false;
    if (current == null) sel.add(new Option("*Varies*", "", false, true));
    for (const t of types) sel.add(new Option(t.label || `${t.name} (${t.thickness_mm} mm)`, t.id, false, t.id === current));
    sel.onchange = () => { if (sel.value !== "") onType(Number(sel.value)); };
  } else {
    sel.disabled = true;
    sel.add(new Option(d && d.type_label ? d.type_label : title, ""));
  }
  renderTypePicker(types, current, onType, d && d.type_label ? d.type_label : title);

  // while drawing walls, the palette edits the tool's own settings
  const toolTarget = {
    method: "__tool", id: 0,
  };
  // Edit Type is available for family types (doors and windows)
  const et = $("#edit-type");
  et.hidden = !(types && types.length);
  // OK on another type (or a duplicate) puts the selection, or the placing tool, on it
  et.onclick = () => openTypeProperties(Number(sel.value) || current, { onType });
  // families with stored geometry open in the family editor
  const ef = $("#edit-family");
  const famOf = (tid) => { const t = (app.info.family_types || []).find((x) => x.id === tid); return t ? (app.info.families || []).find((f) => f.id === t.family_id) : null; };
  const shownFam = types && types.length && types[0].family_id ? famOf(Number(sel.value) || types[0].id) : null;
  ef.hidden = !(shownFam && shownFam.has_body);
  ef.onclick = () => { const f = famOf(Number(sel.value)); if (f) FamModel.open(f.id); };
  body.replaceChildren();
  const hint = document.createElement("div");
  hint.className = "prop-title";
  hint.textContent = title;
  body.append(hint);
  for (const g of groups) {
    const isTool = !d && tool === "wall" && g.name === "Constraints";
    const table = groupTable(g.name, g.rows, g.target || (isTool ? toolTarget : d ? d.target : null));
    if (isTool) wireToolRows(table);
    body.append(table);
  }
  // keep the view control bar in step with the view being shown
  if (d && d.kind === "View" && curView() && d.id === curView().id) {
    const row = (l) => d.groups.flatMap((g) => g.rows).find((r) => r.label === l);
    app.settings = {
      scale: (row("Scale Value  1:") || {}).raw || 100,
      detail_level: ((row("Detail Level") || { options: [] }).options.find((o) => o.selected) || { patch: { settings: { detail_level: "medium" } } }).patch.settings.detail_level,
      visual_style: ((row("Visual Style") || { options: [] }).options.find((o) => o.selected) || { patch: { settings: { visual_style: "shaded" } } }).patch.settings.visual_style,
    };
    syncViewBar();
  }
}

// ---- Type Selector ----------------------------------------------------------------
// A button showing the family and type, and a drop-down that lists every type under its family, with a search box
// and the most recently used types at the bottom. The native select stays (hidden) as the value holder.
const TYPE_GROUP = () => new Map([[app.info.wall_types, "Basic Wall"], [app.info.floor_types, "Floor"], [app.info.roof_types, "Basic Roof"], [app.info.level_types, "Level"], [app.info.grid_types, "Grid"]]);
function typeEntries(types) {
  const group = TYPE_GROUP().get(types) || "";
  return (types || []).map((t) => ({ id: t.id, family: t.family || group, name: t.name, category: t.category || "" }));
}
function renderTypePicker(types, current, onType, title) {
  const host = $("#type-picker");
  if (!host) return;
  host.replaceChildren();
  const entries = typeEntries(types);
  const cur = entries.find((e) => e.id === current);
  const btn = document.createElement("button");
  btn.className = "tp-btn"; btn.type = "button";
  const thumb = document.createElement("span"); thumb.className = "tp-thumb";
  setIcon(thumb, { id: iconForCategory(cur ? cur.category || cur.family : title) });
  const words = document.createElement("span"); words.className = "tp-words";
  const fam = document.createElement("span"); fam.className = "tp-fam";
  const typ = document.createElement("span"); typ.className = "tp-type";
  if (!types) { fam.textContent = title; typ.textContent = ""; btn.disabled = true; }
  else if (!cur) { fam.textContent = current == null && entries.length ? "*Varies*" : title; typ.textContent = ""; }
  else { fam.textContent = cur.family; typ.textContent = cur.name; }
  words.append(fam, typ);
  const caret = document.createElement("span"); caret.className = "tp-caret"; caret.textContent = "\u25be";
  btn.append(thumb, words, caret);
  host.append(btn);
  if (!types || !onType) return;
  btn.onclick = () => openTypePop(host, entries, current, onType);
}
function openTypePop(host, entries, current, onType) {
  const old = document.querySelector(".tp-pop");
  if (old) { old.remove(); return; }
  const pop = document.createElement("div"); pop.className = "tp-pop";
  const search = document.createElement("input"); search.className = "tp-search"; search.type = "search"; search.placeholder = "Search";
  const list = document.createElement("div"); list.className = "tp-list";
  const close = () => { pop.remove(); document.removeEventListener("mousedown", outside, true); };
  const outside = (e) => { if (!pop.contains(e.target) && !host.contains(e.target)) close(); };
  const choose = (id) => { close(); if (id !== current) onType(id); };
  const draw = () => {
    const q = search.value.trim().toLowerCase();
    list.replaceChildren();
    let fam = null;
    for (const e of entries) {
      if (q && !(e.family.toLowerCase().includes(q) || e.name.toLowerCase().includes(q))) continue;
      if (e.family !== fam) {
        fam = e.family;
        const h = document.createElement("div"); h.className = "tp-family";
        const ic = document.createElement("span"); ic.className = "tp-ficon"; setIcon(ic, { id: iconForCategory(e.category || e.family) });
        h.append(ic, document.createTextNode(e.family || "Types"));
        list.append(h);
      }
      const it = document.createElement("div"); it.className = "tp-item" + (e.id === current ? " cur" : "");
      it.textContent = e.name; it.dataset.id = e.id;
      it.onclick = () => choose(e.id);
      list.append(it);
    }
    if (!list.children.length) { const n = document.createElement("div"); n.className = "tp-none"; n.textContent = "No types match."; list.append(n); }
  };
  // families sorted by name, types under each by name, as the selector lists them
  entries.sort((a, b) => a.family.localeCompare(b.family) || a.name.localeCompare(b.name, undefined, { numeric: true }));
  search.oninput = draw;
  search.onkeydown = (e) => { if (e.key === "Escape") close(); if (e.key === "Enter") { const f = list.querySelector(".tp-item"); if (f) choose(Number(f.dataset.id)); } };
  pop.append(search, list);
  const mru = (typeof mruTypes === "function" ? mruTypes() : []).map((id) => entries.find((e) => e.id === id)).filter(Boolean);
  if (mru.length) {
    const h = document.createElement("div"); h.className = "tp-mru-title"; h.textContent = "Most Recently Used Types";
    pop.append(h);
    for (const e of mru) { const it = document.createElement("div"); it.className = "tp-mru"; it.textContent = `${e.family} : ${e.name}`; it.onclick = () => choose(e.id); pop.append(it); }
  }
  draw();
  host.append(pop);
  document.addEventListener("mousedown", outside, true);
  setTimeout(() => { search.focus(); const c = list.querySelector(".tp-item.cur"); if (c) c.scrollIntoView({ block: "nearest" }); }, 0);
}

// the wall tool's palette rows act on app state, not on a stored element
function wallTopOptions() {
  const above = levelsAbove(curLevelId());
  const auto = above.find((l) => l.building_story);
  const cur = app.wallTop;
  return [
    { label: auto ? `Up to level: ${auto.name} (next storey)` : "Unconnected", patch: "auto", selected: cur === "auto" },
    { label: "Unconnected", patch: "unconnected", selected: cur === "unconnected" },
    ...above.map((l) => ({ label: `Up to level: ${l.name}`, patch: String(l.id), selected: cur === l.id })),
  ];
}
function wireToolRows(table) {
  const [sel, topSel] = table.querySelectorAll("select");
  if (sel) sel.onchange = () => { app.locationLine = sel.options[sel.selectedIndex].textContent && app.info.location_lines[sel.selectedIndex].value; updatePlanExtents(); renderOptionsBar(); };
  if (topSel) topSel.onchange = () => { const v = wallTopOptions()[topSel.selectedIndex].patch; app.wallTop = v === "auto" || v === "unconnected" ? v : Number(v); renderOptionsBar(); renderProperties(); };
  const inp = table.querySelector("input[type=number]");
  if (inp) inp.onchange = () => { const v = parseFloat(inp.value); if (v > 0) { app.height = v; renderOptionsBar(); } };
}

// ---- modal helper ---------------------------------------------------------------
function openModal(title, build, { wide = false } = {}) {
  const back = document.createElement("div");
  back.className = "modal-back";
  const box = document.createElement("div");
  box.className = "modal" + (wide ? " wide" : "");
  const h = document.createElement("div");
  h.className = "modal-title";
  h.textContent = title;
  const content = document.createElement("div");
  content.className = "modal-body";
  const foot = document.createElement("div");
  foot.className = "modal-foot";
  box.append(h, content, foot);
  back.append(box);
  document.body.append(back);
  const close = () => back.remove();
  // a dialog that must tidy up on Escape (Type Properties cancels its edits) sets back.onEscape
  back.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); if (back.onEscape) back.onEscape(); else close(); } });
  back.tabIndex = -1;
  back.focus();
  build(content, foot, close);
  return close;
}

function button(text, onclick, primary) {
  const b = document.createElement("button");
  b.textContent = text;
  b.onclick = onclick;
  if (primary) b.classList.add("primary");
  return b;
}

// ---- View Range -----------------------------------------------------------------
async function openViewRange() {
  const v = curView();
  if (!v) return;
  const info = await run("view_range", { view_id: v.id });
  if (!info) return;
  const lvl = (l) => (typeof l === "string" ? l : `level:${l.level}`);
  const levelOptions = () => [["associated", `Associated Level (${currentLevel().name})`], ["above", "Level above"], ["below", "Level below"],
    ...app.info.levels.map((l) => [`level:${l.id}`, l.name]), ["unlimited", "Unlimited"]];
  const toLevel = (s) => (s.startsWith("level:") ? { level: Number(s.slice(6)) } : s);
  const range = JSON.parse(JSON.stringify(info.range));
  openModal(`View Range: Floor Plan: ${info.name}`, (body, foot, close) => {
    const mm = (x) => (x === null ? "unlimited" : `${Math.round(x)} mm`);
    const table = document.createElement("table");
    table.className = "range";
    const addRow = (title, plane, key) => {
      const tr = table.insertRow();
      tr.insertCell().textContent = title;
      const lc = tr.insertCell(), oc = tr.insertCell();
      if (plane) {
        const sel = document.createElement("select");
        levelOptions().forEach(([v2, l]) => sel.add(new Option(l, v2, false, v2 === lvl(plane.level))));
        sel.onchange = () => (plane.level = toLevel(sel.value));
        const off = document.createElement("input");
        off.type = "number"; off.step = "any"; off.value = plane.offset_mm;
        off.onchange = () => (plane.offset_mm = parseFloat(off.value) || 0);
        lc.append(sel); oc.append(off);
        if (info.locked) { sel.disabled = true; off.disabled = true; }
      } else {
        lc.textContent = `Associated Level (${currentLevel().name})`;
        const off = document.createElement("input");
        off.type = "number"; off.step = "any"; off.value = range.cut_offset_mm;
        off.onchange = () => (range.cut_offset_mm = parseFloat(off.value) || 0);
        oc.append(off);
        if (info.locked) off.disabled = true;
      }
      tr.insertCell().textContent = info.resolved[key] === undefined ? "" : mm(info.resolved[key]);
    };
    const head = table.insertRow();
    ["", "Level", "Offset (mm)", "Now at"].forEach((t) => { const th = document.createElement("th"); th.textContent = t; head.append(th); });
    const p1 = document.createElement("tr"); p1.className = "sect"; p1.innerHTML = '<td colspan="4">Primary Range</td>'; table.append(p1);
    addRow("Top:", range.top, "top");
    addRow("Cut plane:", null, "cut");
    addRow("Bottom:", range.bottom, "bottom");
    const p2 = document.createElement("tr"); p2.className = "sect"; p2.innerHTML = '<td colspan="4">View Depth</td>'; table.append(p2);
    addRow("Level:", range.depth, "depth");
    body.append(table);
    if (info.locked) { const n = document.createElement("p"); n.className = "note"; n.textContent = "The view range is controlled by this view's template."; body.append(n); }
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(msg);
    const save = async (andClose) => {
      const r = await Backend.call("patch_view", { id: v.id, patch: { settings: { view_range: range } } }).then(() => null, (e) => e);
      if (r) { msg.textContent = r.message || String(r); return; }
      await refresh();
      if (andClose) close();
    };
    foot.append(button("OK", () => save(true), true), button("Cancel", close), button("Apply", () => save(false)));
  });
}

// ---- Visibility / Graphics --------------------------------------------------------
const hex2rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const rgb2hex = (c) => "#" + c.map((x) => x.toString(16).padStart(2, "0")).join("");

async function openVG() {
  const v = curView();
  if (!v) return;
  const g = await run("view_graphics", { view_id: v.id });
  if (!g) return;
  const std = await run("standards", {});
  const linePats = std ? std.line_patterns.map((p) => p.pattern.name) : ["Solid"];
  const fillPats = std ? std.fill_patterns.map((p) => p.pattern.name) : ["Solid fill"];
  const data = { model: g.model, annotation: g.annotation, imported: g.imported };
  const locked = { model: g.model_locked, annotation: g.annotation_locked, imported: g.imported_locked };
  const draft = { model: {}, annotation: {}, imported: {} }; // changed categories, full override objects
  let tab = "model";
  const names = { model: "Model Categories", annotation: "Annotation Categories", imported: "Imported Categories" };

  openModal(`Visibility/Graphics Overrides for Floor Plan: ${g.name}`, (body, foot, close) => {
    const tabs = document.createElement("div");
    tabs.className = "modal-tabs";
    const host = document.createElement("div");
    host.className = "vg-host";
    body.append(tabs, host);
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(msg);

    const ovOf = (cat) => (draft[tab][cat.name] ||= JSON.parse(JSON.stringify(cat.override)));

    const lineButton = (cat, key, label) => {
      const o = cat.override[key] || {};
      const b = document.createElement("button");
      b.className = "swatch";
      const col = o.color ? rgb2hex(o.color) : null;
      b.innerHTML = `<span class="sw-line" style="${col ? `background:${col}` : ""}"></span><span>${o.weight ? o.weight : ""}</span>`;
      b.title = `${label}: click to override`;
      b.disabled = locked[tab];
      b.onclick = () => editLine(b, cat, key);
      return b;
    };
    const fillButton = (cat, key) => {
      const o = cat.override[key] || {};
      const b = document.createElement("button");
      b.className = "swatch";
      b.innerHTML = `<span class="sw-fill" style="${o.color ? `background:${rgb2hex(o.color)}` : ""}"></span><span>${o.pattern ? o.pattern : ""}</span>`;
      b.disabled = locked[tab];
      b.onclick = () => editFill(b, cat, key);
      return b;
    };

    function popover(anchor, fields, onOk) {
      document.querySelectorAll(".popover").forEach((p) => p.remove());
      const pop = document.createElement("div");
      pop.className = "popover";
      const r = anchor.getBoundingClientRect();
      pop.style.left = `${Math.min(r.left, window.innerWidth - 260)}px`;
      pop.style.top = `${r.bottom + 2}px`;
      pop.append(...fields.nodes);
      pop.append(button("OK", () => { onOk(); pop.remove(); render(); }, true), button("Cancel", () => pop.remove()));
      document.body.append(pop);
    }
    const field = (label, el) => { const l = document.createElement("label"); l.textContent = label; l.append(el); return l; };

    function editLine(anchor, cat, key) {
      const cur = ovOf(cat)[key] || (ovOf(cat)[key] = {});
      const weight = document.createElement("select");
      weight.add(new Option("By category", ""));
      for (let i = 1; i <= 16; i++) weight.add(new Option(String(i), i, false, cur.weight === i));
      const color = document.createElement("input"); color.type = "color"; color.value = cur.color ? rgb2hex(cur.color) : "#000000";
      const useColor = document.createElement("input"); useColor.type = "checkbox"; useColor.checked = !!cur.color;
      const pattern = document.createElement("select");
      for (const p of ["", ...linePats]) pattern.add(new Option(p || "By category", p, false, (cur.pattern || "") === p));
      popover(anchor, { nodes: [field("Weight", weight), field("Override colour", useColor), field("Colour", color), field("Pattern", pattern)] }, () => {
        cur.weight = weight.value ? Number(weight.value) : null;
        cur.color = useColor.checked ? hex2rgb(color.value) : null;
        cur.pattern = pattern.value || null;
      });
    }
    function editFill(anchor, cat, key) {
      const cur = ovOf(cat)[key] || (ovOf(cat)[key] = { visible: true });
      const vis = document.createElement("input"); vis.type = "checkbox"; vis.checked = cur.visible !== false;
      const useColor = document.createElement("input"); useColor.type = "checkbox"; useColor.checked = !!cur.color;
      const color = document.createElement("input"); color.type = "color"; color.value = cur.color ? rgb2hex(cur.color) : "#969ba2";
      const pattern = document.createElement("select");
      for (const p of ["", ...fillPats]) pattern.add(new Option(p || "By category", p, false, (cur.pattern || "") === p));
      popover(anchor, { nodes: [field("Visible", vis), field("Override colour", useColor), field("Colour", color), field("Pattern", pattern)] }, () => {
        cur.visible = vis.checked;
        cur.color = useColor.checked ? hex2rgb(color.value) : null;
        cur.pattern = pattern.value || null;
      });
    }

    function render() {
      tabs.replaceChildren(...Object.keys(names).map((k) => {
        const b = document.createElement("button");
        b.textContent = names[k];
        b.className = k === tab ? "active" : "";
        b.onclick = () => { tab = k; render(); };
        return b;
      }));
      host.replaceChildren();
      if (locked[tab]) { const n = document.createElement("p"); n.className = "note"; n.textContent = "These settings are controlled by this view's template."; host.append(n); }
      const isModel = tab === "model";
      const table = document.createElement("table");
      table.className = "vg";
      const head = table.createTHead().insertRow();
      for (const t of ["Visibility", "Projection/Surface: Lines", "Patterns", "Transparency", ...(isModel ? ["Cut: Lines", "Patterns"] : []), "Halftone", ...(isModel ? ["Detail Level"] : [])]) {
        const th = document.createElement("th"); th.textContent = t; head.append(th);
      }
      const tb = table.createTBody();
      if (!data[tab].length) { const tr = tb.insertRow(); const td = tr.insertCell(); td.colSpan = 9; td.className = "empty"; td.textContent = "Nothing in this tab yet"; }
      for (const raw of data[tab]) {
        const cat = { name: raw.name, override: draft[tab][raw.name] || raw.override };
        const tr = tb.insertRow();
        const vis = document.createElement("label");
        const cb = document.createElement("input"); cb.type = "checkbox"; cb.checked = cat.override.visible !== false; cb.disabled = locked[tab];
        cb.onchange = () => { ovOf(cat).visible = cb.checked; };
        vis.append(cb, " " + cat.name);
        tr.insertCell().append(vis);
        tr.insertCell().append(lineButton(cat, "projection_lines", "Projection lines"));
        tr.insertCell().append(fillButton(cat, "surface_foreground"));
        const tr2 = document.createElement("input"); tr2.type = "number"; tr2.min = 0; tr2.max = 100; tr2.value = cat.override.transparency || 0; tr2.disabled = locked[tab];
        tr2.onchange = () => { ovOf(cat).transparency = Math.max(0, Math.min(100, parseInt(tr2.value) || 0)); };
        tr.insertCell().append(tr2);
        if (isModel) {
          tr.insertCell().append(lineButton(cat, "cut_lines", "Cut lines"));
          tr.insertCell().append(fillButton(cat, "cut_foreground"));
        }
        const ht = document.createElement("input"); ht.type = "checkbox"; ht.checked = !!cat.override.halftone; ht.disabled = locked[tab];
        ht.onchange = () => { ovOf(cat).halftone = ht.checked; };
        tr.insertCell().append(ht);
        if (isModel) {
          const dl = document.createElement("select"); dl.disabled = locked[tab];
          for (const [v2, l] of [["", "By View"], ["coarse", "Coarse"], ["medium", "Medium"], ["fine", "Fine"]]) dl.add(new Option(l, v2, false, (cat.override.detail_level || "") === v2));
          dl.onchange = () => { ovOf(cat).detail_level = dl.value || null; };
          tr.insertCell().append(dl);
        }
      }
      host.append(table);
    }

    const save = async (andClose) => {
      const settings = { overrides: {} };
      for (const k of Object.keys(draft)) if (Object.keys(draft[k]).length) settings.overrides[k] = draft[k];
      if (Object.keys(settings.overrides).length) {
        const err = await Backend.call("patch_view", { id: v.id, patch: { settings } }).then(() => null, (e) => e);
        if (err) { msg.textContent = err.message || String(err); return; }
        // what was applied is now the starting point
        for (const k of Object.keys(draft)) {
          data[k] = data[k].map((c) => (draft[k][c.name] ? { name: c.name, override: draft[k][c.name] } : c));
          draft[k] = {};
        }
        await refresh();
      }
      if (andClose) close(); else render();
    };
    foot.append(button("OK", () => save(true), true), button("Cancel", close), button("Apply", () => save(false)));
    render();
  }, { wide: true });
}

// ---- view commands ------------------------------------------------------------------
async function duplicateView() {
  const v = curView();
  if (!v) return;
  const r = await run("duplicate_view", { id: v.id });
  if (r) { await refresh(); await openView(viewerOf(v.view_kind), r.id); setPrompt("Duplicated the view."); }
}

function createTemplate() {
  const v = curView();
  if (!v) return;
  openModal("Create View Template from Current View", (body, foot, close) => {
    const label = document.createElement("label");
    label.textContent = "Name ";
    const inp = document.createElement("input");
    inp.type = "text";
    inp.id = "tpl-name";
    inp.value = `${v.name} template`;
    inp.style.width = "100%";
    label.append(inp);
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(label, msg);
    const ok = async () => {
      const name = inp.value.trim();
      if (!name) { msg.textContent = "Enter a name."; return; }
      const r = await Backend.call("create_view_template", { name, view_id: v.id }).then((x) => x, (e) => { msg.textContent = e.message || String(e); return null; });
      if (!r) return;
      close();
      await refresh();
      setPrompt(`Created view template '${name}'. Assign it in the view's Properties.`);
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") ok(); };
    foot.append(button("OK", ok, true), button("Cancel", close));
    inp.focus(); inp.select();
  });
}


// ---- Edit Type dialog ---------------------------------------------------------------
function catalogPrompt(family, names, done) {
  openModal(`${family.name}: sizes`, (body, foot, close) => {
    const sel = document.createElement("select");
    sel.id = "catalog-size"; sel.size = Math.min(10, names.length); sel.style.width = "100%";
    for (const n of names) sel.add(new Option(n, n));
    sel.selectedIndex = 0;
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(sel, msg);
    const ok = async () => {
      let r;
      try { r = await Backend.call("add_catalog_type", { family_id: family.id, name: sel.value }); } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
      await done(r.id);
    };
    sel.ondblclick = ok;
    foot.append(button("Add", ok, true), button("Cancel", close));
  });
}

function duplicateTypePrompt(typeId, done, method = "duplicate_family_type") {
  openModal("Name", (body, foot, close) => {
    const inp = document.createElement("input");
    inp.type = "text"; inp.id = "dup-name"; inp.style.width = "100%";
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(inp, msg);
    const ok = async () => {
      let r;
      try { r = await Backend.call(method, { id: typeId, name: inp.value }); } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
      await done(r.id);
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") ok(); };
    foot.append(button("OK", ok, true), button("Cancel", close));
    inp.focus();
  });
}

