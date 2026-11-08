// Toposolids: Create from Sketch, Create from Import (a points file), Add Point, Modify Sub Elements, Simplify, Edit
// Boundary, and how a floor plan draws them (the boundary, the type's contours, and the ground its cut plane passes
// through). Private notes: site.md.
const Topo = (() => {
  // Add Point's settings: the height, measured from a base or above the surface where the point goes
  const pt = { elevation: 0, mode: "absolute", base: "current_level", target: null };
  // Modify Sub Elements: the toposolid, the picked points and boundary corners (by key), and a drag or window
  const ed = { id: null, sel: new Set(), drag: null, box: null };
  const typeId = () => {
    const ts = app.info.toposolid_types || [];
    if (!ts.some((t) => t.id === app.topoTypeId)) app.topoTypeId = ts[0]?.id;
    return app.topoTypeId;
  };
  const offset = () => (Number.isFinite(app.topoOffset) ? app.topoOffset : 0);
  const isFloorPlan = () => app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "floor_plan";
  const data = () => Plan.viewData()?.toposolids || [];

  async function ensureTypes() {
    if ((app.info.toposolid_types || []).length) return true;
    if (!(await run("ensure_toposolid_types", {}))) return false;
    await refresh();
    return true;
  }

  // a toposolid is sketched in a floor plan: stay in one, or open the Site plan
  async function ensureFloorPlan() {
    if (isFloorPlan()) return true;
    const views = app.info.views || [];
    const v = views.find((x) => x.view_kind === "floor_plan" && x.name === "Site") || views.find((x) => x.view_kind === "floor_plan");
    if (!v) { setPrompt("A toposolid is sketched in a floor plan, and this project has none."); return false; }
    await openView("plan", v.id);
    return true;
  }

  async function startSketch(id) {
    if (Sketch.active) return;
    if (!(await ensureTypes())) return;
    if (id != null) {
      const r = await run("toposolid_sketch", { id });
      if (!r) return;
      Plan.startSketch({ kind: "toposolid", id, levelId: r.level_id, typeId: r.type_id, curves: r.curves, tool: "select", title: "Edit Toposolid Boundary" });
    } else {
      if (!(await ensureFloorPlan())) return;
      typeId();
      app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
      Plan.startSketch({ kind: "toposolid", levelId: curLevelId(), typeId: typeId(), curves: [], tool: "line", title: "Create Toposolid Boundary" });
    }
    renderSketchUI();
    renderProperties();
  }

  async function finish(o) {
    const r = o.id
      ? await run("set_toposolid_sketch", { id: o.id, curves: o.curves })
      : await run("add_toposolid", { type_id: typeId(), level_id: o.levelId, offset_mm: offset(), curves: o.curves });
    if (!r) return false;
    const id = o.id || r.id;
    Plan.endSketch(); endSketchUI();
    await refresh();
    select(id);
    setPrompt(o.id ? "Toposolid boundary updated." : "Toposolid created. Add Point (Modify | Toposolid) shapes it.");
    return true;
  }

  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }

  // Create from Import: a points file (x y z, or a point number then x y z), its units, and where its coordinates
  // are measured from. The boundary is the points' outline.
  async function importFile() {
    if (!(await ensureTypes())) return;
    const path = await Backend.pickOpenPoints();
    if (!path) return;
    openModal("Create Toposolid from Points File", (body, foot, close) => {
      const units = el("select", {}, [new Option("Metres", "1000"), new Option("Millimetres", "1"), new Option("Feet", "304.8")]);
      const base = el("select", {}, [new Option("Survey Point (shared coordinates)", "survey_point"), new Option("Project Base Point", "project_base_point"), new Option("Internal Origin", "internal_origin")]);
      const levels = app.info.levels || [];
      const here = app.view.kind === "plan" ? curLevelId() : levels[0]?.id;
      const level = el("select", {}, levels.map((l) => new Option(l.name, l.id, false, l.id === here)));
      const types = el("select", {}, (app.info.toposolid_types || []).map((t) => new Option(t.name, t.id, false, t.id === typeId())));
      const msg = el("div", { className: "modal-msg" });
      const row = (l, c) => el("label", { className: "new-row" }, [el("span", { textContent: l }), c]);
      body.append(el("div", { className: "modal-msg", textContent: path }), row("File units", units), row("Coordinates from", base), row("Level", level), row("Type", types), msg);
      const ok = button("OK", async () => {
        const r = await Backend.call("import_toposolid", { path, scale: Number(units.value), base: base.value, level_id: Number(level.value), type_id: Number(types.value) }).catch((e) => { msg.textContent = String(e.message || e); return null; });
        if (!r) return;
        close();
        app.topoTypeId = Number(types.value);
        await refresh();
        select(r.id);
        setPrompt(`Toposolid created from ${r.read} point${r.read === 1 ? "" : "s"}${r.kept < r.read ? ` (thinned to ${r.kept})` : ""}.`);
      }, true);
      foot.append(button("Cancel", close), ok);
    });
  }

  // ---- a selected toposolid: Edit Boundary and Add Point ----------------------------------------------------------
  function selected() {
    if (app.view.kind !== "plan" || app.selection.length !== 1) return null;
    return data().find((t) => t.id === app.selection[0]) || null;
  }
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (Plan.tool === "topopoint") return pointContext();
    if (Plan.tool === "topoedit") return editContext();
    const t = selected();
    if (!t || Plan.tool !== "select") return null;
    return { auto: true, key: `toposel:${t.id}`, title: "Toposolid", groups: [
      { label: "Mode", items: [{ label: "Edit Boundary", icon: "editBoundary", big: true, onclick: () => startSketch(t.id) }] },
      { label: "Shape Editing", items: [
        { label: "Modify Sub Elements", ico: "MS", big: true, onclick: () => startEdit(t.id) },
        { label: "Add Point", icon: "topoPoints", big: true, onclick: () => startPoints(t.id) },
        { label: "Simplify Toposolid", ico: "Si", big: true, onclick: () => simplify(t.id) },
      ] },
    ] };
  }
  function editContext() {
    return { auto: true, key: "topoedit", title: "Modify Sub Elements", groups: [
      { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stopEdit() }] },
    ] };
  }
  function pointContext() {
    return { auto: true, key: `topopoint:${pt.mode}`, title: "Add Point", groups: [
      { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stopPoints() }] },
      { label: "Add Point", items: [
        { label: "Absolute", ico: "Ab", big: true, on: pt.mode === "absolute", onclick: () => { pt.mode = "absolute"; refreshPointUI(); } },
        { label: "Along Surface", ico: "Su", big: true, on: pt.mode === "surface", onclick: () => { pt.mode = "surface"; refreshPointUI(); } },
      ] },
    ] };
  }
  function refreshPointUI() { updateRibbonContext(); renderProperties(); setPrompt(promptText()); }
  const promptText = () => Plan.tool === "topoedit" ? editPrompt() : pt.mode === "surface"
    ? `Add Point: click on the toposolid to add a point ${pt.elevation} mm above the surface there. Esc finishes.`
    : `Add Point: click on the toposolid to add a point at elevation ${pt.elevation} from the ${baseName(pt.base)}. Esc finishes.`;
  const baseName = (b) => ({ current_level: "current level", project_base_point: "project base point", survey_point: "survey point", internal_origin: "internal origin" })[b] || b;
  function startPoints(id) {
    pt.target = id;
    // Properties shows the point's settings while the tool runs; the toposolid's points stay drawn
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool("topopoint");
    refreshPointUI();
  }
  function stopPoints() {
    const id = pt.target;
    Plan.setTool("select");
    if (id != null) select(id);
  }
  async function click(p) {
    let id = pt.target;
    if (id == null) {
      const r = await Backend.call("toposolid_at", { point: [p.x, p.y] }).catch(() => null);
      id = r?.id;
    }
    if (id == null) { setPrompt("Click on a toposolid."); return; }
    const r = await run("add_toposolid_point", { id, point: [p.x, p.y], elevation: pt.elevation, mode: pt.mode, base: pt.base });
    if (!r) return;
    await refresh();
    setPrompt(`Point added. ${promptText()}`);
  }
  // the Properties pane while adding points: the height and what it is measured from
  function pointProperties() {
    const rows = [{ label: "Elevation", kind: "number", raw: pt.elevation, value: String(pt.elevation), path: "elevation" }];
    if (pt.mode === "absolute") rows.push({ label: "Elevation Base", kind: "select", path: "base", value: baseName(pt.base), options: [["current_level", "Current Level"], ["project_base_point", "Project Base Point"], ["survey_point", "Survey Point"], ["internal_origin", "Internal Origin"]].map(([v, l]) => ({ label: l, patch: { base: v }, selected: pt.base === v })) });
    return { title: "Add Point", groups: [{ name: "Point", target: { apply: (patch) => { if (Number.isFinite(patch.elevation)) pt.elevation = patch.elevation; if (patch.base) pt.base = patch.base; refreshPointUI(); } }, rows }] };
  }

  // ---- Modify Sub Elements: pick points and boundary corners, drag points across, type a height, delete --------------
  // what can be picked: the points (by index) and the boundary's corners that have no point on them (height 0)
  function items() {
    const t = data().find((x) => x.id === ed.id);
    if (!t) return [];
    const out = t.points.map((p, i) => ({ key: `p${i}`, i, x: p[0], y: p[1], z: p[2] }));
    t.rings.forEach((r, ri) => r.forEach((p, k) => {
      if (!t.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1]) <= 1)) out.push({ key: `c${ri}_${k}`, i: null, x: p[0], y: p[1], z: 0 });
    }));
    return out;
  }
  const editPrompt = () => ed.sel.size
    ? `${ed.sel.size} picked. Drag a point to move it, set Elevation in Properties, or press Delete. Esc clears the pick.`
    : "Modify Sub Elements: click a point or a boundary corner, or drag a window round several (Ctrl adds, Shift removes). Esc finishes.";
  function refreshEditUI() { renderProperties(); setPrompt(editPrompt()); Plan.render(); }
  function startEdit(id) {
    ed.id = id; ed.sel = new Set(); ed.drag = null; ed.box = null;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool("topoedit");
    refreshEditUI();
    updateRibbonContext();
  }
  function stopEdit() {
    const id = ed.id;
    ed.id = null; ed.sel = new Set(); ed.drag = null; ed.box = null;
    Plan.setTool("select");
    if (id != null) select(id);
  }
  // the whole point list with a change made: moved by `dx, dy`, or the picked ones set to height `z`, or dropped
  async function commit(change) {
    const its = items();
    const pts = (data().find((x) => x.id === ed.id)?.points || []).map((p) => p.slice());
    const picked = its.filter((it) => ed.sel.has(it.key));
    if (change.dx != null) for (const it of picked) if (it.i != null) { pts[it.i][0] += change.dx; pts[it.i][1] += change.dy; }
    if (change.z != null) {
      for (const it of picked) { if (it.i != null) pts[it.i][2] = change.z; else pts.push([it.x, it.y, change.z]); }
    }
    let out = pts;
    if (change.remove) out = pts.filter((_, i) => !picked.some((it) => it.i === i));
    const r = (v) => Math.round(v * 1000) / 1000;
    if (!(await run("set_toposolid_points", { id: ed.id, points: out.map((p) => [r(p[0]), r(p[1]), r(p[2])]) }))) return;
    // keep the same things picked, found again by where they now are (a point dropped on another replaces it, and a
    // corner given a height becomes a point, so the list's order can change)
    const where = change.remove ? [] : picked.map((it) => it.i != null && change.dx != null ? [it.x + change.dx, it.y + change.dy] : [it.x, it.y]);
    await refresh();
    ed.sel = new Set(items().filter((it) => where.some((w) => Math.hypot(w[0] - it.x, w[1] - it.y) < 0.01)).map((it) => it.key));
    refreshEditUI();
  }
  function editDown(sx, sy, ev, toScreen, toWorld) {
    if (ed.id == null) return false;
    const hit = items().map((it) => ({ it, s: toScreen({ x: it.x, y: it.y }) })).filter((h) => Math.hypot(h.s.x - sx, h.s.y - sy) <= 7)
      .sort((a, b) => Math.hypot(a.s.x - sx, a.s.y - sy) - Math.hypot(b.s.x - sx, b.s.y - sy))[0];
    if (hit) {
      const k = hit.it.key;
      if (ev.shiftKey) ed.sel.delete(k);
      else if (ev.ctrlKey) ed.sel.add(k);
      else if (!ed.sel.has(k)) ed.sel = new Set([k]);
      if (!ev.shiftKey) ed.drag = { from: { x: hit.it.x, y: hit.it.y }, to: null, sx, sy };
    } else {
      ed.box = { a: { x: sx, y: sy }, b: { x: sx, y: sy }, add: ev.ctrlKey, remove: ev.shiftKey };
    }
    refreshEditUI();
    return true;
  }
  // `snapTo` snaps the cursor (to walls, lines, the boundary, the other points, and lengths and angles from where the
  // grabbed point started); the grabbed point goes where it lands and the other picked points keep their offsets
  function editMove(sx, sy, snapTo) {
    if (ed.drag) {
      if (Math.hypot(sx - ed.drag.sx, sy - ed.drag.sy) > 3) {
        const others = items().filter((it) => !ed.sel.has(it.key)).map((it) => ({ x: it.x, y: it.y, kind: "endpoint" }));
        const w = snapTo({ last: ed.drag.from, points: others });
        ed.drag.to = { x: w.x, y: w.y };
      }
      return true;
    }
    if (ed.box) { ed.box.b = { x: sx, y: sy }; return true; }
    return false;
  }
  function editUp(toScreen) {
    if (ed.drag) {
      const d = ed.drag; ed.drag = null;
      if (d.to) commit({ dx: d.to.x - d.from.x, dy: d.to.y - d.from.y });
      return true;
    }
    if (ed.box) {
      const b = ed.box; ed.box = null;
      const [x0, x1] = [Math.min(b.a.x, b.b.x), Math.max(b.a.x, b.b.x)], [y0, y1] = [Math.min(b.a.y, b.b.y), Math.max(b.a.y, b.b.y)];
      const inBox = items().filter((it) => { const s = toScreen({ x: it.x, y: it.y }); return s.x >= x0 && s.x <= x1 && s.y >= y0 && s.y <= y1; }).map((it) => it.key);
      if (!b.add && !b.remove) ed.sel = new Set();
      inBox.forEach((k) => (b.remove ? ed.sel.delete(k) : ed.sel.add(k)));
      refreshEditUI();
      return true;
    }
    return false;
  }
  function editKey(e) {
    if (e.key === "Escape") {
      if (ed.drag || ed.box) { ed.drag = null; ed.box = null; Plan.render(); return true; }
      if (ed.sel.size) { ed.sel = new Set(); refreshEditUI(); return true; }
      stopEdit();
      return true;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && ed.sel.size) {
      if (!items().some((it) => ed.sel.has(it.key) && it.i != null)) { setPrompt("Boundary corners stay; give them a height instead, or use Edit Boundary.", true); return true; }
      commit({ remove: true });
      return true;
    }
    return false;
  }
  // the Properties pane while editing: the picked points' height above the unmodified top
  function editProperties() {
    const picked = items().filter((it) => ed.sel.has(it.key));
    if (!picked.length) return { title: "Modify Sub Elements", groups: [{ name: "Points", rows: [{ label: "Picked", kind: "ro", value: "None" }] }] };
    const same = picked.every((it) => Math.abs(it.z - picked[0].z) < 0.01);
    const z = Math.round(picked[0].z * 100) / 100;
    return { title: "Modify Sub Elements", groups: [{ name: "Points", target: { apply: (patch) => { if (Number.isFinite(patch.elevation)) commit({ z: patch.elevation }); } }, rows: [
      { label: "Picked", kind: "ro", value: String(picked.length) },
      { label: "Elevation", kind: "number", raw: same ? z : null, value: same ? String(z) : "", path: "elevation" },
    ] }] };
  }

  // Simplify Toposolid: the share of points to keep
  function simplify(id) {
    const t = data().find((x) => x.id === id);
    openModal("Simplify Toposolid", (body, foot, close) => {
      const keep = el("input", { type: "number", min: "1", max: "100", step: "1", value: "50" });
      const msg = el("div", { className: "modal-msg", textContent: `${t ? t.points.length : 0} points now. Points that their neighbours already describe go first; peaks and hollows stay.` });
      body.append(el("label", { className: "new-row" }, [el("span", { textContent: "% Points to keep" }), keep]), msg);
      const ok = button("OK", async () => {
        const r = await Backend.call("simplify_toposolid", { id, keep_percent: Number(keep.value) }).catch((e) => { msg.textContent = String(e.message || e); return null; });
        if (!r) return;
        close();
        await refresh();
        select(id);
        setPrompt(r.removed ? `Simplified: ${r.removed} point${r.removed === 1 ? "" : "s"} removed, ${r.kept} kept.` : "Nothing to remove.");
      }, true);
      foot.append(button("Cancel", close), ok);
    });
  }

  // ---- plan drawing --------------------------------------------------------------------------------------------------
  function elements(d) {
    return (d?.toposolids || []).filter((t) => t.id !== Sketch.editingId && t.rings.length).map((t) => ({ id: t.id, kind: "toposolid", category: "Topography", ring: t.rings[0], rings: t.rings, pinned: t.pinned, display: "normal" }));
  }

  // the boundary, then secondary and primary contours; a selected one shows its points
  // `pen` brings the plan's fill and line helpers, for the ground the cut plane passes through
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null, pen = null) {
    const list = (d?.toposolids || []).filter((t) => t.id !== Sketch.editingId);
    if (!list.length) return;
    ctx.save();
    ctx.setLineDash([]);
    const ring = (r) => r.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); });
    for (const t of list) {
      const on = sel.has(t.id);
      for (const primary of [false, true]) {
        if (tag) tag({ cat: "Topography", sub: primary ? "Primary Contours" : "Secondary Contours", kind: "projection" });
        ctx.beginPath();
        for (const c of t.contours) {
          if (c.primary !== primary) continue;
          for (const s of c.segs) { const a = toScreen({ x: s[0], y: s[1] }), b = toScreen({ x: s[2], y: s[3] }); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); }
        }
        ctx.strokeStyle = on ? "#2e6f9e" : primary ? "#6b5a45" : "#a39580";
        ctx.lineWidth = primary ? 1.4 : 0.7;
        ctx.stroke();
      }
      if (tag) tag({ cat: "Topography", sub: null, kind: "projection" });
      ctx.beginPath(); t.rings.forEach((r) => { ring(r); ctx.closePath(); });
      ctx.strokeStyle = on ? "#2e6f9e" : "#555"; ctx.lineWidth = on ? 2 : 1; ctx.stroke();
      // the ground above the cut plane: its cut pattern, and lines where the top and underside meet the plane
      if (t.cut && pen) {
        const st = t.cut.style;
        if (tag) tag({ cat: "Topography", sub: null, kind: "cut" });
        pen.fill(t.cut.polys, st, false, true);
        ctx.beginPath();
        for (const s of t.cut.lines) { const a = toScreen({ x: s[0], y: s[1] }), b = toScreen({ x: s[2], y: s[3] }); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); }
        ctx.strokeStyle = on ? "#2e6f9e" : pen.colour(st, st.line.color); ctx.lineWidth = pen.lw(st); ctx.setLineDash(pen.dash(st)); ctx.lineCap = "round";
        ctx.stroke(); ctx.setLineDash([]);
      }
      const editing = Plan.tool === "topoedit" && ed.id === t.id;
      if (editing) drawEdit(ctx, toScreen);
      else if (on || (Plan.tool === "topopoint" && pt.target === t.id)) {
        ctx.fillStyle = "#2e6f9e";
        for (const p of t.points) { const q = toScreen({ x: p[0], y: p[1] }); ctx.fillRect(q.x - 2.5, q.y - 2.5, 5, 5); }
      }
    }
    if (tag) tag(null);
    ctx.restore();
  }

  // the points and corners while editing: picked ones larger and orange, a drag shown where the points will go
  function drawEdit(ctx, toScreen) {
    const dx = ed.drag?.to ? ed.drag.to.x - ed.drag.from.x : 0, dy = ed.drag?.to ? ed.drag.to.y - ed.drag.from.y : 0;
    for (const it of items()) {
      const picked = ed.sel.has(it.key);
      const move = picked && it.i != null;
      const q = toScreen({ x: it.x + (move ? dx : 0), y: it.y + (move ? dy : 0) });
      const r = picked ? 4 : 3;
      ctx.lineWidth = 1.2;
      if (it.i == null) { ctx.strokeStyle = picked ? "#d9480f" : "#2e6f9e"; ctx.fillStyle = "#fff"; ctx.fillRect(q.x - r, q.y - r, 2 * r, 2 * r); ctx.strokeRect(q.x - r, q.y - r, 2 * r, 2 * r); }
      else { ctx.fillStyle = picked ? "#d9480f" : "#2e6f9e"; ctx.fillRect(q.x - r, q.y - r, 2 * r, 2 * r); }
    }
    if (ed.box) {
      ctx.save(); ctx.setLineDash([4, 3]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1;
      ctx.strokeRect(ed.box.a.x, ed.box.a.y, ed.box.b.x - ed.box.a.x, ed.box.b.y - ed.box.a.y);
      ctx.restore();
    }
  }

  // placing a new toposolid: its type and the height of its top above the level
  function placementProperties() {
    return {
      types: app.info.toposolid_types || [], current: typeId(),
      onType: (id) => { app.topoTypeId = id; renderProperties(); },
      title: "Create Toposolid Boundary",
      groups: [{ name: "Constraints", target: { apply: (patch) => { if (Number.isFinite(patch.offset)) app.topoOffset = patch.offset; renderProperties(); } }, rows: [
        { label: "Level", value: currentLevel().name, kind: "ro" },
        { label: "Height Offset From Level", kind: "number", raw: offset(), value: String(offset()), path: "offset" },
      ] }],
    };
  }

  return { startSketch, finish, importFile, selectionContext, click, startPoints, stopPoints, pointProperties, placementProperties, elements, draw, typeId, promptText, target: () => pt.target,
    startEdit, stopEdit, editDown, editMove, editUp, editKey, editProperties, simplify, dragging: () => !!ed.drag?.to };
})();
