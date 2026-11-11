// Traffic (Site > Traffic): Parking Bay (two clicks along the front of the bays; angle, size, side and wheel stops in
// Properties), Line Marking and Kerb (sketched runs of lines and arcs), Edit Path, and how a floor plan draws them.
// They sit on the toposolid or floor under them. Private notes: site.md.
const Traffic = (() => {
  const items = () => Plan.viewData()?.traffic || [];
  const park = { start: null, at: null };
  // Kerb Ramp: its width and rise (150 suits the Kerb tool's kerbs)
  const rampOpts = () => (app.kerbRampOpts ||= { width_mm: 1200, rise_mm: 150 });
  const opts = () => (app.parkOpts ||= { angle_deg: 90, bay_width_mm: 2500, bay_length_mm: 5400, left: true, wheel_stops: true, accessible: false });
  const sk = { kind: null };
  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }
  async function ensurePlan() {
    const isPlan = app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "floor_plan";
    if (isPlan) return true;
    const views = app.info.views || [];
    const v = views.find((x) => x.view_kind === "floor_plan" && x.name === "Site") || views.find((x) => x.view_kind === "floor_plan");
    if (!v) { setPrompt("Traffic items are placed in a floor plan, and this project has none.", true); return false; }
    await openView("plan", v.id);
    return true;
  }

  // ---- Line Marking and Kerb: a sketched run ------------------------------------------------------------------------
  async function startSketch(kind, id = null) {
    if (Sketch.active) return;
    if (!(await ensurePlan())) return;
    let curves = [];
    if (id != null) { const r = await run("traffic_path", { id }); if (!r) return; curves = r.curves; }
    sk.kind = kind;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    const title = kind === "kerb" ? (id != null ? "Edit Kerb Path" : "Kerb") : id != null ? "Edit Line Marking Path" : "Line Marking";
    Plan.startSketch({ kind, id, levelId: curLevelId(), curves, tool: id != null ? "select" : "line", closed: false, title });
    renderSketchUI();
    renderProperties();
    setPrompt(kind === "kerb" ? "Kerb: sketch its centreline as one run of lines and arcs, then Finish. It sits on the ground under it." : "Line Marking: sketch the line as one run of lines and arcs, then Finish. Properties sets its width, paint and dashes.");
  }
  async function finish(o) {
    const r = o.id ? await run("set_traffic_path", { id: o.id, curves: o.curves })
      : o.kind === "kerb" ? await run("add_kerb", { level_id: o.levelId, curves: o.curves, height_mm: 150, width_mm: 150 })
      : await run("add_line_marking", { level_id: o.levelId, curves: o.curves });
    if (!r) return false;
    const id = o.id || r.id;
    sk.kind = null;
    Plan.endSketch(); endSketchUI();
    await refresh();
    select(id);
    setPrompt(o.id ? "Path updated." : o.kind === "kerb" ? "Kerb placed. Height and width are in Properties." : "Line marking placed.");
    return true;
  }

  // ---- Parking Bay: two clicks along the front of the bays ------------------------------------------------------------
  async function startParking(tool = "parkrow") {
    if (!(await ensurePlan())) return;
    park.start = null; park.at = null;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool(tool);
    updateRibbonContext(true);
    renderProperties();
    setPrompt(promptText());
  }
  const promptText = () => Plan.tool === "kerbramp"
    ? (park.start ? `Kerb Ramp: click its foot, down the slope. AS 1428.1 allows 1:8 at most, so ${rampOpts().rise_mm * 8} long for a ${rampOpts().rise_mm} rise. Esc cancels.` : "Kerb Ramp: click the middle of its top edge, at the kerb. Width and Rise are in Properties.")
    : park.start
    ? `Parking Bay: click the end of the row. The bays go on the ${opts().left ? "left" : "right"} going from start to end (Space flips). Esc cancels.`
    : "Parking Bay: click the start of the row, along the front of the bays (where cars nose in). Angle, bay size and wheel stops are in Properties.";
  async function click(p) {
    if (!p) return;
    if (!park.start) { park.start = { x: p.x, y: p.y }; setPrompt(promptText()); return; }
    if (Plan.tool === "kerbramp") {
      const r = await run("add_kerb_ramp", { level_id: curLevelId(), top: [park.start.x, park.start.y], bottom: [p.x, p.y], ...rampOpts() });
      if (!r) return;
      park.start = null;
      await refresh();
      const k = items().find((x) => x.id === r.id);
      setPrompt(k && !k.ok ? `Kerb ramp placed, but it breaks AS 1428.1: see its Properties. ${promptText()}` : `Kerb ramp placed (${k ? k.grade : ""}). ${promptText()}`);
      return;
    }
    const o = opts();
    const r = await run("add_parking_row", { level_id: curLevelId(), start: [park.start.x, park.start.y], end: [p.x, p.y], ...o });
    if (!r) return;
    park.start = null;
    await refresh();
    setPrompt(`Parking bays placed. ${promptText()}`);
  }
  function hover(p) { park.at = p ? { x: p.x, y: p.y } : null; }
  function key(e) {
    if (e.key === " " && Plan.tool === "kerbramp") return true;
    if (e.key === " " && Plan.tool === "parkrow") { opts().left = !opts().left; renderProperties(); setPrompt(promptText()); Plan.render(); return true; }
    if (e.key !== "Escape") return false;
    if (park.start) { park.start = null; setPrompt(promptText()); Plan.render(); return true; }
    Plan.setTool("select");
    return true;
  }
  function placementProperties() {
    if (Plan.tool === "kerbramp") {
      const o = rampOpts();
      return { title: "Kerb Ramp", groups: [{ name: "Dimensions", target: { apply: (patch) => {
        for (const k of ["width_mm", "rise_mm"]) if (Number.isFinite(patch[k]) && patch[k] > 0) o[k] = patch[k];
        renderProperties(); setPrompt(promptText()); Plan.render();
      } }, rows: [
        { label: "Width", kind: "number", raw: o.width_mm, value: String(o.width_mm), path: "width_mm" },
        { label: "Rise", kind: "number", raw: o.rise_mm, value: String(o.rise_mm), path: "rise_mm" },
      ] }] };
    }
    const o = opts();
    const num = (label, path, v) => ({ label, kind: "number", raw: v, value: String(v), path });
    return { title: "Parking Bay", groups: [{ name: "Layout", target: { apply: (patch) => {
      for (const k of ["angle_deg", "bay_width_mm", "bay_length_mm"]) if (Number.isFinite(patch[k])) o[k] = patch[k];
      for (const k of ["left", "wheel_stops"]) if (typeof patch[k] === "boolean") o[k] = patch[k];
      // accessible spaces (AS 2890.6:2022): 2700 by 5500, each with a shared area as big beside it
      if (typeof patch.accessible === "boolean") { o.accessible = patch.accessible; if (o.accessible) { o.bay_width_mm = Math.max(o.bay_width_mm, 2700); o.bay_length_mm = Math.max(o.bay_length_mm, 5500); } }
      renderProperties(); Plan.render();
    } }, rows: [
      num("Angle", "angle_deg", o.angle_deg),
      num("Bay Width", "bay_width_mm", o.bay_width_mm),
      num("Bay Length", "bay_length_mm", o.bay_length_mm),
      { label: "Bays on the Left", kind: "bool", raw: o.left, value: o.left ? "Yes" : "No", path: "left" },
      { label: "Wheel Stops", kind: "bool", raw: o.wheel_stops, value: o.wheel_stops ? "Yes" : "No", path: "wheel_stops" },
      { label: "Accessible", kind: "bool", raw: o.accessible, value: o.accessible ? "Yes" : "No", path: "accessible" },
    ] }] };
  }
  // the row being placed: its line, and the depth the bays take on their side
  function drawPreview(ctx, toScreen) {
    if (Plan.tool === "kerbramp" && park.start && park.at) {
      // the ramp being placed, with its grade: red when steeper than 1:8
      const a = park.start, b = park.at, l = Math.hypot(b.x - a.x, b.y - a.y), o = rampOpts();
      if (l < 1) return;
      const s = { x: -(b.y - a.y) / l * o.width_mm / 2, y: (b.x - a.x) / l * o.width_mm / 2 };
      const q = [{ x: a.x - s.x, y: a.y - s.y }, { x: a.x + s.x, y: a.y + s.y }, { x: b.x + s.x, y: b.y + s.y }, { x: b.x - s.x, y: b.y - s.y }].map(toScreen);
      const ok = l / o.rise_mm >= 8 - 1e-6 && o.rise_mm <= 190 && l <= 1520;
      ctx.save(); ctx.strokeStyle = ok ? "#c0369a" : "#d33"; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]);
      ctx.beginPath(); q.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y))); ctx.closePath(); ctx.stroke();
      const m = toScreen({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
      ctx.fillStyle = ok ? "#c0369a" : "#d33"; ctx.font = "12px system-ui"; ctx.textAlign = "center"; ctx.fillText(`1:${(l / o.rise_mm).toFixed(1)}  ${Math.round(l)}`, m.x, m.y - 6);
      ctx.restore();
      return;
    }
    if (Plan.tool !== "parkrow" || !park.start || !park.at) return;
    const o = opts(), a = park.start, b = park.at, l = Math.hypot(b.x - a.x, b.y - a.y);
    if (l < 1) return;
    const u = { x: (b.x - a.x) / l, y: (b.y - a.y) / l }, n = o.left ? { x: -u.y, y: u.x } : { x: u.y, y: -u.x };
    const ang = Math.max(1, o.angle_deg) * Math.PI / 180, depth = o.angle_deg < 1 ? o.bay_width_mm : o.bay_length_mm * Math.sin(ang);
    const pitch = o.angle_deg < 1 ? o.bay_length_mm : o.bay_width_mm / Math.sin(ang), lean = o.angle_deg < 1 ? 0 : o.bay_length_mm * Math.cos(ang);
    ctx.save(); ctx.strokeStyle = "#c0369a"; ctx.lineWidth = 1.2; ctx.setLineDash([6, 4]);
    const P = (s, d) => toScreen({ x: a.x + u.x * s + n.x * d, y: a.y + u.y * s + n.y * d });
    ctx.beginPath();
    for (let s = 0; s + pitch + lean <= l + 1e-6; s += pitch) { const p = P(s, 0), q = P(s + lean, depth); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); }
    const A = toScreen(a), B = toScreen(b); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y);
    ctx.stroke(); ctx.restore();
  }

  // ---- selection, picking and drawing ------------------------------------------------------------------------------
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (Plan.tool === "parkrow" || Plan.tool === "kerbramp") return { auto: true, key: Plan.tool, title: Plan.tool === "kerbramp" ? "Kerb Ramp" : "Parking Bay", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] }] };
    if (app.selection.length !== 1 || Plan.tool !== "select") return null;
    const it = items().find((x) => x.id === app.selection[0]);
    if (!it || it.kind === "parkingrow" || it.kind === "kerbramp") return null;
    return { auto: true, key: `traffic:${it.id}`, title: it.kind === "kerb" ? "Kerbs" : "Line Markings", groups: [
      { label: "Mode", labelled: true, items: [{ label: "Edit Path", icon: "editBoundary", big: true, onclick: () => startSketch(it.kind, it.id) }] },
    ] };
  }
  // a run picks along its length (each piece there and back, so it has no inside); a parking row by its bays' outline
  function elements(d) {
    return (d?.traffic || []).filter((t) => t.id !== Sketch.editingId).map((t) => {
      const rings = t.kind === "kerbramp" ? [t.outline]
        : t.kind === "parkingrow" && t.bays.length
        ? [[t.bays[0][0], t.bays[t.bays.length - 1][1], t.bays[t.bays.length - 1][2], t.bays[0][3]]]
        : (t.kind === "kerb" ? [t.path] : t.lines).map((l) => [...l, ...l.slice().reverse()]);
      return { id: t.id, kind: t.kind, category: t.kind === "kerb" || t.kind === "kerbramp" ? "Hardscape" : "Parking", ring: rings[0], rings, pinned: t.pinned, display: "normal" };
    });
  }
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null, pen = null) {
    ctx.save();
    for (const t of d?.traffic || []) {
      if (t.id === Sketch.editingId) continue;
      const on = sel.has(t.id), st = t.style;
      if (tag) tag({ cat: t.kind === "kerb" ? "Hardscape" : "Parking", sub: null, kind: "projection" });
      // painted lines as wide as they are (at least a pixel); yellow paint shows yellow
      const ink = on ? "#2e6f9e" : t.paint === "Yellow" ? "#c99a00" : pen ? pen.colour(st, st.line.color) : st.line.color;
      ctx.strokeStyle = ink; ctx.lineCap = "butt"; ctx.setLineDash([]);
      ctx.lineWidth = t.kind === "kerb" ? (pen ? pen.lw(st) : 1) : Math.max(pen ? pen.lw(st) : 1, (t.width || 80) * k);
      ctx.beginPath();
      for (const l of t.lines) l.forEach((q, i) => { const p = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); });
      ctx.stroke();
      // accessible spaces: the symbol of access, white on a blue square
      for (const c of t.symbols || []) {
        const sc = c.map((q) => toScreen({ x: q[0], y: q[1] }));
        ctx.beginPath(); sc.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y))); ctx.closePath();
        ctx.fillStyle = on ? "#2e6f9e" : "#1e5ab4"; ctx.fill();
        const side = Math.hypot(sc[1].x - sc[0].x, sc[1].y - sc[0].y), cx = (sc[0].x + sc[2].x) / 2, cy = (sc[0].y + sc[2].y) / 2;
        if (side > 8) { ctx.fillStyle = "#fff"; ctx.font = `${Math.round(side * 0.75)}px sans-serif`; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillText("♿", cx, cy); }
      }
      // a kerb ramp: an arrow down the slope and its grade, red when it breaks AS 1428.1
      if (t.kind === "kerbramp") {
        const a = toScreen({ x: t.top[0], y: t.top[1] }), b = toScreen({ x: t.bottom[0], y: t.bottom[1] });
        const col = on ? "#2e6f9e" : t.ok ? ink : "#d33";
        ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
        const ang = Math.atan2(b.y - a.y, b.x - a.x);
        ctx.beginPath(); ctx.moveTo(b.x, b.y); ctx.lineTo(b.x - 8 * Math.cos(ang - 0.4), b.y - 8 * Math.sin(ang - 0.4)); ctx.lineTo(b.x - 8 * Math.cos(ang + 0.4), b.y - 8 * Math.sin(ang + 0.4)); ctx.closePath(); ctx.fill();
        const px = Math.max(8, 2 * ((d.view && d.view.scale) || 100) * k);
        ctx.font = `${px}px sans-serif`; ctx.textAlign = "center"; ctx.textBaseline = "bottom"; ctx.fillText(t.grade, (a.x + b.x) / 2, (a.y + b.y) / 2 - 3);
      }
      // wheel stops: small filled blocks
      if (t.stops) {
        ctx.fillStyle = on ? "#2e6f9e" : "#555";
        for (const c of t.stops) { ctx.beginPath(); c.forEach((q, i) => { const p = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); ctx.closePath(); ctx.fill(); }
      }
    }
    if (tag) tag(null);
    ctx.restore();
    drawPreview(ctx, toScreen);
  }

  return { startSketch, finish, startParking, startKerbRamp: () => startParking("kerbramp"), click, hover, key, promptText, placementProperties, selectionContext, elements, draw, sketching: () => sk.kind };
})();
