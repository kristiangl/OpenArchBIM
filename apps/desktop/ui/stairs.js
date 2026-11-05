// Stairs: the Stair tool (Create Stair mode: draw straight runs, each click pair one run, landings between them
// made on Finish), Edit Stairs, and how a plan draws a stair (solid below the cut, dashed above, the cut mark, the
// walkline with UP or DN). The Rust side lays the stair out (crates/openarch-model/src/stairs.rs). Private notes:
// stairs.md.
const Stairs = (() => {
  // the stair being drawn: its settings, its runs so far, and the run under the cursor
  let draft = null;
  let clicks = [], pendingEnd = null, preview = null, asked = 0, lastAsk = 0;
  // the run shape being drawn, and the clicks it takes: straight (start, end), full-step spiral (centre, start),
  // centre-ends spiral (centre, start, end), L winder (start, corner, end), U winder (start, two corners, end)
  let shape = "straight", clockwise = null, winderStyle = "balanced";
  // a run or landing being sketched: the stair waits; a sketched run's new lines are boundaries or risers
  let sketching = false, sketchRole = "boundary";
  const NEED = { straight: 2, spiral_full: 2, spiral_ends: 3, winder_l: 3, winder_u: 4 };
  const STAGE = {
    straight: ["Click the start of a run.", "Click the end of the run."],
    spiral_full: ["Click the centre of the spiral.", "Click where the run starts (its radius)."],
    spiral_ends: ["Click the centre of the spiral.", "Click where the run starts (its radius).", "Click where the run ends."],
    winder_l: ["Click the start of the run.", "Click the corner.", "Click towards the end of the run."],
    winder_u: ["Click the start of the run.", "Click the first corner.", "Click the second corner.", "Click towards the end of the run."],
  };

  const typeId = () => {
    const ts = app.info.stair_types || [];
    if (!ts.some((t) => t.id === app.stairTypeId)) app.stairTypeId = ts[0]?.id;
    return app.stairTypeId;
  };
  const levelsUp = () => [...app.info.levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
  const levelAbove = (id) => { const ls = levelsUp(), i = ls.findIndex((l) => l.id === id); return i >= 0 && i + 1 < ls.length ? ls[i + 1].id : null; };

  function params(extra = {}) {
    const d = draft;
    return { type_id: d.typeId, base_level: d.baseLevel, top_level: d.topLevel, base_offset_mm: d.baseOffset, top_offset_mm: d.topOffset, unconnected_height_mm: d.height,
      desired_risers: d.risers ?? undefined, tread_depth_mm: d.tread ?? undefined, runs: d.runs, landings: d.landings || [], railing_type: d.id ? undefined : (d.railing ?? undefined), ...extra };
  }
  function pendingOf() {
    if (!clicks.length || !pendingEnd) return null;
    const pts = [...clicks, pendingEnd];
    if (shape === "straight") return { start: [pts[0].x, pts[0].y], end: [pts[1].x, pts[1].y], justification: draft.justification, width_mm: draft.width };
    return { kind: shape, points: pts.map((p) => [p.x, p.y]), clockwise: shape === "spiral_full" ? !!clockwise : clockwise ?? undefined, style: winderStyle, justification: draft.justification, width_mm: draft.width };
  }

  async function start(edit = null) {
    if (!(app.info.stair_types || []).length) { if (!(await run("ensure_stair_types", {}))) return; await refresh(); }
    if (!(await ensurePlan("A stair"))) return;
    if (edit) {
      const r = await run("stair_edit", { id: edit });
      if (!r) return;
      draft = { id: edit, landings: r.landings || [], typeId: r.type_id, baseLevel: r.base_level, topLevel: r.top_level, baseOffset: r.base_offset_mm, topOffset: r.top_offset_mm, height: r.unconnected_height_mm, risers: r.desired_risers, tread: r.tread_depth_mm, justification: r.runs[0]?.justification || "center", width: r.runs[0]?.width_mm || 1000, runs: r.runs };
    } else {
      const base = curLevelId();
      draft = { id: null, landings: [], railing: (app.info.railing_types || [])[0]?.id ?? null, typeId: typeId(), baseLevel: base, topLevel: levelAbove(base), baseOffset: 0, topOffset: 0, height: 3000, risers: null, tread: null, justification: "center", width: (app.info.stair_types || []).find((t) => t.id === typeId())?.width_mm || 1000, runs: [] };
    }
    clicks = []; pendingEnd = null;
    app.selection = []; Plan.setSelection([]);
    Plan.setTool("stair");
    updateRibbonContext(true);
    await ask();
    renderProperties();
    say();
  }

  // the draft laid out again (the preview, and the riser counts)
  async function ask() {
    if (!draft) return;
    const n = ++asked;
    const r = await Backend.call("stair_draft", params({ pending: pendingOf() })).catch((e) => ({ error: String(e.message || e) }));
    if (n !== asked || !draft) return;
    preview = r;
    Plan.render();
  }
  function say(extra = "") {
    if (!draft) return;
    const p = preview;
    if (p && p.error) { setPrompt(p.error, true); return; }
    const done = p ? `${p.created} of ${p.desired_risers} risers created, ${p.remaining} remaining.` : "";
    if (p && p.remaining === 0 && !clicks.length) { setPrompt(`${done} Finish to make the stair, or delete the last run.`.trim()); return; }
    setPrompt(`${STAGE[shape][clicks.length] || ""} ${done} ${extra}`.trim());
  }

  function hover(p) {
    if (!draft || !clicks.length) return;
    pendingEnd = p;
    const now = performance.now();
    if (now - lastAsk < 60) return;
    lastAsk = now;
    ask().then(() => say());
  }
  async function click(p) {
    if (!draft) return;
    if (!clicks.length && preview && preview.remaining === 0) { say(); return; }
    // a full-step spiral needs one click before it can be shown; each shape its own number
    if (clicks.length + 1 < NEED[shape]) { clicks.push(p); pendingEnd = null; say(); return; }
    pendingEnd = p;
    const r = await run("stair_draft", params({ pending: pendingOf() }));
    if (!r || !r.pending) { setPrompt("The run needs a length: click further from its start.", true); return; }
    draft.runs.push(r.pending);
    // the next run starts where the cursor is clicked; a landing joins them on Finish
    clicks = []; pendingEnd = null;
    await ask();
    say();
  }
  const last = () => clicks[clicks.length - 1] || null;

  async function finish() {
    if (!draft) return;
    if (!draft.runs.length) { setPrompt("Draw at least one run first.", true); return; }
    const r = draft.id ? await run("set_stair_runs", { id: draft.id, runs: draft.runs, landings: draft.landings }) : await run("add_stair", params());
    if (!r) return;
    const id = draft.id || r.id;
    draft = null; preview = null; clicks = []; pendingEnd = null;
    Plan.setTool("select");
    await refresh();
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties(); updateRibbonContext();
    setPrompt("Stair finished.");
  }
  function cancel() {
    draft = null; preview = null; clicks = []; pendingEnd = null;
    Plan.setTool("select"); updateRibbonContext(); renderProperties(); setPrompt("");
  }
  function deleteLastRun() {
    if (!draft || !draft.runs.length) return;
    draft.runs.pop(); clicks = []; pendingEnd = null;
    draft.landings = (draft.landings || []).filter((l) => l.after_run < draft.runs.length);
    ask().then(() => say());
  }
  // Esc drops the run being drawn; the stair stays in progress until Finish or Cancel
  function escape() {
    if (clicks.length) { clicks = []; pendingEnd = null; ask().then(() => say()); return true; }
    setPrompt("Finish the stair, or Cancel it.");
    return true;
  }
  function endTool() { if (draft && !sketching) { draft = null; preview = null; clicks = []; pendingEnd = null; } }

  // Sketch Run (two boundaries and riser lines, the first riser drawn at the bottom) and Sketch Landing (a closed
  // outline at the top of the last run): the sketch's Finish gives the stair its new run or landing
  const ROLE_INK = { boundary: "#2a8f3c", riser: "#1f5fbf" };
  function sketch(kind) {
    if (!draft || Sketch.active) return;
    if (kind === "stairlanding" && !draft.runs.length) { setPrompt("Draw a run first: a landing stands at the top of the last run.", true); return; }
    sketching = true; clicks = []; pendingEnd = null;
    sketchRole = "boundary";
    Plan.startSketch({ kind, levelId: draft.baseLevel, typeId: draft.typeId, curves: [], tool: "line", closed: kind === "stairlanding",
      title: kind === "stairrun" ? "Sketch Run" : "Sketch Landing",
      ...(kind === "stairrun" ? { attr: () => ({ role: sketchRole }), ink: (c) => ROLE_INK[c.attr?.role] || ROLE_INK.boundary } : {}) });
    renderSketchUI();
    setPrompt(kind === "stairrun" ? "Draw the run's two boundaries, then switch to Riser and draw its riser lines, the bottom one first." : "Draw the landing's outline as a closed loop.");
  }
  async function sketchFinished(o) {
    let ok = false;
    if (o.kind === "stairrun") {
      const r = await run("stair_sketch_run", { edges: o.edges || [] });
      if (r) { draft.runs.push(r.run); ok = true; }
    } else {
      const r = await run("stair_sketch_landing", { curves: o.curves });
      if (r) { draft.landings = (draft.landings || []).filter((l) => l.after_run !== draft.runs.length - 1); draft.landings.push({ after_run: draft.runs.length - 1, outline: r.outline }); ok = true; }
    }
    if (!ok) return false;
    backToStair();
    return true;
  }
  function sketchCancelled() { backToStair(); }
  function backToStair() {
    Plan.endSketch(); endSketchUI();
    sketching = false;
    Plan.setTool("stair");
    updateRibbonContext(true);
    renderProperties();
    ask().then(() => say());
  }
  const role = () => sketchRole;
  function roleGroup() {
    const set = (r) => { sketchRole = r; if (Sketch.selectedAttr && Sketch.selectedAttr()) Sketch.patchSelected({ role: r }); updateRibbonContext(); };
    return { label: "Draw", items: [
      { label: "Boundary", ico: "Bd", on: sketchRole === "boundary", onclick: () => set("boundary") },
      { label: "Riser", ico: "Rs", on: sketchRole === "riser", onclick: () => set("riser") },
    ] };
  }

  function context() {
    const groups = [
      { label: "Mode", items: [
        { label: "Finish Edit Mode", ico: "✓", big: true, onclick: finish },
        { label: "Cancel Edit Mode", ico: "✕", big: true, onclick: cancel },
      ] },
      { label: "Run", items: [
        ...[["straight", "Straight", "stair"], ["spiral_full", "Full-Step Spiral", null], ["spiral_ends", "Centre-Ends Spiral", null], ["winder_l", "L-Shape Winder", null], ["winder_u", "U-Shape Winder", null]].map(([v, l, icon]) => ({
          label: l, ...(icon ? { icon } : { ico: l.split(" ").map((w) => w[0]).join("").slice(0, 2) }), big: v === "straight", on: shape === v,
          onclick: () => { shape = v; clicks = []; pendingEnd = null; updateRibbonContext(); ask().then(() => say()); },
        })),
        { label: clockwise ? "Clockwise" : "Anticlockwise", ico: clockwise ? "↻" : "↺", disabled: !shape.startsWith("spiral"), onclick: () => { clockwise = !clockwise; updateRibbonContext(); ask(); } },
        { label: winderStyle === "balanced" ? "Balanced Winders" : "Single Point Winders", ico: winderStyle === "balanced" ? "Bw" : "Sp", disabled: !shape.startsWith("winder"), onclick: () => { winderStyle = winderStyle === "balanced" ? "single_point" : "balanced"; updateRibbonContext(); ask(); } },
        { label: "Sketch Run", ico: "Sk", onclick: () => sketch("stairrun") },
        { label: "Sketch Landing", ico: "Ld", onclick: () => sketch("stairlanding") },
        { label: "Delete Last Run", ico: "⌫", disabled: !draft?.runs.length, onclick: deleteLastRun },
      ] },
    ];
    return { auto: true, key: `stair:${draft?.runs.length || 0}:${shape}:${clockwise}:${winderStyle}`, title: draft?.id ? "Edit Stairs" : "Create Stair", groups };
  }
  // Properties while drawing: the stair's type, levels and risers, and the run's location line and width
  function toolProps() {
    if (!draft) return null;
    const levels = levelsUp();
    const opt = (l, patch, sel) => ({ label: l, patch, selected: sel });
    const editing = !!draft.id;
    const rows = [
      { label: "Base Level", kind: editing ? "ro" : "select", value: levels.find((l) => l.id === draft.baseLevel)?.name || "", options: levels.map((l) => opt(l.name, { baseLevel: l.id }, l.id === draft.baseLevel)) },
      { label: "Base Offset", kind: editing ? "ro" : "number", raw: draft.baseOffset, value: String(draft.baseOffset), path: "baseOffset" },
      { label: "Top Level", kind: editing ? "ro" : "select", value: levels.find((l) => l.id === draft.topLevel)?.name || "None", options: [opt("None", { topLevel: null }, draft.topLevel == null), ...levels.map((l) => opt(l.name, { topLevel: l.id }, l.id === draft.topLevel))] },
      { label: "Top Offset", kind: editing || draft.topLevel == null ? "ro" : "number", raw: draft.topOffset, value: String(draft.topOffset), path: "topOffset" },
      { label: "Desired Stair Height", kind: editing || draft.topLevel != null ? "ro" : "number", raw: draft.height, value: String(draft.height), path: "height" },
      { label: "Desired Number of Risers", kind: editing ? "ro" : "number", raw: preview?.desired_risers, value: String(preview?.desired_risers ?? ""), path: "risers" },
      { label: "Actual Riser Height", kind: "ro", value: preview?.riser_mm ? preview.riser_mm.toFixed(1) : "" },
      { label: "Actual Tread Depth", kind: editing ? "ro" : "number", raw: preview?.tread_mm, value: String(preview?.tread_mm ?? ""), path: "tread" },
    ];
    const run = [
      { label: "Location Line", kind: "select", value: "", options: [["left", "Run: Left"], ["center", "Run: Center"], ["right", "Run: Right"]].map(([v, l]) => opt(l, { justification: v }, draft.justification === v)) },
      { label: "Actual Run Width", kind: "number", raw: draft.width, value: String(draft.width), path: "width" },
    ];
    if (!editing) run.push({ label: "Railing", kind: "select", value: "", options: [opt("None", { railing: null }, draft.railing == null), ...(app.info.railing_types || []).map((t) => opt(t.name, { railing: t.id }, t.id === draft.railing))] });
    const apply = (patch) => {
      for (const [k, v] of Object.entries(patch)) draft[k] = ["baseOffset", "topOffset", "height", "width", "risers", "tread"].includes(k) ? Number(v) : v;
      if (patch.baseLevel != null || patch.topLevel !== undefined) draft.risers = null; // the count follows the new height
      ask().then(() => { renderProperties(); say(); });
    };
    return { title: draft.id ? "Edit Stairs" : "Create Stair", typeId: draft.typeId, setType: (id) => { draft.typeId = id; app.stairTypeId = id; draft.tread = null; draft.risers = null; ask().then(() => { renderProperties(); say(); }); },
      groups: [{ name: "Constraints", target: { apply }, rows }, { name: "Run", target: { apply }, rows: run }] };
  }

  // a selected stair: Edit Stairs
  function selectionContext() {
    if (app.view.kind !== "plan" || app.selection.length !== 1 || Plan.tool !== "select") return null;
    const s = (Plan.viewData()?.stairs || []).find((x) => x.id === app.selection[0]);
    if (!s) return null;
    return { auto: true, key: `stairsel:${s.id}`, title: "Stairs", groups: [{ label: "Edit", items: [{ label: "Edit Stairs", icon: "stair", big: true, onclick: () => start(s.id) }] }] };
  }

  function elements(data) {
    return (data?.stairs || []).filter((s) => s.id !== draft?.id && s.rings.length).map((s) => ({ id: s.id, kind: "stair", category: "Stairs", ring: s.rings[0], rings: s.rings, pinned: s.pinned, display: "normal" }));
  }

  // pen: { lw(style), colour(style, hex), dash(style) } from the plan's own drawing
  // `list`: other items drawn as stairs are (a plan's ramps); without it, the plan's stairs and the stair being drawn
  function draw(ctx, data, toScreen, k, sel, pen, list = null) {
    const seg = (a, b) => { const p = toScreen({ x: a[0], y: a[1] }), q = toScreen({ x: b[0], y: b[1] }); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); };
    const stroke = (st, selected, dashed) => {
      ctx.strokeStyle = selected ? "#2e6f9e" : pen.colour(st, st.line.color);
      ctx.lineWidth = pen.lw(st) + (selected ? 0.8 : 0);
      ctx.setLineDash(dashed ? (pen.dash(st).length ? pen.dash(st) : [6, 4]) : pen.dash(st));
      ctx.stroke();
    };
    ctx.save();
    ctx.lineCap = "butt";
    for (const s of list || data?.stairs || []) {
      if (s.id === draft?.id) continue;
      const on = sel.has(s.id);
      for (const above of [false, true]) {
        ctx.beginPath();
        for (const l of s.lines) if (l.above === above) seg(l.a, l.b);
        stroke(above ? s.above_style : s.style, on, above);
      }
      ctx.beginPath();
      for (const line of s.cut) line.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); });
      stroke(s.cut_style, on, false);
      // the walkline, its arrow and UP or DN
      for (const above of [false, true]) {
        ctx.beginPath();
        for (const g of s.path) if (g.above === above) seg(g.a, g.b);
        stroke(s.style, on, above);
      }
      if (s.arrow) {
        const a = toScreen({ x: s.arrow.at[0], y: s.arrow.at[1] }), f = toScreen({ x: s.arrow.from[0], y: s.arrow.from[1] });
        const ang = Math.atan2(a.y - f.y, a.x - f.x), len = Math.max(6, 2.5 * k * (data.view?.scale || 100)), w = len * 0.35;
        ctx.beginPath(); ctx.moveTo(a.x, a.y);
        ctx.lineTo(a.x - len * Math.cos(ang) + w * Math.sin(ang), a.y - len * Math.sin(ang) - w * Math.cos(ang));
        ctx.lineTo(a.x - len * Math.cos(ang) - w * Math.sin(ang), a.y - len * Math.sin(ang) + w * Math.cos(ang));
        ctx.closePath(); ctx.fillStyle = on ? "#2e6f9e" : pen.colour(s.style, s.style.line.color); ctx.fill();
      }
      if (s.label_at) {
        const p = toScreen({ x: s.label_at[0], y: s.label_at[1] });
        ctx.fillStyle = on ? "#2e6f9e" : pen.colour(s.style, s.style.line.color);
        ctx.font = `${Math.max(9, 2.5 * k * (data.view?.scale || 100))}px sans-serif`;
        ctx.textAlign = "center"; ctx.textBaseline = "top";
        ctx.fillText(s.label, p.x, p.y + 3);
      }
    }
    // the stair being drawn
    if (!list && draft && preview && preview.lines) {
      ctx.beginPath();
      for (const [a, b] of preview.lines) seg(a, b);
      ctx.strokeStyle = "#d4572f"; ctx.lineWidth = 1.5; ctx.setLineDash([]); ctx.stroke();
      for (const c of clicks) {
        const p = toScreen(c);
        ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fillStyle = "#d4572f"; ctx.fill();
      }
    }
    ctx.restore();
  }

  return { start, click, hover, last, finish, cancel, escape, endTool, context, toolProps, selectionContext, elements, draw, typeId, active: () => !!draft, sketchFinished, sketchCancelled, roleGroup, role };
})();
