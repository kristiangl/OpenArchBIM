// Door and window tags: Tag by Category, Tag All Not Tagged, Tag on Placement, and the tag drawing that plans,
// elevations, sheets and PDF/DXF export share. A tag's head, element point, leader end and elbow come in view mm
// (plan x and y, or along the view and up in an elevation).
const Tags = (() => {
  // the choices on Modify | Tag and Modify | Place Door/Window, kept between uses
  const opts = { orientation: "horizontal", leader: false, leader_mm: 12 };
  let onPlacement = true;
  const TAGGABLE = ["Doors", "Windows"];
  const scaleOf = (data) => data?.view?.scale || data?.scale || 100;

  // pick each tag by its drawn box (paper mm turned with the tag, times the view scale)
  function elements(data) {
    const out = [], s = scaleOf(data);
    for (const t of data?.element_tags || []) {
      const e = t.family ? Ann.extent({ body: t.family.body, labels: t.labels }) : { minX: -3, maxX: 3, minY: -2, maxY: 2 };
      const pad = 0.8;
      let [x0, x1, y0, y1] = [e.minX - pad, e.maxX + pad, e.minY - pad, e.maxY + pad];
      if (t.vertical) [x0, x1, y0, y1] = [-y1, -y0, x0, x1];
      const p = t.at, ring = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(([x, y]) => [p.x + x * s, p.y + y * s]);
      out.push({ id: t.id, kind: "elementtag", category: t.category, ring, pinned: t.pinned, display: "normal" });
    }
    return out;
  }

  // ---- a selected tag's leader grips (plans): the end, dragged off the element to free it (back onto the
  // element's point it attaches again), and the elbow, a grip midway that adds one (dragged back onto the straight
  // line it goes)
  let gdrag = null; // { id, kind: "end" | "elbow", p (view mm), moved, sx, sy }
  const shown = (t) => (gdrag && gdrag.id === t.id && gdrag.moved ? (gdrag.kind === "end" ? { ...t, end: gdrag.p } : { ...t, elbow: gdrag.p }) : t);
  function grips(t, toScreen) {
    const end = toScreen(t.end), head = toScreen(t.at);
    return { end, elbow: t.elbow ? toScreen(t.elbow) : { x: (end.x + head.x) / 2, y: (end.y + head.y) / 2 }, hasElbow: !!t.elbow, head };
  }
  const selectedTag = (data, sel) => (sel.size === 1 ? (data?.element_tags || []).find((t) => sel.has(t.id) && t.leader && !t.pinned) : null);
  function drawGrips(ctx, data, toScreen, sel) {
    const t0 = selectedTag(data, sel);
    if (!t0) return;
    const g = grips(shown(t0), toScreen);
    ctx.save(); ctx.strokeStyle = "#2e6f9e"; ctx.fillStyle = "#2e6f9e"; ctx.lineWidth = 1.2; ctx.setLineDash([]);
    ctx.beginPath(); ctx.arc(g.end.x, g.end.y, 4, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = "#fff"; ctx.beginPath(); ctx.arc(g.elbow.x, g.elbow.y, g.hasElbow ? 4 : 3, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    ctx.restore();
  }
  function gripDown(sx, sy, H) {
    const t = selectedTag(H.data(), H.selection());
    if (!t) return false;
    const g = grips(t, H.toScreen);
    const kind = Math.hypot(sx - g.end.x, sy - g.end.y) <= 7 ? "end" : Math.hypot(sx - g.elbow.x, sy - g.elbow.y) <= 7 ? "elbow" : null;
    if (!kind) return false;
    gdrag = { id: t.id, kind, sx, sy, p: H.toWorld(sx, sy), moved: false, H };
    return true;
  }
  function gripMove(sx, sy) {
    if (!gdrag) return;
    if (!gdrag.moved && Math.hypot(sx - gdrag.sx, sy - gdrag.sy) > 4) gdrag.moved = true;
    gdrag.p = gdrag.H.toWorld(sx, sy);
    gdrag.H.render();
  }
  async function gripUp(sx, sy) {
    const g = gdrag; gdrag = null;
    if (!g) return;
    const H = g.H, t = (H.data()?.element_tags || []).find((x) => x.id === g.id);
    if (!t || !g.moved) { H.render(); return; }
    const p = H.toWorld(sx, sy), pt = t.point, sp = H.toScreen(p), r2 = (v) => Math.round(v * 10) / 10;
    let patch;
    if (g.kind === "end") {
      const e = H.toScreen(pt);
      patch = Math.hypot(sp.x - e.x, sp.y - e.y) <= 8 ? { leader_end: "attached", end_offset: null } : { leader_end: "free", end_offset: [r2(p.x - pt.x), r2(p.y - pt.y)] };
    } else {
      // near the straight line from the end to the head the elbow goes
      const a = H.toScreen(t.end), b = H.toScreen(t.at), dx = b.x - a.x, dy = b.y - a.y, L = Math.hypot(dx, dy) || 1;
      const off = Math.abs((sp.x - a.x) * dy - (sp.y - a.y) * dx) / L;
      patch = off <= 6 ? { elbow: null } : { elbow: [r2(p.x - pt.x), r2(p.y - pt.y)] };
    }
    await run("patch_tag", { id: t.id, patch });
    await refresh();
  }

  function draw(ctx, data, toScreen, k, sel = new Set()) {
    if (!data || !(data.element_tags || []).length) return;
    const s = scaleOf(data);
    ctx.save();
    for (const t0 of data.element_tags) {
      const t = shown(t0);
      const col = sel.has(t.id) ? "#2e6f9e" : null;
      const head = toScreen(t.at);
      if (t.leader) {
        const end = toScreen(t.end), elbow = t.elbow ? toScreen(t.elbow) : null;
        ctx.strokeStyle = col || "#222"; ctx.lineWidth = Math.max(0.6, 0.18 * k * s); ctx.setLineDash([]);
        ctx.beginPath(); ctx.moveTo(end.x, end.y); if (elbow) ctx.lineTo(elbow.x, elbow.y); ctx.lineTo(head.x, head.y); ctx.stroke();
      }
      // a vertical tag reads from the bottom up
      ctx.save(); ctx.translate(head.x, head.y); if (t.vertical) ctx.rotate(-Math.PI / 2);
      const base = { x: 0, y: 0, k: k * s };
      if (t.family) Ann.paths(ctx, t.family.body, base, col, "#fff");
      Ann.labels(ctx, t.labels || [], base, col);
      ctx.restore();
    }
    ctx.restore();
  }

  // ---- Tag by Category: hover a door or window and click it ----------------------------------------------------
  async function start() {
    if (app.view.kind === "elevation") Elev.setTool("tag");
    else { if (!(await ensurePlan("A tag"))) return; Plan.setTool("tag"); }
    updateRibbonContext(true);
    setPrompt("Click a door or window to tag it. Esc finishes.");
  }
  const taggable = (category) => TAGGABLE.includes(category);
  async function place(elementId) {
    if (elementId == null) { setPrompt("Click a door or window to tag it.", true); return; }
    if (await run("add_tag", { view_id: app.view.viewId, element_id: elementId, options: { ...opts } })) await refresh();
  }
  // after a door or window is placed with Tag on Placement on
  async function placed(elementId) {
    if (!onPlacement || elementId == null) return;
    await run("add_tag", { view_id: app.view.viewId, element_id: elementId, options: { ...opts } });
  }

  // the panels Tag by Category and Place Door/Window add to the Modify tab
  function optionGroups() {
    const orient = el("select", { title: "Orientation of new tags" }, [new Option("Horizontal", "horizontal", false, opts.orientation === "horizontal"), new Option("Vertical", "vertical", false, opts.orientation === "vertical")]);
    orient.onchange = () => { opts.orientation = orient.value; };
    const len = el("input", { type: "number", min: "1", step: "1", value: opts.leader_mm, title: "Leader length on paper (mm)", style: "width:52px" });
    len.onchange = () => { const v = parseFloat(len.value); if (v > 0) opts.leader_mm = v; };
    len.disabled = !opts.leader;
    return [{ label: "Tag Options", items: [
      { big: true, node: el("div", { className: "rb-select" }, [orient]) },
      { label: "Leader", icon: "leader", on: opts.leader, onclick: () => { opts.leader = !opts.leader; updateRibbonContext(); } },
      { big: true, node: el("div", { className: "rb-select" }, [len, " mm"]) },
    ] }];
  }
  function context() {
    const mode = { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => (app.view.kind === "elevation" ? Elev : Plan).setTool("select") }] };
    return { auto: true, key: `tag:${opts.leader}`, title: "Tag", groups: [mode, ...optionGroups()] };
  }
  // Place Door and Place Window: Tag on Placement and the same options
  function placementGroups() {
    return [{ label: "Tag", items: [{ label: "Tag on Placement", icon: "tag", on: onPlacement, onclick: () => { onPlacement = !onPlacement; updateRibbonContext(); } }] }, ...(onPlacement ? optionGroups() : [])];
  }

  // Spacebar on selected tags turns them between horizontal and vertical
  async function flipSelected() {
    const data = app.view.kind === "elevation" ? Elev.data : app.view.kind === "plan" ? Plan.viewData() : null;
    const ids = new Set((data?.element_tags || []).map((t) => t.id));
    if (!app.selection.length || !app.selection.every((id) => ids.has(id))) return false;
    if (await run("flip_tags", { ids: app.selection })) await refresh();
    return true;
  }

  // ---- Tag All Not Tagged --------------------------------------------------------------------------------------
  async function tagAll() {
    if (!["plan", "elevation"].includes(app.view.kind)) { if (!(await ensurePlan("Tag All"))) return; }
    const view = app.view.viewId, picked = [...app.selection];
    const base = await run("tag_all_options", { view_id: view });
    if (!base) return;
    openModal("Tag All Not Tagged", (body, foot, close) => {
      const msg = el("div", { className: "modal-msg" });
      const scope = el("select", {}, [new Option("All objects in current view", "all"), new Option("Only selected objects in current view", "selected", false, picked.length > 0)]);
      scope.disabled = !picked.length;
      const rows = base.map((c) => {
        const on = el("input", { type: "checkbox", checked: c.kind !== "room_tag" && c.untagged > 0 });
        const fam = el("select", {}, c.families.length ? c.families.map((f) => new Option(f.name, f.id)) : [new Option(`${c.label.replace(/s$/, "")} (loaded on first use)`, "")]);
        const count = el("span", { className: "modal-note", textContent: `${c.untagged} untagged` });
        return { c, on, fam, node: el("div", { className: "sched-rule" }, [el("label", { style: "min-width:130px;white-space:nowrap" }, [on, " ", c.label]), fam, count]) };
      });
      const recount = async () => {
        const r = await run("tag_all_options", { view_id: view, ...(scope.value === "selected" ? { only: picked } : {}) });
        if (r) rows.forEach((x) => { const n = r.find((y) => y.kind === x.c.kind)?.untagged ?? 0; x.node.lastChild.textContent = `${n} untagged`; });
      };
      scope.onchange = recount;
      const leader = el("input", { type: "checkbox", checked: opts.leader });
      const len = el("input", { type: "number", min: "1", value: opts.leader_mm, style: "width:60px" });
      const orient = el("select", {}, [new Option("Horizontal", "horizontal", false, opts.orientation === "horizontal"), new Option("Vertical", "vertical", false, opts.orientation === "vertical")]);
      body.append(
        el("label", { className: "new-row" }, ["Tag", scope]),
        el("div", { className: "ex-h", textContent: "Categories and their tag families" }), ...rows.map((x) => x.node),
        el("div", { className: "sched-rule" }, [el("label", {}, [leader, " Leader"]), el("label", {}, ["Length ", len, " mm"])]),
        el("label", { className: "new-row" }, ["Orientation", orient]), msg);
      if (scope.value === "selected") recount();
      foot.append(button("Cancel", close), button("OK", async () => {
        const kinds = rows.filter((x) => x.on.checked).map((x) => ({ kind: x.c.kind, family: x.fam.value ? Number(x.fam.value) : null }));
        if (!kinds.length) { msg.textContent = "Tick at least one category."; return; }
        Object.assign(opts, { leader: leader.checked, leader_mm: parseFloat(len.value) || 12, orientation: orient.value });
        try {
          const r = await Backend.call("tag_all", { view_id: view, kinds, ...(scope.value === "selected" ? { only: picked } : {}), options: { ...opts } });
          close(); await refresh();
          setPrompt(r.ids.length ? `Tagged ${r.ids.length} element${r.ids.length === 1 ? "" : "s"}.` : "Everything in those categories is tagged in this view already.");
        } catch (e) { msg.textContent = String(e.message || e); }
      }, true));
    });
  }

  return { elements, draw, start, taggable, place, placed, context, placementGroups, flipSelected, tagAll, drawGrips, gripDown, gripMove, gripUp, get dragging() { return !!gdrag; } };
})();
