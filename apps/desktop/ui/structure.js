// Structural columns and beams (Structure > Column and Beam; Architecture > Column places a structural column too).
// Column: each click stands a column of the chosen type at the point, from the plan's level up to the Top Level
// (or to a height). Beam: clicks from end to end, each beam starting where the last one ended, at the plan's level
// with its top on the level. The type, top, rotation, usage and offsets are in Properties while the tool runs.
// Brace: a beam from this level down to the one below, as vertical bracing. Beam System: click round a boundary
// (Enter or a click on the first point closes it); beams fill it along its first edge by the layout rule. Truss:
// two clicks for its span. Column > At Grids stands a column wherever grids cross.
// How a floor plan draws them, picks them and snaps to them. Private notes: structural.md.
const Structure = (() => {
  const items = () => Plan.viewData()?.structure || [];
  const st = { start: null, at: null, pts: [] };
  const TOOLS = ["scolumn", "sbeam", "sbrace", "sbsystem", "struss"];
  const owns = (tool) => TOOLS.includes(tool);
  // `use` is "column" or "beam"; the core calls a beam's types framing
  const types = (use) => (app.info.structural_types || []).filter((t) => t.used_for === (use === "column" ? "column" : "framing"));
  const opts = () => (app.structOpts ||= { column_type: null, beam_type: null, top: "above", height_mm: 3000, base_offset_mm: 0, top_offset_mm: 0, rotation_deg: 0, usage: "joist", z_justification: "top", z_offset_mm: 0, slanted: false,
    sys_rule: "fixed_distance", sys_spacing: 600, sys_count: 5, truss_type: null, truss_height: 1200, truss_profile: "parallel", truss_web: "pratt", truss_panels: 6 });
  const trussType = () => {
    const o = opts(), list = types("beam");
    if (!list.some((t) => t.id === o.truss_type)) o.truss_type = (list.find((t) => t.name.includes("RHS")) || list[0])?.id ?? null;
    return list.find((t) => t.id === o.truss_type) || null;
  };
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
    st.start = null; st.at = null; st.pts = [];
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool(tool);
    updateRibbonContext(true);
    renderProperties();
    setPrompt(promptText());
  }
  const promptText = () => {
    if (Plan.tool === "scolumn") {
      const t = typeOf("column"), top = topLevel();
      if (opts().slanted) return st.start ? `Slanted column (${t ? t.label : "no type"}): click where its top is, up at ${top ? top.name : `${opts().height_mm} above the level`}. Esc starts again.` : `Slanted column (${t ? t.label : "no type"}): click its base, then its top. Style is in Properties. Esc finishes.`;
      return `Column (${t ? t.label : "no type"}): click where it stands. It runs up to ${top ? top.name : `${opts().height_mm} above the level`}. Space turns it 90°. Type, top and rotation are in Properties. Esc finishes.`;
    }
    if (Plan.tool === "sbsystem") {
      const t = typeOf("beam");
      return st.pts.length ? `Beam System: click the next corner (${st.pts.length} so far). Enter, or a click on the first corner, closes it. The beams run along the first edge. Esc starts again.` : `Beam System (${t ? t.label : "no type"}): click round its boundary. The beams run along the first edge; the layout rule is in Properties. Esc finishes.`;
    }
    if (Plan.tool === "struss") {
      const t = trussType();
      return st.start ? `Truss: click the other end of its span. Esc starts again.` : `Truss (${t ? t.label : "no type"} members): click one end of its span. Height, profile and web layout are in Properties. Esc finishes.`;
    }
    if (Plan.tool === "sbrace") {
      const t = typeOf("beam");
      return st.start ? `Brace: click where it lands on the level below. Esc starts again.` : `Brace (${t ? t.label : "no type"}): click its top, on this level. It runs down to the level below. Esc finishes.`;
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
      if (o.slanted && !st.start) { st.start = { x: p.x, y: p.y }; setPrompt(promptText()); return; }
      const base = o.slanted ? st.start : p;
      const r = await run("add_structural_column", { type_id: t.id, level_id: curLevelId(), at: [base.x, base.y], base_offset_mm: o.base_offset_mm, rotation_deg: o.rotation_deg,
        ...(o.slanted ? { top_at: [p.x, p.y] } : {}),
        ...(top ? { top_level_id: top.id, top_offset_mm: o.top_offset_mm } : { height_mm: o.height_mm }) });
      st.start = null;
      if (!r) return;
      await refresh();
      setPrompt(`Column placed. ${promptText()}`);
      return;
    }
    if (Plan.tool === "sbsystem") {
      const first = st.pts[0];
      if (st.pts.length >= 3 && Math.hypot(p.x - first.x, p.y - first.y) < Math.max(50, 8 / (Plan.getCamera().s || 0.05))) return finishSystem();
      const last = st.pts[st.pts.length - 1];
      if (!last || Math.hypot(p.x - last.x, p.y - last.y) > 1) st.pts.push({ x: p.x, y: p.y });
      setPrompt(promptText()); Plan.render();
      return;
    }
    if (!st.start) { st.start = { x: p.x, y: p.y }; setPrompt(promptText()); return; }
    if (Math.hypot(p.x - st.start.x, p.y - st.start.y) < 1) return;
    if (Plan.tool === "struss") {
      const tt = trussType();
      if (!tt) return;
      const r = await run("add_truss", { level_id: curLevelId(), start: [st.start.x, st.start.y], end: [p.x, p.y], height_mm: o.truss_height, profile: o.truss_profile, web: o.truss_web, panels: o.truss_panels,
        top_chord_id: tt.id, bottom_chord_id: tt.id, web_type_id: tt.id });
      st.start = null;
      if (!r) return;
      await refresh();
      setPrompt(`Truss placed. ${promptText()}`);
      return;
    }
    const t = typeOf("beam");
    if (!t) return;
    if (Plan.tool === "sbrace") {
      const levels = [...(app.info.levels || [])].sort((a, b) => a.elevation_mm - b.elevation_mm);
      const here = levels.find((l) => l.id === curLevelId()), below = [...levels].reverse().find((l) => here && l.elevation_mm < here.elevation_mm - 1);
      const r = await run("add_beam", { type_id: t.id, level_id: curLevelId(), start: [st.start.x, st.start.y], end: [p.x, p.y], usage: "vertical_bracing", z_justification: "center",
        end_offset_mm: below ? below.elevation_mm - here.elevation_mm : -3000 });
      st.start = null;
      if (!r) return;
      await refresh();
      setPrompt(`Brace placed. ${promptText()}`);
      return;
    }
    const r = await run("add_beam", { type_id: t.id, level_id: curLevelId(), start: [st.start.x, st.start.y], end: [p.x, p.y], usage: o.usage, z_justification: o.z_justification, z_offset_mm: o.z_offset_mm });
    if (!r) return;
    st.start = { x: p.x, y: p.y };
    await refresh();
    setPrompt(`Beam placed. ${promptText()}`);
  }
  async function finishSystem() {
    const o = opts(), t = typeOf("beam"), pts = st.pts;
    if (!t || pts.length < 3) { setPrompt("A beam system's boundary needs three corners or more.", true); return; }
    const r = await run("add_beam_system", { type_id: t.id, level_id: curLevelId(), boundary: pts.map((q) => [q.x, q.y]), direction_deg: Math.atan2(pts[1].y - pts[0].y, pts[1].x - pts[0].x) * 180 / Math.PI,
      rule: o.sys_rule, spacing_mm: o.sys_spacing, count: o.sys_count, usage: o.usage });
    st.pts = [];
    if (!r) { Plan.render(); return; }
    await refresh();
    setPrompt(`Beam system placed. ${promptText()}`);
  }
  // Column > At Grids: a column of the tool's settings wherever grids cross
  async function atGrids() {
    const o = opts(), t = typeOf("column"), top = topLevel();
    if (!t) return;
    const r = await run("columns_at_grids", { type_id: t.id, level_id: curLevelId(), base_offset_mm: o.base_offset_mm, rotation_deg: o.rotation_deg, ...(top ? { top_level_id: top.id, top_offset_mm: o.top_offset_mm } : { height_mm: o.height_mm }) });
    if (!r) return;
    await refresh();
    setPrompt(`${r.ids.length} column${r.ids.length === 1 ? "" : "s"} placed where the grids cross. One undo takes them away.`);
  }
  async function dropTruss(id) {
    const r = await run("drop_truss", { id });
    if (!r) return;
    app.selection = []; Plan.setSelection([]);
    await refresh();
    setPrompt(`The truss is now ${r.ids.length} separate beams.`);
  }
  function hover(p) { st.at = p ? { x: p.x, y: p.y } : null; }
  function key(e) {
    if (e.key === " " && Plan.tool === "scolumn") { opts().rotation_deg = (opts().rotation_deg + 90) % 360; renderProperties(); Plan.render(); return true; }
    if (e.key === "Enter" && Plan.tool === "sbsystem" && st.pts.length >= 3) { finishSystem(); return true; }
    if (e.key !== "Escape") return false;
    if (st.pts.length) { st.pts = []; setPrompt(promptText()); Plan.render(); return true; }
    if (st.start) { st.start = null; setPrompt(promptText()); Plan.render(); return true; }
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
      for (const k of [key, "top", "usage", "z_justification", "slanted", "sys_rule", "truss_type", "truss_profile", "truss_web"]) if (patch[k] !== undefined) o[k] = patch[k];
      for (const k of ["sys_spacing", "sys_count", "truss_height", "truss_panels"]) if (Number.isFinite(patch[k]) && patch[k] > 0) o[k] = patch[k];
      if (patch.slanted !== undefined) st.start = null;
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
        pick("Column Style", o.slanted ? "Slanted" : "Vertical", [{ label: "Vertical", patch: { slanted: false }, selected: !o.slanted }, { label: "Slanted", patch: { slanted: true }, selected: o.slanted }]),
      ] }] };
    }
    if (Plan.tool === "sbsystem") {
      const rules = [["fixed_distance", "Fixed Distance"], ["fixed_number", "Fixed Number"], ["maximum_spacing", "Maximum Spacing"], ["clear_spacing", "Clear Spacing"]];
      return { title: "Structural Beam System", groups: [{ name: "Pattern", target: { apply }, rows: [
        type,
        pick("Layout Rule", rules.find((u) => u[0] === o.sys_rule)?.[1] || "", rules.map(([v, label]) => ({ label, patch: { sys_rule: v }, selected: o.sys_rule === v }))),
        o.sys_rule === "fixed_number" ? num("Number of Lines", "sys_count", o.sys_count) : num("Spacing", "sys_spacing", o.sys_spacing),
      ] }] };
    }
    if (Plan.tool === "struss") {
      const tt = trussType();
      const profiles = [["parallel", "Parallel Chords"], ["pitched", "Pitched"]], webs = [["pratt", "Pratt"], ["howe", "Howe"], ["warren", "Warren"]];
      return { title: "Structural Truss", groups: [{ name: "Layout", target: { apply }, rows: [
        pick("Members", tt ? tt.label : "", types("beam").map((t) => ({ label: t.label, patch: { truss_type: t.id }, selected: tt && t.id === tt.id }))),
        pick("Profile", profiles.find((u) => u[0] === o.truss_profile)?.[1] || "", profiles.map(([v, label]) => ({ label, patch: { truss_profile: v }, selected: o.truss_profile === v }))),
        pick("Web Layout", webs.find((u) => u[0] === o.truss_web)?.[1] || "", webs.map(([v, label]) => ({ label, patch: { truss_web: v }, selected: o.truss_web === v }))),
        num("Truss Height", "truss_height", o.truss_height),
        num("Panels", "truss_panels", o.truss_panels),
      ] }] };
    }
    if (Plan.tool === "sbrace") return { title: "Structural Brace", groups: [{ name: "Constraints", target: { apply }, rows: [type] }] };
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
    if (owns(Plan.tool)) {
      const title = { scolumn: "Structural Column", sbeam: "Beam", sbrace: "Brace", sbsystem: "Beam System", struss: "Truss" }[Plan.tool];
      const groups = [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] }];
      if (Plan.tool === "scolumn") groups.push({ label: "Multiple", labelled: true, items: [{ label: "At Grids", ico: "Gr", big: true, onclick: () => atGrids() }] });
      return { auto: true, key: Plan.tool, title, groups };
    }
    if (app.selection.length !== 1 || Plan.tool !== "select") return null;
    const it = items().find((x) => x.id === app.selection[0]);
    if (it && it.group === "truss") return { auto: true, key: `truss:${it.id}`, title: "Truss", groups: [{ label: "Truss", labelled: true, items: [{ label: "Drop Truss", ico: "Dr", big: true, onclick: () => dropTruss(it.id) }] }] };
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
      if (t.axis) {
        // a slanted column: its line from base to top
        const a = toScreen({ x: t.axis[0][0], y: t.axis[0][1] }), b = toScreen({ x: t.axis[1][0], y: t.axis[1][1] });
        ctx.setLineDash([6, 4]); ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
      if (t.boundary && on) {
        ctx.setLineDash([4, 4]); ctx.lineWidth = 1;
        ctx.beginPath(); path(ctx, t.boundary, toScreen); ctx.stroke();
      }
      if (t.kind === "beam") {
        const a = toScreen({ x: t.start[0], y: t.start[1] }), b = toScreen({ x: t.end[0], y: t.end[1] });
        ctx.setLineDash([12, 4, 2, 4]); ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
    }
    // what is being placed
    ctx.setLineDash([]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1;
    if (Plan.tool === "scolumn" && st.start && st.at) {
      const a = toScreen(st.start), b = toScreen(st.at);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    }
    if (Plan.tool === "scolumn" && st.at) {
      const t = typeOf("column");
      if (t) {
        const a = opts().rotation_deg * Math.PI / 180, cs = Math.cos(a), sn = Math.sin(a), w = t.width / 2, h = t.depth / 2;
        const ring = [[-w, -h], [w, -h], [w, h], [-w, h]].map(([x, y]) => [st.at.x + x * cs - y * sn, st.at.y + x * sn + y * cs]);
        ctx.beginPath(); path(ctx, ring, toScreen); ctx.stroke();
      }
    }
    if (Plan.tool === "sbsystem" && st.pts.length) {
      ctx.beginPath();
      [...st.pts, ...(st.at ? [st.at] : [])].forEach((q, i) => { const p = toScreen(q); i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y); });
      ctx.stroke();
    }
    if ((Plan.tool === "sbeam" || Plan.tool === "sbrace" || Plan.tool === "struss") && st.start && st.at) {
      const a = toScreen(st.start), b = toScreen(st.at);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      const len = Math.hypot(st.at.x - st.start.x, st.at.y - st.start.y);
      ctx.fillStyle = "#2e6f9e"; ctx.font = "11px sans-serif"; ctx.fillText(`${Math.round(len)}`, (a.x + b.x) / 2 + 6, (a.y + b.y) / 2 - 6);
    }
    ctx.restore();
  }

  return { owns, startBrace: () => start("sbrace"), startTruss: () => start("struss"), startSystem: () => start("sbsystem"), startColumn: () => start("scolumn"), startBeam: () => start("sbeam"), click, hover, key, promptText, placementProperties, selectionContext, elements, snapSegs, draw };
})();
