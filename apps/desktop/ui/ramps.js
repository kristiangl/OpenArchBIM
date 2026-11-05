// Ramps: the Ramp tool (Create Ramp mode: draw runs, each click pair one run, stopping where the ramp reaches its
// top at its steepest slope; landings between them on Finish), Edit Ramp, and how a plan draws a ramp (as a stair is
// drawn, with each run's slope written on it). The Rust side lays it out (crates/openarch-model/src/ramp.rs).
const Ramps = (() => {
  let draft = null, pendingStart = null, pendingEnd = null, preview = null, asked = 0, lastAsk = 0;
  const typeId = () => {
    const ts = app.info.ramp_types || [];
    if (!ts.some((t) => t.id === app.rampTypeId)) app.rampTypeId = ts[0]?.id;
    return app.rampTypeId;
  };
  const levelsUp = () => [...app.info.levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
  const params = (extra = {}) => ({ type_id: draft.typeId, base_level: draft.baseLevel, top_level: draft.topLevel, base_offset_mm: draft.baseOffset, top_offset_mm: draft.topOffset, unconnected_height_mm: draft.height, runs: draft.runs, railing_type: draft.id ? undefined : (draft.railing ?? undefined), ...extra });
  const pendingOf = () => (pendingStart && pendingEnd ? { start: [pendingStart.x, pendingStart.y], end: [pendingEnd.x, pendingEnd.y], justification: draft.justification, width_mm: draft.width } : null);

  async function start(edit = null) {
    if (!(app.info.ramp_types || []).length) { if (!(await run("ensure_ramp_types", {}))) return; await refresh(); }
    if (!(await ensurePlan("A ramp"))) return;
    if (edit) {
      const r = await run("ramp_edit", { id: edit });
      if (!r) return;
      draft = { id: edit, typeId: r.type_id, baseLevel: r.base_level, topLevel: r.top_level, baseOffset: r.base_offset_mm, topOffset: r.top_offset_mm, height: r.unconnected_height_mm, justification: r.runs[0]?.justification || "center", width: r.runs[0]?.width_mm || 1200, runs: r.runs };
    } else {
      // most ramps climb less than a storey: an unconnected height to start with
      draft = { id: null, railing: (app.info.railing_types || [])[0]?.id ?? null, typeId: typeId(), baseLevel: curLevelId(), topLevel: null, baseOffset: 0, topOffset: 0, height: 600, justification: "center", width: 1200, runs: [] };
    }
    pendingStart = pendingEnd = null;
    app.selection = []; Plan.setSelection([]);
    Plan.setTool("ramp");
    updateRibbonContext(true);
    await ask();
    renderProperties();
    say();
  }
  async function ask() {
    if (!draft) return;
    const n = ++asked;
    const r = await Backend.call("ramp_draft", params({ pending: pendingOf() })).catch((e) => ({ error: String(e.message || e) }));
    if (n !== asked || !draft) return;
    preview = r;
    Plan.render();
  }
  function say() {
    if (!draft) return;
    const p = preview;
    if (p && p.error) { setPrompt(p.error, true); return; }
    const done = p ? `${Math.round(p.drawn_mm)} of ${Math.round(p.needed_mm)} drawn, ${Math.round(p.remaining_mm)} remaining.` : "";
    if (p && p.remaining_mm < 1 && !pendingStart) { setPrompt(`${done} Finish to make the ramp, or delete the last run.`); return; }
    setPrompt(`${pendingStart ? "Click the end of the run." : "Click the start of a run."} ${done}`);
  }
  function hover(p) {
    if (!draft || !pendingStart) return;
    pendingEnd = p;
    const now = performance.now();
    if (now - lastAsk < 60) return;
    lastAsk = now;
    ask().then(say);
  }
  async function click(p) {
    if (!draft) return;
    if (!pendingStart) { pendingStart = p; pendingEnd = null; say(); return; }
    pendingEnd = p;
    const r = await run("ramp_draft", params({ pending: pendingOf() }));
    if (!r || !r.pending) { setPrompt("The run needs a length: click further from its start.", true); return; }
    draft.runs.push(r.pending);
    pendingStart = pendingEnd = null;
    await ask();
    say();
  }
  const last = () => pendingStart;
  async function finish() {
    if (!draft) return;
    if (!draft.runs.length) { setPrompt("Draw at least one run first.", true); return; }
    const r = draft.id ? await run("set_ramp_runs", { id: draft.id, runs: draft.runs }) : await run("add_ramp", params());
    if (!r) return;
    const id = draft.id || r.id;
    draft = null; preview = null; pendingStart = pendingEnd = null;
    Plan.setTool("select");
    await refresh();
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties(); updateRibbonContext();
    setPrompt("Ramp finished.");
  }
  function cancel() { draft = null; preview = null; pendingStart = pendingEnd = null; Plan.setTool("select"); updateRibbonContext(); renderProperties(); setPrompt(""); }
  function deleteLastRun() { if (!draft || !draft.runs.length) return; draft.runs.pop(); pendingStart = pendingEnd = null; ask().then(say); }
  function escape() {
    if (pendingStart) { pendingStart = pendingEnd = null; ask().then(say); return true; }
    setPrompt("Finish the ramp, or Cancel it.");
    return true;
  }
  function endTool() { draft = null; preview = null; pendingStart = pendingEnd = null; }

  function context() {
    return { auto: true, key: `ramp:${draft?.runs.length || 0}`, title: draft?.id ? "Edit Ramp" : "Create Ramp", groups: [
      { label: "Mode", items: [{ label: "Finish Edit Mode", ico: "✓", big: true, onclick: finish }, { label: "Cancel Edit Mode", ico: "✕", big: true, onclick: cancel }] },
      { label: "Components", items: [{ label: "Run", icon: "ramp", big: true, on: true, onclick: () => {} }, { label: "Delete Last Run", ico: "⌫", disabled: !draft?.runs.length, onclick: deleteLastRun }] },
    ] };
  }
  function toolProps() {
    if (!draft) return null;
    const levels = levelsUp(), editing = !!draft.id;
    const opt = (l, patch, sel) => ({ label: l, patch, selected: sel });
    const rows = [
      { label: "Base Level", kind: editing ? "ro" : "select", value: levels.find((l) => l.id === draft.baseLevel)?.name || "", options: levels.map((l) => opt(l.name, { baseLevel: l.id }, l.id === draft.baseLevel)) },
      { label: "Base Offset", kind: editing ? "ro" : "number", raw: draft.baseOffset, value: String(draft.baseOffset), path: "baseOffset" },
      { label: "Top Level", kind: editing ? "ro" : "select", value: levels.find((l) => l.id === draft.topLevel)?.name || "None", options: [opt("None", { topLevel: null }, draft.topLevel == null), ...levels.map((l) => opt(l.name, { topLevel: l.id }, l.id === draft.topLevel))] },
      { label: "Top Offset", kind: editing || draft.topLevel == null ? "ro" : "number", raw: draft.topOffset, value: String(draft.topOffset), path: "topOffset" },
      { label: "Desired Height", kind: editing || draft.topLevel != null ? "ro" : "number", raw: draft.height, value: String(preview?.height_mm ?? draft.height), path: "height" },
    ];
    const runRows = [
      { label: "Location Line", kind: "select", value: "", options: [["left", "Run: Left"], ["center", "Run: Center"], ["right", "Run: Right"]].map(([v, l]) => opt(l, { justification: v }, draft.justification === v)) },
      { label: "Width", kind: "number", raw: draft.width, value: String(draft.width), path: "width" },
    ];
    if (!editing) runRows.push({ label: "Railing", kind: "select", value: "", options: [opt("None", { railing: null }, draft.railing == null), ...(app.info.railing_types || []).map((t) => opt(t.name, { railing: t.id }, t.id === draft.railing))] });
    const apply = (patch) => {
      for (const [k, v] of Object.entries(patch)) draft[k] = ["baseOffset", "topOffset", "height", "width"].includes(k) ? Number(v) : v;
      ask().then(() => { renderProperties(); say(); });
    };
    return { title: editing ? "Edit Ramp" : "Create Ramp", typeId: draft.typeId, setType: (id) => { draft.typeId = id; app.rampTypeId = id; ask().then(() => { renderProperties(); say(); }); },
      groups: [{ name: "Constraints", target: { apply }, rows }, { name: "Run", target: { apply }, rows: runRows }] };
  }
  function selectionContext() {
    if (app.view.kind !== "plan" || app.selection.length !== 1 || Plan.tool !== "select") return null;
    const r = (Plan.viewData()?.ramps || []).find((x) => x.id === app.selection[0]);
    if (!r) return null;
    return { auto: true, key: `rampsel:${r.id}`, title: "Ramps", groups: [{ label: "Edit", items: [{ label: "Edit Ramp", icon: "ramp", big: true, onclick: () => start(r.id) }] }] };
  }
  function elements(data) {
    return (data?.ramps || []).filter((r) => r.id !== draft?.id && r.rings.length).map((r) => ({ id: r.id, kind: "ramp", category: "Ramps", ring: r.rings[0], rings: r.rings, pinned: r.pinned, display: "normal" }));
  }
  function draw(ctx, data, toScreen, k, sel, pen) {
    const list = (data?.ramps || []).filter((r) => r.id !== draft?.id);
    Stairs.draw(ctx, data, toScreen, k, sel, pen, list);
    ctx.save();
    // each run's slope, along it
    for (const r of list) {
      for (const s of r.slopes || []) {
        const p = toScreen({ x: s.at[0], y: s.at[1] });
        let a = Math.atan2(-s.dir[1], s.dir[0]);
        if (a > Math.PI / 2 || a < -Math.PI / 2) a += Math.PI;
        ctx.save(); ctx.translate(p.x, p.y); ctx.rotate(a);
        ctx.fillStyle = sel.has(r.id) ? "#2e6f9e" : pen.colour(r.style, r.style.line.color);
        ctx.font = `${Math.max(9, 2.5 * k * (data.view?.scale || 100))}px sans-serif`;
        ctx.textAlign = "center"; ctx.textBaseline = "bottom";
        ctx.fillText(s.text, 0, -4);
        ctx.restore();
      }
    }
    if (draft && preview && preview.lines) {
      ctx.beginPath();
      for (const [a, b] of preview.lines) { const p = toScreen({ x: a[0], y: a[1] }), q = toScreen({ x: b[0], y: b[1] }); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); }
      ctx.strokeStyle = "#d4572f"; ctx.lineWidth = 1.5; ctx.setLineDash([]); ctx.stroke();
      if (pendingStart) { const p = toScreen(pendingStart); ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fillStyle = "#d4572f"; ctx.fill(); }
    }
    ctx.restore();
  }
  return { start, click, hover, last, finish, cancel, escape, endTool, context, toolProps, selectionContext, elements, draw, typeId, active: () => !!draft };
})();
