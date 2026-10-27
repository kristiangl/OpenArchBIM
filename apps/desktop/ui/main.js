const $ = (s) => document.querySelector(s);

const app = {
  info: null,
  tdimMemory: {}, // element id -> the dimension ends the person chose for it (kept while the project is open)
  doc: "project", // the document showing: "project", "family" (a model family in the family editor) or "annotation" (an annotation family)
  ribbonTab: { project: "Architecture", family: "Create", annotation: "Create" },
  ctxSig: "", // what the Modify tab last drew after its own panels (the active tool, selection or sketch)
  returnTab: { project: null, family: null, annotation: null }, // the tab to go back to when the tool or selection that brought Modify forward ends
  view: { kind: "plan", viewId: null }, // plan | 3d | schedule; viewId is a stored view
  wallTypeId: null,
  locationLine: "wall_centreline", // for walls about to be drawn
  floorTypeId: null,
  roofTypeId: null,
  doorTypeId: null,
  windowTypeId: null,
  height: 2700,
  tabs: [], // open views, in tab order: { kind, viewId }
  cams: {}, // camera of each view, by kind:viewId, kept while it is not showing
  browserSel: null, // a view picked in the Project Browser (its Properties show without opening it)
  wallTop: "auto", // "auto" (the next building storey up), "unconnected", or a level id
  secRef: { on: false, view: null }, // the Section tool's Reference Other View and the view it points at
  selection: [],
  vp: null, // 3D viewport, once WebGPU is up
  sceneRevision: -1,
};

// ---- small helpers ------------------------------------------------------------
function setPrompt(text, isError = false) {
  const el = $("#prompt");
  el.textContent = text;
  el.style.color = isError ? "#c0392b" : "";
}

async function run(method, params) {
  try {
    return await Backend.call(method, params);
  } catch (e) {
    setPrompt(String(e.message || e), true);
    return null;
  }
}

// ---- commands (ribbon buttons) -------------------------------------------------
const COMMANDS = {
  // the Modify tab's Select and Properties, and its two mirror choices
  sel: () => selectTool(),
  props: () => toggleProperties(),
  "mi-pick": () => { Plan.setEditOptions({ mirrorMode: "pick" }); return startEditTool("mirror"); },
  "mi-draw": () => { Plan.setEditOptions({ mirrorMode: "draw" }); return startEditTool("mirror"); },
  wall: async () => { if (await ensurePlan("A wall")) Plan.setTool("wall"); },
  floor: () => startFloorSketch(null),
  roof: () => startRoofSketch(null),
  "det-line": () => startDetailLines(null, "line"),
  text: async () => { if (!(await ensureDetailView("Text"))) return; noteView().setTool("text"); renderOptionsBar(); renderProperties(); updateRibbonContext(); },
  "text-lead-l": () => addNoteLeader("left", false),
  "text-lead-r": () => addNoteLeader("right", false),
  "text-arc-l": () => addNoteLeader("left", true),
  "text-arc-r": () => addNoteLeader("right", true),
  "text-lead-rm": async () => {
    const n = selectedNote();
    if (!n || !n.leaders.length) { setPrompt("The text note has no leader to remove.", true); return; }
    await patchNoteLeaders(n, n.leaders.slice(0, -1));
  },
  "det-filled": () => startRegionSketch(false, null),
  "det-masking": () => startRegionSketch(true, null),
  "edit-boundary": () => {
    const t = editBoundaryTarget();
    if (!t) { setPrompt("Select an element with a boundary (a floor or a roof) to edit it.", true); return; }
    t.run();
  },
  "show-wp": () => { Plan.setShowWorkGrid(!Plan.showWorkGrid); setPrompt(Plan.showWorkGrid ? "Work plane grid shown." : "Work plane grid hidden."); },
  level: () => startLevelTool(),
  grid: () => startGridTool(),
  "v-elev": () => startElevationTool(),
  "v-section": () => startSectionTool(),
  "split-segment": () => { const s = selectedSection(); if (s) Plan.startSplitSegment(s.id); },
  door: () => startPlacement("door"),
  component: () => startComponentTool(),
  window: () => startPlacement("window"),
  wjoin: async () => { if (await ensurePlan("Wall Joins")) Plan.setTool("joins"); },
  del: () => deleteSelection(),
  mv: () => startEditTool("move"),
  cp: () => startEditTool("copy"),
  ro: () => startEditTool("rotate"),
  mi: () => startEditTool("mirror"),
  of: () => startEditTool("offset"),
  tr: () => startEditTool("trim"),
  al: () => startEditTool("align"),
  pin: () => pinSelection(true),
  unpin: () => pinSelection(false),
  "v-vg": () => openVG(),
  "m-styles": () => openObjectStyles(),
  "m-snaps": () => openSnaps(),
  "m-families": () => FamModel.list(),
  "load-family": () => FamModel.loadFile(),
  "m-tempdim": () => openTempDimSettings(),
  "dim-aligned": () => startDimTool("aligned"),
  "dim-linear": () => startDimTool("linear"),
  "dim-angular": () => startDimTool("angular"),
  "dim-radial": () => startDimTool("radial"),
  "dim-diameter": () => startDimTool("diameter"),
  "dim-arc": () => startDimTool("arclength"),
  "dim-elev": () => startSpotTool("elevation"),
  "dim-coord": () => startSpotTool("coordinate"),
  "dim-slope": () => startSpotTool("slope"),
  "edit-witness": () => {
    const id = app.selection.length === 1 && app.selection[0], V = app.view.kind === "elevation" ? Elev : Plan;
    if (id && V.dimensions().some((d) => d.id === id)) V.startWitness(id);
  },
  "m-materials": () => openMaterials(),
  "m-linestyles": () => openLineStyles(),
  "m-globals": () => openGlobalParams(),
  "m-lineweights": () => openLineWeights(),
  "m-linepatterns": () => openPatterns("line"),
  "m-fillpatterns": () => openPatterns("fill"),
  "v-template": () => createTemplate(),
  "v-dup": () => duplicateView(),
  "v-floorplan": () => newPlanDialog("floor_plan"),
  "v-ceilingplan": () => newPlanDialog("ceiling_plan"),
  "open-log": async () => {
    if (!Backend.isDesktop) { setPrompt("The log file belongs to the desktop app. A browser keeps page errors in its console.", true); return; }
    const dir = await run("open_log_folder");
    if (dir) setPrompt(`Log folder: ${dir}`);
  },
};

// New Plan (View > Plan Views): pick a view type, the levels and the scale. With "Do not duplicate existing views"
// the list holds only the levels that have no plan of this kind yet.
function newPlanDialog(kind) {
  const types = (app.info.view_types || []).filter((t) => t.view_kind === kind);
  const what = kind === "ceiling_plan" ? "Reflected Ceiling Plan" : "Floor Plan";
  openModal(`New ${what}`, (body, foot, close) => {
    const row = (label, ctl) => { const r = document.createElement("div"); r.className = "new-row"; const l = document.createElement("span"); l.textContent = label; r.append(l, ctl); return r; };
    const ts = document.createElement("select");
    types.forEach((t) => ts.add(new Option(t.name, t.id)));
    const list = document.createElement("div"); list.className = "np-levels";
    const skip = document.createElement("input"); skip.type = "checkbox"; skip.checked = true;
    const skipL = document.createElement("label"); skipL.append(skip, " Do not duplicate existing views");
    const scale = document.createElement("select");
    [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000].forEach((n) => scale.add(new Option(`1 : ${n}`, n, false, n === 100)));
    const msg = document.createElement("div"); msg.className = "modal-msg";
    body.append(row("Type", ts), document.createTextNode("Select one or more levels for new views."), list, skipL, row("Scale", scale), msg);
    const picked = new Set();
    let last = null;
    const levels = [...app.info.levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
    const has = (l) => app.info.views.some((v) => v.view_kind === kind && v.level_id === l.id);
    const draw = () => {
      list.replaceChildren();
      const shown = levels.filter((l) => !skip.checked || !has(l));
      for (const l of shown) {
        const d = document.createElement("div");
        d.textContent = l.name + (has(l) ? "  (has a plan)" : "");
        d.classList.toggle("on", picked.has(l.id)); d.classList.toggle("has", has(l));
        d.onclick = (e) => {
          if (e.shiftKey && last != null) {
            const ids = shown.map((x) => x.id), a = ids.indexOf(last), b = ids.indexOf(l.id);
            ids.slice(Math.min(a, b), Math.max(a, b) + 1).forEach((i) => picked.add(i));
          } else if (e.ctrlKey || e.metaKey) { picked.has(l.id) ? picked.delete(l.id) : picked.add(l.id); }
          else { picked.clear(); picked.add(l.id); }
          last = l.id; draw();
        };
        list.append(d);
      }
      if (!shown.length) { const n = document.createElement("div"); n.textContent = "Every level has a plan of this kind."; n.className = "has"; list.append(n); }
    };
    skip.onchange = () => { if (skip.checked) for (const l of levels) if (has(l)) picked.delete(l.id); draw(); };
    draw();
    const ok = async () => {
      if (!picked.size) { msg.textContent = "Select a level."; return; }
      try {
        const r = await Backend.call("create_plan_views", { view_type: Number(ts.value), levels: [...picked], skip_existing: skip.checked, scale: Number(scale.value) });
        close(); await refresh();
        const id = r.ids[r.ids.length - 1];
        if (id != null) openView("plan", id);
        setPrompt(`Made ${r.ids.length} ${what.toLowerCase()}${r.ids.length === 1 ? "" : "s"}.`);
      } catch (e) { msg.textContent = String(e.message || e); }
    };
    foot.append(button("Cancel", close), button("OK", ok, true));
  });
}

// Elements whose boundary can be edited, by category: the button's name says which. Ceilings and
// openings join this table when they are built on the same sketch session.
const BOUNDARY_EDITORS = {
  Floors: { label: "Edit Floor Boundary", run: (id) => startFloorSketch(id) },
  Roofs: { label: "Edit Roof Footprint", run: (id) => startRoofSketch(id) },
  "Detail Items": { label: "Edit Boundary", run: (id) => startRegionSketch(null, id) },
};
function editBoundaryTarget() {
  if (app.selection.length !== 1) return null;
  if (app.view.kind === "elevation") {
    const id = app.selection[0];
    return Elev.regions().some((r) => r.id === id) ? { label: "Edit Boundary", run: () => startRegionSketch(null, id) } : null;
  }
  if (app.view.kind !== "plan") return null;
  const info = Plan.selectionInfo();
  for (const [cat, ed] of Object.entries(BOUNDARY_EDITORS)) {
    if (info[cat] && info[cat].length === 1) { const id = info[cat][0]; return { label: ed.label, run: () => ed.run(id) }; }
  }
  return null;
}
// the view text notes are placed and picked in: a plan, or an elevation or section
const noteView = () => (app.view.kind === "elevation" ? Elev : app.view.kind === "plan" ? Plan : null);
// the text notes selected in the view's Select tool, and the one when it is the whole selection
const selectedNotes = () => { const V = noteView(); return V && V.tool === "select" ? V.textNotes().filter((n) => app.selection.includes(n.id)) : []; };
const selectedNote = () => { const s = selectedNotes(); return s.length === 1 && app.selection.length === 1 ? s[0] : null; };
// the one section selected in a plan, if that is the selection
const selectedSection = () => (app.view.kind === "plan" && app.selection.length === 1 ? Plan.sections().find((s) => s.id === app.selection[0]) : null);

// A Modify button is greyed out when this document or view has nothing for it to do. The button stays in
// place, so the tab keeps the same groups and order in every view.
function modifyEnabled(id) {
  if (!id || !commandFor(id)) return false;
  if (app.doc === "family") {
    const st = FamModel._state && FamModel._state();
    if (!st) return false;
    if (["f-select", "f-props", "f-load", "f-loadclose"].includes(id)) return true;
    // with a sketch open the tab's tools work on the sketch lines
    if (st.sk) return !!FamModel.sketchModify[id];
    if (id === "f-del") return !!st.sel || !!st.editing;
    if (st.editing) return id === "f-align";
    // every 2D view takes the Modify tools (they work in the view they are picked in); the 3D view only selects
    return st.view !== "3d";
  }
  if (app.doc === "annotation") {
    const st = AnnModel._state && AnnModel._state();
    if (!st) return false;
    if (["a-select", "a-props", "a-load", "a-loadclose"].includes(id)) return true;
    if (id === "a-del") return !!(st.sel || (AnnModel._sketch && AnnModel._sketch.active));
    return false;
  }
  if (["sel", "props", "v-vg"].includes(id)) return true;
  if (Sketch.active) return id === "sel" || id === "del" || !!SKETCH_MODIFY[id];
  if (app.view.kind === "elevation") {
    if (id === "del") return app.selection.length > 0;
    return ["mv", "cp", "al"].includes(id);
  }
  if (app.view.kind !== "plan") return id === "del" && app.selection.length > 0;
  if (id === "del") return app.selection.length > 0;
  // a Modify tool picked while another tool runs takes over from it, as it always has
  return ["mv", "cp", "ro", "mi", "mi-pick", "mi-draw", "of", "tr", "al", "pin", "unpin", "wjoin"].includes(id);
}

// Recompute the Modify tab's disabled buttons and the suffix on its tab name after a selection or tool changes.
function syncModifyButtons(c = currentContext()) {
  syncTabLabels(c);
  document.querySelectorAll("#ribbon button.cmd[data-mid]").forEach((b) => {
    const subs = b.dataset.subs ? b.dataset.subs.split("|") : null;
    const on = subs ? subs.some((id) => modifyEnabled(id)) : modifyEnabled(b.dataset.mid);
    b.disabled = !on;
    b.title = b.dataset.tip + (on ? "" : b.dataset.why || " (not available here)");
  });
}

// The tab name carries the tool while one is active: Modify | Extrusion, Modify | Place Wall, Modify | Text Notes.
function modifyTabLabel(c = currentContext()) {
  return c && c.title ? `Modify | ${c.title}` : "Modify";
}
function syncTabLabels(c) {
  document.querySelectorAll("#tabs button").forEach((b) => {
    const name = b.dataset.tab;
    const label = name === "Modify" ? modifyTabLabel(c) : name;
    if (b.textContent !== label) b.textContent = label;
  });
}

// The ribbon and its commands belong to the document showing: the project's, the family editor's, or an annotation family's
const ribbonSet = () => (app.doc === "family" ? FAM_RIBBON : app.doc === "annotation" ? ANN_RIBBON : RIBBON);
const commandFor = (id) => (app.doc === "family" ? FamModel.commands[id] : app.doc === "annotation" ? AnnModel.commands[id] : COMMANDS[id]);

// The constant Modify tab: every document has one, and whatever the active tool, selection or sketch adds
// is drawn after the tab's own panels on a tinted background. There are no contextual tabs.
function currentContext() {
  if (app.doc === "family") return FamModel.context();
  if (app.doc === "annotation") return AnnModel.context();
  if (Sketch.active) return sketchContext();
  if (app.view.kind === "plan" && Plan.tool === "wall") return wallContext();
  if (app.view.kind === "plan" && (Plan.tool === "component" || Plan.tool === "hosted")) return componentContext();
  // a selection that can do something of its own adds one panel after the tab's own (a floor or roof boundary,
  // a section's split); the tab keeps the same groups before it
  const b = editBoundaryTarget();
  if (b) return { auto: true, key: `boundary:${b.label}:${app.selection[0]}`, title: b.label.replace(/^Edit /, ""), groups: [
    { label: "Mode", items: [{ label: b.label, icon: "editBoundary", onclick: b.run }] } ] };
  const sec = selectedSection();
  if (sec && !sec.pinned) return { auto: true, key: `section:${sec.id}`, title: "Section", groups: [
    { label: "Section", items: [{ label: "Split Segment", ico: "Sl", onclick: () => runCommand("split-segment") }] } ] };
  // a selected dimension adds Edit Witness Lines, and the tab takes its name
  const DV = app.view.kind === "plan" ? Plan : app.view.kind === "elevation" ? Elev : null;
  if (DV && DV.tool === "select" && app.selection.length === 1) {
    const dim = DV.dimensions().find((d) => d.id === app.selection[0] && !d.round);
    if (dim) return { auto: true, key: `dim:${dim.id}`, title: "Dimensions", groups: [
      { label: "Label Dimension", items: [{ label: dim.segments.some((s) => s.param != null) ? "Change Label" : "Label", icon: "label", big: true, onclick: () => labelDimension(dim) }] },
      { label: "Witness Lines", items: [{ label: "Edit Witness Lines", icon: "editWitness", onclick: () => runCommand("edit-witness") }] } ] };
  }
  return textContext();
}

// Label Dimension: one segment takes a global parameter here; a chain is labelled segment by segment in Properties
async function labelDimension(dim) {
  if (dim.segments.length > 1) { setPrompt("Label each segment in Properties (Label, segment 1, 2 ...).", false); return; }
  const g = await pickGlobal("Label with Global Parameter", "length", dim.segments[0].param ?? null, true);
  if (g === undefined) return;
  if (await run("label_dimension", { id: dim.id, segment: 0, param: g })) { await refresh(); renderProperties(); }
}

// A context appears, changes or goes. One appearing brings the Modify tab forward and remembers the tab the person
// was on; when it goes, that tab comes back (unless the person picked a tab themselves in between). While the Modify
// tab shows, its added panels are drawn again on every call, so they follow the selection and the tool's own state
// (a dimension's lock, Axis Line, the line style of the selected lines).
let ribbonBusy = false;
function updateRibbonContext(activate) {
  // drawing a tab can reach here again (a selection kept in step); one pass is enough
  if (ribbonBusy) return;
  ribbonBusy = true;
  try {
    const c = currentContext();
    const sig = c ? `${app.doc}:${c.key || c.title || "context"}` : "";
    const fresh = sig !== app.ctxSig;
    app.ctxSig = sig;
    const doc = app.doc, showing = app.ribbonTab[doc];
    if (c && showing !== "Modify" && (activate || (c.auto && fresh))) { app.returnTab[doc] = showing; selectTab("Modify", c); return; }
    if (!c && fresh && showing === "Modify" && app.returnTab[doc]) { const back = app.returnTab[doc]; app.returnTab[doc] = null; selectTab(back, c); return; }
    if (showing === "Modify") drawTab("Modify", c);
    syncModifyButtons(c);
  } finally {
    ribbonBusy = false;
  }
}
// The family and annotation editors call this after a selection or a tool changes.
const renderFamContext = (activate) => updateRibbonContext(activate);

function renderTabs() {
  const tabs = $("#tabs");
  tabs.replaceChildren();
  Object.keys(ribbonSet()).forEach((name) => {
    const b = document.createElement("button");
    b.dataset.tab = name;
    b.textContent = name === "Modify" ? modifyTabLabel() : name;
    // a tab picked by hand stays: nothing jumps back to an earlier one when the tool ends
    b.onclick = () => { app.returnTab[app.doc] = null; selectTab(name); };
    tabs.appendChild(b);
  });
  selectTab(app.ribbonTab[app.doc]);
}

// One panel of a ribbon tab: base items carry a command id, context items carry their own handlers.
// Tool panels are icon-only (the name is the tooltip), and everything a tool adds goes after the tab's own
// panels, never before them.
function groupNode(g, isContext, isModify) {
  const grp = el("div", { className: "group" + (g.label === "Family Editor" ? " fam-editor" : "") });
  const items = el("div", { className: "items" });
  let stack = null;
  for (const it of g.items) {
    if (it.node) {
      if (it.big) { stack = null; items.append(it.node); }
      else {
        if (!stack || stack.children.length >= 3 || it.brk) { stack = el("div", { className: "stack" }); items.append(stack); }
        stack.append(it.node);
      }
      continue;
    }
    // `brk` starts a new column of small buttons; `iconOnly` shows the icon with the name as its tooltip
    const extra = [it.iconOnly ? "icon-only" : "", it.cls || ""].filter(Boolean).join(" ");
    if (isContext) {
      // a tool's buttons need no text: the icon and its tooltip carry the name. Big ones (Finish, Cancel, a mode) keep
      // their large icon; the rest stack three to a column.
      const node = sketchButton(it, !it.big, it.onclick, it.on, extra + " icon-only");
      if (it.big) { stack = null; items.append(node); continue; }
      if (!stack || stack.children.length >= 3 || it.brk) { stack = el("div", { className: "stack" }); items.append(stack); }
      stack.append(node);
    } else {
      if (it.big) { stack = null; items.append(cmdButton(it, false)); continue; }
      if (!stack || stack.children.length >= 3) { stack = el("div", { className: "stack" }); items.append(stack); }
      stack.append(cmdButton(it, true));
    }
  }
  grp.append(items, el("div", { className: "label", textContent: g.label }));
  return grp;
}

// A family document keeps its Family Editor group at the end of every tab, after whatever the tab adds.
const docTailGroup = () => (app.doc === "family" ? FAM_EDITOR_GROUP : app.doc === "annotation" ? ANN_EDITOR_GROUP : null);

// The tab's own panels, then whatever the active tool adds (tinted), then the family document's own group.
function drawTab(name, ctx) {
  const ribbon = $("#ribbon");
  ribbon.replaceChildren();
  for (const g of ribbonSet()[name] || []) ribbon.append(groupNode(g, false, name === "Modify"));
  const c = name !== "Modify" ? null : ctx !== undefined ? ctx : currentContext();
  if (c && c.groups && c.groups.length) {
    const box = el("div", { className: "ctx" });
    for (const g of c.groups) box.append(groupNode(g, true));
    ribbon.append(box);
  }
  const tail = docTailGroup();
  if (tail) ribbon.append(groupNode(tail, false));
  // the family documents mark their running tool and toggles on the new buttons
  if (app.doc === "family") FamModel.syncRibbon(); else if (app.doc === "annotation") AnnModel.syncRibbon();
}

// Switch between the project and the family open in the family editor (each keeps its own ribbon tab)
function setDoc(kind) {
  if (app.doc === kind) return;
  app.doc = kind;
  // model and annotation families share the family document shell; an annotation family has a canvas of its own
  $("#app").classList.toggle("fam", kind !== "project");
  $("#app").classList.toggle("ann", kind === "annotation");
  renderTabs();
  if (kind === "project") { updateTitle(); setPrompt("Ready."); }
  renderDocTabs();
}
function renderDocTabs() {
  const bar = $("#doc-tabs");
  const fam = FamModel.docInfo(), ann = AnnModel.docInfo();
  bar.hidden = !fam && !ann;
  if (!fam && !ann) { bar.replaceChildren(); return; }
  const name = app.info && app.info.path ? app.info.path.split(/[\\/]/).pop() : "Untitled";
  const mk = (label, active, onclick, onclose) => {
    const t = el("div", { className: "dtab" + (active ? " active" : ""), title: label }, [el("span", { textContent: label })]);
    t.onclick = onclick;
    if (onclose) { const x = el("span", { className: "x", textContent: "\u00d7", title: "Close" }); x.onclick = (e) => { e.stopPropagation(); onclose(); }; t.append(x); }
    return t;
  };
  bar.replaceChildren(
    mk(name, app.doc === "project", () => setDoc("project")),
    ...(fam ? [mk(`Family: ${fam.name}${fam.dirty ? " *" : ""}`, app.doc === "family", () => FamModel.activate(), () => FamModel.close())] : []),
    ...(ann ? [mk(`Family: ${ann.name}${ann.dirty ? " *" : ""}`, app.doc === "annotation", () => AnnModel.activate(), () => AnnModel.close())] : []),
  );
}

// While a sketch is open nothing else is editable: the ribbon's Modify tools act on the sketch lines instead,
// and every other command waits for Finish or Cancel.
const SKETCH_MODIFY = { sel: "select", mv: "move", cp: "copy", ro: "rotate", mi: "mirror", "mi-pick": "mirror", "mi-draw": "mirror", of: "offset", tr: "trim_corner", al: "align" };
function runCommand(id, e) {
  if (Sketch.active) {
    if (SKETCH_MODIFY[id]) {
      if (id === "mi-pick" || id === "mi-draw") Sketch.setOpts({ mirrorMode: id === "mi-pick" ? "pick" : "draw" });
      Sketch.setTool(SKETCH_MODIFY[id]); renderSketchRibbon(); return;
    }
    // showing or hiding the Properties palette leaves the sketch as it is
    if (id === "props") { toggleProperties(); return; }
    // Delete takes the selected lines out at once, or with none selected waits for a click on one
    if (id === "del") { if (Sketch.selectedCount) Sketch.key({ key: "Delete", preventDefault() {} }); else { Sketch.setTool("delete"); renderSketchRibbon(); } return; }
    // Detail Line has no Finish: another command leaves it (its lines are already in the model)
    if (Sketch.kind === "detail") {
      if (id === "det-line") { Sketch.setTool("line"); return; }
      leaveDetailLines().then(() => { const f = commandFor(id); if (f) f(e); });
      return;
    }
    setPrompt("Finish or cancel the sketch first.", true);
    return;
  }
  const f = commandFor(id);
  return f ? f(e) : undefined;
}

// A ribbon item's command in the document showing: a canonical Modify item names one per document.
const ribbonItemId = (item) => (item.ids ? item.ids[app.doc] || null : item.id);
const commandInDoc = (doc, id) => (doc === "project" ? COMMANDS[id] : doc === "family" ? FamModel.commands[id] : AnnModel.commands[id]);
// Whether the button is built anywhere ("not available here") or has no command yet at all ("not built yet").
const modifyUnavailable = (item) => (item.ids && Object.entries(item.ids).some(([doc, id]) => id && commandInDoc(doc, id)) ? " (not available here)" : " (not built yet)");

// A ribbon drop-down: the button opens a short list of commands under it
function menuButton(item, small) {
  const b = document.createElement("button");
  b.className = "cmd" + (small ? " small" : "") + (item.iconOnly ? " icon-only" : "");
  const subs = (item.menu || []).map(ribbonItemId);
  const first = subs.find(Boolean) || null;
  const canonical = item.ids !== undefined;
  const openable = canonical ? subs.some((id) => modifyEnabled(id)) : true;
  const why = canonical ? modifyUnavailable(item) : "";
  b.dataset.id = first || "";
  if (canonical) { b.dataset.mid = first || ""; b.dataset.subs = subs.map((id) => id || "").join("|"); b.dataset.tip = item.label; b.dataset.why = why; }
  b.disabled = !openable;
  b.title = item.label + (openable ? "" : why);
  const ico = document.createElement("span");
  ico.className = "ico";
  setIcon(ico, item);
  const lab = document.createElement("span");
  lab.textContent = item.label + " \u25BE";
  b.append(ico, lab);
  // an icon-only menu keeps a caret on its corner, since its name is only in the tooltip
  if (item.iconOnly) { const c = document.createElement("span"); c.className = "menu-caret"; c.textContent = "\u25BE"; b.append(c); }
  b.onclick = (e) => {
    if (b.disabled) return;
    e.stopPropagation();
    const open = document.querySelector(".cmd-menu");
    if (open) { const same = open.dataset.for === (b.dataset.id || item.label); open.remove(); if (same) return; }
    const m = document.createElement("div");
    m.className = "cmd-menu";
    m.dataset.for = b.dataset.id || item.label;
    item.menu.forEach((sub, i) => {
      const id = subs[i];
      const it = document.createElement("button");
      it.textContent = sub.label;
      it.disabled = canonical ? !modifyEnabled(id) : !commandFor(id);
      if (!id) it.title = "Not available here";
      it.onclick = (ev) => { ev.stopPropagation(); m.remove(); runCommand(id, ev); };
      m.append(it);
    });
    const r = b.getBoundingClientRect();
    m.style.left = `${r.left}px`;
    m.style.top = `${r.bottom}px`;
    document.body.append(m);
    const away = () => { m.remove(); document.removeEventListener("click", away, true); };
    setTimeout(() => document.addEventListener("click", away, true), 0);
  };
  return b;
}

function cmdButton(item, small) {
  if (item.menu) return menuButton(item, small);
  const b = document.createElement("button");
  b.className = "cmd" + (small ? " small" : "") + (item.iconOnly ? " icon-only" : "");
  const canonical = item.ids !== undefined;
  const id = ribbonItemId(item);
  const on = canonical ? modifyEnabled(id) : !!commandFor(id);
  const why = canonical ? modifyUnavailable(item) : " (not built yet)";
  b.disabled = !on;
  b.title = item.label + (on ? "" : why);
  b.dataset.id = id || "";
  if (canonical) { b.dataset.mid = id || ""; b.dataset.tip = item.label; b.dataset.why = why; }
  const ico = document.createElement("span");
  ico.className = "ico";
  setIcon(ico, item);
  const lab = document.createElement("span");
  lab.textContent = item.label;
  b.append(ico, lab);
  // attached even while disabled: a selection can enable the button later, and disabled blocks the click anyway
  if (id) b.onclick = (e) => runCommand(id, e);
  return b;
}

function selectTab(name, ctx) {
  const set = ribbonSet();
  if (!set[name]) name = Object.keys(set)[0];
  app.ribbonTab[app.doc] = name;
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("active", b.dataset.tab === name));
  const c = ctx !== undefined ? ctx : currentContext();
  drawTab(name, c);
  syncModifyButtons(c);
}

