// Strip footings (Structure > Strip Footing, Revit's Wall Foundation): placed under walls, by the walls selected when
// the tool starts or one click per wall after. A plan of the walls' level draws them dashed (they lie below the cut),
// all of them as one outline so corners read as one footing; each is picked by its own outline (the core:
// foundation.rs).
const Foundations = (() => {
  function draw(ctx, data, toScreen, k, sel = new Set()) {
    const outline = data?.foundation_outline || [];
    if (!outline.length) return;
    ctx.save();
    ctx.strokeStyle = "#6b6f76"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
    for (const shape of outline) for (const ring of shape) {
      ctx.beginPath();
      ring.forEach(([x, y], i) => { const s = toScreen({ x, y }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
      ctx.closePath(); ctx.stroke();
    }
    ctx.setLineDash([]);
    for (const f of data.wall_foundations || []) {
      if (!sel.has(f.id)) continue;
      ctx.beginPath();
      f.ring.forEach(([x, y], i) => { const s = toScreen({ x, y }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
      ctx.closePath();
      ctx.fillStyle = "rgba(46,111,158,0.15)"; ctx.fill();
      ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 2; ctx.stroke();
    }
    ctx.restore();
  }
  const elements = (data) => (data?.wall_foundations || []).map((f) => ({ id: f.id, kind: "foundation", category: "Structural Foundations", ring: f.ring, pinned: f.pinned, display: "normal" }));

  async function add(walls) {
    const r = await run("add_wall_foundations", { walls });
    if (!r) return false;
    await refresh();
    setPrompt(`${r.ids.length === 1 ? "A strip footing" : `${r.ids.length} strip footings`} added. Click more walls, or Esc.`);
    return true;
  }
  async function start() {
    if (!(await ensurePlan("A strip footing"))) return;
    const walls = (Plan.selectionInfo?.().Walls || []).slice();
    Plan.setTool("wallfoot");
    updateRibbonContext?.(true);
    if (walls.length) await add(walls);
    else setPrompt("Click a wall to put a strip footing under it. Esc finishes.");
  }
  async function click(wallId) {
    if (wallId == null) { setPrompt("Click a wall: strip footings go under walls.", true); return; }
    await add([wallId]);
  }
  // Isolated Footing: under the structural columns selected when it starts (At Columns), else one per click, its
  // centre on the click and its top at the plan's level
  async function startIsolated() {
    if (!(await ensurePlan("An isolated footing"))) return;
    const cols = (Plan.selectionInfo?.()["Structural Columns"] || []).slice();
    if (cols.length) {
      const r = await run("add_isolated_footings", { columns: cols });
      if (r) { await refresh(); setPrompt(`${r.ids.length === 1 ? "A footing" : `${r.ids.length} footings`} placed under the selected columns.`); }
      return;
    }
    Plan.setTool("isofoot");
    setPrompt("Click where the footing's centre goes. Esc finishes.");
  }
  async function place(p) {
    const level = (app.info.views || []).find((v) => v.id === app.view.viewId)?.level_id;
    if (level == null) { setPrompt("Isolated footings are placed in a floor plan.", true); return; }
    if (await run("add_isolated_footings", { level_id: level, points: [[p.x, p.y]] })) await refresh();
  }
  return { draw, elements, start, click, startIsolated, place };
})();
