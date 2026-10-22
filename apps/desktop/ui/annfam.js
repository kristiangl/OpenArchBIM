// Annotation families as documents. A level head, an elevation mark or a section mark opens in the main window as a
// model family does (index.html #fam-*: Properties, Project Browser, view tab, options bar), on a canvas of its own,
// with a ribbon of its own (ribbon.js ANN_RIBBON). The drawing sits round the crossing of two reference lines, in
// paper millimetres: the horizontal one is what the symbol sits on (the level line for a head), the vertical one is
// where a head flips for the left hand end of a line. Changes stay in the document until Load into Project
// (crates/openarch-model/src/annotation.rs checks them there).
//
// The drawing is lines and regions, made with the project's sketch tools (sketch.js, a session of its own here):
// - Line works on the lines themselves: every Draw tool places lines straight away, and the Modify tools (trim,
//   offset, split, move ...) change them, with a subcategory for each line. Esc twice leaves it.
// - Filled Region and Masking Region sketch closed loops with Finish and Cancel; each edge has its own line style.
// Labels (parameters chosen in the Edit Label dialog) and text are written with label and text types, in a box whose
// width they wrap at; the editor lays them out itself (annotation.js Ann.layout), showing the labels' sample values.
// The session works at 100 units to the paper millimetre (annotation.rs SKETCH_SCALE) so the sketch tolerances,
// made for model millimetres, suit symbols a few millimetres across. The core draws the result (annotation_paths).

