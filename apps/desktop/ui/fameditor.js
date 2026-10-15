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
            row.append(n);
            if (f.has_body) row.append(button("Edit...", () => { close(); open(f.id); }));
            if (f.has_body) row.append(button("Duplicate...", () => duplicateTypePrompt(f.id, async () => { await load(); }, "duplicate_family")));
            row.append(button("Export...", () => exportFamily(f.id, f.name)));
            box.append(row);
          }
        }
      };
      foot.append(button("New Family...", () => newFamily(close)), button("Load Family...", () => loadFile(load)), button("Close", close, true));
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

  async function exportFamily(id, name) {
    try {
      const file = await Backend.call("export_family", { id });
      const blob = new Blob([JSON.stringify(file, null, 1)], { type: "application/json" });
      const a = el("a"); a.href = URL.createObjectURL(blob); a.download = `${name}.oaf`; document.body.append(a); a.click(); a.remove();
      setPrompt(`Exported ${name}.oaf`);
    } catch (e) { setPrompt(String(e.message || e), true); }
  }

  function loadFile(after) {
    const inp = el("input"); inp.type = "file"; inp.accept = ".oaf,.ibfam,.json,application/json"; // .ibfam: the old extension
    inp.onchange = async () => {
      const f = inp.files[0];
      if (!f) return;
      try {
        const file = JSON.parse(await f.text());
        const r = await Backend.call("import_family", { file });
        await refresh();
        setPrompt(`Loaded ${file.family.name}.`);
        if (after) after();
        return r;
      } catch (e) { setPrompt(String(e.message || e), true); }
    };
    inp.click();
  }

  // ---------------------------------------------------------------- the editor
  async function open(familyId) {
    if (S && S.id === familyId) { activate(); return; }
    if (S) { activate(); setPrompt(`Close "${S.def.name}" before opening another family.`, true); return; }
    let def;
    try { def = await Backend.call("family_def", { id: familyId }); } catch (e) { setPrompt(String(e.message || e), true); return; }
    const first = def.types[0];
    S = {
      id: familyId, def, body: clone(def.body), params: clone(def.params), types: clone(def.types),
      typeId: first ? first.id : null, values: {}, view: "plan", tool: "select", sketch: "rect", sel: null,
      draft: null, pv: null, cams: {}, dirty: false, undo: [], redo: [], mouse: null, tick: 0, rot: { a: -0.6, b: 0.5 }, drag: null,
      start: "0", end: "100", a0: "0", a1: "360", pathMode: "line", pathClosed: false, dimMode: "drive", blend: null, rev: null, wp: null, msg: "",
      show: { names: true, dims: def.body.planes.length <= 14 }, symMode: "line", open: ["plan", "front"],
    };
    mount();
    await preview();
    activate();
    fitAll();
    render();
  }

  function snapshot() { return JSON.stringify({ body: S.body, params: S.params }); }
  function pushUndo() { S.undo.push(snapshot()); if (S.undo.length > 100) S.undo.shift(); S.redo = []; S.dirty = true; }
  function restore(js) { const o = JSON.parse(js); S.body = o.body; S.params = o.params; S.sel = null; }
  async function undo() { if (!S.undo.length) return; S.redo.push(snapshot()); restore(S.undo.pop()); await preview(); props(); render(); }
  async function redo() { if (!S.redo.length) return; S.undo.push(snapshot()); restore(S.redo.pop()); await preview(); props(); render(); }

  // A change to the body: kept if it still solves, otherwise put back with the reason.
  async function change(fn, keep) {
    const before = snapshot();
    const sel = S.sel;
    try { fn(S.body); } catch (e) { restore(before); S.sel = sel; S.msg = String(e.message || e); status(); render(); return false; }
    const ok = await preview();
    if (!ok) { restore(before); S.sel = sel; S.msg = S.pvError || "That change does not solve."; await preview(); }
    else { S.undo.push(before); S.redo = []; S.dirty = true; S.msg = ""; }
    if (!keep) props();
    status();
    render();
    return ok;
  }

  const newId = () => {
    const b = S.body;
    return 1 + Math.max(0, ...b.planes.map((x) => x.id), ...b.forms.map((x) => x.id), ...b.syms.map((x) => x.id), ...(b.nested || []).map((x) => x.id));
  };
  const planeById = (id) => S.body.planes.find((p) => p.id === id);
  const formById = (id) => S.body.forms.find((f) => f.id === id);
  const symById = (id) => S.body.syms.find((f) => f.id === id);
  const pos = (id) => (S.pv && S.pv.planes ? S.pv.planes[id] : undefined);

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
    if (!force && S.dirty) { askSave(); return; }
    S = null;
    setDoc("project");
    refresh(true);
  }

  function askSave() {
    activate();
    openModal(`Close ${S.def.name}`, (body, foot, close) => {
      body.append(el("div", "modal-msg", "Save the changes to this family before closing it?"));
      foot.append(button("Cancel", close), button("Don't Save", () => { close(); closeEditor(true); }), button("Save", async () => { close(); await save(true); }, true));
    });
  }

  async function save(close) {
    try {
      await Backend.call("set_family_def", { id: S.id, params: S.params, body: S.body });
      for (const t of S.types) {
        const changed = {};
        for (const [k, v] of Object.entries(S.saveVals || {})) if (t.id === S.typeId) changed[k] = v;
        if (Object.keys(changed).length) await Backend.call("patch_family_type", { id: t.id, patch: { values: changed } });
      }
      S.saveVals = {}; S.dirty = false; S.msg = "Saved.";
      await refresh(true);
      if (close) { closeEditor(true); return true; }
      status();
      return true;
    } catch (e) { S.msg = String(e.message || e); status(); return false; }
  }

  // Load into Project: the family goes into the project and the project shows; the family stays open.
  // (The family is stored inside the project file for now; a file of its own comes later.)
  async function loadIntoProject(close) {
    if (!(await save(false))) return;
    const name = S.def.name;
    if (close) closeEditor(true); else setDoc("project");
    await refresh(true);
    setPrompt(`Loaded ${name} into the project.`);
  }

  // ---- ribbon commands ----
  const TOOL_OF = {
    "f-select": "select", "f-plane": "plane", "f-dim": "dim", "f-eq": "eq", "f-align": "align", "f-ext": "ext", "f-void": "void",
    "f-blend": "blend", "f-revolve": "revolve", "f-sweep": "sweep", "f-sym": "sym", "f-region": "region",
  };
  function setTool(t) {
    S.tool = t; S.draft = null; S.alignPlane = null; S.blend = null; S.rev = null; S.picks = [];
    toolbar(); status(); render();
  }
  const commands = {
    "f-types": () => typesDialog(),
    "f-category": () => paramsDialog(),
    "f-del": () => deleteSel(),
    "f-load": () => loadIntoProject(false),
    "f-loadclose": () => loadIntoProject(true),
    "f-3d": () => openFamView("3d"),
    "f-names": () => { S.show.names = !S.show.names; tabsUI(); render(); },
    "f-dims": () => { S.show.dims = !S.show.dims; tabsUI(); render(); },
  };
  for (const [id, t] of Object.entries(TOOL_OF)) commands[id] = () => setTool(t);
  for (const k of Object.keys(commands)) { const f = commands[k]; commands[k] = (e) => (S ? f(e) : undefined); }

  function syncRibbon() {
    document.querySelectorAll("#ribbon button.cmd[data-id^='f-']").forEach((b) => {
      const id = b.dataset.id;
      b.classList.toggle("on", S ? (TOOL_OF[id] ? TOOL_OF[id] === S.tool : (id === "f-names" ? S.show.names : id === "f-dims" ? S.show.dims : false)) : false);
    });
  }

  // ---- options bar: what the active tool needs ----
  function toolbar() {
    syncRibbon();
    const bar = S.opts;
    bar.replaceChildren();
    if (S.tool === "sym") {
      const mode = el("select");
      [["line", "Line"], ["arc", "3-Point Arc"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; mode.append(o); });
      mode.value = S.symMode; mode.onchange = () => { S.symMode = mode.value; S.draft = null; status(); render(); };
      bar.append(el("span", "fe-opt", "Draw"), mode);
    }
    if (S.tool === "dim") {
      const m = el("select");
      [["drive", "Label a dimension"], ["report", "Reporting parameter"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; m.append(o); });
      m.value = S.dimMode; m.onchange = () => { S.dimMode = m.value; S.picks = []; status(); };
      bar.append(el("span", "fe-opt", "Dimension"), m);
    }
    if (S.tool === "sweep") sweepBar(bar);
    if (S.tool === "ext" || S.tool === "void" || S.tool === "blend" || S.tool === "revolve") {
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
      wp.value = S.wp && planeById(S.wp) && planeById(S.wp).axis === axis ? S.wp : (VIEWS[S.view] || VIEWS.plan).origin;
      S.wp = Number(wp.value); wp.onchange = () => { S.wp = Number(wp.value); };
      if (S.tool === "revolve") {
        const a0 = el("input"), a1 = el("input");
        a0.type = a1.type = "text"; a0.style.width = a1.style.width = "50px";
        a0.value = S.a0; a1.value = S.a1;
        a0.onchange = () => { S.a0 = a0.value; }; a1.onchange = () => { S.a1 = a1.value; };
        bar.append(el("span", "fe-opt", "Draw"), sk, el("span", "fe-opt", "Work plane"), wp, el("span", "fe-opt", "Start angle"), a0, el("span", "fe-opt", "End angle"), a1);
      } else bar.append(el("span", "fe-opt", "Draw"), sk, el("span", "fe-opt", "Work plane"), wp, el("span", "fe-opt", S.tool === "blend" ? "Base at" : "Start"), st, el("span", "fe-opt", S.tool === "blend" ? "Top at" : "End"), en);
    }
    bar.hidden = !bar.children.length;
  }

  function sweepBar(bar) {
    const mode = el("select");
    [["line", "Line"], ["arc", "3-Point Arc"]].forEach(([v, t]) => { const o = el("option", "", t); o.value = v; mode.append(o); });
    mode.value = S.pathMode; mode.onchange = () => { S.pathMode = mode.value; if (S.draft) S.draft.arcEnd = null; status(); render(); };
    const cl = el("label", "fe-opt", "Closed path"); cl.prepend(checkIn(S.pathClosed, (v) => { S.pathClosed = v; }));
    const axis = VIEWS[S.view] ? VIEWS[S.view].n : "z";
    const wp = el("select");
    S.body.planes.filter((p) => p.axis === axis).forEach((p) => { const o = el("option", "", p.name); o.value = p.id; wp.append(o); });
    wp.value = S.wp && planeById(S.wp) && planeById(S.wp).axis === axis ? S.wp : (VIEWS[S.view] || VIEWS.plan).origin;
    S.wp = Number(wp.value); wp.onchange = () => { S.wp = Number(wp.value); };
    bar.append(el("span", "fe-opt", "Path"), mode, cl, el("span", "fe-opt", "Work plane"), wp);
  }

  // ---- views: tabs, the Project Browser and the view bar ----
  const viewLabel = (id) => (id === "3d" ? "View 1" : VIEWS[id].label);
  function openFamView(id) {
    if (!S.open.includes(id)) S.open.push(id);
    S.view = id; S.draft = null; S.wp = null; S.picks = [];
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
      select: "Click to select. Drag a reference plane or a sketch point of a selected form to move it. Delete removes the selection.",
      plane: "Click where the plane goes, then drag along it or click a second point to set the direction.",
      dim: "Click two parallel reference planes, then give the label (a parameter) or a value. In Reporting parameter mode the dimension reads its length into a new read-only parameter instead.",
      eq: "Click two outer planes, then the plane that is kept midway between them.",
      ext: "Sketch the profile on the work plane. Points near a reference plane lock to it.",
      void: "Sketch the void. A void that cuts the host makes the opening in the wall.",
      blend: "Sketch the base, then the top. Both need the same number of points. The solid blends from the base height to the top height.",
      revolve: "Sketch the profile, then click two points for the axis it turns about. Set the start and end angle above.",
      sweep: "Draw the path: click points, double-click or press Enter to finish. In 3-Point Arc mode click the end, then a point on the arc. Then set the profile in Properties.",
      align: "Click a reference plane, then an edge of a form or symbolic line that runs along it. The edge moves to the plane and stays locked to it.",
      sym: "Line: click points, double-click to finish. 3-Point Arc: click the start, the end, then a point on the arc. Shows only in this kind of view.",
      region: "Click two corners for a filled region that shows only in this kind of view.",
    };
    const err = !!S.msg && S.msg !== "Saved.";
    if (app.doc === "family") setPrompt((S.msg ? S.msg + "   " : "") + (S.view === "3d" ? "Drag to turn the model." : tips[S.tool] || ""), err);
    syncTitle();
  }

  function syncTitle() {
    if (!S || app.doc !== "family") return;
    $q("#doc-title").textContent = `Family: ${S.def.name}${S.dirty ? " *" : ""} - ${VIEW_TITLE[S.view]}`;
    renderDocTabs();
    updateUndoButtons();
  }
  const docInfo = () => (S ? { name: S.def.name, dirty: S.dirty } : null);

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
    if (!s) {
      r.append(el("div", "famed-hint", "Nothing selected."));
      const b = S.body;
      r.append(field("Family category", selIn((S.def.categories || []).map((c) => [c.id, c.label]), S.def.category, (v) => setMeta({ category: v }))));
      r.append(field("Hosted by", selIn([["none", "Nothing (free standing)"], ["wall", "Wall"], ["floor", "Floor"], ["ceiling", "Ceiling"], ["roof", "Roof"], ["face", "Face"]], S.def.host, (v) => setMeta({ host: v }))));
      r.append(field("Cut with voids", checkIn(b.cut_with_voids, (v) => change((x) => { x.cut_with_voids = v; }, true))));
      r.append(field("Always vertical", checkIn(b.always_vertical, (v) => change((x) => { x.always_vertical = v; }, true))));
    } else if (s.kind === "plane") planeProps(r, planeById(s.id));
    else if (s.kind === "form") formProps(r, formById(s.id));
    else if (s.kind === "sym") symProps(r, symById(s.id));
    flex(r);
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

  function visBlock(r, o, key) {
    const v = o.vis || (o.vis = {});
    const get = (k) => v[k] !== false;
    r.append(el("div", "famed-h", "Visibility"));
    const row = el("div", "fe-vis");
    for (const [k, t] of [["coarse", "Coarse"], ["medium", "Medium"], ["fine", "Fine"], ["plan", "Plan"], ["front_back", "Front/Back"], ["left_right", "Left/Right"]]) {
      const l = el("label", "", t);
      l.prepend(checkIn(get(k), (c) => change((b) => { const t2 = key(b); t2.vis = t2.vis || {}; t2.vis[k] = c; }, true)));
      row.append(l);
    }
    r.append(row);
    r.append(field("Visible when", textIn(v.when || "", (x) => change((b) => { const t2 = key(b); t2.vis = t2.vis || {}; t2.vis.when = x.trim() ? x : null; }, true), { list: S.params.filter((p) => p.param_kind === "yes_no").map((p) => `[${p.name}]`) })));
  }

  function formProps(r, f) {
    if (!f) return;
    r.append(el("div", "famed-hint", { blend: "Blend", revolve: "Revolve", sweep: "Sweep" }[f.shape.type] || (f.void ? "Void extrusion" : "Extrusion")));
    r.append(field("Name", textIn(f.name, (v) => change((b) => { formById(f.id).name = v; }, true))));
    r.append(field("Void", checkIn(f.void, (v) => change((b) => { formById(f.id).void = v; }))));
    const sh = f.shape;
    if (sh.type === "blend") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      r.append(field("Base at", textIn(sh.start, (v) => change((b) => { formById(f.id).shape.start = v; }, true), { list: lengthParams() })));
      r.append(field("Top at", textIn(sh.end, (v) => change((b) => { formById(f.id).shape.end = v; }, true), { list: lengthParams() })));
    }
    if (sh.type === "revolve") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      r.append(field("Start angle", textIn(String(sh.start_deg), (v) => change((b) => { formById(f.id).shape.start_deg = Number(v) || 0; }, true))));
      r.append(field("End angle", textIn(String(sh.end_deg), (v) => change((b) => { formById(f.id).shape.end_deg = v === "" || isNaN(Number(v)) ? 360 : Number(v); }, true))));
    }
    if (sh.type === "sweep") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      r.append(el("div", "famed-h", "Profile"));
      r.append(el("div", "famed-hint", "Measured from the first point of the path. Across is to the left of the path, Up follows the work plane normal."));
      sweepProfile(r, f);
      r.append(field("Turn profile (deg)", textIn(String(sh.profile_angle || 0), (v) => change((b) => { formById(f.id).shape.profile_angle = Number(v) || 0; }, true))));
      r.append(field("Flip profile", checkIn(sh.flip, (v) => change((b) => { formById(f.id).shape.flip = v; }, true))));
    }
    if (sh.type === "extrusion") {
      r.append(field("Work plane", selIn(S.body.planes.filter((p) => p.axis === planeById(sh.plane).axis).map((p) => [p.id, p.name]), sh.plane, (v) => change((b) => { formById(f.id).shape.plane = Number(v); }))));
      for (const which of ["start", "end"]) {
        const lock = sh[which + "_lock"];
        const label = which === "start" ? "Extrusion start" : "Extrusion end";
        if (lock) {
          const pl = planeById(lock);
          const row = el("div", "famed-row"); row.append(el("span", "", label));
          const un = button("Unlock", () => change((b) => {
            const t = formById(f.id).shape, wp = planeById(t.plane);
            const rel = pos(lock) !== undefined && pos(wp.id) !== undefined ? Math.round(pos(lock) - pos(wp.id)) : 0;
            t[which] = String(rel); t[which + "_lock"] = null;
          }));
          row.append(el("span", "", `locked to ${pl ? pl.name : "a deleted plane"}`), un);
          r.append(row);
        } else r.append(field(label, textIn(sh[which], (v) => change((b) => { formById(f.id).shape[which] = v; }, true), { list: lengthParams() })));
      }
    }
    if (f.void) {
      r.append(field("Cuts the host", checkIn(f.cuts_host !== false, (v) => change((b) => { formById(f.id).cuts_host = v; }, true))));
    } else {
      r.append(field("Material", textIn(f.material || "", (v) => change((b) => { formById(f.id).material = v; }, true), { list: (app.info.materials || []).map((m) => m.name || m) })));
      r.append(field("Material parameter", selIn([["", "(none)"], ...S.params.filter((p) => p.param_kind === "material").map((p) => [p.name, p.name])], f.material_param || "", (v) => change((b) => { formById(f.id).material_param = v || null; }, true))));
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
    visBlock(r, f, (b) => formById(f.id));
  }

  // The profile of a sweep: loops of rectangles, circles or points. Every number is an expression
  // (a value or a formula using parameters) measured across and up from the path start.
  function sweepProfile(r, f) {
    const loops = f.shape.profile;
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
          const cur = sh.profile[li];
          let u0 = "-25", u1 = "25", v0 = "-25", v1 = "25";
          if (kind === "rect") { u0 = cur.points[0].u; u1 = cur.points[1].u; v0 = cur.points[0].v; v1 = cur.points[2].v; }
          if (v === "circle") sh.profile[li] = { type: "circle", u: "0", v: "0", r: "25" };
          else sh.profile[li] = rect(u0, u1, v0, v1);
        });
      });
      head.append(sel);
      if (loops.length > 1) { const del = el("button", "", "Remove"); del.onclick = () => set((sh) => { sh.profile.splice(li, 1); }); head.append(del); }
      r.append(head);
      if (kind === "rect") {
        const p = l.points;
        const put = (u0, u1, v0, v1) => set((sh) => { sh.profile[li] = rect(u0, u1, v0, v1); });
        exprIn("Across from", p[0].u, (v) => put(v, p[1].u, p[0].v, p[2].v));
        exprIn("Across to", p[1].u, (v) => put(p[0].u, v, p[0].v, p[2].v));
        exprIn("Up from", p[0].v, (v) => put(p[0].u, p[1].u, v, p[2].v));
        exprIn("Up to", p[2].v, (v) => put(p[0].u, p[1].u, p[0].v, v));
      } else if (kind === "circle") {
        exprIn("Centre across", l.u, (v) => set((sh) => { sh.profile[li].u = v; }));
        exprIn("Centre up", l.v, (v) => set((sh) => { sh.profile[li].v = v; }));
        exprIn("Radius", l.r, (v) => set((sh) => { sh.profile[li].r = v; }));
      } else {
        l.points.forEach((n, i) => {
          const row = el("div", "famed-row");
          const iu = textIn(n.u, (v) => set((sh) => { sh.profile[li].points[i].u = v.trim() || "0"; }), { list: lens });
          const iv = textIn(n.v, (v) => set((sh) => { sh.profile[li].points[i].v = v.trim() || "0"; }), { list: lens });
          const del = el("button", "", "x"); del.title = "Remove this point";
          del.onclick = () => { if (l.points.length > 3) set((sh) => { const q = sh.profile[li]; q.points.splice(i, 1); if ((q.bulges || []).length) q.bulges.splice(i, 1); }); };
          row.append(el("span", "", `Point ${i + 1}`), iu, iv, del);
          r.append(row);
        });
        const add = el("button", "", "Add point");
        add.onclick = () => set((sh) => { const q = sh.profile[li]; q.points.push({ u: "0", v: "0" }); if ((q.bulges || []).length) q.bulges.push(0); });
        r.append(add);
      }
    });
    const addLoop = el("button", "", "Add loop");
    addLoop.onclick = () => set((sh) => { sh.profile.push(rect("-10", "10", "-10", "10")); });
    r.append(addLoop);
  }

  function symProps(r, y) {
    if (!y) return;
    r.append(el("div", "famed-hint", `Symbolic geometry, ${y.view} views only`));
    r.append(field("Subcategory", textIn(y.subcategory || "", (v) => change((b) => { symById(y.id).subcategory = v; }, true), { list: SUBS })));
    r.append(field("Filled", checkIn(y.filled, (v) => change((b) => { symById(y.id).filled = v; }, true))));
    r.append(field("Masking", checkIn(y.masking, (v) => change((b) => { symById(y.id).masking = v; }, true))));
    visBlock(r, y, (b) => symById(y.id));
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
      S.dirty = true; S.msg = "Type values are written when you save."; status();
    }));
  }
  async function setFlex(p, v) {
    S.values[p.name] = v;
    const ok = await preview();
    if (!ok) { S.msg = S.pvError; delete S.values[p.name]; await preview(); } else S.msg = "";
    status(); render(); props();
  }

  // The category and host are kept by the project and change at once (refused while instances are placed).
  async function setMeta(patch) {
    try {
      await Backend.call("set_family_meta", { id: S.id, ...patch });
      if (patch.category) { S.def.category = patch.category; const c = (S.def.categories || []).find((x) => x.id === patch.category); S.def.category_label = c ? c.label : patch.category; const tag = document.querySelector(".fe-cat"); if (tag) tag.textContent = S.def.category_label; }
      if (patch.host) S.def.host = patch.host;
      S.msg = "";
      await refresh(true);
    } catch (e) { S.msg = String(e.message || e); }
    props(); status();
  }

  // ---------------------------------------------------------------- family types dialog
  // Every type of the family side by side: one column per type, one row per type parameter.
  function typesDialog() {
    openModal("Family Types", (body, foot, close) => {
      const work = S.types.map((t) => ({ id: t.id, name: t.name, values: { ...t.values } }));
      const gone = [];
      const msg = el("div", "modal-msg");
      const tbl = el("div", "fe-ptable");
      const rows = () => S.params.filter((p) => !p.instance && !p.formula && !p.reporting && ["length", "number", "integer", "angle", "yes_no", "text", "material"].includes(p.param_kind));
      const draw = () => {
        tbl.replaceChildren();
        tbl.style.gridTemplateColumns = `200px repeat(${work.length}, minmax(110px, 1fr))`;
        const head = el("div", "fe-prow fe-phead"); head.style.gridTemplateColumns = tbl.style.gridTemplateColumns;
        head.append(el("span", "", "Parameter"));
        work.forEach((w, i) => {
          const c = el("span"); const n = textIn(w.name, (v) => { w.name = v; });
          const del = button("x", () => { if (work.length < 2) { msg.textContent = "A family needs at least one type."; return; } const [d] = work.splice(i, 1); if (d.id) gone.push(d); draw(); });
          del.title = "Delete this type"; c.append(n, del); head.append(c);
        });
        tbl.append(head);
        for (const p of rows()) {
          const row = el("div", "fe-prow"); row.style.gridTemplateColumns = tbl.style.gridTemplateColumns;
          row.append(el("span", "", p.name));
          for (const w of work) {
            const cur = w.values[p.name] !== undefined ? w.values[p.name] : p.default;
            const c = el("span");
            if (p.param_kind === "yes_no") c.append(checkIn(cur, (v) => { w.values[p.name] = v; }));
            else if (p.choices && p.choices.length) c.append(selIn(p.choices.map((x) => [x, x]), cur, (v) => { w.values[p.name] = v; }));
            else c.append(textIn(String(cur), (v) => { w.values[p.name] = ["text", "material"].includes(p.param_kind) ? v : num(v); }));
            row.append(c);
          }
          tbl.append(row);
        }
      };
      draw();
      body.append(el("div", "note", "One column per type. Instance parameters are set on each placed copy, not here. Types with placed copies cannot be deleted."), tbl, msg);
      foot.append(
        button("Add type", () => { const src = work[work.length - 1]; work.push({ id: null, from: src.id || src.from, name: `Type ${work.length + 1}`, values: { ...src.values } }); draw(); }),
        button("Cancel", close),
        button("OK", async () => {
          try {
            // the parameters must be in the project before types can hold values for them
            await Backend.call("set_family_def", { id: S.id, params: S.params, body: S.body });
            for (const w of work) {
              let id = w.id;
              if (!id) id = (await Backend.call("duplicate_family_type", { id: w.from, name: w.name })).id;
              const old = S.types.find((t) => t.id === id);
              const patch = { values: w.values };
              if (!old || old.name !== w.name) patch.name = w.name;
              await Backend.call("patch_family_type", { id, patch });
            }
            for (const g of gone) await Backend.call("delete_family_type", { id: g.id });
            close();
            await reloadTypes();
          } catch (e) { msg.textContent = String(e.message || e); await reloadTypes(); }
        }, true),
      );
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

  // ---------------------------------------------------------------- parameters dialog
  function paramsDialog() {
    openModal("Family Parameters", (body, foot, close) => {
      const work = clone(S.params);
      const msg = el("div", "modal-msg");
      const tbl = el("div", "fe-ptable");
      const draw = () => {
        tbl.replaceChildren();
        const hd = el("div", "fe-prow fe-phead");
        ["Name", "Kind", "Type / Instance", "Group", "Default", "Formula", ""].forEach((t) => hd.append(el("span", "", t)));
        tbl.append(hd);
        work.forEach((p, i) => {
          const row = el("div", "fe-prow");
          const nm = textIn(p.name, (v) => { p.name = v; });
          const kind = selIn(KINDS, p.param_kind, (v) => { p.param_kind = v; p.default = v === "yes_no" ? false : ["text", "material"].includes(v) ? "" : 0; draw(); });
          const inst = selIn([["false", "Type"], ["true", "Instance"]], String(!!p.instance), (v) => { p.instance = v === "true"; });
          const grp = textIn(p.group || "", (v) => { p.group = v; }, { list: ["Dimensions", "Constraints", "Construction", "Materials and Finishes", "Identity Data"] });
          const def = p.param_kind === "yes_no" ? checkIn(p.default, (c) => { p.default = c; }) : textIn(String(p.default), (v) => { p.default = ["text", "material"].includes(p.param_kind) ? v : num(v); });
          const dimRep = p.reporting && !p.formula && (S.body.dims || []).some((d) => d.reports_to === p.name);
          const fm = dimRep ? el("span", "", "reads a dimension") : textIn(p.formula || "", (v) => { p.formula = v.trim() ? v : null; });
          const del = button("Delete", () => { work.splice(i, 1); draw(); });
          [nm, kind, inst, grp, def, fm, del].forEach((n) => { const c = el("span"); c.append(n); row.append(c); });
          tbl.append(row);
        });
      };
      draw();
      body.append(el("div", "note", "Type parameters are set per type. Use a parameter in a reference plane's dimension as [Name]. A formula makes the parameter read only."), tbl, msg);
      foot.append(
        button("Add parameter", () => { work.push({ name: `Parameter ${work.length + 1}`, param_kind: "length", instance: false, group: "Dimensions", formula: null, reporting: false, shared_guid: null, choices: [], default: 0 }); draw(); }),
        button("Cancel", close),
        button("OK", async () => {
          const before = snapshot();
          S.params = work;
          // a dimension whose parameter was deleted or renamed stops reporting
          S.body.dims = (S.body.dims || []).filter((d) => work.some((p) => p.name === d.reports_to));
          const ok = await preview();
          if (!ok) { const old = JSON.parse(before); S.params = old.params; S.body.dims = old.body.dims; msg.textContent = S.pvError; await preview(); return; }
          S.undo.push(before); S.dirty = true; close(); props(); render();
        }, true),
      );
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
    if (!S.pv) return;
    if (S.view === "3d") { render3D(); return; }
    const V = VIEWS[S.view];
    const accent = css("--accent"), text = css("--text"), muted = css("--muted");
    const c = cam();
    // geometry
    for (const pass of ["solid", "void"]) {
      for (const pc of S.pv.views[PREVIEW_KEY[S.view]] || []) {
        if ((pc.void ? "void" : "solid") !== pass) continue;
        const selected = S.sel && S.sel.kind === "form" && S.sel.id === pc.form;
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
      const selected = S.sel && S.sel.kind === "sym" && S.sel.id === y.id;
      ctx.beginPath();
      y.points.forEach(([x, yy], i) => { const p = w2s(x, yy); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); });
      if (y.closed) ctx.closePath();
      if (y.filled) { ctx.fillStyle = "rgba(60,60,60,0.35)"; ctx.fill(); }
      ctx.strokeStyle = selected ? accent : "#2f8f5b"; ctx.lineWidth = selected ? 2.5 : 1.5; ctx.stroke();
    }
    // reference planes
    ctx.font = "11px system-ui, sans-serif";
    const labelled = { v: [], h: [] };
    // the selected plane is labelled first so it always has its name
    const ordered = [...viewPlanes()].sort((a, b) => (S.sel && S.sel.kind === "plane" ? (b.id === S.sel.id) - (a.id === S.sel.id) : 0));
    for (const p of ordered) {
      const at = pos(p.id);
      if (at === undefined) continue;
      const selected = S.sel && S.sel.kind === "plane" && S.sel.id === p.id;
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
    drawDraft(V);
  }

  function drawDims(V) {
    const { ctx } = S;
    const selPlane = S.sel && S.sel.kind === "plane" ? S.sel.id : null;
    const dims = S.body.planes.filter((p) => p.anchor.type === "offset" && (p.axis === V.h || p.axis === V.v) && (S.show.dims || p.id === selPlane));
    let ih = 0, iv = 0;
    ctx.font = "11px system-ui, sans-serif";
    for (const p of dims) {
      const a = pos(p.anchor.from), b = pos(p.id);
      if (a === undefined || b === undefined) continue;
      const horizontal = p.axis === V.h; // measured along the view's horizontal axis
      ctx.strokeStyle = "#8a6d1f"; ctx.fillStyle = "#8a6d1f"; ctx.lineWidth = 1;
      const label = `${String(p.anchor.by).replace(/[\[\]]/g, "")}  ${(b - a).toFixed(0)}`;
      if (horizontal) {
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
    // reporting dimensions
    for (const d of S.body.dims || []) {
      const pa = planeById(d.a), pb = planeById(d.b);
      if (!pa || !pb || !(pa.axis === V.h || pa.axis === V.v) || !(S.show.dims)) continue;
      const a = pos(d.a), b = pos(d.b);
      if (a === undefined || b === undefined) continue;
      ctx.strokeStyle = "#2f7d6b"; ctx.fillStyle = "#2f7d6b"; ctx.lineWidth = 1;
      const label = `${d.reports_to || ""} = ${Math.abs(b - a).toFixed(0)}`;
      if (pa.axis === V.h) {
        const y = S.canvas.height - 24 - (ih++ % 4) * 16, x0 = w2s(a, 0).x, x1 = w2s(b, 0).x;
        ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke(); tick(x0, y, true); tick(x1, y, true);
        ctx.textAlign = "center"; ctx.textBaseline = "bottom"; ctx.fillText(label, (x0 + x1) / 2, y - 2);
      } else {
        const x = 40 + (iv++ % 4) * 16, y0 = w2s(0, a).y, y1 = w2s(0, b).y;
        ctx.beginPath(); ctx.moveTo(x, y0); ctx.lineTo(x, y1); ctx.stroke(); tick(x, y0, false); tick(x, y1, false);
        ctx.save(); ctx.translate(x + 12, (y0 + y1) / 2); ctx.rotate(-Math.PI / 2); ctx.textAlign = "center"; ctx.textBaseline = "bottom"; ctx.fillText(label, 0, 0); ctx.restore();
      }
    }
    function tick(x, y, h) { ctx.beginPath(); if (h) { ctx.moveTo(x, y - 5); ctx.lineTo(x, y + 5); } else { ctx.moveTo(x - 5, y); ctx.lineTo(x + 5, y); } ctx.stroke(); }
  }

  // loops of the selected form or symbolic item, with the axis of their sketch plane
  function selectedLoops() {
    if (!S.sel) return [];
    if (S.sel.kind === "form") {
      const f = formById(S.sel.id);
      if (!f) return [];
      const wp = planeById(f.shape.plane), ax = wp && wp.axis;
      if (f.shape.type === "sweep") return [{ loop: f.shape.path, plane: ax }];
      if (f.shape.type === "blend") return [...f.shape.base, ...f.shape.top].map((loop) => ({ loop, plane: ax }));
      if (f.shape.type === "revolve") return [...f.shape.profile.map((loop) => ({ loop, plane: ax })), { loop: { nodes: [f.shape.axis_a, f.shape.axis_b], bulges: [], closed: false }, plane: ax }];
      return f.shape.profile.map((loop) => ({ loop, plane: ax }));
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
    ctx.fillStyle = m.u.plane || m.v.plane ? "#d9822b" : css("--accent");
    ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fill();
  }

  // ---- 3D preview ----
  function render3D() {
    const { ctx, canvas } = S;
    const { a, b } = S.rot;
    const rot = ([x, y, z]) => {
      // z up; turn about z by a, then tilt by b
      const x1 = x * Math.cos(a) - y * Math.sin(a), y1 = x * Math.sin(a) + y * Math.cos(a);
      return [x1, y1 * Math.cos(b) - z * Math.sin(b), y1 * Math.sin(b) + z * Math.cos(b)];
    };
    const tris = [], edgeMap = {};
    let mn = [Infinity, Infinity], mx = [-Infinity, -Infinity];
    for (const f of S.pv.forms) {
      if (!f.present) continue;
      for (let i = 0; i + 8 < f.tris.length; i += 9) {
        const p = [0, 1, 2].map((k) => rot([f.tris[i + k * 3], f.tris[i + k * 3 + 1], f.tris[i + k * 3 + 2]]));
        const u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        const n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        const l = Math.hypot(...n) || 1;
        const depth = (p[0][1] + p[1][1] + p[2][1]) / 3;
        const o = [0, 1, 2].map((k) => [f.tris[i + k * 3], f.tris[i + k * 3 + 1], f.tris[i + k * 3 + 2]]);
        const ou = [o[1][0] - o[0][0], o[1][1] - o[0][1], o[1][2] - o[0][2]], ov = [o[2][0] - o[0][0], o[2][1] - o[0][1], o[2][2] - o[0][2]];
        const on = [ou[1] * ov[2] - ou[2] * ov[1], ou[2] * ov[0] - ou[0] * ov[2], ou[0] * ov[1] - ou[1] * ov[0]];
        const ol = Math.hypot(...on) || 1;
        const nn = on.map((c) => c / ol);
        const edges = [[0, 1], [1, 2], [2, 0]].map(([e0, e1]) => {
          const key = f.id + ":" + [o[e0], o[e1]].map((q) => q.map((c) => Math.round(c)).join(",")).sort().join("|");
          (edgeMap[key] = edgeMap[key] || []).push(nn);
          return key;
        });
        tris.push({ p, edges, depth, shade: Math.abs(n[2] / l) * 0.6 + 0.4, face: n[1] / l, void: f.void, glass: f.sub === "Glass", id: f.id });
        for (const q of p) { mn = [Math.min(mn[0], q[0]), Math.min(mn[1], q[2])]; mx = [Math.max(mx[0], q[0]), Math.max(mx[1], q[2])]; }
      }
    }
    if (!tris.length) return;
    const k = Math.min((canvas.width - 80) / Math.max(mx[0] - mn[0], 1), (canvas.height - 80) / Math.max(mx[1] - mn[1], 1));
    const ox = canvas.width / 2 - ((mx[0] + mn[0]) / 2) * k, oy = canvas.height / 2 + ((mx[1] + mn[1]) / 2) * k;
    // solids back to front, then the translucent voids over them
    tris.sort((p, q) => (p.void - q.void) || (q.depth - p.depth));
    for (const t of tris) {
      ctx.beginPath();
      t.p.forEach((q, i) => { const x = ox + q[0] * k, y = oy - q[2] * k; i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
      ctx.closePath();
      const sel = S.sel && S.sel.kind === "form" && S.sel.id === t.id;
      const g = Math.round(150 + 90 * t.shade);
      ctx.fillStyle = t.void ? "rgba(200,60,150,0.18)" : t.glass ? "rgba(110,190,230,0.4)" : sel ? `rgb(${g - 40},${g},${g + 15})` : `rgb(${g - 30},${g - 10},${g})`;
      ctx.fill();
      ctx.strokeStyle = sel ? css("--accent") : "rgba(0,0,0,0.35)"; ctx.lineWidth = sel ? 1.5 : 0.7;
      // outline real edges only: skip the diagonal between two triangles on the same face
      for (let e = 0; e < 3; e++) {
        const ns = edgeMap[t.edges[e]];
        if (ns.length === 2 && ns[0][0] * ns[1][0] + ns[0][1] * ns[1][1] + ns[0][2] * ns[1][2] > 0.999) continue;
        const a0 = t.p[e], a1 = t.p[(e + 1) % 3];
        ctx.beginPath(); ctx.moveTo(ox + a0[0] * k, oy - a0[2] * k); ctx.lineTo(ox + a1[0] * k, oy - a1[2] * k); ctx.stroke();
      }
    }
  }

  // ---------------------------------------------------------------- pointer
  // The point under the cursor in the view: coordinates that lock to a reference plane when close to one
  function snapPoint(sx, sy) {
    const V = VIEWS[S.view];
    const w = s2w(sx, sy);
    const out = { pt: { u: Math.round(w.u), v: Math.round(w.v) }, u: { plane: null, off: Math.round(w.u) }, v: { plane: null, off: Math.round(w.v) } };
    let bu = 9, bv = 9;
    for (const p of viewPlanes()) {
      const at = pos(p.id);
      if (at === undefined) continue;
      if (p.axis === V.h) { const d = Math.abs(w2s(at, 0).x - sx); if (d < bu) { bu = d; out.u = { plane: p.id, off: 0 }; out.pt.u = at; } }
      else { const d = Math.abs(w2s(0, at).y - sy); if (d < bv) { bv = d; out.v = { plane: p.id, off: 0 }; out.pt.v = at; } }
    }
    return out;
  }
  const coordOf = (c) => (c.plane ? { plane: c.plane, off: 0 } : { off: c.off });
  const mouse = (e) => { const r = S.canvas.getBoundingClientRect(); return { x: ((e.clientX - r.left) * S.canvas.width) / r.width, y: ((e.clientY - r.top) * S.canvas.height) / r.height }; };

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
      if (f.shape.type === "sweep") out.push({ kind: "form", id: f.id, li: 0, loop: f.shape.path, which: "path" });
      if (f.shape.type === "blend") { f.shape.base.forEach((loop, li) => out.push({ kind: "form", id: f.id, li, loop })); f.shape.top.forEach((loop, li) => out.push({ kind: "form", id: f.id, li, loop, which: "top" })); }
    }
    for (const y of S.body.syms) {
      if (y.view === S.view) out.push({ kind: "sym", id: y.id, li: 0, loop: y.path });
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
      if (!f || f.shape.type !== "extrusion" || !pf.span) continue;
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
    const owner = h.kind === "form" ? (({ shape: sh }) => (sh.type === "sweep" ? [sh.path] : sh.type === "blend" ? (h.which === "top" ? sh.top : sh.base) : sh.profile)[h.li])(b.forms.find((f) => f.id === h.id)) : b.syms.find((y) => y.id === h.id).path;
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
    for (const y of [...(S.pv.syms || [])].reverse()) {
      if (!y.present || y.view !== S.view) continue;
      for (let i = 0; i + 1 < y.points.length; i++) {
        const a = w2s(...y.points[i]), b = w2s(...y.points[i + 1]);
        const l2 = (b.x - a.x) ** 2 + (b.y - a.y) ** 2 || 1;
        const t = Math.max(0, Math.min(1, ((sx - a.x) * (b.x - a.x) + (sy - a.y) * (b.y - a.y)) / l2));
        if (Math.hypot(a.x + t * (b.x - a.x) - sx, a.y + t * (b.y - a.y) - sy) < 5) return { kind: "sym", id: y.id };
      }
    }
    const pieces = [...(S.pv.views[PREVIEW_KEY[S.view]] || [])].reverse();
    for (const pc of pieces) {
      let inside = false;
      for (const ring of pc.rings) if (pointIn(ring, w.u, w.v)) inside = !inside;
      if (inside) return { kind: "form", id: pc.form };
    }
    return null;
  }

  async function onDown(e) {
    if (!S) return;
    S.canvas.setPointerCapture(e.pointerId);
    const m = mouse(e);
    S.mouse = m;
    if (S.view === "3d") { S.drag = { rot: true, x: m.x, y: m.y, a: S.rot.a, b: S.rot.b }; return; }
    if (e.button === 1 || e.button === 2) { S.drag = { pan: true, x: m.x, y: m.y, ox: cam().ox, oy: cam().oy }; return; }
    const t = S.tool;
    if (t === "select") {
      const h = hitHandle(m.x, m.y);
      if (h) { S.drag = { handle: h, before: snapshot(), moved: false }; return; }
      const pl = hitPlane(m.x, m.y);
      if (pl) { S.sel = { kind: "plane", id: pl.id }; props(); render(); S.drag = pl.anchor.type === "offset" && /^\s*-?\d+(\.\d+)?\s*$/.test(String(pl.anchor.by)) ? { plane: pl.id, before: snapshot(), moved: false } : null; return; }
      S.sel = hitItem(m.x, m.y); props(); render();
      return;
    }
    if (t === "plane") {
      const w = s2w(m.x, m.y);
      S.draft = { kind: "plane", a: w, down: m };
      return;
    }
    if (t === "dim" || t === "eq") {
      const pl = hitPlane(m.x, m.y);
      if (!pl) return;
      S.picks = (S.picks || []).concat(pl.id);
      if (t === "dim" && S.picks.length === 2) { const [a, b] = S.picks; S.picks = []; await makeDim(a, b); }
      else if (t === "eq" && S.picks.length === 3) { const [a, b, mid] = S.picks; S.picks = []; await makeEq(a, b, mid); }
      else { S.msg = `${S.picks.length} picked.`; status(); }
      return;
    }
    if (t === "align") {
      if (!S.alignPlane) {
        const pl = hitPlane(m.x, m.y);
        if (pl) { S.alignPlane = pl.id; S.msg = `Aligning to ${pl.name}. Click an edge of a form or symbolic line.`; status(); render(); }
        return;
      }
      const pl = planeById(S.alignPlane);
      const hit = pl && (hitEdge(m.x, m.y, pl) || hitDepthEdge(m.x, m.y, pl));
      if (!hit) { S.msg = "Click an edge that runs along the plane, or press Esc."; status(); return; }
      S.alignPlane = null;
      if (hit.kind === "depth") {
        await change((b) => { formById(hit.id).shape[hit.which + "_lock"] = pl.id; b.forms.find((f) => f.id === hit.id); S.sel = { kind: "form", id: hit.id }; });
        S.msg = `The ${hit.which} of the extrusion is aligned and locked to ${pl.name}.`; status();
        return;
      }
      await change((b) => {
        const nodes = hitEdgeNodes(b, hit);
        for (const n of nodes) { if (pl.axis === VIEWS[S.view].h) n.u = { plane: pl.id, off: 0 }; else n.v = { plane: pl.id, off: 0 }; }
      });
      S.msg = "Aligned and locked to the plane."; status();
      return;
    }
    if (t === "sweep") {
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
    if (t === "sym" && S.symMode === "arc") {
      const sp = snapPoint(m.x, m.y);
      S.draft = S.draft && S.draft.kind === "arc" ? S.draft : { kind: "arc", pts: [] };
      S.draft.pts.push(sp);
      if (S.draft.pts.length === 3) await finishArc();
      render();
      return;
    }
    if (t === "sym") {
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
    const d = S.drag;
    if (d) {
      if (d.rot) { S.rot = { a: d.a + (m.x - d.x) * 0.01, b: Math.max(-1.5, Math.min(1.5, d.b + (m.y - d.y) * 0.01)) }; render(); return; }
      if (d.pan) { const c = cam(); c.ox = d.ox + m.x - d.x; c.oy = d.oy + m.y - d.y; render(); return; }
      if (d.plane) {
        const p = planeById(d.plane), V = VIEWS[S.view];
        const w = s2w(m.x, m.y), at = p.axis === V.h ? w.u : w.v;
        const from = pos(p.anchor.from);
        p.anchor.by = String(Math.round(at - from));
        d.moved = true;
        await preview(); props(); render(); return;
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
    if (d && (d.plane || d.handle) && d.moved) {
      const ok = await preview();
      if (!ok) { restore(d.before); S.msg = S.pvError; await preview(); }
      else { S.undo.push(d.before); S.redo = []; S.dirty = true; }
      props(); status(); render();
    }
    if (S.draft && S.draft.kind === "plane" && S.tool === "plane") {
      const dr = S.draft; S.draft = null;
      const dx = m.x - dr.down.x, dy = m.y - dr.down.y;
      const V = VIEWS[S.view];
      // a mostly horizontal stroke makes a horizontal line: a plane normal to the view's vertical axis
      const axis = Math.abs(dx) >= Math.abs(dy) && Math.hypot(dx, dy) > 12 ? V.v : Math.hypot(dx, dy) > 12 ? V.h : null;
      if (!axis) { S.msg = "Drag along the line the plane should follow."; status(); return; }
      const at = Math.round(axis === V.h ? dr.a.u : dr.a.v);
      await change((b) => {
        const id = newId();
        const origin = b.planes.find((p) => p.axis === axis && p.anchor.type === "origin");
        const n = b.planes.filter((p) => /^Reference Plane/.test(p.name)).length + 1;
        b.planes.push({ id, name: `Reference Plane ${n}`, axis, anchor: { type: "offset", from: origin.id, by: String(at) }, ref_kind: "weak" });
        S.sel = { kind: "plane", id };
      });
    }
  }

  async function onDbl(e) {
    if (!S) return;
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
    if (e.key === "Escape") { if (S.alignPlane || S.blend || S.rev) { S.alignPlane = null; S.blend = null; S.rev = null; S.draft = null; S.msg = "Cancelled."; status(); render(); } else if (S.draft || (S.picks && S.picks.length)) { S.draft = null; S.picks = []; render(); } else if (S.tool !== "select") { setTool("select"); } else if (S.sel) { S.sel = null; props(); render(); } e.preventDefault(); e.stopPropagation(); return; }
    if (e.key === "Delete") { deleteSel(); e.preventDefault(); return; }
    if (e.key === "Enter" && S.draft && S.draft.kind === "poly") { await finishPoly(); render(); e.preventDefault(); return; }
    if (e.key === "Enter" && S.draft && S.draft.kind === "path") { await finishPath(); render(); e.preventDefault(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); e.stopPropagation(); await undo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); e.stopPropagation(); await redo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") { e.preventDefault(); e.stopPropagation(); await save(false); }
  }

  // ---------------------------------------------------------------- making things
  // A dimension that only reports: its length goes to a new read-only parameter.
  async function makeReport(a, b) {
    const pa = planeById(a), pb = planeById(b);
    if (pa.axis !== pb.axis) { S.msg = "Pick two parallel planes."; status(); return; }
    if (a === b) { S.msg = "Pick two different planes."; status(); return; }
    askText("Reporting parameter", "Parameter name", "Reported Length", [], async (v) => {
      const name = v.trim();
      if (!name) return;
      if (S.params.some((p) => p.name === name)) { S.msg = `A parameter called '${name}' already exists.`; status(); return; }
      await change((body) => {
        S.params.push({ name, param_kind: "length", instance: false, group: "Dimensions", formula: null, reporting: true, shared_guid: null, choices: [], default: 0 });
        (body.dims = body.dims || []).push({ a, b, reports_to: name });
      });
      S.msg = `${name} reports the distance between the two planes.`; status();
    });
  }
  async function makeDim(a, b) {
    if (S.dimMode === "report") { await makeReport(a, b); return; }
    const pa = planeById(a), pb = planeById(b);
    if (pa.axis !== pb.axis) { S.msg = "Pick two parallel planes."; status(); return; }
    const [lo, hi] = pos(a) <= pos(b) ? [pa, pb] : [pb, pa];
    // the plane that is not an origin plane takes the dimension; both origin planes cannot be dimensioned
    const target = hi.anchor.type !== "origin" ? hi : lo.anchor.type !== "origin" ? lo : null;
    if (!target) { S.msg = "Both are origin planes."; status(); return; }
    const other = target === hi ? lo : hi;
    const sign = target === hi ? 1 : -1;
    const dist = Math.round(Math.abs(pos(a) - pos(b)));
    askText("Dimension", "Label or value", `${dist}`, lengthParams(), async (v) => {
      const by = v.trim() || String(dist);
      await change((body) => {
        const t = body.planes.find((p) => p.id === target.id);
        t.anchor = { type: "offset", from: other.id, by: sign < 0 ? (/^[\d.]+$/.test(by) ? `-${by}` : `-(${by})`) : by };
        S.sel = { kind: "plane", id: t.id };
      });
    });
  }
  async function makeEq(a, b, mid) {
    const pm = planeById(mid);
    if (pm.anchor.type === "origin") { S.msg = "The middle plane cannot be an origin plane."; status(); return; }
    await change((body) => { body.planes.find((p) => p.id === mid).anchor = { type: "mid", a, b }; S.sel = { kind: "plane", id: mid }; });
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
    if (S.tool === "sym") {
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

  async function addForm(profile) {
    if (S.tool === "blend" || S.tool === "revolve") { await commitSpecial(profile); return; }
    const V = VIEWS[S.view];
    const wp = S.wp && planeById(S.wp) && planeById(S.wp).axis === V.n ? S.wp : V.origin;
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => !!f.void === (S.tool === "void")).length + 1;
      b.forms.push({
        id, name: S.tool === "void" ? `Void ${n}` : `Extrusion ${n}`, void: S.tool === "void", cuts_host: true, cut_by: [],
        shape: { type: "extrusion", plane: wp, start: S.start, end: S.end, profile },
        material: "", material_param: null, subcategory: S.tool === "void" ? "" : "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
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
    const wp = S.wp && planeById(S.wp) && planeById(S.wp).axis === V.n ? S.wp : V.origin;
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => f.shape.type === "blend").length + 1;
      b.forms.push({
        id, name: `Blend ${n}`, void: false, cuts_host: true, cut_by: [],
        shape: { type: "blend", plane: wp, start: S.start, end: S.end, base, top: profile },
        material: "", material_param: null, subcategory: "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
  }
  async function addRevolve() {
    const { profile, pts } = S.rev;
    S.rev = null; S.msg = "";
    if (pts[0].pt.u === pts[1].pt.u && pts[0].pt.v === pts[1].pt.v) { S.msg = "The axis needs two different points."; status(); return; }
    const V = VIEWS[S.view];
    const wp = S.wp && planeById(S.wp) && planeById(S.wp).axis === V.n ? S.wp : V.origin;
    const node = (p) => ({ u: coordOf(p.u), v: coordOf(p.v) });
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => f.shape.type === "revolve").length + 1;
      b.forms.push({
        id, name: `Revolve ${n}`, void: false, cuts_host: true, cut_by: [],
        shape: { type: "revolve", plane: wp, axis_a: node(pts[0]), axis_b: node(pts[1]), start_deg: Number(S.a0) || 0, end_deg: S.a1 === "" || isNaN(Number(S.a1)) ? 360 : Number(S.a1), profile },
        material: "", material_param: null, subcategory: "Frame/Mullion", vis: {},
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
    const wp = S.wp && planeById(S.wp) && planeById(S.wp).axis === V.n ? S.wp : V.origin;
    const bulges = d.bulges.slice();
    if (S.pathClosed) bulges.push(0);
    const path = { nodes: q.map((p) => ({ u: coordOf(p.u), v: coordOf(p.v) })), bulges, closed: S.pathClosed && q.length > 2 };
    if (!path.closed) path.bulges = d.bulges.slice();
    await change((b) => {
      const id = newId();
      const n = b.forms.filter((f) => f.shape.type === "sweep").length + 1;
      b.forms.push({
        id, name: `Sweep ${n}`, void: false, cuts_host: true, cut_by: [],
        shape: { type: "sweep", plane: wp, path, profile: [{ type: "poly", points: [["-25", "-25"], ["25", "-25"], ["25", "25"], ["-25", "25"]].map(([u, v]) => ({ u, v })), bulges: [] }], profile_angle: 0, flip: false },
        material: "", material_param: null, subcategory: "Frame/Mullion", vis: {},
      });
      S.sel = { kind: "form", id };
    });
    S.msg = "Swept a 50 x 50 profile. Set its size and position in Properties.";
    status();
  }
  async function addSym(path, filled) {
    const view = S.view;
    await change((b) => {
      const id = newId();
      b.syms.push({ id, view, path, filled, masking: false, subcategory: filled ? "Panel" : "Plan Swing", vis: {} });
      S.sel = { kind: "sym", id };
    });
  }

  async function deleteSel() {
    const s = S.sel;
    if (!s) return;
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
      S.sel = null;
    });
  }

  return { list, open, newFamily, loadFile, exportFamily, activate, close: closeEditor, commands, syncRibbon, docInfo, undoState: () => (S ? { undo: S.undo.length, redo: S.redo.length } : null), undo, redo, save, _state: () => S };
})();
