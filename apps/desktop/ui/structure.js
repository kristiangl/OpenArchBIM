// Structural columns and beams (Structure > Column and Beam; Architecture > Column places a structural column too).
// Column: each click stands a column of the chosen type at the point, from the plan's level up to the Top Level
// (or to a height). Beam: clicks from end to end, each beam starting where the last one ended, at the plan's level
// with its top on the level. The type, top, rotation, usage and offsets are in Properties while the tool runs.
// How a floor plan draws them, picks them and snaps to them. Private notes: structural.md.
const Structure = (() => {
  const items = () => Plan.viewData()?.structure || [];
  const st = { start: null, at: null };
  // `use` is "column" or "beam"; the core calls a beam's types framing
  const types = (use) => (app.info.structural_types || []).filter((t) => t.used_for === (use === "column" ? "column" : "framing"));
  const opts = () => (app.structOpts ||= { column_type: null, beam_type: null, top: "above", height_mm: 3000, base_offset_mm: 0, top_offset_mm: 0, rotation_deg: 0, usage: "joist", z_justification: "top", z_offset_mm: 0 });
  const typeOf = (use) => {
    const o = opts(), key = use === "column" ? "column_type" : "beam_type", list = types(use);
    if (!list.some((t) => t.id === o[key])) o[key] = (use === "column" ? list.find((t) => t.name === "200UC46.2") : list.find((t) => t.name === "310UB40.4"))?.id ?? list[0]?.id ?? null;
    return list.find((t) => t.id === o[key]) || null;
  };
  // the level a new column runs up to: the next one above the plan's, a chosen one, or none (a height instead)
  function topLevel() {
    const o = opts(), levels = [...(app.info.levels || [])].sort((a, b) => a.elevation_mm - b.elevation_mm);
    const here = levels.find((l) => l.id === curLevelId());
    if (o.top === "above") return levels.find((l) => here && l.elevation_mm > here.elevation_mm + 1) || null;
    if (o.top === "unconnected") return null;
    const l = levels.find((x) => x.id === o.top);
    return l && here && l.elevation_mm + o.top_offset_mm > here.elevation_mm + o.base_offset_mm ? l : null;
  }

  async function ensure() {
    const isPlan = app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "floor_plan";
    if (!isPlan) {
      const v = (app.info.views || []).find((x) => x.view_kind === "floor_plan");
      if (!v) { setPrompt("Columns and beams are placed in a floor plan, and this project has none.", true); return false; }
      await openView("plan", v.id);
    }
    if (!(app.info.structural_types || []).length) {
      if (!(await run("ensure_structural_types", {}))) return false;
      await refresh();
    }
    return true;
  }
  async function start(tool) {
    if (!(await ensure())) return;
    st.start = null; st.at = null;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool(tool);
    updateRibbonContext(true);
    renderProperties();
    setPrompt(promptText());
  }
  const promptText = () => {
    if (Plan.tool === "scolumn") {
      const t = typeOf("column"), top = topLevel();
      return `Column (${t ? t.label : "no type"}): click where it stands. It runs up to ${top ? top.name : `${opts().height_mm} above the level`}. Space turns it 90°. Type, top and rotation are in Properties. Esc finishes.`;
    }
    const t = typeOf("beam");
    return st.start ? `Beam (${t ? t.label : "no type"}): click its other end. The next beam starts there. Esc ends the run.` : `Beam (${t ? t.label : "no type"}): click its start. Its top sits on this level; usage and offsets are in Properties. Esc finishes.`;
  };

  async function click(p) {
    if (!p) return;
    const o = opts();
    if (Plan.tool === "scolumn") {
      const t = typeOf("column"), top = topLevel();
      if (!t) return;
      const r = await run("add_structural_column", { type_id: t.id, level_id: curLevelId(), at: [p.x, p.y], base_offset_mm: o.base_offset_mm, rotation_deg: o.rotation_deg,
        ...(top ? { top_level_id: top.id, top_offset_mm: o.top_offset_mm } : { height_mm: o.height_mm }) });
      if (!r) return;
      await refresh();
      setPrompt(`Column placed. ${promptText()}`);
      return;
    }
    if (!st.start) { st.start = { x: p.x, y: p.y }; setPrompt(promptText()); return; }
    const t = typeOf("beam");
    if (!t) return;
    if (Math.hypot(p.x - st.start.x, p.y - st.start.y) < 1) return;
    const r = await run("add_beam", { type_id: t.id, level_id: curLevelId(), start: [st.start.x, st.start.y], end: [p.x, p.y], usage: o.usage, z_justification: o.z_justification, z_offset_mm: o.z_offset_mm });
    if (!r) return;
    st.start = { x: p.x, y: p.y };
    await refresh();
    setPrompt(`Beam placed. ${promptText()}`);
  }
  function hover(p) { st.at = p ? { x: p.x, y: p.y } : null; }
  function key(e) {
    if (e.key === " " && Plan.tool === "scolumn") { opts().rotation_deg = (opts().rotation_deg + 90) % 360; renderProperties(); Plan.render(); return true; }
    if (e.key !== "Escape") return false;
    if (Plan.tool === "sbeam" && st.start) { st.start = null; setPrompt(promptText()); Plan.render(); return true; }
    Plan.setTool("select");
    return true;
  }

  function placementProperties() {
    const o = opts(), column = Plan.tool === "scolumn";
    const use = column ? "column" : "beam", key = column ? "column_type" : "beam_type";
    const cur = typeOf(use);
    const num = (label, path, v) => ({ label, kind: "number", raw: v, value: String(v), path });
    const pick = (label, value, options) => ({ label, kind: "select", value, options });
    const apply = (patch) => {
      for (const k of [key, "top", "usage", "z_justification"]) if (patch[k] !== undefined) o[k] = patch[k];
      for (const k of ["height_mm", "base_offset_mm", "top_offset_mm", "rotation_deg", "z_offset_mm"]) if (Number.isFinite(patch[k])) o[k] = patch[k];
      renderProperties(); setPrompt(promptText()); Plan.render();
    };
    const type = pick("Type", cur ? cur.label : "", types(use).map((t) => ({ label: t.label, patch: { [key]: t.id }, selected: cur && t.id === cur.id })));
    if (column) {
      const top = topLevel();
      const tops = [{ label: "Level Above", patch: { top: "above" }, selected: o.top === "above" }, { label: "Unconnected", patch: { top: "unconnected" }, selected: o.top === "unconnected" },
        ...(app.info.levels || []).map((l) => ({ label: l.name, patch: { top: l.id }, selected: o.top === l.id }))];
      return { title: "Structural Column", groups: [{ name: "Constraints", target: { apply }, rows: [
        type,
        pick("Top", top ? top.name : "Unconnected", tops),
        ...(top ? [num("Top Offset", "top_offset_mm", o.top_offset_mm)] : [num("Unconnected Height", "height_mm", o.height_mm)]),
        num("Base Offset", "base_offset_mm", o.base_offset_mm),
        num("Rotation", "rotation_deg", o.rotation_deg),
      ] }] };
    }
    const usages = [["girder", "Girder"], ["joist", "Joist"], ["purlin", "Purlin"], ["horizontal_bracing", "Horizontal Bracing"], ["vertical_bracing", "Vertical Bracing"], ["other", "Other"]];
    const zj = [["top", "Top"], ["center", "Center"], ["bottom", "Bottom"]];
    return { title: "Structural Framing", groups: [{ name: "Constraints", target: { apply }, rows: [
      type,
      pick("Structural Usage", usages.find((u) => u[0] === o.usage)?.[1] || "", usages.map(([v, label]) => ({ label, patch: { usage: v }, selected: o.usage === v }))),
      pick("z Justification", zj.find((u) => u[0] === o.z_justification)?.[1] || "", zj.map(([v, label]) => ({ label, patch: { z_justification: v }, selected: o.z_justification === v }))),
      num("z Offset Value", "z_offset_mm", o.z_offset_mm),
    ] }] };
  }

  // the tool's ribbon panel, and nothing extra for a selected member (its Properties do the rest)
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (Plan.tool === "scolumn" || Plan.tool === "sbeam") return { auto: true, key: Plan.tool, title: Plan.tool === "scolumn" ? "Structural Column" : "Beam", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] }] };
    return null;
  }

  function elements(d) {
    return (d?.structure || []).map((t) => ({ id: t.id, kind: t.kind === "column" ? "scolumn" : "sbeam", category: t.kind === "column" ? "Structural Columns" : "Structural Framing", ring: t.outline, rings: [t.outline], pinned: t.pinned, display: t.display }));
  }
  // what the snap index takes: outlines, a beam's line, and a cross through a column's centre
  function snapSegs(d) {
    const out = [];
    for (const t of d?.structure || []) {
      const o = t.outline;
      for (let i = 0; i < o.length; i++) { const a = o[i], b = o[(i + 1) % o.length]; out.push([a[0], a[1], b[0], b[1]]); }
      if (t.kind === "beam") out.push([t.start[0], t.start[1], t.end[0], t.end[1]]);
      else { const r = 150; out.push([t.at[0] - r, t.at[1], t.at[0] + r, t.at[1]], [t.at[0], t.at[1] - r, t.at[0], t.at[1] + r]); }
    }
    return out;
  }

  const path = (ctx, ring, toScreen) => { ring.forEach((q, i) => { const p = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); }); ctx.closePath(); };
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null, pen = null) {
    ctx.save();
    for (const t of d?.structure || []) {
      const on = sel.has(t.id), s = t.style;
      if (tag) tag({ cat: t.kind === "column" ? "Structural Columns" : "Structural Framing", sub: null, kind: t.display });
      const ink = on ? "#2e6f9e" : pen ? pen.colour(s, s.line.color) : s.line.color;
      ctx.strokeStyle = ink; ctx.lineWidth = pen ? pen.lw(s) : 1;
      ctx.setLineDash(t.kind === "beam" && t.display !== "cut" ? [8, 4] : []);
      ctx.beginPath();
      path(ctx, t.outline, toScreen);
      if (t.inner) path(ctx, t.inner, toScreen);
      // a cut column is filled (a hollow one round its wall only)
      if (t.kind === "column" && t.display === "cut") { ctx.fillStyle = on ? "rgba(46,111,158,0.35)" : "rgba(90,90,90,0.55)"; ctx.fill("evenodd"); }
      ctx.stroke();
      if (t.kind === "beam") {
        const a = toScreen({ x: t.start[0], y: t.start[1] }), b = toScreen({ x: t.end[0], y: t.end[1] });
        ctx.setLineDash([12, 4, 2, 4]); ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
    }
    // what is being placed
    ctx.setLineDash([]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1;
    if (Plan.tool === "scolumn" && st.at) {
      const t = typeOf("column");
      if (t) {
        const a = opts().rotation_deg * Math.PI / 180, cs = Math.cos(a), sn = Math.sin(a), w = t.width / 2, h = t.depth / 2;
        const ring = [[-w, -h], [w, -h], [w, h], [-w, h]].map(([x, y]) => [st.at.x + x * cs - y * sn, st.at.y + x * sn + y * cs]);
        ctx.beginPath(); path(ctx, ring, toScreen); ctx.stroke();
      }
    }
    if (Plan.tool === "sbeam" && st.start && st.at) {
      const a = toScreen(st.start), b = toScreen(st.at);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      const len = Math.hypot(st.at.x - st.start.x, st.at.y - st.start.y);
      ctx.fillStyle = "#2e6f9e"; ctx.font = "11px sans-serif"; ctx.fillText(`${Math.round(len)}`, (a.x + b.x) / 2 + 6, (a.y + b.y) / 2 - 6);
    }
    ctx.restore();
  }

  return { startColumn: () => start("scolumn"), startBeam: () => start("sbeam"), click, hover, key, promptText, placementProperties, selectionContext, elements, snapSegs, draw };
})();
