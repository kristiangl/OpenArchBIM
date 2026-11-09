// Areas: Area Plan, Area Boundary (Pick Walls by the scheme's rule, or Draw lines), Area, Tag Area, and how an area
// plan draws its areas, boundary lines and tags. Private notes: areas.md.
const Areas = (() => {
  let tagOnPlacement = true, applyRules = true;
  const isAreaPlan = () => app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "area_plan";
  const schemeOfView = () => { const v = (app.info.views || []).find((x) => x.id === app.view.viewId); return (app.info.area_schemes || []).find((s) => s.id === v?.area_scheme); };

  // Area Plan: a scheme, the levels, then whether to draw boundaries round the outside of the building
  async function newPlan() {
    if (!(app.info.area_schemes || []).length) { if (!(await run("ensure_area_schemes", {}))) return; await refresh(); }
    openModal("New Area Plan", (body, foot, close) => {
      const scheme = el("select", {}, app.info.area_schemes.map((s) => new Option(s.name, s.id)));
      const levels = app.info.levels.map((l) => { const c = el("input", { type: "checkbox", checked: l.id === curLevelId() }); return { l, c, node: el("label", {}, [c, " ", l.name]) }; });
      const msg = el("div", { className: "modal-msg" });
      body.append(el("label", { className: "new-row" }, ["Type", scheme]), el("div", { className: "ex-h", textContent: "Levels" }), ...levels.map((x) => el("div", {}, [x.node])), msg);
      foot.append(button("Cancel", close), button("OK", async () => {
        const chosen = levels.filter((x) => x.c.checked).map((x) => x.l.id);
        if (!chosen.length) { msg.textContent = "Tick at least one level."; return; }
        close();
        const boundaries = await askConfirm("Automatically create area boundary lines along the outside of the building's exterior walls?");
        const r = await run("create_area_plans", { scheme_id: Number(scheme.value), levels: chosen, boundaries });
        if (!r) return;
        await refresh();
        await openView("plan", r.ids[0]);
      }, true));
    });
  }

  async function needPlan(what) {
    if (isAreaPlan()) return true;
    setPrompt(`${what} goes in an area plan: open one, or make one with Area Plan.`, true);
    return false;
  }

  // Area: click inside closed boundaries; Area Boundary: pick walls (the scheme's rule) or draw lines; Tag Area
  async function start(tool) {
    if (!(await needPlan(tool === "area" ? "An area" : tool === "areatag" ? "An area tag" : "An area boundary"))) return;
    Plan.setTool(tool);
    updateRibbonContext(true);
    setPrompt(tool === "area" ? "Click inside closed area boundaries to place an area. Esc finishes." : tool === "areatag" ? "Click inside a placed area to tag it. Esc finishes." : "Pick walls: the boundary follows the wall, placed by the area rules (Apply Area Rules) or on its centre. Line draws lines of your own. Esc finishes.");
  }
  function context() {
    const mode = { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] };
    if (Plan.tool === "area") return { auto: true, key: `area:${tagOnPlacement}`, title: "Place Area", groups: [mode, { label: "Area", items: [
      { label: "Tag on Placement", icon: "tagRoom", on: tagOnPlacement, onclick: () => { tagOnPlacement = !tagOnPlacement; updateRibbonContext(); } },
      { label: "Place Areas Automatically", icon: "areasAuto", onclick: async () => { const r = await run("place_all_areas", { view_id: app.view.viewId, tag: tagOnPlacement }); if (r) { await refresh(); setPrompt(r.ids.length ? `Placed ${r.ids.length} area${r.ids.length === 1 ? "" : "s"}.` : "Every enclosed area already has an area."); } } },
    ] }] };
    if (Plan.tool === "areatag") return { auto: true, key: "areatag", title: "Tag Area", groups: [mode] };
    return { auto: true, key: `areaboundary:${applyRules}`, title: "Place Area Boundary", groups: [mode, { label: "Draw", items: [
      { label: "Pick Walls", icon: "wall", on: Plan.tool === "areaboundary", onclick: () => Plan.setTool("areaboundary") },
      { label: "Line", icon: "line", onclick: () => drawLines() },
    ] }, { label: "Options", items: [
      { label: "Apply Area Rules", icon: "areaRules", on: applyRules, onclick: () => { applyRules = !applyRules; updateRibbonContext(); } },
    ] }] };
  }
  async function click(tool, p, wallId) {
    if (tool === "area") {
      if (await run("add_area", { view_id: app.view.viewId, point: [p.x, p.y], tag: tagOnPlacement })) await refresh();
    } else if (tool === "areatag") {
      if (await run("add_area_tag", { view_id: app.view.viewId, point: [p.x, p.y] })) await refresh();
    } else if (tool === "areaboundary") {
      if (wallId == null) { setPrompt("Pick a wall.", true); return; }
      if (await run("add_wall_area_boundaries", { view_id: app.view.viewId, wall_ids: [wallId], apply_rules: applyRules, offset_mm: 0 })) await refresh();
    }
  }
  function drawLines() {
    if (Sketch.active) return;
    Plan.startSketch({ kind: "areaboundary", title: "Area Boundary", levelId: curLevelId(), curves: [], tool: "line", closed: false });
    renderSketchUI();
  }
  async function finish(o) {
    const r = await run("add_area_boundaries", { view_id: app.view.viewId, curves: o.curves, apply_rules: false });
    if (!r) return false;
    Plan.endSketch(); endSketchUI(); await refresh();
    return true;
  }

  function elements(data, k) {
    const out = [], sq = (p, r) => [[p.x - r, p.y - r], [p.x + r, p.y - r], [p.x + r, p.y + r], [p.x - r, p.y + r]];
    for (const a of data?.areas || []) out.push({ id: a.id, kind: "area", category: "Areas", ring: sq(a.at, 4 / k), pinned: a.pinned, display: "normal" });
    for (const t of data?.area_tags || []) out.push({ id: t.id, kind: "areatag", category: "Area Tags", ring: sq(t.at, 6 * (data.view?.scale || 100)), pinned: t.pinned, display: "normal" });
    for (const b of data?.area_boundaries || []) for (let i = 1; i < b.pts.length; i++) {
      const [a, c] = [b.pts[i - 1], b.pts[i]], l = Math.hypot(c[0] - a[0], c[1] - a[1]) || 1, nx = -(c[1] - a[1]) * 3 / k / l, ny = (c[0] - a[0]) * 3 / k / l;
      out.push({ id: b.id, kind: "areaboundary", category: "Area Boundary", ring: [[a[0] + nx, a[1] + ny], [c[0] + nx, c[1] + ny], [c[0] - nx, c[1] - ny], [a[0] - nx, a[1] - ny]], pinned: b.pinned, display: "normal" });
    }
    return out;
  }

  function draw(ctx, data, toScreen, k, sel = new Set(), printing = false) {
    if (!data?.areas && !data?.area_boundaries) return;
    ctx.save();
    const path = (pts, close) => { ctx.beginPath(); pts.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); }); if (close) ctx.closePath(); };
    // an area's face, pale when selected; the reference cross while editing
    for (const a of data.areas || []) {
      if (sel.has(a.id) && a.rings.length) { ctx.fillStyle = "rgba(46,111,158,0.16)"; ctx.beginPath(); for (const r of a.rings) { r.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); }); ctx.closePath(); } ctx.fill("evenodd"); }
      if (!printing) { const p = toScreen(a.at); ctx.strokeStyle = a.status === "placed" ? "#7a5da8" : "#b63727"; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(p.x - 4, p.y - 4); ctx.lineTo(p.x + 4, p.y + 4); ctx.moveTo(p.x - 4, p.y + 4); ctx.lineTo(p.x + 4, p.y - 4); ctx.stroke(); }
    }
    for (const b of data.area_boundaries || []) { ctx.strokeStyle = sel.has(b.id) ? "#2e6f9e" : "#7a5da8"; ctx.lineWidth = sel.has(b.id) ? 2.5 : 1.6; path(b.pts, false); ctx.stroke(); }
    for (const t of data.area_tags || []) {
      const p = toScreen(t.at), base = { x: p.x, y: p.y, k: k * (data.view?.scale || 100) }, col = sel.has(t.id) ? "#2e6f9e" : null;
      if (t.leader && t.area_at) { const q = toScreen(t.area_at); ctx.strokeStyle = col || "#333"; ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); ctx.stroke(); }
      if (t.family) Ann.paths(ctx, t.family.body, base, col, "#fff");
      Ann.labels(ctx, t.labels || [], base, col);
    }
    ctx.restore();
  }

  return { isAreaPlan, newPlan, start, context, click, finish, elements, draw };
})();
