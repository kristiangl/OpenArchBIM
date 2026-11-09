// Railings: Sketch Path (and Edit Path) on a plan, Place on Stair (a railing each side of the stair clicked), and how
// a plan draws them (each part from above, rails above the cut dashed). The Rust side builds them
// (crates/openarch-model/src/railing.rs). Private notes: stairs.md, step 2.
const Railings = (() => {
  let placement = "treads";
  // Pick New Host: the railing waiting for a floor to be clicked
  let hostFor = null;
  const typeId = () => {
    const ts = app.info.railing_types || [];
    if (!ts.some((t) => t.id === app.railingTypeId)) app.railingTypeId = ts[0]?.id;
    return app.railingTypeId;
  };
  async function ensureTypes() {
    if ((app.info.railing_types || []).length) return true;
    if (!(await run("ensure_railing_types", {}))) return false;
    await refresh();
    return true;
  }

  async function startSketch(id = null) {
    if (Sketch.active || !(await ensureTypes())) return;
    if (id != null) {
      const r = await run("railing_path", { id });
      if (!r) return;
      Plan.startSketch({ kind: "railing", id, levelId: r.level_id, typeId: r.type_id, curves: r.curves, tool: "select", title: "Edit Railing Path", closed: false });
    } else {
      if (!(await ensurePlan("A railing"))) return;
      typeId();
      app.selection = []; Plan.setSelection([]);
      Plan.startSketch({ kind: "railing", levelId: curLevelId(), typeId: typeId(), curves: [], tool: "line", title: "Create Railing Path", closed: false });
    }
    renderSketchUI();
    renderProperties();
  }
  async function finish(o) {
    const r = o.id ? await run("set_railing_path", { id: o.id, curves: o.curves }) : await run("add_railing", { type_id: typeId(), level_id: o.levelId, curves: o.curves });
    if (!r) return false;
    const id = o.id || r.id;
    Plan.endSketch(); endSketchUI();
    await refresh();
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    setPrompt(o.id ? "Railing path updated." : "Railing created.");
    return true;
  }

  async function placeOnStair() {
    if (!(await ensureTypes()) || !(await ensurePlan("A railing"))) return;
    typeId();
    Plan.setTool("railingstair");
    updateRibbonContext(true);
    renderProperties();
    setPrompt("Click a stair or a ramp to put a railing on each side of it. Esc finishes.");
  }
  // `ids`: what is under the click, nearest first
  async function placeClick(ids) {
    const stairs = new Set([...(Plan.viewData()?.stairs || []), ...(Plan.viewData()?.ramps || [])].map((s) => s.id));
    const id = ids.find((i) => stairs.has(i));
    if (id == null) { setPrompt("Click a stair or a ramp.", true); return; }
    const r = await run("add_stair_railings", { stair_id: id, type_id: typeId(), placement });
    if (!r) return;
    await refresh();
    setPrompt("Railings placed on both sides. Click another stair, or press Esc.");
  }
  function context() {
    const items = [["treads", "Treads"], ["stringer", "Stringer"]].map(([v, l]) => ({ label: l, ico: l[0], on: placement === v, onclick: () => { placement = v; updateRibbonContext(); } }));
    return { auto: true, key: `railingstair:${placement}`, title: "Place Railing on Stair", groups: [
      { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => { Plan.setTool("select"); updateRibbonContext(); setPrompt(""); } }] },
      { label: "Position", items },
    ] };
  }

  // Pick New Host: a click on a floor stands the railing on it; a click off every floor puts it back on its level
  function pickHost(id) {
    hostFor = id;
    Plan.setTool("railinghost");
    updateRibbonContext(true);
    setPrompt("Click the floor the railing stands on, or somewhere off any floor to put it back on its level. Esc cancels.");
  }
  function inside(p, ring) {
    let c = false;
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const a = ring[i], b = ring[j];
      if ((a[1] > p.y) !== (b[1] > p.y) && p.x < ((b[0] - a[0]) * (p.y - a[1])) / (b[1] - a[1]) + a[0]) c = !c;
    }
    return c;
  }
  async function hostClick(p) {
    const id = hostFor;
    if (id == null) return;
    // the topmost floor under the click (a floor's rings: its outline and any openings)
    const f = (Plan.viewData()?.floors || []).filter((x) => (x.rings && x.rings.length ? x.rings.filter((r) => inside(p, r)).length % 2 === 1 : inside(p, x.points))).pop();
    const ok = await run("patch_railing", { id, patch: { floor_host: f ? f.id : null } });
    hostFor = null;
    Plan.setTool("select");
    if (ok) await refresh();
    app.selection = [id]; Plan.setSelection([id]);
    updateRibbonContext(); renderProperties();
    if (ok) setPrompt(f ? "The railing stands on the floor." : "The railing is back on its level.");
  }
  function hostContext() {
    return { auto: true, key: "railinghost", title: "Pick New Host", groups: [{ label: "Mode", items: [{ label: "Cancel", ico: "✕", big: true, onclick: () => { hostFor = null; Plan.setTool("select"); updateRibbonContext(); setPrompt(""); } }] }] };
  }

  // a selected railing: Flip; a sketched one also Edit Path and Pick New Host
  function selectionContext() {
    if (app.view.kind !== "plan" || app.selection.length !== 1 || Plan.tool !== "select") return null;
    const r = (Plan.viewData()?.railings || []).find((x) => x.id === app.selection[0]);
    if (!r) return null;
    const flip = { label: "Flip", ico: "⇄", onclick: async () => { if (await run("patch_railing", { id: r.id, patch: { flipped: !r.flipped } })) { await refresh(); renderProperties(); } } };
    const items = r.hosted ? [flip] : [
      { label: "Edit Path", icon: "editBoundary", big: true, onclick: () => startSketch(r.id) },
      { label: "Pick New Host", icon: "pickHost", big: true, onclick: () => pickHost(r.id) },
      flip,
    ];
    return { auto: true, key: `railingsel:${r.id}:${r.flipped}`, title: "Railings", groups: [{ label: "Mode", items }] };
  }

  function elements(data) {
    return (data?.railings || []).filter((r) => r.id !== Sketch.editingId && r.rings.length).map((r) => ({ id: r.id, kind: "railing", category: "Railings", ring: r.rings[0], rings: r.rings, pinned: r.pinned, display: "normal" }));
  }

  function draw(ctx, data, toScreen, sel, pen) {
    ctx.save();
    for (const r of data?.railings || []) {
      if (r.id === Sketch.editingId) continue;
      const on = sel.has(r.id);
      for (const above of [false, true]) {
        ctx.beginPath();
        for (const p of r.polys) {
          if (p.above !== above) continue;
          p.pts.forEach((q, i) => { const s = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
          if (!p.open) ctx.closePath();
        }
        ctx.strokeStyle = on ? "#2e6f9e" : pen.colour(r.style, r.style.line.color);
        ctx.lineWidth = Math.max(1, pen.lw(r.style) * 0.8) + (on ? 0.8 : 0);
        ctx.setLineDash(above ? [5, 3] : pen.dash(r.style));
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  return { typeId, startSketch, finish, placeOnStair, placeClick, context, selectionContext, elements, draw, ensureTypes, pickHost, hostClick, hostContext };
})();