const AnnModel = (() => {
  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const snapMM = (v) => Math.round(v * 2) / 2; // half a millimetre, for labels and text
  const FONTS = ["Arial", "Arial Narrow", "Calibri", "Century Gothic", "Consolas", "Courier New", "Georgia", "Segoe UI", "Tahoma", "Times New Roman", "Verdana"];
  const $q = (s) => document.querySelector(s);
  const U = 100; // sketch units to the paper millimetre
  const INVISIBLE = "<Invisible lines>";
  const POINTER_INK = "#a0522d";
  // the sketch tools' distances in paper mm, and the snap increments (10, 5, 1, 0.5 and 0.1 mm)
  const PAPER_OPTS = { cornerRadius: 0, radius: 5, filletRadius: 1, offsetDist: 1, wallOffset: 0 };
  const LENGTHS = [1000, 500, 100, 50, 10];
  const NOTE = {
    level_head: "Level head: the level line arrives from the left and ends at the vertical reference line.",
    elevation_marker: "Elevation mark: the body is drawn once. The pointer is drawn for each view, turned to point along +x here.",
    section_marker: "Section mark: the body is drawn at the head of the line. The pointer is drawn at each end for the view direction, along +x here.",
  };

  let S = null; // the open document
  let wired = false;

  // ---------------------------------------------------------------- curves between paper mm and sketch units
  function mapCurve(c, f, k) {
    switch (c.type) {
      case "line": return { ...c, a: f(c.a), b: f(c.b) };
      case "arc": return { ...c, start: f(c.start), end: f(c.end), centre: f(c.centre) };
      case "circle": return { ...c, centre: f(c.centre), radius: c.radius * k };
      case "ellipse": return { ...c, centre: f(c.centre), rx: c.rx * k, ry: c.ry * k };
      case "spline": return { ...c, points: c.points.map(f) };
      default: return c;
    }
  }
  const round6 = (v) => Math.round(v * 1e6) / 1e6;
  const toSketch = (c) => mapCurve(c, (p) => ({ x: p.x * U, y: p.y * U }), U);
  const toPaperCurve = (c) => mapCurve(c, (p) => ({ x: round6(p.x / U), y: round6(p.y / U) }), 1 / U);
  const moveCurve = (c, dx, dy) => mapCurve(c, (p) => ({ x: round6(p.x + dx), y: round6(p.y + dy) }), 1);

  // ---------------------------------------------------------------- open, close, load
  async function open(id) {
    if (S && S.id === id) { activate(); return; }
    if (S) { activate(); setPrompt(`Close "${S.fam.name}" before opening another annotation family.`, true); return; }
    let raw;
    try { raw = await Backend.call("annotation_family", { id }); } catch (e) { setPrompt(String(e.message || e), true); return; }
    const fam = clone(raw);
    for (const k of ["id", "params", "kind", "in_use", "category", "subcategories", "fill_patterns", "drawing", "annotation_widths"]) delete fam[k];
    S = {
      id, fam, params: raw.params, category: raw.category, subs: raw.subcategories || [], pats: raw.fill_patterns || {}, drawing: raw.drawing, tick: 0,
      widths: raw.annotation_widths || [],
      tool: "select", mode: null, layer: "body", sel: null, drag: null, mouse: null, flipped: false, undo: [], redo: [], dirty: false,
      view: { k: 14, ox: 0, oy: 0 }, msg: "", msgErr: false, sub: "", boundarySub: "", newType: (fam.region_types[0] || {}).name || "", lines: null, region: null,
    };
    mount();
    activate();
    fit();
  }

  const hasPointer = () => S.fam.family_kind !== "level_head";
  const docInfo = () => (S ? { name: S.fam.name, dirty: S.dirty, id: S.id } : null);
  const ctxRefresh = () => { if (typeof renderFamContext === "function") renderFamContext(); };

  // Bring the document to the front (after opening it, or switching back to it)
  function activate() {
    if (!S) return;
    setDoc("annotation");
    resize();
    tabsUI(); toolbar(); props(); status(); ctxRefresh();
  }

  function close(force) {
    if (!S) return;
    if (!force && S.dirty) { askLoad(); return; }
    if (ASk.active) ASk.end();
    S = null;
    setDoc("project");
    refresh(true);
  }

  // Closing with changes the project has not got: load them, leave them, or stay
  function askLoad() {
    activate();
    openModal(`Close ${S.fam.name}`, (body, foot, closeModal) => {
      body.append(el("p", "", `The changes to "${S.fam.name}" are not loaded into the project yet.`));
      foot.append(
        button("Load into Project and Close", async () => { closeModal(); await loadIntoProject(true); }, true),
        button("Close without Loading", () => { closeModal(); close(true); }),
        button("Cancel", closeModal),
      );
    });
  }

  async function loadIntoProject(andClose) {
    if (!S) return;
    if (S.mode === "region") { say("Finish or cancel the region's sketch first.", true); return; }
    if (S.mode === "lines") endLines();
    const name = S.fam.name;
    try { await Backend.call("set_annotation_family", { id: S.id, family: S.fam }); }
    catch (e) { say(String(e.message || e), true); return; }
    S.dirty = false;
    await refresh();
    if (andClose) { close(true); setPrompt(`Loaded ${name} into the project.`); return; }
    S.msg = "Loaded into the project.";
    activate();
  }

  // ---------------------------------------------------------------- undo, and the drawing the core makes
  const snapshot = () => JSON.stringify(S.fam);
  let drawSeq = 0;
  async function refreshDrawing() {
    if (!S) return;
    const my = ++drawSeq;
    let d;
    try { d = await Backend.call("annotation_paths", { family: S.fam }); } catch { return; }
    if (!S || my !== drawSeq) return;
    S.drawing = d; S.tick++;
    if (S.sel && S.sel.kind === "region") props();
    render();
  }
  // every change goes through here: one undo step, then redraw
  function edit(fn, keepProps) {
    S.undo.push(snapshot());
    if (S.undo.length > 100) S.undo.shift();
    S.redo = [];
    fn(S.fam);
    S.dirty = true;
    if (!keepProps) props();
    status(); render(); refreshDrawing();
  }
  function restore(json) { S.fam = JSON.parse(json); S.sel = null; S.dirty = true; props(); status(); ctxRefresh(); refreshDrawing(); }
  // Line steps back through its own changes, then the document's; a region's sketch keeps to its own
  function undo() {
    if (!S) return;
    if (S.mode === "region") { ASk.undo(); return; }
    if (S.mode === "lines") { if (ASk.canUndo) { ASk.undo(); return; } endLines(); }
    if (!S.undo.length) return;
    S.redo.push(snapshot()); restore(S.undo.pop());
  }
  function redo() {
    if (!S) return;
    if (S.mode === "region") { ASk.redo(); return; }
    if (S.mode === "lines") { if (ASk.canRedo) { ASk.redo(); return; } endLines(); }
    if (!S.redo.length) return;
    S.undo.push(snapshot()); restore(S.redo.pop());
  }
  function undoState() {
    if (!S) return null;
    if (S.mode === "region") return { undo: ASk.canUndo ? 1 : 0, redo: ASk.canRedo ? 1 : 0 };
    if (S.mode === "lines") return { undo: ASk.canUndo || S.undo.length ? 1 : 0, redo: ASk.canRedo || S.redo.length ? 1 : 0 };
    return { undo: S.undo.length, redo: S.redo.length };
  }

  // ---------------------------------------------------------------- the shell
  function mount() {
    let canvas = $q("#ann-canvas");
    if (!canvas) { canvas = el("canvas"); canvas.id = "ann-canvas"; $q("#fam-wrap").append(canvas); }
    S.canvas = canvas; S.ctx = canvas.getContext("2d");
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
    if (r.width < 10 || r.height < 10) return; // hidden while another document is showing
    S.canvas.width = Math.max(300, Math.round(r.width)); S.canvas.height = Math.max(200, Math.round(r.height));
    render();
  }

  // the one view: a tab, the type selector showing the category, the Project Browser of the family
  function tabsUI() {
    const tab = el("div", "vtab active only");
    tab.append(el("span", "", S.fam.name), el("span", "x", "×"));
    tab.title = `${S.category}: ${S.fam.name}`;
    $q("#fam-vtabs").replaceChildren(tab);
    const ts = $q("#fam-type-select");
    ts.replaceChildren(new Option(`Family: ${S.category}`, ""));
    ts.onchange = null;
    const vb = $q("#fam-viewbar");
    vb.replaceChildren();
    if (S.fam.family_kind === "level_head") {
      const b = el("button", S.flipped ? "on" : "", "Preview Flipped");
      b.title = "Show the head as it draws at the left hand end of a level line";
      b.onclick = () => commands["a-flip"]();
      vb.append(b);
    }
    const f = el("button", "", "Zoom to Fit");
    f.onclick = () => fit();
    vb.append(f);
    tree();
    syncRibbon();
  }

  function tree() {
    const t = $q("#fam-tree");
    t.replaceChildren(...["Views (all)", "Sheets (all)", "Families", "Groups", "Links"].map((n) => { const li = el("li"); li.append(el("span", "", n)); return li; }));
    $q("#fam-browser-title").textContent = `Project Browser - ${S.fam.name}`;
  }

  function syncTitle() {
    if (!S || app.doc !== "annotation") return;
    $q("#doc-title").textContent = `Family: ${S.fam.name}${S.dirty ? " *" : ""} - ${S.category}`;
    renderDocTabs();
    updateUndoButtons();
  }

  function say(text, isError) { if (!S) return; S.msg = text; S.msgErr = !!isError; status(); }
  function status() {
    if (!S || app.doc !== "annotation") return;
    if (ASk.active) { setPrompt(S.msg || ASk.promptText(), S.msgErr); S.msg = ""; S.msgErr = false; syncTitle(); return; }
    const tips = {
      select: "Click a line to work on the lines; a region, label or text to select it. Drag to move; drag the ends of a label's box to change its width. Double-click to edit.",
      label: "Click where the label goes, then choose its parameters in Edit Label.",
      text: "Click where the text goes and type. Click outside the text or press Esc when it is done.",
    };
    setPrompt((S.msg ? S.msg + "   " : "") + (tips[S.tool] || ""), S.msgErr);
    S.msg = ""; S.msgErr = false;
    syncTitle();
  }

  // ---------------------------------------------------------------- ribbon commands and the options bar
  function setTool(t) {
    S.tool = t;
    S.canvas.style.cursor = t === "select" ? "default" : "crosshair";
    toolbar(); status(); render(); ctxRefresh();
  }
  const busy = () => { if (S.mode === "region") { say("Finish or cancel the region's sketch first.", true); return true; } return false; };
  const commands = {
    "a-select": () => { if (S.mode === "region") { ASk.setTool("select"); return; } if (S.mode === "lines") endLines(); setTool("select"); },
    "a-line": () => { if (busy()) return; if (S.mode === "lines") ASk.setTool("line"); else startLines(null, "line"); },
    "a-region": () => { if (!busy()) startRegion(false, null); },
    "a-mask": () => { if (!busy()) startRegion(true, null); },
    "a-label": () => { if (busy()) return; if (S.mode === "lines") endLines(); setTool("label"); },
    "a-text": () => { if (busy()) return; if (S.mode === "lines") endLines(); setTool("text"); },
    "a-del": () => { if (ASk.active) ASk.key({ key: "Delete" }); else removeSel(); },
    "a-load": () => loadIntoProject(false),
    "a-loadclose": () => loadIntoProject(true),
    "a-fit": () => fit(),
    "a-flip": () => {
      if (S.fam.family_kind !== "level_head") { say("Only a level head flips.", true); return; }
      if (ASk.active) { say("Leave the lines or the sketch before previewing the head flipped.", true); return; }
      S.flipped = !S.flipped; tabsUI(); render();
    },
  };
  for (const k of Object.keys(commands)) { const f = commands[k]; commands[k] = (e) => (S ? f(e) : undefined); }
  const TOOL_OF = { "a-select": "select", "a-label": "label", "a-text": "text" };

  function syncRibbon() {
    document.querySelectorAll("#ribbon button.cmd[data-id^='a-']").forEach((b) => {
      const id = b.dataset.id;
      const on = !!S && (id === "a-line" ? S.mode === "lines" : id === "a-region" ? S.mode === "region" && !S.region.masking : id === "a-mask" ? S.mode === "region" && S.region.masking
        : id === "a-flip" ? S.flipped : TOOL_OF[id] ? !S.mode && TOOL_OF[id] === S.tool : false);
      b.classList.toggle("on", on);
    });
  }

  function toolbar() {
    syncRibbon();
    const bar = $q("#fam-options");
    bar.replaceChildren();
    if (S.mode === "lines" || (S.mode === "region" && S.region.index == null)) {
      if (hasPointer()) {
        const s = el("select");
        for (const [v, t] of [["body", "Body"], ["pointer", "Pointer"]]) s.add(new Option(t, v, false, v === (S.mode === "lines" ? S.layer : S.region.pointer ? "pointer" : "body")));
        s.onchange = () => {
          if (S.mode === "lines") { S.layer = s.value; const tool = ASk.tool; endLines(); startLines(null, tool); }
          else S.region.pointer = s.value === "pointer";
        };
        s.title = "Draw on the mark's body, or on the pointer drawn for each view";
        bar.append(el("span", "fe-opt", "Draw on"), s);
      }
      bar.append(...sketchBarKids(ASk));
    } else if (S.mode === "region") bar.append(...sketchBarKids(ASk));
    bar.hidden = !bar.children.length;
  }

  // the line styles a line or an edge can take: the category itself, its subcategories, and invisible lines
  function styleSelect(value, onchange) {
    const s = el("select");
    s.add(new Option(S.category, "", false, value === ""));
    for (const n of S.subs) s.add(new Option(n, n, false, value === n));
    s.add(new Option(INVISIBLE, INVISIBLE, false, value === INVISIBLE));
    if (value === null) { s.add(new Option("<varies>", "*", false, true)); }
    s.onchange = () => { if (s.value !== "*") onchange(s.value); };
    s.onkeydown = (e) => e.stopPropagation();
    return s;
  }

  // What the ribbon shows for the selection or the session: null for none. main.js draws it as a "Modify | ..." tab.
  function context() {
    if (!S || app.doc !== "annotation") return null;
    const c = contextBase();
    if (c) c.key = ASk.active ? `${S.mode}:${ASk.tool}` : S.sel ? `${S.sel.kind}:${S.sel.i}` : "";
    return c;
  }
  function contextBase() {
    const t = ASk.tool;
    const modify = { label: "Modify", items: ASk.MODIFY.map((x) => ({ ...x, onclick: () => ASk.setTool(x.id), on: t === x.id })) };
    const draw = { label: "Draw", items: ASk.DRAW.filter((x) => x.id !== "pick_walls").map((x) => ({ ...x, onclick: () => ASk.setTool(x.id), on: t === x.id })) };
    if (S.mode === "lines") {
      const a = ASk.selectedAttr();
      const node = styleSelect(a ? a.sub : S.sub, (v) => { S.sub = v; if (ASk.selectedAttr()) ASk.patchSelected({ sub: v }); ctxRefresh(); });
      node.title = "The subcategory of new lines, and of the selected lines";
      const placing = ASk.DRAW.some((x) => x.id === t);
      return { auto: true, title: placing ? "Modify | Place Lines" : "Modify | Lines", groups: [modify, draw, { label: "Subcategory", items: [{ node }] }] };
    }
    if (S.mode === "region") {
      const a = ASk.selectedAttr();
      const node = styleSelect(a ? a.sub : S.boundarySub, (v) => { S.boundarySub = v; if (ASk.selectedAttr()) ASk.patchSelected({ sub: v }); ctxRefresh(); });
      node.title = "The line style of new boundary lines, and of the selected ones";
      return { auto: true, title: `Modify | ${ASk.title}`, groups: [
        { label: "Mode", items: [{ label: "Finish Edit Mode", ico: "✓", big: true, onclick: () => ASk.finish() }, { label: "Cancel Edit Mode", ico: "✕", big: true, onclick: () => ASk.cancel() }] },
        modify, draw, { label: "Line Style", items: [{ node }] },
      ] };
    }
    const sel = S.sel;
    if (!sel) return null;
    const auto = S.tool === "select";
    if (sel.kind === "region") {
      const r = S.fam.regions[sel.i];
      if (!r) return null;
      return { auto, title: r.masking ? "Modify | Masking Region" : "Modify | Filled Region", groups: [
        { label: "Mode", items: [{ label: "Edit Boundary", ico: "Ed", big: true, onclick: () => startRegion(r.masking, sel.i) }] },
        { label: "Modify", items: [{ label: "Delete", ico: "Dl", big: true, onclick: () => removeSel() }] },
      ] };
    }
    if (sel.kind === "label") return { auto, title: "Modify | Label", groups: [
      { label: "Label", items: [{ label: "Edit Label", ico: "Ed", big: true, onclick: () => editLabel(sel.i) }] },
      { label: "Modify", items: [{ label: "Delete", ico: "Dl", big: true, onclick: () => removeSel() }] },
    ] };
    if (sel.kind === "text") return { auto, title: "Modify | Text", groups: [
      { label: "Text", items: [{ label: "Edit Text", ico: "Ed", big: true, onclick: () => editText(sel.i) }] },
      { label: "Modify", items: [{ label: "Delete", ico: "Dl", big: true, onclick: () => removeSel() }] },
    ] };
    return null;
  }

  // ---------------------------------------------------------------- properties
  function field(label, node) { const r = el("div", "famed-row"); r.append(el("span", "", label), node); return r; }
  const numIn = (value, onChange, step = 0.5) => {
    const i = el("input"); i.type = "number"; i.step = String(step); i.value = String(Math.round(value * 1000) / 1000);
    i.onchange = () => { const v = parseFloat(i.value); if (Number.isFinite(v)) edit(() => onChange(v), true); };
    return i;
  };
  const textIn = (value, onChange) => { const i = el("input"); i.type = "text"; i.value = value ?? ""; i.onchange = () => edit(() => onChange(i.value)); return i; };
  const selIn = (value, opts, onChange) => { const s = el("select"); for (const [v, l] of opts) s.add(new Option(l, v, false, v === value)); s.onchange = () => edit(() => onChange(s.value), true); return s; };
  const checkIn = (value, onChange) => { const i = el("input"); i.type = "checkbox"; i.checked = !!value; i.onchange = () => edit(() => onChange(i.checked), true); return i; };
  const roIn = (text) => el("span", "fe-ro", text);
  const typeOptions = () => S.fam.region_types.map((t) => [t.name, t.name]);
  const areaOf = (i) => {
    for (const p of [S.drawing && S.drawing.body, S.drawing && S.drawing.pointer]) for (const f of (p && p.fills) || []) if (f.region === i) return f.area_mm2;
    return null;
  };

  function props() {
    const r = $q("#fam-prop-body");
    r.replaceChildren();
    if (S.mode === "lines") {
      const a = ASk.selectedAttr();
      r.append(el("div", "famed-h", a ? `Lines (${a.count} selected)` : "Lines"));
      if (a) {
        const v = el("input"); v.type = "checkbox"; v.checked = a.visible !== false; v.indeterminate = a.visible === null;
        v.onchange = () => ASk.patchSelected({ visible: v.checked });
        r.append(field("Visible", v));
      }
      r.append(el("div", "famed-hint", "Draw lines with the Draw tools on the ribbon; the Modify tools trim, offset, split and move them. Select lines to change their subcategory on the ribbon. Esc twice leaves Line."));
      return;
    }
    if (S.mode === "region") {
      const R = S.region;
      r.append(el("div", "famed-h", R.masking ? "Masking Region" : "Filled Region"));
      if (!R.masking) {
        const s = el("select");
        for (const [v, l] of typeOptions()) s.add(new Option(l, v, false, v === (R.index != null ? S.fam.regions[R.index].region_type : S.newType)));
        s.onchange = () => { if (R.index != null) edit((f) => { f.regions[R.index].region_type = s.value; }, true); else S.newType = s.value; };
        r.append(field("Type", s));
      }
      r.append(el("div", "famed-hint", `${ASk.title}: draw closed loops (a loop inside another is a hole), then Finish Edit Mode. Each line has its own line style: choose it on the ribbon.`));
      return;
    }
    const sel = S.sel;
    const it = sel ? get(sel) : null;
    if (!it) {
      r.append(el("div", "famed-h", "Family"));
      r.append(field("Name", textIn(S.fam.name, (v) => { S.fam.name = v.trim() || S.fam.name; })));
      r.append(field("Category", roIn(S.category)));
      if (S.fam.family_kind === "section_marker") {
        r.append(field("Broken Section Display Style", selIn(S.fam.broken_style || "gapped", [["gapped", "Gapped"], ["continuous", "Continuous"]], (v) => { S.fam.broken_style = v; })));
        r.append(field("Reference Label", textIn(S.fam.reference_label == null ? "SIM" : S.fam.reference_label, (v) => { S.fam.reference_label = v; })));
      }
      r.append(el("div", "famed-hint", `${NOTE[S.fam.family_kind]} Line weights, colours and patterns come from ${S.category} in Object Styles.`));
      return;
    }
    if (sel.kind === "region") {
      r.append(el("div", "famed-h", `${it.masking ? "Masking Region" : "Filled Region"}${it.pointer ? " (pointer)" : ""}`));
      if (!it.masking) {
        r.append(field("Type", selIn(it.region_type, typeOptions(), (v) => { it.region_type = v; })));
        r.append(button("Edit Type...", () => typeDialog(it.region_type)));
      }
      r.append(field("Visible", checkIn(it.visible !== false, (v) => { it.visible = v; })));
      const area = areaOf(sel.i);
      r.append(field("Area (mm²)", roIn(area == null ? "" : (Math.round(area * 100) / 100).toString())));
      r.append(button("Edit Boundary", () => startRegion(it.masking, sel.i)));
    } else {
      const isLabel = sel.kind === "label", key = isLabel ? "label_types" : "text_types", tkey = isLabel ? "label_type" : "text_type";
      r.append(el("div", "famed-h", isLabel ? "Label" : "Text"));
      r.append(field("Type", selIn(it[tkey], S.fam[key].map((t) => [t.name, t.name]), (v) => { it[tkey] = v; })));
      r.append(button("Edit Type...", () => textTypeDialog(key, it[tkey])));
      if (isLabel) {
        r.append(field("Label", button("Edit...", () => editLabel(sel.i))));
        r.append(field("Sample Text", roIn(sampleText(it))));
        r.append(field("Wrap between parameters only", checkIn(it.wrap_params_only, (v) => { it.wrap_params_only = v; })));
      } else r.append(field("Text", button("Edit...", () => editText(sel.i))));
      // a new alignment keeps the box where it is: the anchor moves to the box's other edge or middle
      const align = (k, v) => {
        const L = Ann.layout(textOf(sel)), nx = { left: L.x0, center: L.x0 + L.w / 2, right: L.x0 + L.w }, ny = { top: L.top, middle: L.top - L.h / 2, bottom: L.top - L.h };
        it[k] = v;
        if (k === "halign") it.x = round6(nx[v]); else it.y = round6(ny[v]);
      };
      r.append(field("Vertical Align", selIn(it.valign, [["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]], (v) => align("valign", v))));
      r.append(field("Horizontal Align", selIn(it.halign, [["left", "Left"], ["center", "Center"], ["right", "Right"]], (v) => align("halign", v))));
      r.append(field("Keep Readable", checkIn(it.keep_readable !== false, (v) => { it.keep_readable = v; })));
      r.append(field("Visible", checkIn(it.visible !== false, (v) => { it.visible = v; })));
    }
    r.append(button("Delete", removeSel));
  }

  const LISTS = { region: "regions", label: "labels", text: "texts" };
  const get = (sel) => (LISTS[sel.kind] ? S.fam[LISTS[sel.kind]][sel.i] : null);
  function removeSel() {
    if (!S.sel) return;
    const it = S.sel;
    S.sel = null;
    edit((f) => { f[LISTS[it.kind]].splice(it.i, 1); });
    ctxRefresh();
  }

  // Type Properties of the filled region types: the patterns (drafting patterns, from the Fill Patterns window),
  // their colours, the pattern line weight and masking. Duplicate makes a new type; the name can be changed here.
  function typeDialog(name) {
    const types = clone(S.fam.region_types);
    let cur = types.find((t) => t.name === name) || types[0];
    openModal("Type Properties", (body, foot, close) => {
      const rows = el("div", "famed-rows"), msg = el("div", "modal-msg");
      body.append(rows, msg);
      const hex = (c) => "#" + c.map((x) => x.toString(16).padStart(2, "0")).join("");
      const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
      const colour = (key) => { const i = el("input"); i.type = "color"; i.value = hex(cur[key]); i.onchange = () => { cur[key] = rgb(i.value); }; return i; };
      const patternBtn = (key) => pickerField(cur[key] || "<none>", async () => {
        const v = await pickFillPattern(cur[key] || "", "drafting", "drafting");
        if (v === null) return;
        cur[key] = v || null; draw();
      });
      function draw() {
        rows.replaceChildren();
        const ts = el("select");
        for (const t of types) ts.add(new Option(t.name, t.name, false, t === cur));
        ts.onchange = () => { cur = types.find((t) => t.name === ts.value); draw(); };
        const dup = button("Duplicate...", () => {
          let n = `${cur.name} 2`, k = 3;
          while (types.some((t) => t.name === n)) n = `${cur.name} ${k++}`;
          const t = { ...clone(cur), name: n };
          types.push(t); cur = t; draw();
        });
        const nm = el("input"); nm.type = "text"; nm.value = cur.name;
        nm.onchange = () => { const v = nm.value.trim(); if (!v || types.some((t) => t !== cur && t.name === v)) { msg.textContent = "Each type needs a name of its own."; nm.value = cur.name; return; } msg.textContent = ""; cur._was = cur._was || cur.name; cur.name = v; draw(); };
        const lw = el("select");
        for (let w = 1; w <= 16; w++) lw.add(new Option(String(w), String(w), false, w === cur.line_weight));
        lw.onchange = () => { cur.line_weight = parseInt(lw.value, 10); };
        const mk = el("input"); mk.type = "checkbox"; mk.checked = cur.masking !== false; mk.onchange = () => { cur.masking = mk.checked; };
        rows.append(
          field("Type", ts), field("", dup), field("Name", nm),
          el("div", "famed-h", "Graphics"),
          field("Foreground Fill Pattern", patternBtn("foreground")), field("Foreground Pattern Color", colour("foreground_color")),
          field("Background Fill Pattern", patternBtn("background")), field("Background Pattern Color", colour("background_color")),
          field("Line Weight", lw), field("Masking", mk),
        );
      }
      draw();
      foot.append(button("OK", () => {
        // regions of a renamed type follow it
        const renamed = new Map(types.filter((t) => t._was && t._was !== t.name).map((t) => [t._was, t.name]));
        for (const t of types) delete t._was;
        edit((f) => {
          f.region_types = types;
          for (const r of f.regions) if (renamed.has(r.region_type)) r.region_type = renamed.get(r.region_type);
        });
        if (renamed.has(S.newType)) S.newType = renamed.get(S.newType);
        close();
      }, true), button("Cancel", close));
    });
  }

  // ---------------------------------------------------------------- labels and text
  const typeIn = (list, name) => list.find((t) => t.name === name) || list[0] || { name: "", size_mm: 2.5, color: [0, 0, 0] };
  const styleOf = (t) => ({ ...t, line_mm: S.widths[(t.line_weight || 1) - 1] || 0.18 });
  // a label's lines as the core makes them (annotation.rs symbol_texts), with the sample values in place
  function labelLines(lb) {
    const lines = [];
    let line = [];
    lb.params.forEach((p, k) => {
      const gap = p.break || k + 1 === lb.params.length ? 0 : p.spaces ?? 1;
      line.push(`${p.prefix || ""}${p.sample || p.param}${p.suffix || ""}${" ".repeat(gap)}`);
      if (p.break) { lines.push(line); line = []; }
    });
    lines.push(line);
    return lines.filter((l) => l.length);
  }
  const sampleText = (lb) => labelLines(lb).map((l) => l.join("").trimEnd()).join(" / ");
  function textOf(sel) {
    if (sel.kind === "label") {
      const lb = S.fam.labels[sel.i];
      return { x: lb.x, y: lb.y, halign: lb.halign, valign: lb.valign, width_mm: lb.width_mm, lines: labelLines(lb), wrap: lb.wrap_params_only ? "pieces" : "words", style: styleOf(typeIn(S.fam.label_types, lb.label_type)), label: sel.i, text: null, hidden: lb.visible === false };
    }
    const t = S.fam.texts[sel.i];
    return { x: t.x, y: t.y, halign: t.halign, valign: t.valign, width_mm: t.width_mm, lines: (t.text || "").split("\n").map((l) => [l]), wrap: "words", style: styleOf(typeIn(S.fam.text_types, t.text_type)), label: null, text: sel.i, hidden: t.visible === false };
  }
  const allTexts = () => [...S.fam.labels.map((_, i) => ({ kind: "label", i })), ...S.fam.texts.map((_, i) => ({ kind: "text", i }))];
  // the box of a label or text on screen (a flip mirrors it), and its edges in the family
  function boxOf(sel) {
    const L = Ann.layout(textOf(sel));
    const a = toScreen(S.flipped ? -(L.x0 + L.w) : L.x0, L.top), b = toScreen(S.flipped ? -L.x0 : L.x0 + L.w, L.top - L.h);
    return { L, left: Math.min(a.x, b.x), right: Math.max(a.x, b.x), top: a.y, bottom: b.y };
  }
  // the width grips of the selected label or text: its box's two ends
  function widthGrips() {
    if (!S.sel || !["label", "text"].includes(S.sel.kind) || !get(S.sel)) return [];
    const B = boxOf(S.sel), y = (B.top + B.bottom) / 2;
    // which end of the box in the family each grip moves (a flip swaps them on screen)
    return [{ x: B.left, y, edge: S.flipped ? 1 : 0 }, { x: B.right, y, edge: S.flipped ? 0 : 1 }];
  }
  // drag one end of the box: the other end stays, and the anchor keeps to the alignment
  function setWidth(it, L, edge, px) {
    const e0 = edge === 0 ? Math.min(px, L.x0 + L.w - 0.5) : L.x0, e1 = edge === 1 ? Math.max(px, L.x0 + 0.5) : L.x0 + L.w;
    it.width_mm = round6(e1 - e0);
    it.x = round6(it.halign === "right" ? e1 : it.halign === "center" ? (e0 + e1) / 2 : e0);
  }

  // Edit Label: the category's parameters on the left, the label's on the right with their spaces, prefix, sample
  // value, suffix and break; a new label (`at`) is made with OK
  function editLabel(index, at) {
    const lb = index != null ? S.fam.labels[index] : null;
    const rows = lb ? clone(lb.params) : [];
    let wrapOnly = lb ? !!lb.wrap_params_only : false, cur = rows.length ? 0 : -1, cat = S.params[0];
    openModal("Edit Label", (body, foot, close) => {
      const catList = el("div", "el-list"), table = el("table", "el-table"), ok = button("OK", () => {
        if (!rows.length) return;
        for (const r of rows) r.spaces = Math.max(0, Math.min(20, parseInt(r.spaces, 10) || 0));
        close();
        if (lb) { edit((f) => { f.labels[index].params = rows; f.labels[index].wrap_params_only = wrapOnly; }); return; }
        const type = (S.fam.label_types[0] || {}).name || "";
        const fresh = { x: at.x, y: at.y, halign: "left", valign: "middle", label_type: type, width_mm: 1, params: rows, wrap_params_only: wrapOnly, keep_readable: true, visible: true };
        // the box starts as wide as the sample text, and a little more
        const L = Ann.layout({ x: 0, y: 0, halign: "left", valign: "middle", width_mm: 0, lines: labelLines(fresh), wrap: "words", style: styleOf(typeIn(S.fam.label_types, type)) });
        fresh.width_mm = Math.max(3, Math.round((L.w + 1) * 2) / 2);
        edit((f) => { f.labels.push(fresh); });
        // the tool stays on for the next label; the new one is selected
        S.sel = { kind: "label", i: S.fam.labels.length - 1 };
        props(); status(); ctxRefresh(); render();
      }, true);
      const add = () => { if (!cat || rows.some((r) => r.param === cat)) return; rows.push({ param: cat, spaces: 1, prefix: "", suffix: "", sample: cat, break: false }); cur = rows.length - 1; draw(); };
      const move = (d) => { const j = cur + d; if (cur < 0 || j < 0 || j >= rows.length) return; [rows[cur], rows[j]] = [rows[j], rows[cur]]; cur = j; draw(); };
      function draw() {
        catList.replaceChildren(...S.params.map((p) => {
          const b = el("button", "mat-item" + (p === cat ? " active" : "") + (rows.some((r) => r.param === p) ? " used" : ""), p);
          b.onclick = () => { cat = p; draw(); };
          b.ondblclick = () => { cat = p; add(); };
          return b;
        }));
        const head = el("tr");
        for (const h of ["", "Parameter Name", "Spaces", "Prefix", "Sample Value", "Suffix", "Break"]) head.append(el("th", "", h));
        table.replaceChildren(head);
        rows.forEach((r, k) => {
          const tr = el("tr", k === cur ? "active" : "");
          tr.onclick = (e) => { if (cur !== k) { cur = k; if (!["INPUT"].includes(e.target.tagName)) draw(); else for (const x of table.querySelectorAll("tr")) x.classList.toggle("active", x === tr); } };
          const inp = (key, type) => {
            const i = el("input"); i.type = type || "text";
            if (type === "checkbox") { i.checked = !!r[key]; i.onchange = () => { r[key] = i.checked; draw(); }; }
            else { i.value = r[key] ?? ""; i.oninput = () => { r[key] = type === "number" ? i.value : i.value; }; }
            if (type === "number") { i.min = "0"; i.max = "20"; i.step = "1"; i.disabled = !!r.break; }
            i.onkeydown = (e) => e.stopPropagation();
            const td = el("td"); td.append(i); return td;
          };
          tr.append(el("td", "", String(k + 1)), el("td", "", r.param), inp("spaces", "number"), inp("prefix"), inp("sample"), inp("suffix"), inp("break", "checkbox"));
          table.append(tr);
        });
        ok.disabled = !rows.length;
      }
      const left = el("div", "el-col"), mid = el("div", "el-mid"), right = el("div", "el-col el-wide");
      left.append(el("div", "famed-h", "Category Parameters"), el("div", "famed-hint", `Select parameters to add to the label. ${S.category}.`), catList);
      const addB = button("\u2192", add), rem = button("\u2190", () => { if (cur < 0) return; rows.splice(cur, 1); cur = Math.min(cur, rows.length - 1); draw(); });
      addB.title = "Add parameter(s) to label"; rem.title = "Remove parameter from label";
      mid.append(addB, rem);
      const wrap = el("label", "el-wrap"), wc = el("input"); wc.type = "checkbox"; wc.checked = wrapOnly; wc.onchange = () => { wrapOnly = wc.checked; };
      wrap.append(wc, " Wrap between parameters only");
      const up = button("Move Up", () => move(-1)), down = button("Move Down", () => move(1));
      const order = el("div", "el-row");
      order.append(up, down);
      right.append(el("div", "famed-h", "Label Parameters"), table, order, wrap);
      body.append(el("div", "el-grid"));
      body.lastChild.append(left, mid, right);
      draw();
      foot.append(ok, button("Cancel", close));
    }, { wide: true });
  }

  // Text in place: a box over the canvas to type in. Click outside it or press Esc to finish; empty text goes.
  function editText(index, at) {
    const t = index != null ? S.fam.texts[index] : null;
    const type = typeIn(S.fam.text_types, t ? t.text_type : (S.fam.text_types[0] || {}).name);
    const x = t ? t.x : at.x, y = t ? t.y : at.y;
    const p = toScreen(S.flipped ? -x : x, y), r = S.canvas.getBoundingClientRect(), px = Math.max(10, type.size_mm * S.view.k);
    const ta = el("textarea", "ann-text-edit");
    ta.value = t ? t.text : "";
    ta.style.cssText = `position:fixed;left:${r.left + p.x}px;top:${r.top + p.y - (t && t.valign === "middle" ? px * 0.6 : 0)}px;min-width:120px;min-height:${px * 1.4}px;font:${type.bold ? "600 " : ""}${px}px "${type.font}", system-ui, sans-serif;z-index:60;`;
    document.body.append(ta);
    S.textEdit = ta;
    let done = false;
    const finish = () => {
      if (done) return;
      done = true; S.textEdit = null; ta.remove();
      const text = ta.value.replace(/\s+$/, "");
      if (t) {
        if (!text) { S.sel = { kind: "text", i: index }; removeSel(); }
        else if (text !== t.text) edit((f) => { f.texts[index].text = text; });
      } else if (text) {
        edit((f) => { f.texts.push({ x, y, text, text_type: type.name, halign: "left", valign: "top", width_mm: 0, keep_readable: true, visible: true }); });
        S.sel = { kind: "text", i: S.fam.texts.length - 1 };
      }
      props(); status(); ctxRefresh(); render();
    };
    ta.onkeydown = (e) => { e.stopPropagation(); if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) { e.preventDefault(); finish(); } };
    ta.onblur = finish;
    setTimeout(() => ta.focus(), 0);
  }

  // Type Properties of the label or text types (`key`: "label_types" or "text_types"). Labels or text of a renamed
  // type follow it.
  function textTypeDialog(key, name) {
    const types = clone(S.fam[key]);
    let cur = types.find((t) => t.name === name) || types[0];
    openModal("Type Properties", (body, foot, close) => {
      const rows = el("div", "famed-rows"), msg = el("div", "modal-msg");
      body.append(rows, msg);
      const hex = (c) => "#" + c.map((x) => x.toString(16).padStart(2, "0")).join("");
      const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
      const num = (k, step, min, max) => { const i = el("input"); i.type = "number"; i.step = String(step); i.min = String(min); i.max = String(max); i.value = String(cur[k]); i.onchange = () => { const v = parseFloat(i.value); if (Number.isFinite(v)) cur[k] = v; }; i.onkeydown = (e) => e.stopPropagation(); return i; };
      const chk = (k) => { const i = el("input"); i.type = "checkbox"; i.checked = !!cur[k]; i.onchange = () => { cur[k] = i.checked; }; return i; };
      function draw() {
        rows.replaceChildren();
        const ts = el("select");
        for (const t of types) ts.add(new Option(t.name, t.name, false, t === cur));
        ts.onchange = () => { cur = types.find((t) => t.name === ts.value); draw(); };
        const dup = button("Duplicate...", () => {
          let n = `${cur.name} 2`, k = 3;
          while (types.some((t) => t.name === n)) n = `${cur.name} ${k++}`;
          const t = { ...clone(cur), name: n };
          delete t._was;
          types.push(t); cur = t; draw();
        });
        const nm = el("input"); nm.type = "text"; nm.value = cur.name;
        nm.onkeydown = (e) => e.stopPropagation();
        nm.onchange = () => { const v = nm.value.trim(); if (!v || types.some((t) => t !== cur && t.name === v)) { msg.textContent = "Each type needs a name of its own."; nm.value = cur.name; return; } msg.textContent = ""; cur._was = cur._was || cur.name; cur.name = v; draw(); };
        const col = el("input"); col.type = "color"; col.value = hex(cur.color || [0, 0, 0]); col.onchange = () => { cur.color = rgb(col.value); };
        const lw = el("select");
        for (let w = 1; w <= 16; w++) lw.add(new Option(String(w), String(w), false, w === cur.line_weight));
        lw.onchange = () => { cur.line_weight = parseInt(lw.value, 10); };
        const bg = el("select");
        for (const [v, l] of [["opaque", "Opaque"], ["transparent", "Transparent"]]) bg.add(new Option(l, v, false, (v === "opaque") === !!cur.opaque));
        bg.onchange = () => { cur.opaque = bg.value === "opaque"; };
        const font = el("select");
        for (const f of FONTS.includes(cur.font) ? FONTS : [cur.font, ...FONTS]) font.add(new Option(f, f, false, f === cur.font));
        font.onchange = () => { cur.font = font.value; };
        rows.append(
          field("Type", ts), field("", dup), field("Name", nm),
          el("div", "famed-h", "Graphics"),
          field("Color", col), field("Line Weight", lw), field("Background", bg), field("Show Border", chk("show_border")), field("Leader/Border Offset (mm)", num("border_offset_mm", 0.1, 0, 20)),
          el("div", "famed-h", "Text"),
          field("Text Font", font), field("Text Size (mm)", num("size_mm", 0.1, 0.2, 20)), field("Tab Size (mm)", num("tab_size_mm", 0.5, 0.5, 100)),
          field("Bold", chk("bold")), field("Italic", chk("italic")), field("Underline", chk("underline")), field("Width Factor", num("width_factor", 0.05, 0.1, 10)),
        );
      }
      draw();
      foot.append(button("OK", () => {
        const renamed = new Map(types.filter((t) => t._was && t._was !== t.name).map((t) => [t._was, t.name]));
        for (const t of types) delete t._was;
        const list = key === "label_types" ? "labels" : "texts", tk = key === "label_types" ? "label_type" : "text_type";
        edit((f) => {
          f[key] = types;
          for (const x of f[list]) if (renamed.has(x[tk])) x[tk] = renamed.get(x[tk]);
        });
        close();
      }, true), button("Cancel", close));
    });
  }

  // ---------------------------------------------------------------- the sketch session: Line, Filled and Masking Region
  const ASk = makeSketch();
  // the curves of the drawing the session is not holding, as segments in sketch units (for snaps, Pick Lines, Align)
  function inSession(layer, x) {
    if (S.mode === "lines") return x.line != null && layer === S.layer;
    if (S.mode === "region") return S.region.index != null && x.region === S.region.index && layer === (S.region.pointer ? "pointer" : "body");
    return false;
  }
  function otherStrokes() {
    const out = [];
    for (const layer of ["body", "pointer"]) for (const s of ((S.drawing && S.drawing[layer]) || {}).strokes || []) if (!inSession(layer, s)) out.push(s);
    return out;
  }
  const REF = 1000 * U;
  function skEdges(sx, sy) {
    const out = [], tol = 8;
    const near = (a, b) => {
      const A = toScreen(a[0], a[1]), B = toScreen(b[0], b[1]), l2 = (B.x - A.x) ** 2 + (B.y - A.y) ** 2 || 1;
      const t = Math.max(0, Math.min(1, ((sx - A.x) * (B.x - A.x) + (sy - A.y) * (B.y - A.y)) / l2));
      return Math.hypot(A.x + t * (B.x - A.x) - sx, A.y + t * (B.y - A.y) - sy) <= tol;
    };
    for (const s of otherStrokes()) {
      const n = s.pts.length;
      for (let i = 0; i + 1 < n + (s.closed ? 1 : 0); i++) {
        const a = s.pts[i], b = s.pts[(i + 1) % n];
        if (near(a, b)) out.push({ a: { x: a[0] * U, y: a[1] * U }, b: { x: b[0] * U, y: b[1] * U } });
      }
    }
    // the reference lines
    if (near([-1000, 0], [1000, 0])) out.push({ a: { x: -REF, y: 0 }, b: { x: REF, y: 0 } });
    if (near([0, -1000], [0, 1000])) out.push({ a: { x: 0, y: -REF }, b: { x: 0, y: REF } });
    return out;
  }
  // the snap index: the reference lines (in pieces, so every part of them is in reach; no ends or middles of their
  // own) and the rest of the drawing
  function skSnapIndex() {
    const key = `${S.tick}|${S.mode}|${S.layer}|${S.region && S.region.index}`;
    if (Snap.owner === "annotation" && S.snapKey === key) return;
    const segs = [], pts = [];
    for (let v = -REF; v < REF; v += 2000) { segs.push([v, 0, v + 2000, 0, 2]); segs.push([0, v, 0, v + 2000, 2]); }
    for (const s of otherStrokes()) {
      const q = s.pts.map(([x, y]) => [x * U, y * U]), straight = q.length === 2 && !s.closed;
      for (let i = 0; i + 1 < q.length; i++) segs.push(straight ? [...q[i], ...q[i + 1], 4] : [...q[i], ...q[i + 1], 2]);
      if (!straight && !s.closed) pts.push({ x: q[0][0], y: q[0][1], kind: "endpoint" }, { x: q[q.length - 1][0], y: q[q.length - 1][1], kind: "endpoint" });
    }
    Snap.build(segs, "annotation");
    S.snapPts = pts; S.snapKey = key;
  }
  function skSnap(sx, sy) {
    skSnapIndex();
    const w = toPaper(sx, sy);
    return Snap.resolve({ x: w.x * U, y: w.y * U }, { zoom: S.view.k / U, last: ASk.last(), extra: ASk.segs(), points: [...ASk.points(), ...(S.snapPts || [])], track: ASk.trackPoints(), cycle: S.snapCycle || 0, shift: !!S.shift, gridStep: 0, lengths: LENGTHS });
  }
  ASk.attach({
    get ctx() { return S && S.ctx; },
    get canvas() { return S && S.canvas; },
    unit: U,
    size: () => ({ w: S.canvas.width, h: S.canvas.height }),
    toScreen: (p) => toScreen(p.x / U, p.y / U),
    toWorld: (sx, sy) => { const q = toPaper(sx, sy); return { x: q.x * U, y: q.y * U }; },
    label: (text, at, color) => {
      const ctx = S.ctx;
      ctx.font = "11px system-ui, sans-serif";
      const m = ctx.measureText(text);
      ctx.fillStyle = css("--panel"); ctx.fillRect(at.x - m.width / 2 - 3, at.y - 8, m.width + 6, 16);
      ctx.fillStyle = color; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillText(text, at.x, at.y);
    },
    css, render: () => render(),
    scale: () => S.view.k / U,
    wallAt: () => null, wallById: () => null, wallChain: (id) => [id],
    edgeAt: (sx, sy) => skEdges(sx, sy)[0] || null,
    edgesNear: (sx, sy) => skEdges(sx, sy),
    cycleSnap: (d) => { S.snapCycle = (S.snapCycle || 0) + d; if (S.mouse) { S.skHover = skSnap(S.mouse.x, S.mouse.y); ASk.move(S.skHover, S.mouse.x, S.mouse.y); } render(); },
  });
  ASk.callbacks.prompt = (t, err) => say(t, err);
  ASk.callbacks.change = () => {
    if (!S) return;
    if (S.mode === "lines") writeLines();
    toolbar(); props(); status(); ctxRefresh(); render();
  };
  ASk.callbacks.finish = (out) => finishRegion(out);
  ASk.callbacks.cancel = () => { endSession(); say("Sketch cancelled."); };
  ASk.callbacks.slope = () => {};

  // ---- Line: the session holds the lines of one layer (the body, or a mark's pointer) and every change goes
  // straight into the family; the document's undo takes the whole visit back in one step
  async function startLines(index, tool) {
    if (!S) return;
    if (S.mode === "region") return;
    S.sel = null; S.flipped = false;
    if (index != null) S.layer = S.fam.lines[index].pointer ? "pointer" : "body";
    const ptr = S.layer === "pointer";
    const mine = S.fam.lines.map((l, i) => ({ l, i })).filter((x) => !!x.l.pointer === ptr);
    let r;
    try { r = await Backend.call("sketch_describe", { curves: mine.map((x) => toSketch(x.l.curve)) }); } catch (e) { say(String(e.message || e), true); return; }
    S.mode = "lines";
    S.lines = { before: snapshot(), pushed: false, last: null, starting: true };
    ASk.begin({
      kind: "annotation", title: "Lines", tool: tool || "select", fade: false, opts: PAPER_OPTS,
      ink: (c) => (c.attr && (c.attr.sub === INVISIBLE || c.attr.visible === false) ? "#9aa0a8" : ptr ? POINTER_INK : css("--text")),
      curves: r.curves.map((d, k) => ({ ...d, attr: { sub: mine[k].l.subcategory || "", visible: mine[k].l.visible !== false } })),
      attr: (extra) => ({ sub: S.sub, visible: true, ...(extra || {}) }),
    });
    const st = ASk.state();
    if (index != null) { const k = mine.findIndex((x) => x.i === index); if (k >= 0) st.sel = new Set([st.curves[k].uid]); }
    S.lines.last = JSON.stringify(sessionLines());
    S.lines.starting = false;
    S.canvas.style.cursor = "crosshair";
    tabsUI(); toolbar(); props(); status(); ctxRefresh(); render();
  }
  const sessionLines = () => ASk.state().curves.map((c) => ({ curve: toPaperCurve(c.curve), subcategory: c.attr.sub || "", visible: c.attr.visible !== false, pointer: S.layer === "pointer" }));
  function writeLines() {
    const L = S.lines;
    if (!L || L.starting) return;
    const mine = sessionLines(), json = JSON.stringify(mine);
    if (json === L.last) return; // a change of tool or selection
    L.last = json;
    if (!L.pushed) { S.undo.push(L.before); S.redo = []; L.pushed = true; }
    const ptr = S.layer === "pointer";
    S.fam.lines = [...S.fam.lines.filter((l) => !!l.pointer !== ptr), ...mine];
    S.dirty = true;
    refreshDrawing();
  }
  function endLines() { if (S && S.mode === "lines") endSession(); }

  // ---- Filled Region and Masking Region: a sketch with Finish and Cancel, new or the boundary of region `index`
  async function startRegion(masking, index) {
    if (!S || S.mode === "region") return;
    if (S.mode === "lines") endLines();
    const r = index != null ? S.fam.regions[index] : null;
    let curves = [];
    if (r) {
      const edges = r.loops.flat();
      try { curves = (await Backend.call("sketch_describe", { curves: edges.map((e) => toSketch(e.curve)) })).curves.map((d, k) => ({ ...d, attr: { sub: edges[k].subcategory || "" } })); }
      catch (e) { say(String(e.message || e), true); return; }
    }
    S.sel = null; S.flipped = false;
    S.mode = "region";
    S.region = { index, masking, pointer: r ? !!r.pointer : S.layer === "pointer" };
    // a masking region's edges start invisible, so it hides what is behind it without an outline
    S.boundarySub = masking ? INVISIBLE : "";
    const noun = masking ? "Masking Region" : "Filled Region";
    ASk.begin({ kind: "annotation", title: r ? `Edit ${noun} Boundary` : `Create ${noun} Boundary`, tool: r ? "select" : "line", opts: PAPER_OPTS, curves, attr: (extra) => ({ sub: S.boundarySub, ...(extra || {}) }) });
    S.canvas.style.cursor = "crosshair";
    tabsUI(); toolbar(); props(); status(); ctxRefresh(); render();
  }
  async function finishRegion(out) {
    const R = S && S.region;
    if (!R) return false;
    const edges = (out.edges || []).map((e) => ({ curve: toPaperCurve(e.curve), subcategory: e.sub || "" }));
    let r;
    try { r = await Backend.call("annotation_region_loops", { edges }); } catch (e) { say(String(e.message || e), true); return false; }
    if (!r.ok) { say(r.error.message, true); return false; }
    let i = R.index;
    endSession();
    edit((f) => {
      if (i != null) f.regions[i].loops = r.loops;
      else { f.regions.push({ loops: r.loops, masking: R.masking, region_type: R.masking ? "" : S.newType || (f.region_types[0] || {}).name, visible: true, pointer: R.pointer }); i = f.regions.length - 1; }
    });
    S.sel = { kind: "region", i };
    say(`${R.masking ? "Masking" : "Filled"} region ${R.index != null ? "changed" : "made"}.`);
    props(); ctxRefresh(); render();
    return true;
  }

  function endSession() {
    if (ASk.active) ASk.end();
    if (!S) return;
    S.mode = null; S.lines = null; S.region = null; S.skHover = null; S.drag = null;
    S.tool = "select"; S.canvas.style.cursor = "default";
    toolbar(); props(); status(); ctxRefresh(); render(); updateUndoButtons();
  }

  // ---------------------------------------------------------------- drawing
  const toScreen = (x, y) => ({ x: S.view.ox + x * S.view.k, y: S.view.oy - y * S.view.k });
  const toPaper = (sx, sy) => ({ x: (sx - S.view.ox) / S.view.k, y: (S.view.oy - sy) / S.view.k });

  // frame the drawing, with room round it
  function fit() {
    if (!S || !S.canvas) return;
    const W = S.canvas.width || 800, H = S.canvas.height || 600;
    const e = Ann.extent({ body: S.drawing && S.drawing.body, pointer: hasPointer() && S.drawing ? S.drawing.pointer : null, labels: allTexts().map(textOf) });
    const minX = Math.min(e.minX, -8), maxX = Math.max(e.maxX, 8), minY = Math.min(e.minY, -8), maxY = Math.max(e.maxY, 8);
    S.view.k = Math.max(4, Math.min(60, 0.7 * Math.min(W / (maxX - minX), H / (maxY - minY))));
    S.view.ox = W / 2 - ((minX + maxX) / 2) * S.view.k;
    S.view.oy = H / 2 + ((minY + maxY) / 2) * S.view.k;
    render();
  }

  function render() {
    if (!S || !S.canvas || app.doc !== "annotation") return;
    const ctx = S.ctx, W = S.canvas.width, H = S.canvas.height, fam = S.fam;
    const text = css("--text"), accent = css("--accent"), bg = css("--view-bg"), muted = css("--muted");
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = bg; ctx.fillRect(0, 0, W, H);
    // a paper grid every 5 mm
    ctx.strokeStyle = css("--line"); ctx.lineWidth = 0.5; ctx.globalAlpha = 0.5;
    ctx.beginPath();
    for (let x = Math.ceil(-S.view.ox / S.view.k / 5) * 5; toScreen(x, 0).x < W; x += 5) { const p = toScreen(x, 0); ctx.moveTo(p.x, 0); ctx.lineTo(p.x, H); }
    for (let y = Math.ceil((S.view.oy - H) / S.view.k / 5) * 5; toScreen(0, y).y > 0; y += 5) { const p = toScreen(0, y); ctx.moveTo(0, p.y); ctx.lineTo(W, p.y); }
    ctx.stroke(); ctx.globalAlpha = 1;
    // the reference lines
    const o = toScreen(0, 0);
    ctx.save(); ctx.strokeStyle = "#3b7fd1"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
    ctx.beginPath(); ctx.moveTo(0, o.y); ctx.lineTo(W, o.y); ctx.moveTo(o.x, 0); ctx.lineTo(o.x, H); ctx.stroke(); ctx.restore();
    ctx.fillStyle = "#3b7fd1"; ctx.font = "11px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "bottom";
    ctx.fillText(fam.family_kind === "level_head" ? "Reference line (level line)" : "Reference line", 6, o.y - 3);
    ctx.textBaseline = "top";
    ctx.fillText("Flip line", o.x + 4, 4);
    if (fam.family_kind === "level_head") {
      ctx.save(); ctx.strokeStyle = muted; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(0, o.y); ctx.lineTo(o.x, o.y); ctx.stroke(); ctx.restore();
    }
    const base = { x: o.x, y: o.y, k: S.view.k, ...(S.flipped ? { flip: true } : {}) };
    // the drawing: what a project shows, and greyed what it leaves out; what the session holds it draws itself
    const d = S.drawing || {};
    const opt = (layer) => ({ pats: S.pats, hidden: true, only: (x) => !inSession(layer, x) });
    Ann.paths(ctx, d.body, base, null, bg, opt("body"));
    if (hasPointer()) Ann.paths(ctx, d.pointer, base, POINTER_INK, bg, opt("pointer"));
    if (S.sel && S.sel.kind === "region") {
      const r = fam.regions[S.sel.i];
      if (r) Ann.paths(ctx, r.pointer ? d.pointer : d.body, base, accent, null, { pats: S.pats, hidden: true, only: (x) => x.region === S.sel.i });
    }
    // labels with their sample values and text; a label's box shows dotted, the selected one's with its width grips
    for (const sel of allTexts()) {
      const hot = S.sel && S.sel.kind === sel.kind && S.sel.i === sel.i;
      Ann.labels(ctx, [textOf(sel)], base, hot ? accent : null, { hidden: true, bg });
      if (sel.kind !== "label" && !hot) continue;
      const B = boxOf(sel), it = get(sel);
      ctx.save(); ctx.strokeStyle = hot ? accent : muted; ctx.lineWidth = hot ? 1.5 : 0.7; ctx.setLineDash([2, 3]); ctx.strokeRect(B.left, B.top, B.right - B.left, B.bottom - B.top); ctx.restore();
      const anchor = toScreen(S.flipped ? -it.x : it.x, it.y);
      ctx.fillStyle = hot ? accent : muted; ctx.fillRect(anchor.x - 2, anchor.y - 2, 4, 4);
    }
    for (const g of widthGrips()) { ctx.fillStyle = "#ffffff"; ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.arc(g.x, g.y, 4.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); }
    // the session's curves, grips, previews and dimensions, and the snap under the cursor
    if (ASk.active) {
      ASk.draw(ctx);
      const h = S.skHover;
      if (h && ASk.wantsSnap() && !["free", "length"].includes(h.kind)) {
        const p = toScreen(h.x / U, h.y / U);
        Snap.drawGuides(ctx, h, (q) => toScreen(q.x / U, q.y / U));
        Snap.glyph(ctx, h.kind, p.x, p.y);
        if (h.label) { ctx.font = "11px system-ui, sans-serif"; ctx.fillStyle = "#d4572f"; ctx.textAlign = "left"; ctx.textBaseline = "top"; ctx.fillText(h.label, p.x + 10, p.y + 8); }
      }
      if (S.drag && S.drag.band && S.drag.cur) {
        const d0 = S.drag, x0 = Math.min(d0.x, d0.cur.x), y0 = Math.min(d0.y, d0.cur.y);
        ctx.save(); ctx.strokeStyle = accent; ctx.setLineDash(d0.cur.x >= d0.x ? [] : [5, 4]); ctx.lineWidth = 1;
        ctx.strokeRect(x0, y0, Math.abs(d0.cur.x - d0.x), Math.abs(d0.cur.y - d0.y)); ctx.restore();
      }
    }
  }

  // ---------------------------------------------------------------- hit testing and the mouse
  const distSeg = (p, a, b) => {
    const dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2)) : 0;
    return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
  };
  // what is under the cursor: { kind: "line" | "region" | "label" | "text", i }
  function hit(sx, sy) {
    const P = { x: sx, y: sy };
    const sc = ([x, y]) => toScreen(S.flipped ? -x : x, y);
    let best = null;
    const consider = (it, dd) => { if (dd <= 7 && (!best || dd < best.d)) best = { it, d: dd }; };
    for (const sel of allTexts()) {
      const B = boxOf(sel);
      if (sx >= B.left - 3 && sx <= B.right + 3 && sy >= B.top - 3 && sy <= B.bottom + 3) consider(sel, 2);
    }
    const d = S.drawing || {};
    for (const layer of hasPointer() ? ["body", "pointer"] : ["body"]) {
      const p = d[layer] || {};
      for (const s of p.strokes || []) {
        const q = s.pts.map(sc);
        let dd = Infinity;
        for (let i = 0; i + 1 < q.length; i++) dd = Math.min(dd, distSeg(P, q[i], q[i + 1]));
        if (s.closed && q.length > 2) dd = Math.min(dd, distSeg(P, q[q.length - 1], q[0]));
        consider(s.line != null ? { kind: "line", i: s.line } : { kind: "region", i: s.region }, dd);
      }
      for (const f of p.fills || []) {
        let inside = false;
        for (const ring of f.rings) {
          const q = ring.map(sc);
          for (let i = 0, j = q.length - 1; i < q.length; j = i++) {
            if ((q[i].y > sy) !== (q[j].y > sy) && sx < ((q[j].x - q[i].x) * (sy - q[i].y)) / (q[j].y - q[i].y) + q[i].x) inside = !inside;
          }
        }
        if (inside) consider({ kind: "region", i: f.region }, 4);
      }
    }
    return best ? best.it : null;
  }
  // whether a click lands on a curve the session holds
  function sessionCurveAt(sx, sy) {
    const st = ASk.state();
    if (!st) return false;
    const P = { x: sx, y: sy };
    return st.curves.some((c) => {
      const q = c.poly.map(([x, y]) => toScreen(x / U, y / U));
      for (let i = 0; i + 1 < q.length; i++) if (distSeg(P, q[i], q[i + 1]) <= 8) return true;
      return false;
    });
  }

  const mouseAt = (e) => { const r = S.canvas.getBoundingClientRect(); return { x: ((e.clientX - r.left) * S.canvas.width) / r.width, y: ((e.clientY - r.top) * S.canvas.height) / r.height }; };
  const paperAt = (m) => { const p = toPaper(m.x, m.y); return { x: snapMM(S.flipped ? -p.x : p.x), y: snapMM(p.y) }; };

  function onDown(e) {
    if (!S) return;
    S.mouse = mouseAt(e); S.shift = e.shiftKey;
    if (e.button === 1 || (e.button === 2 && !ASk.active)) { S.pan = { x: S.mouse.x, y: S.mouse.y, ox: S.view.ox, oy: S.view.oy, button: e.button }; S.canvas.setPointerCapture(e.pointerId); return; }
    if (ASk.active) {
      if (e.button === 2) { ASk.rightClick(); render(); return; }
      if (e.button !== 0) return;
      S.canvas.setPointerCapture(e.pointerId);
      if (ASk.gripDown(S.mouse.x, S.mouse.y)) { S.drag = { skGrip: true }; return; }
      S.drag = { sk: true, x: S.mouse.x, y: S.mouse.y };
      return;
    }
    if (e.button !== 0) return;
    // a click outside text being typed finishes it
    if (S.textEdit) { S.textEdit.blur(); return; }
    const q = paperAt(S.mouse);
    if (S.tool === "label") { editLabel(null, q); return; }
    if (S.tool === "text") { editText(null, q); return; }
    // the ends of the selected label's or text's box change its width
    const g = widthGrips().find((w) => Math.hypot(w.x - S.mouse.x, w.y - S.mouse.y) <= 7);
    if (g) { S.drag = { width: g.edge, L: boxOf(S.sel).L, started: false }; S.canvas.setPointerCapture(e.pointerId); return; }
    const h = hit(S.mouse.x, S.mouse.y);
    if (h && h.kind === "line") { startLines(h.i, "select"); return; }
    S.sel = h;
    if (h) { S.drag = { last: q, started: false }; S.canvas.setPointerCapture(e.pointerId); }
    props(); render(); ctxRefresh();
  }

  function onMove(e) {
    if (!S) return;
    S.mouse = mouseAt(e); S.shift = e.shiftKey;
    const m = S.mouse;
    if (S.pan) { S.view.ox = S.pan.ox + m.x - S.pan.x; S.view.oy = S.pan.oy + m.y - S.pan.y; render(); return; }
    if (ASk.active) {
      const d = S.drag;
      if (d && d.skGrip) { S.skHover = skSnap(m.x, m.y); ASk.gripMove(m.x, m.y, S.skHover); return; }
      if (d && d.sk && (d.band || (Math.hypot(m.x - d.x, m.y - d.y) > 5 && ASk.canBand()))) { d.band = true; d.cur = m; render(); return; }
      if (!S.lastSkMouse || Math.hypot(m.x - S.lastSkMouse.x, m.y - S.lastSkMouse.y) > 3) S.snapCycle = 0;
      S.lastSkMouse = m;
      S.skHover = skSnap(m.x, m.y);
      ASk.move(S.skHover, m.x, m.y);
      return;
    }
    if (S.drag && S.sel && S.drag.width !== undefined) {
      const p = toPaper(m.x, m.y), px = Math.round((S.flipped ? -p.x : p.x) * 10) / 10;
      if (!S.drag.started) { S.undo.push(snapshot()); S.redo = []; S.drag.started = true; S.dirty = true; }
      setWidth(get(S.sel), S.drag.L, S.drag.width, px);
      render();
      return;
    }
    if (S.drag && S.sel) {
      const q = paperAt(m), dx = q.x - S.drag.last.x, dy = q.y - S.drag.last.y;
      if (dx || dy) {
        // the whole drag is one undo step
        if (!S.drag.started) { S.undo.push(snapshot()); S.redo = []; S.drag.started = true; S.dirty = true; }
        const it = get(S.sel);
        if (S.sel.kind === "label" || S.sel.kind === "text") { it.x = round6(it.x + dx); it.y = round6(it.y + dy); }
        else for (const l of it.loops) for (const ed of l) ed.curve = moveCurve(ed.curve, dx, dy);
        S.drag.last = q;
        if (S.sel.kind === "region") refreshDrawing();
      }
    }
    render();
  }

  async function onUp(e) {
    if (!S) return;
    const m = mouseAt(e);
    if (S.pan) {
      const p = S.pan; S.pan = null;
      if (p.button === 2 && Math.hypot(m.x - p.x, m.y - p.y) < 4 && !ASk.active && S.tool !== "select") setTool("select");
      return;
    }
    const d = S.drag;
    if (ASk.active) {
      S.drag = null;
      if (!d) return;
      if (d.skGrip) { await ASk.gripUp(); return; }
      if (d.band) { const a = toPaper(d.x, d.y), b = toPaper(m.x, m.y); ASk.bandSelect({ x0: Math.min(a.x, b.x) * U, y0: Math.min(a.y, b.y) * U, x1: Math.max(a.x, b.x) * U, y1: Math.max(a.y, b.y) * U }, m.x >= d.x, e); render(); return; }
      if (Math.hypot(m.x - d.x, m.y - d.y) < 5) {
        // in Line's select tool a click off the lines picks a region or a label instead, leaving Line
        if (S.mode === "lines" && ASk.tool === "select" && !sessionCurveAt(m.x, m.y)) {
          const h = hit(m.x, m.y);
          if (h && h.kind !== "line") { endLines(); S.sel = h; props(); render(); ctxRefresh(); return; }
          if (h && h.kind === "line" && S.fam.lines[h.i] && (S.fam.lines[h.i].pointer ? "pointer" : "body") !== S.layer) { endLines(); startLines(h.i, "select"); return; }
        }
        const p = skSnap(m.x, m.y);
        Snap.clearOverride(); S.snapCycle = 0;
        await ASk.click(p, m.x, m.y, e);
      }
      return;
    }
    if (d) { const moved = d.started; S.drag = null; if (moved) { props(); status(); refreshDrawing(); } }
  }

  function onDbl() {
    if (!S) return;
    if (ASk.active) { ASk.dblclick(); return; }
    if (S.sel && S.sel.kind === "region") startRegion(S.fam.regions[S.sel.i].masking, S.sel.i);
    else if (S.sel && S.sel.kind === "label") editLabel(S.sel.i);
    else if (S.sel && S.sel.kind === "text") editText(S.sel.i);
  }

  function onWheel(e) {
    if (!S) return;
    e.preventDefault();
    const m = mouseAt(e), before = toPaper(m.x, m.y);
    S.view.k = Math.min(200, Math.max(2, S.view.k * Math.exp(-e.deltaY * 0.0012)));
    S.view.ox = m.x - before.x * S.view.k; S.view.oy = m.y + before.y * S.view.k;
    render();
  }

  // keys while the document is showing; the project's shortcuts stay out of it
  function keys(e) {
    if (!S || app.doc !== "annotation" || document.querySelector(".modal-back")) return;
    if (e.target && ["INPUT", "SELECT", "TEXTAREA"].includes(e.target.tagName)) return;
    const mod = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
    let used = true;
    if (mod && k === "z") { undo(); updateUndoButtons(); }
    else if (mod && k === "y") { redo(); updateUndoButtons(); }
    else if (mod && k === "s") loadIntoProject(false);
    else if (ASk.active) {
      const st = ASk.state();
      // Esc in Line's select tool with nothing selected leaves Line
      if (e.key === "Escape" && S.mode === "lines" && ASk.tool === "select" && !st.sel.size && !st.pts.length && !ASk.gripDragging && !st.typed) endLines();
      else used = ASk.key(e);
    }
    else if (e.key === "Escape") { if (S.tool !== "select") setTool("select"); else if (S.sel) { S.sel = null; props(); render(); ctxRefresh(); } }
    else if (e.key === "Delete" || e.key === "Backspace") removeSel();
    else used = false;
    if (used) e.preventDefault();
    e.stopPropagation();
  }

  return {
    open, close: () => close(false), activate, docInfo, commands, syncRibbon, context, undo, redo, undoState, save: () => loadIntoProject(false),
    isOpen: (id) => !!S && S.id === id,
    // test hooks
    _state: () => S, _sketch: ASk,
  };
})();
