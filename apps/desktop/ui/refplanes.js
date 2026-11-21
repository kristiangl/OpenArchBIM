// Reference planes (Architecture > Ref Plane): upright planes drawn in plan by two clicks, shown in every plan as a
// dashed line with its name at the end, picked and snapped to like a line (the core: refplane.rs).
const RefPlanes = (() => {
  const st = { a: null, at: null };
  const COLOUR = "#3b8f5a";

  function draw(ctx, data, toScreen, k, sel = new Set(), tag = null) {
    const list = data?.ref_planes || [];
    ctx.save();
    ctx.font = "11px system-ui, sans-serif";
    for (const r of list) {
      const a = toScreen({ x: r.a[0], y: r.a[1] }), b = toScreen({ x: r.b[0], y: r.b[1] }), on = sel.has(r.id);
      ctx.strokeStyle = on ? "#2e6f9e" : tag && tag.has?.(r.id) ? "#d4572f" : COLOUR;
      ctx.lineWidth = on ? 2 : 1;
      ctx.setLineDash([8, 4]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      if (r.name) {
        ctx.setLineDash([]); ctx.fillStyle = ctx.strokeStyle;
        const ang = Math.atan2(b.y - a.y, b.x - a.x), up = Math.abs(ang) > Math.PI / 2 ? ang + Math.PI : ang;
        ctx.save(); ctx.translate(b.x, b.y); ctx.rotate(up);
        ctx.textAlign = Math.abs(ang) > Math.PI / 2 ? "right" : "left"; ctx.textBaseline = "bottom";
        ctx.fillText(r.name, Math.abs(ang) > Math.PI / 2 ? -6 : 6, -2);
        ctx.restore();
      }
    }
    // the plane being drawn
    if (Plan.tool === "refplane" && st.a && st.at) {
      const a = toScreen(st.a), b = toScreen(st.at);
      ctx.strokeStyle = COLOUR; ctx.lineWidth = 1; ctx.setLineDash([8, 4]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    }
    ctx.restore();
  }
  function elements(data, k) {
    const t = 4 / k;
    return (data?.ref_planes || []).map((r) => {
      const [ax, ay] = r.a, [bx, by] = r.b, l = Math.hypot(bx - ax, by - ay) || 1, nx = (-(by - ay) / l) * t, ny = ((bx - ax) / l) * t;
      return { id: r.id, kind: "refplane", category: "Reference Planes", ring: [[ax + nx, ay + ny], [bx + nx, by + ny], [bx - nx, by - ny], [ax - nx, ay - ny]], pinned: r.pinned, display: "normal" };
    });
  }
  // the snaps take the planes as straight lines
  const snapSegs = (data) => (data?.ref_planes || []).map((r) => [r.a[0], r.a[1], r.b[0], r.b[1], 4]);

  async function start() {
    if (!(await ensurePlan("A reference plane"))) return;
    st.a = null; st.at = null;
    Plan.setTool("refplane");
    setPrompt(promptText());
  }
  const promptText = () => (st.a ? "Click the plane's end. Esc starts again." : "Click the start of the reference plane. Esc cancels.");
  const last = () => st.a;
  function hover(p) { st.at = p ? { x: p.x, y: p.y } : null; }
  async function click(p) {
    if (!st.a) { st.a = { x: p.x, y: p.y }; setPrompt(promptText()); return; }
    const a = st.a;
    if (Math.hypot(p.x - a.x, p.y - a.y) < 1) return;
    // cleared first, so a click while this is saved starts the next plane
    st.a = null;
    const r = await run("add_ref_plane", { a: [a.x, a.y], b: [p.x, p.y] });
    if (r) { await refresh(); setPrompt("Reference plane added. Click the start of the next one, or Esc."); }
  }
  function key(e) {
    if (e.key !== "Escape") return false;
    if (st.a) { st.a = null; setPrompt(promptText()); Plan.render(); return true; }
    Plan.setTool("select");
    setPrompt("Ready.");
    return true;
  }
  return { draw, elements, snapSegs, start, hover, click, key, last, promptText };
})();
