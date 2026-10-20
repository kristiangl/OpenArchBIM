const $ = (s) => document.querySelector(s);

const app = {
  info: null,
  tdimMemory: {}, // element id -> the dimension ends the person chose for it (kept while the project is open)
  doc: "project", // the document showing: "project", or "family" while a family is open in the family editor
  ribbonTab: { project: "Architecture", family: "Create" },
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
  wall: async () => { if (await ensurePlan("A wall")) Plan.setTool("wall"); },
  floor: () => startFloorSketch(null),
  roof: () => startRoofSketch(null),
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
  door: () => startPlacement("door"),
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
  "m-annfam": () => FamEd.list(),
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
  "edit-witness": () => { const id = app.selection.length === 1 && app.selection[0]; if (id && Plan.dimensions().some((d) => d.id === id)) Plan.startWitness(id); },
  "m-materials": () => openMaterials(),
  "m-lineweights": () => openLineWeights(),
  "m-linepatterns": () => openPatterns("line"),
  "m-fillpatterns": () => openPatterns("fill"),
  "v-template": () => createTemplate(),
  "v-dup": () => duplicateView(),
};

// Elements whose boundary can be edited, by category: the button's name says which. Ceilings and
// openings join this table when they are built on the same sketch session.
const BOUNDARY_EDITORS = {
  Floors: { label: "Edit Floor Boundary", run: (id) => startFloorSketch(id) },
  Roofs: { label: "Edit Roof Footprint", run: (id) => startRoofSketch(id) },
};
function editBoundaryTarget() {
  if (app.selection.length !== 1 || app.view.kind !== "plan") return null;
  const info = Plan.selectionInfo();
  for (const [cat, ed] of Object.entries(BOUNDARY_EDITORS)) {
    if (info[cat] && info[cat].length === 1) { const id = info[cat][0]; return { label: ed.label, run: () => ed.run(id) }; }
  }
  return null;
}
// Context-aware ribbon buttons: shown only while the selection can use them
function updateRibbonContext() {
  const b = document.querySelector('#ribbon button[data-id="edit-boundary"]');
  const w = document.querySelector('#ribbon button[data-id="edit-witness"]');
  if (!b && !w) return;
  const t = b && editBoundaryTarget();
  if (b) b.hidden = !t;
  if (b && t) { b.lastChild.textContent = t.label; b.title = t.label; }
  // witness lines can be added to and taken from the chain kinds only
  const dimSel = app.view.kind === "plan" && app.selection.length === 1 && Plan.dimensions().some((d) => d.id === app.selection[0] && !d.round);
  if (w) w.hidden = !dimSel;
  for (const btn of [b, w]) {
    const group = btn && btn.closest(".group");
    if (group) group.hidden = ![...group.querySelectorAll("button.cmd")].some((x) => !x.hidden);
  }
}

// The ribbon and its commands belong to the document showing: the project's, or the family editor's
const ribbonSet = () => (app.doc === "family" ? FAM_RIBBON : RIBBON);
const commandFor = (id) => (app.doc === "family" ? FamModel.commands[id] : COMMANDS[id]);

function renderTabs() {
  const tabs = $("#tabs");
  tabs.replaceChildren();
  Object.keys(ribbonSet()).forEach((name) => {
    const b = document.createElement("button");
    b.textContent = name;
    b.onclick = () => { app.famCtxOn = false; selectTab(name); };
    tabs.appendChild(b);
  });
  selectTab(app.ribbonTab[app.doc]);
  if (app.doc === "family") renderFamContext();
}