// A ribbon wider than the window fits itself in steps (style.css #ribbon.fit-1 to fit-3) rather than scrolling: it
// is checked whenever its buttons change and whenever the window changes size.
function fitRibbon() {
  const r = $("#ribbon");
  r.classList.remove("fit-1", "fit-2", "fit-3");
  for (const level of ["fit-1", "fit-2", "fit-3"]) {
    if (r.scrollWidth <= r.clientWidth + 1) return;
    r.classList.add(level);
  }
}
{
  let pending = false;
  const soon = () => { if (pending) return; pending = true; requestAnimationFrame(() => { pending = false; fitRibbon(); }); };
  new MutationObserver(soon).observe($("#ribbon"), { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden"] });
  new ResizeObserver(soon).observe($("#ribbon"));
}

// ---- project browser -----------------------------------------------------------
const viewById = (id) => app.info.views.find((v) => v.id === id);
const curView = () => viewById(app.view.viewId);
// Levels above the one being drawn on, lowest first. The default wall top is the lowest building storey among them.
function levelsAbove(levelId) {
  const cur = app.info.levels.find((l) => l.id === levelId);
  if (!cur) return [];
  return app.info.levels.filter((l) => l.elevation_mm > cur.elevation_mm + 1e-6).sort((a, b) => a.elevation_mm - b.elevation_mm);
}
// What a new wall gets for its top: { top_level_id, height_mm }. "Unconnected" and a missing level above both use the typed height.
function wallTopParams(levelId) {
  const above = levelsAbove(levelId);
  let top = null;
  if (app.wallTop === "auto") top = above.find((l) => l.building_story) || null;
  else if (app.wallTop !== "unconnected") top = above.find((l) => l.id === app.wallTop) || null;
  return top ? { top_level_id: top.id, top_offset_mm: 0, height_mm: top.elevation_mm - app.info.levels.find((l) => l.id === levelId).elevation_mm } : { height_mm: app.height };
}
// section views are drawn by the elevation viewer
const viewerOf = (k) => (k === "three_d" ? "3d" : k === "elevation" || k === "section" ? "elevation" : "plan");
const curLevelId = () => (curView() && curView().level_id) || app.info.levels[0].id;

function renderTree() {
  const tree = $("#tree");
  const open = new Set([...tree.querySelectorAll("li.node.open > span")].map((s) => s.textContent));
  if (!tree.children.length) ["Views (all)", "Floor Plans", "3D Views", "Elevations", "Sections", "Schedules / Quantities"].forEach((n) => open.add(n));
  tree.replaceChildren();
  const v = app.view;
  const leaf = (name, active, onopen, viewId) => ({ name, active, onopen, viewId });
  const ofKind = (k) => app.info.views.filter((x) => x.view_kind === k);
  const viewLeaf = (x) => leaf(x.name, v.kind !== "schedule" && v.viewId === x.id, () => openView(viewerOf(x.view_kind), x.id), x.id);
  const nodes = [
    { name: "Views (all)", children: [
      { name: "Floor Plans", children: ofKind("floor_plan").map(viewLeaf) },
      { name: "Ceiling Plans", children: ofKind("ceiling_plan").map(viewLeaf) },
      { name: "3D Views", children: ofKind("three_d").map(viewLeaf) },
      { name: "Elevations", children: ofKind("elevation").map(viewLeaf) },
      { name: "Sections", children: ofKind("section").map(viewLeaf) },
      { name: "Drafting Views", children: [] },
      { name: "Legends", children: [] },
      { name: "Schedules / Quantities", children: [leaf("Wall and Floor Quantities", v.kind === "schedule", () => openView("schedule"))] },
      { name: "Sheets", children: [] },
    ] },
    { name: "Families", children: (() => {
      // every category that has a family, in name order, with the system families among them
      const byCat = new Map();
      for (const f of app.info.families) {
        if (!byCat.has(f.category)) byCat.set(f.category, []);
        const types = app.info.family_types.filter((t) => t.family_id === f.id).map((t) => ({ ...leaf(t.name, false, () => openTypeProperties(t.id)), menu: [{ label: "Type Properties...", run: () => openTypeProperties(t.id) }] }));
        byCat.get(f.category).push({ name: f.name, children: types, menu: familyMenu(f) });
      }
      // level heads, elevation marks and section marks are annotation families: Edit opens them as documents
      if (app.info.annotation_families.length) byCat.set("Annotation Symbols", app.info.annotation_families.map((f) => ({ name: f.name, title: f.category, onopen: () => AnnModel.open(f.id), menu: annotationMenu(f) })));
      byCat.set("Floors", [{ name: "Floor", children: app.info.floor_types.map((t) => leaf(t.name)) }]);
      byCat.set("Roofs", [{ name: "Basic Roof", children: app.info.roof_types.map((t) => leaf(t.name)) }]);
      byCat.set("Walls", [{ name: "Basic Wall", children: app.info.wall_types.map((t) => leaf(t.name)) }]);
      return [...byCat.entries()].sort((a, b) => a[0].localeCompare(b[0])).map(([cat, fams]) => ({ name: cat, children: fams.sort((a, b) => a.name.localeCompare(b.name)) }));
    })() },
    { name: "View Templates", children: app.info.view_templates.map((t) => leaf(t.name)) },
    { name: "Groups", children: [] },
    { name: "Links", children: [] },
  ];
  const build = (list, parent) => {
    for (const n of list) {
      const li = document.createElement("li");
      const span = document.createElement("span");
      span.textContent = n.name;
      if (n.title) span.title = n.title;
      if (n.menu) span.oncontextmenu = (e) => { e.preventDefault(); showMenu(e.clientX, e.clientY, n.menu); };
      li.appendChild(span);
      if (n.children) {
        if (n.children.length) {
          li.classList.add("node");
          if (open.has(n.name)) li.classList.add("open");
          span.onclick = () => li.classList.toggle("open");
          const ul = document.createElement("ul");
          build(n.children, ul);
          li.appendChild(ul);
        }
      } else {
        if (n.active) li.classList.add("sel");
        if (n.viewId != null && n.viewId === app.browserSel) li.classList.add("picked");
        if (n.onopen) span.ondblclick = n.onopen;
        // one click picks the view and shows its Properties without opening it
        if (n.viewId != null) span.onclick = () => { app.browserSel = n.viewId; app.selection = []; Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]); renderTree(); renderProperties(); updateRibbonContext(); };
      }
      parent.appendChild(li);
    }
  };
  build(nodes, tree);
}

