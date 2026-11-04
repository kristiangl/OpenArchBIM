// Ceilings: Automatic Ceiling and Sketch Ceiling from a reflected ceiling plan, Edit Boundary, and how a ceiling plan
// draws them (outline and the type's grid, clipped to the ceiling). Private notes: ceilings.md.
const Ceilings = (() => {
  let mode = "auto"; // Automatic Ceiling or Sketch Ceiling
  let preview = null, asked = 0; // the enclosure under the cursor, for Automatic Ceiling
  const typeId = () => {
    const ts = app.info.ceiling_types || [];
    if (!ts.some((t) => t.id === app.ceilingTypeId)) app.ceilingTypeId = ts[0]?.id;
    return app.ceilingTypeId;
  };
  const offset = () => (Number.isFinite(app.ceilingOffset) ? app.ceilingOffset : 2400);
  const isCeilingPlan = () => app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "ceiling_plan";

  // Ceilings are placed in a reflected ceiling plan: open the level's, making one if it has none
  async function ensureCeilingPlan() {
    if (isCeilingPlan()) return true;
    const level = app.view.kind === "plan" ? curLevelId() : (app.info.levels[0] || {}).id;
    let v = app.info.views.find((x) => x.view_kind === "ceiling_plan" && x.level_id === level);
    if (!v) {
      const vt = (app.info.view_types || []).find((t) => t.view_kind === "ceiling_plan");
      const r = vt && (await run("create_plan_views", { view_type: vt.id, levels: [level] }));
      if (!r) return false;
      await refresh();
      v = app.info.views.find((x) => x.id === r.ids[0]);
    }
    await openView("plan", v.id);
    return true;
  }

  async function start() {
    if (!(app.info.ceiling_types || []).length) { if (!(await run("ensure_ceiling_types", {}))) return; await refresh(); }
    if (!(await ensureCeilingPlan())) return;
    typeId();
    mode = "auto";
    Plan.setTool("ceiling");
    updateRibbonContext(true);
    renderProperties();
    setPrompt("Click inside walls that form a closed loop to place a ceiling, or pick Sketch Ceiling. Esc finishes.");
  }

  function context() {
    const groups = [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] },
      { label: "Ceiling", items: [
        { label: "Automatic Ceiling", icon: "ceiling", big: true, on: mode === "auto", onclick: () => { mode = "auto"; Plan.setTool("ceiling"); updateRibbonContext(); } },
        { label: "Sketch Ceiling", icon: "editBoundary", big: true, on: false, onclick: () => startSketch(null) },
      ] }];
    return { auto: true, key: `ceiling:${mode}`, title: "Place Ceiling", groups };
  }

  // Automatic Ceiling: the enclosure under the cursor shows before the click
  let lastAsk = 0;
  async function hover(p) {
    // at most every 120 ms: finding the enclosure walks the level's walls
    const now = performance.now();
    if (now - lastAsk < 120) return;
    lastAsk = now;
    const ask = ++asked;
    const r = await Backend.call("ceiling_region_at", { level_id: curLevelId(), offset_mm: offset(), point: [p.x, p.y] }).catch(() => null);
    if (ask !== asked) return;
    preview = r && r.rings ? r.rings : null;
    Plan.render();
  }
  async function place(p) {
    const r = await run("add_ceiling", { type_id: typeId(), level_id: curLevelId(), offset_mm: offset(), point: [p.x, p.y] });
    if (!r) return;
    preview = null;
    await refresh();
    setPrompt("Ceiling placed. Click inside another enclosure, or press Esc.");
  }

  async function startSketch(id) {
    if (Sketch.active) return;
    if (id == null && !(await ensureCeilingPlan())) return;
    if (id != null) {
      const r = await run("ceiling_sketch", { id });
      if (!r) return;
      Plan.startSketch({ kind: "ceiling", id, levelId: r.level_id, typeId: r.type_id, curves: r.curves, slope: r.slope, tool: "select", title: "Edit Ceiling Boundary" });
    } else {
      app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
      Plan.startSketch({ kind: "ceiling", levelId: curLevelId(), typeId: typeId(), curves: [], tool: "line", title: "Create Ceiling Boundary" });
    }
    renderSketchUI();
  }
  async function finish(o) {
    const r = o.id
      ? await run("set_ceiling_sketch", { id: o.id, curves: o.curves, slope: o.slope })
      : await run("add_ceiling", { type_id: typeId(), level_id: o.levelId, offset_mm: offset(), curves: o.curves, slope: o.slope });
    if (!r) return false;
    const id = o.id || r.id;
    Plan.endSketch(); endSketchUI();
    await refresh();
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    setPrompt(o.id ? "Ceiling boundary updated." : "Ceiling created.");
    return true;
  }

  function elements(data) {
    return (data?.ceilings || []).filter((c) => c.id !== Sketch.editingId && c.rings.length).map((c) => ({ id: c.id, kind: "ceiling", category: "Ceilings", ring: c.rings[0], rings: c.rings, pinned: c.pinned, display: "normal" }));
  }

  // each ceiling's outline, its grid clipped to it, and Automatic Ceiling's preview
  function draw(ctx, data, toScreen, k, sel = new Set()) {
    const ring = (r) => r.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); });
    ctx.save();
    for (const c of data?.ceilings || []) {
      if (c.id === Sketch.editingId) continue;
      ctx.beginPath(); c.rings.forEach((r) => { ring(r); ctx.closePath(); });
      if (c.grid) {
        ctx.save(); ctx.clip("evenodd");
        const [across, along] = c.grid, o = c.grid_origin || { x: 0, y: 0 }, a = ((c.grid_angle_deg || 0) * Math.PI) / 180;
        const u = { x: Math.cos(a), y: Math.sin(a) }, v = { x: -Math.sin(a), y: Math.cos(a) };
        const pts = c.rings.flat(), proj = (p, d) => (p[0] - o.x) * d.x + (p[1] - o.y) * d.y;
        const span = (d) => [Math.min(...pts.map((p) => proj(p, d))), Math.max(...pts.map((p) => proj(p, d)))];
        const [u0, u1] = span(u), [v0, v1] = span(v);
        ctx.strokeStyle = sel.has(c.id) ? "#2e6f9e" : "#9aa0a8"; ctx.lineWidth = 0.6; ctx.setLineDash([]);
        ctx.beginPath();
        const line = (p, q) => { const s1 = toScreen(p), s2 = toScreen(q); ctx.moveTo(s1.x, s1.y); ctx.lineTo(s2.x, s2.y); };
        if (across * k > 2) for (let t = Math.floor(u0 / across) * across; t <= u1; t += across) line({ x: o.x + u.x * t + v.x * v0, y: o.y + u.y * t + v.y * v0 }, { x: o.x + u.x * t + v.x * v1, y: o.y + u.y * t + v.y * v1 });
        if (along * k > 2) for (let t = Math.floor(v0 / along) * along; t <= v1; t += along) line({ x: o.x + v.x * t + u.x * u0, y: o.y + v.y * t + u.y * u0 }, { x: o.x + v.x * t + u.x * u1, y: o.y + v.y * t + u.y * u1 });
        ctx.stroke(); ctx.restore();
      }
      ctx.beginPath(); c.rings.forEach((r) => { ring(r); ctx.closePath(); });
      ctx.strokeStyle = sel.has(c.id) ? "#2e6f9e" : "#555"; ctx.lineWidth = sel.has(c.id) ? 2 : 1; ctx.stroke();
    }
    if (preview && Plan.tool === "ceiling") {
      ctx.beginPath(); preview.forEach((r) => { ring(r); ctx.closePath(); });
      ctx.strokeStyle = "#d4572f"; ctx.lineWidth = 2; ctx.setLineDash([6, 4]); ctx.stroke();
    }
    ctx.restore();
  }

  // A selected ceiling: Edit Boundary, and Move Grid / Rotate Grid when its type has a grid. Move Grid takes a base
  // point and a destination; Rotate Grid a centre, a start direction and an end direction.
  let gridEdit = null; // { id, tool: "move" | "rotate", points: [] }
  function selected() {
    if (app.view.kind !== "plan" || app.selection.length !== 1) return null;
    return (Plan.viewData()?.ceilings || []).find((c) => c.id === app.selection[0]) || null;
  }
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (gridEdit && Plan.tool !== "ceilinggrid") gridEdit = null;
    const c = selected() || (gridEdit && (Plan.viewData()?.ceilings || []).find((x) => x.id === gridEdit.id));
    if (!c || Plan.tool !== "select" && Plan.tool !== "ceilinggrid") return null;
    const items = [{ label: "Edit Boundary", icon: "editBoundary", big: true, onclick: () => startSketch(c.id) }];
    const grid = c.grid ? [{ label: "Grid", items: [
      { label: "Move Grid", ico: "Mv", on: gridEdit?.tool === "move", onclick: () => startGrid(c, "move") },
      { label: "Rotate Grid", ico: "Ro", on: gridEdit?.tool === "rotate", onclick: () => startGrid(c, "rotate") },
    ] }] : [];
    return { auto: true, key: `ceilingsel:${c.id}:${gridEdit?.tool || ""}`, title: "Ceilings", groups: [{ label: "Mode", items }, ...grid] };
  }
  function startGrid(c, tool) {
    gridEdit = { id: c.id, tool, points: [], origin: c.grid_origin || { x: 0, y: 0 }, angle: c.grid_angle_deg || 0 };
    Plan.setTool("ceilinggrid");
    updateRibbonContext();
    setPrompt(tool === "move" ? "Move Grid: click a base point, then where it goes." : "Rotate Grid: click the centre, then the start direction, then the end direction.");
  }
  async function gridClick(p) {
    const g = gridEdit;
    if (!g) return;
    g.points.push(p);
    const need = g.tool === "move" ? 2 : 3;
    if (g.points.length < need) { setPrompt(g.tool === "move" ? "Click where the base point goes." : g.points.length === 1 ? "Click the start direction." : "Click the end direction."); return; }
    let patch;
    if (g.tool === "move") {
      const [a, b] = g.points;
      patch = { grid_origin: { x: g.origin.x + b.x - a.x, y: g.origin.y + b.y - a.y } };
    } else {
      const [c, s, e] = g.points;
      const turn = Math.atan2(e.y - c.y, e.x - c.x) - Math.atan2(s.y - c.y, s.x - c.x), cos = Math.cos(turn), sin = Math.sin(turn);
      const dx = g.origin.x - c.x, dy = g.origin.y - c.y;
      patch = { grid_origin: { x: c.x + dx * cos - dy * sin, y: c.y + dx * sin + dy * cos }, grid_angle_deg: (((g.angle + (turn * 180) / Math.PI) % 360) + 360) % 360 };
    }
    const id = g.id;
    gridEdit = null;
    Plan.setTool("select");
    if (await run("patch_ceiling", { id, patch })) { await refresh(); app.selection = [id]; Plan.setSelection([id]); renderProperties(); setPrompt("Ceiling grid moved."); }
    updateRibbonContext();
  }

  const clearPreview = () => { preview = null; asked++; };
  return { ensureCeilingPlan, selectionContext, gridClick, start, context, hover, place, startSketch, finish, elements, draw, clearPreview, isCeilingPlan, offset, typeId };
})();