// The family editor's contextual tab ("Modify | Extrusion" and so on): FamModel.context() says what the selection offers.
// It appears while something is selected, comes to the front when the person selects it, and goes when they let go.
function renderFamContext(activate) {
  if (app.doc !== "family") return;
  const tabs = $("#tabs");
  let ctx = tabs.querySelector("button.contextual.fam");
  const c = FamModel.context();
  if (!c) {
    if (ctx) { const was = ctx.classList.contains("active"); ctx.remove(); if (was || app.famCtxOn) selectTab(app.ribbonTab.family); app.famCtxOn = false; }
    return;
  }
  const fresh = !ctx;
  if (fresh) { ctx = document.createElement("button"); ctx.className = "contextual fam"; ctx.onclick = () => renderFamContext(true); tabs.append(ctx); }
  ctx.textContent = c.title;
  const changed = ctx.dataset.key !== c.key || (c.auto && ctx.dataset.auto !== "1");
  ctx.dataset.key = c.key;
  ctx.dataset.auto = c.auto ? "1" : "";
  if (activate === true || ((fresh || changed) && c.auto) || app.famCtxOn) drawFamContext(c, ctx);
}
function drawFamContext(c, ctx) {
  app.famCtxOn = true;
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("active", b === ctx));
  const ribbon = $("#ribbon");
  ribbon.replaceChildren();
  for (const g of c.groups) {
    const grp = el("div", { className: "group" });
    const items = el("div", { className: "items" });
    let stack = null;
    for (const it of g.items) {
      if (it.node) { stack = null; items.append(it.node); continue; }
      const node = sketchButton(it, !it.big, it.onclick, it.on);
      if (it.big) { stack = null; items.append(node); continue; }
      if (!stack || stack.children.length >= 3) { stack = el("div", { className: "stack" }); items.append(stack); }
      stack.append(node);
    }
    grp.append(items, el("div", { className: "label", textContent: g.label }));
    ribbon.append(grp);
  }
}

// Switch between the project and the family open in the family editor (each keeps its own ribbon tab)
function setDoc(kind) {
  if (app.doc === kind) return;
  app.doc = kind;
  $("#app").classList.toggle("fam", kind === "family");
  renderTabs();
  if (kind === "project") { updateTitle(); setPrompt("Ready."); }
  renderDocTabs();
}
function renderDocTabs() {
  const bar = $("#doc-tabs");
  const fam = FamModel.docInfo();
  bar.hidden = !fam;
  if (!fam) { bar.replaceChildren(); return; }
  const name = app.info && app.info.path ? app.info.path.split(/[\\/]/).pop() : "Untitled";
  const mk = (label, active, onclick, onclose) => {
    const t = el("div", { className: "dtab" + (active ? " active" : ""), title: label }, [el("span", { textContent: label })]);
    t.onclick = onclick;
    if (onclose) { const x = el("span", { className: "x", textContent: "\u00d7", title: "Close" }); x.onclick = (e) => { e.stopPropagation(); onclose(); }; t.append(x); }
    return t;
  };
  bar.replaceChildren(
    mk(name, app.doc === "project", () => setDoc("project")),
    mk(`Family: ${fam.name}${fam.dirty ? " *" : ""}`, app.doc === "family", () => FamModel.activate(), () => FamModel.close()),
  );
}

// While a sketch is open nothing else is editable: the ribbon's Modify tools act on the sketch lines instead,
// and every other command waits for Finish or Cancel.
const SKETCH_MODIFY = { mv: "move", cp: "copy", ro: "rotate", mi: "mirror", of: "offset", tr: "trim_corner", al: "align" };
function runCommand(id, e) {
  if (Sketch.active) {
    if (SKETCH_MODIFY[id]) { Sketch.setTool(SKETCH_MODIFY[id]); renderSketchRibbon(); return; }
    if (id === "del") { Sketch.key({ key: "Delete", preventDefault() {} }); return; }
    setPrompt("Finish or cancel the sketch first.", true);
    return;
  }
  const f = commandFor(id);
  return f ? f(e) : undefined;
}

