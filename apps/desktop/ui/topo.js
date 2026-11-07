// Toposolids: Create from Sketch, Create from Import (a points file), Add Point, Edit Boundary, and how a floor plan
// draws them (the boundary and the type's contours). Private notes: site.md.
const Topo = (() => {
  // Add Point's settings: the height, measured from a base or above the surface where the point goes
  const pt = { elevation: 0, mode: "absolute", base: "current_level", target: null };
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
    const t = selected();
    if (!t || Plan.tool !== "select") return null;
    return { auto: true, key: `toposel:${t.id}`, title: "Toposolid", groups: [
      { label: "Mode", items: [{ label: "Edit Boundary", icon: "editBoundary", big: true, onclick: () => startSketch(t.id) }] },
      { label: "Shape Editing", items: [{ label: "Add Point", icon: "topoPoints", big: true, onclick: () => startPoints(t.id) }] },
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
  const promptText = () => pt.mode === "surface"
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

  // ---- plan drawing --------------------------------------------------------------------------------------------------
  function elements(d) {
    return (d?.toposolids || []).filter((t) => t.id !== Sketch.editingId && t.rings.length).map((t) => ({ id: t.id, kind: "toposolid", category: "Topography", ring: t.rings[0], rings: t.rings, pinned: t.pinned, display: "normal" }));
  }

  // the boundary, then secondary and primary contours; a selected one shows its points
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null) {
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
      if (on || (Plan.tool === "topopoint" && pt.target === t.id)) {
        ctx.fillStyle = "#2e6f9e";
        for (const p of t.points) { const q = toScreen({ x: p[0], y: p[1] }); ctx.fillRect(q.x - 2.5, q.y - 2.5, 5, 5); }
      }
    }
    if (tag) tag(null);
    ctx.restore();
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

  return { startSketch, finish, importFile, selectionContext, click, startPoints, stopPoints, pointProperties, placementProperties, elements, draw, typeId, promptText, target: () => pt.target };
})();
