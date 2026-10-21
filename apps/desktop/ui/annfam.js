// Annotation families as documents. A level head, an elevation mark or a section mark opens in the main window as a
// model family does (index.html #fam-*: Properties, Project Browser, view tab, options bar), on a canvas of its own,
// with a ribbon of its own (ribbon.js ANN_RIBBON). The drawing sits round the crossing of two reference lines, in
// paper millimetres: the horizontal one is what the symbol sits on (the level line for a head), the vertical one is
// where a head flips for the left hand end of a line. Changes stay in the document until Load into Project
// (crates/openarch-model/src/annotation.rs checks them there).

const AnnModel = (() => {
  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const snapMM = (v) => Math.round(v * 2) / 2; // half a millimetre
  const $q = (s) => document.querySelector(s);
  const NOTE = {
    level_head: "Level head: the level line arrives from the left and ends at the vertical reference line.",
    elevation_marker: "Elevation mark: the body is drawn once. The pointer is drawn for each view, turned to point along +x here.",
    section_marker: "Section mark: the body is drawn at the head of the line. The pointer is drawn at each end for the view direction, along +x here.",
  };
  const DRAW = [["line", "Line"], ["arc", "Center-ends Arc"], ["circle", "Circle"], ["polygon", "Polygon"]];

  let S = null; // the open document
  let wired = false;

  // ---------------------------------------------------------------- open, close, load
  async function open(id) {
    if (S && S.id === id) { activate(); return; }
    if (S) { activate(); setPrompt(`Close "${S.fam.name}" before opening another annotation family.`, true); return; }
    let raw;
    try { raw = await Backend.call("annotation_family", { id }); } catch (e) { setPrompt(String(e.message || e), true); return; }
    const fam = clone(raw);
    for (const k of ["id", "params", "kind", "in_use", "category"]) delete fam[k];
    S = {
      id, fam, params: raw.params, category: raw.category, tool: "select", shape: "line", layer: "shapes", sel: null, draft: null, drag: null,
      mouse: null, flipped: false, undo: [], redo: [], dirty: false, view: { k: 14, ox: 0, oy: 0 }, msg: "",
    };
    mount();
    activate();
    fit();
  }

  const hasPointer = () => S.fam.family_kind !== "level_head";
  const docInfo = () => (S ? { name: S.fam.name, dirty: S.dirty, id: S.id } : null);

  // Bring the document to the front (after opening it, or switching back to it)
  function activate() {
    if (!S) return;
    setDoc("annotation");
    resize();
    tabsUI(); toolbar(); props(); status();
  }

  function close(force) {
    if (!S) return;
    if (!force && S.dirty) { askLoad(); return; }
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
    const name = S.fam.name;
    try { await Backend.call("set_annotation_family", { id: S.id, family: S.fam }); }
    catch (e) { S.msg = String(e.message || e); status(true); return; }
    S.dirty = false;
    await refresh();
    if (andClose) { close(true); setPrompt(`Loaded ${name} into the project.`); return; }
    S.msg = "Loaded into the project.";
    activate();
  }

  // ---------------------------------------------------------------- undo
  const snapshot = () => JSON.stringify(S.fam);
  // every change goes through here: one undo step, then redraw
  function edit(fn, keepProps) {
    S.undo.push(snapshot());
    if (S.undo.length > 100) S.undo.shift();
    S.redo = [];
    fn(S.fam);
    S.dirty = true;
    if (!keepProps) props();
    status(); render();
  }
  function undo() { if (!S || !S.undo.length) return; S.redo.push(snapshot()); S.fam = JSON.parse(S.undo.pop()); S.sel = null; S.dirty = true; props(); status(); render(); }
  function redo() { if (!S || !S.redo.length) return; S.undo.push(snapshot()); S.fam = JSON.parse(S.redo.pop()); S.sel = null; S.dirty = true; props(); status(); render(); }
  const undoState = () => (S ? { undo: S.undo.length, redo: S.redo.length } : null);

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
      b.onclick = () => { S.flipped = !S.flipped; tabsUI(); syncRibbon(); render(); };
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

  function status(isError) {
    if (!S || app.doc !== "annotation") return;
    const tips = {
      select: "Click to select a line, shape or label; drag to move it. Delete removes it. Scroll to zoom.",
      line: { line: "Click the start of the line, then its end.", arc: "Click the centre of the arc, then its start, then its end (anticlockwise).", circle: "Click the centre, then a point on the circle.", polygon: "Click the corners. Click the first corner again, double-click or press Enter to close it." }[S.shape],
      region: "Click the corners of the filled region. Click the first corner again, double-click or press Enter to close it.",
      label: "Click where the label goes. Then choose its parameters in Properties.",
    };
    setPrompt((S.msg ? S.msg + "   " : "") + (tips[S.tool] || ""), !!isError);
    S.msg = "";
    syncTitle();
  }

  // ---------------------------------------------------------------- ribbon commands and the options bar
  const TOOL_OF = { "a-select": "select", "a-line": "line", "a-region": "region", "a-label": "label" };
  function setTool(t) {
    S.tool = t; S.draft = null;
    S.canvas.style.cursor = t === "select" ? "default" : "crosshair";
    toolbar(); status(); render();
  }
  const commands = {
    "a-del": () => removeSel(),
    "a-load": () => loadIntoProject(false),
    "a-loadclose": () => loadIntoProject(true),
    "a-fit": () => fit(),
    "a-flip": () => { if (S.fam.family_kind !== "level_head") { S.msg = "Only a level head flips."; status(true); return; } S.flipped = !S.flipped; tabsUI(); render(); },
  };
  for (const [id, t] of Object.entries(TOOL_OF)) commands[id] = () => setTool(t);
  for (const k of Object.keys(commands)) { const f = commands[k]; commands[k] = (e) => (S ? f(e) : undefined); }

  function syncRibbon() {
    document.querySelectorAll("#ribbon button.cmd[data-id^='a-']").forEach((b) => {
      const id = b.dataset.id;
      b.classList.toggle("on", !!S && (TOOL_OF[id] ? TOOL_OF[id] === S.tool : id === "a-flip" ? S.flipped : false));
    });
  }

  function toolbar() {
    syncRibbon();
    const bar = $q("#fam-options");
    bar.replaceChildren();
    const sel = (label, options, value, onchange) => {
      const s = el("select");
      for (const [v, t] of options) s.add(new Option(t, v, false, v === value));
      s.onchange = () => onchange(s.value);
      bar.append(el("span", "fe-opt", label), s);
    };
    if (S.tool === "line") sel("Draw", DRAW, S.shape, (v) => { S.shape = v; S.draft = null; status(); render(); });
    if ((S.tool === "line" || S.tool === "region") && hasPointer()) sel("Draw on", [["shapes", "Body"], ["pointer", "Pointer"]], S.layer, (v) => { S.layer = v; });
    bar.hidden = !bar.children.length;
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
  const items = () => [
    ...S.fam.shapes.map((s, i) => ({ layer: "shapes", i })),
    ...(hasPointer() ? S.fam.pointer.map((s, i) => ({ layer: "pointer", i })) : []),
    ...S.fam.labels.map((s, i) => ({ layer: "labels", i })),
  ];
  const get = (it) => (it ? S.fam[it.layer][it.i] : null);
  const NAMES = { line: "Line", arc: "Arc", circle: "Circle", polygon: "Filled Region" };

  function props() {
    const r = $q("#fam-prop-body");
    r.replaceChildren();
    const s = S.sel ? get(S.sel) : null;
    if (!s) {
      r.append(el("div", "famed-h", "Family"));
      r.append(field("Name", textIn(S.fam.name, (v) => { S.fam.name = v.trim() || S.fam.name; })));
      r.append(field("Category", el("span", "fe-ro", S.category)));
      r.append(field("Pen width (mm)", numIn(S.fam.pen_mm, (v) => { S.fam.pen_mm = v; }, 0.05)));
      if (S.fam.family_kind === "section_marker") {
        r.append(field("Broken Section Display Style", selIn(S.fam.broken_style || "gapped", [["gapped", "Gapped"], ["continuous", "Continuous"]], (v) => { S.fam.broken_style = v; })));
        r.append(field("Reference Label", textIn(S.fam.reference_label == null ? "SIM" : S.fam.reference_label, (v) => { S.fam.reference_label = v; })));
      }
      r.append(el("div", "famed-hint", NOTE[S.fam.family_kind]));
      return;
    }
    if (S.sel.layer === "labels") {
      r.append(el("div", "famed-h", "Label"));
      r.append(field("X (mm)", numIn(s.x, (v) => { s.x = v; })), field("Y (mm)", numIn(s.y, (v) => { s.y = v; })));
      r.append(field("Text height (mm)", numIn(s.size_mm, (v) => { s.size_mm = v; }, 0.25)));
      r.append(field("Horizontal", selIn(s.halign, [["left", "Left"], ["center", "Centre"], ["right", "Right"]], (v) => { s.halign = v; })));
      r.append(field("Vertical", selIn(s.valign, [["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]], (v) => { s.valign = v; })));
      r.append(field("Bold", checkIn(s.bold, (v) => { s.bold = v; })));
      r.append(el("div", "famed-h", "Label parameters (one per line)"));
      s.lines.forEach((ln, k) => {
        const box = el("div", "famed-line");
        box.append(
          field("Parameter", selIn(ln.param, S.params.map((p) => [p, p]), (v) => { ln.param = v; })),
          field("Prefix", textIn(ln.prefix, (v) => { ln.prefix = v; })),
          field("Suffix", textIn(ln.suffix, (v) => { ln.suffix = v; })),
          field("Sample", textIn(ln.sample, (v) => { ln.sample = v; })),
        );
        if (s.lines.length > 1) box.append(button("Remove line", () => edit(() => { s.lines.splice(k, 1); })));
        r.append(box);
      });
      r.append(button("Add parameter line", () => edit(() => { s.lines.push({ param: S.params[0], prefix: "", suffix: "", sample: "" }); })));
    } else {
      r.append(el("div", "famed-h", `${s.type === "polygon" && !s.filled ? "Polygon" : NAMES[s.type]}${S.sel.layer === "pointer" ? " (pointer)" : ""}`));
      if (s.type === "line") {
        r.append(field("X1", numIn(s.a[0], (v) => { s.a[0] = v; })), field("Y1", numIn(s.a[1], (v) => { s.a[1] = v; })), field("X2", numIn(s.b[0], (v) => { s.b[0] = v; })), field("Y2", numIn(s.b[1], (v) => { s.b[1] = v; })));
      } else if (s.type === "circle" || s.type === "arc") {
        r.append(field("Centre X", numIn(s.c[0], (v) => { s.c[0] = v; })), field("Centre Y", numIn(s.c[1], (v) => { s.c[1] = v; })), field("Radius", numIn(s.r, (v) => { s.r = Math.max(0.1, v); })));
        if (s.type === "circle") r.append(field("Filled", checkIn(s.filled, (v) => { s.filled = v; })));
        else r.append(field("Start (deg)", numIn(s.start_deg, (v) => { s.start_deg = v; }, 5)), field("End (deg)", numIn(s.end_deg, (v) => { s.end_deg = v; }, 5)));
      } else if (s.type === "polygon") {
        r.append(field("Filled", checkIn(s.filled, (v) => { s.filled = v; })));
        s.pts.forEach((p, k) => {
          const d = el("span", "famed-pt");
          d.append(numIn(p[0], (v) => { p[0] = v; }), numIn(p[1], (v) => { p[1] = v; }));
          r.append(field(`Point ${k + 1}`, d));
        });
      }
    }
    r.append(button("Delete", removeSel));
  }

  function removeSel() {
    if (!S.sel) return;
    const it = S.sel;
    S.sel = null;
    edit((f) => { f[it.layer].splice(it.i, 1); });
  }

  // ---------------------------------------------------------------- drawing
  const toScreen = (x, y) => ({ x: S.view.ox + x * S.view.k, y: S.view.oy - y * S.view.k });
  const toPaper = (sx, sy) => ({ x: (sx - S.view.ox) / S.view.k, y: (S.view.oy - sy) / S.view.k });
  const sampleLines = (lb) => lb.lines.map((l) => `${l.prefix || ""}${l.sample || ""}${l.suffix || ""}`);

  // frame the drawing, with room round it
  function fit() {
    if (!S || !S.canvas) return;
    const W = S.canvas.width || 800, H = S.canvas.height || 600;
    const e = Ann.extent({ shapes: [...S.fam.shapes, ...(hasPointer() ? S.fam.pointer : [])], labels: S.fam.labels });
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
    const pen = Math.max(1, (fam.pen_mm || 0.25) * S.view.k);
    const on = (layer, i) => S.sel && S.sel.layer === layer && S.sel.i === i;
    fam.shapes.forEach((s, i) => Ann.shapes(ctx, [s], base, on("shapes", i) ? accent : text, bg, pen));
    if (hasPointer()) fam.pointer.forEach((s, i) => Ann.shapes(ctx, [s], base, on("pointer", i) ? accent : "#a0522d", bg, pen));
    fam.labels.forEach((lb, i) => {
      const hot = on("labels", i);
      Ann.labels(ctx, [{ ...lb, lines: sampleLines(lb) }], base, hot ? accent : text);
      const e = Ann.extent({ labels: [lb] });
      const a = toScreen(S.flipped ? -e.maxX : e.minX, e.maxY), b = toScreen(S.flipped ? -e.minX : e.maxX, e.minY);
      ctx.save(); ctx.strokeStyle = hot ? accent : muted; ctx.lineWidth = hot ? 1.5 : 0.7; ctx.setLineDash([2, 3]); ctx.strokeRect(a.x, a.y, b.x - a.x, b.y - a.y); ctx.restore();
      const anchor = toScreen(S.flipped ? -lb.x : lb.x, lb.y);
      ctx.fillStyle = hot ? accent : muted; ctx.fillRect(anchor.x - 2, anchor.y - 2, 4, 4);
    });
    // the shape being drawn
    if (S.draft && S.mouse) {
      const p = toPaper(S.mouse.x, S.mouse.y), q = { x: snapMM(S.flipped ? -p.x : p.x), y: snapMM(p.y) };
      const prev = draftShape(q);
      if (prev) Ann.shapes(ctx, [prev], base, accent, null, pen);
    }
    if (S.sel && S.sel.layer !== "labels") {
      const sh = get(S.sel);
      if (sh) for (const [x, y] of handles(sh)) { const s = toScreen(S.flipped ? -x : x, y); ctx.fillStyle = accent; ctx.fillRect(s.x - 3, s.y - 3, 6, 6); }
    }
  }
  const handles = (sh) => (sh.type === "line" ? [sh.a, sh.b] : sh.type === "polygon" ? sh.pts : [sh.c]);

  // ---------------------------------------------------------------- drawing tools
  // the Line tool draws with the shape chosen in the options bar; Filled Region is a closed, filled polygon
  const shapeOf = () => (S.tool === "region" ? "polygon" : S.shape);
  function draftShape(q, closing) {
    const d = S.draft;
    if (!d) return null;
    if (d.shape === "line") return { type: "line", a: d.pts[0], b: [q.x, q.y] };
    if (d.shape === "circle") return { type: "circle", c: d.pts[0], r: Math.max(0.1, Math.hypot(q.x - d.pts[0][0], q.y - d.pts[0][1])), filled: false };
    if (d.shape === "polygon") return d.pts.length ? { type: "polygon", pts: closing ? d.pts : [...d.pts, [q.x, q.y]], filled: S.tool === "region" } : null;
    if (d.shape === "arc") {
      const c = d.pts[0];
      if (d.pts.length === 1) return { type: "line", a: c, b: [q.x, q.y] };
      const r = Math.hypot(d.pts[1][0] - c[0], d.pts[1][1] - c[1]);
      const a0 = (Math.atan2(d.pts[1][1] - c[1], d.pts[1][0] - c[0]) * 180) / Math.PI;
      let a1 = (Math.atan2(q.y - c[1], q.x - c[0]) * 180) / Math.PI;
      while (a1 <= a0) a1 += 360;
      return { type: "arc", c, r, start_deg: a0, end_deg: a1 };
    }
    return null;
  }
  function finishDraft(q, closing) {
    const sh = draftShape(q, closing);
    S.draft = null;
    if (!sh || (sh.type === "polygon" && sh.pts.length < 3)) { render(); return; }
    const target = S.layer === "pointer" && hasPointer() ? "pointer" : "shapes";
    edit((f) => { f[target].push(sh); });
    S.sel = { layer: target, i: S.fam[target].length - 1 };
    props(); render();
  }

  // ---------------------------------------------------------------- hit testing and the mouse
  const distSeg = (p, a, b) => {
    const dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2)) : 0;
    return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
  };
  function hit(sx, sy) {
    const P = { x: sx, y: sy }, k = S.view.k;
    const sc = (x, y) => toScreen(S.flipped ? -x : x, y);
    let best = null;
    const consider = (it, d) => { if (d <= 7 && (!best || d < best.d)) best = { it, d }; };
    for (const it of items()) {
      const s = get(it);
      if (it.layer === "labels") {
        const e = Ann.extent({ labels: [s] }), a = sc(e.minX, e.maxY), b = sc(e.maxX, e.minY);
        if (sx >= Math.min(a.x, b.x) - 3 && sx <= Math.max(a.x, b.x) + 3 && sy >= Math.min(a.y, b.y) - 3 && sy <= Math.max(a.y, b.y) + 3) consider(it, 2);
      } else if (s.type === "line") consider(it, distSeg(P, sc(...s.a), sc(...s.b)));
      else if (s.type === "circle") { const c = sc(...s.c), r = Math.hypot(sx - c.x, sy - c.y); consider(it, s.filled && r < s.r * k ? 1 : Math.abs(r - s.r * k)); }
      else if (s.type === "arc") { const c = sc(...s.c); consider(it, Math.abs(Math.hypot(sx - c.x, sy - c.y) - s.r * k)); }
      else if (s.type === "polygon") {
        const pts = s.pts.map((p) => sc(...p));
        let d = Infinity, inside = false;
        for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
          d = Math.min(d, distSeg(P, pts[j], pts[i]));
          if ((pts[i].y > sy) !== (pts[j].y > sy) && sx < ((pts[j].x - pts[i].x) * (sy - pts[i].y)) / (pts[j].y - pts[i].y) + pts[i].x) inside = !inside;
        }
        consider(it, s.filled && inside ? 1 : d);
      }
    }
    return best ? best.it : null;
  }
  function translate(sh, dx, dy) {
    if (sh.type === "line") { sh.a = [sh.a[0] + dx, sh.a[1] + dy]; sh.b = [sh.b[0] + dx, sh.b[1] + dy]; }
    else if (sh.type === "circle" || sh.type === "arc") sh.c = [sh.c[0] + dx, sh.c[1] + dy];
    else if (sh.type === "polygon") sh.pts = sh.pts.map((p) => [p[0] + dx, p[1] + dy]);
    else { sh.x += dx; sh.y += dy; }
  }

  const mouseAt = (e) => { const r = S.canvas.getBoundingClientRect(); return { x: ((e.clientX - r.left) * S.canvas.width) / r.width, y: ((e.clientY - r.top) * S.canvas.height) / r.height }; };
  const paperAt = (m) => { const p = toPaper(m.x, m.y); return { x: snapMM(S.flipped ? -p.x : p.x), y: snapMM(p.y) }; };

  function onDown(e) {
    if (!S) return;
    S.mouse = mouseAt(e);
    if (e.button === 1) { S.pan = { x: S.mouse.x, y: S.mouse.y, ox: S.view.ox, oy: S.view.oy }; S.canvas.setPointerCapture(e.pointerId); return; }
    if (e.button !== 0) return;
    const q = paperAt(S.mouse);
    if (S.tool === "select") {
      const h = hit(S.mouse.x, S.mouse.y);
      S.sel = h;
      if (h) { S.drag = { last: q, started: false }; S.canvas.setPointerCapture(e.pointerId); }
      props(); render();
      return;
    }
    if (S.tool === "label") {
      edit((f) => { f.labels.push({ x: q.x, y: q.y, halign: "left", valign: "middle", size_mm: 2.5, bold: false, lines: [{ param: S.params[0], prefix: "", suffix: "", sample: "Sample" }] }); });
      S.sel = { layer: "labels", i: S.fam.labels.length - 1 };
      setTool("select"); props();
      return;
    }
    const shape = shapeOf();
    if (!S.draft) { S.draft = { shape, pts: [[q.x, q.y]] }; render(); return; }
    if (S.draft.shape === "polygon") {
      const first = S.draft.pts[0];
      if (S.draft.pts.length >= 3 && Math.hypot(q.x - first[0], q.y - first[1]) < 0.6) { finishDraft({ x: first[0], y: first[1] }, true); return; }
      S.draft.pts.push([q.x, q.y]); render(); return;
    }
    if (S.draft.shape === "arc" && S.draft.pts.length === 1) { S.draft.pts.push([q.x, q.y]); render(); return; }
    finishDraft(q);
  }

  function onMove(e) {
    if (!S) return;
    S.mouse = mouseAt(e);
    if (S.pan) { S.view.ox = S.pan.ox + S.mouse.x - S.pan.x; S.view.oy = S.pan.oy + S.mouse.y - S.pan.y; render(); return; }
    if (S.drag && S.sel) {
      const q = paperAt(S.mouse), dx = q.x - S.drag.last.x, dy = q.y - S.drag.last.y;
      if (dx || dy) {
        // the whole drag is one undo step
        if (!S.drag.started) { S.undo.push(snapshot()); S.redo = []; S.drag.started = true; S.dirty = true; }
        translate(get(S.sel), dx, dy);
        S.drag.last = q;
      }
    }
    render();
  }

  function onUp() {
    if (!S) return;
    if (S.pan) { S.pan = null; return; }
    if (S.drag) { const moved = S.drag.started; S.drag = null; if (moved) { props(); status(); } }
  }

  function onDbl() {
    if (S && S.draft && S.draft.shape === "polygon" && S.draft.pts.length >= 3) { S.draft.pts.pop(); finishDraft({ x: 0, y: 0 }, true); }
  }

  function onWheel(e) {
    if (!S) return;
    e.preventDefault();
    const m = mouseAt(e), before = toPaper(m.x, m.y);
    S.view.k = Math.min(80, Math.max(2, S.view.k * Math.exp(-e.deltaY * 0.0012)));
    S.view.ox = m.x - before.x * S.view.k; S.view.oy = m.y + before.y * S.view.k;
    render();
  }

  // keys while the document is showing; the project's shortcuts stay out of it
  function keys(e) {
    if (!S || app.doc !== "annotation" || document.querySelector(".modal-back")) return;
    if (e.target && ["INPUT", "SELECT", "TEXTAREA"].includes(e.target.tagName)) return;
    const mod = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
    if (mod && k === "z") { e.preventDefault(); undo(); }
    else if (mod && k === "y") { e.preventDefault(); redo(); }
    else if (mod && k === "s") { e.preventDefault(); loadIntoProject(false); }
    else if (e.key === "Escape") { if (S.draft) { S.draft = null; render(); } else if (S.tool !== "select") setTool("select"); else if (S.sel) { S.sel = null; props(); render(); } e.preventDefault(); }
    else if (e.key === "Enter" && S.draft && S.draft.shape === "polygon" && S.draft.pts.length >= 3) { finishDraft({ x: 0, y: 0 }, true); e.preventDefault(); }
    else if (e.key === "Delete" || e.key === "Backspace") { removeSel(); e.preventDefault(); }
    e.stopPropagation();
  }

  return { open, close: () => close(false), activate, docInfo, commands, syncRibbon, undo, redo, undoState, save: () => loadIntoProject(false), isOpen: (id) => !!S && S.id === id };
})();