// A ribbon drop-down: the button opens a short list of commands under it
function menuButton(item, small) {
  const b = document.createElement("button");
  b.className = "cmd" + (small ? " small" : "");
  b.title = item.label;
  b.dataset.id = item.id;
  const ico = document.createElement("span");
  ico.className = "ico";
  ico.textContent = item.ico || item.label.split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase();
  const lab = document.createElement("span");
  lab.textContent = item.label + " \u25BE";
  b.append(ico, lab);
  b.onclick = (e) => {
    e.stopPropagation();
    const open = document.querySelector(".cmd-menu");
    if (open) { const same = open.dataset.for === item.id; open.remove(); if (same) return; }
    const m = document.createElement("div");
    m.className = "cmd-menu";
    m.dataset.for = item.id;
    for (const sub of item.menu) {
      const it = document.createElement("button");
      it.textContent = sub.label;
      it.disabled = !commandFor(sub.id);
      it.onclick = (ev) => { ev.stopPropagation(); m.remove(); runCommand(sub.id, ev); };
      m.append(it);
    }
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
  b.className = "cmd" + (small ? " small" : "");
  b.disabled = !commandFor(item.id);
  b.title = item.label + (commandFor(item.id) ? "" : " (not built yet)");
  b.dataset.id = item.id;
  const ico = document.createElement("span");
  ico.className = "ico";
  ico.textContent = item.ico || item.label.split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase();
  const lab = document.createElement("span");
  lab.textContent = item.label;
  b.append(ico, lab);
  if (commandFor(item.id)) b.onclick = (e) => runCommand(item.id, e);
  return b;
}

function selectTab(name) {
  const set = ribbonSet();
  if (!set[name]) name = Object.keys(set)[0];
  app.ribbonTab[app.doc] = name;
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("active", b.textContent === name));
  const ribbon = $("#ribbon");
  ribbon.replaceChildren();
  for (const g of set[name]) {
    const grp = document.createElement("div");
    grp.className = "group" + (g.label === "Family Editor" ? " fam-editor" : "");
    const items = document.createElement("div");
    items.className = "items";
    let stack = null;
    for (const it of g.items) {
      if (it.big) { stack = null; items.appendChild(cmdButton(it, false)); continue; }
      if (!stack || stack.children.length >= 3) { stack = document.createElement("div"); stack.className = "stack"; items.appendChild(stack); }
      stack.appendChild(cmdButton(it, true));
    }
    const lab = document.createElement("div");
    lab.className = "label";
    lab.textContent = g.label;
    grp.append(items, lab);
    ribbon.appendChild(grp);
  }
  if (app.doc === "family") FamModel.syncRibbon(); else updateRibbonContext();
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
    { name: "Families", children: [
      { name: "Doors", children: app.info.families.filter((f) => f.category === "Doors").map((f) => ({ name: f.name, children: app.info.family_types.filter((t) => t.family_id === f.id).map((t) => leaf(t.name)) })) },
      { name: "Floors", children: app.info.floor_types.map((t) => leaf(t.name)) },
      { name: "Roofs", children: app.info.roof_types.map((t) => leaf(t.name)) },
      { name: "Walls", children: app.info.wall_types.map((t) => leaf(t.name)) },
      { name: "Windows", children: app.info.families.filter((f) => f.category === "Windows").map((f) => ({ name: f.name, children: app.info.family_types.filter((t) => t.family_id === f.id).map((t) => leaf(t.name)) })) },
    ] },
    { name: "View Templates", children: app.info.view_templates.map((t) => leaf(t.name)) },
    { name: "Groups", children: [] },
    { name: "Links", children: [] },
  ];
  const build = (list, parent) => {
    for (const n of list) {
      const li = document.createElement("li");
      const span = document.createElement("span");
      span.textContent = n.name;
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

// ---- views ---------------------------------------------------------------------
function currentLevel() {
  return app.info.levels.find((l) => l.id === curLevelId()) || app.info.levels[0];
}

async function openView(kind, viewId) {
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
  if (app.doc === "family") return;
  const name = app.info.path ? app.info.path.split(/[\\/]/).pop() : "Untitled";
  const v = curView();
  const vn = app.view.kind === "plan" && v ? `Floor Plan: ${v.name}` : app.view.kind === "elevation" && v ? `${v.view_kind === "section" ? "Section" : "Elevation"}: ${v.name}` : app.view.kind === "3d" && v ? `3D View: ${v.name}` : "Schedule: Wall and Floor Quantities";
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
  if (!(await ensurePlan("A dimension"))) return;
  Plan.startDim(kind);
}
async function startSpotTool(kind) {
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
const placeTypeId = (tool) => (tool === "door" ? app.doorTypeId : app.windowTypeId);

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
  if (r) { await refresh(); setPrompt(r.warning ? `Placed a ${tool}. ${r.warning}` : `Placed a ${tool}. Click another wall or press Esc.`); }
};

// options bar: wall options while the wall tool is active, join options for the Wall Joins tool
function renderOptionsBar() {
  const bar = $("#optionsbar");
  const inPlan = app.view.kind === "plan";
  const wallTool = inPlan && Plan.tool === "wall", joinTool = inPlan && Plan.tool === "joins", editTool = inPlan && Plan.isEditTool(), sketchTool = inPlan && Plan.sketching;
  const levelTool = app.view.kind === "elevation" && Elev.tool === "level", elevTool = inPlan && Plan.tool === "elevation", gridTool = inPlan && Plan.tool === "grid";
  renderWallRibbon();
  bar.hidden = !(wallTool || joinTool || editTool || sketchTool || levelTool || elevTool || gridTool);
  $("#ob-grid").hidden = !gridTool;
  $("#ob-level").hidden = !levelTool;
  $("#ob-elev").hidden = !elevTool;
  $("#ob-wall").hidden = !wallTool;
  $("#ob-joins").hidden = !joinTool;
  $("#ob-edit").hidden = !editTool;
  $("#ob-sketch").hidden = !sketchTool;
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

// ---- Modify | Place Wall: the draw palette ---------------------------------
const WALL_SHAPES = ["line", "rectangle", "polygon_in", "polygon_out", "circle", "arc_3pt", "arc_centre", "ellipse", "partial_ellipse", "spline"];
let wallReturnTab = "Architecture";
function renderWallRibbon() {
  const tabs = $("#tabs");
  let ctx = tabs.querySelector("button.contextual");
  const on = app.view.kind === "plan" && Plan.tool === "wall";
  if (!on) {
    if (ctx && ctx.dataset.kind === "wall") { ctx.remove(); selectTab(wallReturnTab); }
    return;
  }
  if (Sketch.active) return;
  if (!ctx) {
    wallReturnTab = [...tabs.querySelectorAll("button")].find((b) => b.classList.contains("active"))?.textContent || "Architecture";
    ctx = document.createElement("button"); ctx.className = "contextual"; ctx.dataset.kind = "wall";
    ctx.onclick = () => renderWallRibbon();
    tabs.append(ctx);
  }
  ctx.textContent = "Modify | Place Wall";
  tabs.querySelectorAll("button").forEach((b) => b.classList.toggle("active", b === ctx));
  const cur = Plan.wallOptions().shape;
  const mk = (label, nodes) => {
    const grp = el("div", { className: "group" });
    const items = el("div", { className: "items" });
    let stack = null;
    for (const n of nodes) {
      if (n.big) { stack = null; items.append(n.node); continue; }
      if (!stack || stack.children.length >= 3) { stack = el("div", { className: "stack" }); items.append(stack); }
      stack.append(n.node);
    }
    grp.append(items, el("div", { className: "label", textContent: label }));
    return grp;
  };
  const draws = WALL_SHAPES.map((id) => ({ node: sketchButton(Sketch.TOOLS[id], true, () => { Plan.setWallOptions({ shape: id }); renderOptionsBar(); }, cur === id) }));
  $("#ribbon").replaceChildren(
    mk("Select", [{ big: true, node: sketchButton({ label: "Modify", ico: "Se" }, false, () => Plan.setTool("select"), false) }]),
    mk("Draw", draws),
  );
}

// ---- Modify tools ---------------------------------------------------------------------
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
  if (tool === "door" || tool === "window") {
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
Plan.callbacks.openView = (viewId) => openView("elevation", viewId);
Plan.callbacks.addSection = async (start, end) => {
  const r = await run("add_section", { level_id: curLevelId(), start, end });
  if (!r) return;
  Plan.setTool("select");
  await refresh();
  app.selection = [r.id]; Plan.setSelection([r.id]);
  await renderProperties();
  setPrompt("Section created. Double-click the head or open it in the Project Browser to see the view. Drag the grips to move the ends, the cut depth or flip the side it looks at.");
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
Elev.callbacks.moveBy = async (ids, du, dz, copy) => {
  const f = Elev.data.frame;
  const r = await run("move_vertical", { ids, dz, dx: f.right[0] * du, dy: f.right[1] * du, copy: !!copy });
  await refresh();
  if (!r) return;
  if (copy || ids.length === 1) { app.selection = r.ids; Elev.setSelection(r.ids); app.vp?.setSelection(r.ids); }
  renderProperties();
};
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
  if (Sketch.active) { if (Sketch.undoOrLeave() === "left") setPrompt("Left the sketch. Undo again to undo the last change to the model."); return; }
  if (app.info.undo) { await run("undo", {}); await refresh(); }
  updateUndoButtons();
}
async function redo() {
  if (app.doc === "family") { await FamModel.redo(); updateUndoButtons(); return; }
  if (Sketch.active) { Sketch.redo(); return; }
  if (app.info.redo) { await run("redo", {}); await refresh(); }
}
function updateUndoButtons() {
  if (!app.info) return;
  const u = $("#qa-undo"), r = $("#qa-redo");
  const fs = app.doc === "family" && FamModel.undoState();
  if (fs) { u.disabled = !fs.undo; r.disabled = !fs.redo; u.title = fs.undo ? "Undo (Ctrl+Z)" : "Nothing to undo"; r.title = fs.redo ? "Redo (Ctrl+Y)" : "Nothing to redo"; return; }
  const sk = Sketch.active;
  u.disabled = !(app.info.undo || (sk && Sketch.canUndo));
  u.title = sk && Sketch.canUndo ? "Undo sketch step (Ctrl+Z)" : app.info.undo ? `Undo ${app.info.undo} (Ctrl+Z)` : "Nothing to undo";
  r.disabled = sk ? !Sketch.canRedo : !app.info.redo;
  r.title = sk ? (Sketch.canRedo ? "Redo sketch step (Ctrl+Y)" : "Nothing to redo") : app.info.redo ? `Redo ${app.info.redo} (Ctrl+Y)` : "Nothing to redo";
}

async function save(asNew) {
  if (app.doc === "family") { await FamModel.save(!!asNew); return; }
  let path = app.info.path;
  if (!path || asNew) path = await Backend.pickSavePath(path);
  if (!path) return;
  if (await run("save", { path })) { setPrompt(`Saved ${path}`); await refresh(); }
}

async function openProject() {
  const path = await Backend.pickOpenPath();
  if (!path) return;
  if (await run("open", { path })) { app.selection = []; app.tdimMemory = {}; Plan.setSelection([]); await refresh(true); setPrompt(`Opened ${path}`); }
}

async function newProject() {
  if (app.info.dirty && !confirm("Discard unsaved changes?")) return;
  await run("new_project", {});
  app.selection = []; app.tdimMemory = {}; Plan.setSelection([]);
  await refresh(true);
}

$("#qa-new").onclick = newProject;
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
  if (e.key === " " && !mod && !e.repeat && !Sketch.active && Plan.tool !== "elevation") { flipAction().then((used) => { if (used) renderProperties(); }); e.preventDefault(); return; }
  if (app.view.kind === "plan" && Plan.key(e)) { e.preventDefault(); return; }
  if (app.view.kind === "3d" && e.key === "Escape" && app.selection.length) { app.selection = []; app.vp?.setSelection([]); Plan.setSelection([]); renderProperties(); return; }
  if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); deleteSelection(); return; }
  // Two letter shortcuts
  if (/^[a-z]$/i.test(e.key) && !mod) {
    keyBuf = (keyBuf + e.key.toUpperCase()).slice(-2);
    clearTimeout(keyTimer);
    keyTimer = setTimeout(() => (keyBuf = ""), 900);
    const map = Plan.sketching ? {} : { GR: "grid", LL: "level", WA: "wall", DE: "del", DR: "door", WN: "window", MV: "mv", CO: "cp", RO: "ro", MM: "mi", OF: "of", TR: "tr", AL: "al", PN: "pin", UP: "unpin" };
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
// The sketch lives in sketch.js. Here: the contextual ribbon tab, the options bar, the slope arrow
// dialog and what Finish and Cancel do to the model. claude/interaction-tools.md, item 2.
let sketchReturnTab = "Architecture";
async function startFloorSketch(existingId) {
  if (Sketch.active) return;
  if (!(await ensurePlan("A floor sketch"))) return;
  sketchReturnTab = document.querySelector("#tabs button.active")?.textContent || "Architecture";
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
  sketchReturnTab = document.querySelector("#tabs button.active")?.textContent || "Architecture";
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
  ico.className = "ico"; ico.textContent = item.ico || item.label.slice(0, 2);
  const lab = document.createElement("span");
  lab.textContent = item.label;
  b.append(ico, lab);
  b.onclick = onclick;
  return b;
}

function renderSketchRibbon() {
  const tabs = $("#tabs");
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.remove("active"));
  let ctx = tabs.querySelector("button.contextual");
  if (!ctx) { ctx = document.createElement("button"); ctx.className = "contextual"; ctx.onclick = () => renderSketchRibbon(); tabs.append(ctx); }
  ctx.textContent = `Modify | ${Sketch.title}`;
  ctx.classList.add("active");
  const ribbon = $("#ribbon");
  const group = (label, nodes) => {
    const grp = el("div", { className: "group" });
    const items = el("div", { className: "items" });
    let stack = null;
    for (const n of nodes) {
      if (n.big) { stack = null; items.append(n.node); continue; }
      if (!stack || stack.children.length >= 3) { stack = el("div", { className: "stack" }); items.append(stack); }
      stack.append(n.node);
    }
    grp.append(items, el("div", { className: "label", textContent: label }));
    return grp;
  };
  const cur = Sketch.tool;
  const modes = [
    { big: true, node: sketchButton({ label: "Finish Edit Mode", ico: "✓" }, false, () => Sketch.finish(), false, "finish") },
    { big: true, node: sketchButton({ label: "Cancel Edit Mode", ico: "✕" }, false, () => Sketch.cancel(), false, "cancel") },
    { node: sketchButton({ label: "Check Sketch", ico: "Ck" }, true, async () => { const r = await Sketch.check(); if (r && r.ok) setPrompt(`The sketch is good: ${r.loops} loop${r.loops === 1 ? "" : "s"}.`); }) },
  ];
  const mods = Sketch.MODIFY.map((t) => ({ node: sketchButton(t, true, () => Sketch.setTool(t.id), cur === t.id) }));
  const draws = Sketch.DRAW.map((t) => ({ node: sketchButton(t, true, () => Sketch.setTool(t.id), cur === t.id) }));
  const notes = Sketch.ANNOTATE.map((t) => ({ node: sketchButton(t, true, () => Sketch.setTool(t.id), cur === t.id) }));
  ribbon.replaceChildren(group("Mode", modes), group("Modify", mods), group("Draw", draws), ...(Sketch.isRoof ? [] : [group("Slope and Span", notes)]));
}

function renderSketchBar() {
  const box = $("#ob-sketch");
  if (!Sketch.active) { box.replaceChildren(); return; }
  const t = Sketch.tool, o = Sketch.opts;
  const set = (p) => Sketch.setOpts(p);
  const check = (label, key, title) => el("label", { title: title || "" }, [el("input", { type: "checkbox", checked: !!o[key], onchange: (e) => set({ [key]: e.target.checked }) }), ` ${label}`]);
  const num = (label, key, unit = " mm", min = "0") => el("label", {}, [`${label} `, el("input", { type: "number", step: "any", min, value: o[key], onchange: (e) => set({ [key]: parseFloat(e.target.value) || 0 }) }), unit]);
  const radios = (key, items) => items.map(([v, l]) => el("label", {}, [el("input", { type: "radio", name: "sk-" + key, checked: o[key] === v, onchange: () => set({ [key]: v }) }), ` ${l}`]));
  const kids = [el("b", { textContent: Sketch.TOOLS[t].label })];
  if (t === "line") kids.push(check("Chain", "chain", "Keep drawing from the end of each line. Esc ends the chain."));
  if (t === "rectangle") kids.push(num("Radius", "cornerRadius"));
  if (t === "polygon_in" || t === "polygon_out") kids.push(num("Sides", "sides", "", "3"), num("Radius", "cornerRadius"));
  if (t === "arc_3pt") kids.push(check("Radius", "radiusOn", "Fix the radius instead of picking a point on the arc"), num("", "radius"));
  if (t === "arc_centre") kids.push(check("Counter-clockwise", "ccw"));
  if (t === "fillet") kids.push(num("Radius", "filletRadius", " mm", "1"));
  if (Sketch.isRoof) {
    // The options bar for a roof footprint: Defines slope, then the overhang for picked walls
    const sel = Sketch.selectedAttr();
    if (sel && t === "select") {
      kids.push(
        el("label", { title: "Selected edges: whether the roof slopes up from them. Off makes a gable end." }, [el("input", { type: "checkbox", checked: sel.defines_slope === true, onchange: (e) => Sketch.patchSelected({ defines_slope: e.target.checked }) }), ` Defines Slope (${sel.count} selected)`]),
        el("label", {}, ["Slope ", el("input", { type: "number", step: "any", min: "0", max: "89", value: sel.slope_deg ?? "", placeholder: "varies", onchange: (e) => { const v = parseFloat(e.target.value); if (v >= 0 && v < 90) Sketch.patchSelected({ slope_deg: v }); } }), "\u00b0"]),
      );
    } else if (Sketch.DRAW.some((d) => d.id === t)) {
      kids.push(check("Defines Slope", "definesSlope", "New edges slope up from this line. Off makes a gable end."), num("Slope", "slopeDeg", "\u00b0", "0"));
    }
  }
  if (t === "pick_walls") kids.push(...radios("wallFace", [["exterior", "Exterior face"], ["interior", "Interior face"], ["centre", "Centreline"]]), num(Sketch.isRoof ? "Overhang" : "Offset", "wallOffset", " mm", "-100000"), check("Extend into wall (to core)", "toCore", Sketch.isRoof ? "Measure the overhang from the wall's core face instead of its finish face" : "Measure the offset from the wall's core face instead of its finish face"), el("span", { textContent: "Tab: whole run of walls" }));
  if (t === "offset") kids.push(num("Offset", "offsetDist", " mm", "1"), check("Copy", "copy", "Keep the original"), check("Chain", "offsetChain", "Offset the whole run of connected lines (Tab turns this on and off)"));
  if (t === "align") kids.push(check("Multiple Alignment", "multiple", "Keep the same reference for several lines (Ctrl does the same for one pick)"));
  if (t === "copy") kids.push(check("Multiple", "multiple"));
  if (t === "rotate") kids.push(check("Copy", "copy"));
  if (t === "mirror") kids.push(...radios("mirrorMode", [["draw", "Draw axis"], ["pick", "Pick axis"]]), check("Copy", "mirrorCopy"));
  if (t === "trim_single") kids.push(check("Multiple", "multiple", "Keep trimming against the same boundary"));
  if (Sketch.slope) kids.push(el("button", { textContent: "Slope Arrow...", title: "Edit the slope", onclick: openSlopeDialog }), el("button", { textContent: "Remove Slope", onclick: () => Sketch.removeSlope() }));
  if (Sketch.span) kids.push(el("button", { textContent: "Remove Span", onclick: () => Sketch.removeSpan() }));
  box.replaceChildren(...kids);
}

function renderSketchUI() {
  if (!Sketch.active) return;
  // keep the person on whichever tab they chose; the contextual tab stays one click away
  const ctx = $("#tabs button.contextual");
  if (!ctx || ctx.classList.contains("active")) renderSketchRibbon();
  renderSketchBar();
  renderSketchToolbar();
  updateUndoButtons();
  $("#optionsbar").hidden = false;
}

// Finish and Cancel always reachable, whatever ribbon tab is showing
function renderSketchToolbar() {
  let bar = $("#sketch-toolbar");
  if (!Sketch.active) { if (bar) bar.remove(); return; }
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
  const ctx = $("#tabs button.contextual");
  if (ctx) ctx.remove();
  renderSketchToolbar();
  updateUndoButtons();
  selectTab(sketchReturnTab);
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

Sketch.callbacks.change = () => renderSketchUI();
Sketch.callbacks.prompt = (t, isError) => setPrompt(t, !!isError);
Sketch.callbacks.slope = () => openSlopeDialog();
Sketch.callbacks.finish = async (o) => {
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
  Plan.endSketch();
  endSketchUI();
  renderProperties();
  setPrompt("Sketch cancelled.");
};

// ---- viewport -------------------------------------------------------------------
async function initViewport() {
  const pill = $("#gpu-status"), msg = $("#gpu-msg");
  try {
    const vp = await Viewport.init($("#viewport"), (fps, ms) => { $("#fps").textContent = `${fps.toFixed(0)} fps, GPU ${ms.toFixed(1)} ms/frame`; });
    app.vp = window.__vp = vp;
    // clicking in the 3D view selects like the plan does: Shift or Ctrl adds, empty space clears
    vp.onPick((id, additive) => {
      let ids = additive ? [...app.selection] : [];
      if (id !== null) ids = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
      app.selection = ids; vp.setSelection(ids); Plan.setSelection(ids); renderProperties();
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
    msg.textContent = `WebGPU could not start: ${e.message}. The 3D view needs it; the fallback is native wgpu rendering.`;
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