// Right-click menus of the Project Browser's families
function familyMenu(f) {
  return [
    { label: "Edit", run: () => FamModel.open(f.id), disabled: !f.has_body, title: f.has_body ? "" : "Drawn by code, not editable" },
    { label: "Duplicate...", run: () => duplicateTypePrompt(f.id, async () => { await refresh(); }, "duplicate_family") },
    { label: "Save as File...", run: () => FamModel.exportFamily(f.id, f.name) },
  ];
}
function annotationMenu(f) {
  const open = () => AnnModel.isOpen(f.id) && (setPrompt(`${f.name} is open in the family editor; close it there first.`, true), true);
  return [
    { label: "Edit", run: () => AnnModel.open(f.id) },
    { label: "Duplicate...", run: () => duplicateTypePrompt(f.id, async () => { await refresh(); }, "duplicate_annotation_family") },
    { label: "Rename...", run: () => { if (!open()) renameAnnotationFamily(f); } },
    { label: "Delete", disabled: f.in_use, title: f.in_use ? "In use by a level type, an elevation mark or a section" : "", run: async () => {
      if (open()) return;
      if (await run("delete_annotation_family", { id: f.id })) { await refresh(); setPrompt(`Deleted ${f.name}.`); }
    } },
  ];
}
function renameAnnotationFamily(f) {
  openModal("Rename", (body, foot, close) => {
    const inp = el("input", { type: "text", value: f.name });
    inp.style.width = "100%";
    const msg = el("div", { className: "modal-msg" });
    body.append(inp, msg);
    const ok = async () => {
      try {
        const fam = await Backend.call("annotation_family", { id: f.id });
        fam.name = inp.value.trim();
        await Backend.call("set_annotation_family", { id: f.id, family: fam });
      } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
      await refresh();
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") ok(); };
    foot.append(button("OK", ok, true), button("Cancel", close));
    inp.focus(); inp.select();
  });
}

// A small menu at the cursor; it goes on a choice, a click elsewhere or Escape
function showMenu(x, y, items) {
  document.querySelectorAll(".ctx-menu").forEach((m) => m.remove());
  const menu = el("div", { className: "ctx-menu" });
  const shut = () => { menu.remove(); document.removeEventListener("mousedown", away, true); document.removeEventListener("keydown", esc, true); };
  const away = (e) => { if (!menu.contains(e.target)) shut(); };
  const esc = (e) => { if (e.key === "Escape") { e.stopPropagation(); shut(); } };
  for (const it of items) {
    const b = el("button", { textContent: it.label, disabled: !!it.disabled, title: it.title || "" });
    b.onclick = () => { shut(); it.run(); };
    menu.append(b);
  }
  document.body.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(x, innerWidth - r.width - 4)}px`;
  menu.style.top = `${Math.min(y, innerHeight - r.height - 4)}px`;
  document.addEventListener("mousedown", away, true);
  document.addEventListener("keydown", esc, true);
}

// ---- views ---------------------------------------------------------------------
function currentLevel() {
  return app.info.levels.find((l) => l.id === curLevelId()) || app.info.levels[0];
}

async function openView(kind, viewId) {
  // Detail Line has no Finish: its lines are in the model already, so another view just leaves it
  if (Sketch.active && Sketch.kind === "detail") await leaveDetailLines();
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return; }
  // only re-frame the camera when the view actually changes
  const firstOf = (k) => (app.info.views.find((x) => x.view_kind === k) || {}).id;
  const target = kind === "schedule" ? app.view.viewId : (viewId ?? (kind === "plan" && curView() && curView().level_id ? app.view.viewId : firstOf(kind === "3d" ? "three_d" : kind === "elevation" ? "elevation" : "floor_plan")));
  const changed = app.view.kind !== kind || target !== app.view.viewId;
  if (changed) saveCamera();
  app.view = { kind, viewId: target };
  if (kind === "plan") app.lastPlanId = target;
  if (kind !== "plan" && Plan.tool !== "select") Plan.setTool("select");
  if (kind !== "elevation" && Elev.tool !== "select") Elev.setTool("select");
  $("#viewport-wrap").className = "v-" + kind;
  $("#stress").hidden = kind !== "3d";
  app.browserSel = null;
  if (changed) { app.selection = []; Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]); }
  await refreshView(changed);
  if (changed) restoreCamera();
  ensureTab();
  renderOptionsBar();
  await renderProperties();
  renderTree();
  updateTitle();
  syncViewBar();
}

// ---- tabs of the open views, and the camera each view keeps --------------------------
const viewKey = (v) => (v.kind === "schedule" ? "schedule" : `${v.kind}:${v.viewId}`);
function saveCamera() {
  const k = app.view.kind;
  const c = k === "plan" ? Plan.getCamera() : k === "elevation" ? Elev.getCamera() : k === "3d" ? app.vp?.getCamera() : null;
  if (c) app.cams[viewKey(app.view)] = c;
}
function restoreCamera() {
  const c = app.cams[viewKey(app.view)], k = app.view.kind;
  if (!c) return;
  if (k === "plan") Plan.setCamera(c.x, c.y, c.s);
  else if (k === "elevation") Elev.setCameraState(c);
  else if (k === "3d") app.vp?.setCameraState(c);
}
function ensureTab() {
  const key = viewKey(app.view);
  if (!app.tabs.some((t) => viewKey(t) === key)) app.tabs.push({ kind: app.view.kind, viewId: app.view.viewId });
  renderViewTabs();
}
function tabName(t) {
  if (t.kind === "schedule") return "Wall and Floor Quantities";
  const v = viewById(t.viewId);
  return v ? v.name : "";
}
function renderViewTabs() {
  // views that no longer exist lose their tab
  app.tabs = app.tabs.filter((t) => t.kind === "schedule" || viewById(t.viewId));
  const bar = $("#view-tabs");
  if (!bar) return;
  const here = viewKey(app.view);
  bar.replaceChildren(...app.tabs.map((t) => {
    const x = el("span", { className: "x", textContent: "\u00d7", title: "Close (middle-click also closes)" });
    const tab = el("div", { className: "vtab" + (viewKey(t) === here ? " active" : "") + (app.tabs.length === 1 ? " only" : ""), title: tabName(t) }, [el("span", { textContent: tabName(t) }), x]);
    tab.onclick = () => { if (viewKey(t) !== here) openView(t.kind, t.viewId); };
    tab.onauxclick = (e) => { if (e.button === 1) { e.preventDefault(); closeTab(t); } };
    x.onclick = (e) => { e.stopPropagation(); closeTab(t); };
    return tab;
  }));
}
async function closeTab(t) {
  if (app.tabs.length <= 1) return;
  const i = app.tabs.findIndex((x) => viewKey(x) === viewKey(t));
  if (i < 0) return;
  const wasActive = viewKey(t) === viewKey(app.view);
  app.tabs.splice(i, 1);
  delete app.cams[viewKey(t)];
  if (wasActive) { const n = app.tabs[Math.min(i, app.tabs.length - 1)]; await openView(n.kind, n.viewId); }
  else renderViewTabs();
}

// Tools that only work in a plan view ask where to go instead of switching on their own (the Go To View).
// Returns true when a plan is showing, false when the person cancelled and the view stays as it was.
function planViews() {
  return app.info.views.filter((x) => x.view_kind === "floor_plan" || x.view_kind === "ceiling_plan" || x.view_kind === "structural_plan");
}
async function ensurePlan(what) {
  if (app.view.kind === "plan") return true;
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return false; }
  const plans = planViews();
  if (!plans.length) { setPrompt(`${what} needs a plan view, and this project has none.`, true); return false; }
  const last = plans.find((x) => x.id === app.lastPlanId) || plans.find((x) => x.view_kind === "floor_plan") || plans[0];
  const picked = await new Promise((resolve) => {
    openModal("Go To View", (body, foot, close) => {
      const msg = document.createElement("div");
      msg.className = "modal-msg";
      msg.textContent = `${what} is placed in a plan view. Open one:`;
      const sel = document.createElement("select");
      sel.size = Math.min(8, plans.length + 1);
      sel.style.width = "100%";
      for (const x of plans) { const o = document.createElement("option"); o.value = x.id; o.textContent = x.name; o.selected = x.id === last.id; sel.append(o); }
      body.append(msg, sel);
      let done = false;
      const finish = (v) => { if (done) return; done = true; close(); resolve(v); };
      sel.ondblclick = () => finish(Number(sel.value));
      foot.append(button("Open View", () => finish(Number(sel.value)), true), button("Cancel", () => finish(null)));
      const back = body.closest(".modal-back");
      back.addEventListener("keydown", (e) => { if (e.key === "Escape") finish(null); });
      new MutationObserver(() => { if (!back.isConnected) finish(null); }).observe(document.body, { childList: true });
      sel.focus();
    });
  });
  if (picked === null) { setPrompt("Cancelled. The view stays as it is."); return false; }
  await openView("plan", picked);
  return true;
}

async function refreshView(force) {
  const k = app.view.kind;
  if (k === "plan") {
    const data = await run("plan", { view_id: app.view.viewId });
    if (data) { Plan.setData(data); if (force) Plan.fit(); await refreshTempDims(); }
  } else if (k === "elevation") {
    const data = await run("elevation", { view_id: app.view.viewId });
    if (data) Elev.setData(data, force);
  } else if (k === "3d") {
    if (!app.vp) return;
    if (force || app.sceneRevision !== app.info.revision) {
      const scene = await run("scene", { view_id: app.view.viewId });
      if (!scene) return;
      app.sceneRevision = scene.revision;
      app.sceneStyle = scene.visual_style || "shaded";
      app.vp.setMeshes(scene.meshes, app.selection);
      if (force) app.vp.fit();
    }
    app.vp.setStyle(app.sceneStyle || "shaded");
    app.vp.poke();
  } else if (k === "schedule") {
    await renderSchedule();
  }
}

const rates = { wall: 180, floor: 220 };

async function renderSchedule() {
  const r = await run("schedule", { wall_rate: rates.wall, floor_rate: rates.floor });
  if (!r) return;
  const table = (t, rateKey, rateLabel) => {
    const head = t.columns.map((c, i) => `<th class="${t.calculated[i] ? "calc" : ""}">${c}</th>`).join("");
    const body = t.rows.map((row) => `<tr>${row.map((v, i) => `<td class="${t.calculated[i] ? "calc" : ""} ${/^[\d.-]+$/.test(v) ? "num" : ""}">${v}</td>`).join("")}</tr>`).join("");
    const foot = `<tr class="total">${t.totals.map((v, i) => `<td class="${/^[\d.-]+$/.test(v) ? "num" : ""}">${i === 0 ? "Total" : v}</td>`).join("")}</tr>`;
    return `<h3>${t.name} <label class="rate">${rateLabel} <input type="number" min="0" step="1" data-rate="${rateKey}" value="${rates[rateKey]}"></label></h3>
      <table class="sched"><thead><tr>${head}</tr></thead><tbody>${body || `<tr><td colspan="${t.columns.length}" class="empty">No ${t.name.toLowerCase()} yet</td></tr>`}</tbody><tfoot>${foot}</tfoot></table>`;
  };
  const plain = (t) => `<h3>${t.name} <span class="rate">${t.count} placed</span></h3>
    <table class="sched"><thead><tr>${t.columns.map((c) => `<th>${c}</th>`).join("")}</tr></thead><tbody>${t.rows.map((row) => `<tr>${row.map((v, i) => `<td class="${i >= 3 && i <= 5 ? "num" : ""}">${v}</td>`).join("")}</tr>`).join("") || `<tr><td colspan="${t.columns.length}" class="empty">No ${t.name.toLowerCase()} yet</td></tr>`}</tbody></table>`;
  const host = $("#schedule-view");
  const focused = document.activeElement && document.activeElement.dataset ? document.activeElement.dataset.rate : null;
  host.innerHTML = `<div class="sched-wrap">
    <h2>Wall and Floor Quantities</h2>
    <p class="note">Cost is a calculated column: <code>Area x Rate</code>. Totals feed the project total below, using the same formula engine that will drive families and project parameters.</p>
    ${table(r.walls, "wall", "Wall rate $/m&sup2;")}
    ${table(r.floors, "floor", "Floor rate $/m&sup2;")}
    ${plain(r.doors)}
    ${plain(r.windows)}
    <div class="grand">Total cost (formula: <code>[Walls.Cost.Sum] + [Floors.Cost.Sum]</code>): <b id="total-cost">${r.total_cost}</b></div>
  </div>`;
  host.querySelectorAll("input[data-rate]").forEach((inp) => {
    inp.oninput = () => { rates[inp.dataset.rate] = parseFloat(inp.value) || 0; renderSchedule(); };
    if (focused === inp.dataset.rate) inp.focus();
  });
}

// ---- document refresh ---------------------------------------------------------
function updateTitle() {
  if (app.doc !== "project") return;
  const name = app.info.path ? app.info.path.split(/[\\/]/).pop() : "Untitled";
  const v = curView();
  const vn = app.view.kind === "plan" && v ? `${v.view_kind === "ceiling_plan" ? "Reflected Ceiling Plan" : "Floor Plan"}: ${v.name}` : app.view.kind === "elevation" && v ? `${v.view_kind === "section" ? "Section" : "Elevation"}: ${v.name}` : app.view.kind === "3d" && v ? `3D View: ${v.name}` : "Schedule: Wall and Floor Quantities";
  $("#doc-title").textContent = `${name}${app.info.dirty ? " *" : ""} - ${vn}`;
  document.querySelector("#browser .panel-title").textContent = `Project Browser - ${name}`;
  updateUndoButtons();
}

async function refresh(force = false) {
  const info = await run("doc_info", {});
  if (!info) return;
  app.info = info;
  const pick = (cur, cat, label) => (cur && info.family_types.some((t) => t.id === cur) ? cur : (info.family_types.find((t) => t.label === label) || info.family_types.find((t) => t.category === cat) || {}).id || null);
  app.doorTypeId = pick(app.doorTypeId, "Doors", "Single-Flush: 820 x 2040");
  app.windowTypeId = pick(app.windowTypeId, "Windows", "Fixed: 1200 x 1200");
  if (!app.wallTypeId || !info.wall_types.some((t) => t.id === app.wallTypeId)) app.wallTypeId = (info.wall_types.find((t) => t.name === "Brick Veneer") || info.wall_types[0]).id;
  if (!app.floorTypeId || !info.floor_types.some((t) => t.id === app.floorTypeId)) app.floorTypeId = info.floor_types[0].id;
  if (!app.roofTypeId || !info.roof_types.some((t) => t.id === app.roofTypeId)) app.roofTypeId = info.roof_types[0].id;
  if (app.view.kind !== "schedule" && !viewById(app.view.viewId)) {
    const first = info.views.find((x) => x.view_kind === "floor_plan") || info.views[0];
    app.view = { kind: viewerOf(first.view_kind), viewId: first.id };
    $("#viewport-wrap").className = "v-" + app.view.kind;
    force = true;
  }
  updatePlanExtents();
  await refreshView(force);
  ensureTab();
  await renderProperties();
  renderTree();
  updateTitle();
  syncViewBar();
  if (Plan.tool === "joins") renderOptionsBar();
}

function updatePlanExtents() {
  const t = app.info.wall_types.find((x) => x.id === app.wallTypeId);
  const e = t && t.extents[app.locationLine];
  const [l, r] = e ? e : [100, 100];
  // a flipped wall puts the exterior side on the right
  if (app.flipped) Plan.setExtents(r, l); else Plan.setExtents(l, r);
}

// view control bar: scale, detail level and style edit the current view
function syncViewBar() {
  const bar = $("#viewbar");
  const v = curView();
  const sc = $("#vb-scale"), dl = $("#vb-detail"), st = $("#vb-style");
  if (!sc.options.length) {
    for (const n of [1, 2, 5, 10, 20, 25, 50, 75, 100, 125, 150, 200, 250, 300, 400, 500, 1000, 2000, 5000]) sc.add(new Option(`1 : ${n}`, n));
    for (const [v2, l] of [["coarse", "Coarse"], ["medium", "Medium"], ["fine", "Fine"]]) dl.add(new Option(l, v2));
    for (const [v2, l] of [["wireframe", "Wireframe"], ["hidden_line", "Hidden Line"], ["shaded", "Shaded"], ["consistent_colors", "Consistent Colors"], ["realistic", "Realistic"]]) st.add(new Option(l, v2));
    const send = (path, value) => async () => {
      const cur = curView(); if (!cur) return;
      if (await run("patch_view", { id: cur.id, patch: nestPatch(path, value()) })) await refresh();
    };
    sc.onchange = send("settings", () => ({ scale: Number(sc.value), custom_scale: false }));
    dl.onchange = send("settings", () => ({ detail_level: dl.value }));
    st.onchange = send("settings", () => ({ visual_style: st.value }));
    const tl = $("#vb-thin");
    tl.onclick = () => { Plan.setThinLines(!Plan.thinLines); tl.classList.toggle("on", Plan.thinLines); };
  }
  bar.classList.toggle("no-view", app.view.kind === "schedule" || !v);
  const on = app.view.kind !== "schedule" && !!v;
  [sc, dl, st].forEach((e) => (e.disabled = !on));
  if (app.settings) {
    sc.value = app.settings.scale; dl.value = app.settings.detail_level; st.value = app.settings.visual_style;
  }
}

const nestPatch = (path, value) => path.split(".").reverse().reduce((v, k) => ({ [k]: v }), value);

// ---- plan callbacks -------------------------------------------------------------
Plan.callbacks.addWall = async (a, b) => {
  const r = await run("add_wall", { type_id: app.wallTypeId, level_id: curLevelId(), start: a, end: b, ...wallTopParams(curLevelId()), location_line: app.locationLine, flipped: !!app.flipped });
  if (r) await refresh();
};
Plan.callbacks.moveWallEnd = async (id, end, point) => {
  const r = await run("move_wall_end", { id, end, point });
  if (r) { await refresh(); renderProperties(); }
};
Plan.callbacks.moveWallEnds = async (ends, point) => {
  const r = await run("move_wall_ends", { ends, point });
  if (r) { await refresh(); renderProperties(); }
};
Plan.callbacks.reshapeWall = async (id, k, point) => {
  const r = await run("reshape_wall", { id, k, point });
  if (r) { await refresh(); renderProperties(); }
};
Plan.callbacks.reshapePreview = async (id, k, point) => {
  try { const r = await Backend.call("reshape_wall", { id, k, point, preview: true }); return r.poly; } catch { return null; }
};
Plan.callbacks.addWalls = async (segments) => {
  // a segment is two points, or { curve } for a curved wall (a closed circle goes round in two)
  const r = await run("add_walls", { type_id: app.wallTypeId, level_id: curLevelId(), segments, ...wallTopParams(curLevelId()), location_line: app.locationLine, flipped: !!app.flipped });
  if (r) await refresh();
};
Plan.callbacks.editFloor = (id) => { app.selection = [id]; Plan.setSelection([id]); startFloorSketch(id); };
Plan.callbacks.editRoof = (id) => { app.selection = [id]; Plan.setSelection([id]); startRoofSketch(id); };
Plan.callbacks.select = (ids) => { app.browserSel = null; app.selection = ids; app.tdimRefs = {}; app.tdimPins = (ids.length === 1 && app.tdimMemory[ids[0]]) || {}; app.vp?.setSelection(ids); renderProperties(); refreshTempDims(); updateRibbonContext(); };

// ---- temporary dimensions ---------------------------------------------------------
// The model works out the dimensions and what editing each one does (claude/interaction-tools.md, item 7).
// Where they measure from is a per-user setting, kept in the browser per user.
const TDIM_DEFAULT = { wall_ref: "faces", opening_ref: "opening" };
function tdimSettings() {
  try { return { ...TDIM_DEFAULT, ...JSON.parse(localStorage.getItem("openarch.tempdim") || "{}") }; } catch { return { ...TDIM_DEFAULT }; }
}
async function refreshTempDims() {
  const id = app.selection.length === 1 ? app.selection[0] : null;
  if (app.view.kind !== "plan" || id === null || Plan.tool !== "select") { Plan.setTempDims([]); return; }
  try {
    const r = await Backend.call("temp_dimensions", { id, settings: tdimSettings(), refs: app.tdimRefs || {}, pins: app.tdimPins || {} });
    if (app.selection.length === 1 && app.selection[0] === id) Plan.setTempDims(r.dims);
  } catch { Plan.setTempDims([]); }
}
Plan.callbacks.tempEdit = async (dim, value) => {
  const r = await run("set_temp_dimension", { edit: dim.edit, current: dim.value, value });
  if (!r) { await refreshTempDims(); return; } // run() has already put the reason in the status bar
  await refresh();
  renderProperties();
  setPrompt("Dimension changed.");
};
// the lines the selected element's dimensions can be dragged to (faces, layers, opening and frame edges)
Plan.callbacks.tempLines = async () => {
  try { return await Backend.call("temp_dim_lines", { id: app.selection[0] }); } catch { return null; }
};
// an end of a dimension dropped on a line (or on nothing, which puts it back)
Plan.callbacks.tempPin = (key, role, spec) => {
  const pins = { ...(app.tdimPins || {}) };
  const pin = { ...(pins[key] || {}) };
  if (spec) pin[role] = spec; else delete pin[role];
  if (pin.own || pin.target) pins[key] = pin; else delete pins[key];
  app.tdimPins = pins;
  if (app.selection.length === 1) app.tdimMemory[app.selection[0]] = pins; // the choice is kept for that element
  refreshTempDims();
  setPrompt(spec ? "Dimension re-pointed." : "Dimension back to its default.");
};
// ---- permanent dimensions ---------------------------------------------------------------
// The tools and drawing are in dims.js; the model does the measuring and keeps the constraints.
async function startDimTool(kind) {
  // elevations and sections take the chain kinds, measured across the view and up
  if (app.view.kind === "elevation") {
    if (kind === "aligned" || kind === "linear") { Elev.startDim(kind); return; }
    setPrompt("Radial, diameter, angular and arc length dimensions are placed in plan views.", true);
    return;
  }
  if (!(await ensurePlan("A dimension"))) return;
  Plan.startDim(kind);
}
async function startSpotTool(kind) {
  // an elevation or section takes spot elevations on the lines it names
  if (app.view.kind === "elevation") {
    if (kind === "elevation") { Elev.startSpot(kind); return; }
    setPrompt("Spot coordinates and spot slopes are placed in plan views.", true);
    return;
  }
  if (!(await ensurePlan("A spot dimension"))) return;
  Plan.startSpot(kind);
}
Plan.callbacks.dimCall = (method, params) => run(method, params);
Plan.callbacks.dimRefresh = async () => { await refresh(); renderProperties(); };
Plan.callbacks.dimSettings = () => tdimSettings();
// the glyph beside a temporary dimension makes it a permanent one, a little further out than the temporary line
Plan.callbacks.tempPromote = async (dim, offsetMM) => {
  if (app.selection.length !== 1) return;
  const r = await run("promote_temp_dimension", { view_id: app.view.viewId, id: app.selection[0], key: dim.key, settings: tdimSettings(), refs: app.tdimRefs || {}, pins: app.tdimPins || {}, offset_mm: Math.round(offsetMM) });
  if (r) { await refresh(); setPrompt("Permanent dimension created. Select it to lock, equalise or edit its text."); }
};
// text of a dimension segment: prefix, suffix, above, below, or words in place of the number
function openDimText(d, seg) {
  openModal("Dimension Text", (body, foot, close) => {
    const field = (label, value, hint) => {
      const l = document.createElement("label"); l.style.cssText = "display:block;margin:6px 0";
      const i = document.createElement("input"); i.type = "text"; i.value = value || ""; i.style.cssText = "width:100%;box-sizing:border-box";
      l.append(label, document.createElement("br"), i);
      if (hint) { const h = document.createElement("small"); h.textContent = hint; l.append(document.createElement("br"), h); }
      return [l, i];
    };
    const [l1, prefix] = field("Prefix", seg.prefix), [l2, suffix] = field("Suffix", seg.suffix);
    const [l3, above] = field("Text above dimension line", seg.above), [l4, below] = field("Text below dimension line", seg.below);
    const [l5, words] = field("Replace with text", seg.replaced ? seg.text : "", "Words only (V.I.F., EXISTING). A number cannot stand in for the measured value.");
    const val = document.createElement("div"); val.textContent = `Value: ${seg.text && !seg.replaced ? seg.text : Math.round(seg.value_mm * 10) / 10 + " mm"}`; val.style.marginBottom = "6px";
    body.append(val, l1, l2, l3, l4, l5);
    const reset = button("Reset Text Position", async () => {
      if (await run("patch_dimension", { id: d.id, patch: { segments: [{ index: seg.index, text_shift: [0, 0] }] } })) { close(); await refresh(); }
    });
    reset.disabled = !seg.moved;
    foot.append(reset, button("Cancel", close), button("OK", async () => {
      const r = await run("patch_dimension", { id: d.id, patch: { segments: [{ index: seg.index, prefix: prefix.value, suffix: suffix.value, above: above.value, below: below.value, value_text: words.value }] } });
      if (r) { close(); await refresh(); renderProperties(); }
    }, true));
    prefix.focus();
  });
}
Plan.callbacks.dimText = openDimText;
Elev.callbacks.dimCall = (method, params) => run(method, params);
Elev.callbacks.dimRefresh = async () => { await refresh(); renderProperties(); };
Elev.callbacks.dimText = openDimText;
Plan.callbacks.placeDims = async (wallId, point) => {
  try {
    const r = await Backend.call("temp_dimensions", { type_id: placeTypeId(Plan.tool), host_id: wallId, point: [point.x, point.y], settings: tdimSettings() });
    return r.dims;
  } catch { return []; }
};
function openTempDimSettings() {
  openModal("Temporary Dimension Properties", (body, foot, close) => {
    const cur = tdimSettings();
    const group = (title, key, opts) => {
      const fs = document.createElement("fieldset");
      const lg = document.createElement("legend"); lg.textContent = title; fs.append(lg);
      for (const [v, label] of opts) {
        const l = document.createElement("label"); l.style.display = "block";
        const i = document.createElement("input"); i.type = "radio"; i.name = key; i.checked = cur[key] === v; i.onchange = () => { cur[key] = v; };
        l.append(i, ` ${label}`); fs.append(l);
      }
      return fs;
    };
    const note = document.createElement("p"); note.className = "note";
    note.textContent = "Where temporary dimensions measure from. This is kept for you in this browser, not in the project.";
    body.append(
      group("Walls", "wall_ref", [["centreline", "Centrelines"], ["faces", "Faces"], ["core_centre", "Centre of core"], ["core_faces", "Faces of core"]]),
      group("Doors and Windows", "opening_ref", [["centreline", "Centrelines"], ["opening", "Openings"]]),
      note,
    );
    foot.append(button("Cancel", close), button("OK", () => {
      try { localStorage.setItem("openarch.tempdim", JSON.stringify(cur)); } catch {}
      close();
      refreshTempDims();
      if (app.view.kind === "elevation") Elev.render();
    }, true));
  });
}


// ---- selection options and Filter (status bar) -----------------------------------
// Kept per user in the browser, not in the project.
const SEL_ICONS = {
  links: ["Select Links", "M6.5 9.5l3-3M5 8L3.8 9.2a2.4 2.4 0 003.4 3.4L8.4 11.4M11 8l1.2-1.2a2.4 2.4 0 00-3.4-3.4L7.6 4.6"],
  underlay: ["Select Underlay Elements", "M8 2l6 3-6 3-6-3zM2 8l6 3 6-3M2 11l6 3 6-3"],
  pinned: ["Select Pinned Elements", "M9.5 2.5l4 4-1.6.5-2.4 2.4.4 2.6-1 1-2.6-2.6L3 14M7 6.6l2.4-2.4"],
  face: ["Select Elements by Face", "M3 3h10v10H3zM3 3l10 10M3 8l5-5M8 13l5-5"],
  dragOnSel: ["Drag Elements on Selection", "M8 2v12M2 8h12M8 2L6 4M8 2l2 2M8 14l-2-2M8 14l2-2M2 8l2-2M2 8l2 2M14 8l-2-2M14 8l-2 2"],
};
function loadSelOpts() {
  try { return { ...Plan.selectOptions(), ...JSON.parse(localStorage.getItem("openarch.select") || "{}") }; } catch { return Plan.selectOptions(); }
}
function renderSelOpts() {
  const o = Plan.selectOptions();
  const box = $("#selopts");
  box.replaceChildren(...Object.entries(SEL_ICONS).map(([k, [title, path]]) => {
    const b = el("button", { title: `${title}: ${o[k] ? "on" : "off"}${k === "links" ? " (no linked models yet)" : k === "dragOnSel" ? " (applies once Move arrives)" : ""}` });
    b.innerHTML = `<svg viewBox="0 0 16 16"><path d="${path}"/></svg>`;
    b.classList.toggle("on", !!o[k]);
    b.onclick = () => {
      const n = { ...Plan.selectOptions(), [k]: !o[k] };
      Plan.setSelectOptions(n);
      try { localStorage.setItem("openarch.select", JSON.stringify(n)); } catch {}
      renderSelOpts();
    };
    return b;
  }));
}
function renderFilterCount() { $("#filter-btn").textContent = `Filter: ${app.selection.length}`; }
function openFilter() {
  const groups = Plan.selectionInfo();
  const cats = Object.keys(groups).sort();
  if (!cats.length) { setPrompt("Select elements first, then use Filter to choose which categories to keep."); return; }
  openModal("Filter", (body, foot, close) => {
    const on = Object.fromEntries(cats.map((c) => [c, true]));
    const rows = cats.map((c) => {
      const i = el("input", { type: "checkbox", checked: true, onchange: () => { on[c] = i.checked; } });
      return el("label", { className: "snap-row" }, [i, ` ${c} `, el("span", { className: "snap-code", textContent: `(${groups[c].length})` })]);
    });
    const boxes = () => body.querySelectorAll("input");
    body.append(el("div", { className: "snap-grid" }, rows));
    foot.append(
      button("Check All", () => boxes().forEach((b) => { b.checked = true; b.onchange(); })),
      button("Check None", () => boxes().forEach((b) => { b.checked = false; b.onchange(); })),
      button("Cancel", close),
      button("OK", () => {
        const keep = cats.filter((c) => on[c]).flatMap((c) => groups[c]);
        Plan.setSelection(keep);
        app.selection = keep; app.vp?.setSelection(keep);
        renderProperties(); close();
      }, true),
    );
  });
}
$("#filter-btn").onclick = openFilter;

async function pinSelection(pinned) {
  if (!app.selection.length) { setPrompt(`Select elements to ${pinned ? "pin" : "unpin"} first.`); return; }
  if (await run("pin", { ids: app.selection, pinned })) { await refresh(); setPrompt(pinned ? "Pinned. Pinned elements cannot be moved, deleted or flipped." : "Unpinned."); }
}
Plan.callbacks.cursor = (p) => { $("#cursor-pos").textContent = `X: ${Math.round(p.x)}  Y: ${Math.round(p.y)}`; };
// Choosing a build tool drops the selection, so the palette shows what is about to be placed
const BUILD_TOOLS = new Set(["wall", "door", "window", "grid", "level", "elevation", "section", "dimension", "spot"]);
function clearSelectionForTool(t) {
  if (!BUILD_TOOLS.has(t) || !app.selection.length) return;
  app.selection = []; app.tdimRefs = {}; app.tdimPins = {};
  Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]);
}
Plan.callbacks.tool = (t) => { if (t !== "select") app.browserSel = null; clearSelectionForTool(t); renderProperties(); renderOptionsBar(); refreshTempDims(); updateRibbonContext(); };
Plan.callbacks.prompt = (t) => setPrompt(t);

// ---- doors and windows -----------------------------------------------------------
const familyTypes = (category) => app.info.family_types.filter((t) => t.category === category);
const placeCategory = (tool) => (tool === "door" ? "Doors" : "Windows");
const placeTypeId = (tool) => (tool === "door" ? app.doorTypeId : tool === "hosted" || tool === "component" ? app.componentTypeId : app.windowTypeId);

// ---- Component: place any loaded family that is not a door or window ----------------------
// Free standing and work-plane-based families follow the cursor and drop on the view's level; wall-hosted ones are
// placed by clicking a wall, as doors are. The type selector lists every such family and type, searchable.
const componentTypes = () => {
  const fams = new Map((app.info.families || []).map((f) => [f.id, f]));
  return app.info.family_types.filter((t) => { const f = fams.get(t.family_id); return f && !["Doors", "Windows"].includes(f.category) && (f.has_body || f.host === "wall"); });
};
const familyOfType = (tid) => { const t = (app.info.family_types || []).find((x) => x.id === tid); return t ? (app.info.families || []).find((f) => f.id === t.family_id) : null; };
app.componentOffset = 0;
async function startComponentTool() {
  const types = componentTypes();
  if (!types.length) {
    openModal("No Component Families", (body, foot, close) => {
      const m = document.createElement("div"); m.className = "modal-msg";
      m.textContent = "No component families are loaded in the project. Load one from a family file, or make a new family.";
      body.append(m);
      const b = (t, fn, primary) => { const x = document.createElement("button"); x.textContent = t; if (primary) x.className = "primary"; x.onclick = () => { close(); fn && fn(); }; return x; };
      foot.append(b("Cancel"), b("New Family...", () => FamModel.newFamily()), b("Load Family...", () => FamModel.loadFile(async () => { await refresh(); startComponentTool(); }), true));
    });
    return;
  }
  if (!(await ensurePlan("A component"))) return;
  if (!types.some((t) => t.id === app.componentTypeId)) app.componentTypeId = (mruTypes().find((id) => types.some((t) => t.id === id))) ?? types[0].id;
  await useComponentType(app.componentTypeId);
  renderProperties();
}
async function useComponentType(tid) {
  app.componentTypeId = tid;
  const f = familyOfType(tid), t = app.info.family_types.find((x) => x.id === tid);
  if (f && f.host === "wall") {
    Plan.setTool("hosted");
    Plan.setPlacement(t && t.width > 0 ? t.width : 600, f.name);
    return;
  }
  if (Plan.tool !== "component") Plan.setTool("component");
  const pv = await run("type_plan_preview", { type_id: tid, detail: (app.settings && app.settings.detail_level) || "medium" });
  Plan.setComponent(pv || { parts: [], footprint: [] });
}
Plan.callbacks.placeComponent = async (point, rot) => {
  const r = await run("place_free_instance", { type_id: app.componentTypeId, level_id: curLevelId(), point: [point.x, point.y], rotation: rot, offset: app.componentOffset || 0 });
  if (r) { noteTypeUsed(app.componentTypeId); await refresh(); const f = familyOfType(app.componentTypeId); setPrompt(`Placed ${f ? f.name : "a component"}. Click to place another, Space turns it, Esc stops.`); }
};

// Most Recently Used Types: kept per user in the browser, by type id within the open project
function mruTypes() { try { return JSON.parse(localStorage.getItem("openarch.mruTypes") || "[]"); } catch { return []; } }
function noteTypeUsed(id) {
  if (id == null) return;
  const list = [id, ...mruTypes().filter((x) => x !== id)].slice(0, 10);
  try { localStorage.setItem("openarch.mruTypes", JSON.stringify(list)); } catch { /* storage off: not kept */ }
}

function syncPlacement(tool) {
  const t = app.info.family_types.find((x) => x.id === placeTypeId(tool));
  Plan.setPlacement(t ? t.width : 900, tool);
}

async function startPlacement(tool) {
  if (!(await ensurePlan(tool === "door" ? "A door" : "A window"))) return;
  Plan.setTool(tool);
  syncPlacement(tool);
}

Plan.callbacks.placeInstance = async (wallId, point) => {
  const tool = Plan.tool;
  const [flip_hand, flip_facing] = [[false, false], [true, false], [true, true], [false, true]][app.placeFlip || 0];
  const r = await run("place_instance", { type_id: placeTypeId(tool), host_id: wallId, point: [point.x, point.y], flip_hand, flip_facing });
  const what = tool === "hosted" ? "component" : tool;
  if (r) { noteTypeUsed(placeTypeId(tool)); await refresh(); setPrompt(r.warning ? `Placed a ${what}. ${r.warning}` : `Placed a ${what}. Click another wall or press Esc.`); }
};

// options bar: wall options while the wall tool is active, join options for the Wall Joins tool
function renderOptionsBar() {
  const bar = $("#optionsbar");
  const inPlan = app.view.kind === "plan";
  const wallTool = inPlan && Plan.tool === "wall", joinTool = inPlan && Plan.tool === "joins", editTool = inPlan && Plan.isEditTool(), sketchTool = Sketch.active;
  const levelTool = app.view.kind === "elevation" && Elev.tool === "level", elevTool = inPlan && Plan.tool === "elevation", gridTool = inPlan && Plan.tool === "grid";
  const secTool = inPlan && Plan.tool === "section";
  // Place Wall and Place Component add their panels to the Modify tab (one pass covers both)
  updateRibbonContext();
  bar.hidden = !(wallTool || joinTool || editTool || sketchTool || levelTool || elevTool || gridTool || secTool);
  $("#ob-section").hidden = !secTool;
  $("#ob-grid").hidden = !gridTool;
  $("#ob-level").hidden = !levelTool;
  $("#ob-elev").hidden = !elevTool;
  $("#ob-wall").hidden = !wallTool;
  $("#ob-joins").hidden = !joinTool;
  $("#ob-edit").hidden = !editTool;
  $("#ob-sketch").hidden = !sketchTool;
  if (secTool) { renderSectionBar(); return; }
  if (gridTool) {
    const go = Plan.gridOptions(), sh = $("#ob-grid-shape"), off = $("#ob-grid-offset");
    sh.value = go.shape; off.value = go.offset;
    $("#ob-grid-offset-label").hidden = go.shape !== "pick";
    sh.onchange = () => { Plan.setGridOptions({ shape: sh.value }); renderOptionsBar(); };
    off.onchange = () => Plan.setGridOptions({ offset: parseFloat(off.value) || 0 });
    return;
  }
  if (levelTool || elevTool) return;
  if (sketchTool) { renderSketchBar(); return; }
  if (joinTool) { renderJoinBar(); return; }
  if (editTool) { renderEditBar(); return; }
  if (bar.hidden) return;
  const sel = $("#ob-location");
  sel.replaceChildren(...app.info.location_lines.map((l) => { const o = new Option(l.label, l.value); o.selected = l.value === app.locationLine; return o; }));
  sel.onchange = () => { app.locationLine = sel.value; updatePlanExtents(); renderProperties(); };
  const fl = $("#ob-flip");
  fl.checked = !!app.flipped;
  fl.onchange = () => { app.flipped = fl.checked; updatePlanExtents(); };
  const h = $("#ob-height");
  h.value = app.height;
  h.onchange = () => { const v = parseFloat(h.value); if (v > 0) { app.height = v; renderProperties(); } };
  // Height: up to a level above, or unconnected at a typed height. The default is the next building storey up.
  const topSel = $("#ob-top");
  const above = levelsAbove(curLevelId());
  const auto = above.find((l) => l.building_story);
  topSel.replaceChildren(
    new Option(auto ? `Up to level: ${auto.name} (next storey)` : "Unconnected", "auto"),
    new Option("Unconnected", "unconnected"),
    ...above.map((l) => new Option(`Up to level: ${l.name}`, String(l.id))),
  );
  topSel.value = app.wallTop === "auto" || app.wallTop === "unconnected" ? app.wallTop : String(app.wallTop);
  if (topSel.value !== String(app.wallTop)) topSel.value = "auto";
  topSel.onchange = () => { app.wallTop = topSel.value === "auto" || topSel.value === "unconnected" ? topSel.value : Number(topSel.value); renderOptionsBar(); renderProperties(); };
  const unconnected = app.wallTop === "unconnected" || (app.wallTop === "auto" && !auto) || (app.wallTop !== "auto" && !above.some((l) => l.id === app.wallTop));
  $("#ob-height-label").hidden = !unconnected;
  // the draw shapes are a palette in the ribbon (Modify | Place Wall); only Sides stays in the options bar
  if (!$("#ob-sides")) {
    const sl = document.createElement("label"); sl.id = "ob-sides-label"; sl.append("Sides ");
    const sides = document.createElement("input"); sides.id = "ob-sides"; sides.type = "number"; sides.min = "3"; sides.max = "64"; sides.style.width = "52px";
    sl.append(sides);
    $("#ob-wall").prepend(sl);
  }
  const wo = Plan.wallOptions();
  const sd = $("#ob-sides");
  sd.value = wo.sides;
  sd.onchange = () => { const v = parseInt(sd.value, 10); if (v >= 3) Plan.setWallOptions({ sides: v }); };
  $("#ob-sides-label").hidden = !(wo.shape === "polygon_in" || wo.shape === "polygon_out");
}

// ---- Place Wall: the draw palette, appended to the Modify tab ---------------
const WALL_SHAPES = ["line", "rectangle", "polygon_in", "polygon_out", "circle", "arc_3pt", "arc_centre", "ellipse", "partial_ellipse", "spline"];
function wallContext() {
  if (app.view.kind !== "plan" || Plan.tool !== "wall" || Sketch.active) return null;
  const cur = Plan.wallOptions().shape;
  const draws = WALL_SHAPES.map((id) => ({ ...Sketch.TOOLS[id], onclick: () => { Plan.setWallOptions({ shape: id }); renderOptionsBar(); }, on: cur === id }));
  return { auto: true, key: `wall:${cur}`, title: "Place Wall", groups: [
    { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] },
    { label: "Draw", items: draws },
  ] };
}
const renderWallRibbon = () => updateRibbonContext();

// ---- Place Component: Load Family and the placement choice, appended to the Modify tab ----
function componentContext() {
  if (app.view.kind !== "plan" || (Plan.tool !== "component" && Plan.tool !== "hosted") || Sketch.active) return null;
  const hosted = Plan.tool === "hosted";
  const face = sketchButton({ label: "Place on Face", icon: "pickFaces" }, false, () => {}, false);
  face.disabled = true; face.title = "Place on Face: not built yet";
  const load = sketchButton({ label: "Load Family", icon: "loadFamily" }, false, () => FamModel.loadFile(async () => { await refresh(); renderProperties(); }), false);
  return { auto: true, key: `component:${hosted}`, title: hosted ? "Place on Face" : "Place Component", groups: [
    { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }, { big: true, node: load }] },
    { label: "Placement", items: hosted
      ? [{ node: sketchButton({ label: "Place on Wall", icon: "wall" }, false, () => {}, true) }]
      : [{ node: sketchButton({ label: "Place on Work Plane", icon: "setWp" }, false, () => {}, true) }, { node: face }] },
  ] };
}
const renderComponentRibbon = () => updateRibbonContext();

// ---- Modify tools ---------------------------------------------------------------------
// Select leaves whatever tool is running; the family and annotation editors have their own Select commands.
function selectTool() {
  if (Sketch.active) { Sketch.setTool("select"); renderSketchRibbon(); return; }
  if (app.view.kind === "plan") Plan.setTool("select");
  else if (app.view.kind === "elevation") Elev.setTool("select");
  else setPrompt("Click in the view to select, or pick a view in the Project Browser.");
  updateRibbonContext();
}
// Properties in the Modify tab shows or hides the Properties palette; the Project Browser stays and takes the column.
function toggleProperties() {
  const hidden = $("#workspace").classList.toggle("no-props");
  setPrompt(hidden ? "Properties palette hidden. Properties shows it again." : "Properties palette shown.");
}

const ELEV_EDIT = new Set(["move", "copy", "align"]);
async function startEditTool(name) {
  // Modify tools work where the person is: in an elevation Move, Copy and Align run in the view
  if (app.view.kind === "elevation") {
    if (ELEV_EDIT.has(name)) { Elev.setSelection(app.selection); Elev.setTool(name); }
    else setPrompt(`${EDIT_PROMPT[name] || name} works in plan views only.`, true);
    return;
  }
  if (!(await ensurePlan(EDIT_PROMPT[name] || name))) return;
  Plan.setSelection(app.selection);
  Plan.setTool(name);
}

const EDIT_PROMPT = { move: "Move", copy: "Copy", rotate: "Rotate", mirror: "Mirror", offset: "Offset", trim: "Trim/Extend", align: "Align" };

function renderEditBar() {
  const box = $("#ob-edit");
  const t = Plan.tool, o = Plan.editOptions();
  const save = () => { try { localStorage.setItem("openarch.edit", JSON.stringify(Plan.editOptions())); } catch {} };
  const set = (patch) => { Plan.setEditOptions(patch); save(); renderEditBar(); };
  const check = (label, key, title) => el("label", { title: title || "" }, [el("input", { type: "checkbox", checked: !!o[key], onchange: (e) => set({ [key]: e.target.checked }) }), ` ${label}`]);
  const radios = (key, items) => items.map(([v, l]) => el("label", {}, [el("input", { type: "radio", name: "ob-" + key, checked: o[key] === v, onchange: () => set({ [key]: v }) }), ` ${l}`]));
  const kids = [el("b", { textContent: EDIT_PROMPT[t] })];
  if (t === "move") kids.push(check("Constrain", "constrain", "Keep to the element's own direction or its perpendicular (Shift does the same for one move)"), check("Disjoin", "disjoin", "Break joins first: joined walls stay where they are, and a door or window may move to another wall"));
  if (t === "copy") kids.push(check("Constrain", "constrain"), check("Multiple", "multiple", "Keep placing copies from the same start point"));
  if (t === "rotate") {
    const ang = el("input", { type: "number", step: "any", value: o.angle, style: "width:70px", title: "Type an angle and press Enter to rotate now" });
    ang.onkeydown = async (e) => {
      if (e.key !== "Enter") return;
      const v = parseFloat(ang.value);
      if (isNaN(v) || !app.selection.length) return;
      const c = Plan.editCentre();
      await Plan.callbacks.edit({ kind: "rotate", ids: app.selection, cx: c.x, cy: c.y, angle_deg: v, copy: o.copy, disjoin: o.disjoin });
      Plan.setTool("select");
    };
    kids.push(check("Disjoin", "disjoin"), check("Copy", "copy"), el("label", {}, ["Angle ", ang]),
      el("button", { textContent: "Place centre", onclick: () => set({ centrePlace: true }) }), el("button", { textContent: "Default centre", onclick: () => { Plan.resetEditCentre(); } }));
  }
  if (t === "mirror") kids.push(...radios("mirrorMode", [["pick", "Pick axis"], ["draw", "Draw axis"]]), check("Copy", "mirrorCopy"));
  if (t === "offset") {
    const d = el("input", { type: "number", step: "any", min: "1", value: o.offsetDist, style: "width:80px", onchange: () => set({ offsetDist: parseFloat(d.value) || 0 }) });
    kids.push(...radios("offsetMode", [["graphical", "Graphical"], ["numerical", "Numerical"]]), el("label", {}, ["Offset ", d, " mm"]), check("Copy", "offsetCopy", "Keep the original (Ctrl does the same for one pick)"), el("span", { textContent: "Tab: whole chain" }));
  }
  if (t === "trim") kids.push(...radios("trimMode", [["corner", "To corner"], ["single", "Single element"], ["multiple", "Multiple elements"]]));
  if (t === "align") {
    const pf = el("select", { onchange: () => set({ prefer: pf.value }) });
    for (const [v, l] of [["all", "Any line"], ["face", "Wall faces"], ["centre", "Wall centrelines"], ["core", "Faces of core"]]) { const op = new Option(l, v); op.selected = o.prefer === v; pf.add(op); }
    kids.push(check("Multiple Alignment", "multiple", "Keep the same reference for several elements (Ctrl does the same for one pick)"), el("label", {}, ["Prefer ", pf]));
  }
  box.replaceChildren(...kids);
}

Plan.callbacks.options = () => renderEditBar();
Plan.callbacks.edit = async (p) => {
  const r = await run("transform", p);
  if (!r) return; // run() has already put the reason in the status bar
  app.selection = r.ids;
  Plan.setSelection(r.ids);
  app.vp?.setSelection(r.ids);
  await refresh();
  setPrompt(p.copy ? "Copied." : "Done.");
};
Plan.callbacks.offset = async (p) => {
  const r = await run("offset", p);
  if (!r) return;
  if (p.copy) { app.selection = r.ids; Plan.setSelection(r.ids); }
  await refresh();
};
Plan.callbacks.trim = async (p) => { if (await run("trim", p)) await refresh(); };
Plan.callbacks.align = async (p) => { if (await run("align", p)) await refresh(); };
try { Plan.setEditOptions(JSON.parse(localStorage.getItem("openarch.edit") || "{}")); } catch {}

// ---- Wall Joins tool ---------------------------------------------------------------
function el(tag, props = {}, kids = []) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}
const JOIN_LABEL = { butt: "Butt", miter: "Miter", square_off: "Square off" };
const CLEANUP_LABEL = [["use_view", "Use view setting"], ["clean", "Clean join"], ["dont_clean", "Don't clean join"]];

async function renderJoinBar() {
  const box = $("#ob-joins");
  const ends = app.joinEnd || [];
  if (!ends.length) { box.replaceChildren(el("span", { textContent: "Click a wall end to edit its join. Ctrl-click adds more ends." })); return; }
  const primary = ends[ends.length - 1];
  const info = await run("wall_join_info", primary);
  if (!info) { box.replaceChildren(); return; }
  const many = ends.length > 1;
  // a change applies to the whole join, so any wall in it can carry the request
  const apply = async (extra, list = ends) => {
    let ok = false;
    for (const e of list) ok = (await run("set_wall_join", { ...e, ...extra })) || ok;
    if (ok) await refresh();
  };
  const kids = [];
  if (info.too_complex) kids.push(el("span", { textContent: "Four or more walls meet here, so only allow and disallow apply." }));
  else {
    // Configuration: Butt, Miter, Square off. A wall meeting the body of another is always a butt.
    const kindSel = el("select", { onchange: () => apply({ kind: kindSel.value }) });
    for (const k of info.kinds) {
      const op = new Option(JOIN_LABEL[k.kind], k.kind);
      op.disabled = !many && !k.enabled;
      op.selected = k.kind === info.kind;
      kindSel.add(op);
    }
    if (!info.kind) kindSel.add(new Option("-", "", true, true), 0);
    kindSel.disabled = !many && !info.kinds.some((k) => k.enabled);
    kids.push(el("label", {}, ["Configuration ", kindSel]));
    // Previous / Next step through the walls that can run through (Butt and Square off only)
    const mine = info.options.filter((o) => o.kind === info.kind);
    const stepTo = (d) => () => {
      const at = Math.max(0, mine.findIndex((o) => o.through === info.options[info.current]?.through));
      const n = mine[(at + d + mine.length) % mine.length];
      return apply({ kind: n.kind, through: n.through }, [primary]);
    };
    const can = !many && info.steps > 0 && mine.length > 1;
    kids.push(el("button", { textContent: "Previous", disabled: !can, onclick: stepTo(-1) }));
    if (can && info.current != null) kids.push(el("b", { textContent: `wall ${info.options[info.current].through} runs through` }));
    kids.push(el("button", { textContent: "Next", disabled: !can, onclick: stepTo(1) }));
    if (info.mid != null && !many) kids.push(el("span", { textContent: `Wall ${primary.id} butts into wall ${info.mid}.` }));
    else if (!info.editable && !many) kids.push(el("span", { textContent: "Configuration needs exactly two joined walls." }));
    const cleanSel = el("select", { onchange: () => apply({ cleanup: cleanSel.value }) });
    const cu = info.walls.find((w) => w.allowed)?.cleanup || "use_view";
    for (const [v, l] of CLEANUP_LABEL) { const op = new Option(l, v); op.selected = v === cu; cleanSel.add(op); }
    kids.push(el("label", {}, ["Display ", cleanSel]));
  }
  if (many) kids.push(el("span", { textContent: `${ends.length} ends selected` }));
  else for (const w of info.walls) {
    kids.push(el("button", { textContent: `${w.allowed ? "Disallow" : "Allow"} join: wall ${w.id}`, onclick: async () => {
      if (await run("set_wall_join", { id: w.id, end: w.end, allowed: !w.allowed })) await refresh();
    } }));
  }
  box.replaceChildren(...kids);
}

Plan.callbacks.joinEnd = (list) => { app.joinEnd = list; renderJoinBar(); };

// ---- Spacebar flip ---------------------------------------------------------------
// Walls: while the wall tool is active it flips the walls about to be drawn, otherwise the
// selected walls (the layers swap sides across the location line). Doors and windows: while
// placing, it steps the swing through facing and hand; on a selection it flips the swing.
async function flipAction() {
  if (app.view.kind === "plan" && Plan.isEditTool()) return false; // Space is not a flip while a Modify tool is running
  const tool = app.view.kind === "plan" ? Plan.tool : "select";
  if (tool === "wall") {
    app.flipped = !app.flipped;
    updatePlanExtents();
    renderOptionsBar();
    setPrompt(app.flipped ? "Wall flipped: the exterior side is on the right of the drawing direction. Space flips it back." : "Wall exterior side is on the left of the drawing direction. Space flips it.");
    return true;
  }
  if (tool === "component") { Plan.rotateComponent(); return true; }
  if (tool === "door" || tool === "window" || tool === "hosted") {
    app.placeFlip = ((app.placeFlip || 0) + 1) % 4;
    const [h, f] = [[false, false], [true, false], [true, true], [false, true]][app.placeFlip];
    setPrompt(`Swing: ${h ? "hand flipped" : "normal hand"}, ${f ? "facing flipped" : "normal facing"}. Space steps through the options.`);
    return true;
  }
  if (!app.selection.length) return false;
  const kinds = await Promise.all(app.selection.map(async (id) => ({ id, d: await run("describe", { id }) })));
  const walls = kinds.filter((k) => k.d && k.d.kind === "Wall").map((k) => k.id);
  const inst = kinds.filter((k) => k.d && k.d.kind === "FamilyInstance" && ["Doors", "Windows"].includes(k.d.category)).map((k) => k.id);
  if (!walls.length && !inst.length) return false;
  if (walls.length && !(await run("flip_walls", { ids: walls }))) return true;
  if (inst.length) await run("flip_instances", { ids: inst });
  await refresh();
  return true;
}

// ---- levels, elevation markers and the project points ---------------------------------------
// Levels are drawn in an elevation (elevation.js); markers are placed in a plan. The model checks every
// change (names, pins, doors that would stop fitting) and the status bar shows why one is refused.
async function startLevelTool() {
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return; }
  if (app.view.kind !== "elevation") {
    const ev = app.info.views.find((x) => x.view_kind === "elevation");
    if (!ev) { setPrompt("Levels are drawn in an elevation view, and this project has none.", true); return; }
    await openView("elevation", ev.id);
  }
  Elev.setTool("level");
}
async function startElevationTool() {
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return; }
  if (!(await ensurePlan("An elevation marker"))) return;
  Plan.setTool("elevation");
}
async function startSectionTool() {
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return; }
  if (!(await ensurePlan("A section"))) return;
  Plan.startSection();
}
// The Section tool's options: Reference Other View places a marker that points at an existing section, callout or
// drafting view instead of making a new view.
async function renderSectionBar() {
  const on = $("#ob-sec-ref"), pick = $("#ob-sec-view");
  const targets = (await run("reference_targets", {})) || [];
  if (!targets.some((t) => t.id === app.secRef.view)) app.secRef.view = targets.length ? targets[0].id : null;
  if (!targets.length) app.secRef.on = false;
  on.checked = app.secRef.on; on.disabled = !targets.length;
  on.parentElement.title = targets.length ? "Place a section marker that points at an existing view instead of making a new one" : "There is no section, callout or drafting view to point at yet";
  pick.replaceChildren(...targets.map((t) => { const o = new Option(t.name, String(t.id)); o.selected = t.id === app.secRef.view; return o; }));
  pick.disabled = !app.secRef.on;
  on.onchange = () => { app.secRef.on = on.checked; pick.disabled = !on.checked; };
  pick.onchange = () => { app.secRef.view = Number(pick.value); };
}
Plan.callbacks.openView = (viewId) => openView("elevation", viewId);
Plan.callbacks.addSection = async (start, end) => {
  const ref = app.secRef.on && app.secRef.view != null ? { reference: app.secRef.view } : {};
  const r = await run("add_section", { level_id: curLevelId(), start, end, ...ref });
  if (!r) return;
  Plan.setTool("select");
  await refresh();
  app.selection = [r.id]; Plan.setSelection([r.id]);
  await renderProperties();
  updateRibbonContext();
  setPrompt(ref.reference != null
    ? "Reference section created. It points at an existing view and makes none of its own; double-click the head to open that view."
    : "Section created. Double-click the head or open it in the Project Browser to see the view. Drag the grips to move the ends, the cut depth or flip the side it looks at. Modify > Split Segment jogs the line; the zigzag control breaks it in this plan.");
};
async function startGridTool() {
  if (Sketch.active) { setPrompt("Finish or cancel the sketch first.", true); return; }
  if (app.view.kind === "elevation") { Elev.setTool("grid"); return; }
  if (!(await ensurePlan("A grid"))) return;
  Plan.setTool("grid");
}
const gridResult = async (r, msg) => {
  if (!r) return;
  await refresh();
  setPrompt(msg);
};
Plan.callbacks.addGrid = async (curves, locks = []) => {
  const r = await run("add_grid", { curves });
  // ends that lined up with another grid's end are locked to it
  if (r) for (const l of locks) { try { await Backend.call("lock_ends", { a: { id: r.id, end: l.end }, b: { id: l.id, end: l.otherEnd } }); } catch { /* not parallel after all */ } }
  if (r) { const g = await run("describe", { id: r.id }); await gridResult(r, `Grid ${g ? g.title.replace("Grid: ", "") : ""} created.`); }
};
Plan.callbacks.gridFromLine = async (source, offset, point) => {
  const r = await run("grid_from_line", { source, offset, point: [point.x, point.y] });
  if (r) await gridResult(r, "Grid created.");
};
Plan.callbacks.patchGrid = async (id, patch) => {
  await run("patch_grid", { id, patch });
  await refresh();
  Plan.callbacks.select(Plan.selection());
};
Plan.callbacks.moveGridEnd = async (id, end, point, lock) => {
  if (await run("set_grid_end", { id, end, point }) && lock) {
    try { await Backend.call("lock_ends", { a: { id: lock.id, end: lock.otherEnd }, b: { id, end } }); } catch { /* not parallel */ }
  }
  await refresh();
  renderProperties();
};
// the padlock between two aligned ends: closed unlocks, open locks
Plan.callbacks.toggleGridLock = async (pin) => {
  const a = { id: pin.id, end: pin.end }, b = { id: pin.other, end: pin.otherEnd };
  await run(pin.locked ? "unlock_ends" : "lock_ends", { a, b });
  await refresh();
  Plan.callbacks.select(Plan.selection());
};
// a grid placed in an elevation runs away from the viewer at the picked position
Elev.callbacks.addGrid = async (u) => {
  const f = Elev.data.frame, r3 = (v) => Math.round(v * 1000) / 1000;
  const a = { x: r3(f.origin[0] + f.right[0] * u), y: r3(f.origin[1] + f.right[1] * u) };
  const b = { x: r3(a.x + f.look[0] * f.depth_mm), y: r3(a.y + f.look[1] * f.depth_mm) };
  await Plan.callbacks.addGrid([{ type: "line", a, b }]);
};
// Move, Copy and Align in an elevation: sideways along the view's right axis and up or down
// (detail lines, regions and text notes belong to the view and move in its own frame, across and up)
Elev.callbacks.moveBy = async (ids, du, dz, copy) => {
  const f = Elev.data.frame, d = Elev.data;
  const own = new Set([...(d.detail_lines || []), ...(d.regions || []), ...(d.text_notes || [])].map((x) => x.id));
  const det = ids.filter((i) => own.has(i)), model = ids.filter((i) => !own.has(i));
  let out = [];
  if (model.length) { const r = await run("move_vertical", { ids: model, dz, dx: f.right[0] * du, dy: f.right[1] * du, copy: !!copy }); if (r) out.push(...r.ids); }
  if (det.length) { const r = await run("transform", { kind: "move", ids: det, dx: du, dy: dz, copy: !!copy, disjoin: false }); if (r) out.push(...r.ids); }
  await refresh();
  if (!out.length) return;
  if (copy || ids.length === 1 || !model.length) { app.selection = out; Elev.setSelection(out); app.vp?.setSelection(out); }
  renderProperties();
};
Elev.callbacks.leaveDetail = () => leaveDetailLines();
Elev.callbacks.editDetail = (id) => startDetailLines(id, "select");
Elev.callbacks.editRegion = (id) => startRegionSketch(null, id);
Elev.callbacks.editNote = (id) => DetailCallbacks.editNote(id);
Elev.callbacks.tempSettings = () => tdimSettings();
Elev.callbacks.tool = (t) => { clearSelectionForTool(t); renderOptionsBar(); renderProperties(); };
Elev.callbacks.prompt = (t) => setPrompt(t);
Elev.callbacks.cursor = (t) => { $("#cursor-pos").textContent = `Elev: ${t}`; };
Elev.callbacks.select = (ids) => Plan.callbacks.select(ids);
Elev.callbacks.cropSection = async (side, v) => {
  const d = Elev.data, f = d.frame, s = d.section;
  if (!s) return;
  let patch;
  if (side === "left" || side === "right") {
    // the left and right edges are the ends of the section line
    const p = [Math.round((f.origin[0] + f.right[0] * v) * 1000) / 1000, Math.round((f.origin[1] + f.right[1] * v) * 1000) / 1000];
    const leftKey = s.flipped ? "end" : "start", rightKey = s.flipped ? "start" : "end";
    patch = { [side === "left" ? leftKey : rightKey]: p };
  } else {
    patch = { z_range: side === "bottom" ? [v, f.z_max] : [f.z_min, v] };
  }
  if (!(await run("patch_section", { id: s.id, patch }))) return;
  await refresh();
  Elev.fit();
};
Elev.callbacks.addLevel = async (elevation) => {
  const r = await run("add_level", { elevation, make_plan: $("#ob-level-plan").checked });
  if (!r) return;
  await refresh();
  const l = app.info.levels.find((x) => x.id === r.id);
  setPrompt(`Level created: ${l ? l.name : ""}. Keep clicking for more, Esc to finish.`);
};
const levelPatch = async (id, patch) => { if (await run("patch_level", { id, patch })) { await refresh(); renderProperties(); } else await refresh(); };
Elev.callbacks.moveLevel = (id, elev) => levelPatch(id, { display_elevation_mm: elev });
Elev.callbacks.editElevation = (id, elev) => levelPatch(id, { display_elevation_mm: elev });
Elev.callbacks.renameLevel = (id, name) => levelPatch(id, { name });
Elev.callbacks.toggleBubble = (id, end, bubbles) => { const b = bubbles.slice(); b[end] = !b[end]; return levelPatch(id, { bubbles: b }); };
// Dragging the end of a level. `end` is the side in this view (0 left, 1 right); a level that fitted the view gets an
// extent of its own from the view's ends, and an end that lined up with another level's end is locked to it.
Elev.callbacks.setExtent = async (id, end, u, l, align) => {
  const f = Elev.data.frame, at = (v) => [Math.round((f.origin[0] + f.right[0] * v) * 1000) / 1000, Math.round((f.origin[1] + f.right[1] * v) * 1000) / 1000];
  const mEnd = end === 0 ? l.end_left : 1 - l.end_left;
  const r = await run("set_level_end", { id, end: mEnd, point: at(u), extent: [at(l.u0), at(l.u1)] });
  if (r && align) {
    // the level it lined up with needs an extent of its own before it can hold a lock
    if (!align.level.custom_extent) await run("patch_level", { id: align.id, patch: { extent: [at(align.level.u0), at(align.level.u1)].map(([x, y]) => ({ x, y })) } });
    try { await Backend.call("lock_ends", { a: { id: align.id, end: align.end }, b: { id, end: mEnd } }); } catch { /* not parallel */ }
  }
  await refresh(); renderProperties();
};
// the padlock between two aligned level ends: closed unlocks, open locks (an open one may need the extents first)
Elev.callbacks.toggleLevelLock = async (pin) => {
  const a = { id: pin.id, end: pin.end }, b = { id: pin.other, end: pin.otherEnd };
  const f = Elev.data.frame, at = (v) => ({ x: Math.round((f.origin[0] + f.right[0] * v) * 1000) / 1000, y: Math.round((f.origin[1] + f.right[1] * v) * 1000) / 1000 });
  if (!pin.locked) for (const id of [pin.id, pin.other]) { const lv = Elev.data.levels.find((x) => x.id === id); if (lv && !lv.custom_extent) await run("patch_level", { id, patch: { extent: [at(lv.u0), at(lv.u1)] } }); }
  await run(pin.locked ? "unlock_ends" : "lock_ends", { a, b });
  await refresh(); renderProperties();
};
Plan.callbacks.markerSlots = () => [...document.querySelectorAll(".ob-slot")].map((c) => c.checked);
Plan.callbacks.placeMarker = async (p, rotation_deg) => {
  const slots = Plan.callbacks.markerSlots();
  if (!slots.some(Boolean)) { setPrompt("Tick at least one pointer for the new marker.", true); return; }
  const r = await run("add_elevation_marker", { level_id: curLevelId(), point: [p.x, p.y], rotation_deg, slots });
  if (!r) return;
  await refresh();
  setPrompt("Elevation marker placed. Pick it to tick more pointers. Esc to stop.");
};
Plan.callbacks.markerSlot = async (id, slot, on) => {
  await run("patch_marker", { id, patch: { slot_on: { [slot]: on } } });
  await refresh();
  Plan.callbacks.select(Plan.selection());
};

// ---- actions --------------------------------------------------------------------
async function deleteSelection() {
  if (!app.selection.length) return;
  const ids = app.selection;
  // keep the selection when the delete is refused (pinned elements, for one)
  if (!(await run("delete", { ids }))) return;
  app.selection = [];
  Plan.setSelection([]);
  Elev.setSelection([]);
  app.vp?.setSelection([]);
  await refresh();
}

// Undo steps back through the sketch first. With no sketch steps left it leaves the sketch (nothing to lose) and carries on into the model.
async function undo() {
  if (app.doc === "family") { await FamModel.undo(); updateUndoButtons(); return; }
  if (app.doc === "annotation") { AnnModel.undo(); updateUndoButtons(); return; }
  // Detail Line's changes are in the model already: undo them there and pick the lines up again
  if (Sketch.active && Sketch.kind === "detail") { await syncDetailLines(); if (app.info.undo) { await run("undo", {}); await refresh(); restartDetailLines(); } return; }
  if (Sketch.active) { if (Sketch.undoOrLeave() === "left") setPrompt("Left the sketch. Undo again to undo the last change to the model."); return; }
  if (app.info.undo) { await run("undo", {}); await refresh(); }
  updateUndoButtons();
}
async function redo() {
  if (app.doc === "family") { await FamModel.redo(); updateUndoButtons(); return; }
  if (app.doc === "annotation") { AnnModel.redo(); updateUndoButtons(); return; }
  if (Sketch.active && Sketch.kind === "detail") { if (app.info.redo) { await run("redo", {}); await refresh(); restartDetailLines(); } return; }
  if (Sketch.active) { Sketch.redo(); return; }
  if (app.info.redo) { await run("redo", {}); await refresh(); }
}
function updateUndoButtons() {
  if (!app.info) return;
  const u = $("#qa-undo"), r = $("#qa-redo");
  const fs = (app.doc === "family" && FamModel.undoState()) || (app.doc === "annotation" && AnnModel.undoState());
  if (fs) { u.disabled = !fs.undo; r.disabled = !fs.redo; u.title = fs.undo ? "Undo (Ctrl+Z)" : "Nothing to undo"; r.title = fs.redo ? "Redo (Ctrl+Y)" : "Nothing to redo"; return; }
  const sk = Sketch.active && Sketch.kind !== "detail";
  u.disabled = !(app.info.undo || (sk && Sketch.canUndo));
  u.title = sk && Sketch.canUndo ? "Undo sketch step (Ctrl+Z)" : app.info.undo ? `Undo ${app.info.undo} (Ctrl+Z)` : "Nothing to undo";
  r.disabled = sk ? !Sketch.canRedo : !app.info.redo;
  r.title = sk ? (Sketch.canRedo ? "Redo sketch step (Ctrl+Y)" : "Nothing to redo") : app.info.redo ? `Redo ${app.info.redo} (Ctrl+Y)` : "Nothing to redo";
}

async function save(asNew) {
  if (app.doc === "family") { await FamModel.save(!!asNew); return; }
  // an annotation family lives in the project: saving it loads it into the project
  if (app.doc === "annotation") { await AnnModel.save(); return; }
  let path = app.info.path;
  if (!path || asNew) path = await Backend.pickSavePath(path);
  if (!path) return;
  if (await run("save", { path })) { setPrompt(`Saved ${path}`); await refresh(); }
}

// ---- closing the program: each open family is offered its own save first, then the project (Yes, No or Cancel)
let exiting = false;
function askYesNoCancel(title, text) {
  return new Promise((resolve) => {
    openModal(title, (body, foot, close) => {
      const done = (v) => { close(); resolve(v); };
      body.append(el("div", { className: "modal-msg", textContent: text }));
      body.closest(".modal-back").onEscape = () => done("cancel");
      foot.append(button("Yes", () => done("yes"), true), button("No", () => done("no")), button("Cancel", () => done("cancel")));
    });
  });
}
async function confirmExit() {
  if (!(await FamModel.closeForExit())) return false;
  if (!(await AnnModel.closeForExit())) return false;
  await refresh();
  if (!app.info.dirty) return true;
  const name = app.info.path ? app.info.path.split(/[\\/]/).pop() : "Untitled";
  const a = await askYesNoCancel("OpenArch BIM", `Save changes to ${name}?`);
  if (a === "cancel") return false;
  if (a === "no") return true;
  await save(false);
  await refresh();
  return !app.info.dirty;
}
async function exitRequested() {
  if (exiting) return; // already asking
  exiting = true;
  try { if (await confirmExit()) await window.__TAURI__.core.invoke("quit"); }
  finally { exiting = false; }
}
if (Backend.isDesktop && window.__TAURI__.event) {
  // the desktop shell holds the window open and asks here first; answering says the page is listening
  window.__TAURI__.event.listen("close-requested", () => { window.__TAURI__.core.invoke("close_ack").catch(() => {}); exitRequested(); });
} else {
  // in a browser, leaving the page with unsaved work asks first
  window.addEventListener("beforeunload", (e) => {
    const fam = FamModel.docInfo(), ann = AnnModel.docInfo();
    if ((app.info && app.info.dirty) || (fam && fam.dirty) || (ann && ann.dirty)) { e.preventDefault(); e.returnValue = ""; }
  });
}

async function openProject() {
  const path = await Backend.pickOpenPath();
  if (!path) return;
  if (await run("open", { path })) { app.selection = []; app.tdimMemory = {}; Plan.setSelection([]); await refresh(true); setPrompt(`Opened ${path}`); }
}

async function newProject(confirmed) {
  if (!confirmed && app.info.dirty && !confirm("Discard unsaved changes?")) return;
  await run("new_project", {});
  app.selection = []; app.tdimMemory = {}; Plan.setSelection([]);
  await refresh(true);
}

// New: a project, or a family from a template. A family needs a project behind it, so it is made in the open one.
const FAMILY_TEMPLATES = [
  ["Generic Model", "generic_models", "none"], ["Generic Model face based", "generic_models", "face"], ["Furniture", "furniture", "none"],
  ["Casework", "casework", "none"], ["Specialty Equipment", "specialty_equipment", "none"], ["Plumbing Fixture", "plumbing_fixtures", "none"],
  ["Lighting Fixture", "lighting_fixtures", "face"], ["Electrical Fixture", "electrical_fixtures", "face"], ["Mechanical Equipment", "mechanical_equipment", "none"],
  ["Planting", "planting", "none"], ["Entourage", "entourage", "none"], ["Signage", "signage", "none"], ["Door", "doors", "wall"], ["Window", "windows", "wall"],
];
function newDocument() {
  openModal("New", (body, foot, close) => {
    let kind = "project";
    const choice = document.createElement("div"); choice.className = "new-choice";
    const pick = (k, label, icon) => {
      const b = document.createElement("button"); b.type = "button"; b.dataset.kind = k;
      const ic = document.createElement("span"); ic.className = "ico"; setIcon(ic, { id: icon });
      const t = document.createElement("span"); t.textContent = label;
      b.append(ic, t);
      b.onclick = () => { kind = k; draw(); };
      return b;
    };
    const bp = pick("project", "Project", "planView"), bf = pick("family", "Family", "component");
    choice.append(bp, bf);
    const opts = document.createElement("div");
    const msg = document.createElement("div"); msg.className = "modal-msg";
    body.append(choice, opts, msg);
    const row = (label, ctl) => { const r = document.createElement("div"); r.className = "new-row"; const l = document.createElement("span"); l.textContent = label; r.append(l, ctl); return r; };
    const tplP = document.createElement("select"); tplP.add(new Option("Australian (metric)", "au"));
    const tplF = document.createElement("select");
    FAMILY_TEMPLATES.forEach(([label, cat, host], i) => tplF.add(new Option(label + (host === "wall" ? " (hosted in a wall)" : host === "face" ? " (work plane based)" : ""), i)));
    const name = document.createElement("input"); name.type = "text"; name.value = "New Family";
    tplF.onchange = () => { const t = FAMILY_TEMPLATES[Number(tplF.value)]; if (/^New /.test(name.value) || !name.value.trim()) name.value = `New ${t[0]}`; };
    const draw = () => {
      bp.classList.toggle("on", kind === "project"); bf.classList.toggle("on", kind === "family");
      opts.replaceChildren(...(kind === "project" ? [row("Template", tplP)] : [row("Template", tplF), row("Name", name)]));
      msg.textContent = kind === "project" ? (app.info.dirty ? "The open project has unsaved changes; they will be lost." : "") : "The family opens in the family editor. Load into Project puts it in this project.";
    };
    draw();
    const go = async () => {
      if (kind === "project") { close(); app.info.dirty = false; await newProject(true); return; }
      const [, category, host] = FAMILY_TEMPLATES[Number(tplF.value)];
      try {
        const r = await Backend.call("new_family", { name: name.value.trim() || "New Family", category, host });
        close(); await refresh(); FamModel.open(r.id);
      } catch (e) { msg.textContent = String(e.message || e); }
    };
    const b = (t, fn, primary) => { const x = document.createElement("button"); x.textContent = t; if (primary) x.className = "primary"; x.onclick = fn; return x; };
    foot.append(b("Cancel", close), b("OK", go, true));
  });
}
$("#qa-new").onclick = newDocument;
$("#qa-open").onclick = openProject;
$("#qa-save").onclick = () => save(false);
$("#qa-undo").onclick = undo;
$("#qa-redo").onclick = redo;

let keyBuf = "", keyTimer = null;
document.addEventListener("keydown", (e) => {
  const typing = e.target && ["INPUT", "SELECT", "TEXTAREA"].includes(e.target.tagName);
  const mod = e.ctrlKey || e.metaKey;
  if (mod && !typing) {
    const k = e.key.toLowerCase();
    if (k === "z" && !e.shiftKey) { e.preventDefault(); undo(); return; }
    if (k === "y" || (k === "z" && e.shiftKey)) { e.preventDefault(); redo(); return; }
    if (k === "s") { e.preventDefault(); save(e.shiftKey); return; }
  }
  if (typing) return;
  if (app.view.kind === "elevation" && Elev.key(e)) { e.preventDefault(); return; }
  if (e.key === " " && !mod && !e.repeat && !Sketch.active && Plan.tool !== "elevation" && !(app.view.kind === "plan" && Plan.tool === "text")) { flipAction().then((used) => { if (used) renderProperties(); }); e.preventDefault(); return; }
  if (app.view.kind === "plan" && Plan.key(e)) { e.preventDefault(); return; }
  if (app.view.kind === "3d" && e.key === "Escape" && app.selection.length) { app.selection = []; app.vp?.setSelection([]); Plan.setSelection([]); renderProperties(); return; }
  if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); deleteSelection(); return; }
  // Two letter shortcuts
  if (/^[a-z]$/i.test(e.key) && !mod) {
    keyBuf = (keyBuf + e.key.toUpperCase()).slice(-2);
    clearTimeout(keyTimer);
    keyTimer = setTimeout(() => (keyBuf = ""), 900);
    const map = Sketch.active ? {} : { GR: "grid", LL: "level", WA: "wall", DE: "del", DR: "door", WN: "window", MV: "mv", CO: "cp", RO: "ro", MM: "mi", OF: "of", TR: "tr", AL: "al", PN: "pin", UP: "unpin" };
    if (map[keyBuf] && COMMANDS[map[keyBuf]]) { COMMANDS[map[keyBuf]](); keyBuf = ""; }
    else if (app.view.kind === "plan" && Snap.isOverride(keyBuf) && (["wall", "sketch"].includes(Plan.tool) || Plan.isEditTool())) {
      Snap.setOverride(keyBuf);
      const name = keyBuf === "SS" ? "off" : keyBuf === "SO" ? "Snaps off for the next pick" : `${Snap.LABEL[Snap.override] || ""} only for the next pick`;
      setPrompt(`Snap override: ${name}.`);
      Plan.refreshSnap();
      keyBuf = "";
    }
  }
});

// ---- Sketch mode (floors and roofs now; ceilings and openings reuse it) -----------------------------
// The sketch lives in sketch.js. Here: its panels on the Modify tab, the options bar, the slope arrow
// dialog and what Finish and Cancel do to the model. claude/interaction-tools.md, item 2.
async function startFloorSketch(existingId) {
  if (Sketch.active) return;
  if (!(await ensurePlan("A floor sketch"))) return;
  if (existingId) {
    const r = await run("floor_sketch", { id: existingId });
    if (!r) return;
    Plan.startSketch({ kind: "floor", id: existingId, levelId: r.level_id, typeId: r.type_id, curves: r.curves, slope: r.slope, span: r.span, tool: "select", title: "Edit Boundary" });
  } else {
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.startSketch({ kind: "floor", levelId: curLevelId(), typeId: app.floorTypeId, curves: [], tool: "line", title: "Create Floor Boundary" });
  }
  renderSketchUI();
}

async function startRoofSketch(existingId) {
  if (Sketch.active) return;
  if (!(await ensurePlan("A roof sketch"))) return;
  if (existingId) {
    const r = await run("roof_sketch", { id: existingId });
    if (!r) return;
    Plan.startSketch({ kind: "roof", id: existingId, levelId: r.level_id, typeId: r.type_id, curves: r.curves, tool: "select", title: "Edit Roof Footprint" });
  } else {
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.startSketch({ kind: "roof", levelId: curLevelId(), typeId: app.roofTypeId, curves: [], tool: "line", title: "Create Roof Footprint" });
  }
  renderSketchUI();
}

function sketchButton(item, small, onclick, on, extra = "") {
  const b = document.createElement("button");
  b.className = "cmd" + (small ? " small" : "") + (on ? " on" : "") + (extra ? " " + extra : "");
  b.title = item.label;
  const ico = document.createElement("span");
  ico.className = "ico"; setIcon(ico, item);
  const lab = document.createElement("span");
  lab.textContent = item.label;
  b.append(ico, lab);
  b.onclick = onclick;
  return b;
}

// The sketch's own panels (Mode, Draw, its tools) go after the constant Modify tab's own tools.
function sketchContext() {
  if (!Sketch.active) return null;
  const cur = Sketch.tool;
  const modes = Sketch.kind === "detail"
    ? [{ label: "Modify", ico: "Se", big: true, onclick: () => Sketch.setTool("select"), on: cur === "select" }]
    : [
      { label: "Finish Edit Mode", ico: "✓", big: true, onclick: () => Sketch.finish(), cls: "finish" },
      { label: "Cancel Edit Mode", ico: "✕", big: true, onclick: () => Sketch.cancel(), cls: "cancel" },
      { label: "Check Sketch", ico: "Ck", onclick: async () => { const r = await Sketch.check(); if (r && r.ok) setPrompt(`The sketch is good: ${r.loops} loop${r.loops === 1 ? "" : "s"}.`); } },
      { label: "Modify", ico: "Se", onclick: () => Sketch.setTool("select"), on: cur === "select" },
    ];
  const detail = Sketch.kind === "detail" || Sketch.kind === "region";
  const draw = Sketch.DRAW.filter((t) => !detail || t.id !== "pick_walls").map((t) => ({ ...t, onclick: () => Sketch.setTool(t.id), on: cur === t.id }));
  const trims = Sketch.MODIFY.filter((t) => t.id === "trim_single" || t.id === "split").map((t) => ({ ...t, onclick: () => Sketch.setTool(t.id), on: cur === t.id }));
  const groups = [{ label: "Mode", items: modes }, { label: "Draw", items: draw }];
  if (trims.length) groups.push({ label: "Trim and Split", items: trims });
  // Detail Line and the regions: no slope or span; a line style for new lines and the selected ones
  if (detail) {
    const a = Sketch.selectedAttr();
    const styles = [...(app.info.line_styles || []), ...(Sketch.kind === "detail" ? [] : ["<Invisible lines>"])];
    const now = a ? a.style : Sketch.kind === "detail" ? app.lineStyle : app.edgeStyle;
    const s = el("select", { title: Sketch.kind === "detail" ? "The line style of new lines, and of the selected ones" : "The line style of new boundary lines, and of the selected ones" }, styles.map((n) => new Option(n, n, false, n === now)));
    if (a && a.style === null) s.add(new Option("<varies>", "*", false, true));
    s.onchange = () => {
      if (s.value === "*") return;
      if (Sketch.kind === "detail") app.lineStyle = s.value; else app.edgeStyle = s.value;
      if (Sketch.selectedAttr()) Sketch.patchSelected({ style: s.value });
    };
    groups.push({ label: "Line Style", items: [{ big: true, node: el("div", { className: "rb-select" }, [s]) }] });
  } else if (!Sketch.isRoof) {
    groups.push({ label: "Slope and Span", items: Sketch.ANNOTATE.map((t) => ({ ...t, onclick: () => Sketch.setTool(t.id), on: cur === t.id })) });
  }
  return { auto: true, key: `sketch:${Sketch.kind}:${Sketch.tool}`, title: Sketch.title || "Sketch", groups };
}
const renderSketchRibbon = () => updateRibbonContext();

function renderSketchBar() {
  const box = $("#ob-sketch");
  if (!Sketch.active) { box.replaceChildren(); return; }
  box.replaceChildren(...sketchBarKids(Sketch));
}

// The options bar of a sketch session's tool (the project's sketch, or a family form's)
function sketchBarKids(sk) {
  const t = sk.tool, o = sk.opts;
  const set = (p) => sk.setOpts(p);
  const check = (label, key, title) => el("label", { title: title || "" }, [el("input", { type: "checkbox", checked: !!o[key], onchange: (e) => set({ [key]: e.target.checked }) }), ` ${label}`]);
  const num = (label, key, unit = " mm", min = "0") => el("label", {}, [`${label} `, el("input", { type: "number", step: "any", min, value: o[key], onchange: (e) => set({ [key]: parseFloat(e.target.value) || 0 }) }), unit]);
  const radios = (key, items) => items.map(([v, l]) => el("label", {}, [el("input", { type: "radio", name: "sk-" + key, checked: o[key] === v, onchange: () => set({ [key]: v }) }), ` ${l}`]));
  const kids = [el("b", { textContent: sk.TOOLS[t].label })];
  if (t === "line") kids.push(check("Chain", "chain", "Keep drawing from the end of each line. Esc ends the chain."));
  if (t === "rectangle") kids.push(num("Radius", "cornerRadius"));
  if (t === "polygon_in" || t === "polygon_out") kids.push(num("Sides", "sides", "", "3"), num("Radius", "cornerRadius"));
  if (t === "arc_3pt") kids.push(check("Radius", "radiusOn", "Fix the radius instead of picking a point on the arc"), num("", "radius"));
  if (t === "arc_centre") kids.push(check("Counter-clockwise", "ccw"));
  if (t === "fillet") kids.push(num("Radius", "filletRadius", " mm", "1"));
  if (sk.isRoof) {
    // The options bar for a roof footprint: Defines slope, then the overhang for picked walls
    const sel = sk.selectedAttr();
    if (sel && t === "select") {
      kids.push(
        el("label", { title: "Selected edges: whether the roof slopes up from them. Off makes a gable end." }, [el("input", { type: "checkbox", checked: sel.defines_slope === true, onchange: (e) => sk.patchSelected({ defines_slope: e.target.checked }) }), ` Defines Slope (${sel.count} selected)`]),
        el("label", {}, ["Slope ", el("input", { type: "number", step: "any", min: "0", max: "89", value: sel.slope_deg ?? "", placeholder: "varies", onchange: (e) => { const v = parseFloat(e.target.value); if (v >= 0 && v < 90) sk.patchSelected({ slope_deg: v }); } }), "\u00b0"]),
      );
    } else if (sk.DRAW.some((d) => d.id === t)) {
      kids.push(check("Defines Slope", "definesSlope", "New edges slope up from this line. Off makes a gable end."), num("Slope", "slopeDeg", "\u00b0", "0"));
    }
  }
  if (t === "pick_walls") kids.push(...radios("wallFace", [["exterior", "Exterior face"], ["interior", "Interior face"], ["centre", "Centreline"]]), num(sk.isRoof ? "Overhang" : "Offset", "wallOffset", " mm", "-100000"), check("Extend into wall (to core)", "toCore", sk.isRoof ? "Measure the overhang from the wall's core face instead of its finish face" : "Measure the offset from the wall's core face instead of its finish face"), el("span", { textContent: "Tab: whole run of walls" }));
  if (t === "offset") kids.push(num("Offset", "offsetDist", " mm", "1"), check("Copy", "copy", "Keep the original"), check("Chain", "offsetChain", "Offset the whole run of connected lines (Tab turns this on and off)"));
  if (t === "align") kids.push(check("Multiple Alignment", "multiple", "Keep the same reference for several lines (Ctrl does the same for one pick)"));
  if (t === "copy") kids.push(check("Multiple", "multiple"));
  if (t === "rotate") kids.push(check("Copy", "copy"));
  if (t === "mirror") kids.push(...radios("mirrorMode", [["draw", "Draw axis"], ["pick", "Pick axis"]]), check("Copy", "mirrorCopy"));
  if (t === "trim_single") kids.push(check("Multiple", "multiple", "Keep trimming against the same boundary"));
  if (sk.slope) kids.push(el("button", { textContent: "Slope Arrow...", title: "Edit the slope", onclick: openSlopeDialog }), el("button", { textContent: "Remove Slope", onclick: () => sk.removeSlope() }));
  if (sk.span) kids.push(el("button", { textContent: "Remove Span", onclick: () => sk.removeSpan() }));
  return kids;
}

function renderSketchUI() {
  if (!Sketch.active) return;
  // the Modify tab shows the sketch's own panels while it is open
  renderSketchRibbon();
  renderSketchBar();
  renderSketchToolbar();
  updateUndoButtons();
  $("#optionsbar").hidden = false;
}

// Finish and Cancel always reachable, whatever ribbon tab is showing
function renderSketchToolbar() {
  let bar = $("#sketch-toolbar");
  if (!Sketch.active || Sketch.kind === "detail") { if (bar) bar.remove(); return; }
  if (!bar) {
    bar = el("div", { id: "sketch-toolbar" });
    $("#viewport-wrap").append(bar);
  }
  bar.replaceChildren(
    el("span", { className: "st-title", textContent: Sketch.title }),
    el("button", { className: "st-finish", title: "Finish Edit Mode", textContent: "\u2713 Finish", onclick: () => Sketch.finish() }),
    el("button", { className: "st-cancel", title: "Cancel Edit Mode", textContent: "\u2715 Cancel", onclick: () => Sketch.cancel() }),
  );
}

function endSketchUI() {
  renderSketchToolbar();
  updateUndoButtons();
  updateRibbonContext();
  renderOptionsBar();
}

function openSlopeDialog() {
  const sp = Sketch.slope;
  if (!sp) return;
  openModal("Slope Arrow", (body, foot, close) => {
    const spec = { ...sp.spec };
    const by = el("select", {}, [new Option("Slope", "slope", false, spec.by === "slope"), new Option("Height at Tail", "height_at_tail", false, spec.by === "height_at_tail")]);
    const tail = el("input", { type: "number", step: "any", value: spec.tail_offset_mm ?? 0 });
    const pct = el("input", { type: "number", step: "any", value: spec.percent ?? 2 });
    const head = el("input", { type: "number", step: "any", value: spec.head_offset_mm ?? 150 });
    const rows = el("div", { className: "dlg-rows" });
    const draw = () => {
      rows.replaceChildren(
        el("label", {}, ["Specify ", by]),
        el("label", {}, ["Height Offset at Tail (mm) ", tail]),
        by.value === "slope" ? el("label", {}, ["Slope (%) ", pct]) : el("label", {}, ["Height Offset at Head (mm) ", head]),
        el("p", { className: "note", textContent: "Heights are measured from the floor's own level. The top of the floor rises along the arrow from the tail to the head." }),
      );
    };
    by.onchange = draw;
    draw();
    body.append(rows);
    foot.append(button("Cancel", close), button("OK", () => {
      const t = parseFloat(tail.value) || 0;
      Sketch.setSlope(by.value === "slope" ? { by: "slope", tail_offset_mm: t, percent: parseFloat(pct.value) || 0 } : { by: "height_at_tail", tail_offset_mm: t, head_offset_mm: parseFloat(head.value) || 0 });
      close();
    }, true));
  });
}

// the view hosting the sketch: an elevation or section while one is open and sketching, else the plan
const sketchView = () => (Elev.sketching ? Elev : Plan);
Sketch.callbacks.change = () => { if (Sketch.kind === "detail") syncDetailLines(); renderSketchUI(); };
Sketch.callbacks.prompt = (t, isError) => setPrompt(t, !!isError);
Sketch.callbacks.slope = () => openSlopeDialog();
Sketch.callbacks.finish = async (o) => {
  if (o.kind === "region") return finishRegion(o);
  const roof = o.kind === "roof";
  const r = roof
    ? (o.id ? await run("set_roof_sketch", { id: o.id, edges: o.edges }) : await run("add_roof_sketch", { type_id: app.roofTypeId, level_id: o.levelId, edges: o.edges }))
    : o.id
      ? await run("set_floor_sketch", { id: o.id, curves: o.curves, slope: o.slope, span: o.span })
      : await run("add_floor_sketch", { type_id: app.floorTypeId, level_id: o.levelId, curves: o.curves, slope: o.slope, span: o.span });
  if (!r) return false; // run() has put the reason in the status bar
  const id = o.id || r.id;
  Plan.endSketch();
  endSketchUI();
  await refresh();
  app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
  renderProperties();
  setPrompt(roof ? (o.id ? "Roof footprint updated." : "Roof created.") : o.id ? "Floor boundary updated." : "Floor created.");
  return true;
};
Sketch.callbacks.cancel = () => {
  sketchView().endSketch();
  endSketchUI();
  renderProperties();
  setPrompt("Sketch cancelled.");
};

// ---- Detail Line and the regions (private notes: annotate-tools.md) ----------------------------------
// They are drawn in the view that is open: a plan, an elevation or a section. The view hosts the sketch while it lasts.
// Detail Line works on the view's detail lines themselves: the sketch session holds them all, every Draw tool places
// lines and the Modify tools change them, and each change goes into the model at once (one undo step). It has no
// Finish: Esc in its select tool, or another command, leaves it. Filled Region and Masking Region sketch closed loops
// with Finish and Cancel; each edge has its own line style.
let detailSync = Promise.resolve(), detailKey = "";
const detailPayload = (curves) => curves.map((c) => ({ id: c.attr.id ?? null, curve: c.curve, style: c.attr.style || app.lineStyle }));

async function startDetailLines(selectId, tool) {
  if (Sketch.active) {
    if (Sketch.kind === "detail") { if (tool) Sketch.setTool(tool); return; }
    setPrompt("Finish or cancel the sketch first.", true);
    return;
  }
  if (!(await ensureDetailView("A detail line"))) return;
  app.lineStyle ||= "Thin Lines";
  app.selection = []; Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]);
  beginDetailSession(selectId, tool || "line");
  setPrompt(Sketch.promptText());
}
function beginDetailSession(selectId, tool) {
  const V = noteView(), lines = V.detailLines();
  V.startSketch({
    kind: "detail", title: "Detail Lines", tool, fade: false, ink: getComputedStyle(document.documentElement).getPropertyValue("--text").trim(),
    curves: lines.map((l) => ({ ...l.entry, attr: { id: l.id, style: l.style } })),
    attr: (extra) => ({ style: app.lineStyle, ...(extra || {}) }),
  });
  const st = Sketch.state();
  if (selectId) { const c = st.curves.find((x) => x.attr.id === selectId); if (c) st.sel = new Set([c.uid]); }
  detailKey = JSON.stringify(detailPayload(st.curves));
  renderSketchUI();
}
// after an undo or redo of the model: the view's lines as they are now, the same tool on
function restartDetailLines() {
  const t = Sketch.tool;
  Sketch.end();
  beginDetailSession(null, t);
}
// the session's lines into the model, one change at a time; each line learns the element it is
function syncDetailLines() {
  detailSync = detailSync.then(async () => {
    if (!Sketch.active || Sketch.kind !== "detail") return;
    const entries = Sketch.state().curves.slice(), payload = detailPayload(entries), key = JSON.stringify(payload);
    if (key === detailKey) return;
    const r = await run("set_detail_lines", { view_id: sketchView().viewId(), lines: payload });
    if (!r) { detailKey = key; return; } // run() put the reason in the status bar
    entries.forEach((c, i) => { if (r.ids[i] != null) c.attr.id = r.ids[i]; });
    if (Sketch.active) detailKey = JSON.stringify(detailPayload(Sketch.state().curves));
    await refresh();
  });
  return detailSync;
}
async function leaveDetailLines() {
  if (!Sketch.active || Sketch.kind !== "detail") return;
  await syncDetailLines();
  sketchView().endSketch();
  endSketchUI();
  await refresh();
  renderProperties();
  setPrompt("Ready.");
}

async function startRegionSketch(masking, editId) {
  if (Sketch.active) {
    if (Sketch.kind === "detail") await leaveDetailLines();
    else { setPrompt("Finish or cancel the sketch first.", true); return; }
  }
  if (!(await ensureDetailView(masking ? "A masking region" : "A region"))) return;
  let curves = [];
  if (editId) {
    const r = await run("detail_region_sketch", { id: editId });
    if (!r) return;
    masking = r.masking;
    curves = r.edges.map((e) => ({ ...e, attr: { style: e.style } }));
  }
  app.lineStyle ||= "Thin Lines";
  app.regionMasking = !!masking;
  // a masking region's edges start invisible, so it hides what is behind it without an outline
  app.edgeStyle = masking ? "<Invisible lines>" : app.lineStyle;
  if (app.regionTypeId == null || !(app.info.filled_region_types || []).some((t) => t.id === app.regionTypeId)) app.regionTypeId = ((app.info.filled_region_types || [])[0] || {}).id;
  app.selection = []; Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]);
  const noun = masking ? "Masking Region" : "Filled Region";
  noteView().startSketch({ kind: "region", id: editId || null, title: editId ? "Edit Boundary" : `Create ${noun} Boundary`, curves, tool: editId ? "select" : "line", attr: (extra) => ({ style: app.edgeStyle, ...(extra || {}) }) });
  renderSketchUI();
  renderProperties();
}
async function finishRegion(o) {
  const edges = (o.edges || []).map((e) => ({ curve: e.curve, subcategory: e.style || app.edgeStyle }));
  const r = o.id
    ? await run("set_detail_region_edges", { id: o.id, edges })
    : await run("add_detail_region", { view_id: sketchView().viewId(), edges, type_id: app.regionMasking ? null : app.regionTypeId });
  if (!r) return false;
  const id = o.id || r.id;
  const V = sketchView();
  V.endSketch();
  endSketchUI();
  await refresh();
  app.selection = [id]; V.setSelection([id]); app.vp?.setSelection([id]);
  renderProperties();
  setPrompt(o.id ? "Boundary updated." : app.regionMasking ? "Masking region created." : "Filled region created.");
  return true;
}
Plan.callbacks.leaveDetail = () => leaveDetailLines();

// ---- Text notes (details.js draws and picks them) -------------------------------------------------------------------
// Placed with the Text tool in a plan, elevation or section: no leader, one or two segments, or curved. With a leader
// the last click is where it meets the text, and the note goes where its attachment point lands there. Typed in place,
// edited by a double-click. Modify | Place Text sets the leader, its attachments and the alignment of the next notes;
// Modify | Text Notes changes the selected ones, and adds and removes leaders.
app.noteOpts = { leader: "none", left_attach: "top", right_attach: "bottom", halign: "left", valign: "top" };
DetailCallbacks.options = () => app.noteOpts;
const notePt = (q) => ({ x: Math.round(q.x * 1000) / 1000, y: Math.round(q.y * 1000) / 1000 });
// a view that can hold text notes and detail items: the current one when it is a plan, elevation or section
async function ensureDetailView(what) {
  if (app.view.kind === "plan" || app.view.kind === "elevation") return true;
  return ensurePlan(what);
}
async function patchNoteLeaders(n, leaders) {
  const ds = noteView().viewScale();
  const pt = (q) => ({ x: q[0] * ds, y: q[1] * ds });
  await run("patch_text_note", { id: n.id, patch: { leaders: leaders.map((l) => ({ side: l.side, end: l.end.x !== undefined ? l.end : pt(l.end), elbow: l.elbow ? (l.elbow.x !== undefined ? l.elbow : pt(l.elbow)) : null, arc: !!l.arc })) } });
  await refresh();
}
// A box over the view to type the text in: click outside it or press Esc to finish; Ctrl+Enter too. With a leader on the
// right the text goes to the left of the click.
function noteEditor(at, sizeMm, value, onDone, o = {}) {
  const V = noteView(), c = V.canvasRect(), s = V.screenOf(at.x, at.y), px = Math.max(10, sizeMm * V.paperPx());
  const x = c.left + s.x, y = c.top + s.y - (o.lead ? px * 0.7 : 0);
  const ta = el("textarea", { className: "ann-text-edit", value: value || "" });
  ta.style.cssText = `position:fixed;${o.right ? `right:${window.innerWidth - x}px` : `left:${x}px`};top:${y}px;min-width:140px;min-height:${px * 1.4}px;font:${px}px Arial, system-ui, sans-serif;z-index:60;`;
  document.body.append(ta);
  let done = false;
  const finish = () => { if (done) return; done = true; const v = ta.value.replace(/\s+$/, ""); ta.remove(); onDone(v); };
  ta.onkeydown = (e) => { e.stopPropagation(); if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) { e.preventDefault(); finish(); } };
  ta.onblur = finish;
  setTimeout(() => ta.focus(), 0);
}
const noteType = () => {
  const types = app.info.text_note_types || [];
  if (app.textTypeId == null || !types.some((t) => t.id === app.textTypeId)) app.textTypeId = (types[0] || {}).id;
  return types.find((t) => t.id === app.textTypeId);
};
DetailCallbacks.placeNote = (pts, o) => {
  const V = noteView();
  if (!V) return;
  const type = noteType(), at = notePt(pts[pts.length - 1]), lead = pts.length > 1;
  const side = lead && pts[0].x > at.x ? "right" : "left";
  // a curved leader flipped with Space keeps its bend; one that was not bends the usual way, wherever the text goes
  const arc = o.leader === "arc", flipBend = arc && o.arc_flip ? NoteGeom.arcBend([at.x, at.y], [pts[0].x, pts[0].y], true) : null;
  const leaders = lead ? [{ side, end: notePt(pts[0]), elbow: pts.length === 3 ? notePt(pts[1]) : flipBend ? notePt({ x: flipBend[0], y: flipBend[1] }) : null, arc }] : [];
  noteEditor(at, type && type.style ? type.style.size_mm : 2.5, "", async (text) => {
    if (!text && !leaders.length) return;
    const note = { view: V.viewId(), at, text, text_type: app.textTypeId, halign: o.halign, valign: o.valign, width_mm: 0, leaders, left_attach: o.left_attach, right_attach: o.right_attach };
    if (lead && type && type.style) {
      // lay the text out as the view will, then move it so the leader's attachment point is the click
      const ds = V.viewScale(), t = { x: 0, y: 0, halign: o.halign, valign: o.valign, width_mm: 0, lines: text.split("\n").map((l) => [l]), wrap: "words", style: type.style };
      const s0 = NoteGeom.leaderStart({ text: t, left_attach: o.left_attach, right_attach: o.right_attach }, Ann.layout(t), side);
      note.at = notePt({ x: at.x - s0[0] * ds, y: at.y - s0[1] * ds });
    }
    const r = await run("add_text_note", { note });
    await refresh();
    if (r) { app.selection = [r.id]; V.setSelection([r.id]); renderProperties(); updateRibbonContext(); }
  }, { right: side === "right" && lead, lead });
};
DetailCallbacks.editNote = (id) => {
  const V = noteView(), n = V && V.textNotes().find((x) => x.id === id);
  if (!n) return;
  const ds = V.viewScale(), L = Ann.layout(n.text), tl = NoteGeom.corners(n, L)[0];
  V.setEditingNote(id);
  noteEditor({ x: tl[0] * ds, y: tl[1] * ds }, n.text.style.size_mm, n.text.lines.map((l) => l.join("")).join("\n"), async (text) => {
    V.setEditingNote(null);
    if (!text && !n.leaders.length) { await run("delete", { ids: [id] }); app.selection = []; V.setSelection([]); }
    else await run("patch_text_note", { id, patch: { text } });
    await refresh();
  });
};
DetailCallbacks.patchNote = async (id, patch) => { await run("patch_text_note", { id, patch }); await refresh(); };
// the ends of selected detail lines dragged by their grips: the view's lines go back with those ends moved (one undo step)
DetailCallbacks.moveLineEnds = async (changes) => {
  const V = noteView();
  if (!V) return;
  const lines = V.detailLines().map((l) => {
    const c = { ...l.entry.curve };
    for (const ch of changes) if (ch.id === l.id) c[ch.end] = notePt(ch.to);
    return { id: l.id, curve: c, style: l.style };
  });
  if (lines.some((l) => l.curve.type === "line" && Math.hypot(l.curve.a.x - l.curve.b.x, l.curve.a.y - l.curve.b.y) < 1)) { setPrompt("That would leave a line with no length.", true); V.render(); return; }
  await run("set_detail_lines", { view_id: V.viewId(), lines });
  await refresh();
};
DetailCallbacks.addLeader = async (id, side, p, arc) => {
  const n = noteView().textNotes().find((x) => x.id === id);
  if (!n) return;
  await patchNoteLeaders(n, [...n.leaders, { side, end: notePt(p), elbow: null, arc: !!arc }]);
};
Plan.callbacks.editNote = (id) => DetailCallbacks.editNote(id);
// Add Left or Right Leader (straight or arc) on the one selected note: click where each new leader points, Esc ends
function addNoteLeader(side, arc) {
  const n = selectedNote();
  if (!n) { setPrompt("Select one text note to add a leader to.", true); return; }
  noteView().startNoteLeader(n.id, side, arc);
  updateRibbonContext();
}
// Change the selected notes. A new alignment keeps each box where it is: the point the note hangs from moves to the
// box's side, middle or other side, and its top, middle or bottom.
async function patchNotes(list, patch) {
  const V = noteView(), ds = V.viewScale();
  for (const n of list) {
    const p = { ...patch };
    if (patch.halign || patch.valign) {
      const L = Ann.layout(n.text), ha = patch.halign || n.text.halign, va = patch.valign || n.text.valign;
      const x = ha === "right" ? L.x0 + L.w : ha === "center" ? L.x0 + L.w / 2 : L.x0, y = va === "top" ? L.top : va === "bottom" ? L.top - L.h : L.top - L.h / 2;
      const q = NoteGeom.turn(n, [x, y]); // where that point is with the note turned
      p.at = notePt({ x: q[0] * ds, y: q[1] * ds });
    }
    await run("patch_text_note", { id: n.id, patch: p });
  }
  await refresh();
  renderProperties();
  updateRibbonContext();
}
// Modify | Place Text while the Text tool is on (its options for the next notes), Modify | Text Notes while text notes
// are selected (for them, with Add and Remove Leader)
const NOTE_LEADERS = [["none", "No Leader", "leaderNone"], ["one", "One Segment", "leaderOne"], ["two", "Two Segments", "leaderTwo"], ["arc", "Curved", "leaderArc"]];
const NOTE_ATTACH = [["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]];
function textContext() {
  const V = noteView();
  if (!V || Sketch.active) return null;
  const placing = V.tool === "text" || V.tool === "noteleader", sel = placing ? [] : selectedNotes();
  if (!placing && !sel.length) return null;
  const o = app.noteOpts, f = sel[0];
  // what the buttons show: the Text tool's options, or the first selected note's
  const cur = placing ? o : { left_attach: f.left_attach, right_attach: f.right_attach, halign: f.text.halign, valign: f.text.valign };
  const set = (k, v) => {
    if (placing) { app.noteOpts = { ...app.noteOpts, [k]: v }; V.noteOptionsChanged(); renderFamContext(true); return; }
    patchNotes(sel, { [k]: v });
  };
  const attach = [];
  for (const [side, key] of [["L", "left_attach"], ["R", "right_attach"]]) {
    for (const [v, label] of NOTE_ATTACH) attach.push({ label: `${side === "L" ? "Leader Left" : "Leader Right"} ${label}`, icon: `attach${side}${label[0]}`, on: cur[key] === v, onclick: () => set(key, v), iconOnly: true, brk: v === "top" });
  }
  const align = [
    ...[["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]].map(([v, l]) => ({ label: `Align ${l}`, icon: `align${l}`, on: cur.valign === v, onclick: () => set("valign", v), iconOnly: true, brk: v === "top" })),
    ...[["left", "Left"], ["center", "Center"], ["right", "Right"]].map(([v, l]) => ({ label: `Align ${l}`, icon: `align${l}`, on: cur.halign === v, onclick: () => set("halign", v), iconOnly: true, brk: v === "left" })),
  ];
  const mode = { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => { V.setTool("select"); updateRibbonContext(); } }] };
  if (placing) {
    // the leader kinds two by two, as the Leader panel has them
    const leaders = NOTE_LEADERS.map(([v, label, icon], i) => ({ label, icon, on: o.leader === v, onclick: () => set("leader", v), iconOnly: true, brk: i % 2 === 0 }));
    return { auto: true, key: `place:${JSON.stringify(o)}`, title: "Place Text", groups: [mode, { label: "Leader", items: [...leaders, ...attach] }, { label: "Paragraph", items: align }] };
  }
  const one = sel.length === 1 && app.selection.length === 1;
  // straight leaders over arc leaders, left and right, then Remove Last Leader
  const adds = one ? [
    { label: "Add Left Side Straight Leader", icon: "leaderAddL", onclick: () => addNoteLeader("left", false), brk: true },
    { label: "Add Left Side Arc Leader", icon: "leaderAddArcL", onclick: () => addNoteLeader("left", true) },
    { label: "Add Right Side Straight Leader", icon: "leaderAddR", onclick: () => addNoteLeader("right", false), brk: true },
    { label: "Add Right Side Arc Leader", icon: "leaderAddArcR", onclick: () => addNoteLeader("right", true) },
    { label: "Remove Last Leader", icon: "leaderRemove", onclick: () => runCommand("text-lead-rm"), brk: true },
  ].map((x) => ({ ...x, iconOnly: true })) : [];
  return { auto: true, key: `notes:${sel.map((n) => n.id).join(",")}:${JSON.stringify(cur)}`, title: "Text Notes", groups: [{ label: "Leader", items: [...adds, ...attach] }, { label: "Paragraph", items: align }] };
}
Plan.callbacks.editDetail = (id) => startDetailLines(id, "select");
Plan.callbacks.editRegion = (id) => startRegionSketch(null, id);

// ---- viewport -------------------------------------------------------------------
async function initViewport() {
  const pill = $("#gpu-status"), msg = $("#gpu-msg");
  try {
    const vp = await Viewport.init($("#viewport"), (fps, ms) => { $("#fps").textContent = app.vp && app.vp.backend === "WebGL2" ? `${fps.toFixed(0)} fps (WebGL2)` : `${fps.toFixed(0)} fps, GPU ${ms.toFixed(1)} ms/frame`; });
    app.vp = window.__vp = vp;
    // clicking in the 3D view selects like the plan does: Shift or Ctrl adds, empty space clears
    vp.onPick((id, additive) => {
      let ids = additive ? [...app.selection] : [];
      if (id !== null) ids = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
      app.selection = ids; vp.setSelection(ids); Plan.setSelection(ids); renderProperties(); updateRibbonContext();
    });
    pill.textContent = `GPU: ${vp.info}`;
    pill.classList.add("ok");
    const btn = $("#stress");
    let on = false;
    btn.onclick = () => {
      on = !on;
      vp.setStress(on);
      btn.classList.toggle("on", on);
      $("#inst").textContent = on ? "1200 instances, 1 draw call" : "";
      $("#fps").textContent = "";
    };
    if (app.view.kind === "3d") refreshView(true);
  } catch (e) {
    pill.textContent = "GPU: unavailable";
    pill.classList.add("bad");
    msg.hidden = false;
    msg.textContent = `The 3D view could not start: ${e.message}. It needs WebGPU or WebGL2 in the webview.`;
    $("#stress").disabled = true;
  }
}

async function initCore() {
  const pill = $("#core-status");
  try {
    await refresh(true);
    pill.textContent = "core: ok";
    pill.classList.add("ok");
  } catch (e) {
    pill.textContent = "core: error";
    pill.classList.add("bad");
    setPrompt(String(e), true);
  }
}

Plan.setSelectOptions(loadSelOpts());
renderSelOpts();
window.__app = app; // test hook
window.__refresh = refresh; // test hook
renderTabs();
$("#viewport-wrap").className = "v-plan";
$("#stress").hidden = true;
initCore().then(() => { setPrompt("Ready. Wall tool: WA, or use the Architecture tab."); });
initViewport();

// After a value is entered in the options bar or the palette, give focus back to the view so the shortcuts (Ctrl+Z and the rest) work
for (const id of ["optionsbar", "properties"]) {
  const box = document.getElementById(id);
  if (box) box.addEventListener("change", (e) => { if (e.target && e.target.matches("input[type=number], input[type=text], select")) setTimeout(() => e.target.blur(), 0); });
}
