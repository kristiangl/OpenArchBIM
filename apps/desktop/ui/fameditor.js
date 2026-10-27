// The family editor for model families (doors, windows, furniture and so on). A family is reference planes,
// dimensions that drive them from parameters, and forms (extrusions and voids) whose sketch points lock to the
// planes. Views: Ref. Level (plan), Front and Left draw the planes and the geometry; 3D is a quick preview.
// Everything is stored in the family's body (see crates/openarch-model/src/famgeom.rs); this file only edits it.

const FamModel = (() => {
  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  // h and v are the axes across and up the view; n is the view direction (the work plane's normal)
  const VIEWS = {
    plan: { h: "x", v: "y", n: "z", label: "Ref. Level", origin: 3, group: "Floor Plans" },
    front: { h: "x", v: "z", n: "y", label: "Front", origin: 2, group: "Elevations" },
    back: { h: "x", v: "z", n: "y", label: "Back", origin: 2, flip: true, group: "Elevations" },
    left: { h: "y", v: "z", n: "x", label: "Left", origin: 1, flip: true, group: "Elevations" },
    right: { h: "y", v: "z", n: "x", label: "Right", origin: 1, group: "Elevations" },
  };
  // Back and Left look the other way: the same projection with the horizontal axis running right to left
  const PREVIEW_KEY = { plan: "plan", front: "front", back: "front", left: "left", right: "left" };
  const VIEW_TITLE = { plan: "Floor Plan: Ref. Level", front: "Elevation: Front", back: "Elevation: Back", left: "Elevation: Left", right: "Elevation: Right", "3d": "3D View: View 1" };
  const KINDS = [["length", "Length"], ["number", "Number"], ["integer", "Integer"], ["angle", "Angle"], ["yes_no", "Yes/No"], ["text", "Text"], ["material", "Material"]];
  const SUBS = ["Frame/Mullion", "Panel", "Glass", "Plan Swing"];
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const num = (v) => { const n = Number(v); return Number.isFinite(n) ? n : 0; };

  let S = null; // the open editor

  // ---------------------------------------------------------------- the manager list
  async function list() {
    openModal("Families", (body, foot, close) => {
      const msg = el("div", "modal-msg");
      const box = el("div");
      body.append(box, msg);
      const load = async () => {
        await refresh();
        box.replaceChildren();
        for (const cat of ["Doors", "Windows", "Generic Models"]) {
          const fams = app.info.families.filter((f) => f.category === cat);
          if (!fams.length) continue;
          box.append(el("div", "famed-h", cat));
          for (const f of fams) {
            const row = el("div", "famed-list-row");
            const n = el("span", "", f.name + (f.has_body ? "" : "  (drawn by code, not editable)"));
            if (f.source_path) n.title = `File: ${f.source_path}`;
            row.append(n);
            if (f.has_body) row.append(button("Edit...", () => { close(); open(f.id); }));
            if (f.has_body) row.append(button("Duplicate...", () => duplicateTypePrompt(f.id, async () => { await load(); }, "duplicate_family")));
            row.append(button("Save as File...", () => exportFamily(f.id, f.name)));
            box.append(row);
          }
        }
      };
      foot.append(button("New Family...", () => newFamily(close)), button("Open File...", () => openFile(close)), button("Load Family...", () => loadFile(load)), button("Close", close, true));
      load();
    });
  }

  function askText(title, label, initial, options, ok) {
    openModal(title, (body, foot, close) => {
      const row = el("div", "famed-row");
      row.append(el("span", "", label));
      const inp = el("input");
      inp.type = "text"; inp.value = initial || "";
      if (options && options.length) {
        const dl = el("datalist"); dl.id = "fe-dl-" + Math.random().toString(36).slice(2);
        options.forEach((o) => { const op = el("option"); op.value = o; dl.append(op); });
        inp.setAttribute("list", dl.id); body.append(dl);
      }
      row.append(inp);
      body.append(row);
      const go = () => { close(); ok(inp.value); };
      inp.onkeydown = (e) => { if (e.key === "Enter") go(); };
      foot.append(button("Cancel", close), button("OK", go, true));
      setTimeout(() => { inp.focus(); inp.select(); }, 0);
    });
  }

  function newFamily(closeList) {
    openModal("New Family - Select Template", (body, foot, close) => {
      const name = el("input"); name.type = "text"; name.value = "New Family";
      const cat = el("select");
      [["generic_models", "Generic Model (free standing)"], ["doors", "Door (hosted in a wall)"], ["windows", "Window (hosted in a wall)"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; cat.append(o); });
      const r1 = el("div", "famed-row"), r2 = el("div", "famed-row");
      r1.append(el("span", "", "Name"), name); r2.append(el("span", "", "Template"), cat);
      const msg = el("div", "modal-msg");
      body.append(r1, r2, msg);
      foot.append(button("Cancel", close), button("Create", async () => {
        try {
          const host = cat.value === "generic_models" ? "none" : "wall";
          const r = await Backend.call("new_family", { name: name.value, category: cat.value, host });
          close(); if (closeList) closeList();
          await refresh();
          open(r.id);
        } catch (e) { msg.textContent = String(e.message || e); }
      }, true));
    });
  }

  // Write a project family to a file (the project's copy remembers where, nothing stays linked)
  async function exportFamily(id, name) {
    try {
      const path = await Backend.pickSaveFamily(`${name}.oaf`);
      if (!path) return;
      const r = await Backend.call("save_family_file", { id, path, loaded_at: today() });
      await refresh();
      setPrompt(`Saved ${r.path}`);
    } catch (e) { setPrompt(String(e.message || e), true); }
  }

  // Loading a file over a family the project already has: say which copy wins (the type values too, or not)
  function askReplace(info) {
    return new Promise((resolve) => {
      let settled = false;
      const done = (v, close) => { settled = true; if (close) close(); resolve(v); };
      openModal("Family Already Exists", (body, foot, close) => {
        body.append(el("div", "modal-msg", `A family called "${info.name}" is already in this project. Placed copies keep their place and take the loaded family.`));
        foot.append(button("Cancel", () => done(null, close)));
        foot.append(button("Overwrite the Existing Version", () => done("keep_values", close)));
        foot.append(button("Overwrite the Existing Version and its Parameter Values", () => done("overwrite", close), true));
      });
      const back = [...document.querySelectorAll(".modal-back")].pop();
      const watch = new MutationObserver(() => { if (!back.isConnected) { watch.disconnect(); if (!settled) resolve(null); } });
      watch.observe(document.body, { childList: true });
    });
  }

  // Load a family file into the project: the project gets its own copy that remembers the file's place.
  // Returns the family id, or null when nothing was loaded.
  async function loadFile(after, { quiet } = {}) {
    const path = await Backend.pickOpenFamily();
    if (!path) return null;
    try {
      const info = await Backend.call("family_file_info", { path });
      let mode = "refuse";
      if (info.exists != null) {
        if (S && S.id === info.exists) { setPrompt(`Close "${S.def.name}" in the family editor before loading a file over it.`, true); return null; }
        mode = await askReplace(info);
        if (!mode) return null;
      }
      const r = await Backend.call("import_family_file", { path, mode, loaded_at: today() });
      await refresh();
      if (!quiet) setPrompt(`${r.replaced ? "Reloaded" : "Loaded"} ${info.name}.`);
      if (after) after();
      return r.id;
    } catch (e) { setPrompt(String(e.message || e), true); return null; }
  }

  // Open a family file for editing. The project keeps a copy of it while it is open (a family with no project behind it comes later).
  async function openFile(closeList) {
    if (S) { activate(); setPrompt(`Close "${S.def.name}" before opening another family.`, true); return; }
    const id = await loadFile(null, { quiet: true });
    if (id == null) return;
    if (closeList) closeList();
    open(id);
  }

  // ---------------------------------------------------------------- the editor
  async function open(familyId) {
    if (S && S.id === familyId) { activate(); return; }
    if (S) { activate(); setPrompt(`Close "${S.def.name}" before opening another family.`, true); return; }
    let def;
    try { def = await Backend.call("family_def", { id: familyId }); } catch (e) { setPrompt(String(e.message || e), true); return; }
    const first = def.types[0];
    S = {
      id: familyId, def, body: clone(def.body), params: clone(def.params), settings: clone(def.settings || {}), types: clone(def.types),
      typeId: first ? first.id : null, values: {}, sourcePath: def.source_path || null, fileDirty: false, projDirty: false, view: "plan", tool: "select", sketch: "rect", sel: null,
      draft: null, pv: null, cams: {}, dirty: false, undo: [], redo: [], mouse: null, tick: 0, drag: null,
      start: "0", end: "100", a0: "0", a1: "360", pathMode: "line", pathClosed: false, blend: null, rev: null, wp: null, msg: "",
      show: { names: true, dims: def.body.planes.length <= 14 }, symMode: "line", ctrlKind: "single_horizontal", connKind: "duct", open: ["plan", "front"],
      lockNew: true, alignOpts: { multiple: false, lock: false, prefer: "planes" }, alignRef: null, wps: {}, showWp: false, editing: null, lockHits: [],
    };
    vp3Fitted = false; vp3Key = null; // a new family is framed afresh in 3D
    mount();
    await preview();
    loadComps();
    activate();
    fitAll();
    render();
  }

  // The families of the project that can be placed in this one
  async function loadComps() {
    try { S.comps = (await Backend.call("family_components", { id: S.id })).families; } catch (e) { S.comps = []; }
    if (S.tool === "comp") toolbar();
  }
  // what undo takes back: the body, parameters and settings, and the open type's values (a labelled dimension and
  // Flex change those)
  function snapshot() { return JSON.stringify({ body: S.body, params: S.params, settings: S.settings, values: S.values, saveVals: S.saveVals || {} }); }
  // Unsaved changes: ones the project copy has not got (Load into Project) and ones the family file has not got (Save)
  function touch() { S.projDirty = true; S.fileDirty = true; }
  const isDirty = () => !!(S && (S.projDirty || S.fileDirty));
  const today = () => new Date().toISOString().slice(0, 10);
  function pushUndo() { S.undo.push(snapshot()); if (S.undo.length > 100) S.undo.shift(); S.redo = []; touch(); }
  function restore(js) { const o = JSON.parse(js); S.body = o.body; S.params = o.params; if (o.settings) S.settings = o.settings; if (o.values) S.values = o.values; if (o.saveVals) S.saveVals = o.saveVals; S.sel = null; }
  async function undo() { if (!S.undo.length) return; S.redo.push(snapshot()); restore(S.undo.pop()); await preview(); props(); render(); }
  async function redo() { if (!S.redo.length) return; S.undo.push(snapshot()); restore(S.redo.pop()); await preview(); props(); render(); }

  // A change to the body: kept if it still solves, otherwise put back with the reason.
  async function change(fn, keep) {
    const before = snapshot();
    const sel = S.sel;
    try { fn(S.body); } catch (e) { restore(before); S.sel = sel; S.msg = String(e.message || e); status(); render(); return false; }
    const ok = await preview();
    if (!ok) { restore(before); S.sel = sel; S.msg = S.pvError || "That change does not solve."; await preview(); }
    else { S.undo.push(before); S.redo = []; touch(); S.msg = ""; }
    if (!keep) props();
    status();
    render();
    ctxRefresh();
    return ok;
  }

  // An edit done by the Rust side (dimensions, plane moves): it needs to know where the planes stand.
  async function edit(op, args, keep, pre) {
    const before = snapshot();
    const sel = S.sel;
    let r;
    if (pre) pre();
    try { r = await Backend.call("family_edit", { id: S.id, type: S.typeId, values: S.values, body: S.body, params: S.params, op, ...args }); }
    catch (e) { if (pre) { restore(before); S.sel = sel; } S.msg = String(e.message || e); status(); render(); return null; }
    S.body = r.body;
    const ok = await preview();
    if (!ok) { restore(before); S.sel = sel; S.msg = S.pvError || "That change does not solve."; await preview(); status(); render(); return null; }
    S.undo.push(before); S.redo = []; touch(); S.msg = "";
    if (!keep) props();
    status(); render(); ctxRefresh();
    return r;
  }
  const dimById = (id) => (S.body.dims || []).find((d) => d.id === id);
  const dimRefs = (d) => [d.a, ...(d.via || []), d.b];
  const drivenBy = (planeId) => (S.body.dims || []).find((d) => (d.drives || []).includes(planeId));

  const newId = () => {
    const b = S.body;
    return 1 + Math.max(0, ...b.planes.map((x) => x.id), ...b.forms.map((x) => x.id), ...b.syms.map((x) => x.id), ...(b.nested || []).map((x) => x.id), ...(b.model_lines || []).map((x) => x.id), ...(b.controls || []).map((x) => x.id), ...(b.connectors || []).map((x) => x.id), ...(b.model_texts || []).map((x) => x.id), ...(b.dims || []).map((x) => x.id));
  };
  const planeById = (id) => S.body.planes.find((p) => p.id === id);
  const formById = (id) => S.body.forms.find((f) => f.id === id);
  const symById = (id) => S.body.syms.find((f) => f.id === id);
  const mlineById = (id) => (S.body.model_lines || []).find((f) => f.id === id);
  const ctrlById = (id) => (S.body.controls || []).find((f) => f.id === id);
  const connById = (id) => (S.body.connectors || []).find((f) => f.id === id);
  const mtextById = (id) => (S.body.model_texts || []).find((f) => f.id === id);
  const compById = (id) => (S.body.nested || []).find((f) => f.id === id);
  const AXI = { x: 0, y: 1, z: 2 };
  const sketchOf = (ax) => (ax === "z" ? ["x", "y"] : ax === "y" ? ["x", "z"] : ["y", "z"]);
  // a piece that came from a component belongs to it: clicking it selects the whole component
  const ownerOf = (id) => (S.owners ? S.owners[id] : undefined);
  // the selection: one item, or several made by a box or Ctrl+click ({ kind: "multi", items })
  const selItems = () => (!S.sel ? [] : S.sel.kind === "multi" ? S.sel.items : [S.sel]);
  const selHas = (kind, id) => selItems().some((x) => x.id === id && x.kind === kind);
  const ownedSel = (id) => ownerOf(id) != null && selHas("comp", ownerOf(id));
  const isSel = (kind, id) => selHas(kind, id) || (kind === "form" && selHas("mtext", id)) || ownedSel(id);
  // set the selection from a list, one item standing alone
  function setSel(items) {
    const seen = new Set(), list = [];
    for (const x of items) { const k = `${x.kind}:${x.id}`; if (!seen.has(k)) { seen.add(k); list.push({ kind: x.kind, id: x.id }); } }
    S.sel = !list.length ? null : list.length === 1 ? list[0] : { kind: "multi", id: list.map((x) => `${x.kind}${x.id}`).join(","), items: list };
  }
  // a solved 3D point as the view sees it
  const proj = (p) => (S.view === "plan" ? [p[0], p[1]] : S.view === "front" || S.view === "back" ? [p[0], p[2]] : [p[1], p[2]]);
  // What each kind of connector carries: [property, kind, default, choices]. Numbers may be formulas of the family parameters.
  const CONN = {
    electrical: { label: "Electrical Connector", props: [["System Type", "choice", "Power - Balanced", ["Power - Balanced", "Power - Unbalanced", "Data", "Telephone", "Fire Alarm", "Security", "Communication", "Controls", "Nurse Call"]], ["Voltage", "num", "230"], ["Apparent Load", "num", "100"], ["Number of Poles", "choice", "1", ["1", "2", "3"]]] },
    duct: { label: "Duct Connector", props: [["System Type", "choice", "Supply Air", ["Supply Air", "Return Air", "Exhaust Air", "Global"]], ["Shape", "choice", "Round", ["Round", "Rectangular", "Oval"]], ["Diameter", "num", "150"], ["Width", "num", "150"], ["Height", "num", "150"], ["Flow", "num", "0"], ["Flow Direction", "choice", "Bidirectional", ["In", "Out", "Bidirectional"]]] },
    pipe: { label: "Pipe Connector", props: [["System Type", "choice", "Domestic Cold Water", ["Domestic Hot Water", "Domestic Cold Water", "Sanitary", "Hydronic Supply", "Hydronic Return", "Fire Protection Wet", "Vent", "Other"]], ["Diameter", "num", "20"], ["Flow", "num", "0"], ["Flow Direction", "choice", "Bidirectional", ["In", "Out", "Bidirectional"]]] },
    cable_tray: { label: "Cable Tray Connector", props: [["Width", "num", "100"], ["Height", "num", "50"]] },
    conduit: { label: "Conduit Connector", props: [["Diameter", "num", "20"]] },
  };
  const CONTROL_KINDS = [["single_vertical", "Single Vertical"], ["double_vertical", "Double Vertical"], ["single_horizontal", "Single Horizontal"], ["double_horizontal", "Double Horizontal"]];
  const pos = (id) => (S.pv && S.pv.planes ? S.pv.planes[id] : undefined);
  // The plane new sketches go on in the view showing: the one chosen with Work Plane > Set or in the options bar,
  // otherwise the origin plane facing the view.
  function workPlane() {
    const V = VIEWS[S.view] || VIEWS.plan, id = S.wps[S.view];
    return id && planeById(id) && planeById(id).axis === V.n ? id : V.origin;
  }

  // ---- preview (the Rust solver) ----
  async function preview() {
    const my = ++S.tick;
    const values = {};
    for (const [k, v] of Object.entries(S.values)) values[k] = v;
    try {
      const pv = await Backend.call("family_preview", { id: S.id, type: S.typeId, values, body: S.body, params: S.params });
      if (my !== S.tick) return true;
      if (pv.error) { S.pvError = pv.error; return false; }
      S.pv = pv; S.pvError = null;
      S.owners = {};
      for (const k of ["forms", "syms", "model_lines"]) for (const x of pv[k] || []) if (x.owner != null) S.owners[x.id] = x.owner;
      return true;
    } catch (e) { S.pvError = String(e.message || e); return false; }
  }

  // ---- the shell ----
  // The editor is a document in the main window: its own ribbon, Properties, Project Browser and view tabs
  // (index.html #fam-*; main.js setDoc switches between it and the project). Only one family is open for now.
  const $q = (s) => document.querySelector(s);
  let wired = false;

  function mount() {
    const canvas = $q("#fam-canvas");
    Object.assign(S, { canvas, ctx: canvas.getContext("2d"), right: $q("#fam-prop-body"), opts: $q("#fam-options") });
    if (wired) return;
    wired = true;
    new ResizeObserver(resize).observe(canvas);
    canvas.addEventListener("pointerdown", onDown);
    canvas.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerup", onUp);
    canvas.addEventListener("dblclick", onDbl);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("contextmenu", (e) => e.preventDefault());
    window.addEventListener("keydown", keys, true);
  }

  function resize() {
    if (!S || !S.canvas) return;
    const r = S.canvas.getBoundingClientRect();
    if (r.width < 10 || r.height < 10) return; // hidden while the project is showing
    S.canvas.width = Math.max(300, Math.round(r.width)); S.canvas.height = Math.max(200, Math.round(r.height));
    render();
  }

  function keys(e) {
    if (!S || app.doc !== "family" || document.querySelector(".modal-back")) return;
    if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "SELECT" || e.target.tagName === "TEXTAREA")) return;
    onKey(e);
    e.stopPropagation();
  }

  // Bring the family document to the front (after opening it, or switching back from the project)
  function activate() {
    if (!S) return;
    setDoc("family");
    resize();
    tabsUI(); toolbar(); props(); status();
  }

  function closeEditor(force) {
    if (!S) return;
    if (!force && isDirty()) { askSave(); return; }
    S = null;
    setDoc("project");
    refresh(true);
  }

  // Closing with changes: the project copy and the family file are separate, so each can be saved on its own.
  // `done(closed)` hears how it ended: true once the editor has closed, false when the person stays.
  function askSave(done = () => {}) {
    activate();
    openModal(`Close ${S.def.name}`, (body, foot, close) => {
      const gaps = [];
      if (S.projDirty) gaps.push("loaded into the project");
      if (S.fileDirty) gaps.push(S.sourcePath ? "saved to its family file" : "saved to a family file");
      body.append(el("div", "modal-msg", `This family has changes that are not ${gaps.join(" and not ")}.`));
      const stay = () => { close(); done(false); };
      body.closest(".modal-back").onEscape = stay;
      foot.append(button("Cancel", stay), button("Don't Save", () => { close(); closeEditor(true); done(true); }));
      if (S.fileDirty) foot.append(button(S.sourcePath ? "Save Family File" : "Save Family File...", async () => { close(); if (await saveFile(false)) { if (S.projDirty) askSave(done); else { closeEditor(true); done(true); } } else done(false); }, !S.projDirty));
      if (S.projDirty) foot.append(button("Load into Project and Close", async () => { close(); await loadIntoProject(true); done(!S); }, true));
    });
  }
  // Closing as the program closes: resolves true once the editor is closed, false when the person stays
  function closeForExit() {
    if (!S) return Promise.resolve(true);
    if (!isDirty()) { closeEditor(true); return Promise.resolve(true); }
    return new Promise((resolve) => askSave(resolve));
  }

  // Write the editor's state into this project's copy of the family (not into any file).
  async function commitToProject() {
    try {
      await Backend.call("set_family_def", { id: S.id, params: S.params, body: S.body, settings: S.settings });
      for (const t of S.types) {
        const changed = {};
        for (const [k, v] of Object.entries(S.saveVals || {})) if (t.id === S.typeId) changed[k] = v;
        if (Object.keys(changed).length) await Backend.call("patch_family_type", { id: t.id, patch: { values: changed } });
      }
      S.saveVals = {}; S.projDirty = false; S.msg = "";
      await refresh(true);
      return true;
    } catch (e) { S.msg = String(e.message || e); status(); return false; }
  }

  // Save: write the family file. A family with no file yet (or Save As) asks where. The project's copy is not touched.
  async function saveFile(asNew) {
    const safe = (S.def.name || "Family").replace(/[\\/:*?"<>|]/g, "_");
    const write = async (path) => {
      const values = S.typeId && S.saveVals && Object.keys(S.saveVals).length ? { [S.typeId]: S.saveVals } : null;
      const r = await Backend.call("save_family_file", { id: S.id, path, params: S.params, body: S.body, settings: S.settings, values, loaded_at: today() });
      S.sourcePath = r.path; S.fileDirty = false; S.msg = `Saved ${r.path}`;
      await refresh(true);
      status();
      return true;
    };
    try {
      if (S.sourcePath && !asNew) {
        try { return await write(S.sourcePath); } catch (e) { S.msg = `${String(e.message || e)} Choose another place.`; status(); }
      }
      const path = await Backend.pickSaveFamily(S.sourcePath || `${safe}.oaf`);
      if (!path) return false;
      return await write(path);
    } catch (e) { S.msg = String(e.message || e); status(); return false; }
  }

  // Load into Project: the project's copy updates first. The family file is then offered the same changes.
  // Saying no keeps them in this project only, so one project can use a customised copy of a shared family.
  async function loadIntoProject(close) {
    if (!(await commitToProject())) return;
    const name = S.def.name;
    if (S.fileDirty) await offerFileSave();
    if (close) closeEditor(true); else setDoc("project");
    await refresh(true);
    setPrompt(`Loaded ${name} into the project.`);
  }

  function offerFileSave() {
    return new Promise(async (resolve) => {
      let st = { source_path: null, found: false };
      try { st = await Backend.call("family_file_status", { id: S.id }); } catch { /* treated as no file */ }
      const path = st.source_path;
      let settled = false;
      const settle = resolve;
      resolve = (v) => { settled = true; settle(v); };
      openModal(path ? (st.found ? "Overwrite Family File" : "Family File Not Found") : "Save Family File", (body, foot, close) => {
        const done = (v) => { close(); resolve(v); };
        body.append(el("div", "modal-msg", path
          ? (st.found ? `${S.def.name} was loaded from ${path}. Overwrite that file with these changes?` : `${S.def.name} was loaded from ${path}, which cannot be found now. Save the family to a file?`)
          : `${S.def.name} has no family file yet. Save it to a file as well?`),
          el("div", "modal-msg", "Choosing project only keeps the changes in this project. Other projects that use the file are not touched."));
        foot.append(button("Project Only", () => { S.fileDirty = false; done(false); }));
        foot.append(button(path && st.found ? "Overwrite File" : "Save As...", async () => { close(); const ok = await saveFile(!(path && st.found)); if (!ok) S.fileDirty = false; resolve(ok); }, true));
      });
      // Esc closes a dialog without a button: count that as "project only" so Load into Project goes on
      const back = [...document.querySelectorAll(".modal-back")].pop();
      const watch = new MutationObserver(() => { if (!back.isConnected) { watch.disconnect(); if (!settled) { S.fileDirty = false; resolve(false); } } });
      watch.observe(document.body, { childList: true });
    });
  }

  // ---- ribbon commands ----
  const TOOL_OF = {
    "f-select": "select", "f-plane": "plane", "f-dim": "dim", "f-align": "align", "f-ext": "ext", "f-void": "void",
    "f-blend": "blend", "f-revolve": "revolve", "f-sweep": "sweep", "f-swblend": "swblend", "f-sym": "sym", "f-region": "region",
    "f-mline": "mline", "f-control": "control", "f-mtext": "mtext", "f-comp": "comp",
    "f-mv": "move", "f-cp": "copy", "f-ro": "rotate", "f-of": "offset",
  };
  function setTool(t, o = {}) {
    // in sketch mode only the sketch's own tools work, until Finish or Cancel
    if (S.sk) { if (t !== S.tool) { S.msg = "Finish or cancel the sketch first."; status(); } return; }
    S.voidNext = false; S.openingNext = false;
    S.tool = t; S.draft = null; S.alignRef = null; S.blend = null; S.rev = null; S.picks = []; S.mod = { pts: [] };
    if (t === "comp") loadComps();
    // every form is sketched: Extrusion and Void Extrusion, Blend, Revolve, Sweep and Swept Blend (and their voids)
    if (["ext", "void", "blend", "revolve", "sweep", "swblend"].includes(t) && !S.editing) { startFormSketch({ kind: t, isVoid: !!o.isVoid }); return; }
    toolbar(); status(); render(); ctxRefresh();
  }
  const ctxRefresh = () => { if (typeof renderFamContext === "function") renderFamContext(); };
  // the Modify tab's commands and the sketch tool each one picks while a sketch is open
  const SK_MODIFY = { "f-select": "select", "f-align": "align", "f-mv": "move", "f-cp": "copy", "f-ro": "rotate", "f-mi-pick": "mirror", "f-mi-draw": "mirror", "f-of": "offset", "f-del": "delete" };
  const commands = {
    "f-props": () => toggleProperties(),
    "f-types": () => typesDialog(),
    // Void Forms: the same five forms, made as voids
    "f-void-ext": () => setTool("void"),
    "f-void-blend": () => setTool("blend", { isVoid: true }),
    "f-void-revolve": () => setTool("revolve", { isVoid: true }),
    "f-void-sweep": () => setTool("sweep", { isVoid: true }),
    "f-void-swblend": () => setTool("swblend", { isVoid: true }),
    "f-opening": () => { setTool("void"); S.openingNext = true; toolbar(); status(); },
    "f-conn-electrical": () => { S.connKind = "electrical"; setTool("conn"); },
    "f-conn-duct": () => { S.connKind = "duct"; setTool("conn"); },
    "f-conn-pipe": () => { S.connKind = "pipe"; setTool("conn"); },
    "f-conn-cable_tray": () => { S.connKind = "cable_tray"; setTool("conn"); },
    "f-conn-conduit": () => { S.connKind = "conduit"; setTool("conn"); },
    "f-category": () => categoryDialog(),
    "f-sharedparams": () => sharedParamsDialog(null),
    "f-lookup": () => lookupDialog({
      tables: () => S.body.lookup_tables || [],
      set: async (next) => { const ok = await change((b) => { b.lookup_tables = next; }); return ok ? null : S.msg || "That does not solve."; },
    }),
    "f-del": () => deleteSel(),
    "f-mi-pick": () => { S.mirrorPick = true; setTool("mirror"); },
    "f-mi-draw": () => { S.mirrorPick = false; setTool("mirror"); },
    "f-eq": () => toggleEq(), // kept for keyboard and old ribbons; EQ is on the dimension itself
    "f-load": () => loadIntoProject(false),
    "f-loadclose": () => loadIntoProject(true),
    "f-3d": () => openFamView("3d"),
    "f-names": () => { S.show.names = !S.show.names; tabsUI(); render(); },
    "f-dims": () => { S.show.dims = !S.show.dims; tabsUI(); render(); },
    "f-wpset": () => setWorkPlane(),
    "f-wpshow": () => { S.showWp = !S.showWp; syncRibbon(); S.msg = S.showWp ? `Showing the work plane: ${(planeById(S.view === "3d" ? S.wps["3d"] ?? VIEWS.plan.origin : workPlane()) || {}).name}.` : ""; status(); render(); },
    "f-wpviewer": () => viewerCmd(),
  };
  for (const [id, t] of Object.entries(TOOL_OF)) commands[id] = () => setTool(t);
  // With a form's sketch open the Modify tab's tools work on the sketch lines, as the sketch's own tools
  for (const [id, t] of Object.entries(SK_MODIFY)) {
    const f = commands[id];
    commands[id] = (e) => {
      if (!skActive()) return f(e);
      // Delete takes the selected lines out at once, or with none selected waits for a click on one
      if (t === "delete" && FSk.selectedCount) { FSk.key({ key: "Delete", preventDefault() {} }); ctxRefresh(); return; }
      if (id === "f-mi-pick" || id === "f-mi-draw") FSk.setOpts({ mirrorMode: id === "f-mi-pick" ? "pick" : "draw" });
      FSk.setTool(t); ctxRefresh();
    };
  }
  for (const k of Object.keys(commands)) { const f = commands[k]; commands[k] = (e) => (S ? f(e) : undefined); }

  function syncRibbon() {
    document.querySelectorAll("#ribbon button.cmd[data-id^='f-']").forEach((b) => {
      const id = b.dataset.id;
      b.classList.toggle("on", S ? (TOOL_OF[id] ? TOOL_OF[id] === S.tool : id === "f-mi-pick" || id === "f-mi-draw" ? S.tool === "mirror" && !!S.mirrorPick === (id === "f-mi-pick") : (id === "f-names" ? S.show.names : id === "f-dims" ? S.show.dims : id === "f-wpshow" ? S.showWp : id === "f-void-menu" ? ((S.tool === "void" && !S.openingNext) || !!S.voidNext) : id === "f-opening" ? !!S.openingNext : id === "f-conn" ? S.tool === "conn" : false)) : false);
    });
  }

  // ---- options bar: what the active tool needs ----
  function toolbar() {
    syncRibbon();
    const bar = S.opts;
    bar.replaceChildren();
    if (S.tool === "sym" || S.tool === "mline") {
      const mode = el("select");
      [["line", "Line"], ["arc", "3-Point Arc"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; mode.append(o); });
      mode.value = S.symMode; mode.onchange = () => { S.symMode = mode.value; S.draft = null; status(); render(); };
      bar.append(el("span", "fe-opt", "Draw"), mode);
      if (S.tool === "mline") bar.append(el("span", "fe-opt", "Work plane"), planeSel());
    }
    if (S.tool === "comp") {
      const fams = S.comps || [];
      if (!fams.length) bar.append(el("span", "fe-opt", "No other families in the project to place. Load one into the project first."));
      else {
        const pick = fams.find((f) => f.name === (S.compPick && S.compPick.family)) || fams[0];
        const type = pick.types.includes(S.compPick && S.compPick.type) ? S.compPick.type : pick.types[0] || "";
        S.compPick = { family: pick.name, type };
        const fs = selIn(fams.map((f) => [f.name, f.name]), pick.name, (v) => { S.compPick = { family: v, type: "" }; toolbar(); });
        const ts = selIn(pick.types.map((t) => [t, t]), type, (v) => { S.compPick.type = v; });
        bar.append(el("span", "fe-opt", "Family"), fs, el("span", "fe-opt", "Type"), ts, el("span", "fe-opt", "Work plane"), planeSel());
      }
    }
    if (S.tool === "mtext") bar.append(el("span", "fe-opt", "Work plane"), planeSel());
    if (S.tool === "copy") { const l = el("label", "fe-opt", "Multiple"); l.prepend(checkIn(!!S.copyMany, (v) => { S.copyMany = v; })); bar.append(l); }
    if (S.tool === "rotate") {
      const l = el("label", "fe-opt", "Place centre"); l.title = "Click where to turn about first (otherwise the middle of the selection)";
      l.prepend(checkIn(!!S.rotPlace, (v) => { S.rotPlace = v; S.mod = { pts: [] }; render(); }));
      const ang = el("input"); ang.type = "text"; ang.style.width = "60px"; ang.placeholder = "degrees";
      ang.onkeydown = (e) => { if (e.key === "Enter") { const d = parseFloat(ang.value); if (Number.isFinite(d) && d) applyXf({ type: "rotate", cu: rotCentre().u, cv: rotCentre().v, deg: d }, false); } e.stopPropagation(); };
      bar.append(l, el("span", "fe-opt", "Angle"), ang);
    }
    if (S.tool === "mirror") bar.append(el("span", "fe-opt", "Axis"), selIn([["pick", "Pick Axis (a reference plane)"], ["draw", "Draw Axis"]], S.mirrorPick ? "pick" : "draw", (v) => { S.mirrorPick = v === "pick"; S.mod = { pts: [] }; syncRibbon(); status(); render(); }), (() => { const l = el("label", "fe-opt", "Copy"); l.prepend(checkIn(S.mirrorCopy !== false, (v) => { S.mirrorCopy = v; })); return l; })());
    if (S.tool === "offset") {
      const d = el("input"); d.type = "text"; d.style.width = "70px"; d.value = String(S.offsetDist || 100);
      d.onchange = () => { const v = parseFloat(d.value); if (v > 0) S.offsetDist = v; else d.value = String(S.offsetDist || 100); };
      d.onkeydown = (e) => e.stopPropagation();
      const l = el("label", "fe-opt", "Copy"); l.prepend(checkIn(!!S.offsetCopy, (v) => { S.offsetCopy = v; }));
      bar.append(el("span", "fe-opt", "Offset"), d, l);
    }
    if (S.tool === "control") bar.append(el("span", "fe-opt", "Control"), selIn(CONTROL_KINDS, S.ctrlKind, (v) => { S.ctrlKind = v; }));
    if (S.tool === "conn") bar.append(el("span", "fe-opt", "Connector"), selIn(Object.entries(CONN).map(([k, c]) => [k, c.label]), S.connKind, (v) => { S.connKind = v; status(); }), el("span", "fe-opt", "Face (work plane)"), planeSel());
    if (S.tool === "sweep" || S.tool === "swblend") sweepBar(bar);
    if (S.tool === "align") {
      const o = S.alignOpts;
      const lab = (text, tip, key) => { const l = el("label", "fe-opt", text); l.title = tip; l.prepend(checkIn(o[key], (v) => { o[key] = v; })); return l; };
      bar.append(
        lab("Multiple Alignment", "Keep the same reference for several edges (holding Ctrl does the same for one pick)", "multiple"),
        lab("Lock", "Tie the edge to the reference plane so it follows it when the plane moves", "lock"),
        el("span", "fe-opt", "Prefer"), selIn([["planes", "Reference planes"], ["geometry", "Geometry faces"]], o.prefer, (v) => { o.prefer = v; }),
        el("span", "fe-opt", "Pattern"), selIn([["entire", "Entire Surface"]], "entire", () => {}),
      );
    }
    if (["sym", "region", "mline", "control"].includes(S.tool)) bar.append(lockOpt());
    if (S.sk) {
      // sketch mode: the depth and work plane of the form, Lock, then the sketch tool's own options
      const st = el("input"), en = el("input");
      st.type = en.type = "text"; st.style.width = en.style.width = "60px"; st.id = "fe-sk-start"; en.id = "fe-sk-end";
      const k = S.sk.kind, rev = k === "revolve", path = k === "sweep" || k === "swblend";
      st.value = rev ? S.sk.a0 : S.sk.start; en.value = rev ? S.sk.a1 : S.sk.end;
      st.onchange = () => { if (rev) S.sk.a0 = st.value; else S.sk.start = st.value; }; en.onchange = () => { if (rev) S.sk.a1 = en.value; else S.sk.end = en.value; };
      const wp = el("select");
      const axis = (planeById(S.sk.plane) || {}).axis;
      S.body.planes.filter((p) => p.axis === axis).forEach((p) => { const o = el("option", "", p.name); o.value = p.id; wp.append(o); });
      wp.value = S.sk.plane; wp.onchange = () => { S.sk.plane = Number(wp.value); };
      const depth = path ? [] : [el("span", "fe-opt", rev ? "Start angle" : k === "blend" ? "First end" : "Start"), st, el("span", "fe-opt", rev ? "End angle" : k === "blend" ? "Second end" : "End"), en];
      bar.append(el("span", "fe-opt", "Work plane"), wp, ...depth, lockOpt(), ...sketchBarKids(FSk));
      bar.hidden = false;
      return;
    }
    if (S.editing && S.tool === "ext") {
      const sk = el("select");
      [["rect", "Rectangle"], ["circle", "Circle"], ["poly", "Polygon"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; sk.append(o); });
      sk.value = S.sketch; sk.onchange = () => { S.sketch = sk.value; S.draft = null; };
      bar.append(el("span", "fe-opt", "Draw loop"), sk);
    } else if (S.tool === "ext" || S.tool === "void" || S.tool === "blend" || S.tool === "revolve") {
      const sk = el("select");
      [["rect", "Rectangle"], ["circle", "Circle"], ["poly", "Polygon"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; sk.append(o); });
      sk.value = S.sketch; sk.onchange = () => { S.sketch = sk.value; S.draft = null; };
      const st = el("input"), en = el("input");
      st.type = en.type = "text"; st.style.width = en.style.width = "60px";
      st.value = S.start; en.value = S.end;
      st.onchange = () => { S.start = st.value; }; en.onchange = () => { S.end = en.value; };
      const wp = el("select");
      const axis = VIEWS[S.view] ? VIEWS[S.view].n : "z";
      S.body.planes.filter((p) => p.axis === axis).forEach((p) => { const o = el("option", "", p.name); o.value = p.id; wp.append(o); });
      wp.value = workPlane(); wp.onchange = () => { S.wps[S.view] = Number(wp.value); };
      if (S.tool === "revolve") {
        const a0 = el("input"), a1 = el("input");
        a0.type = a1.type = "text"; a0.style.width = a1.style.width = "50px";
        a0.value = S.a0; a1.value = S.a1;
        a0.onchange = () => { S.a0 = a0.value; }; a1.onchange = () => { S.a1 = a1.value; };
        bar.append(el("span", "fe-opt", "Draw"), sk, el("span", "fe-opt", "Work plane"), wp, el("span", "fe-opt", "Start angle"), a0, el("span", "fe-opt", "End angle"), a1);
      } else bar.append(el("span", "fe-opt", "Draw"), sk, el("span", "fe-opt", "Work plane"), wp, el("span", "fe-opt", S.tool === "blend" ? "Base at" : "Start"), st, el("span", "fe-opt", S.tool === "blend" ? "Top at" : "End"), en);
    }
    if (["ext", "void", "blend", "revolve", "sweep", "swblend"].includes(S.tool)) bar.append(lockOpt());
    bar.hidden = !bar.children.length;
  }
  function planeSel() {
    const axis = VIEWS[S.view] ? VIEWS[S.view].n : "z";
    const wp = el("select");
    S.body.planes.filter((p) => p.axis === axis).forEach((p) => { const o = el("option", "", p.name); o.value = p.id; wp.append(o); });
    wp.value = workPlane(); wp.onchange = () => { S.wps[S.view] = Number(wp.value); };
    return wp;
  }
  function lockOpt() {
    const l = el("label", "fe-opt", "Lock to planes");
    l.title = "Points drawn on a reference plane are tied to it. Off draws them free; the padlock on the finished edge locks it later.";
    l.prepend(checkIn(S.lockNew, (v) => { S.lockNew = v; }));
    return l;
  }

  function sweepBar(bar) {
    const mode = el("select");
    [["line", "Line"], ["arc", "3-Point Arc"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; mode.append(o); });
    mode.value = S.pathMode; mode.onchange = () => { S.pathMode = mode.value; if (S.draft) S.draft.arcEnd = null; status(); render(); };
    const cl = el("label", "fe-opt", "Closed path"); cl.prepend(checkIn(S.tool === "swblend" ? false : S.pathClosed, (v) => { S.pathClosed = v; }));
    if (S.tool === "swblend") cl.hidden = true;
    const axis = VIEWS[S.view] ? VIEWS[S.view].n : "z";
    const wp = el("select");
    S.body.planes.filter((p) => p.axis === axis).forEach((p) => { const o = el("option", "", p.name); o.value = p.id; wp.append(o); });
    wp.value = workPlane(); wp.onchange = () => { S.wps[S.view] = Number(wp.value); };
    bar.append(el("span", "fe-opt", "Path"), mode, cl, el("span", "fe-opt", "Work plane"), wp);
  }

  // ---- views: tabs, the Project Browser and the view bar ----
  const viewLabel = (id) => (id === "3d" ? "View 1" : VIEWS[id].label);
  function openFamView(id) {
    if (!S.open.includes(id)) S.open.push(id);
    S.view = id; S.draft = null; S.picks = []; S.alignRef = null;
    tabsUI(); toolbar(); status(); render();
  }
  function closeFamView(id) {
    if (S.open.length <= 1) return;
    const i = S.open.indexOf(id);
    if (i < 0) return;
    S.open.splice(i, 1);
    if (S.view === id) openFamView(S.open[Math.min(i, S.open.length - 1)]);
    else tabsUI();
  }

  function tabsUI() {
    const bar = $q("#fam-vtabs");
    bar.replaceChildren(...S.open.map((id) => {
      const x = el("span", "x", "×"); x.title = "Close (middle-click also closes)";
      const tab = el("div", "vtab" + (S.view === id ? " active" : "") + (S.open.length === 1 ? " only" : ""));
      tab.append(el("span", "", viewLabel(id)), x);
      tab.title = VIEW_TITLE[id];
      tab.onclick = () => { if (S.view !== id) openFamView(id); };
      tab.onauxclick = (e) => { if (e.button === 1) { e.preventDefault(); closeFamView(id); } };
      x.onclick = (e) => { e.stopPropagation(); closeFamView(id); };
      return tab;
    }));
    const ts = $q("#fam-type-select");
    ts.replaceChildren();
    S.types.forEach((t) => { const o = el("option", "", t.name); o.value = t.id; ts.append(o); });
    ts.value = S.typeId;
    ts.onchange = () => { S.typeId = Number(ts.value); S.values = {}; S.saveVals = {}; preview().then(() => { props(); render(); }); };
    const vb = $q("#fam-viewbar");
    vb.replaceChildren();
    const tog = (key, label, tip) => {
      const b = el("button", S.show[key] ? "on" : "", label);
      b.title = tip;
      b.onclick = () => { S.show[key] = !S.show[key]; tabsUI(); syncRibbon(); render(); };
      return b;
    };
    vb.append(tog("names", "Names", "Show the names of reference planes"), tog("dims", "Dimensions", "Show the dimension behind every reference plane that follows a parameter"));
    famTree();
    syncRibbon();
  }

  function famTree() {
    const tree = $q("#fam-tree");
    const open = new Set([...tree.querySelectorAll("li.node.open > span")].map((s) => s.textContent));
    if (!tree.children.length) ["Views (all)", "Floor Plans", "3D Views", "Elevations"].forEach((n) => open.add(n));
    tree.replaceChildren();
    const leaf = (id) => ({ name: viewLabel(id), active: S.view === id, onopen: () => openFamView(id) });
    const nodes = [
      { name: "Views (all)", children: [
        { name: "Floor Plans", children: [leaf("plan")] },
        { name: "Ceiling Plans", children: [{ name: "Ref. Level", onopen: () => setPrompt("Ceiling plan views are not built yet.", true) }] },
        { name: "3D Views", children: [leaf("3d")] },
        { name: "Elevations", children: ["back", "front", "left", "right"].map(leaf) },
        { name: "Sheets", children: [] },
      ] },
      { name: "Families", children: [] },
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
          if (n.onopen) span.ondblclick = n.onopen;
        }
        parent.appendChild(li);
      }
    };
    build(nodes, tree);
    $q("#fam-browser-title").textContent = `Project Browser - ${S.def.name}`;
  }

  function status() {
    const tips = {
      select: "Click to select. Drag a reference plane or a sketch point of a selected form to move it. Click a dimension's padlock to lock it, or its EQ to equalise. Delete removes the selection.",
      plane: "Click the start of the reference plane, then click its end (or drag). The line it follows sets the direction.",
      dim: "Click two or more parallel reference planes, then click in empty space to place the dimension. Select it to label it with a parameter, lock it or equalise it (EQ).",
      ext: "Sketch the profile on the work plane. Points near a reference plane lock to it (Lock to planes in the options bar). Select the form to see a padlock on each edge that sits on a plane.",
      void: "Sketch the void. A void that cuts the host makes the opening in the wall.",
      blend: "Sketch the base (one closed loop), then Edit Top on the ribbon and sketch the top with the same number of points. Finish Edit Mode makes the blend from the first end to the second.",
      revolve: "Sketch the profile with Boundary Line, then choose Axis Line and draw the axis it turns about. Set the angles in the options bar, then Finish Edit Mode.",
      swblend: "Sketch Path: draw the path with the Draw tools (open), then Finish Edit Mode. Set the start and end profile in Properties.",
      sweep: "Sketch Path: draw the path with the Draw tools (open or closed), then Finish Edit Mode. Set the profile in Properties.",
      move: "Click a start point, then where it goes. Points near a reference plane take its position. Esc cancels.",
      copy: "Click a start point, then where the copy goes. Multiple in the options bar keeps copying. Esc finishes.",
      rotate: "Click a point on the line to turn from, then on the line to turn to (about the middle of the selection, or the centre placed first). Or type the angle in the options bar.",
      mirror: "Pick Axis: click a reference plane to mirror across. Draw Axis: click two points of the axis. Copy in the options bar keeps the original.",
      offset: "Set the distance in the options bar, then click a model line or symbolic line on the side to offset it to. Copy keeps the original.",
      align: "Click a reference plane or the edge of a form to align to, then click the edge to move to it. Lock in the options bar ties it to a reference plane; Multiple Alignment (or Ctrl) keeps the same reference.",
      sym: "Line: click points, double-click to finish. 3-Point Arc: click the start, the end, then a point on the arc. Shows only in this kind of view.",
      mline: "Model line: click points, double-click to finish. 3-Point Arc: click the start, the end, then a point on the arc. It is 3D, so it shows in every view its visibility settings allow.",
      comp: "Pick the family and type in the options bar, then click where the component goes. Its parameters can follow this family's in Properties.",
      mtext: "Click where the text starts (its baseline). Type the text, then change font, size, depth and alignment in Properties.",
      control: "Click where the flip arrows go in this view. Choose the kind in the options bar.",
      conn: `${(CONN[S.connKind] || CONN.duct).label}: click where it sits on the face (the work plane). It points out of that plane; flip it in Properties.`,
      region: "Click two corners for a filled region that shows only in this kind of view.",
      pickwp: "Click the reference plane to use as the work plane. Esc cancels.",
    };
    const err = !!S.msg && S.msg !== "Saved.";
    // the sketch's own prompts, as in the project's sketch mode
    if (app.doc === "family" && skActive()) { setPrompt(S.msgErr && S.msg ? S.msg : FSk.promptText(), !!S.msgErr); S.msgErr = false; syncTitle(); return; }
    if (app.doc === "family") setPrompt((S.msg ? S.msg + "   " : "") + (S.view === "3d" ? "Drag to orbit, right-drag or Shift-drag to pan, wheel to zoom, click to select." : tips[S.tool] || ""), err);
    syncTitle();
  }

  function syncTitle() {
    if (!S || app.doc !== "family") return;
    $q("#doc-title").textContent = `Family: ${S.def.name}${isDirty() ? " *" : ""} - ${VIEW_TITLE[S.view]}`;
    renderDocTabs();
    updateUndoButtons();
  }
  const docInfo = () => (S ? { name: S.def.name, dirty: isDirty(), path: S.sourcePath } : null);

  // ---------------------------------------------------------------- properties
  function field(label, node) { const r = el("div", "famed-row"); r.append(el("span", "", label), node); return r; }
  function textIn(value, onchange, opts = {}) {
    const i = el("input"); i.type = "text"; i.value = value ?? "";
    i.onchange = () => onchange(i.value);
    if (!opts.list) return i;
    const dl = el("datalist"); dl.id = "fe-p-" + Math.random().toString(36).slice(2);
    opts.list.forEach((o) => { const op = el("option"); op.value = o; dl.append(op); });
    i.setAttribute("list", dl.id);
    const wrap = el("span"); wrap.append(i, dl);
    return wrap;
  }
  // a material is chosen in the Material Browser (where a new one can be made); <By Category> leaves it to the subcategory
  function matIn(value, onchange) {
    const shown = value || "<By Category>";
    return pickerField(shown, async () => {
      const n = await pickMaterial(shown, ["<By Category>"]);
      if (n != null) onchange(n === "<By Category>" ? "" : n);
    });
  }
  function checkIn(value, onchange) { const i = el("input"); i.type = "checkbox"; i.checked = !!value; i.onchange = () => onchange(i.checked); return i; }
  function selIn(options, value, onchange) {
    const s = el("select");
    options.forEach(([v, t]) => { const o = el("option", "", t); o.value = v; s.append(o); });
    s.value = value; s.onchange = () => onchange(s.value); return s;
  }
  const lengthParams = () => S.params.filter((p) => p.param_kind === "length" || p.param_kind === "number").map((p) => `[${p.name}]`);

  function props() {
    const r = S.right;
    r.replaceChildren();
    const s = S.sel;
    if (skActive()) {
      r.append(el("div", "famed-hint", `${FSk.title}: draw closed loops, then Finish Edit Mode or Cancel Edit Mode in the ribbon. Lines on a reference plane show a padlock; click it to lock the line to the plane.`));
    } else if (!s) {
      r.append(el("div", "famed-hint", `Family: ${S.def.name}`));
      r.append(button("Family Category and Parameters...", () => categoryDialog()));
      for (const g of settingRows(currentSettings(), (w) => applyFamilySettings(w))) r.append(g);
    } else if (s.kind === "multi") multiProps(r, s.items);
    else if (s.kind === "dim") dimProps(r, dimById(s.id));
    else if (s.kind === "plane") planeProps(r, planeById(s.id));
    else if (s.kind === "form") formProps(r, formById(s.id));
    else if (s.kind === "sym") symProps(r, symById(s.id));
    else if (s.kind === "mline") mlineProps(r, mlineById(s.id));
    else if (s.kind === "ctrl") ctrlProps(r, ctrlById(s.id));
    else if (s.kind === "mtext") mtextProps(r, mtextById(s.id));
    else if (s.kind === "comp") compProps(r, compById(s.id));
    else if (s.kind === "conn") connProps(r, connById(s.id));
    flex(r);
    ctxRefresh();
  }

  const KIND_NAMES = { plane: "Reference Planes", form: "Forms", sym: "Symbolic Lines", mline: "Model Lines", ctrl: "Controls", conn: "Connectors", mtext: "Model Text", comp: "Components", dim: "Dimensions" };
  function multiProps(r, items) {
    r.append(el("div", "famed-hint", `Multiple (${items.length})`));
    const counts = {};
    for (const x of items) counts[x.kind] = (counts[x.kind] || 0) + 1;
    for (const [k, n] of Object.entries(counts)) r.append(field(KIND_NAMES[k] || k, el("span", "", String(n))));
    r.append(el("div", "famed-hint", "Delete removes them all. Ctrl+click adds to the selection, Shift+click takes away."));
  }

  function planeProps(r, p) {
    if (!p) return;
    r.append(el("div", "famed-hint", `Reference plane (${p.axis === "x" ? "left/right" : p.axis === "y" ? "front/back" : "level"})`));
    r.append(field("Name", textIn(p.name, (v) => change((b) => { planeById(p.id).name = v; }))));
    const REFS = {
      not_reference: "Not a Reference", strong: "Strong Reference", weak: "Weak Reference",
      left: "Left", center_left_right: "Center (Left/Right)", right: "Right",
      front: "Front", center_front_back: "Center (Front/Back)", back: "Back",
      bottom: "Bottom", center_elevation: "Center (Elevation)", top: "Top",
    };
    const named = { x: ["left", "center_left_right", "right"], y: ["front", "center_front_back", "back"], z: ["bottom", "center_elevation", "top"] }[p.axis] || [];
    r.append(field("Is reference", selIn(["not_reference", "strong", "weak", ...named].map((k) => [k, REFS[k]]), p.ref_kind || "weak", (v) => change((b) => {
      // a named reference belongs to one plane at a time
      if (named.includes(v)) for (const q of b.planes) if (q.ref_kind === v && q.id !== p.id) q.ref_kind = "weak";
      planeById(p.id).ref_kind = v;
    }, true))));
    const a = p.anchor;
    if (a.type === "origin") { r.append(el("div", "famed-hint", "This plane marks the origin.")); return; }
    const same = S.body.planes.filter((q) => q.axis === p.axis && q.id !== p.id).map((q) => [q.id, q.name]);
    const held = drivenBy(p.id);
    if (held) {
      r.append(el("div", "famed-hint", held.label ? `Positioned by the dimension labelled ${held.label}.` : held.eq ? "Positioned by an equal (EQ) dimension." : "Held by a locked dimension."));
      const go = button("Select the dimension", () => { S.sel = { kind: "dim", id: held.id }; props(); render(); });
      r.append(go);
      return;
    }
    if (a.type === "div") { r.append(el("div", "famed-hint", `Step ${a.i} of ${a.n} between two planes.`)); return; }
    if (a.type === "offset") {
      r.append(field("Measured from", selIn(same, a.from, (v) => change((b) => { planeById(p.id).anchor.from = Number(v); }))));
      r.append(field("Dimension / label", textIn(a.by, (v) => change((b) => { planeById(p.id).anchor.by = v; }), { list: lengthParams() })));
      const here = pos(p.id), from = pos(a.from);
      if (here !== undefined && from !== undefined) r.append(el("div", "famed-hint", `Distance now ${(here - from).toFixed(1)} mm`));
    } else if (a.type === "mid") {
      r.append(field("Between", selIn(same, a.a, (v) => change((b) => { planeById(p.id).anchor.a = Number(v); }))));
      r.append(field("and", selIn(same, a.b, (v) => change((b) => { planeById(p.id).anchor.b = Number(v); }))));
    }
    r.append(el("div", "famed-hint", "Type a number, a label such as [Width], or a formula such as [Width]/2 - 10."));
  }

  // ---- Associate Family Parameter: a property row that a family parameter can drive ----
  const KIND_NAME = { length: "Length", number: "Number", integer: "Integer", angle: "Angle", yes_no: "Yes/No", material: "Material", text: "Text" };
  // The family parameter a value names on its own (`Width` or `[Width]`), or null when it is a number or a formula
  function assocOf(v, kind) {
    const m = /^\s*\[?([^\[\]]+?)\]?\s*$/.exec(String(v ?? ""));
    const p = m && S.params.find((q) => q.name === m[1] && q.param_kind === kind && !q.reporting);
    return p ? p.name : null;
  }
  // A row whose value can be driven by a parameter. `apply(name, newParam)` makes the change; name is null to let go.
  function assocRow(label, kind, cur, ctrl, apply, note, seed) {
    const r = el("div", "famed-row");
    r.append(el("span", "", label));
    const box = el("span", "fe-assocbox");
    if (cur) { const ro = el("span", "fe-ro", cur); ro.title = `Driven by the family parameter ${cur}`; box.append(ro); }
    else if (note) { const ro = el("span", "fe-ro", note.text); ro.title = note.title || ""; box.append(ro); }
    else box.append(ctrl);
    const b = el("button", "fe-assoc" + (cur ? " on" : ""), "=");
    b.title = cur ? `Associated with ${cur}. Click to change.` : "Associate Family Parameter";
    b.onclick = () => assocDialog(kind, cur, apply, seed);
    box.append(b);
    r.append(box);
    return r;
  }
  function assocDialog(kind, cur, apply, seed) {
    openModal("Associate Family Parameter", (body, foot, close) => {
      body.append(el("div", "famed-hint", `Family parameters of type ${KIND_NAME[kind]}. Pick the one that drives this property, or <none>.`));
      let sel = cur || "";
      const list = el("div", "fe-assoclist");
      body.append(list);
      const ok = () => { close(); apply(sel || null); };
      const draw = () => {
        list.replaceChildren();
        const items = [["", "<none>"], ...S.params.filter((p) => p.param_kind === kind && !p.reporting).map((p) => [p.name, `${p.name}   (${p.instance ? "instance" : "type"})`])];
        for (const [v, t] of items) {
          const row = el("div", "fe-assocrow" + (v === sel ? " sel" : ""), t);
          row.onclick = () => { sel = v; draw(); };
          row.ondblclick = ok;
          list.append(row);
        }
      };
      draw();
      const group = { length: "Dimensions", yes_no: "Graphics", material: "Materials and Finishes", text: "Identity Data" }[kind] || "Other";
      foot.append(
        button("New Parameter...", () => paramProps({ kinds: [[kind, KIND_NAME[kind]]], group, value: seed }, (param) => { close(); apply(param.name, param); })),
        button("Cancel", close),
        button("OK", ok, true),
      );
    });
  }

  // The Graphics rows every piece of geometry has: Visible (a checkbox a Yes/No parameter can drive) and its Visibility Settings
  function graphicsRows(r, o, key) {
    const v = o.vis || {};
    const cur = assocOf(v.when, "yes_no");
    const cond = !cur && v.when && v.when.trim() ? { text: "(condition)", title: `Visible when ${v.when}. Edit it in Visibility Settings.` } : null;
    r.append(el("div", "famed-h", "Graphics"));
    r.append(assocRow("Visible", "yes_no", cur, checkIn(v.visible !== false, (c) => change((b) => { const t = key(b); t.vis = t.vis || {}; t.vis.visible = c; }, true)), (name, add) => change((b) => {
      if (add) S.params.push(add);
      const t = key(b); t.vis = t.vis || {}; t.vis.when = name ? `[${name}]` : null;
    }), cond, v.visible !== false));
    r.append(field("Visibility/Graphics Overrides", button("Edit...", () => visDialog(key))));
  }
  // Visibility Settings: which views and detail levels show the piece, and an optional condition
  function visDialog(key) {
    openModal("Family Element Visibility Settings", (body, foot, close) => {
      const v = { ...(key(S.body).vis || {}) };
      const get = (k) => v[k] !== false;
      body.append(el("div", "famed-h", "Display in views"));
      const mk = (k, t) => { const l = el("label", "fe-vis", t); l.prepend(checkIn(get(k), (c) => { v[k] = c; })); return l; };
      body.append(mk("plan", "Plan/RCP"), mk("front_back", "Front/Back"), mk("left_right", "Left/Right"));
      body.append(el("div", "famed-h", "Detail levels"));
      body.append(mk("coarse", "Coarse"), mk("medium", "Medium"), mk("fine", "Fine"));
      body.append(el("div", "famed-h", "Condition"));
      body.append(field("Visible when", textIn(v.when || "", (x) => { v.when = x.trim() ? x : null; }, { list: S.params.filter((p) => p.param_kind === "yes_no").map((p) => `[${p.name}]`) })));
      body.append(el("div", "famed-hint", "A Yes/No parameter, or an expression such as Width > 1000. The piece shows only while it is true."));
      foot.append(button("Cancel", close), button("OK", () => { close(); change((b) => { const t = key(b); t.vis = { ...(t.vis || {}), ...v }; }); }, true));
    });
  }

  function formProps(r, f) {
    if (!f) return;
    r.append(el("div", "famed-hint", { blend: "Blend", revolve: "Revolve", sweep: "Sweep", swept_blend: "Swept Blend" }[f.shape.type] || (f.void ? "Void extrusion" : "Extrusion")));
    r.append(field("Name", textIn(f.name, (v) => change((b) => { formById(f.id).name = v; }, true))));
    r.append(field("Void", checkIn(f.void, (v) => change((b) => { formById(f.id).void = v; }))));
    const sh = f.shape;
    if (sh.type === "blend") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      depthRows(r, f, ["Base at", "Top at"]);
    }
    if (sh.type === "revolve") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      r.append(field("Start angle", textIn(String(sh.start_deg), (v) => change((b) => { formById(f.id).shape.start_deg = Number(v) || 0; }, true))));
      r.append(field("End angle", textIn(String(sh.end_deg), (v) => change((b) => { formById(f.id).shape.end_deg = v === "" || isNaN(Number(v)) ? 360 : Number(v); }, true))));
    }
    if (sh.type === "sweep" || sh.type === "swept_blend") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      r.append(el("div", "famed-h", sh.type === "swept_blend" ? "Start profile" : "Profile"));
      r.append(el("div", "famed-hint", "Measured from the first point of the path. Across is to the left of the path, Up follows the work plane normal."));
      sweepProfile(r, f, "profile");
      if (sh.type === "swept_blend") {
        r.append(el("div", "famed-h", "End profile"));
        r.append(el("div", "famed-hint", "Measured from the last point of the path. It needs the same kind and number of points as the start profile."));
        sweepProfile(r, f, "profile_end");
      }
      r.append(field("Turn profile (deg)", textIn(String(sh.profile_angle || 0), (v) => change((b) => { formById(f.id).shape.profile_angle = Number(v) || 0; }, true))));
      r.append(field("Flip profile", checkIn(sh.flip, (v) => change((b) => { formById(f.id).shape.flip = v; }, true))));
    }
    if (sh.type === "extrusion") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      depthRows(r, f, ["Extrusion start", "Extrusion end"]);
    }
    if (f.void) {
      r.append(field("Cuts the host", checkIn(f.cuts_host !== false, (v) => change((b) => { formById(f.id).cuts_host = v; }, true))));
    } else {
      const mp = f.material_param && S.params.find((p) => p.name === f.material_param && p.param_kind === "material") ? f.material_param : null;
      r.append(assocRow("Material", "material", mp, matIn(f.material || "", (v) => change((b) => { formById(f.id).material = v; }, true)), (name, add) => change((b) => {
        if (add) S.params.push(add);
        formById(f.id).material_param = name;
      })));
      const voids = S.body.forms.filter((x) => x.void);
      if (voids.length) {
        r.append(el("div", "famed-h", "Cut by"));
        for (const v of voids) {
          const l = el("label", "fe-vis", v.name || `Void ${v.id}`);
          l.prepend(checkIn((f.cut_by || []).includes(v.id), (c) => change((b) => { const t = formById(f.id); t.cut_by = (t.cut_by || []).filter((x) => x !== v.id); if (c) t.cut_by.push(v.id); }, true)));
          r.append(l);
        }
      }
    }
    r.append(field("Subcategory", textIn(f.subcategory || "", (v) => change((b) => { formById(f.id).subcategory = v; }, true), { list: SUBS })));
    graphicsRows(r, f, (b) => formById(f.id));
  }

  // The start and end of an extrusion or blend: a locked end shows its plane, a free one takes a number, a formula
  // or a Length parameter (Associate Family Parameter)
  function depthRows(r, f, labels) {
    ["start", "end"].forEach((which, k) => {
      const sh = f.shape, lock = sh[which + "_lock"];
      const apply = (name, add) => change((b) => {
        if (add) S.params.push(add);
        const t = formById(f.id).shape;
        t[which + "_lock"] = null;
        t[which] = name ? `[${name}]` : String(curDepth(f.id, which));
      });
      if (lock) {
        const pl = planeById(lock);
        const row = el("div", "famed-row"); row.append(el("span", "", labels[k]));
        const box = el("span", "fe-assocbox");
        const un = button("Unlock", () => change((b) => { const t = formById(f.id).shape; t[which] = String(curDepth(f.id, which)); t[which + "_lock"] = null; }));
        const ab = el("button", "fe-assoc", "="); ab.title = "Associate Family Parameter"; ab.onclick = () => assocDialog("length", null, apply, curDepth(f.id, which));
        box.append(el("span", "", `locked to ${pl ? pl.name : "a deleted plane"}`), un, ab);
        row.append(box);
        r.append(row);
      } else r.append(assocRow(labels[k], "length", assocOf(sh[which], "length"), textIn(sh[which], (v) => change((b) => { formById(f.id).shape[which] = v; }, true), { list: lengthParams() }), apply, null, curDepth(f.id, which)));
    });
  }
  // Where an end of an extrusion or blend stands now, measured from its work plane (what a number there would be)
  function curDepth(formId, which) {
    const f = formById(formId), pf = ((S.pv && S.pv.forms) || []).find((x) => x.id === formId);
    if (!pf || !pf.span || pos(f.shape.plane) === undefined) return 0;
    return Math.round((pf.span[which === "start" ? 0 : 1] - pos(f.shape.plane)) * 100) / 100;
  }

  // The profile of a sweep: loops of rectangles, circles or points. Every number is an expression
  // (a value or a formula using parameters) measured across and up from the path start.
  function sweepProfile(r, f, key = "profile") {
    const loops = f.shape[key];
    const set = (fn) => change((b) => { fn(formById(f.id).shape); });
    const lens = lengthParams();
    const kindOf = (l) => {
      if (l.type === "circle") return "circle";
      const p = l.points;
      if (p.length === 4 && !(l.bulges || []).some((x) => x) && p[0].u === p[3].u && p[1].u === p[2].u && p[0].v === p[1].v && p[2].v === p[3].v) return "rect";
      return "poly";
    };
    const rect = (u0, u1, v0, v1) => ({ type: "poly", points: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]].map(([u, v]) => ({ u, v })), bulges: [] });
    const exprIn = (label, value, onset) => r.append(field(label, textIn(String(value), (v) => onset(v.trim() === "" ? "0" : v.trim()), { list: lens })));
    loops.forEach((l, li) => {
      const kind = kindOf(l);
      const head = el("div", "famed-row");
      head.append(el("span", "", loops.length > 1 ? `Loop ${li + 1}` : "Loop"));
      const sel = selIn([["rect", "Rectangle"], ["circle", "Circle"], ["poly", "Points"]], kind, (v) => {
        if (v === kind) return;
        set((sh) => {
          const cur = sh[key][li];
          let u0 = "-25", u1 = "25", v0 = "-25", v1 = "25";
          if (kind === "rect") { u0 = cur.points[0].u; u1 = cur.points[1].u; v0 = cur.points[0].v; v1 = cur.points[2].v; }
          if (v === "circle") sh[key][li] = { type: "circle", u: "0", v: "0", r: "25" };
          else sh[key][li] = rect(u0, u1, v0, v1);
        });
      });
      head.append(sel);
      if (loops.length > 1) { const del = el("button", "", "Remove"); del.onclick = () => set((sh) => { sh[key].splice(li, 1); }); head.append(del); }
      r.append(head);
      if (kind === "rect") {
        const p = l.points;
        const put = (u0, u1, v0, v1) => set((sh) => { sh[key][li] = rect(u0, u1, v0, v1); });
        exprIn("Across from", p[0].u, (v) => put(v, p[1].u, p[0].v, p[2].v));
        exprIn("Across to", p[1].u, (v) => put(p[0].u, v, p[0].v, p[2].v));
        exprIn("Up from", p[0].v, (v) => put(p[0].u, p[1].u, v, p[2].v));
        exprIn("Up to", p[2].v, (v) => put(p[0].u, p[1].u, p[0].v, v));
      } else if (kind === "circle") {
        exprIn("Centre across", l.u, (v) => set((sh) => { sh[key][li].u = v; }));
        exprIn("Centre up", l.v, (v) => set((sh) => { sh[key][li].v = v; }));
        exprIn("Radius", l.r, (v) => set((sh) => { sh[key][li].r = v; }));
      } else {
        l.points.forEach((n, i) => {
          const row = el("div", "famed-row");
          const iu = textIn(n.u, (v) => set((sh) => { sh[key][li].points[i].u = v.trim() || "0"; }), { list: lens });
          const iv = textIn(n.v, (v) => set((sh) => { sh[key][li].points[i].v = v.trim() || "0"; }), { list: lens });
          const del = el("button", "", "x"); del.title = "Remove this point";
          del.onclick = () => { if (l.points.length > 3) set((sh) => { const q = sh[key][li]; q.points.splice(i, 1); if ((q.bulges || []).length) q.bulges.splice(i, 1); }); };
          row.append(el("span", "", `Point ${i + 1}`), iu, iv, del);
          r.append(row);
        });
        const add = el("button", "", "Add point");
        add.onclick = () => set((sh) => { const q = sh[key][li]; q.points.push({ u: "0", v: "0" }); if ((q.bulges || []).length) q.bulges.push(0); });
        r.append(add);
      }
    });
    const addLoop = el("button", "", "Add loop");
    addLoop.onclick = () => set((sh) => { sh[key].push(rect("-10", "10", "-10", "10")); });
    r.append(addLoop);
  }

  function symProps(r, y) {
    if (!y) return;
    r.append(el("div", "famed-hint", `Symbolic geometry, ${y.view} views only`));
    r.append(field("Subcategory", textIn(y.subcategory || "", (v) => change((b) => { symById(y.id).subcategory = v; }, true), { list: SUBS })));
    r.append(field("Filled", checkIn(y.filled, (v) => change((b) => { symById(y.id).filled = v; }, true))));
    r.append(field("Masking", checkIn(y.masking, (v) => change((b) => { symById(y.id).masking = v; }, true))));
    graphicsRows(r, y, (b) => symById(y.id));
  }

  function mlineProps(r, m) {
    if (!m) return;
    const wp = planeById(m.plane);
    r.append(el("div", "famed-hint", `Model line on ${wp ? wp.name : "a missing plane"}. It is 3D and shows in every view its visibility settings allow.`));
    r.append(field("Name", textIn(m.name || "", (v) => change((b) => { mlineById(m.id).name = v; }))));
    r.append(field("Subcategory", textIn(m.subcategory || "", (v) => change((b) => { mlineById(m.id).subcategory = v; }), { list: ["Model Lines", ...SUBS] })));
    graphicsRows(r, m, (b) => mlineById(m.id));
  }
  // A coordinate as text: a number, or a reference plane and an offset from it
  function coordRow(r, label, c, set, axis) {
    const pl = c.plane ? planeById(c.plane) : null;
    const opts = [["", "(free)"], ...S.body.planes.filter((p) => !axis || p.axis === axis).map((p) => [String(p.id), p.name])];
    const sel = selIn(opts, c.plane ? String(c.plane) : "", (v) => set({ plane: v ? Number(v) : null, off: c.off || 0 }));
    const off = textIn(String(c.off || 0), (v) => set({ plane: c.plane || null, off: num(v) }));
    const w = el("span", "fe-coord"); w.append(sel, off);
    r.append(field(label + (pl ? "" : ""), w));
  }
  function mtextProps(r, t) {
    if (!t) return;
    const wp = planeById(t.plane);
    const set = (fn) => change((b) => { fn(mtextById(t.id)); });
    r.append(el("div", "famed-hint", `Model text on ${wp ? wp.name : "a missing plane"}. It stands out from the plane by its depth.`));
    r.append(field("Name", textIn(t.name || "", (v) => set((x) => { x.name = v; }))));
    const tp = t.text_param && S.params.find((p) => p.name === t.text_param && p.param_kind === "text") ? t.text_param : null;
    const box = el("textarea"); box.value = t.text || ""; box.rows = 2; box.onchange = () => set((x) => { x.text = box.value; });
    r.append(assocRow("Text", "text", tp, box, (name, add) => change((b) => { if (add) S.params.push(add); mtextById(t.id).text_param = name; })));
    r.append(field("Font", textIn(t.font || "", (v) => set((x) => { x.font = v; }), { list: ["DejaVu Sans"] })));
    r.append(field("Text size", textIn(t.size, (v) => set((x) => { x.size = v; }), { list: lengthParams() })));
    r.append(el("div", "famed-hint", "Text size is the height of a capital letter."));
    r.append(field("Depth", textIn(t.depth, (v) => set((x) => { x.depth = v; }), { list: lengthParams() })));
    r.append(field("Bold", checkIn(t.bold, (v) => set((x) => { x.bold = v; }))));
    r.append(field("Italic", checkIn(t.italic, (v) => set((x) => { x.italic = v; }))));
    r.append(field("Horizontal align", selIn([["left", "Left"], ["center", "Center"], ["right", "Right"]], t.align || "left", (v) => set((x) => { x.align = v; }))));
    const tax = wp ? sketchOf(wp.axis) : [];
    coordRow(r, "Horizontal", t.u, (v) => change((b) => { mtextById(t.id).u = v; }), tax[0]);
    coordRow(r, "Vertical", t.v, (v) => change((b) => { mtextById(t.id).v = v; }), tax[1]);
    const mp = t.material_param && S.params.find((p) => p.name === t.material_param && p.param_kind === "material") ? t.material_param : null;
    r.append(assocRow("Material", "material", mp, matIn(t.material || "", (v) => set((x) => { x.material = v; })), (name, add) => change((b) => { if (add) S.params.push(add); mtextById(t.id).material_param = name; })));
    r.append(field("Subcategory", textIn(t.subcategory || "", (v) => set((x) => { x.subcategory = v; }), { list: SUBS })));
    graphicsRows(r, t, (b) => mtextById(t.id));
  }
  // A component's text-like values are stored as expressions: a [Parameter] or a quoted string
  const litShow = (v) => { const m = /^\s*"(.*)"\s*$/.exec(v || ""); return m ? m[1] : v || ""; };
  const litStore = (v) => (/^\s*\[[^\[\]]+\]\s*$/.test(v) ? v : JSON.stringify(v));
  function compProps(r, n) {
    if (!n) return;
    const fams = S.comps || [];
    const fam = fams.find((f) => f.name === n.family);
    const set = (fn) => change((b) => { fn(compById(n.id)); });
    r.append(el("div", "famed-hint", fam ? `Component. Its own parameters can take their values from this family's.` : `The family "${n.family}" is not in the project, so this component is not drawn.`));
    r.append(field("Name", textIn(n.name || "", (v) => set((x) => { x.name = v; }))));
    const names = fams.map((f) => [f.name, f.name]);
    if (!fam) names.unshift([n.family, n.family]);
    r.append(field("Family", selIn(names, n.family, (v) => set((x) => { x.family = v; const f2 = fams.find((f) => f.name === v); x.type_name = f2 && f2.types[0] || ""; x.params = {}; x.type_param = null; }))));
    const tp = n.type_param && S.params.find((p) => p.name === n.type_param && p.param_kind === "text") ? n.type_param : null;
    const types = (fam ? fam.types : []).map((t) => [t, t]);
    r.append(assocRow("Type", "text", tp, selIn(types, n.type_name, (v) => set((x) => { x.type_name = v; })), (name, add) => change((b) => { if (add) S.params.push(add); compById(n.id).type_param = name; })));
    const wps = [["", "(none)"], ...S.body.planes.map((p) => [String(p.id), p.name])];
    r.append(field("Work plane", selIn(wps, n.work_plane ? String(n.work_plane) : "", (v) => set((x) => { x.work_plane = v ? Number(v) : null; }))));
    r.append(el("div", "famed-h", "Position"));
    [["x", "X (left/right)"], ["y", "Y (front/back)"], ["z", "Z (height)"]].forEach(([ax, label]) => coordRow(r, label, n.origin[AXI[ax]], (v) => change((b) => { compById(n.id).origin[AXI[ax]] = v; }), ax));
    r.append(field("Rotation (degrees)", textIn(String(n.rotation_deg || 0), (v) => set((x) => { x.rotation_deg = num(v); }))));
    r.append(field("Flip", checkIn(n.flip, (v) => set((x) => { x.flip = v; }))));
    r.append(field("Mirror", checkIn(n.mirror, (v) => set((x) => { x.mirror = v; }))));
    const ps = fam ? fam.params : [];
    if (ps.length) {
      r.append(el("div", "famed-h", "Parameters"));
      r.append(el("div", "famed-hint", "Blank keeps the component's own value."));
    }
    for (const p of ps) {
      const kind = p.kind;
      const cur = (n.params || {})[p.name] || "";
      const lit = kind === "text" || kind === "material";
      const apply = (name, add) => change((b) => {
        if (add) S.params.push(add);
        const c = compById(n.id); c.params = c.params || {};
        if (name) c.params[p.name] = `[${name}]`; else delete c.params[p.name];
      });
      const store = (v) => set((x) => { x.params = x.params || {}; if (!String(v).trim()) delete x.params[p.name]; else x.params[p.name] = lit ? litStore(v) : v; });
      const ctrl = kind === "yes_no" ? selIn([["", "(its own)"], ["true", "Yes"], ["false", "No"]], cur.trim(), store) : textIn(lit ? litShow(cur) : cur, store, { list: kind === "length" ? lengthParams() : [] });
      const assocKind = KIND_NAME[kind] ? kind : null;
      if (assocKind) r.append(assocRow(p.name + (p.instance ? "" : " (type)"), assocKind, assocOf(cur, assocKind), ctrl, apply));
      else r.append(field(p.name, ctrl));
    }
    graphicsRows(r, n, (b) => compById(n.id));
  }
  function ctrlProps(r, c) {
    if (!c) return;
    r.append(el("div", "famed-hint", `Control, ${c.view} views only. It shows the flip arrows in the project.`));
    r.append(field("Kind", selIn(CONTROL_KINDS, c.kind, (v) => change((b) => { ctrlById(c.id).kind = v; }))));
    coordRow(r, "Horizontal", c.u, (v) => change((b) => { ctrlById(c.id).u = v; }), VIEWS[c.view].h);
    coordRow(r, "Vertical", c.v, (v) => change((b) => { ctrlById(c.id).v = v; }), VIEWS[c.view].v);
    graphicsRows(r, c, (b) => ctrlById(c.id));
  }
  function connProps(r, c) {
    if (!c) return;
    const def = CONN[c.kind] || CONN.duct, wp = planeById(c.plane);
    r.append(el("div", "famed-hint", `${def.label} on ${wp ? wp.name : "a missing plane"}`));
    r.append(field("Name", textIn(c.name || "", (v) => change((b) => { connById(c.id).name = v; }))));
    r.append(field("Flip direction", checkIn(c.flip, (v) => change((b) => { connById(c.id).flip = v; }))));
    const cax = wp ? sketchOf(wp.axis) : [];
    coordRow(r, "Horizontal", c.u, (v) => change((b) => { connById(c.id).u = v; }), cax[0]);
    coordRow(r, "Vertical", c.v, (v) => change((b) => { connById(c.id).v = v; }), cax[1]);
    const showShape = c.kind === "duct" ? (c.props.Shape || "Round") : null;
    const hidden = (name) => (showShape === "Round" && (name === "Width" || name === "Height")) || (showShape && showShape !== "Round" && name === "Diameter");
    for (const [name, kind, , choices] of def.props) {
      if (hidden(name)) continue;
      const cur = c.props[name] ?? "";
      const set = (v) => change((b) => { connById(c.id).props[name] = v; });
      r.append(field(name, kind === "choice" ? selIn(choices.map((x) => [x, x]), cur, set) : textIn(cur, set, { list: lengthParams() })));
    }
    const got = ((S.pv && S.pv.connectors) || []).find((q) => q.id === c.id);
    if (got) for (const [k, v] of Object.entries(got.numbers)) if (!hidden(k) && !/^\d+(\.\d+)?$/.test(String(c.props[k]).trim())) r.append(field(k + " (worked out)", el("span", "", String(Math.round(v * 100) / 100))));
  }

  // the flex panel: values of the parameters for the chosen type, to try sizes without saving
  function flex(r) {
    r.append(el("div", "famed-h", "Flex (try sizes)"));
    const t = S.types.find((x) => x.id === S.typeId);
    for (const p of S.params) {
      if (p.formula || p.reporting) continue;
      if (!["length", "number", "integer", "angle", "yes_no", "text"].includes(p.param_kind)) continue;
      const cur = S.values[p.name] ?? (t && t.values[p.name] !== undefined ? t.values[p.name] : p.default);
      let inp;
      if (p.param_kind === "yes_no") inp = checkIn(cur, (c) => setFlex(p, c));
      else if (p.choices && p.choices.length) inp = selIn(p.choices.map((c) => [c, c]), cur, (v) => setFlex(p, v));
      else { inp = textIn(String(cur), (v) => setFlex(p, p.param_kind === "text" ? v : num(v))); }
      r.append(field(p.name + (p.instance ? " (inst.)" : ""), inp));
    }
    const rep = (S.pv && S.pv.reports) || [];
    if (rep.length) {
      r.append(el("div", "famed-h", "Reported by dimensions"));
      for (const q of rep) r.append(field(q.name, el("span", "", `${Math.round(q.value * 10) / 10} mm`)));
    }
    r.append(button("Apply to this type", async () => {
      S.saveVals = { ...(S.saveVals || {}) };
      for (const [k, v] of Object.entries(S.values)) { const d = S.params.find((p) => p.name === k); if (d && !d.instance) S.saveVals[k] = v; }
      touch(); S.msg = "Type values are written when you save."; status();
    }));
  }
  async function setFlex(p, v) {
    const before = snapshot();
    S.values[p.name] = v;
    const ok = await preview();
    if (!ok) { S.msg = S.pvError; delete S.values[p.name]; await preview(); } else { S.msg = ""; S.undo.push(before); S.redo = []; }
    status(); render(); props();
  }

  // The category and host are kept by the project and change at once (refused while instances are placed).
  async function setMeta(patch) {
    try {
      await Backend.call("set_family_meta", { id: S.id, ...patch });
      if (patch.category) { S.def.category = patch.category; const c = (S.def.categories || []).find((x) => x.id === patch.category); S.def.category_label = c ? c.label : patch.category; const tag = document.querySelector(".fe-cat"); if (tag) tag.textContent = S.def.category_label; }
      if (patch.host) S.def.host = patch.host;
      S.fileDirty = true;
      S.msg = "";
      await refresh(true);
    } catch (e) { S.msg = String(e.message || e); props(); status(); return false; }
    props(); status();
    return true;
  }

  // ---------------------------------------------------------------- family types dialog
  // Every type of the family side by side: one column per type, one row per type parameter.
  // ---------------------------------------------------------------- family types
  const GROUP_ORDER = ["Constraints", "Construction", "Graphics", "Dimensions", "Materials and Finishes", "Identity Data", "IFC Parameters", "Other"];
  const groupOf = (p) => (p.group && p.group.trim() ? p.group.trim() : "Other");
  const groupsOf = (params) => [...new Set(params.map(groupOf))].sort((a, b) => {
    const ia = GROUP_ORDER.indexOf(a), ib = GROUP_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });
  const showValue = (v) => (v === undefined || v === null ? "" : typeof v === "boolean" ? (v ? "Yes" : "No") : typeof v === "number" ? String(Math.round(v * 100) / 100) : String(v));

  // The Family Types dialog: one type at a time, parameters grouped, with value, formula and lock.
  // Everything is worked on in copies and goes into the family on Apply or OK.
  function typesDialog() {
    openModal("Family Types", (body, foot, close) => {
      let wp = clone(S.params), wb = clone(S.body);
      let renames = [];
      let work = S.types.map((t) => ({ id: t.id, name: t.name, values: { ...t.values } }));
      let gone = [];
      let cur = Math.max(0, work.findIndex((t) => t.id === S.typeId));
      let sel = null, search = "", calc = {};
      const collapsed = new Set();
      const msg = el("div", "modal-msg");
      const top = el("div", "fe-tbar"), tbl = el("div", "fe-ptable fe-gtable"), bar = el("div", "fe-tbar");
      const cols = "minmax(180px,2fr) minmax(130px,1.4fr) minmax(130px,1.4fr) 44px";
      const typeKey = () => work[cur].id || work[cur].from;

      async function recalc() {
        try {
          const r = await Backend.call("family_param_values", { id: S.id, type: typeKey(), values: work[cur].values, params: wp, body: wb });
          calc = {}; r.values.forEach((x) => { calc[x.name] = x.value; });
          msg.textContent = "";
        } catch (e) { calc = {}; msg.textContent = String(e.message || e); }
      }
      const sizesRead = (p) => p.reporting || !!p.formula;
      const valueOf = (p) => (p.instance ? p.default : work[cur].values[p.name] !== undefined ? work[cur].values[p.name] : p.default);
      const setValue = async (p, v) => {
        if (p.instance) p.default = v; else work[cur].values[p.name] = v;
        await recalc(); draw();
      };

      function valueCell(p) {
        const c = el("span");
        if (sizesRead(p)) { c.append(el("span", "fe-ro", showValue(calc[p.name]))); return c; }
        const cur0 = valueOf(p);
        let inp;
        if (p.param_kind === "yes_no") inp = checkIn(cur0, (v) => setValue(p, v));
        else if (p.choices && p.choices.length) inp = selIn(p.choices.map((x) => [x, x]), cur0, (v) => setValue(p, v));
        else if (p.param_kind === "material") inp = matIn(cur0 || "", (v) => setValue(p, v));
        else inp = textIn(showValue(cur0), (v) => setValue(p, p.param_kind === "text" ? v : num(v)));
        if (p.locked) inp.disabled = true;
        c.append(inp);
        return c;
      }

      function draw() {
        top.replaceChildren();
        const ts = selIn(work.map((w, i) => [i, w.name]), cur, async (v) => { cur = Number(v); await recalc(); draw(); });
        top.append(el("span", "fe-opt", "Type name:"), ts,
          button("New Type...", () => askText("New Type", "Type name", `Type ${work.length + 1}`, [], async (v) => {
            const n = v.trim();
            if (!n) return;
            if (work.some((w) => w.name === n)) { msg.textContent = `A type called '${n}' already exists.`; return; }
            const src = work[cur];
            work.push({ id: null, from: src.id || src.from, name: n, values: { ...src.values } });
            cur = work.length - 1; await recalc(); draw();
          })),
          button("Rename Type...", () => askText("Rename Type", "Type name", work[cur].name, [], (v) => {
            const n = v.trim();
            if (!n) return;
            if (work.some((w, i) => i !== cur && w.name === n)) { msg.textContent = `A type called '${n}' already exists.`; return; }
            work[cur].name = n; draw();
          })),
          button("Delete Type", async () => {
            if (work.length < 2) { msg.textContent = "A family needs at least one type."; return; }
            const [d] = work.splice(cur, 1);
            if (d.id) gone.push(d);
            cur = Math.max(0, cur - 1); await recalc(); draw();
          }));
        const q = el("input"); q.type = "text"; q.placeholder = "Search parameters"; q.value = search; q.style.marginLeft = "auto";
        q.oninput = () => { search = q.value; drawTable(); };
        top.append(q);
        drawTable();
      }

      function drawTable() {
        tbl.replaceChildren();
        const head = el("div", "fe-prow fe-phead"); head.style.gridTemplateColumns = cols;
        ["Parameter", "Value", "Formula", "Lock"].forEach((t) => head.append(el("span", "", t)));
        tbl.append(head);
        const needle = search.trim().toLowerCase();
        for (const g of groupsOf(wp)) {
          const rows = wp.filter((p) => groupOf(p) === g && (!needle || p.name.toLowerCase().includes(needle)));
          if (!rows.length) continue;
          const open = needle || !collapsed.has(g);
          const gh = el("div", "fe-pgroup", `${open ? "▾" : "▸"} ${g}`);
          gh.onclick = () => { collapsed.has(g) ? collapsed.delete(g) : collapsed.add(g); drawTable(); };
          tbl.append(gh);
          if (!open) continue;
          for (const p of rows) {
            const row = el("div", "fe-prow" + (sel === p.name ? " sel" : "") + (sizesRead(p) ? " ro" : "")); row.style.gridTemplateColumns = cols;
            const nm = el("span", "fe-pname", p.name + (p.instance ? " (default)" : ""));
            nm.title = p.tooltip || (p.instance ? "Instance parameter: this is the value a new copy starts with" : "Type parameter");
            nm.onclick = () => { sel = p.name; drawTable(); };
            const reads = wb.dims.some((d) => d.reports_to === p.name);
            const fm = el("span");
            if (reads) fm.append(el("span", "fe-ro", "reads a dimension"));
            else if (p.reporting && !p.formula) fm.append(el("span", "fe-ro", "reporting"));
            else {
              const f = textIn(p.formula || "", async (v) => {
                p.formula = v.trim() ? v.trim() : null;
                if (p.formula) work.forEach((w) => { delete w.values[p.name]; });
                await recalc(); draw();
              });
              if (p.locked) f.disabled = true;
              fm.append(f);
            }
            const lk = el("span"); lk.append(checkIn(p.locked, (v) => { p.locked = v; drawTable(); }));
            row.append(nm, valueCell(p), fm, lk);
            tbl.append(row);
          }
        }
      }

      const move = (dir) => {
        const i = wp.findIndex((p) => p.name === sel);
        if (i < 0) return;
        const g = groupOf(wp[i]);
        let j = i + dir;
        while (j >= 0 && j < wp.length && groupOf(wp[j]) !== g) j += dir;
        if (j < 0 || j >= wp.length) return;
        [wp[i], wp[j]] = [wp[j], wp[i]];
        drawTable();
      };
      const sortAll = (dir) => {
        const out = [];
        for (const g of groupsOf(wp)) out.push(...wp.filter((p) => groupOf(p) === g).sort((a, b) => dir * a.name.localeCompare(b.name)));
        wp = out; drawTable();
      };
      const names = () => wp.map((p) => p.name);

      bar.append(
        button("New Parameter...", () => paramProps({ kinds: KINDS, names: names(), canReport: false }, async (np) => {
          wp.push(np); sel = np.name; await recalc(); draw();
        })),
        button("Modify Parameter...", () => {
          const p = wp.find((q) => q.name === sel);
          if (!p) { msg.textContent = "Select a parameter first."; return; }
          const used = wb.dims.some((d) => d.reports_to === p.name);
          paramProps({ initial: p, kinds: KINDS, names: names(), lockKind: used || p.reporting }, async (np) => {
            try {
              if (np.name !== p.name) {
                const r = await Backend.call("family_rename_param", { params: wp, body: wb, old: p.name, new: np.name });
                wp = r.params; wb = r.body;
                renames.push([p.name, np.name]);
                for (const w of work) if (w.values[p.name] !== undefined) { w.values[np.name] = w.values[p.name]; delete w.values[p.name]; }
                sel = np.name;
              }
              const t = wp.find((q) => q.name === np.name);
              Object.assign(t, { param_kind: np.param_kind, instance: np.instance, group: np.group, discipline: np.discipline, tooltip: np.tooltip, default: np.default, shared_guid: np.shared_guid || null });
              if (np.instance) for (const w of work) delete w.values[t.name];
              await recalc(); draw();
            } catch (e) { msg.textContent = String(e.message || e); }
          });
        }),
        button("Delete Parameter", async () => {
          const p = wp.find((q) => q.name === sel);
          if (!p) { msg.textContent = "Select a parameter first."; return; }
          try {
            const r = await Backend.call("family_param_uses", { params: wp, body: wb, name: p.name });
            if (r.uses.length) { msg.textContent = `'${p.name}' is used by ${r.uses.slice(0, 4).join(", ")}${r.uses.length > 4 ? " and more" : ""}. Remove those first.`; return; }
            wp = wp.filter((q) => q !== p);
            for (const w of work) delete w.values[p.name];
            sel = null; await recalc(); draw();
          } catch (e) { msg.textContent = String(e.message || e); }
        }),
        button("↑", () => move(-1)), button("↓", () => move(1)),
        button("A→Z", () => sortAll(1)), button("Z→A", () => sortAll(-1)),
      );
      bar.children[3].title = "Move up"; bar.children[4].title = "Move down"; bar.children[5].title = "Sort ascending"; bar.children[6].title = "Sort descending";
      const lookups = button("Manage Lookup Tables...", () => lookupDialog({
        tables: () => wb.lookup_tables || [],
        set: async (next) => { wb.lookup_tables = next; try { await recalc(); draw(); } catch (e) { return String(e.message || e); } return null; },
      }));
      lookups.title = "Tables of sizes read from CSV files by size_lookup in formulas";
      lookups.style.marginLeft = "auto"; bar.append(lookups);

      body.append(top, tbl, bar, msg);

      async function apply() {
        const before = snapshot();
        S.params = clone(wp); S.body = clone(wb);
        if (!(await preview())) { restore(before); msg.textContent = S.pvError || "That does not solve."; await preview(); return false; }
        try {
          // the parameters must be in the project before types can hold values for them
          await Backend.call("set_family_def", { id: S.id, params: S.params, body: S.body, settings: S.settings, renames });
          for (const w of work) {
            let id = w.id;
            if (!id) id = (await Backend.call("duplicate_family_type", { id: w.from, name: w.name })).id;
            const old = S.types.find((t) => t.id === id);
            const patch = { values: w.values };
            if (!old || old.name !== w.name) patch.name = w.name;
            await Backend.call("patch_family_type", { id, patch });
          }
          for (const g of gone) await Backend.call("delete_family_type", { id: g.id });
        } catch (e) { msg.textContent = String(e.message || e); await reloadTypes(); return false; }
        const keep = work[cur].name;
        S.undo.push(before); S.redo = []; S.fileDirty = true;
        if (renames.length) { S.undo = []; }
        renames = []; gone = [];
        await reloadTypes();
        const t = S.types.find((x) => x.name === keep);
        if (t) { S.typeId = t.id; S.values = {}; tabsUI(); await preview(); }
        work = S.types.map((x) => ({ id: x.id, name: x.name, values: { ...x.values } }));
        cur = Math.max(0, work.findIndex((x) => x.id === S.typeId));
        wp = clone(S.params); wb = clone(S.body);
        S.msg = ""; props(); status(); render();
        await recalc(); draw();
        return true;
      }
      foot.append(
        button("Cancel", close),
        button("Apply", () => apply()),
        button("OK", async () => { if (await apply()) close(); }, true),
      );
      recalc().then(draw);
    }, { wide: true });
  }
  async function reloadTypes() {
    const def = await Backend.call("family_def", { id: S.id });
    S.types = clone(def.types);
    if (!S.types.some((t) => t.id === S.typeId)) S.typeId = S.types[0] ? S.types[0].id : null;
    S.def.params = def.params;
    await refresh(true);
    tabsUI(); await preview(); props(); render();
  }

  // ---------------------------------------------------------------- family category and parameters
  const currentSettings = () => ({
    category: S.def.category, host: S.def.host,
    always_vertical: !!S.body.always_vertical, cut_with_voids: !!S.body.cut_with_voids,
    can_host_rebar: !!S.settings.can_host_rebar, part_type: S.settings.part_type || "Normal", shared: !!S.settings.shared,
    room_calculation_point: !!S.settings.room_calculation_point, room_point: [...(S.settings.room_point || [0, 0, 0])],
    maintain_annotation_orientation: !!S.settings.maintain_annotation_orientation,
  });

  // The rows of Family Parameters, for the dialog and for the Properties palette. `w` is a working copy that
  // the rows change; `changed(w)` is called after each change.
  function settingRows(w, changed) {
    const rows = [];
    const row = (label, node) => { rows.push(field(label, node)); };
    const go = () => changed(w);
    row("Hosted by", selIn([["none", "Nothing (free standing)"], ["wall", "Wall"], ["floor", "Floor"], ["ceiling", "Ceiling"], ["roof", "Roof"], ["face", "Face"]], w.host, (v) => { w.host = v; go(); }));
    row("Work Plane-Based", checkIn(w.host === "face", (v) => { w.host = v ? "face" : "none"; go(); }));
    row("Always vertical", checkIn(w.always_vertical, (v) => { w.always_vertical = v; go(); }));
    row("Cut with Voids When Loaded", checkIn(w.cut_with_voids, (v) => { w.cut_with_voids = v; go(); }));
    row("Can host rebar", checkIn(w.can_host_rebar, (v) => { w.can_host_rebar = v; go(); }));
    row("Part Type", selIn([["Normal", "Normal"]], w.part_type, (v) => { w.part_type = v; go(); }));
    row("Shared", checkIn(w.shared, (v) => { w.shared = v; go(); }));
    row("Room Calculation Point", checkIn(w.room_calculation_point, (v) => { w.room_calculation_point = v; go(); }));
    if (w.room_calculation_point) {
      const pt = el("div", "famed-pt");
      ["x", "y", "z"].forEach((a, i) => { const t = el("input"); t.type = "text"; t.title = `${a} (mm)`; t.value = String(w.room_point[i] || 0); t.onchange = () => { w.room_point[i] = num(t.value); go(); }; pt.append(t); });
      row("Point (x, y, z mm)", pt);
    }
    row("Maintain Annotation Orientation", checkIn(w.maintain_annotation_orientation, (v) => { w.maintain_annotation_orientation = v; go(); }));
    return rows;
  }

  // Settings that are not the category or the host change at once and can be undone; a new category or host is
  // kept by the project and refused while copies are placed.
  async function applyFamilySettings(w) {
    if (w.category !== S.def.category || w.host !== S.def.host) {
      const ok = await setMeta({ category: w.category, host: w.host });
      if (!ok) return false;
    }
    const before = snapshot();
    const o = currentSettings();
    const same = ["always_vertical", "cut_with_voids", "can_host_rebar", "part_type", "shared", "room_calculation_point", "maintain_annotation_orientation"].every((k) => o[k] === w[k]) && JSON.stringify(o.room_point) === JSON.stringify(w.room_point);
    if (same) { props(); return true; }
    S.body.always_vertical = w.always_vertical; S.body.cut_with_voids = w.cut_with_voids;
    S.settings = { ...S.settings, can_host_rebar: w.can_host_rebar, part_type: w.part_type, shared: w.shared, room_calculation_point: w.room_calculation_point, room_point: w.room_point, maintain_annotation_orientation: w.maintain_annotation_orientation };
    S.undo.push(before); S.redo = []; touch(); S.msg = "";
    props(); status(); render();
    return true;
  }

  // Manage Lookup Tables: CSV tables kept in the family and read by size_lookup(table, column, default, value1, ...).
  // `target` reads the tables and writes a new list (it answers with an error text, or null when it worked).
  function lookupDialog(target) {
    openModal("Manage Lookup Tables", (body, foot, close) => {
      let sel = 0;
      const list = el("div", "fe-lk-list"), view = el("div", "fe-lk-view"), msg = el("div", "modal-msg");
      const wrap = el("div", "fe-lk"); wrap.append(list, view);
      body.append(wrap, msg);
      const ext = el("div", "famed-hint", "In a formula: size_lookup(Table, \"Column\", default, value1, value2, ...). value1 is matched against the second column, value2 against the third. The first column only labels the row. When no row matches the default is used.");
      body.append(ext);
      const cellText = (c, col) => (typeof c === "number" ? String(Math.round(c * 1e6) / 1e6) : c);
      function draw() {
        const t = target.tables();
        sel = Math.min(sel, Math.max(0, t.length - 1));
        list.replaceChildren(...(t.length ? t.map((x, i) => { const b = el("button", "fe-lk-item" + (i === sel ? " on" : ""), x.name); b.onclick = () => { sel = i; draw(); }; return b; }) : [el("div", "famed-hint", "No tables yet. Import a CSV file.")]));
        view.replaceChildren();
        const cur = t[sel];
        if (!cur) return;
        view.append(el("div", "famed-hint", `${cur.name}: ${cur.rows.length} rows, ${cur.columns.length} columns. Lengths are shown in millimetres.`));
        const tbl = el("table", "fe-lk-table");
        const hr = el("tr"); cur.columns.forEach((c) => { const h = el("th", "", c.kind ? `${c.name} (${c.kind}${c.unit && c.kind === "length" ? ", file in " + c.unit : ""})` : c.name); hr.append(h); }); tbl.append(hr);
        cur.rows.slice(0, 40).forEach((r) => { const tr = el("tr"); r.forEach((c, j) => tr.append(el("td", "", cellText(c, cur.columns[j])))); tbl.append(tr); });
        view.append(tbl);
        if (cur.rows.length > 40) view.append(el("div", "famed-hint", `${cur.rows.length - 40} more rows not shown.`));
      }
      draw();
      foot.append(
        button("Import CSV...", async () => {
          msg.textContent = "";
          const path = await Backend.pickOpenCsv();
          if (!path) return;
          try {
            const r = await Backend.call("read_lookup_csv", { path });
            const next = target.tables().filter((x) => x.name.toLowerCase() !== r.table.name.toLowerCase());
            const replaced = next.length !== target.tables().length;
            next.push(r.table);
            const err = await target.set(next);
            if (err) { msg.textContent = err; return; }
            sel = target.tables().length - 1;
            msg.textContent = replaced ? `Replaced ${r.table.name}.` : `Imported ${r.table.name}.`;
            draw();
          } catch (e) { msg.textContent = String(e.message || e); }
        }),
        button("Export CSV...", async () => {
          const cur = target.tables()[sel];
          if (!cur) return;
          const path = await Backend.pickSaveCsv(cur.name + ".csv");
          if (!path) return;
          try { const r = await Backend.call("write_lookup_csv", { path, table: cur }); msg.textContent = `Wrote ${r.path}.`; } catch (e) { msg.textContent = String(e.message || e); }
        }),
        button("Delete", async () => {
          const cur = target.tables()[sel];
          if (!cur) return;
          const err = await target.set(target.tables().filter((x) => x !== cur));
          msg.textContent = err ? `${err} (A formula still uses '${cur.name}'.)` : `Deleted ${cur.name}.`;
          draw();
        }),
        button("Close", close, true),
      );
    });
  }

  function categoryDialog() {
    openModal("Family Category and Parameters", (body, foot, close) => {
      const w = currentSettings();
      const msg = el("div", "modal-msg");
      let filter = "", search = "";
      const DISC = [["", "<all>"], ["architecture", "Architecture"], ["structure", "Structure"], ["mechanical", "Mechanical"], ["electrical", "Electrical"], ["piping", "Piping"]];
      const wrap = el("div", "fe-catwrap");
      const left = el("div", "fe-catleft"), right = el("div", "fe-catright");
      const list = el("div", "fe-catlist");
      const drawList = () => {
        list.replaceChildren();
        const needle = search.trim().toLowerCase();
        for (const [d, label] of DISC.slice(1)) {
          const cats = (S.def.categories || []).filter((c) => c.discipline === d && (!filter || filter === d) && (!needle || c.label.toLowerCase().includes(needle)));
          if (!cats.length) continue;
          list.append(el("div", "fe-pgroup", label));
          for (const c of cats) {
            const r = el("div", "fe-catrow" + (c.id === w.category ? " sel" : ""), c.label);
            r.onclick = () => { w.category = c.id; drawList(); };
            list.append(r);
          }
        }
      };
      const fl = selIn(DISC, filter, (v) => { filter = v; drawList(); });
      const q = el("input"); q.type = "text"; q.placeholder = "Search"; q.oninput = () => { search = q.value; drawList(); };
      left.append(el("div", "famed-h", "Family Category"), q, field("Filter list", fl), list);
      const drawRight = () => {
        right.replaceChildren(el("div", "famed-h", "Family Parameters"));
        for (const r of settingRows(w, () => drawRight())) right.append(r);
      };
      wrap.append(left, right);
      body.append(wrap, msg);
      drawList(); drawRight();
      foot.append(button("Cancel", close), button("OK", async () => {
        const ok = await applyFamilySettings(w);
        if (ok) close(); else msg.textContent = S.msg || "That change was refused.";
      }, true));
    }, { wide: true });
  }

  // ---------------------------------------------------------------- view geometry
  function cam() {
    const v = S.view;
    if (!S.cams[v]) S.cams[v] = { k: 0.22, ox: S.canvas.width / 2, oy: v === "plan" ? S.canvas.height / 2 : S.canvas.height - 70 };
    return S.cams[v];
  }
  function fitAll() {
    for (const v of Object.keys(VIEWS)) {
      const ext = viewExtent(v);
      const c = { k: 0.22, ox: 0, oy: 0 };
      if (ext) {
        const w = S.canvas.width - 260, h = S.canvas.height - 100;
        c.k = Math.max(0.02, Math.min(w / Math.max(ext[2] - ext[0], 200), h / Math.max(ext[3] - ext[1], 200)));
        c.ox = (S.canvas.width - (ext[2] - ext[0]) * c.k) / 2 - ext[0] * c.k;
        c.oy = (S.canvas.height + (ext[3] - ext[1]) * c.k) / 2 + ext[1] * c.k;
      } else { c.ox = S.canvas.width / 2; c.oy = v === "plan" ? S.canvas.height / 2 : S.canvas.height - 70; }
      S.cams[v] = c;
    }
  }
  function viewExtent(v) {
    if (!S.pv || !S.pv.views) return null;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const pc of S.pv.views[PREVIEW_KEY[v]] || []) for (const ring of pc.rings) for (const [x, y] of ring) { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); }
    if (!Number.isFinite(x0)) return null;
    const ext = [Math.min(x0, -50), Math.min(y0, -50), Math.max(x1, 50), Math.max(y1, 50)];
    return VIEWS[v].flip ? [-ext[2], ext[1], -ext[0], ext[3]] : ext;
  }
  // Back and Left run right to left on the screen
  const flipOf = () => (VIEWS[S.view] && VIEWS[S.view].flip ? -1 : 1);
  const w2s = (u, v) => { const c = cam(); return { x: c.ox + flipOf() * u * c.k, y: c.oy - v * c.k }; };
  const s2w = (x, y) => { const c = cam(); return { u: (flipOf() * (x - c.ox)) / c.k, v: (c.oy - y) / c.k }; };
  const viewPlanes = () => { const V = VIEWS[S.view]; return S.body.planes.filter((p) => p.axis === V.h || p.axis === V.v); };

  // ---------------------------------------------------------------- drawing
  function render() {
    if (!S) return;
    const { ctx, canvas } = S;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = css("--view-bg"); ctx.fillRect(0, 0, canvas.width, canvas.height);
    $q("#fam-wrap").classList.toggle("v3d", S.view === "3d");
    if (!S.pv) return;
    if (S.view === "3d") { render3D(); return; }
    const V = VIEWS[S.view];
    const accent = css("--accent"), text = css("--text"), muted = css("--muted");
    const c = cam();
    // geometry
    for (const pass of ["solid", "void"]) {
      for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) {
        if ((pc.void ? "void" : "solid") !== pass) continue;
        const selected = isSel("form", pc.form);
        ctx.beginPath();
        for (const ring of pc.rings) { ring.forEach(([x, y], i) => { const p = w2s(x, y); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); ctx.closePath(); }
        ctx.fillStyle = pc.void ? "rgba(200,60,150,0.16)" : pc.sub === "Glass" ? "rgba(110,190,230,0.35)" : "rgba(120,150,190,0.30)";
        ctx.fill("evenodd");
        ctx.strokeStyle = selected ? accent : pc.void ? "#c03c96" : text; ctx.lineWidth = selected ? 2.5 : 1.2;
        ctx.setLineDash(pc.void ? [6, 4] : []); ctx.stroke(); ctx.setLineDash([]);
      }
    }
    // symbolic geometry
    for (const y of S.pv.syms || []) {
      if (y.view !== S.view) continue;
      if (!y.present) continue;
      const selected = selHas("sym", y.id) || ownedSel(y.id);
      ctx.beginPath();
      y.points.forEach(([x, yy], i) => { const p = w2s(x, yy); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); });
      if (y.closed) ctx.closePath();
      if (y.filled) { ctx.fillStyle = "rgba(60,60,60,0.35)"; ctx.fill(); }
      ctx.strokeStyle = selected ? accent : "#2f8f5b"; ctx.lineWidth = selected ? 2.5 : 1.5; ctx.stroke();
    }
    drawModelItems(accent);
    drawSketchOverlay(accent);
    // reference planes
    ctx.font = "11px system-ui, sans-serif";
    const labelled = { v: [], h: [] };
    // the selected plane is labelled first so it always has its name
    const ordered = [...viewPlanes()].sort((a, b) => selHas("plane", b.id) - selHas("plane", a.id));
    for (const p of ordered) {
      const at = pos(p.id);
      if (at === undefined) continue;
      const selected = selHas("plane", p.id) || (S.picks || []).includes(p.id);
      const vertical = p.axis === V.h; // a plane normal to the view's horizontal axis is a vertical line
      ctx.beginPath();
      if (vertical) { const x = w2s(at, 0).x; ctx.moveTo(x, 0); ctx.lineTo(x, canvas.height); }
      else { const y = w2s(0, at).y; ctx.moveTo(0, y); ctx.lineTo(canvas.width, y); }
      ctx.strokeStyle = selected ? accent : p.anchor.type === "origin" ? "#2a5db0" : "#4a90d9"; ctx.lineWidth = selected ? 2.5 : 1;
      ctx.setLineDash(p.anchor.type === "origin" ? [10, 4, 2, 4] : [6, 4]); ctx.stroke(); ctx.setLineDash([]);
      ctx.fillStyle = selected ? accent : "#2a5db0";
      // names crowd on a complicated family: skip one that would land on another
      if (!S.show.names && !selected) continue;
      const slot = vertical ? w2s(at, 0).x : w2s(0, at).y;
      if (!selected && labelled[vertical ? "v" : "h"].some((q) => Math.abs(q - slot) < 13)) continue;
      labelled[vertical ? "v" : "h"].push(slot);
      if (vertical) { ctx.save(); ctx.translate(slot + 3, 6); ctx.rotate(Math.PI / 2); ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.fillText(p.name, 0, 0); ctx.restore(); }
      else { ctx.textAlign = "left"; ctx.textBaseline = "bottom"; ctx.fillText(p.name, 6, slot - 2); }
    }
    drawDims(V);
    drawModTool();
    const sf = S.sel && S.sel.kind === "form" ? formById(S.sel.id) : null;
    if (sf && sf.shape.type === "revolve" && planeById(sf.shape.plane) && planeById(sf.shape.plane).axis === V.n) {
      const a = nodePos(sf.shape.axis_a), b = nodePos(sf.shape.axis_b), pa = w2s(a.u, a.v), pb = w2s(b.u, b.v);
      ctx.strokeStyle = "#d9822b"; ctx.lineWidth = 1.5; ctx.setLineDash([10, 3, 2, 3]); ctx.beginPath(); ctx.moveTo(pa.x, pa.y); ctx.lineTo(pb.x, pb.y); ctx.stroke(); ctx.setLineDash([]);
    }
    // node handles of the selected form or symbol
    const item = selectedLoops();
    for (const { loop, plane } of item) {
      if (!plane || plane !== V.n) continue;
      for (const n of loop.nodes) {
        const q = nodePos(n);
        if (!q) continue;
        const p = w2s(q.u, q.v);
        ctx.fillStyle = (n.u.plane || n.v.plane) ? accent : "#fff"; ctx.strokeStyle = accent; ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.rect(p.x - 4, p.y - 4, 8, 8); ctx.fill(); ctx.stroke();
      }
    }
    if (S.editing && S.editing.loop != null) {
      const f = formById(S.editing.id), loop = f && f.shape.profile && f.shape.profile[S.editing.loop];
      if (loop) { ctx.strokeStyle = accent; ctx.lineWidth = 3.5; ctx.beginPath(); flatten(loop).forEach((q, i) => { const p = w2s(q.u, q.v); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); ctx.stroke(); }
    }
    drawLocks(accent);
    drawShapeHandles(accent);
    // a face being dragged onto a plane lights the plane
    if (S.drag && S.drag.shape && S.drag.snapped != null && pos(S.drag.snapped) !== undefined) {
      const at = pos(S.drag.snapped), p = planeById(S.drag.snapped);
      ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.setLineDash([8, 4]); ctx.beginPath();
      if (p.axis === V.h) { const x = w2s(at, 0).x; ctx.moveTo(x, 0); ctx.lineTo(x, canvas.height); } else { const y = w2s(0, at).y; ctx.moveTo(0, y); ctx.lineTo(canvas.width, y); }
      ctx.stroke(); ctx.setLineDash([]);
    }
    if (S.alignRef) {
      const r = S.alignRef;
      ctx.strokeStyle = accent; ctx.lineWidth = 2.5; ctx.setLineDash([2, 3]); ctx.beginPath();
      if (r.axis === V.h) { const x = w2s(r.pos, 0).x; ctx.moveTo(x, 0); ctx.lineTo(x, canvas.height); } else { const y = w2s(0, r.pos).y; ctx.moveTo(0, y); ctx.lineTo(canvas.width, y); }
      ctx.stroke(); ctx.setLineDash([]);
    }
    if (S.showWp) {
      const wp = planeById(workPlane());
      ctx.fillStyle = "rgba(60,160,90,0.07)"; ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.strokeStyle = "rgba(40,140,70,0.8)"; ctx.lineWidth = 3; ctx.setLineDash([8, 5]); ctx.strokeRect(1.5, 1.5, canvas.width - 3, canvas.height - 3); ctx.setLineDash([]);
      ctx.fillStyle = "rgb(30,120,60)"; ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "right"; ctx.textBaseline = "top";
      ctx.fillText(`Work plane: ${wp ? wp.name : "?"}`, canvas.width - 10, 8);
    }
    drawDraft(V);
    // a selection box: solid left to right (window), dashed right to left (crossing)
    const bx = S.drag && (S.drag.box || S.drag.band) && S.drag.cur;
    if (bx) {
      const d = S.drag, crossing = bx.x < d.x;
      ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash(crossing ? [6, 4] : []);
      ctx.fillStyle = crossing ? "rgba(60,160,90,0.08)" : "rgba(46,111,158,0.08)";
      ctx.fillRect(Math.min(d.x, bx.x), Math.min(d.y, bx.y), Math.abs(bx.x - d.x), Math.abs(bx.y - d.y));
      ctx.strokeRect(Math.min(d.x, bx.x) + 0.5, Math.min(d.y, bx.y) + 0.5, Math.abs(bx.x - d.x), Math.abs(bx.y - d.y));
      ctx.setLineDash([]);
    }
  }

  // What a selection box takes in: with `crossing` anything it touches, otherwise only what lies wholly inside.
  // Pieces of a component select the component. Reference planes run the width of the view, so only a
  // crossing box picks them up.
  function boxItems(x0, y0, x1, y1, crossing) {
    const [lx, hx, ly, hy] = [Math.min(x0, x1), Math.max(x0, x1), Math.min(y0, y1), Math.max(y0, y1)];
    const inBox = (p) => p.x >= lx && p.x <= hx && p.y >= ly && p.y <= hy;
    const segHits = (a, b) => {
      if (inBox(a) || inBox(b)) return true;
      const cross = (p, q, r, s) => { const d = (q.x - p.x) * (s.y - r.y) - (q.y - p.y) * (s.x - r.x); if (Math.abs(d) < 1e-9) return false; const t = ((r.x - p.x) * (s.y - r.y) - (r.y - p.y) * (s.x - r.x)) / d, u = ((r.x - p.x) * (q.y - p.y) - (r.y - p.y) * (q.x - p.x)) / d; return t >= 0 && t <= 1 && u >= 0 && u <= 1; };
      const c = [{ x: lx, y: ly }, { x: hx, y: ly }, { x: hx, y: hy }, { x: lx, y: hy }];
      return c.some((p, i) => cross(a, b, p, c[(i + 1) % 4]));
    };
    // a polyline (screen points) is taken when every point is inside, or with crossing when any part is
    const takes = (pts, closed) => {
      if (!pts.length) return false;
      if (!crossing) return pts.every(inBox);
      const ring = closed ? [...pts, pts[0]] : pts;
      if (ring.length === 1) return inBox(ring[0]);
      for (let i = 0; i + 1 < ring.length; i++) if (segHits(ring[i], ring[i + 1])) return true;
      return false;
    };
    const out = [];
    const add = (kind, id) => { const o = ownerOf(id); out.push(o != null ? { kind: "comp", id: o } : { kind, id }); };
    const pieces = S.pv.views[PREVIEW_KEY[S.view]] || [];
    const byForm = new Map();
    for (const pc of pieces) { const l = byForm.get(pc.form) || []; l.push(pc); byForm.set(pc.form, l); }
    for (const [fid, pcs] of byForm) {
      const rings = pcs.flatMap((pc) => pc.rings.map((r) => r.map(([x, y]) => w2s(x, y))));
      const hit = crossing ? rings.some((r) => takes(r, true)) || pcs.some((pc) => { let ins = false; const w = s2w((lx + hx) / 2, (ly + hy) / 2); for (const r of pc.rings) if (pointIn(r, w.u, w.v)) ins = !ins; return ins; }) : rings.every((r) => takes(r, true));
      if (hit) add(mtextById(fid) ? "mtext" : "form", fid);
    }
    for (const y of S.pv.syms || []) if (y.present && y.view === S.view && takes(y.points.map(([a, b]) => w2s(a, b)), y.closed)) add("sym", y.id);
    for (const m of S.pv.model_lines || []) if (m.present && takes(m.points.map((q) => { const a = proj(q); return w2s(a[0], a[1]); }), m.closed)) add("mline", m.id);
    for (const c of S.pv.controls || []) if (c.present && c.view === S.view && takes([w2s(c.at[0], c.at[1])], false)) out.push({ kind: "ctrl", id: c.id });
    for (const c of S.pv.connectors || []) { const a = proj(c.origin); if (takes([w2s(a[0], a[1])], false)) out.push({ kind: "conn", id: c.id }); }
    for (const h of S.dimHits || []) {
      if (h.kind !== "line") continue;
      const corners = [{ x: h.x, y: h.y }, { x: h.x + h.w, y: h.y }, { x: h.x + h.w, y: h.y + h.h }, { x: h.x, y: h.y + h.h }];
      if (takes(corners, true) && !out.some((o) => o.kind === "dim" && o.id === h.id)) out.push({ kind: "dim", id: h.id });
    }
    if (crossing) {
      const V = VIEWS[S.view];
      for (const p of viewPlanes()) {
        const at = pos(p.id);
        if (at === undefined || p.anchor.type === "origin") continue;
        const hit = p.axis === V.h ? (() => { const x = w2s(at, 0).x; return x >= lx && x <= hx; })() : (() => { const y = w2s(0, at).y; return y >= ly && y <= hy; })();
        if (hit) out.push({ kind: "plane", id: p.id });
      }
    }
    return out;
  }

  // Model lines, controls and connectors in a 2D view
  function drawModelItems(accent) {
    const { ctx } = S;
    for (const m of S.pv.model_lines || []) {
      if (!m.present) continue;
      const selected = selHas("mline", m.id) || ownedSel(m.id);
      ctx.beginPath();
      m.points.forEach((q, i) => { const a = proj(q), p = w2s(a[0], a[1]); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); });
      if (m.closed) ctx.closePath();
      ctx.strokeStyle = selected ? accent : "#7a4fb5"; ctx.lineWidth = selected ? 2.5 : 1.5; ctx.stroke();
    }
    for (const c of S.pv.connectors || []) {
      const selected = selHas("conn", c.id);
      const a = proj(c.origin), p = w2s(a[0], a[1]);
      const d = proj(c.direction), dl = Math.hypot(d[0], d[1]);
      ctx.strokeStyle = selected ? accent : "#b5651d"; ctx.fillStyle = ctx.strokeStyle; ctx.lineWidth = selected ? 2.5 : 1.5;
      ctx.beginPath(); ctx.arc(p.x, p.y, 7, 0, Math.PI * 2); ctx.stroke();
      if (dl < 0.2) {
        // pointing into or out of the view: a dot for out, a cross for in
        const out = (S.view === "plan" ? c.direction[2] : S.view === "front" || S.view === "right" ? -c.direction[1] : c.direction[1]) > 0;
        ctx.beginPath();
        if (out) ctx.arc(p.x, p.y, 2, 0, Math.PI * 2); else { ctx.moveTo(p.x - 4, p.y - 4); ctx.lineTo(p.x + 4, p.y + 4); ctx.moveTo(p.x + 4, p.y - 4); ctx.lineTo(p.x - 4, p.y + 4); }
        out ? ctx.fill() : ctx.stroke();
      } else {
        const b = w2s(a[0] + (d[0] / dl) * 1, a[1] + (d[1] / dl) * 1), bx = b.x - p.x, by = b.y - p.y, bl = Math.hypot(bx, by) || 1;
        const ex = p.x + (bx / bl) * 24, ey = p.y + (by / bl) * 24;
        ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(ex, ey); ctx.stroke();
        ctx.beginPath(); ctx.moveTo(ex, ey); ctx.lineTo(ex - (bx / bl) * 6 + (by / bl) * 4, ey - (by / bl) * 6 - (bx / bl) * 4); ctx.lineTo(ex - (bx / bl) * 6 - (by / bl) * 4, ey - (by / bl) * 6 + (bx / bl) * 4); ctx.closePath(); ctx.fill();
      }
    }
    for (const c of S.pv.controls || []) {
      if (c.view !== S.view || !c.present) continue;
      const selected = selHas("ctrl", c.id);
      const p = w2s(c.at[0], c.at[1]);
      const vert = c.kind.endsWith("vertical"), dbl = c.kind.startsWith("double");
      ctx.strokeStyle = selected ? accent : "#1f7a8c"; ctx.fillStyle = ctx.strokeStyle; ctx.lineWidth = selected ? 2.5 : 1.5;
      const L = 18, ax = vert ? 0 : 1, ay = vert ? 1 : 0;
      const tip = (sx) => { const x = p.x + ax * L * sx, y = p.y + ay * L * sx; ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x - ax * 6 * sx + ay * 4, y - ay * 6 * sx + ax * 4); ctx.lineTo(x - ax * 6 * sx - ay * 4, y - ay * 6 * sx - ax * 4); ctx.closePath(); ctx.fill(); };
      ctx.beginPath(); ctx.moveTo(p.x - ax * L * (dbl ? 1 : 0), p.y - ay * L * (dbl ? 1 : 0)); ctx.lineTo(p.x + ax * L, p.y + ay * L); ctx.stroke();
      tip(1); if (dbl) tip(-1);
      if (!dbl) { ctx.beginPath(); ctx.arc(p.x, p.y, 2.5, 0, Math.PI * 2); ctx.fill(); }
    }
  }

  // Dimension objects draw only in their own view. S.dimHits keeps what can be clicked for the select tool.
  function drawDims(V) {
    const { ctx } = S;
    const selPlane = S.sel && S.sel.kind === "plane" ? S.sel.id : null;
    const selDim = S.sel && S.sel.kind === "dim" ? S.sel.id : null;
    const accent = css("--accent");
    S.dimHits = [];
    ctx.font = "11px system-ui, sans-serif";
    let ih = 0, iv = 0;
    // planes that follow a parameter but have no dimension object yet (older families): a plain read-out. A plane
    // placed at a plain distance shows nothing until a dimension is added (or it is selected: temporary dimensions).
    const covered = new Set((S.body.dims || []).flatMap((d) => d.drives || []));
    const formula = (p) => !/^\s*-?\d+(\.\d+)?\s*$/.test(String(p.anchor.by));
    const implicit = S.body.planes.filter((p) => p.anchor.type === "offset" && formula(p) && !covered.has(p.id) && (p.axis === V.h || p.axis === V.v) && (S.show.dims || p.id === selPlane));
    for (const p of implicit) {
      const a = pos(p.anchor.from), b = pos(p.id);
      if (a === undefined || b === undefined) continue;
      ctx.strokeStyle = "#8a6d1f"; ctx.fillStyle = "#8a6d1f"; ctx.lineWidth = 1;
      const label = `${String(p.anchor.by).replace(/[\[\]]/g, "")}  ${(b - a).toFixed(0)}`;
      if (p.axis === V.h) {
        const y = 22 + (ih++ % 6) * 16, x0 = w2s(a, 0).x, x1 = w2s(b, 0).x;
        ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke();
        tick(x0, y, true); tick(x1, y, true);
        ctx.textAlign = "center"; ctx.textBaseline = "bottom"; ctx.fillText(label, (x0 + x1) / 2, y - 2);
      } else {
        const x = S.canvas.width - 40 - (iv++ % 5) * 16, y0 = w2s(0, a).y, y1 = w2s(0, b).y;
        ctx.beginPath(); ctx.moveTo(x, y0); ctx.lineTo(x, y1); ctx.stroke();
        tick(x, y0, false); tick(x, y1, false);
        ctx.save(); ctx.translate(x - 3, (y0 + y1) / 2); ctx.rotate(-Math.PI / 2); ctx.textAlign = "center"; ctx.textBaseline = "bottom"; ctx.fillText(label, 0, 0); ctx.restore();
      }
    }
    for (const d of S.body.dims || []) {
      const pa = planeById(d.a);
      if (!pa || !(pa.axis === V.h || pa.axis === V.v)) continue;
      if (d.view ? d.view !== S.view : !S.show.dims) continue;
      if (d.view && !S.show.dims && !selHas("dim", d.id)) continue;
      drawChain(d, selHas("dim", d.id));
    }
    // temporary dimensions of the selected plane: to the nearest parallel plane each side; click a value to type it
    if (selPlane != null && S.tool === "select" && !(S.drag && S.drag.plane)) {
      const sp = planeById(selPlane), at = pos(selPlane);
      if (sp && at !== undefined && (sp.axis === V.h || sp.axis === V.v)) {
        const horizontal = sp.axis === V.h;
        const others = viewPlanes().filter((p) => p.id !== selPlane && p.axis === sp.axis).map((p) => ({ p, at: pos(p.id) })).filter((o) => o.at !== undefined && Math.abs(o.at - at) > 0.01);
        const below = others.filter((o) => o.at < at).sort((x, y) => y.at - x.at)[0], above = others.filter((o) => o.at > at).sort((x, y) => x.at - y.at)[0];
        ctx.save(); ctx.strokeStyle = accent; ctx.fillStyle = accent; ctx.lineWidth = 1;
        const lineAt = horizontal ? Math.min(S.canvas.height - 60, Math.max(60, (S.tdimAt ? S.tdimAt.y : S.canvas.height / 2) - 30)) : Math.min(S.canvas.width - 60, Math.max(60, (S.tdimAt ? S.tdimAt.x : S.canvas.width / 2) + 30));
        for (const o of [below, above]) {
          if (!o) continue;
          const s0 = horizontal ? w2s(at, 0).x : w2s(0, at).y, s1 = horizontal ? w2s(o.at, 0).x : w2s(0, o.at).y;
          ctx.beginPath();
          if (horizontal) { ctx.moveTo(s0, lineAt); ctx.lineTo(s1, lineAt); } else { ctx.moveTo(lineAt, s0); ctx.lineTo(lineAt, s1); }
          ctx.stroke();
          tick(horizontal ? s0 : lineAt, horizontal ? lineAt : s0, horizontal); tick(horizontal ? s1 : lineAt, horizontal ? lineAt : s1, horizontal);
          const str = Math.abs(o.at - at).toFixed(0), w = ctx.measureText(str).width + 8, mid = (s0 + s1) / 2;
          const box = horizontal ? { x: mid - w / 2, y: lineAt - 17, w, h: 15 } : { x: lineAt - 17, y: mid - w / 2, w: 15, h: w };
          ctx.fillStyle = css("--view-bg"); ctx.fillRect(box.x, box.y, box.w, box.h); ctx.strokeRect(box.x + 0.5, box.y + 0.5, box.w - 1, box.h - 1);
          ctx.fillStyle = accent; ctx.textAlign = "center"; ctx.textBaseline = "middle";
          if (horizontal) ctx.fillText(str, mid, lineAt - 9.5);
          else { ctx.save(); ctx.translate(lineAt - 9.5, mid); ctx.rotate(-Math.PI / 2); ctx.fillText(str, 0, 0); ctx.restore(); }
          S.dimHits.push({ kind: "tval", plane: selPlane, other: o.p.id, value: Math.abs(o.at - at), sign: Math.sign(at - o.at), ...box });
        }
        ctx.restore();
      }
    }
    // the dimension being made follows the pointer once two references are picked
    const picks = S.tool === "dim" ? S.picks || [] : [];
    if (picks.length >= 2 && S.mouse) {
      const w = s2w(S.mouse.x, S.mouse.y), first = planeById(picks[0]);
      const sorted = picks.slice().sort((x, y) => pos(x) - pos(y));
      drawChain({ id: 0, a: sorted[0], b: sorted[sorted.length - 1], via: sorted.slice(1, -1), at: first.axis === V.h ? w.v : w.u, seg_labels: [] }, true, true);
    }

    function tick(x, y, h) { ctx.beginPath(); if (h) { ctx.moveTo(x, y - 5); ctx.lineTo(x, y + 5); } else { ctx.moveTo(x - 5, y); ctx.lineTo(x + 5, y); } ctx.stroke(); }

    function drawChain(d, selected, ghost) {
      const refs = dimRefs(d);
      const ps = refs.map(pos);
      if (ps.some((x) => x === undefined)) return;
      const horizontal = planeById(d.a).axis === V.h; // measured along the view's horizontal axis
      const col = ghost ? "#999" : selected ? accent : d.reports_to ? "#2f7d6b" : d.label || (d.seg_labels || []).some(Boolean) ? "#7a5a00" : "#8a6d1f";
      ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = selected ? 1.8 : 1;
      let lineAt;
      if (d.at !== null && d.at !== undefined) lineAt = horizontal ? w2s(0, d.at).y : w2s(d.at, 0).x;
      else lineAt = horizontal ? S.canvas.height - 24 - (ih++ % 4) * 16 : 40 + (iv++ % 4) * 16;
      const s = ps.map((v) => (horizontal ? w2s(v, 0).x : w2s(0, v).y));
      const lo = Math.min(...s) - 8, hi = Math.max(...s) + 8;
      ctx.beginPath();
      if (horizontal) { ctx.moveTo(lo, lineAt); ctx.lineTo(hi, lineAt); } else { ctx.moveTo(lineAt, lo); ctx.lineTo(lineAt, hi); }
      ctx.stroke();
      s.forEach((v) => (horizontal ? tick(v, lineAt, true) : tick(lineAt, v, false)));
      const n = refs.length - 1;
      const text = (str, c0, c1, row, seg) => {
        const mid = (c0 + c1) / 2, w = ctx.measureText(str).width;
        let box;
        ctx.textAlign = "center"; ctx.textBaseline = "bottom";
        if (horizontal) { ctx.fillText(str, mid, lineAt - 2 - row * 14); box = { x: mid - w / 2, y: lineAt - 16 - row * 14, w, h: 14 }; }
        else { ctx.save(); ctx.translate(lineAt - 3 - row * 14, mid); ctx.rotate(-Math.PI / 2); ctx.fillText(str, 0, 0); ctx.restore(); box = { x: lineAt - 17 - row * 14, y: mid - w / 2, w: 14, h: w }; }
        if (!ghost) S.dimHits.push({ id: d.id, kind: "val", seg, ...box });
        return box;
      };
      for (let j = 0; j < n; j++) {
        const val = Math.abs(ps[j + 1] - ps[j]);
        const lab = n === 1 ? d.label : d.eq ? "" : (d.seg_labels || [])[j];
        const str = d.eq ? "EQ" : lab ? `${lab} = ${val.toFixed(0)}` : d.reports_to ? `${d.reports_to} = ${val.toFixed(0)}` : val.toFixed(0);
        text(str, s[j], s[j + 1], 0, j);
      }
      // EQ: the whole length above the parts (the value a click on it edits)
      if (d.eq && d.label) text(`${d.label} = ${Math.abs(ps[n] - ps[0]).toFixed(0)}`, s[0], s[n], 1, 0);
      else if (d.eq) text(`${Math.abs(ps[n] - ps[0]).toFixed(0)}`, s[0], s[n], 1, 0);
      if (ghost) return;
      // the padlock, and the EQ switch for a chain of three or more
      const end = (horizontal ? hi : hi) + 16;
      const px = horizontal ? end : lineAt, py = horizontal ? lineAt : end;
      padlock(ctx, px, py, d.locked);
      S.dimHits.push({ id: d.id, kind: "lock", x: px - PADLOCK_HIT, y: py - PADLOCK_HIT, w: 2 * PADLOCK_HIT, h: 2 * PADLOCK_HIT });
      if (refs.length >= 3 && (selected || d.eq)) {
        ctx.fillStyle = d.eq ? accent : "#999"; ctx.textAlign = "left"; ctx.textBaseline = "middle";
        const ex = horizontal ? px + 14 : px + 8, ey = horizontal ? py : py + 16;
        ctx.fillText("EQ", ex, ey);
        S.dimHits.push({ id: d.id, kind: "eq", x: ex - 2, y: ey - 7, w: 20, h: 14 });
      }
      // the line itself, for selecting and dragging
      if (horizontal) S.dimHits.push({ id: d.id, kind: "line", x: lo, y: lineAt - 4, w: hi - lo, h: 8 });
      else S.dimHits.push({ id: d.id, kind: "line", x: lineAt - 4, y: lo, w: 8, h: hi - lo });
    }
  }

  function hitDim(sx, sy) {
    const hits = S.dimHits || [];
    for (const kind of ["tval", "lock", "eq", "val", "line"]) {
      const h = hits.find((q) => q.kind === kind && sx >= q.x && sx <= q.x + q.w && sy >= q.y && sy <= q.y + q.h);
      if (h) return h;
    }
    return null;
  }

  // loops of the selected form or symbolic item, with the axis of their sketch plane
  function selectedLoops() {
    if (!S.sel || S.sel.kind === "multi") return [];
    if (S.sel.kind === "form") {
      const f = formById(S.sel.id);
      if (!f) return [];
      const wp = planeById(f.shape.plane), ax = wp && wp.axis;
      if (f.shape.type === "sweep" || f.shape.type === "swept_blend") return [{ loop: f.shape.path, plane: ax }];
      if (f.shape.type === "blend") return [...f.shape.base, ...f.shape.top].map((loop) => ({ loop, plane: ax }));
      if (f.shape.type === "revolve") return [...f.shape.profile.map((loop) => ({ loop, plane: ax })), { loop: { nodes: [f.shape.axis_a, f.shape.axis_b], bulges: [], closed: false }, plane: ax }];
      return f.shape.profile.map((loop) => ({ loop, plane: ax }));
    }
    if (S.sel.kind === "mline") {
      const m = mlineById(S.sel.id), wp = m && planeById(m.plane);
      return m && wp ? [{ loop: m.path, plane: wp.axis }] : [];
    }
    if (S.sel.kind === "sym") {
      const y = symById(S.sel.id);
      return y ? [{ loop: y.path, plane: { plan: "z", front: "y", back: "y", left: "x", right: "x" }[y.view] }] : [];
    }
    return [];
  }
  function nodePos(n) {
    const co = (c) => (c.plane ? (pos(c.plane) ?? 0) + (c.off || 0) : c.off || 0);
    return { u: co(n.u), v: co(n.v) };
  }

  function drawDraft(V) {
    const { ctx } = S;
    const d = S.draft;
    const P = (q) => w2s(q.u, q.v);
    const ghost = (loops) => {
      ctx.strokeStyle = "#999"; ctx.lineWidth = 1.2; ctx.setLineDash([2, 3]);
      for (const l of loops) { ctx.beginPath(); l.nodes.forEach((n, i) => { const q = nodePos(n), p = P(q); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); if (l.closed) ctx.closePath(); ctx.stroke(); }
      ctx.setLineDash([]);
    };
    if (S.blend) ghost(S.blend.base);
    if (S.rev) {
      ghost(S.rev.profile);
      if (S.rev.pts.length && S.mouse) {
        const a = P(S.rev.pts[0].pt), b = S.rev.pts.length > 1 ? P(S.rev.pts[1].pt) : S.mouse;
        ctx.strokeStyle = "#d9822b"; ctx.lineWidth = 1.5; ctx.setLineDash([10, 3, 2, 3]); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.setLineDash([]);
      }
    }
    if (S.rev && !S.draft && S.mouse) {
      const mm = snapPoint(S.mouse.x, S.mouse.y), p = P(mm.pt);
      ctx.fillStyle = css("--accent"); ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fill();
    }
    if (!d || !S.mouse) return;
    ctx.strokeStyle = css("--accent"); ctx.lineWidth = 1.5; ctx.setLineDash([4, 3]);
    const m = snapPoint(S.mouse.x, S.mouse.y);
    if (d.kind === "plane") {
      const a = P(d.a), dx = S.mouse.x - a.x, dy = S.mouse.y - a.y;
      ctx.beginPath();
      if (Math.abs(dx) >= Math.abs(dy)) { ctx.moveTo(0, a.y); ctx.lineTo(S.canvas.width, a.y); } else { ctx.moveTo(a.x, 0); ctx.lineTo(a.x, S.canvas.height); }
      ctx.stroke(); ctx.setLineDash([]); ctx.fillStyle = css("--accent"); ctx.beginPath(); ctx.arc(a.x, a.y, 3.5, 0, Math.PI * 2); ctx.fill();
      return;
    }
    if (d.kind === "rect" && d.a) { const a = P(d.a), b = P(m.pt); ctx.strokeRect(a.x, a.y, b.x - a.x, b.y - a.y); }
    else if (d.kind === "circle" && d.a) { const a = P(d.a), b = P(m.pt); ctx.beginPath(); ctx.arc(a.x, a.y, Math.hypot(b.x - a.x, b.y - a.y), 0, Math.PI * 2); ctx.stroke(); }
    else if (d.kind === "poly" && d.pts.length) { ctx.beginPath(); d.pts.forEach((q, i) => { const p = P(q.pt); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); const p = P(m.pt); ctx.lineTo(p.x, p.y); ctx.stroke(); }
    else if (d.kind === "arc" && d.pts.length) {
      const A = d.pts[0].pt;
      if (d.pts.length === 1) { const a = P(A), b = S.mouse; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); }
      else {
        const B = d.pts[1].pt, a3 = arcFrom3(A, B, m.pt);
        ctx.beginPath();
        if (!a3) { const a = P(A), b = P(B); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); }
        else for (let i = 0; i <= 40; i++) { const t = a3.a0 + (a3.sweep * i) / 40, p = P({ u: a3.c.u + a3.r * Math.cos(t), v: a3.c.v + a3.r * Math.sin(t) }); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }
        ctx.stroke();
      }
    }
    else if (d.kind === "path" && d.pts.length) {
      ctx.beginPath();
      d.pts.forEach((q, i) => {
        const p = P(q.pt);
        if (!i) { ctx.moveTo(p.x, p.y); return; }
        const pts = bulgePoints(d.pts[i - 1].pt, q.pt, d.bulges[i - 1] || 0);
        for (const w of pts) { const pp = P(w); ctx.lineTo(pp.x, pp.y); }
      });
      if (S.pathMode === "arc" && d.arcEnd) { const a3 = arcFrom3(d.pts[d.pts.length - 1].pt, d.arcEnd.pt, m.pt); if (a3) for (const w of bulgePoints(d.pts[d.pts.length - 1].pt, d.arcEnd.pt, a3.bulge)) { const pp = P(w); ctx.lineTo(pp.x, pp.y); } else ctx.lineTo(P(d.arcEnd.pt).x, P(d.arcEnd.pt).y); }
      else { const p = P(m.pt); ctx.lineTo(p.x, p.y); }
      ctx.stroke();
    }
    else if (d.kind === "plane" && d.a) { const a = w2s(d.a.u, d.a.v), b = S.mouse; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); }
    ctx.setLineDash([]);
    // snap marker
    const p = P(m.pt);
    ctx.fillStyle = m.snap ? "#d9822b" : css("--accent");
    ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fill();
  }

  // ---- 3D preview: the project's 3D viewport (drag to orbit, right-drag or Shift-drag to pan, wheel to zoom, click to
  // pick), with the model lines, connectors and work plane drawn over it on the 2D canvas, in step with its camera
  let vp3 = null, vp3Starting = false, vp3Key = null, vp3Fitted = false;
  function ensure3D() {
    if (vp3 || vp3Starting) return vp3;
    vp3Starting = true;
    Viewport.init($q("#fam-3d"), null, { minDist: 0.05, minFit: 0.6 }).then((v) => {
      vp3 = v; vp3Starting = false;
      v.onPick((id, additive) => pick3D(id, additive));
      v.onRender(() => { if (S && S.view === "3d" && app.doc === "family") overlay3D(); });
      render();
    }).catch((e) => { vp3Starting = false; if (S) { S.msg = `The 3D view could not start: ${e.message || e}`; status(); } });
    return null;
  }
  // the forms as the viewport's meshes: metres, a flat normal per triangle; voids pink, glass as glass
  function formMeshes() {
    const out = [];
    for (const f of S.pv.forms) {
      if (!f.present || !f.tris || f.tris.length < 9) continue;
      const t = f.tris, verts = [];
      // a solid whose triangles face inward (its volume comes out negative) is turned round so its faces light
      let vol = 0;
      for (let i = 0; i + 8 < t.length; i += 9) vol += t[i] * (t[i + 4] * t[i + 8] - t[i + 5] * t[i + 7]) - t[i + 1] * (t[i + 3] * t[i + 8] - t[i + 5] * t[i + 6]) + t[i + 2] * (t[i + 3] * t[i + 7] - t[i + 4] * t[i + 6]);
      const order = vol < 0 ? [0, 2, 1] : [0, 1, 2];
      for (let i = 0; i + 8 < t.length; i += 9) {
        const [a, b, c] = order.map((k) => i + k * 3);
        const ux = t[b] - t[a], uy = t[b + 1] - t[a + 1], uz = t[b + 2] - t[a + 2], vx = t[c] - t[a], vy = t[c + 1] - t[a + 1], vz = t[c + 2] - t[a + 2];
        let nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
        const l = Math.hypot(nx, ny, nz) || 1; nx /= l; ny /= l; nz /= l;
        for (const k of [a, b, c]) verts.push(t[k] / 1000, t[k + 1] / 1000, t[k + 2] / 1000, nx, ny, nz);
      }
      out.push({ id: f.id, kind: f.sub === "Glass" ? "Glass" : "Form", verts, color: f.void ? [214, 120, 190] : null });
    }
    return out;
  }
  const selFormIds = () => S.pv.forms.filter((f) => isSel("form", f.id)).map((f) => f.id);
  function pick3D(id, additive) {
    if (!S) return;
    const hit = id == null ? null : ownerOf(id) != null ? { kind: "comp", id: ownerOf(id) } : mtextById(id) ? { kind: "mtext", id } : { kind: "form", id };
    if (additive) { if (hit) { const cur = selItems(); setSel(cur.some((x) => x.kind === hit.kind && x.id === hit.id) ? cur.filter((x) => !(x.kind === hit.kind && x.id === hit.id)) : [...cur, hit]); } }
    else S.sel = hit;
    props(); render(); ctxRefresh();
  }
  function render3D() {
    overlay3D();
    const v = ensure3D();
    if (!v) return;
    if (vp3Key !== S.pv) {
      vp3Key = S.pv;
      v.setMeshes(formMeshes(), selFormIds());
      if (!vp3Fitted) { v.fit(); vp3Fitted = true; }
    } else v.setSelection(selFormIds());
    v.poke();
  }
  // what is drawn over the 3D view, from the camera as it stands
  function overlay3D() {
    const { ctx, canvas } = S;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (!vp3 || !S.pv) return;
    const T = (q) => { const p = vp3.project([q[0] / 1000, q[1] / 1000, q[2] / 1000]); return [p.x, p.y]; };
    for (const m of S.pv.model_lines || []) {
      if (!m.present) continue;
      const selected = selHas("mline", m.id) || ownedSel(m.id);
      ctx.beginPath();
      m.points.forEach((q, i) => { const [x, y] = T(q); i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
      if (m.closed) ctx.closePath();
      ctx.strokeStyle = selected ? css("--accent") : "#7a4fb5"; ctx.lineWidth = selected ? 2.5 : 1.5; ctx.stroke();
    }
    for (const c of S.pv.connectors || []) {
      const selected = selHas("conn", c.id);
      const [x, y] = T(c.origin), [x2, y2] = T(c.origin.map((v, i) => v + c.direction[i] * 120));
      ctx.strokeStyle = selected ? css("--accent") : "#b5651d"; ctx.lineWidth = 2;
      ctx.beginPath(); ctx.arc(x, y, 5, 0, Math.PI * 2); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x2, y2); ctx.stroke();
    }
    if (S.showWp) {
      // the work plane as a sheet a little larger than the model
      const id = S.wps["3d"] !== undefined ? S.wps["3d"] : VIEWS.plan.origin, p = planeById(id), at = pos(id);
      if (p && at !== undefined) {
        const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
        for (const f of S.pv.forms) for (let i = 0; i + 2 < f.tris.length; i += 3) for (let c = 0; c < 3; c++) { lo[c] = Math.min(lo[c], f.tris[i + c]); hi[c] = Math.max(hi[c], f.tris[i + c]); }
        if (!Number.isFinite(lo[0])) { lo.fill(-500); hi.fill(500); }
        const m = 150, A = [lo[0] - m, hi[0] + m], B = [lo[1] - m, hi[1] + m], C = [lo[2] - m, hi[2] + m];
        const quad = p.axis === "z" ? [[A[0], B[0], at], [A[1], B[0], at], [A[1], B[1], at], [A[0], B[1], at]] : p.axis === "y" ? [[A[0], at, C[0]], [A[1], at, C[0]], [A[1], at, C[1]], [A[0], at, C[1]]] : [[at, B[0], C[0]], [at, B[1], C[0]], [at, B[1], C[1]], [at, B[0], C[1]]];
        ctx.beginPath();
        quad.forEach((q, i) => { const [x, y] = T(q); i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
        ctx.closePath(); ctx.fillStyle = "rgba(60,160,90,0.14)"; ctx.fill();
        ctx.strokeStyle = "rgba(40,140,70,0.9)"; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]); ctx.stroke(); ctx.setLineDash([]);
        const [x0, y0] = T(quad[0]); ctx.fillStyle = "rgb(30,120,60)"; ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "bottom";
        ctx.fillText(`Work plane: ${p.name}`, x0 + 4, y0 - 4);
      }
    }
  }

  // ---- Modify tools: Move, Copy, Rotate, Mirror and Offset (famedit.rs does the work, in the view's coordinates) ----
  const MOD_TOOLS = new Set(["move", "copy", "rotate", "mirror", "offset"]);
  // the selection as the core names it; dimensions stay out (they follow their planes)
  const modItems = () => selItems().filter((x) => ["form", "sym", "mline", "mtext", "ctrl", "conn", "comp", "plane"].includes(x.kind));
  // the view coordinates of the change, as famedit.rs works them (for the preview)
  function xfMap(xf) {
    if (xf.type === "move") return ([u, v]) => [u + xf.du, v + xf.dv];
    if (xf.type === "rotate") { const r = (xf.deg * Math.PI) / 180, c = Math.cos(r), s = Math.sin(r); return ([u, v]) => [xf.cu + (u - xf.cu) * c - (v - xf.cv) * s, xf.cv + (u - xf.cu) * s + (v - xf.cv) * c]; }
    const dx = xf.bu - xf.au, dy = xf.bv - xf.av, l2 = dx * dx + dy * dy || 1, a = (dx * dx - dy * dy) / l2, b = (2 * dx * dy) / l2;
    return ([u, v]) => [xf.au + a * (u - xf.au) + b * (v - xf.av), xf.av + b * (u - xf.au) - a * (v - xf.av)];
  }
  // the outlines of what is selected in this view (view coordinates)
  function selOutlines() {
    const out = [];
    for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) if (isSel("form", pc.form)) for (const r of pc.rings) out.push({ pts: r, closed: true });
    for (const y of S.pv.syms || []) if (y.view === S.view && selHas("sym", y.id)) out.push({ pts: y.points, closed: y.closed });
    for (const ml of S.pv.model_lines || []) if (ml.present && (selHas("mline", ml.id) || ownedSel(ml.id))) out.push({ pts: ml.points.map(proj), closed: ml.closed });
    const V = VIEWS[S.view];
    for (const it of selItems()) {
      if (it.kind !== "plane") continue;
      const p = planeById(it.id), at = pos(it.id);
      if (!p || at === undefined || (p.axis !== V.h && p.axis !== V.v)) continue;
      const a = s2w(0, 0), b = s2w(S.canvas.width, S.canvas.height);
      out.push({ pts: p.axis === V.h ? [[at, a.v], [at, b.v]] : [[a.u, at], [b.u, at]], closed: false });
    }
    return out;
  }
  // the middle of the selection's outlines: where Rotate turns about unless a centre is placed
  function rotCentre() {
    if (S.rotPlace && S.mod && S.mod.centre) return S.mod.centre;
    let lo = [Infinity, Infinity], hi = [-Infinity, -Infinity];
    for (const o of selOutlines()) for (const [u, v] of o.pts) { lo = [Math.min(lo[0], u), Math.min(lo[1], v)]; hi = [Math.max(hi[0], u), Math.max(hi[1], v)]; }
    return Number.isFinite(lo[0]) ? { u: (lo[0] + hi[0]) / 2, v: (lo[1] + hi[1]) / 2 } : { u: 0, v: 0 };
  }
  // the angle from the start ray to the cursor's, in whole degrees, held at multiples of 15 within 3
  function rotAngle(c, a, b) {
    let d = ((Math.atan2(b.v - c.v, b.u - c.u) - Math.atan2(a.v - c.v, a.u - c.u)) * 180) / Math.PI;
    d = ((d + 540) % 360) - 180;
    const n15 = Math.round(d / 15) * 15;
    return Math.abs(d - n15) < 3 ? n15 : Math.round(d);
  }
  // the change the picks so far, and the cursor, make (null until there are enough)
  function modXf(cur) {
    const M = S.mod || { pts: [] }, p = M.pts;
    if (S.tool === "move" || S.tool === "copy") return p.length ? { type: "move", du: cur.u - p[0].u, dv: cur.v - p[0].v } : null;
    if (S.tool === "rotate") { const c = rotCentre(); return p.length ? { type: "rotate", cu: c.u, cv: c.v, deg: rotAngle(c, p[0], cur) } : null; }
    if (S.tool === "mirror" && !S.mirrorPick) return p.length ? { type: "mirror", au: p[0].u, av: p[0].v, bu: cur.u, bv: cur.v } : null;
    return null;
  }
  async function applyXf(xf, copy) {
    const items = modItems();
    if (!items.length) { S.msg = "Select what to change first."; status(); return; }
    const r = await edit("transform", { items, view: S.view, xf, copy });
    S.mod = { pts: [] };
    if (!r) return;
    setSel(r.items.map((x) => ({ kind: x.kind, id: x.id })));
    const many = S.tool === "copy" && S.copyMany;
    S.msg = copy ? (many ? "Copied. Click another start point, or press Esc." : "Copied.") : "Done.";
    if (!many) setTool("select");
    props(); status(); render(); ctxRefresh();
  }
  async function modClick(m) {
    const sp = snapPoint(m.x, m.y).pt, t = S.tool, M = S.mod || (S.mod = { pts: [] });
    if (t === "offset") {
      const h = hitItem(m.x, m.y);
      if (!h || (h.kind !== "sym" && h.kind !== "mline")) { S.msg = "Click a model line or a symbolic line, on the side to offset it to."; status(); return; }
      const r = await edit("offset", { item: { kind: h.kind, id: h.id }, view: S.view, dist: S.offsetDist || 100, toward: [sp.u, sp.v], copy: !!S.offsetCopy });
      if (r && r.items.length) { S.sel = { kind: r.items[0].kind, id: r.items[0].id }; S.msg = "Offset."; props(); status(); render(); }
      return;
    }
    if (!modItems().length) { S.msg = "Select what to change first (Esc, then click it), then pick the tool."; status(); return; }
    if (t === "mirror" && S.mirrorPick) {
      // a reference plane seen edge on is the axis
      const pl = hitPlane(m.x, m.y), V = VIEWS[S.view], at = pl && pos(pl.id);
      if (!pl || at === undefined) { S.msg = "Click a reference plane to mirror across, or choose Draw Axis."; status(); return; }
      const xf = pl.axis === V.h ? { type: "mirror", au: at, av: 0, bu: at, bv: 1000 } : { type: "mirror", au: 0, av: at, bu: 1000, bv: at };
      await applyXf(xf, S.mirrorCopy !== false);
      return;
    }
    if (t === "rotate" && S.rotPlace && !M.centre) { M.centre = sp; S.msg = "Centre placed. Click a point on the line to turn from."; status(); render(); return; }
    if (!M.pts.length) { M.pts.push(sp); S.msg = t === "rotate" ? "Click on the line to turn to (or type the angle)." : t === "mirror" ? "Click the second point of the axis." : "Click where it goes."; status(); render(); return; }
    const xf = modXf(sp);
    if (!xf) return;
    if (xf.type === "rotate" && !xf.deg) { S.msg = "That is no turn."; status(); return; }
    await applyXf(xf, t === "copy" || (t === "mirror" && S.mirrorCopy !== false));
  }
  // the picks, and the selection where the change would put it
  function drawModTool() {
    if (!MOD_TOOLS.has(S.tool) || S.tool === "offset" || !S.mouse) return;
    const { ctx } = S, accent = css("--accent"), cur = snapPoint(S.mouse.x, S.mouse.y).pt;
    const xf = modXf(cur), w = (q) => w2s(q[0], q[1]);
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 1.2; ctx.setLineDash([5, 4]);
    if (xf) {
      const f = xfMap(xf);
      for (const o of selOutlines()) {
        ctx.beginPath();
        o.pts.forEach((q, i) => { const s2 = w(f(q)); i ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
        if (o.closed) ctx.closePath();
        ctx.stroke();
      }
    }
    const M = S.mod || { pts: [] };
    if (S.tool === "rotate") {
      const c = rotCentre(), cs = w([c.u, c.v]);
      ctx.setLineDash([]); ctx.beginPath(); ctx.arc(cs.x, cs.y, 5, 0, Math.PI * 2); ctx.moveTo(cs.x - 9, cs.y); ctx.lineTo(cs.x + 9, cs.y); ctx.moveTo(cs.x, cs.y - 9); ctx.lineTo(cs.x, cs.y + 9); ctx.stroke();
      ctx.setLineDash([4, 3]);
      for (const q of [M.pts[0], M.pts.length ? cur : null]) { if (!q) continue; const s2 = w([q.u, q.v]); ctx.beginPath(); ctx.moveTo(cs.x, cs.y); ctx.lineTo(s2.x, s2.y); ctx.stroke(); }
      if (xf) { const s2 = w([cur.u, cur.v]); ctx.fillStyle = accent; ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "bottom"; ctx.fillText(`${xf.deg}°`, s2.x + 10, s2.y - 6); }
    } else if (M.pts.length) {
      const a = w([M.pts[0].u, M.pts[0].v]), b = w([cur.u, cur.v]);
      ctx.setLineDash(S.tool === "mirror" ? [10, 4, 2, 4] : [4, 3]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      if (xf && xf.type === "move") { ctx.fillStyle = accent; ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "bottom"; ctx.fillText(`${Math.round(Math.hypot(xf.du, xf.dv))}`, b.x + 10, b.y - 6); }
    }
    ctx.restore();
  }

  // ---------------------------------------------------------------- pointer
  // The point under the cursor in the view: coordinates that lock to a reference plane when close to one
  function snapPoint(sx, sy) {
    const V = VIEWS[S.view];
    const w = s2w(sx, sy);
    const out = { pt: { u: Math.round(w.u), v: Math.round(w.v) }, u: { plane: null, off: Math.round(w.u) }, v: { plane: null, off: Math.round(w.v) }, snap: false };
    let bu = 9, bv = 9;
    // a point near a reference plane takes its position; with Lock to planes on it is also tied to the plane,
    // otherwise it only sits there and the padlock on the finished edge offers the lock
    const tie = (p, at) => (S.lockNew ? { plane: p.id, off: 0 } : { plane: null, off: at });
    for (const p of viewPlanes()) {
      const at = pos(p.id);
      if (at === undefined) continue;
      if (p.axis === V.h) { const d = Math.abs(w2s(at, 0).x - sx); if (d < bu) { bu = d; out.u = tie(p, at); out.pt.u = at; out.snap = true; } }
      else { const d = Math.abs(w2s(0, at).y - sy); if (d < bv) { bv = d; out.v = tie(p, at); out.pt.v = at; out.snap = true; } }
    }
    return out;
  }
  const coordOf = (c) => (c.plane ? { plane: c.plane, off: 0 } : { off: c.off });
  const mouse = (e) => { const r = S.canvas.getBoundingClientRect(); return { x: ((e.clientX - r.left) * S.canvas.width) / r.width, y: ((e.clientY - r.top) * S.canvas.height) / r.height }; };

  // the shared padlock; `col` was the old ink and is no longer used
  const padlock = (ctx, px, py, locked) => drawPadlock(ctx, px, py, locked, getComputedStyle(document.documentElement).getPropertyValue("--accent").trim());

  // The straight edges of the selected form or symbolic line that run along a reference plane, with whether each is
  // locked to it. A closed padlock is a lock; an open one marks an edge that sits on the plane but is free of it.
  function edgeLocks() {
    const V = VIEWS[S.view], out = [];
    if (S.view === "3d" || !S.sel || (S.sel.kind !== "form" && S.sel.kind !== "sym" && S.sel.kind !== "mline")) return out;
    for (const it of viewLoops()) {
      if (it.kind !== S.sel.kind || it.id !== S.sel.id) continue;
      const nd = it.loop.nodes, n = nd.length, bl = it.loop.bulges || [];
      for (let i = 0; i < (it.loop.closed ? n : n - 1); i++) {
        const j = (i + 1) % n;
        if (bl[i]) continue; // an arc has no straight edge to lock
        const pa = nodePos(nd[i]), pb = nodePos(nd[j]);
        for (const key of ["u", "v"]) {
          const other = key === "u" ? "v" : "u", axis = key === "u" ? V.h : V.v;
          if (Math.abs(pa[key] - pb[key]) > 1e-6 || Math.abs(pa[other] - pb[other]) < 1e-6) continue;
          const ca = nd[i][key], cb = nd[j][key];
          const at = { kind: it.kind, id: it.id, li: it.li, which: it.which, i, j, key, a: pa, b: pb };
          if (ca.plane && ca.plane === cb.plane && !ca.off && !cb.off) out.push({ ...at, plane: ca.plane, locked: true });
          else if (!ca.plane && !cb.plane) {
            const pl = viewPlanes().find((q) => q.axis === axis && pos(q.id) !== undefined && Math.abs(pos(q.id) - pa[key]) < 0.01);
            if (pl) out.push({ ...at, plane: pl.id, locked: false });
          }
        }
      }
    }
    return out;
  }
  function drawLocks(accent) {
    S.lockHits = [];
    for (const e of edgeLocks()) {
      const m = w2s((e.a.u + e.b.u) / 2, (e.a.v + e.b.v) / 2);
      const px = e.key === "u" ? m.x + 14 : m.x, py = e.key === "u" ? m.y : m.y - 14;
      padlock(S.ctx, px, py, e.locked, e.locked ? accent : "#999");
      S.lockHits.push({ ...e, x: px - PADLOCK_HIT, y: py - PADLOCK_HIT, w: 2 * PADLOCK_HIT, h: 2 * PADLOCK_HIT });
    }
    // faces seen edge-on across the work plane: the start and end, and the outermost faces of the profile
    for (const g of faceLocks()) {
      const m = w2s(g.mid.u, g.mid.v);
      const px = g.key === "u" ? m.x + 14 * flipOf() * -g.dir : m.x + 16, py = g.key === "u" ? m.y + 16 : m.y + 14 * g.dir;
      padlock(S.ctx, px, py, g.on, g.on ? accent : "#999");
      S.lockHits.push({ ...g, face: true, x: px - PADLOCK_HIT, y: py - PADLOCK_HIT, w: 2 * PADLOCK_HIT, h: 2 * PADLOCK_HIT });
    }
  }

  // ---- shape handles: drag a face of the selected extrusion ----
  // In the view facing its work plane, each straight profile edge along a view axis is a face seen edge-on. In a
  // view across the work plane the start and end faces are, and so are the outermost faces of the profile. Each
  // gets an arrow; dragging it snaps to reference planes, and a face left on a plane shows a padlock to lock it.
  function shapeHandles(forLocks = false) {
    const out = [];
    if (!S.pv || S.view === "3d" || S.sk || S.editing || (!forLocks && S.tool !== "select") || !S.sel || S.sel.kind !== "form") return out;
    const f = formById(S.sel.id);
    if (!f || f.shape.type !== "extrusion") return out;
    const V = VIEWS[S.view], wp = planeById(f.shape.plane);
    if (!wp) return out;
    const A = wp.axis, [su, sv] = sketchOf(A);
    if (V.n === A) {
      f.shape.profile.forEach((loop, li) => {
        const nd = loop.nodes.map(nodePos), n = nd.length;
        if (!n || nd.some((q) => !q)) return;
        const cu = nd.reduce((s, q) => s + q.u, 0) / n, cv = nd.reduce((s, q) => s + q.v, 0) / n;
        for (let i = 0; i < (loop.closed ? n : n - 1); i++) {
          const j = (i + 1) % n;
          if ((loop.bulges || [])[i] || (loop.shapes || [])[i]) continue;
          const a = nd[i], b = nd[j];
          for (const key of ["u", "v"]) {
            const other = key === "u" ? "v" : "u";
            if (Math.abs(a[key] - b[key]) > 1e-6 || Math.abs(a[other] - b[other]) < 1e-6) continue;
            out.push({ type: "edge", form: f.id, li, nodes: [[li, i], [li, j]], sk: key, key, at: a[key], dir: Math.sign(a[key] - (key === "u" ? cu : cv)) || 1, mid: { u: (a.u + b.u) / 2, v: (a.v + b.v) / 2 }, locked: [loop.nodes[i][key], loop.nodes[j][key]].some((c) => c.plane) });
          }
        }
      });
      return out;
    }
    const pf = (S.pv.forms || []).find((x) => x.id === f.id);
    const pieces = (S.pv.views[PREVIEW_KEY[S.view]] || []).filter((c) => c.form === f.id);
    const aKey = A === V.h ? "u" : A === V.v ? "v" : null;
    if (!pf || !pf.span || !pieces.length || !aKey) return out;
    const bKey = aKey === "u" ? "v" : "u", bAxis = bKey === "u" ? V.h : V.v;
    let lo = Infinity, hi = -Infinity;
    for (const c of pieces) for (const ring of c.rings) for (const q of ring) { const t = bKey === "u" ? q[0] : q[1]; lo = Math.min(lo, t); hi = Math.max(hi, t); }
    const mk = (aVal, bVal) => (aKey === "u" ? { u: aVal, v: bVal } : { u: bVal, v: aVal });
    for (const which of ["start", "end"]) {
      const at = which === "start" ? pf.span[0] : pf.span[1], other = which === "start" ? pf.span[1] : pf.span[0];
      out.push({ type: "depth", form: f.id, which, key: aKey, at, dir: Math.sign(at - other) || (which === "end" ? 1 : -1), mid: mk(at, (lo + hi) / 2), locked: !!f.shape[which + "_lock"], driven: !/^\s*-?\d+(\.\d+)?\s*$/.test(String(f.shape[which])) });
    }
    const sk = bAxis === su ? "u" : bAxis === sv ? "v" : null;
    if (!sk) return out;
    const all = f.shape.profile.flatMap((loop, li) => loop.nodes.map((n, i) => ({ li, i, n, p: nodePos(n) })));
    if (!all.length || all.some((x) => !x.p)) return out;
    const vals = all.map((x) => x.p[sk]);
    for (const [ext, dir] of [[Math.min(...vals), -1], [Math.max(...vals), 1]]) {
      const on = all.filter((x) => Math.abs(x.p[sk] - ext) < 0.01);
      out.push({ type: "extreme", form: f.id, key: bKey, sk, at: ext, dir, mid: mk((pf.span[0] + pf.span[1]) / 2, ext), nodes: on.map((x) => [x.li, x.i]), locked: on.some((x) => x.n[sk].plane) });
    }
    return out;
  }
  // where a handle's arrow sits on the screen: just outside the face
  function handleAt(g) {
    const m = w2s(g.mid.u, g.mid.v);
    return g.key === "u" ? { x: m.x + 12 * g.dir * flipOf(), y: m.y, horiz: true } : { x: m.x, y: m.y - 12 * g.dir, horiz: false };
  }
  function drawShapeHandles(accent) {
    const ctx = S.ctx;
    S.shapeHits = [];
    for (const g of shapeHandles()) {
      const h = handleAt(g), s = g.key === "u" ? g.dir * flipOf() : -g.dir;
      ctx.beginPath();
      if (h.horiz) { ctx.moveTo(h.x + 7 * s, h.y); ctx.lineTo(h.x - 3 * s, h.y - 6); ctx.lineTo(h.x - 3 * s, h.y + 6); }
      else { ctx.moveTo(h.x, h.y + 7 * s); ctx.lineTo(h.x - 6, h.y - 3 * s); ctx.lineTo(h.x + 6, h.y - 3 * s); }
      ctx.closePath();
      ctx.fillStyle = g.locked ? "#9aa7b5" : accent; ctx.fill();
      ctx.strokeStyle = "#fff"; ctx.lineWidth = 1; ctx.stroke();
      S.shapeHits.push({ ...g, x: h.x, y: h.y });
    }
  }
  const hitShapeHandle = (sx, sy) => (S.shapeHits || []).find((g) => Math.hypot(g.x - sx, g.y - sy) <= 9) || null;
  // The face of a drag at view coordinate `val`, from the values it had when the drag began
  function applyFace(g, val) {
    const f = formById(g.form);
    if (!f) return;
    if (g.type === "depth") {
      f.shape[g.which] = String(Math.round((val - pos(f.shape.plane)) * 100) / 100);
      f.shape[g.which + "_lock"] = null;
      return;
    }
    for (const [li, i] of g.nodes) f.shape.profile[li].nodes[i][g.sk] = { plane: null, off: Math.round(val * 100) / 100 };
  }
  // the planes a face could land on: parallel to it, seen as lines in this view
  const facePlanes = (g) => { const V = VIEWS[S.view], axis = g.key === "u" ? V.h : V.v; return viewPlanes().filter((p) => p.axis === axis && pos(p.id) !== undefined); };
  function faceLocks() {
    const out = [];
    for (const g of shapeHandles(true)) {
      if (g.type === "edge") continue; // the edge padlocks cover these
      const f = formById(g.form), near = facePlanes(g).find((p) => Math.abs(pos(p.id) - g.at) < 0.01);
      if (g.type === "depth") {
        const lock = f.shape[g.which + "_lock"];
        if (lock) out.push({ ...g, plane: lock, on: true });
        else if (near && !g.driven) out.push({ ...g, plane: near.id, on: false });
        continue;
      }
      const cs = g.nodes.map(([li, i]) => f.shape.profile[li].nodes[i][g.sk]);
      const ps = new Set(cs.map((c) => c.plane || null));
      if (ps.size === 1 && cs[0].plane && cs.every((c) => !c.off)) out.push({ ...g, plane: cs[0].plane, on: true });
      else if (cs.every((c) => !c.plane) && near) out.push({ ...g, plane: near.id, on: false });
    }
    return out;
  }
  async function toggleFaceLock(h) {
    const name = (planeById(h.plane) || {}).name, at = Math.round(pos(h.plane) * 100) / 100;
    const ok = await change((b) => {
      const f = b.forms.find((x) => x.id === h.form);
      if (h.type === "depth") {
        if (h.on) { f.shape[h.which] = String(Math.round((at - pos(f.shape.plane)) * 100) / 100); f.shape[h.which + "_lock"] = null; }
        else f.shape[h.which + "_lock"] = h.plane;
      } else for (const [li, i] of h.nodes) f.shape.profile[li].nodes[i][h.sk] = h.on ? { plane: null, off: at } : { plane: h.plane, off: 0 };
    }, true);
    if (ok) { S.msg = h.on ? `Unlocked from ${name}.` : `Locked to ${name}. The face follows the plane when it moves.`; status(); }
    return ok;
  }
  async function toggleEdgeLock(h) {
    if (h.face) return toggleFaceLock(h);
    const at = Math.round(pos(h.plane) * 100) / 100, name = (planeById(h.plane) || {}).name;
    const ok = await change((b) => {
      for (const n of hitEdgeNodes(b, h)) n[h.key] = h.locked ? { plane: null, off: at } : { plane: h.plane, off: 0 };
    }, true);
    if (ok) { S.msg = h.locked ? `Unlocked from ${name}.` : `Locked to ${name}.`; status(); }
    return ok;
  }
  // A line to align to, taken from the edges of the geometry drawn in this view (a face seen edge-on)
  function hitGeomEdge(sx, sy) {
    const V = VIEWS[S.view];
    if (!S.pv || !S.pv.views) return null;
    let best = null, bd = 7;
    for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) {
      for (const ring of pc.rings) {
        for (let k = 0; k < ring.length; k++) {
          const A = ring[k], B = ring[(k + 1) % ring.length];
          const vert = Math.abs(A[0] - B[0]) < 1e-6, hor = Math.abs(A[1] - B[1]) < 1e-6;
          if (vert === hor) continue;
          const pa = w2s(A[0], A[1]), pb = w2s(B[0], B[1]);
          const l2 = (pb.x - pa.x) ** 2 + (pb.y - pa.y) ** 2 || 1;
          const t = Math.max(0, Math.min(1, ((sx - pa.x) * (pb.x - pa.x) + (sy - pa.y) * (pb.y - pa.y)) / l2));
          const d = Math.hypot(pa.x + t * (pb.x - pa.x) - sx, pa.y + t * (pb.y - pa.y) - sy);
          if (d < bd) { bd = d; best = { axis: vert ? V.h : V.v, pos: vert ? A[0] : A[1], form: pc.form }; }
        }
      }
    }
    if (!best) return null;
    const f = formById(best.form), plane = viewPlanes().find((q) => q.axis === best.axis && pos(q.id) !== undefined && Math.abs(pos(q.id) - best.pos) < 0.01);
    return { axis: best.axis, pos: best.pos, plane: plane ? plane.id : null, name: f ? f.name || `Form ${f.id}` : "a face" };
  }

  function hitPlane(sx, sy) {
    const V = VIEWS[S.view];
    let best = null, bd = 6;
    for (const p of viewPlanes()) {
      const at = pos(p.id);
      if (at === undefined) continue;
      const d = p.axis === V.h ? Math.abs(w2s(at, 0).x - sx) : Math.abs(w2s(0, at).y - sy);
      if (d < bd) { bd = d; best = p; }
    }
    return best;
  }
  function hitHandle(sx, sy) {
    const V = VIEWS[S.view];
    for (const { loop, plane } of selectedLoops()) {
      if (plane !== V.n) continue;
      for (let i = 0; i < loop.nodes.length; i++) {
        const q = nodePos(loop.nodes[i]);
        const p = w2s(q.u, q.v);
        if (Math.abs(p.x - sx) < 7 && Math.abs(p.y - sy) < 7) return { loop, i };
      }
    }
    return null;
  }
  // Edges of forms and symbolic geometry drawn in this view that run along a plane (a vertical edge for a vertical
  // plane): the ones Align can lock to it.
  function viewLoops() {
    const V = VIEWS[S.view];
    const out = [];
    for (const f of S.body.forms) {
      const wp = planeById(f.shape.plane);
      if (!wp || wp.axis !== V.n) continue;
      if (f.shape.type === "extrusion" || f.shape.type === "revolve") f.shape.profile.forEach((loop, li) => out.push({ kind: "form", id: f.id, li, loop }));
      if (f.shape.type === "sweep" || f.shape.type === "swept_blend") out.push({ kind: "form", id: f.id, li: 0, loop: f.shape.path, which: "path" });
      if (f.shape.type === "blend") { f.shape.base.forEach((loop, li) => out.push({ kind: "form", id: f.id, li, loop })); f.shape.top.forEach((loop, li) => out.push({ kind: "form", id: f.id, li, loop, which: "top" })); }
    }
    for (const y of S.body.syms) {
      if (y.view === S.view) out.push({ kind: "sym", id: y.id, li: 0, loop: y.path });
    }
    for (const m of S.body.model_lines || []) {
      const wp = planeById(m.plane);
      if (wp && wp.axis === V.n) out.push({ kind: "mline", id: m.id, li: 0, loop: m.path });
    }
    return out;
  }
  function hitEdge(sx, sy, pl) {
    const V = VIEWS[S.view];
    const vertical = pl.axis === V.h;
    let best = null, bd = 7;
    for (const it of viewLoops()) {
      const nd = it.loop.nodes, n = nd.length;
      for (let i = 0; i < (it.loop.closed ? n : n - 1); i++) {
        const a = nodePos(nd[i]), b = nodePos(nd[(i + 1) % n]);
        if (vertical ? Math.abs(a.u - b.u) > 1e-6 : Math.abs(a.v - b.v) > 1e-6) continue;
        const pa = w2s(a.u, a.v), pb = w2s(b.u, b.v);
        const l2 = (pb.x - pa.x) ** 2 + (pb.y - pa.y) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((sx - pa.x) * (pb.x - pa.x) + (sy - pa.y) * (pb.y - pa.y)) / l2));
        const d = Math.hypot(pa.x + t * (pb.x - pa.x) - sx, pa.y + t * (pb.y - pa.y) - sy);
        if (d < bd) { bd = d; best = { kind: it.kind, id: it.id, li: it.li, which: it.which, i, j: (i + 1) % n }; }
      }
    }
    return best;
  }
  // The start or end of an extrusion seen edge-on: a line across the form at that depth, which Align can
  // lock to a plane parallel to it.
  function hitDepthEdge(sx, sy, pl) {
    const V = VIEWS[S.view];
    if (!S.pv || S.view === "3d") return null;
    const horizontal = pl.axis === V.v; // a plane normal to the view's vertical axis is a horizontal line
    if (!horizontal && pl.axis !== V.h) return null;
    let best = null, bd = 7;
    for (const pf of S.pv.forms || []) {
      const f = formById(pf.id);
      if (!f || !["extrusion", "blend"].includes(f.shape.type) || !pf.span) continue;
      const wp = planeById(f.shape.plane);
      if (!wp || wp.axis !== pl.axis) continue;
      const pieces = (S.pv.views[PREVIEW_KEY[S.view]] || []).filter((c) => c.form === f.id);
      if (!pieces.length) continue;
      let lo = Infinity, hi = -Infinity;
      for (const c of pieces) for (const ring of c.rings) for (const [x, y] of ring) { const t = horizontal ? x : y; lo = Math.min(lo, t); hi = Math.max(hi, t); }
      for (const which of ["start", "end"]) {
        const val = which === "start" ? pf.span[0] : pf.span[1];
        const a = horizontal ? w2s(lo, val) : w2s(val, lo), b = horizontal ? w2s(hi, val) : w2s(val, hi);
        const t = horizontal ? Math.max(Math.min(a.x, b.x) - sx, 0, sx - Math.max(a.x, b.x)) : Math.max(Math.min(a.y, b.y) - sy, 0, sy - Math.max(a.y, b.y));
        const d = horizontal ? Math.hypot(t, a.y - sy) : Math.hypot(t, a.x - sx);
        if (d < bd) { bd = d; best = { kind: "depth", id: f.id, which }; }
      }
    }
    return best;
  }
  function hitEdgeNodes(b, h) {
    const owner = h.kind === "form" ? (({ shape: sh }) => (sh.type === "sweep" || sh.type === "swept_blend" ? [sh.path] : sh.type === "blend" ? (h.which === "top" ? sh.top : sh.base) : sh.profile)[h.li])(b.forms.find((f) => f.id === h.id)) : h.kind === "mline" ? b.model_lines.find((y) => y.id === h.id).path : b.syms.find((y) => y.id === h.id).path;
    return [owner.nodes[h.i], owner.nodes[h.j]];
  }
  function pointIn(ring, x, y) {
    let inside = false;
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const [xi, yi] = ring[i], [xj, yj] = ring[j];
      if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
    return inside;
  }
  function hitItem(sx, sy) {
    const w = s2w(sx, sy);
    for (const c of S.pv.controls || []) {
      if (c.view !== S.view || !c.present) continue;
      const p = w2s(c.at[0], c.at[1]);
      if (Math.abs(p.x - sx) < 22 && Math.abs(p.y - sy) < 22) return { kind: "ctrl", id: c.id };
    }
    for (const c of S.pv.connectors || []) {
      const a = proj(c.origin), p = w2s(a[0], a[1]);
      if (Math.hypot(p.x - sx, p.y - sy) < 11) return { kind: "conn", id: c.id };
    }
    for (const m of [...(S.pv.model_lines || [])].reverse()) {
      if (!m.present) continue;
      const pts = m.closed ? [...m.points, m.points[0]] : m.points;
      for (let i = 0; i + 1 < pts.length; i++) {
        const a0 = proj(pts[i]), b0 = proj(pts[i + 1]), a = w2s(a0[0], a0[1]), b = w2s(b0[0], b0[1]);
        const l2 = (b.x - a.x) ** 2 + (b.y - a.y) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((sx - a.x) * (b.x - a.x) + (sy - a.y) * (b.y - a.y)) / l2));
        if (Math.hypot(a.x + t * (b.x - a.x) - sx, a.y + t * (b.y - a.y) - sy) < 5) return ownerOf(m.id) != null ? { kind: "comp", id: ownerOf(m.id) } : { kind: "mline", id: m.id };
      }
    }
    for (const y of [...(S.pv.syms || [])].reverse()) {
      if (!y.present || y.view !== S.view) continue;
      for (let i = 0; i + 1 < y.points.length; i++) {
        const a = w2s(...y.points[i]), b = w2s(...y.points[i + 1]);
        const l2 = (b.x - a.x) ** 2 + (b.y - a.y) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((sx - a.x) * (b.x - a.x) + (sy - a.y) * (b.y - a.y)) / l2));
        if (Math.hypot(a.x + t * (b.x - a.x) - sx, a.y + t * (b.y - a.y) - sy) < 5) return ownerOf(y.id) != null ? { kind: "comp", id: ownerOf(y.id) } : { kind: "sym", id: y.id };
      }
    }
    const pieces = [...(S.pv.views[PREVIEW_KEY[S.view]] || [])].reverse();
    for (const pc of pieces) {
      let inside = false;
      for (const ring of pc.rings) if (pointIn(ring, w.u, w.v)) inside = !inside;
      if (inside) return ownerOf(pc.form) != null ? { kind: "comp", id: ownerOf(pc.form) } : mtextById(pc.form) ? { kind: "mtext", id: pc.form } : { kind: "form", id: pc.form };
    }
    return null;
  }

  async function onDown(e) {
    if (!S) return;
    S.canvas.setPointerCapture(e.pointerId);
    const m = mouse(e);
    S.mouse = m;

    if (e.button === 1 || e.button === 2) { S.drag = { pan: true, x: m.x, y: m.y, ox: cam().ox, oy: cam().oy, button: e.button }; return; }
    if (await skDown(e, m)) return;
    const t = S.tool;
    // a padlock that shows takes a click whatever tool is on (the Select tool handles its own below)
    if (t !== "select") {
      const inBox = (q) => m.x >= q.x && m.x <= q.x + q.w && m.y >= q.y && m.y <= q.y + q.h;
      const dl = (S.dimHits || []).find((q) => q.kind === "lock" && inBox(q)), d = dl && dimById(dl.id);
      if (d) { await dimPatch(d, { locked: !d.locked }); render(); return; }
      const lk = t !== "align" && (S.lockHits || []).find(inBox);
      if (lk) { await toggleEdgeLock(lk); render(); return; }
    }
    if (t === "select") {
      const lk = (S.lockHits || []).find((q) => m.x >= q.x && m.x <= q.x + q.w && m.y >= q.y && m.y <= q.y + q.h);
      if (lk) { await toggleEdgeLock(lk); return; }
      const sh = hitShapeHandle(m.x, m.y);
      if (sh) {
        if (sh.locked) { S.msg = "That face is locked to a reference plane. Click its padlock to unlock it, or move the plane."; status(); return; }
        if (sh.driven) { S.msg = `Extrusion ${sh.which === "start" ? "Start" : "End"} follows a parameter. Change the parameter, or take it off in Properties.`; status(); return; }
        S.drag = { shape: sh, before: snapshot(), moved: false };
        return;
      }
      const h = hitHandle(m.x, m.y);
      if (h) { S.drag = { handle: h, before: snapshot(), moved: false }; return; }
      if (S.editing) { S.editing.loop = hitLoop(m.x, m.y); render(); ctxRefresh(); return; }
      const dh = hitDim(m.x, m.y);
      // Ctrl+click adds to the selection and Shift+click takes away; a drag from empty space draws a box
      const add = e.ctrlKey || e.metaKey, remove = e.shiftKey && !add;
      if (add || remove) {
        const pl0 = !dh && hitPlane(m.x, m.y);
        const hit = dh && (dh.kind === "line" || dh.kind === "val") ? { kind: "dim", id: dh.id } : pl0 ? { kind: "plane", id: pl0.id } : !dh ? hitItem(m.x, m.y) : null;
        if (hit) {
          const cur = selItems();
          setSel(remove ? cur.filter((x) => !(x.kind === hit.kind && x.id === hit.id)) : [...cur, hit]);
          props(); render();
          return;
        }
        if (!dh) { S.drag = { box: true, x: m.x, y: m.y, mode: add ? "add" : "remove" }; return; }
      }
      if (dh) { await clickDim(dh); return; }
      const pl = hitPlane(m.x, m.y);
      if (pl) {
        // temporary dimensions sit near where the plane was picked
        S.sel = { kind: "plane", id: pl.id }; S.tdimAt = { x: m.x, y: m.y }; props(); render();
        const held = drivenBy(pl.id);
        if (held) { S.msg = `${pl.name} is held by a dimension. Change the dimension instead.`; status(); }
        S.drag = !held && pl.anchor.type === "offset" && /^\s*-?\d+(\.\d+)?\s*$/.test(String(pl.anchor.by)) ? { plane: pl.id, before: snapshot(), moved: false } : null;
        return;
      }
      S.sel = hitItem(m.x, m.y); props(); render();
      if (!S.sel) { S.drag = { box: true, x: m.x, y: m.y, mode: "replace" }; return; }
      const it = S.sel && (S.sel.kind === "ctrl" ? ctrlById(S.sel.id) : S.sel.kind === "conn" ? connById(S.sel.id) : S.sel.kind === "mtext" ? mtextById(S.sel.id) : S.sel.kind === "comp" ? compById(S.sel.id) : null);
      const wp = it && S.sel.kind !== "ctrl" && S.sel.kind !== "comp" ? planeById(it.plane) : null;
      if (it && S.sel.kind === "comp" && S.view !== "3d") S.drag = { comp: it, before: snapshot(), moved: false };
      else if (it && (S.sel.kind === "ctrl" || (wp && wp.axis === VIEWS[S.view].n))) S.drag = { pt: it, before: snapshot(), moved: false };
      return;
    }
    if (t === "comp") {
      if (S.view === "3d") { S.msg = "Place components in a plan or an elevation."; status(); return; }
      const pick = S.compPick;
      if (!pick || !pick.family) { S.msg = "Pick a family in the options bar first."; status(); return; }
      const sp = snapPoint(m.x, m.y), V = VIEWS[S.view], plane = workPlane();
      await change((b) => {
        const id = newId();
        const origin = [null, null, null];
        origin[AXI[V.h]] = coordOf(sp.u); origin[AXI[V.v]] = coordOf(sp.v); origin[AXI[V.n]] = { plane, off: 0 };
        (b.nested = b.nested || []).push({ id, family: pick.family, type_name: pick.type || "", type_param: null, name: "", origin, rotation_deg: 0, work_plane: plane, flip: false, mirror: false, params: {}, vis: {} });
        S.sel = { kind: "comp", id };
      });
      S.msg = "Component placed. Its parameters are in Properties."; status(); render();
      return;
    }
    if (t === "mtext") {
      const sp = snapPoint(m.x, m.y);
      const plane = workPlane();
      askText("Model Text", "Text", "Text", [], async (txt) => {
        if (!txt.trim()) return;
        await change((b) => {
          const id = newId();
          const n = (b.model_texts || []).length + 1;
          (b.model_texts = b.model_texts || []).push({ id, name: `Text ${n}`, plane, u: coordOf(sp.u), v: coordOf(sp.v), text: txt, text_param: null, font: "DejaVu Sans", size: "100", depth: "20", bold: false, italic: false, align: "left", material: "", material_param: null, subcategory: "", vis: {} });
          S.sel = { kind: "mtext", id };
        });
        S.msg = "Text placed. Change its size, depth, font and alignment in Properties."; status(); render();
      });
      return;
    }
    if (t === "control") {
      const sp = snapPoint(m.x, m.y);
      await change((b) => {
        const id = newId();
        (b.controls = b.controls || []).push({ id, name: "", view: S.view, u: coordOf(sp.u), v: coordOf(sp.v), kind: S.ctrlKind, vis: {} });
        S.sel = { kind: "ctrl", id };
      });
      S.msg = "Control placed. Drag it, or change the kind in Properties."; status(); render();
      return;
    }
    if (t === "conn") {
      const sp = snapPoint(m.x, m.y);
      const k = S.connKind, wp = workPlane();
      await change((b) => {
        const id = newId();
        const props = {};
        for (const [name, kind, def] of CONN[k].props) props[name] = def;
        (b.connectors = b.connectors || []).push({ id, name: "", kind: k, plane: wp, u: coordOf(sp.u), v: coordOf(sp.v), flip: false, props });
        S.sel = { kind: "conn", id };
      });
      S.msg = `${CONN[k].label} placed on ${(planeById(wp) || {}).name}. Set its size and system in Properties.`; status(); render();
      return;
    }
    if (t === "pickwp") {
      const pl = hitPlane(m.x, m.y);
      if (!pl) { S.msg = "Click a reference plane."; status(); return; }
      await finishPick(pl);
      return;
    }
    if (t === "plane") {
      if (S.draft && S.draft.kind === "plane") { await finishPlane(m); return; }
      S.draft = { kind: "plane", a: s2w(m.x, m.y), down: m };
      return;
    }
    if (t === "dim") {
      const V = VIEWS[S.view];
      const pl = hitPlane(m.x, m.y);
      const picks = S.picks || (S.picks = []);
      if (pl) {
        if (picks.length && planeById(picks[0]).axis !== pl.axis) { S.msg = "Pick parallel reference planes."; status(); return; }
        if (!picks.includes(pl.id)) picks.push(pl.id);
        S.msg = picks.length < 2 ? "Pick another reference plane." : "Pick more planes, or click in empty space to place the dimension.";
        status(); render();
        return;
      }
      if (picks.length < 2) { S.msg = "Pick two or more parallel reference planes first."; status(); return; }
      const w = s2w(m.x, m.y);
      const at = Math.round(planeById(picks[0]).axis === V.h ? w.v : w.u);
      const refs = picks.slice();
      S.picks = [];
      const r = await edit("add_dim", { refs, view: S.view, at }, true);
      if (r && r.dim) { S.sel = { kind: "dim", id: r.dim }; S.msg = "Dimension placed. Label it, lock it or set EQ in Properties."; props(); status(); render(); }
      return;
    }
    if (MOD_TOOLS.has(t)) { await modClick(m); return; }
    if (t === "align") {
      const o = S.alignOpts;
      // the padlock left by the last alignment
      const lk0 = (S.lockHits || []).find((q) => m.x >= q.x && m.x <= q.x + q.w && m.y >= q.y && m.y <= q.y + q.h);
      if (lk0 && !S.alignRef) { await toggleEdgeLock(lk0); render(); return; }
      if (!S.alignRef) {
        const pl = hitPlane(m.x, m.y), g = hitGeomEdge(m.x, m.y);
        const fromPlane = pl && { axis: pl.axis, pos: pos(pl.id), plane: pl.id, name: pl.name };
        const ref = o.prefer === "geometry" ? g || fromPlane : fromPlane || g;
        if (ref) { S.alignRef = ref; S.msg = `Aligning to ${ref.name}. Click an edge of a form or symbolic line, or the start or end of an extrusion.`; status(); render(); }
        return;
      }
      const ref = S.alignRef;
      const hit = hitEdge(m.x, m.y, ref) || hitDepthEdge(m.x, m.y, ref);
      if (!hit) { S.msg = "Click an edge that runs along the reference, or press Esc."; status(); return; }
      const lock = o.lock && ref.plane;
      const at = Math.round(ref.pos * 100) / 100;
      const keep = o.multiple || e.ctrlKey;
      if (!keep) S.alignRef = null;
      if (hit.kind === "depth") {
        await change((b) => {
          const sh = formById(hit.id).shape;
          if (lock) sh[hit.which + "_lock"] = ref.plane;
          else { sh[hit.which] = String(Math.round((ref.pos - pos(sh.plane)) * 100) / 100); sh[hit.which + "_lock"] = null; }
          S.sel = { kind: "form", id: hit.id };
        });
      } else {
        const key = ref.axis === VIEWS[S.view].h ? "u" : "v";
        await change((b) => { for (const n of hitEdgeNodes(b, hit)) n[key] = lock ? { plane: ref.plane, off: 0 } : { plane: null, off: at }; S.sel = { kind: hit.kind, id: hit.id }; });
      }
      // what was aligned stays selected, so its padlock shows on the plane and can be clicked to lock it
      S.msg = `Aligned to ${ref.name}${lock ? " and locked" : ref.plane ? ". Click the padlock to lock it to the plane" : o.lock ? ". It could not be locked: that edge is not on a reference plane" : ""}.`; props(); status(); render();
      return;
    }
    if (t === "sweep" || t === "swblend") {
      const sp = snapPoint(m.x, m.y);
      const d = S.draft && S.draft.kind === "path" ? S.draft : (S.draft = { kind: "path", pts: [], bulges: [], arcEnd: null });
      if (S.pathMode === "arc" && d.pts.length) {
        if (!d.arcEnd) { d.arcEnd = sp; }
        else {
          const A = d.pts[d.pts.length - 1], a = arcFrom3(A.pt, d.arcEnd.pt, sp.pt);
          if (!a) { S.msg = "Those three points do not make an arc."; status(); d.arcEnd = null; }
          else { d.pts.push(d.arcEnd); d.bulges.push(a.bulge); d.arcEnd = null; S.msg = ""; }
        }
      } else { if (d.pts.length) d.bulges.push(0); d.pts.push(sp); }
      render();
      return;
    }
    if (t === "revolve" && S.rev) {
      const sp = snapPoint(m.x, m.y);
      S.rev.pts.push(sp);
      if (S.rev.pts.length === 2) await addRevolve();
      render();
      return;
    }
    if (t === "ext" || t === "void" || t === "blend" || t === "revolve") {
      const sp = snapPoint(m.x, m.y);
      const d = S.draft;
      if (S.sketch === "poly") {
        S.draft = d && d.kind === "poly" ? d : { kind: "poly", pts: [] };
        S.draft.pts.push(sp);
      } else if (!d) S.draft = { kind: S.sketch, a: sp.pt, an: sp };
      else await finishSketch(sp);
      render();
      return;
    }
    if ((t === "sym" || t === "mline") && S.symMode === "arc") {
      const sp = snapPoint(m.x, m.y);
      S.draft = S.draft && S.draft.kind === "arc" ? S.draft : { kind: "arc", pts: [] };
      S.draft.pts.push(sp);
      if (S.draft.pts.length === 3) await finishArc();
      render();
      return;
    }
    if (t === "sym" || t === "mline") {
      const sp = snapPoint(m.x, m.y);
      S.draft = S.draft && S.draft.kind === "poly" ? S.draft : { kind: "poly", pts: [] };
      S.draft.pts.push(sp);
      render();
      return;
    }
    if (t === "region") {
      const sp = snapPoint(m.x, m.y);
      if (!S.draft) S.draft = { kind: "rect", a: sp.pt, an: sp };
      else await finishRegion(sp);
      render();
    }
  }

  async function onMove(e) {
    if (!S) return;
    const m = mouse(e);
    S.mouse = m;
    if (skMove(e, m)) return;
    const d = S.drag;
    if (d) {

      if (d.pan) { const c = cam(); c.ox = d.ox + m.x - d.x; c.oy = d.oy + m.y - d.y; render(); return; }
      if (d.box) { if (d.cur || Math.hypot(m.x - d.x, m.y - d.y) > 4) { d.cur = m; render(); } return; }
      if (d.shape) {
        // the face follows the cursor across the view, and snaps onto a parallel reference plane
        const g = d.shape, w = s2w(m.x, m.y);
        let val = g.key === "u" ? w.u : w.v, snapped = null, best = 8;
        for (const p of facePlanes(g)) {
          const at = pos(p.id), px = g.key === "u" ? Math.abs(w2s(at, 0).x - m.x) : Math.abs(w2s(0, at).y - m.y);
          if (px < best) { best = px; val = at; snapped = p.id; }
        }
        if (snapped == null) val = Math.round(val);
        applyFace(g, val);
        d.moved = true; d.snapped = snapped;
        await preview(); render(); return;
      }
      if (d.plane) {
        const p = planeById(d.plane), V = VIEWS[S.view];
        const w = s2w(m.x, m.y), at = p.axis === V.h ? w.u : w.v;
        const from = pos(p.anchor.from);
        p.anchor.by = String(Math.round(at - from));
        d.moved = true;
        await preview(); props(); render(); return;
      }
      if (d.dimAt) {
        const dm = dimById(d.dimAt), w = s2w(m.x, m.y);
        if (dm) { dm.at = Math.round(planeById(dm.a).axis === VIEWS[S.view].h ? w.v : w.u); d.moved = true; render(); }
        return;
      }
      if (d.comp) {
        const sp = snapPoint(m.x, m.y), V = VIEWS[S.view];
        d.comp.origin[AXI[V.h]] = coordOf(sp.u); d.comp.origin[AXI[V.v]] = coordOf(sp.v);
        d.moved = true;
        await preview(); render(); return;
      }
      if (d.pt) {
        const sp = snapPoint(m.x, m.y);
        d.pt.u = coordOf(sp.u); d.pt.v = coordOf(sp.v);
        d.moved = true;
        await preview(); render(); return;
      }
      if (d.handle) {
        const sp = snapPoint(m.x, m.y);
        moveNode(d.handle, sp);
        d.moved = true;
        await preview(); render(); return;
      }
    }
    if (S.draft || S.tool !== "select") render();
  }

  // Move a sketch node; the other nodes of the loop that shared its u or v follow, so a rectangle stays one
  function moveNode(h, sp) {
    const n = h.loop.nodes[h.i];
    const same = (a, b) => (a.plane || null) === (b.plane || null) && (a.off || 0) === (b.off || 0);
    const ou = { ...n.u }, ov = { ...n.v };
    for (const o of h.loop.nodes) {
      if (same(o.u, ou)) o.u = coordOf(sp.u);
      if (same(o.v, ov)) o.v = coordOf(sp.v);
    }
    if (sp.u.plane == null && sp.v.plane == null) { /* free point */ }
  }

  async function onUp(e) {
    if (!S) return;
    const d = S.drag;
    S.drag = null;
    const m = mouse(e);
    // a right click that did not pan ends the sketch tool's current step
    if (d && d.pan && d.button === 2 && skHere() && Math.hypot(m.x - d.x, m.y - d.y) < 4) { FSk.rightClick(); return; }
    if (await skUp(e, m, d)) return;
    if (d && d.box) {
      if (d.cur) {
        const got = boxItems(d.x, d.y, m.x, m.y, m.x < d.x);
        const cur = selItems();
        const same = (a, b) => a.kind === b.kind && a.id === b.id;
        setSel(d.mode === "add" ? [...cur, ...got] : d.mode === "remove" ? cur.filter((x) => !got.some((g) => same(g, x))) : got);
        props();
      }
      render();
      return;
    }
    if (d && (d.plane || d.handle || d.dimAt || d.pt || d.comp || d.shape) && d.moved) {
      const ok = await preview();
      if (!ok) { restore(d.before); S.msg = S.pvError; await preview(); }
      else {
        S.undo.push(d.before); S.redo = []; touch();
        if (d.shape) { S.sel = { kind: "form", id: d.shape.form }; S.msg = d.snapped != null ? `On ${(planeById(d.snapped) || {}).name}. Click the padlock to lock the face to it.` : ""; }
      }
      props(); status(); render();
    }
    // a drag along the line finishes the plane; a plain click leaves it waiting for the second click
    if (S.draft && S.draft.kind === "plane" && S.tool === "plane" && Math.hypot(m.x - S.draft.down.x, m.y - S.draft.down.y) > 12) await finishPlane(m);
  }

  // The second point of a reference plane: the stroke from the first decides which way it runs, like a grid.
  async function finishPlane(m) {
    const dr = S.draft;
    const dx = m.x - dr.down.x, dy = m.y - dr.down.y;
    const V = VIEWS[S.view];
    if (Math.hypot(dx, dy) <= 12) { S.msg = "Click the end of the plane, away from its start."; status(); return; }
    S.draft = null;
    // a mostly horizontal stroke makes a horizontal line: a plane normal to the view's vertical axis
    const axis = Math.abs(dx) >= Math.abs(dy) ? V.v : V.h;
    const at = Math.round(axis === V.h ? dr.a.u : dr.a.v);
    await change((b) => {
      const id = newId();
      const origin = b.planes.find((p) => p.axis === axis && p.anchor.type === "origin");
      const n = b.planes.filter((p) => /^Reference Plane/.test(p.name)).length + 1;
      b.planes.push({ id, name: `Reference Plane ${n}`, axis, anchor: { type: "offset", from: origin.id, by: String(at) }, ref_kind: "weak" });
      S.sel = { kind: "plane", id };
    });
  }

  async function onDbl(e) {
    if (!S) return;
    if (skHere()) { FSk.dblclick(); return; }
    const d = S.draft;
    if (d && d.kind === "poly") { await finishPoly(); render(); }
    if (d && d.kind === "path") { await finishPath(); render(); }
  }

  function onWheel(e) {
    if (!S || S.view === "3d") return;
    e.preventDefault();
    const m = mouse(e), c = cam();
    const f = e.deltaY < 0 ? 1.15 : 1 / 1.15;
    c.ox = m.x - (m.x - c.ox) * f; c.oy = m.y - (m.y - c.oy) * f; c.k *= f;
    render();
  }

  async function onKey(e) {
    if (skActive()) {
      const k = e.key.toLowerCase();
      if ((e.ctrlKey || e.metaKey) && k === "z") { e.preventDefault(); FSk.undo(); return; }
      if ((e.ctrlKey || e.metaKey) && k === "y") { e.preventDefault(); FSk.redo(); return; }
      if (skHere() && FSk.key(e)) { e.preventDefault(); return; }
      if (e.key === "Escape" || e.key === "Delete") e.preventDefault();
      return;
    }
    if (e.key === "Escape") { if (S.alignRef || S.blend || S.rev) { S.alignRef = null; S.blend = null; S.rev = null; S.draft = null; S.msg = "Cancelled."; status(); render(); } else if (S.draft || (S.picks && S.picks.length) || (S.mod && S.mod.pts.length)) { S.draft = null; S.picks = []; S.mod = { pts: [] }; render(); } else if (S.tool !== "select") { setTool("select"); } else if (S.sel && !S.editing) { S.sel = null; props(); render(); } e.preventDefault(); e.stopPropagation(); return; }
    if (e.key === "Delete") { deleteSel(); e.preventDefault(); return; }
    if (e.key === "Enter" && S.draft && S.draft.kind === "poly") { await finishPoly(); render(); e.preventDefault(); return; }
    if (e.key === "Enter" && S.draft && S.draft.kind === "path") { await finishPath(); render(); e.preventDefault(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); e.stopPropagation(); await undo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); e.stopPropagation(); await redo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") { e.preventDefault(); e.stopPropagation(); await save(false); }
  }

  // ---------------------------------------------------------------- work planes
  const AXIS_NAME = { x: "left/right", y: "front/back", z: "level" };
  const VIEW_FOR_AXIS = { z: "plan", y: "front", x: "left" }; // the view that looks straight at a plane of each kind
  const EDGE_VIEW_FOR_AXIS = { z: "front", y: "plan", x: "plan" }; // a view that shows one edge-on, as a line

  // Make a plane the work plane of the view that faces it (3D keeps its own, used by Show and Viewer)
  function applyWorkPlane(id) {
    const p = planeById(id);
    if (!p) return;
    if (S.view === "3d") S.wps["3d"] = id;
    else {
      const v = VIEWS[S.view].n === p.axis ? S.view : VIEW_FOR_AXIS[p.axis];
      S.wps[v] = id;
      if (v !== S.view) openFamView(v);
    }
    S.msg = S.view === "3d" ? `Work plane: ${p.name}. Viewer looks straight at it.` : `Work plane: ${p.name}.`;
    toolbar(); status(); render();
  }
  function setWorkPlane() {
    if (!S) return;
    openModal("Work Plane", (body, foot, close) => {
      body.append(el("div", "famed-hint", "Specify a new work plane. New sketches are drawn on it."));
      const cur = S.view === "3d" ? S.wps["3d"] : workPlane();
      const sel = selIn(S.body.planes.map((p) => [p.id, `${p.name}  (${AXIS_NAME[p.axis]})`]), cur !== undefined ? cur : S.body.planes[0].id, () => {});
      body.append(field("Name", sel));
      foot.append(button("Cancel", close), button("Pick a Plane", () => { close(); startPickPlane({ mode: "set" }); }), button("OK", () => { close(); applyWorkPlane(Number(sel.value)); }, true));
    });
  }
  // Pick New / Pick a Plane: click a reference plane in a view that shows it as a line
  function startPickPlane(o) {
    S.pick = o;
    if (o.mode === "form") { o.returnView = S.view; openFamView(EDGE_VIEW_FOR_AXIS[planeById(formById(o.form).shape.plane).axis]); }
    else if (S.view === "3d") openFamView("plan");
    setTool("pickwp");
    S.msg = "Click the reference plane to use."; status();
  }
  async function finishPick(pl) {
    const o = S.pick; S.pick = null;
    setTool("select");
    if (!o) return;
    if (o.mode === "set") { applyWorkPlane(pl.id); return; }
    const f = formById(o.form);
    if (!f) return;
    const cur = planeById(f.shape.plane);
    openFamView(o.returnView);
    S.sel = { kind: "form", id: o.form };
    if (pl.axis !== cur.axis) { S.msg = `${pl.name} faces another way. Pick a plane parallel to ${cur.name}.`; props(); status(); return; }
    const ok = await change((b) => { formById(o.form).shape.plane = pl.id; });
    if (ok) { S.msg = `The work plane of ${f.name} is now ${pl.name}.`; status(); }
  }
  function editWorkPlane(id) {
    const f = formById(id);
    if (!f) return;
    const cur = planeById(f.shape.plane);
    openModal("Work Plane", (body, foot, close) => {
      body.append(el("div", "famed-hint", `The plane ${f.name} is sketched on. It has to be parallel to ${cur.name} (Pick New clicks one in a view).`));
      const sel = selIn(S.body.planes.filter((p) => p.axis === cur.axis).map((p) => [p.id, p.name]), cur.id, () => {});
      body.append(field("Name", sel));
      foot.append(button("Cancel", close), button("OK", () => { close(); change((b) => { formById(id).shape.plane = Number(sel.value); }); }, true));
    });
  }
  function viewerCmd() {
    const id = S.view === "3d" ? S.wps["3d"] : workPlane();
    const p = id !== undefined ? planeById(id) : null;
    if (!p) { S.msg = "Set a work plane first (Work Plane > Set)."; status(); return; }
    if (S.view !== "3d" && VIEWS[S.view].n === p.axis) { S.msg = "This view already looks straight at the work plane."; status(); return; }
    const v = VIEW_FOR_AXIS[p.axis];
    S.wps[v] = id;
    openFamView(v);
    S.msg = `Looking straight at ${p.name}.`; status();
  }

  // ---------------------------------------------------------------- the panels the Modify tab adds, and edit mode
  const isPlain = (v) => /^\s*-?\d+(\.\d+)?\s*$/.test(String(v));
  function depthNow(f) { return String(Math.round((curDepth(f.id, "end") - curDepth(f.id, "start")) * 100) / 100); }
  async function setDepth(id, v) {
    const d = Number(String(v).replace(/\s*mm$/i, ""));
    if (!Number.isFinite(d) || d === 0) { S.msg = "Type a depth in millimetres."; status(); ctxRefresh(); return; }
    await change((b) => {
      const sh = formById(id).shape;
      sh.end = isPlain(sh.start) ? String(Math.round((Number(sh.start) + d) * 100) / 100) : `${sh.start} + (${d})`;
    });
  }
  function depthBox(f) {
    const sh = f.shape;
    const driven = sh.end_lock || assocOf(sh.end, "length") || !isPlain(sh.end);
    const wrap = el("div", "fe-ctxbox");
    const inp = el("input", "fe-depth"); inp.type = "text"; inp.value = depthNow(f); inp.disabled = !!driven;
    inp.title = driven ? "The end is locked, or driven by a parameter or a formula. Change it in Properties." : "Depth: the end measured from the start";
    inp.onchange = () => setDepth(f.id, inp.value);
    inp.onkeydown = (e) => { e.stopPropagation(); };
    wrap.append(el("span", "", "Depth"), inp);
    return wrap;
  }
  const NOUN = (f) => ({ extrusion: "Extrusion", blend: "Blend", revolve: "Revolve", sweep: "Sweep", swept_blend: "Swept Blend" }[f.shape.type]);
  // What the ribbon shows for the selection: null for none. main.js draws it as a "Modify | ..." tab.
  function context() {
    const c = contextBase();
    if (c) c.key = S.sk ? `sk:${FSk.tool}` : S.editing ? `edit:${S.editing.id}` : `${S.sel.kind}:${S.sel.id}`;
    return c;
  }
  function contextBase() {
    if (!S || app.doc !== "family") return null;
    if (skActive()) return skContext();
    if (S.editing) return editContext();
    const sel = S.sel;
    if (!sel) return null;
    const auto = S.tool === "select";
    if (sel.kind === "multi") return { auto, title: "Multi-Select", groups: [] };
    if (sel.kind === "form") {
      const f = formById(sel.id);
      if (!f) return null;
      const noun = NOUN(f), title = (f.void ? "Void " : "") + noun;
      const form = [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => formById(sel.id)) }];
      if (["extrusion", "blend"].includes(f.shape.type)) form.push({ big: true, node: depthBox(f) });
      return { auto, title, groups: [
        { label: "Mode", items: [{ label: `Edit ${noun}`, ico: "Ed", big: true, onclick: () => startEdit(sel.id) }] },
        { label: "Form", items: form },
        { label: "Work Plane", items: [{ label: "Edit Work Plane", ico: "Wp", big: true, onclick: () => editWorkPlane(sel.id) }, { label: "Pick New", ico: "Pk", big: true, onclick: () => startPickPlane({ mode: "form", form: sel.id }) }] },
      ] };
    }
    if (sel.kind === "mline") return { auto, title: "Model Lines", groups: [{ label: "Graphics", items: [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => mlineById(sel.id)) }] }] };
    if (sel.kind === "comp") return { auto, title: "Components", groups: [
      { label: "Graphics", items: [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => compById(sel.id)) }] },
      { label: "Component", items: [{ label: "Rotate 90", ico: "Ro", big: true, onclick: () => change((b) => { const c = compById(sel.id); c.rotation_deg = ((c.rotation_deg || 0) + 90) % 360; }) }, { label: "Mirror", ico: "Mi", big: true, onclick: () => change((b) => { const c = compById(sel.id); c.mirror = !c.mirror; }) }] },
    ] };
    if (sel.kind === "mtext") return { auto, title: "Model Text", groups: [{ label: "Graphics", items: [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => mtextById(sel.id)) }] }] };
    if (sel.kind === "ctrl") return { auto, title: "Controls", groups: [{ label: "Graphics", items: [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => ctrlById(sel.id)) }] }] };
    if (sel.kind === "conn") return { auto, title: "Connectors", groups: [{ label: "Connector", items: [{ label: "Flip", ico: "Fl", big: true, onclick: () => change((b) => { const c = connById(sel.id); c.flip = !c.flip; }) }] }] };
    if (sel.kind === "sym") return { auto, title: "Symbolic Lines", groups: [{ label: "Graphics", items: [{ label: "Visibility Settings", ico: "Vs", big: true, onclick: () => visDialog(() => symById(sel.id)) }] }] };
    if (sel.kind === "dim") {
      const d = dimById(sel.id);
      if (!d) return null;
      const refs = dimRefs(d), n = refs.length - 1, single = n === 1 || d.eq;
      const lens = S.params.filter((p) => ["length", "number"].includes(p.param_kind) && !p.reporting && !p.formula).map((p) => [p.name, p.name]);
      const box = el("div", "fe-ctxbox");
      if (single) {
        box.append(el("span", "", "Label"), selIn([["", "<None>"], ...lens], d.label || "", (v) => dimPatch(dimById(sel.id), { label: v, reports_to: "" })), button("Add Parameter", () => addParamToDim(dimById(sel.id), 0)));
      } else box.append(el("span", "", "Label each length in Properties"));
      return { auto, title: "Dimensions", groups: [
        { label: "Label Dimension", items: [{ big: true, node: box }] },
        { label: "Constrain", items: [
          { label: d.locked ? "Unlock" : "Lock", ico: "Lk", big: true, on: !!d.locked, onclick: () => dimPatch(dimById(sel.id), { locked: !d.locked }) },
          ...(refs.length >= 3 ? [{ label: "Equal", ico: "EQ", big: true, on: !!d.eq, onclick: () => toggleEq(sel.id) }] : []),
        ] },
      ] };
    }
    return null;
  }
  function editContext() {
    const f = formById(S.editing.id);
    if (!f) return null;
    const canDraw = ["extrusion", "revolve"].includes(f.shape.type);
    const shape = (v, label, ico) => ({ label, ico, onclick: () => { S.sketch = v; setTool("ext"); }, on: S.tool === "ext" && S.sketch === v });
    // the form keeps its own name on the tab while it is edited: Modify | Extrusion
    return { auto: true, title: NOUN(f), groups: [
      { label: "Mode", items: [{ label: "Finish Edit Mode", ico: "\u2713", big: true, onclick: finishEdit }, { label: "Cancel Edit Mode", ico: "\u2715", big: true, onclick: cancelEdit }] },
      ...(canDraw ? [{ label: "Draw", items: [shape("rect", "Rectangle", "Rc"), shape("circle", "Circle", "Ci"), shape("poly", "Polygon", "Pg")] }] : []),
      ...(canDraw ? [{ label: "Loop", items: [{ label: "Delete Loop", ico: "Dl", onclick: () => deleteLoop() }] }] : []),
    ] };
  }
  // Edit a form: its sketch opens in the sketch tools (Edit Extrusion, Edit Blend, Edit Revolve, Edit Path)
  function startEdit(id) {
    if (formById(id)) startFormSketch({ form: id });
  }
  function finishEdit() {
    const e = S.editing;
    if (!e) return;
    S.editing = null;
    // everything done in the mode undoes as one step
    if (S.undo.length > e.undoLen) { S.undo.length = Math.min(S.undo.length, e.undoLen); S.undo.push(e.before); }
    setTool("select"); props();
    S.msg = ""; status(); render();
  }
  async function cancelEdit() {
    const e = S.editing;
    if (!e) return;
    S.editing = null;
    restore(e.before);
    S.undo.length = Math.min(S.undo.length, e.undoLen);
    S.sel = formById(e.id) ? { kind: "form", id: e.id } : null;
    await preview();
    setTool("select"); props();
    S.msg = "Edit cancelled."; status(); render();
  }
  // the loop's outline as points, arcs included
  function flatten(loop) {
    const nd = loop.nodes.map(nodePos), n = nd.length, out = [nd[0]];
    for (let i = 0; i < (loop.closed ? n : n - 1); i++) for (const w of bulgePoints(nd[i], nd[(i + 1) % n], (loop.bulges || [])[i] || 0)) out.push(w);
    return out;
  }
  function hitLoop(sx, sy) {
    const f = formById(S.editing.id);
    if (!f || !f.shape.profile) return null;
    let best = null, bd = 8;
    f.shape.profile.forEach((loop, li) => {
      const pts = flatten(loop).map((q) => w2s(q.u, q.v));
      for (let i = 0; i + 1 < pts.length; i++) {
        const a = pts[i], b = pts[i + 1], l2 = (b.x - a.x) ** 2 + (b.y - a.y) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((sx - a.x) * (b.x - a.x) + (sy - a.y) * (b.y - a.y)) / l2));
        const d = Math.hypot(a.x + t * (b.x - a.x) - sx, a.y + t * (b.y - a.y) - sy);
        if (d < bd) { bd = d; best = li; }
      }
    });
    return best;
  }
  async function deleteLoop() {
    const e = S.editing, f = e && formById(e.id);
    if (!f || !f.shape.profile) return;
    if (e.loop == null) { S.msg = "Click the edge of a loop to select it first."; status(); return; }
    if (f.shape.profile.length < 2) { S.msg = "A form needs at least one loop. Cancel the edit, or delete the form instead."; status(); return; }
    const li = e.loop;
    e.loop = null;
    await change((b) => { formById(e.id).shape.profile.splice(li, 1); });
  }

  // ---------------------------------------------------------------- sketch mode for forms
  // Extrusion, Void Extrusion and Edit Extrusion sketch with the project's sketch tools: a second session of
  // sketch.js (makeSketch) drawn on this canvas in the view's own axes, in mm. A straight sketch line lying on a
  // reference plane shows a padlock; a locked line ties its points to the plane. Finish turns the curves into
  // the form's loops (openarch_model::famsketch); Cancel leaves the family as it was.
  const FSk = makeSketch();
  const SK_DRAW = FSk.DRAW.filter((t) => t.id !== "pick_walls");
  const SK_DRAW_IDS = new Set(SK_DRAW.map((t) => t.id));
  const skActive = () => !!(S && S.sk && FSk.active);
  const skHere = () => skActive() && S.view === S.sk.view;
  // Pick Lines and Align take reference planes and the edges of the geometry drawn in this view
  function skEdges(sx, sy) {
    const out = [], V = VIEWS[S.view], tol = 8;
    const segDist = (a, b) => {
      const A = w2s(a[0], a[1]), B = w2s(b[0], b[1]), l2 = (B.x - A.x) ** 2 + (B.y - A.y) ** 2 || 1;
      const t = Math.max(0, Math.min(1, ((sx - A.x) * (B.x - A.x) + (sy - A.y) * (B.y - A.y)) / l2));
      return Math.hypot(A.x + t * (B.x - A.x) - sx, A.y + t * (B.y - A.y) - sy);
    };
    const add = (a, b) => out.push({ a: { x: a[0], y: a[1] }, b: { x: b[0], y: b[1] } });
    for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) for (const r of pc.rings) for (let i = 0; i < r.length; i++) { const a = r[i], b = r[(i + 1) % r.length]; if (segDist(a, b) <= tol) add(a, b); }
    for (const y of S.pv.syms || []) if (y.view === S.view) for (let i = 0; i + 1 < y.points.length; i++) if (segDist(y.points[i], y.points[i + 1]) <= tol) add(y.points[i], y.points[i + 1]);
    // a plane gives a line across what the view shows
    const c0 = s2w(0, 0), c1 = s2w(S.canvas.width, S.canvas.height);
    const [u0, u1, v0, v1] = [Math.min(c0.u, c1.u), Math.max(c0.u, c1.u), Math.min(c0.v, c1.v), Math.max(c0.v, c1.v)];
    for (const p of viewPlanes()) {
      const at = pos(p.id);
      if (at === undefined) continue;
      const seg = p.axis === V.h ? [[at, v0], [at, v1]] : [[u0, at], [u1, at]];
      if (segDist(seg[0], seg[1]) <= tol) add(seg[0], seg[1]);
    }
    return out;
  }
  // The snap index while sketching: plane lines (no ends or middles of their own), and the edges drawn in the view
  function skSnapIndex() {
    const key = `${S.view}|${S.tick}`;
    if (Snap.owner === "family" && S.snapKey === key) return;
    const segs = [], V = VIEWS[S.view], R = 50000;
    for (const p of viewPlanes()) {
      const at = pos(p.id);
      if (at === undefined) continue;
      segs.push(p.axis === V.h ? [at, -R, at, R, 2] : [-R, at, R, at, 2]);
    }
    // the forms' edges are faces: they have extensions
    for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) for (const r of pc.rings) for (let i = 0; i < r.length; i++) { const a = r[i], b = r[(i + 1) % r.length]; segs.push([a[0], a[1], b[0], b[1], 3]); }
    for (const y of S.pv.syms || []) if (y.view === S.view) for (let i = 0; i + 1 < y.points.length; i++) segs.push([...y.points[i], ...y.points[i + 1]]);
    Snap.build(segs, "family");
    S.snapKey = key;
  }
  function skSnap(sx, sy) {
    skSnapIndex();
    const w = s2w(sx, sy);
    return Snap.resolve({ x: w.u, y: w.v }, { zoom: cam().k, last: FSk.last(), extra: FSk.segs(), points: FSk.points(), track: FSk.trackPoints(), cycle: S.snapCycle || 0, shift: !!S.shift, gridStep: 0 });
  }
  FSk.attach({
    get ctx() { return S && S.ctx; },
    get canvas() { return S && S.canvas; },
    size: () => ({ w: S.canvas.width, h: S.canvas.height }),
    toScreen: (p) => w2s(p.x, p.y),
    toWorld: (sx, sy) => { const w = s2w(sx, sy); return { x: w.u, y: w.v }; },
    label: (text, at, color) => {
      const ctx = S.ctx;
      ctx.font = "11px system-ui, sans-serif";
      const m = ctx.measureText(text);
      ctx.fillStyle = css("--panel"); ctx.fillRect(at.x - m.width / 2 - 3, at.y - 8, m.width + 6, 16);
      ctx.fillStyle = color; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillText(text, at.x, at.y);
    },
    css, render: () => render(),
    scale: () => cam().k,
    wallAt: () => null, wallById: () => null, wallChain: (id) => [id],
    edgeAt: (sx, sy) => skEdges(sx, sy)[0] || null,
    edgesNear: (sx, sy) => skEdges(sx, sy),
    cycleSnap: (d) => { S.snapCycle = (S.snapCycle || 0) + d; if (S.mouse) { S.skHover = skSnap(S.mouse.x, S.mouse.y); FSk.move(S.skHover, S.mouse.x, S.mouse.y); } render(); },
  });
  FSk.callbacks.finish = () => finishFormSketch();
  FSk.callbacks.cancel = () => { endFormSketch(); S.msg = "Sketch cancelled."; status(); };
  FSk.callbacks.prompt = (t, err) => { if (!S) return; S.msg = t; S.msgErr = !!err; status(); };
  FSk.callbacks.change = () => {
    if (!S || !S.sk) return;
    const sk = S.sk, st = FSk.state();
    if (!sk.starting) {
      // a new line takes Lock to planes when it was drawn, or the lock of a line it replaced on the same plane
      const now = new Set(st.curves.map((c) => c.uid));
      const gone = [...sk.seen].filter((u) => !now.has(u) && sk.alive.has(u));
      const goneLocked = new Set(gone.flatMap((u) => skLockCands(sk.alive.get(u)).filter((l) => l.on).map((l) => `${l.plane}|${l.coord}`)));
      for (const c of st.curves) {
        if (sk.seen.has(c.uid)) continue;
        sk.seen.add(c.uid);
        if (SK_DRAW_IDS.has(st.tool)) sk.lockDefault.set(c.uid, !!S.lockNew);
        else for (const l of skLockCands(c)) if (goneLocked.has(`${l.plane}|${l.coord}`)) sk.locks.set(c.uid, [...(sk.locks.get(c.uid) || []), { plane: l.plane, coord: l.coord, on: true }]);
      }
      sk.alive = new Map(st.curves.map((c) => [c.uid, c]));
    }
    toolbar(); status(); ctxRefresh(); render();
  };
  // the planes a straight sketch line lies on, with whether it is locked to each
  function skLockCands(c) {
    const out = [];
    if (!c || !c.curve || c.curve.type !== "line" || !S.sk) return out;
    const V = VIEWS[S.sk.view], { a, b } = c.curve;
    for (const [coord, axis, ka, kb, oa, ob] of [["u", V.h, a.x, b.x, a.y, b.y], ["v", V.v, a.y, b.y, a.x, b.x]]) {
      if (Math.abs(ka - kb) > 0.01 || Math.abs(oa - ob) < 0.01) continue;
      for (const p of S.body.planes) {
        if (p.axis !== axis) continue;
        const at = pos(p.id);
        if (at === undefined || Math.abs(at - ka) > 0.01) continue;
        const set = (S.sk.locks.get(c.uid) || []).find((x) => x.plane === p.id && x.coord === coord);
        out.push({ uid: c.uid, plane: p.id, coord, a, b, on: set ? set.on : !!S.sk.lockDefault.get(c.uid) });
      }
    }
    return out;
  }
  function drawSkLocks(accent) {
    S.skLockHits = [];
    const st = FSk.state();
    for (const c of st ? st.curves : []) {
      for (const l of skLockCands(c)) {
        const m = w2s((l.a.x + l.b.x) / 2, (l.a.y + l.b.y) / 2);
        const px = l.coord === "u" ? m.x + 14 : m.x, py = l.coord === "u" ? m.y : m.y - 14;
        padlock(S.ctx, px, py, l.on, l.on ? accent : "#888");
        S.skLockHits.push({ ...l, x: px - PADLOCK_HIT, y: py - PADLOCK_HIT, w: 2 * PADLOCK_HIT, h: 2 * PADLOCK_HIT });
      }
    }
  }
  function toggleSkLock(h) {
    const list = (S.sk.locks.get(h.uid) || []).filter((x) => !(x.plane === h.plane && x.coord === h.coord));
    list.push({ plane: h.plane, coord: h.coord, on: !h.on });
    S.sk.locks.set(h.uid, list);
    const name = (planeById(h.plane) || {}).name;
    S.msg = h.on ? `Unlocked from ${name}.` : `Locked to ${name}. The line follows the plane when it moves.`; status(); render();
  }

  // A form's sketch: Extrusion and Void Extrusion (closed loops), Blend (closed loops for its base, then its top, switched
  // with Edit Top and Edit Base), Revolve (closed loops of Boundary Line, and one Axis Line), Sweep and Swept Blend
  // (Sketch Path: one chain of lines and arcs, open or closed; the profile is set in Properties). o: { kind, isVoid,
  // form (editing that form) }.
  const SK_KIND = { extrusion: "ext", blend: "blend", revolve: "revolve", sweep: "sweep", swept_blend: "swblend" };
  const AXIS_INK = "#1f6fd1";
  async function curvesOfLoops(loops) {
    try { return await Backend.call("family_sketch_of", { loops, planes: S.pv.planes }); }
    catch (e) { S.msg = String(e.message || e); status(); return null; }
  }
  async function startFormSketch(o) {
    const f = o.form != null ? formById(o.form) : null;
    const kind = f ? (f.shape.type === "extrusion" ? (f.void ? "void" : "ext") : SK_KIND[f.shape.type]) : o.kind;
    const isVoid = f ? !!f.void : kind === "void" || !!o.isVoid;
    const plane = f ? f.shape.plane : workPlane();
    const axis = (planeById(plane) || {}).axis;
    if (S.view === "3d" || !VIEWS[S.view] || VIEWS[S.view].n !== axis) openFamView(VIEW_FOR_AXIS[axis]);
    // what an existing form's sketch holds, as curves (and for a blend, its top too; for a revolve, its axis)
    let first = { curves: [], locks: [], kept: [] }, top = null;
    if (f) {
      const sh = f.shape;
      const loops = sh.type === "blend" ? sh.base : sh.type === "sweep" || sh.type === "swept_blend" ? [sh.path] : sh.profile;
      const r = await curvesOfLoops(loops);
      if (!r) return;
      first = r;
      if (sh.type === "blend") { const t = await curvesOfLoops(sh.top); if (!t) return; top = t; }
      if (sh.type === "revolve") {
        const pt = (n) => { const u = (n.u.plane != null ? pos(n.u.plane) || 0 : 0) + n.u.off, v = (n.v.plane != null ? pos(n.v.plane) || 0 : 0) + n.v.off; return { x: u, y: v }; };
        try {
          const ax = await Backend.call("sketch_describe", { curves: [{ type: "line", a: pt(sh.axis_a), b: pt(sh.axis_b) }] });
          first.curves.push({ ...ax.curves[0], attr: { axis: true } });
        } catch (e) { S.msg = String(e.message || e); status(); return; }
      }
    }
    S.tool = kind === "void" ? "void" : kind;
    S.draft = null;
    const sh = f ? f.shape : null;
    S.sk = {
      form: f ? f.id : null, kind, isVoid, plane, view: S.view, stage: "base",
      start: sh && sh.start != null ? sh.start : S.start, end: sh && sh.end != null ? sh.end : S.end,
      a0: sh && sh.start_deg != null ? String(sh.start_deg) : S.a0, a1: sh && sh.end_deg != null ? String(sh.end_deg) : S.a1,
      locks: new Map(), lockDefault: new Map(), seen: new Set(), alive: new Map(), kept: [...first.kept, ...(top ? top.kept : [])], starting: true,
      drawAxis: false,
      // the stage not being drawn (a blend's top while its base is, and the other way)
      other: top ? { curves: top.curves, locks: top.locks } : { curves: [], locks: [] },
    };
    beginStage(first.curves, first.locks, f ? "select" : "line");
    S.sel = f ? { kind: "form", id: f.id } : null;
    S.msg = f ? `Editing ${f.name}. Change it with the Draw and Modify tools, then Finish Edit Mode.` : skHint();
    props(); toolbar(); status(); render(); ctxRefresh();
  }
  // what the sketch is for, in a sentence
  function skHint() {
    const k = S.sk.kind;
    if (k === "blend") return S.sk.stage === "base" ? "Sketch the base: one closed loop. Then Edit Top and sketch the top with the same number of points, and Finish Edit Mode." : "Sketch the top: one closed loop with as many points as the base. Edit Base goes back to the base.";
    if (k === "revolve") return "Sketch the profile with Boundary Line (closed loops), then draw one Axis Line it turns about. Set the angles in the options bar, then Finish Edit Mode.";
    if (k === "sweep" || k === "swblend") return "Sketch the path: lines and arcs joined end to end, open or closed. Finish Edit Mode, then set the profile in Properties.";
    return "Sketch closed loops with the Draw tools, then Finish Edit Mode. Loops inside loops make holes.";
  }
  function skTitle() {
    const sk = S.sk, k = sk.kind, edit = sk.form != null, v = sk.isVoid ? "Void " : "";
    if (k === "blend") return `${edit ? "Edit" : "Create"} ${v}Blend ${sk.stage === "base" ? "Base" : "Top"} Boundary`;
    if (k === "revolve") return `${edit ? "Edit" : "Create"} ${v}Revolve`;
    if (k === "sweep" || k === "swblend") return `${k === "sweep" ? "Sweep" : "Swept Blend"} > ${edit ? "Edit Path" : "Sketch Path"}`;
    return edit ? `Edit ${sk.isVoid ? "Void " : ""}Extrusion` : k === "void" ? "Create Void Extrusion" : "Create Extrusion";
  }
  // start the sketch session on a set of curves (with locks by index)
  function beginStage(curves, locks, tool) {
    const sk = S.sk;
    sk.starting = true; sk.locks = new Map(); sk.lockDefault = new Map(); sk.seen = new Set();
    const revolve = sk.kind === "revolve";
    FSk.begin({
      kind: "family", title: skTitle(), curves, tool,
      // a revolve's axis and a sweep's path are open: finishing checks those here
      closed: !(revolve || sk.kind === "sweep" || sk.kind === "swblend"),
      ...(revolve ? { attr: (extra) => ({ ...(sk.drawAxis ? { axis: true } : {}), ...(extra || {}) }), ink: (c) => (c.attr && c.attr.axis ? AXIS_INK : "#b0309c") } : {}),
    });
    const st = FSk.state();
    for (const c of st.curves) sk.seen.add(c.uid);
    for (const l of locks) { const c = st.curves[l.curve]; if (c) sk.locks.set(c.uid, [...(sk.locks.get(c.uid) || []), { plane: l.plane, coord: l.coord, on: l.on }]); }
    sk.alive = new Map(st.curves.map((c) => [c.uid, c]));
    sk.starting = false;
  }
  // the session's curves and their locks (by index), to keep or to finish
  function stageNow() {
    const st = FSk.state(), locks = [];
    st.curves.forEach((c, i) => { for (const l of skLockCands(c)) locks.push({ curve: i, plane: l.plane, coord: l.coord, on: l.on }); });
    return { curves: st.curves.map((c) => ({ curve: c.curve, poly: c.poly, closed: c.closed, snaps: c.snaps, attr: c.attr })), locks };
  }
  // a blend's base and top: keep the one drawn, sketch the other
  function switchBlendStage() {
    const sk = S.sk;
    if (!sk || sk.kind !== "blend") return;
    const now = stageNow(), next = sk.other;
    sk.other = now;
    sk.stage = sk.stage === "base" ? "top" : "base";
    FSk.end();
    beginStage(next.curves, next.locks, next.curves.length ? "select" : "line");
    S.msg = skHint(); props(); toolbar(); status(); render(); ctxRefresh();
  }
  function endFormSketch() {
    FSk.end();
    if (!S) return;
    S.sk = null; S.skHover = null; S.skLockHits = [];
    S.tool = "select"; S.voidNext = false; S.openingNext = false; S.drag = null;
    props(); toolbar(); render(); ctxRefresh();
  }
  // the loops of a set of curves (with locks by index), or the reason they do not make any
  async function loopsOf(stage) {
    const r = await Backend.call("family_sketch_loops", { curves: stage.curves.map((c) => c.curve), locks: stage.locks, kept: S.sk.kept, planes: S.pv.planes });
    if (!r.ok) throw new Error(r.error.message);
    return r.loops;
  }
  // a point of the sketch (an axis end) as a node at plain distances
  const skNode = (q) => ({ u: { plane: null, off: Math.round(q.x * 1000) / 1000 }, v: { plane: null, off: Math.round(q.y * 1000) / 1000 } });
  async function finishFormSketch() {
    const sk = S.sk;
    if (!sk || !FSk.state()) return false;
    const now = stageNow(), k = sk.kind, editing = sk.form != null;
    const name = (stem) => { const n = S.body.forms.filter((f) => !!f.void === sk.isVoid && f.name.replace(/^Void /, "").startsWith(stem)).length + 1; return `${sk.isVoid ? "Void " : ""}${stem} ${n}`; };
    let shape, made;
    try {
      if (k === "ext" || k === "void") shape = { type: "extrusion", plane: sk.plane, start: sk.start, end: sk.end, profile: await loopsOf(now) };
      else if (k === "blend") {
        const [base, top] = sk.stage === "base" ? [now, sk.other] : [sk.other, now];
        if (!top.curves.length) throw new Error("Sketch the top as well: Edit Top, then draw one closed loop with as many points as the base.");
        const bl = await loopsOf(base), tl = await loopsOf(top);
        if (bl.length !== 1 || tl.length !== 1) throw new Error("A blend's base and top are one closed loop each.");
        if (bl[0].nodes.length !== tl[0].nodes.length) throw new Error(`The base has ${bl[0].nodes.length} points and the top ${tl[0].nodes.length}: they need the same number.`);
        shape = { type: "blend", plane: sk.plane, start: sk.start, end: sk.end, base: bl, top: tl };
      } else if (k === "revolve") {
        const axes = now.curves.map((c, i) => ({ c, i })).filter((x) => x.c.attr && x.c.attr.axis);
        if (axes.length !== 1 || axes[0].c.curve.type !== "line") throw new Error("A revolve needs one straight Axis Line to turn about: choose Axis Line on the ribbon and draw it.");
        const keep = now.curves.map((c, i) => i).filter((i) => i !== axes[0].i), remap = new Map(keep.map((i, j) => [i, j]));
        const boundary = { curves: keep.map((i) => now.curves[i]), locks: now.locks.filter((l) => remap.has(l.curve)).map((l) => ({ ...l, curve: remap.get(l.curve) })) };
        if (!boundary.curves.length) throw new Error("Sketch the profile with Boundary Line as well as the axis.");
        const ax = axes[0].c.curve;
        const a0 = Number(sk.a0) || 0, a1 = sk.a1 === "" || isNaN(Number(sk.a1)) ? 360 : Number(sk.a1);
        shape = { type: "revolve", plane: sk.plane, axis_a: skNode(ax.a), axis_b: skNode(ax.b), start_deg: a0, end_deg: a1, profile: await loopsOf(boundary) };
      } else {
        const r = await Backend.call("family_sketch_path", { curves: now.curves.map((c) => c.curve), locks: now.locks, kept: sk.kept, planes: S.pv.planes });
        if (!r.ok) throw new Error(r.error.message);
        if (editing) shape = { ...formById(sk.form).shape, path: r.path, plane: sk.plane };
        else {
          const sq = (h) => [{ type: "poly", points: [[-h, -h], [h, -h], [h, h], [-h, h]].map(([u, v]) => ({ u: String(u), v: String(v) })), bulges: [] }];
          shape = k === "swblend" ? { type: "swept_blend", plane: sk.plane, path: r.path, profile: sq(25), profile_end: sq(10), profile_angle: 0, flip: false } : { type: "sweep", plane: sk.plane, path: r.path, profile: sq(25), profile_angle: 0, flip: false };
        }
      }
    } catch (e) { FSk.callbacks.prompt(String(e.message || e), true); return false; }
    let id = sk.form, ok;
    if (editing) {
      // an edit keeps what the form had beyond its sketch (locked ends, materials, the profile of a sweep)
      ok = await change(() => { const f = formById(id); f.shape = { ...f.shape, ...shape }; }, true);
    } else {
      ok = await change((b) => {
        id = newId();
        const stem = { ext: "Extrusion", void: "Extrusion", blend: "Blend", revolve: "Revolve", sweep: "Sweep", swblend: "Swept Blend" }[k];
        const no = b.forms.filter((f) => f.opening).length + 1;
        b.forms.push({
          id, name: S.openingNext && k === "void" ? `Opening ${no}` : name(stem), void: sk.isVoid, cuts_host: true, opening: !!(S.openingNext && k === "void"), cut_by: [],
          shape, material: "", material_param: null, subcategory: sk.isVoid ? "" : "Frame/Mullion", vis: {},
        });
      }, true);
      if (ok) { S.start = sk.start; S.end = sk.end; S.a0 = sk.a0; S.a1 = sk.a1; }
    }
    if (!ok) { FSk.callbacks.prompt(S.msg || "That sketch does not make a form.", true); return false; }
    made = formById(id);
    endFormSketch();
    S.sel = { kind: "form", id };
    S.msg = k === "sweep" || k === "swblend"
      ? `${made ? made.name : "The sweep"} ${editing ? "updated" : "made"}. Set its profile in Properties.`
      : `${made ? made.name : "The form"} ${editing ? "updated" : "made"}. Drag the arrows on its faces to stretch it; a face dropped on a reference plane offers a padlock.`;
    props(); status(); render(); ctxRefresh();
    return true;
  }
  function skContext() {
    const t = FSk.tool, sk = S.sk;
    const mode = [{ label: "Finish Edit Mode", ico: "✓", big: true, onclick: () => FSk.finish() }, { label: "Cancel Edit Mode", ico: "✕", big: true, onclick: () => FSk.cancel() }];
    if (sk && sk.kind === "blend") mode.push(sk.stage === "base" ? { label: "Edit Top", ico: "Ed", big: true, onclick: () => switchBlendStage() } : { label: "Edit Base", ico: "Ed", big: true, onclick: () => switchBlendStage() });
    // a revolve draws two kinds of line: its boundary and the axis it turns about
    const kinds = sk && sk.kind === "revolve" ? [
      { label: "Boundary Line", icon: "symbolicLine", on: !sk.drawAxis, onclick: () => { sk.drawAxis = false; ctxRefresh(); status(); } },
      { label: "Axis Line", icon: "refLine", on: !!sk.drawAxis, onclick: () => { sk.drawAxis = true; if (FSk.tool !== "line") FSk.setTool("line"); S.msg = "Axis Line: click its two points."; ctxRefresh(); status(); } },
    ] : [];
    const trims = FSk.MODIFY.filter((x) => ["trim_corner", "trim_single", "split"].includes(x.id)).map((x) => ({ ...x, onclick: () => FSk.setTool(x.id), on: t === x.id }));
    return { auto: true, title: sk ? skTitle() : FSk.title, groups: [
      { label: "Mode", items: mode },
      { label: "Draw", items: [...kinds, ...SK_DRAW.map((x) => ({ ...x, onclick: () => FSk.setTool(x.id), on: t === x.id }))] },
      { label: "Work Plane", items: [{ label: "Set", icon: "setWp", onclick: () => commands["f-wpset"]() }, { label: "Show", icon: "showWp", on: !!S.showWp, onclick: () => commands["f-wpshow"]() }, { label: "Viewer", icon: "viewer", onclick: () => commands["f-wpviewer"]() }] },
      ...(trims.length ? [{ label: "Trim and Split", items: trims }] : []),
    ] };
  }
  const worldRect = (x0, y0, x1, y1) => { const a = s2w(x0, y0), b = s2w(x1, y1); return { x0: Math.min(a.u, b.u), y0: Math.min(a.v, b.v), x1: Math.max(a.u, b.u), y1: Math.max(a.v, b.v) }; };
  // Pointer and keys while sketching (in the sketch's own view). Returns true when the sketch took the event.
  async function skDown(e, m) {
    if (!skActive()) return false;
    if (S.view !== S.sk.view) { S.msg = `The sketch is in ${VIEW_TITLE[S.sk.view]}. Go back to it to keep sketching, or Finish there.`; status(); return true; }
    const lk = (S.skLockHits || []).find((q) => m.x >= q.x && m.x <= q.x + q.w && m.y >= q.y && m.y <= q.y + q.h);
    if (lk) { toggleSkLock(lk); return true; }
    if (FSk.gripDown(m.x, m.y)) { S.drag = { skGrip: true }; return true; }
    S.drag = { sk: true, x: m.x, y: m.y };
    return true;
  }
  function skMove(e, m) {
    if (!skHere() || (S.drag && (S.drag.pan || S.drag.rot))) return false;
    S.shift = e.shiftKey;
    const d = S.drag;
    if (d && d.skGrip) { S.skHover = skSnap(m.x, m.y); FSk.gripMove(m.x, m.y, S.skHover); return true; }
    if (d && d.sk && (d.band || (Math.hypot(m.x - d.x, m.y - d.y) > 5 && FSk.canBand()))) { d.band = true; d.cur = m; render(); return true; }
    if (!S.lastSkMouse || Math.hypot(m.x - S.lastSkMouse.x, m.y - S.lastSkMouse.y) > 3) S.snapCycle = 0;
    S.lastSkMouse = m;
    S.skHover = skSnap(m.x, m.y);
    FSk.move(S.skHover, m.x, m.y);
    render();
    return true;
  }
  async function skUp(e, m, d) {
    if (!d || !(d.sk || d.skGrip)) return false;
    if (d.skGrip) { await FSk.gripUp(); return true; }
    if (d.band) { FSk.bandSelect(worldRect(d.x, d.y, m.x, m.y), m.x >= d.x, e); render(); return true; }
    if (Math.hypot(m.x - d.x, m.y - d.y) < 5) {
      const p = skSnap(m.x, m.y);
      Snap.clearOverride(); S.snapCycle = 0;
      await FSk.click(p, m.x, m.y, e);
    }
    return true;
  }
  function drawSketchOverlay(accent) {
    if (!skHere()) return;
    const ctx = S.ctx;
    FSk.draw(ctx);
    drawSkLocks(accent);
    const h = S.skHover;
    if (h && FSk.wantsSnap() && !["free", "length"].includes(h.kind)) {
      const p = w2s(h.x, h.y);
      Snap.drawGuides(ctx, h, (q) => w2s(q.x, q.y));
      Snap.glyph(ctx, h.kind, p.x, p.y);
      if (h.label) { ctx.font = "11px system-ui, sans-serif"; ctx.fillStyle = "#d4572f"; ctx.textAlign = "left"; ctx.textBaseline = "top"; ctx.fillText(h.label, p.x + 10, p.y + 8); }
    }
  }

  // ---------------------------------------------------------------- making things
  // ---- dimensions ----
  async function toggleEq(id) {
    const d = id ? dimById(id) : S.sel && S.sel.kind === "dim" ? dimById(S.sel.id) : null;
    if (!d) { S.msg = "Select a dimension with three or more references first."; status(); return; }
    if (dimRefs(d).length < 3) { S.msg = "EQ needs a dimension across three or more references."; status(); return; }
    S.sel = { kind: "dim", id: d.id };
    await edit("patch_dim", { dim: d.id, patch: { eq: !d.eq } });
  }
  async function dimPatch(d, patch) { return edit("patch_dim", { dim: d.id, patch }); }

  // The segment's value: a labelled dimension changes its parameter for the open type; a plain one moves its plane.
  // `movePlane`: the selected plane, which moves when the value is typed (when it can)
  function editDimValue(d, seg, movePlane) {
    const refs = dimRefs(d), single = refs.length === 2 || d.eq;
    // an EQ dimension's value is its whole length, not one of its equal parts
    const val = d.eq ? Math.abs(pos(refs[refs.length - 1]) - pos(refs[0])) : Math.abs(pos(refs[seg + 1]) - pos(refs[seg]));
    const label = single ? d.label : (d.seg_labels || [])[seg];
    if (d.reports_to) { S.msg = `${d.reports_to} reads this dimension, so it cannot be typed over.`; status(); return; }
    if (label) {
      const p = S.params.find((q) => q.name === label);
      askText("Dimension", label, String(Math.round(val * 100) / 100), [], async (v) => {
        await setFlex(p, num(v));
        S.saveVals = { ...(S.saveVals || {}), ...(p.instance ? {} : { [p.name]: num(v) }) }; touch();
        render();
      });
      return;
    }
    if (d.eq) { S.msg = "Equal segments follow the whole length: label the dimension and change its parameter."; status(); return; }
    askText("Dimension", "Length", String(Math.round(val * 100) / 100), [], (v) => edit("dim_value", { dim: d.id, seg, value: num(v), ...(movePlane != null ? { move: movePlane } : {}) }));
  }
  // a temporary dimension's value typed: the selected plane moves to that distance from the other plane
  function editTempDim(h) {
    askText("Dimension", "Length", String(Math.round(h.value * 100) / 100), [], async (v) => {
      const n = num(v);
      if (!(n >= 0)) return;
      S.sel = { kind: "plane", id: h.plane };
      await edit("move_plane", { plane: h.plane, to: pos(h.other) + h.sign * n });
      S.sel = { kind: "plane", id: h.plane }; props(); render();
    });
  }

  async function clickDim(h) {
    if (h.kind === "tval") { editTempDim(h); return; }
    const d = dimById(h.id);
    if (!d) return;
    // with a reference plane selected, a dimension to it takes a typed value at once and moves that plane
    const plane = S.sel && S.sel.kind === "plane" && dimRefs(d).includes(S.sel.id) ? S.sel.id : null;
    if (h.kind === "val" && plane != null) { editDimValue(d, h.seg, plane); return; }
    const was = S.sel && S.sel.kind === "dim" && S.sel.id === d.id;
    S.sel = { kind: "dim", id: d.id };
    if (h.kind === "lock") { await dimPatch(d, { locked: !d.locked }); return; }
    if (h.kind === "eq") { await toggleEq(d.id); return; }
    props(); render();
    if (h.kind === "val" && was) { editDimValue(d, h.seg); return; }
    if (h.kind === "line" || h.kind === "val") S.drag = { dimAt: d.id, before: snapshot(), moved: false };
  }

  // ---- shared parameters: a text file of parameters with fixed GUIDs, kept outside any family ----
  const SP_KEY = "openarch.shared_params";
  const sharedPath = () => { try { return localStorage.getItem(SP_KEY) || ""; } catch (e) { return ""; } };
  const setSharedPath = (v) => { try { v ? localStorage.setItem(SP_KEY, v) : localStorage.removeItem(SP_KEY); } catch (e) { /* the path is not remembered */ } };
  const SP_TYPES = [["length", "Length"], ["number", "Number"], ["integer", "Integer"], ["angle", "Angle"], ["yes_no", "Yes/No"], ["text", "Text"], ["material", "Material"]];
  const spCall = (op, args) => Backend.call("shared_params", { path: sharedPath(), op, ...args });

  // Shared Parameters: make or choose the file, then add groups and parameters to it. With `pick`, a parameter
  // can be chosen and handed back (the Select button of Parameter Properties).
  function sharedParamsDialog(pick) {
    openModal(pick ? "Choose Shared Parameter" : "Shared Parameters", (body, foot, close) => {
      let data = null, grp = null, sel = null;
      const msg = el("div", "modal-msg");
      const pathRow = el("div", "fe-sp-path"), cols = el("div", "fe-sp"), gcol = el("div", "fe-sp-col"), pcol = el("div", "fe-sp-col");
      cols.append(gcol, pcol);
      body.append(pathRow, cols, msg);
      const run = async (op, args) => {
        try { const r = await spCall(op, args); data = r; msg.textContent = ""; return r; } catch (e) { msg.textContent = String(e.message || e); return null; }
      };
      async function load() {
        if (!sharedPath()) { data = null; draw(); return; }
        try { data = await spCall("read", {}); msg.textContent = ""; } catch (e) { data = null; msg.textContent = String(e.message || e); }
        draw();
      }
      function draw() {
        pathRow.replaceChildren();
        const has = !!sharedPath();
        pathRow.append(el("span", "fe-sp-file", has ? sharedPath() : "No shared parameter file chosen."),
          button("Create...", async () => {
            const path = await Backend.pickSaveShared();
            if (!path) return;
            const r = await (async () => { try { return await Backend.call("shared_params", { path, op: "create" }); } catch (e) { msg.textContent = String(e.message || e); return null; } })();
            if (r) { setSharedPath(r.path); data = r; grp = null; sel = null; draw(); }
          }),
          button("Browse...", async () => {
            const path = await Backend.pickOpenShared();
            if (!path) return;
            setSharedPath(path); grp = null; sel = null; await load();
          }));
        gcol.replaceChildren(); pcol.replaceChildren();
        if (!data) { gcol.append(el("div", "famed-hint", "Create a file, or browse to one.")); return; }
        if (grp === null || !data.groups.some((g) => g.id === grp)) grp = data.groups.length ? data.groups[0].id : null;
        gcol.append(el("div", "famed-h", "Groups"));
        data.groups.forEach((g) => { const b = el("button", "fe-lk-item" + (g.id === grp ? " on" : ""), g.name); b.onclick = () => { grp = g.id; sel = null; draw(); }; gcol.append(b); });
        const gb = el("div", "fe-sp-btns");
        gb.append(
          button("New...", () => askText("New Group", "Name", "", [], async (v) => { const r = await run("add_group", { name: v }); if (r) { grp = r.made; draw(); } else draw(); })),
          button("Rename...", () => { const g = data.groups.find((q) => q.id === grp); if (g) askText("Rename Group", "Name", g.name, [], async (v) => { await run("rename_group", { group: grp, name: v }); draw(); }); }),
          button("Delete", async () => { if (grp !== null) { await run("remove_group", { group: grp }); draw(); } }),
        );
        gcol.append(gb);
        pcol.append(el("div", "famed-h", "Parameters"));
        const mine = data.params.filter((q) => q.group === grp);
        if (!mine.length) pcol.append(el("div", "famed-hint", grp === null ? "Add a group first." : "No parameters in this group."));
        mine.forEach((q) => {
          const b = el("button", "fe-lk-item" + (q.guid === sel ? " on" : ""), `${q.name}  (${q.kind ? (SP_TYPES.find((t) => t[0] === q.kind) || [0, q.datatype])[1] : q.datatype})`);
          if (!q.kind) b.title = "Families cannot hold this type yet";
          b.onclick = () => { sel = q.guid; draw(); };
          b.ondblclick = () => { if (pick && q.kind) { close(); pick(q); } };
          pcol.append(b);
        });
        const pb = el("div", "fe-sp-btns");
        pb.append(
          button("New...", () => { if (grp !== null) sharedParamForm(null); else msg.textContent = "Add a group first."; }),
          button("Properties...", () => { const q = data.params.find((x) => x.guid === sel); if (q) sharedParamForm(q); }),
          button("Delete", async () => { if (sel) { await run("remove_param", { guid: sel }); sel = null; draw(); } }),
        );
        pcol.append(pb);
      }
      function sharedParamForm(q) {
        openModal(q ? "Shared Parameter Properties" : "New Shared Parameter", (b2, f2, close2) => {
          const m2 = el("div", "modal-msg");
          const st = { name: q ? q.name : "", kind: q && q.kind ? q.kind : "length", group: q ? q.group : grp, description: q ? q.description : "" };
          b2.append(field("Name", textIn(st.name, (v) => { st.name = v; })));
          const ty = selIn(q ? [[q.kind || "", q.kind ? (SP_TYPES.find((t) => t[0] === q.kind) || [0, q.datatype])[1] : q.datatype]] : SP_TYPES, q ? q.kind || "" : st.kind, (v) => { st.kind = v; });
          ty.disabled = !!q;
          b2.append(field("Type of parameter", ty));
          b2.append(field("Group", selIn(data.groups.map((g) => [String(g.id), g.name]), String(st.group), (v) => { st.group = Number(v); })));
          b2.append(field("Description", textIn(st.description, (v) => { st.description = v; })));
          if (q) b2.append(field("GUID", el("span", "fe-sp-guid", q.guid)));
          b2.append(m2);
          f2.append(button("Cancel", close2), button("OK", async () => {
            const r = q ? await run("edit_param", { guid: q.guid, name: st.name, group: st.group, description: st.description }) : await run("add_param", { group: st.group, name: st.name, kind: st.kind, description: st.description });
            if (!r) { m2.textContent = msg.textContent; return; }
            if (!q) sel = r.made;
            grp = st.group;
            close2(); draw();
          }, true));
        });
      }
      draw(); load();
      if (pick) foot.append(button("Cancel", close), button("OK", () => {
        const q = data && data.params.find((x) => x.guid === sel);
        if (!q) { msg.textContent = "Pick a parameter."; return; }
        if (!q.kind) { msg.textContent = "Families cannot hold that type of parameter yet."; return; }
        close(); pick(q);
      }, true));
      else foot.append(button("Close", close, true));
    }, { wide: true });
  }

  // The Parameter Properties dialog: a new family parameter for a dimension, as a label or a reporting parameter,
  // or a parameter made by Family Types. A shared parameter takes its name, type and GUID from the shared file.
  function paramProps(opts, done) {
    openModal(opts.initial ? "Modify Parameter" : "Parameter Properties", (body, foot, close) => {
      const msg = el("div", "modal-msg");
      const ini = opts.initial;
      const st = ini ? { name: ini.name, kind: ini.param_kind, instance: !!ini.instance, group: ini.group || "Other", reporting: !!ini.reporting, tooltip: ini.tooltip || "", discipline: ini.discipline || "common", guid: ini.shared_guid || null } : { name: opts.name || "", kind: opts.kinds && opts.kinds[0] ? opts.kinds[0][0] : "length", instance: false, group: opts.group || "Dimensions", reporting: false, tooltip: "", discipline: "common", guid: null };
      let shared = !!st.guid;
      const canShare = !opts.canReport && !st.reporting; // a dimension's own parameter cannot be shared (its value is read, not typed)
      function draw() {
        body.replaceChildren();
        const row = (l, n) => { body.append(field(l, n)); return n; };
        const fam = row("Parameter type", selIn([["family", "Family parameter"], ...(canShare ? [["shared", "Shared parameter"]] : [])], shared ? "shared" : "family", (v) => { shared = v === "shared"; if (!shared) st.guid = null; draw(); }));
        if (ini && ini.shared_guid) fam.disabled = true;
        if (shared) {
          const bar = el("span");
          bar.append(el("span", "fe-sp-guid", st.guid ? `GUID ${st.guid}` : "None chosen"));
          if (!(ini && ini.shared_guid)) bar.append(button("Select...", () => sharedParamsDialog((q) => {
            if (S.params.some((x) => x.shared_guid === q.guid && !(ini && x === ini))) { msg.textContent = `'${q.name}' is already in this family.`; return; }
            st.guid = q.guid; st.name = q.name; st.kind = q.kind; st.tooltip = st.tooltip || q.description || "";
            msg.textContent = ""; draw();
          })));
          row("Shared parameter", bar);
        }
        const nm = row("Name", textIn(st.name, (v) => { st.name = v; }));
        row("Discipline", selIn([["common", "Common"]], st.discipline, (v) => { st.discipline = v; }));
        const ty = row("Type of parameter", selIn(opts.kinds || KINDS, st.kind, (v) => { st.kind = v; }));
        if (shared) { nm.disabled = true; ty.disabled = true; }
        if (ini && opts.lockKind) ty.disabled = true;
        row("Group parameter under", textIn(st.group, (v) => { st.group = v; }, { list: ["Dimensions", "Constraints", "Construction", "Materials and Finishes", "Identity Data"] }));
        row("Type or instance", selIn([["false", "Type"], ["true", "Instance"]], String(st.instance), (v) => { st.instance = v === "true"; }));
        if (opts.canReport) row("Reporting parameter", checkIn(false, (v) => { st.reporting = v; }));
        row("Tooltip description", textIn(st.tooltip, (v) => { st.tooltip = v; }));
        if (ini && !shared && canShare && !ini.reporting) body.append(field("Shared file", button("Export...", () => exportParam())));
        body.append(msg);
      }
      // Add this parameter to the shared file (a new GUID) and make it the shared one
      async function exportParam() {
        if (!sharedPath()) { msg.textContent = "Choose or create a shared parameter file first (Manage > Shared Parameters)."; return; }
        let data;
        try { data = await spCall("read", {}); } catch (e) { msg.textContent = String(e.message || e); return; }
        if (!data.groups.length) { msg.textContent = "The shared file has no groups yet. Add one in Manage > Shared Parameters."; return; }
        openModal("Export Parameter", (b2, f2, close2) => {
          let g = data.groups[0].id;
          b2.append(field("Group", selIn(data.groups.map((x) => [String(x.id), x.name]), String(g), (v) => { g = Number(v); })));
          const m2 = el("div", "modal-msg"); b2.append(m2);
          f2.append(button("Cancel", close2), button("OK", async () => {
            try {
              const r = await spCall("add_param", { group: g, name: st.name.trim(), kind: st.kind, description: st.tooltip });
              st.guid = r.made; shared = true; close2(); msg.textContent = `Added '${st.name.trim()}' to the shared file.`; draw();
            } catch (e) { m2.textContent = String(e.message || e); }
          }, true));
        });
      }
      draw();
      const ok = () => {
        const name = st.name.trim();
        if (shared && !st.guid) { msg.textContent = "Choose a shared parameter with Select."; return; }
        if (!name) { msg.textContent = "Give the parameter a name."; return; }
        if ((opts.names || S.params.map((p) => p.name)).some((n) => n.toLowerCase() === name.toLowerCase() && !(ini && n === ini.name))) { msg.textContent = `A parameter called '${name}' already exists.`; return; }
        if (/[\[\]]/.test(name)) { msg.textContent = "A parameter name cannot contain [ or ]."; return; }
        close();
        const dflt = (k) => (k === "yes_no" ? opts.value === true : ["text", "material"].includes(k) ? "" : opts.value || 0);
        if (ini) done({ ...ini, name, param_kind: st.kind, instance: st.instance, group: st.group, discipline: st.discipline, tooltip: st.tooltip, shared_guid: st.guid, default: st.kind === ini.param_kind ? ini.default : dflt(st.kind) });
        else done({ name, param_kind: st.kind, instance: st.instance, group: st.group, formula: null, reporting: st.reporting, shared_guid: st.guid, choices: [], discipline: st.discipline, tooltip: st.tooltip, locked: false, default: dflt(st.kind) });
      };
      foot.append(button("Cancel", close), button("OK", ok, true));
    });
  }

  // Add a parameter and use it on a dimension: as its label, or (reporting) as the parameter that reads it.
  function addParamToDim(d, seg) {
    const refs = dimRefs(d), val = Math.round(Math.abs(pos(refs[seg + 1]) - pos(refs[seg])) * 100) / 100;
    const single = refs.length === 2 || d.eq;
    const total = Math.round(Math.abs(pos(refs[refs.length - 1]) - pos(refs[0])) * 100) / 100;
    paramProps({ kinds: [["length", "Length"]], canReport: single, value: d.eq ? total : val }, async (param) => {
      const patch = param.reporting ? { reports_to: param.name, label: "" } : single ? { label: param.name, reports_to: "" } : { seg_labels: refs.slice(1).map((_, j) => (j === seg ? param.name : (d.seg_labels || [])[j] || "")) };
      await edit("patch_dim", { dim: d.id, patch }, false, () => { S.params.push(param); });
    });
  }

  function dimProps(r, d) {
    if (!d) return;
    const refs = dimRefs(d), n = refs.length - 1;
    r.append(el("div", "famed-hint", `Dimension${d.view ? ` (${d.view} view)` : ""}, ${n} segment${n > 1 ? "s" : ""}`));
    const lens = S.params.filter((p) => ["length", "number"].includes(p.param_kind) && !p.reporting && !p.formula).map((p) => [p.name, p.name]);
    const single = n === 1 || d.eq;
    const labelSel = (cur, apply) => {
      const s = selIn([["", "<none>"], ...lens, ["__new", "<Add parameter...>"]], cur || "", (v) => { if (v === "__new") { s.value = cur || ""; addParamToDim(d, 0); } else apply(v); });
      return s;
    };
    for (let j = 0; j < n; j++) {
      const val = pos(refs[j]) !== undefined ? Math.abs(pos(refs[j + 1]) - pos(refs[j])) : 0;
      if (single && j > 0) break;
      const shown = d.eq ? Math.abs(pos(refs[n]) - pos(refs[0])) : val;
      r.append(field(n > 1 && !d.eq ? `Length ${j + 1}` : d.eq ? "Total length" : "Length", textIn(String(Math.round(shown * 100) / 100), () => editDimValue(d, j))));
    }
    if (single) {
      r.append(field("Label", labelSel(d.label, (v) => dimPatch(d, { label: v, reports_to: "" }))));
    } else {
      for (let j = 0; j < n; j++) {
        const cur = (d.seg_labels || [])[j] || "";
        const s = selIn([["", "<none>"], ...lens, ["__new", "<Add parameter...>"]], cur, (v) => {
          if (v === "__new") { s.value = cur; addParamToDim(d, j); return; }
          const labels = refs.slice(1).map((_, k) => (k === j ? v : (d.seg_labels || [])[k] || ""));
          dimPatch(d, { seg_labels: labels });
        });
        r.append(field(`Label ${j + 1}`, s));
      }
    }
    r.append(button("Add Parameter...", () => addParamToDim(d, 0)));
    if (single) {
      const reps = S.params.filter((p) => p.reporting && !p.formula).map((p) => [p.name, p.name]);
      r.append(field("Reports to", selIn([["", "<none>"], ...reps], d.reports_to || "", (v) => dimPatch(d, { reports_to: v, label: "" }))));
    }
    r.append(field("Locked", checkIn(d.locked, (v) => dimPatch(d, { locked: v }))));
    if (refs.length >= 3) r.append(field("Equal segments (EQ)", checkIn(d.eq, (v) => dimPatch(d, { eq: v }))));
    const names = refs.map((id) => (planeById(id) || {}).name).filter(Boolean).join(", ");
    r.append(el("div", "famed-hint", `References: ${names}.`));
    if ((d.drives || []).length) r.append(el("div", "famed-hint", `Holds: ${d.drives.map((id) => (planeById(id) || {}).name).filter(Boolean).join(", ")}.`));
    r.append(el("div", "famed-hint", "Click the padlock on the dimension to lock it. Click its value to type a new length. Drag the dimension line to move it."));
  }

  function loopFromDraft(sp) {
    const d = S.draft;
    const a = d.an, b = sp;
    const nd = (u, v) => ({ u: coordOf(u), v: coordOf(v) });
    if (d.kind === "rect") return { nodes: [nd(a.u, a.v), nd(b.u, a.v), nd(b.u, b.v), nd(a.u, b.v)], bulges: [], closed: true };
    // circle: the centre and a point on it; two half arcs through the points either side of the centre
    const r = Math.round(Math.hypot(b.pt.u - a.pt.u, b.pt.v - a.pt.v));
    if (r < 1) return null;
    const left = { plane: a.u.plane, off: a.u.plane ? -r : a.pt.u - r }, right = { plane: a.u.plane, off: a.u.plane ? r : a.pt.u + r };
    const cv = coordOf(a.v);
    return { nodes: [{ u: right, v: cv }, { u: left, v: cv }], bulges: [1, 1], closed: true };
  }

  async function finishSketch(sp) {
    const loop = loopFromDraft(sp);
    S.draft = null;
    if (!loop) return;
    if (loop.nodes.length === 4 && (sp.pt.u === S.draftA?.u)) { /* degenerate checked server side */ }
    await addForm([loop]);
  }
  async function finishPoly() {
    const d = S.draft; S.draft = null;
    if (!d || d.kind !== "poly") return;
    const q = d.pts, k = q.length;
    if (k > 1 && q[k - 1].pt.u === q[k - 2].pt.u && q[k - 1].pt.v === q[k - 2].pt.v) q.pop(); // a double click adds its point twice
    if (S.tool === "sym" || S.tool === "mline") {
      if (d.pts.length < 2) return;
      await addSym({ nodes: d.pts.map((p) => ({ u: coordOf(p.u), v: coordOf(p.v) })), bulges: [], closed: false }, false);
      return;
    }
    if (d.pts.length < 3) { S.msg = "A polygon needs three points."; status(); return; }
    await addForm([{ nodes: d.pts.map((p) => ({ u: coordOf(p.u), v: coordOf(p.v) })), bulges: [], closed: true }]);
  }
  // The arc through three points (u, v): its centre, radius, start angle, signed sweep and bulge
  function arcFrom3(A, B, M) {
    const d = 2 * (A.u * (B.v - M.v) + B.u * (M.v - A.v) + M.u * (A.v - B.v));
    if (Math.abs(d) < 1e-9) return null;
    const sq = (p) => p.u * p.u + p.v * p.v;
    const cx = (sq(A) * (B.v - M.v) + sq(B) * (M.v - A.v) + sq(M) * (A.v - B.v)) / d;
    const cy = (sq(A) * (M.u - B.u) + sq(B) * (A.u - M.u) + sq(M) * (B.u - A.u)) / d;
    const ang = (p) => Math.atan2(p.v - cy, p.u - cx);
    const norm = (x) => ((x % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
    const a0 = ang(A), toB = norm(ang(B) - a0), toM = norm(ang(M) - a0);
    const sweep = toM < toB ? toB : -norm(a0 - ang(B));
    let bulge = Math.tan(sweep / 4);
    for (const nice of [Math.tan(Math.PI / 8), 1, Math.tan(Math.PI / 12), Math.tan(Math.PI / 6)]) { if (Math.abs(Math.abs(bulge) - nice) < 0.004) bulge = Math.sign(bulge) * nice; }
    return { c: { u: cx, v: cy }, r: Math.hypot(A.u - cx, A.v - cy), a0, sweep, bulge };
  }
  // points along the segment a to b with this bulge (tan of a quarter of the sweep, positive counter-clockwise), ending at b
  function bulgePoints(a, b, bulge) {
    if (!bulge) return [b];
    const th = 4 * Math.atan(bulge), c = Math.hypot(b.u - a.u, b.v - a.v);
    if (c < 1e-9) return [b];
    const h = c / 2 / Math.tan(th / 2), dx = (b.u - a.u) / c, dy = (b.v - a.v) / c;
    const cu = (a.u + b.u) / 2 - dy * h, cv = (a.v + b.v) / 2 + dx * h, r = Math.hypot(a.u - cu, a.v - cv), a0 = Math.atan2(a.v - cv, a.u - cu);
    const out = [];
    for (let i = 1; i < 32; i++) out.push({ u: cu + r * Math.cos(a0 + (th * i) / 32), v: cv + r * Math.sin(a0 + (th * i) / 32) });
    out.push(b);
    return out;
  }
  async function finishArc() {
    const d = S.draft; S.draft = null;
    const [A, B, M] = d.pts;
    const a = arcFrom3(A.pt, B.pt, M.pt);
    if (!a || Math.hypot(A.pt.u - B.pt.u, A.pt.v - B.pt.v) < 1) { S.msg = "Those three points do not make an arc."; status(); return; }
    await addSym({ nodes: [A, B].map((p) => ({ u: coordOf(p.u), v: coordOf(p.v) })), bulges: [a.bulge], closed: false }, false);
  }
  async function finishRegion(sp) {
    const d = S.draft; S.draft = null;
    const nd = (u, v) => ({ u: coordOf(u), v: coordOf(v) });
    const a = d.an, b = sp;
    await addSym({ nodes: [nd(a.u, a.v), nd(b.u, a.v), nd(b.u, b.v), nd(a.u, b.v)], bulges: [], closed: true }, true);
  }

  // After a sketch: say how its edges that sit on reference planes were left (the padlocks show on the canvas)
  function lockNote() {
    const e = edgeLocks();
    if (!e.length) return;
    const n = e.filter((q) => q.locked).length;
    S.msg = n ? `${n} edge${n > 1 ? "s are" : " is"} locked to a reference plane. Click a padlock to unlock it.` : `${e.length} edge${e.length > 1 ? "s sit" : " sits"} on a reference plane without a lock. Click a padlock to lock it.`;
    status();
  }
  async function addForm(profile) {
    if (S.editing) {
      const id = S.editing.id;
      const ok = await change((b) => { formById(id).shape.profile.push(...profile); S.sel = { kind: "form", id }; });
      if (ok) lockNote();
      return;
    }
    if (S.tool === "blend" || S.tool === "revolve") { await commitSpecial(profile); return; }
    const V = VIEWS[S.view];
    const wp = workPlane();
    const ok = await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => !!f.void === (S.tool === "void")).length + 1;
      const no = b.forms.filter((f) => f.opening).length + 1;
      b.forms.push({
        id, name: S.openingNext ? `Opening ${no}` : S.tool === "void" ? `Void ${n}` : `Extrusion ${n}`, void: S.tool === "void", cuts_host: true, opening: !!S.openingNext, cut_by: [],
        shape: { type: "extrusion", plane: wp, start: S.start, end: S.end, profile },
        material: "", material_param: null, subcategory: S.tool === "void" ? "" : "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
    if (ok) lockNote();
  }
  // Blend: the base first, then the top. Revolve: the profile, then the two axis points.
  async function commitSpecial(profile) {
    if (S.tool === "revolve") {
      S.rev = { profile, pts: [] };
      S.msg = "Click two points for the axis."; status();
      return;
    }
    if (!S.blend) {
      S.blend = { base: profile };
      S.msg = "Now sketch the top."; status();
      return;
    }
    const base = S.blend.base;
    if (profile.length !== 1 || base.length !== 1 || profile[0].nodes.length !== base[0].nodes.length) {
      S.msg = `The top needs ${base[0].nodes.length} points like the base. Sketch it again, or press Esc.`; status();
      return;
    }
    S.blend = null; S.msg = "";
    const V = VIEWS[S.view];
    const wp = workPlane();
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => f.shape.type === "blend").length + 1;
      b.forms.push({
        id, name: `${S.voidNext ? "Void " : ""}Blend ${n}`, void: !!S.voidNext, cuts_host: true, cut_by: [],
        shape: { type: "blend", plane: wp, start: S.start, end: S.end, base, top: profile },
        material: "", material_param: null, subcategory: S.voidNext ? "" : "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
  }
  async function addRevolve() {
    const { profile, pts } = S.rev;
    S.rev = null; S.msg = "";
    if (pts[0].pt.u === pts[1].pt.u && pts[0].pt.v === pts[1].pt.v) { S.msg = "The axis needs two different points."; status(); return; }
    const V = VIEWS[S.view];
    const wp = workPlane();
    const node = (p) => ({ u: coordOf(p.u), v: coordOf(p.v) });
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => f.shape.type === "revolve").length + 1;
      b.forms.push({
        id, name: `${S.voidNext ? "Void " : ""}Revolve ${n}`, void: !!S.voidNext, cuts_host: true, cut_by: [],
        shape: { type: "revolve", plane: wp, axis_a: node(pts[0]), axis_b: node(pts[1]), start_deg: Number(S.a0) || 0, end_deg: S.a1 === "" || isNaN(Number(S.a1)) ? 360 : Number(S.a1), profile },
        material: "", material_param: null, subcategory: S.voidNext ? "" : "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
  }
  async function finishPath() {
    const d = S.draft; S.draft = null;
    if (!d || d.kind !== "path") return;
    const q = d.pts;
    // a double click adds its last point twice
    if (q.length > 1 && q[q.length - 1].pt.u === q[q.length - 2].pt.u && q[q.length - 1].pt.v === q[q.length - 2].pt.v) { q.pop(); d.bulges.pop(); }
    if (q.length < 2) { S.msg = "A path needs at least two points."; status(); return; }
    const V = VIEWS[S.view];
    const wp = workPlane();
    const bulges = d.bulges.slice();
    if (S.pathClosed) bulges.push(0);
    const path = { nodes: q.map((p) => ({ u: coordOf(p.u), v: coordOf(p.v) })), bulges, closed: S.pathClosed && q.length > 2 };
    if (!path.closed) path.bulges = d.bulges.slice();
    const ok = await change((b) => {
      const id = newId();
      const blend = S.tool === "swblend";
      const n = b.forms.filter((f) => f.shape.type === (blend ? "swept_blend" : "sweep")).length + 1;
      const sq = (h) => [{ type: "poly", points: [[-h, -h], [h, -h], [h, h], [-h, h]].map(([u, v]) => ({ u: String(u), v: String(v) })), bulges: [] }];
      const shape = blend ? { type: "swept_blend", plane: wp, path, profile: sq(25), profile_end: sq(10), profile_angle: 0, flip: false } : { type: "sweep", plane: wp, path, profile: sq(25), profile_angle: 0, flip: false };
      b.forms.push({
        id, name: `${S.voidNext ? "Void " : ""}${blend ? "Swept Blend" : "Sweep"} ${n}`, void: !!S.voidNext, cuts_host: true, cut_by: [],
        shape,
        material: "", material_param: null, subcategory: S.voidNext ? "" : "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
    if (!ok) return;
    S.msg = S.tool === "swblend" ? "Blended a 50 x 50 profile into a 20 x 20 one. Set both in Properties." : "Swept a 50 x 50 profile. Set its size and position in Properties.";
    status();
  }
  async function addSym(path, filled) {
    const view = S.view;
    if (S.tool === "mline") {
      const plane = workPlane();
      const ok = await change((b) => {
        const id = newId();
        const n = (b.model_lines || []).length + 1;
        (b.model_lines = b.model_lines || []).push({ id, name: `Model Line ${n}`, plane, path, subcategory: "Model Lines", vis: {} });
        S.sel = { kind: "mline", id };
      });
      if (ok) lockNote();
      return;
    }
    const ok = await change((b) => {
      const id = newId();
      b.syms.push({ id, view, path, filled, masking: false, subcategory: filled ? "Panel" : "Plan Swing", vis: {} });
      S.sel = { kind: "sym", id };
    });
    if (ok) lockNote();
  }

  async function deleteSel() {
    if (S.editing) { await deleteLoop(); return; }
    const s = S.sel;
    if (!s) return;
    if (s.kind === "multi") {
      // geometry first, in one step, then dimensions, then the planes nothing holds any more
      const items = s.items, of = (k) => new Set(items.filter((x) => x.kind === k).map((x) => x.id));
      const forms = of("form"), syms = of("sym"), mlines = of("mline"), ctrls = of("ctrl"), comps = of("comp"), mtexts = of("mtext"), conns = of("conn");
      S.sel = null;
      if (forms.size || syms.size || mlines.size || ctrls.size || comps.size || mtexts.size || conns.size) {
        await change((b) => {
          b.forms = b.forms.filter((f) => !forms.has(f.id));
          b.forms.forEach((f) => { f.cut_by = (f.cut_by || []).filter((x) => !forms.has(x)); });
          b.syms = b.syms.filter((y) => !syms.has(y.id));
          b.model_lines = (b.model_lines || []).filter((y) => !mlines.has(y.id));
          b.controls = (b.controls || []).filter((y) => !ctrls.has(y.id));
          b.nested = (b.nested || []).filter((y) => !comps.has(y.id));
          b.model_texts = (b.model_texts || []).filter((y) => !mtexts.has(y.id));
          b.connectors = (b.connectors || []).filter((y) => !conns.has(y.id));
        });
      }
      for (const id of of("dim")) if (dimById(id)) await edit("remove_dim", { dim: id }, true);
      const kept = [];
      for (const id of of("plane")) {
        const p = planeById(id);
        if (!p || p.anchor.type === "origin") { if (p) kept.push(p.name); continue; }
        if (!(await edit("remove_plane", { plane: id }, true))) kept.push(p.name);
      }
      S.msg = kept.length ? `Kept ${kept.join(", ")}: something still depends on ${kept.length > 1 ? "them" : "it"}.` : "";
      props(); status(); render();
      return;
    }
    if (s.kind === "dim") { S.sel = null; await edit("remove_dim", { dim: s.id }); return; }
    if (s.kind === "plane") { S.sel = null; if (!(await edit("remove_plane", { plane: s.id }))) { S.sel = s; props(); } return; }
    await change((b) => {
      if (s.kind === "plane") {
        const used = b.planes.some((p) => (p.anchor.type === "offset" && p.anchor.from === s.id) || (p.anchor.type === "mid" && (p.anchor.a === s.id || p.anchor.b === s.id)));
        const inLoop = (l) => l.nodes.some((n) => n.u.plane === s.id || n.v.plane === s.id);
        const inForm = b.forms.some((f) => f.shape.plane === s.id || [...(f.shape.profile || []), ...(f.shape.base || []), ...(f.shape.top || []), ...(f.shape.path ? [f.shape.path] : [])].some(inLoop) || [f.shape.axis_a, f.shape.axis_b].some((n) => n && (n.u.plane === s.id || n.v.plane === s.id))) || b.syms.some((y) => inLoop(y.path));
        const p = b.planes.find((q) => q.id === s.id);
        if (used || inForm || (p && p.anchor.type === "origin")) throw new Error("Something is still locked to this plane.");
        b.planes = b.planes.filter((q) => q.id !== s.id);
      } else if (s.kind === "form") {
        b.forms = b.forms.filter((f) => f.id !== s.id);
        b.forms.forEach((f) => { f.cut_by = (f.cut_by || []).filter((x) => x !== s.id); });
      } else if (s.kind === "sym") b.syms = b.syms.filter((y) => y.id !== s.id);
      else if (s.kind === "mline") b.model_lines = (b.model_lines || []).filter((y) => y.id !== s.id);
      else if (s.kind === "ctrl") b.controls = (b.controls || []).filter((y) => y.id !== s.id);
      else if (s.kind === "comp") b.nested = (b.nested || []).filter((y) => y.id !== s.id);
      else if (s.kind === "mtext") b.model_texts = (b.model_texts || []).filter((y) => y.id !== s.id);
      else if (s.kind === "conn") b.connectors = (b.connectors || []).filter((y) => y.id !== s.id);
      S.sel = null;
    });
  }

  return { list, open, newFamily, loadFile, exportFamily, activate, close: closeEditor, commands, syncRibbon, context, docInfo, undoState: () => (S ? (skActive() ? { undo: FSk.canUndo ? 1 : 0, redo: FSk.canRedo ? 1 : 0 } : { undo: S.undo.length, redo: S.redo.length }) : null),
    undo: () => (skActive() ? FSk.undo() : undo()), redo: () => (skActive() ? FSk.redo() : redo()), save: saveFile, closeForExit, sketchModify: SK_MODIFY, _state: () => S,
    // test hooks: where a point of the open view is on the canvas, the sketch session, the 3D view
    _screenOf: (u, v) => w2s(u, v), _sketch: FSk, _vp3: () => vp3, _refresh: async () => { await preview(); props(); render(); } };
})();
