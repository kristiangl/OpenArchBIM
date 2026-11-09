// Toposolids: Create from Sketch, Create from Import (a points file), Add Point, Modify Sub Elements and Add Split Line
// (shapeedit.js), Simplify, Edit Boundary, Sub-Divide, Excavate and Remove Excavation, Split and Merge, Graded Region
// and Cut / Fill, Label Contours, and how a floor plan draws them (the boundary, the type's contours and their labels,
// a graded region's existing contours, and the ground its cut plane passes through). Private notes: site.md.
const Topo = (() => {
  // Add Point's settings: the height, measured from a base or above the surface where the point goes
  const pt = { elevation: 0, mode: "absolute", base: "current_level", target: null };
  const typeId = () => {
    const ts = app.info.toposolid_types || [];
    if (!ts.some((t) => t.id === app.topoTypeId)) app.topoTypeId = ts[0]?.id;
    return app.topoTypeId;
  };
  const offset = () => (Number.isFinite(app.topoOffset) ? app.topoOffset : 0);
  const subOffset = () => (Number.isFinite(app.topoSubOffset) ? app.topoSubOffset : 0);
  // what a toposolid sketch makes: a toposolid (or its new boundary), a sub-division of `host`, or the line `host` is
  // split along
  const sk = { mode: null, host: null };
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
    sk.mode = null; sk.host = null;
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

  // Sub-Divide: a boundary sketched on the toposolid; Split: lines across it, or a closed loop inside it
  async function startSubSketch(id, mode) {
    if (Sketch.active) return;
    const t = data().find((x) => x.id === id);
    if (!t) return;
    sk.mode = mode; sk.host = id;
    typeId();
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    const r = await run("toposolid_sketch", { id });
    if (!r) { sk.mode = null; return; }
    Plan.startSketch({ kind: "toposolid", levelId: r.level_id, typeId: typeId(), curves: [], tool: "line", closed: mode !== "split", title: mode === "split" ? "Split Toposolid" : "Create Sub-division Boundary" });
    renderSketchUI();
    renderProperties();
    setPrompt(mode === "split" ? "Split Toposolid: sketch lines from one edge of the toposolid to another, or a closed loop inside it, then Finish." : "Sub-Divide: sketch the sub-division's closed boundary on the toposolid, then Finish.");
  }

  async function finish(o) {
    if (sk.mode) {
      const { mode, host } = sk;
      const r = mode === "split"
        ? await run("split_toposolid", { id: host, curves: o.curves })
        : await run("add_toposolid_subdivision", { host_id: host, type_id: typeId(), offset_mm: subOffset(), curves: o.curves });
      if (!r) return false;
      sk.mode = null; sk.host = null;
      Plan.endSketch(); endSketchUI();
      await refresh();
      select(mode === "split" ? host : r.id);
      setPrompt(mode === "split" ? `Split into ${r.ids.length} toposolids.` : "Sub-division created. It follows the toposolid's shape; Sub-divide Height in Properties raises or lowers it.");
      return true;
    }
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
    if (Plan.tool === "topolabel") return labelContext();
    if (Plan.tool === "topoexcavate") return excavateContext();
    if (ShapeEdit.editing() && host.get(ShapeEdit.id())) return ShapeEdit.context();
    const t = selected();
    if (!t || Plan.tool !== "select") return null;
    // a sub-division follows its toposolid's shape: no points of its own, and it isn't sub-divided, graded or merged
    const sub = t.host != null;
    const flat = !t.points.length && !(t.lines || []).length;
    const excavate = { label: "Excavate", icon: "excavate", menu: [
      { label: "Excavate", icon: "excavate", onclick: () => startExcavate(t.id) },
      { label: "Remove Excavation", icon: "removeExcavation", disabled: !t.excavated_by?.length, title: t.excavated_by?.length ? "" : "Nothing excavates it", onclick: () => startExcavate(t.id, true) },
    ] };
    const split = { label: "Split Toposolid", icon: "splitTopo", onclick: () => startSubSketch(t.id, "split") };
    return { auto: true, key: `toposel:${t.id}:${sub}:${t.graded}:${flat}:${t.excavated_by?.length || 0}`, title: sub ? "Toposolid Sub-division" : "Toposolid", groups: [
      { label: "Mode", labelled: true, items: [{ label: "Edit Sketch", icon: "editBoundary", big: true, onclick: () => startSketch(t.id) }] },
      ...(sub ? [] : [{ label: "Shape Editing", labelled: true, items: [
        { label: "Modify Sub Elements", icon: "modifySub", big: true, onclick: () => startEdit(t.id) },
        { label: "Add Point", icon: "topoPoints", onclick: () => startPoints(t.id) },
        { label: "Add Split Line", icon: "splitLine", onclick: () => startEdit(t.id, "line") },
        { label: "Reset Shape", icon: "resetShape", big: true, disabled: flat, title: flat ? "It has no points or split lines" : "Remove its points and split lines: flat at Height Offset From Level", onclick: () => resetShape(t.id) },
      ] }]),
      { label: "Toposolid Shaping", labelled: true, items: sub ? [excavate, split] : [
        { label: "Sub-divide", icon: "subdivide", onclick: () => startSubSketch(t.id, "sub") },
        { label: "Simplify Toposolid", icon: "simplify", onclick: () => simplify(t.id) },
        excavate,
        t.graded
          ? { label: "Remove Graded Region", icon: "removeGraded", brk: true, onclick: () => removeGraded(t.id) }
          : { label: "Graded Region", icon: "grading", brk: true, onclick: () => gradedRegion(t.id) },
        split,
        { label: "Merge Toposolids", icon: "mergeTopo", onclick: () => startExcavate(t.id, false, true) },
      ] },
    ] };
  }
  function pointContext() {
    return { auto: true, key: `topopoint:${pt.mode}`, title: "Add Point", groups: [
      { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stopPoints() }] },
      { label: "Add Point", items: [
        { label: "Absolute", icon: "pointAbsolute", big: true, on: pt.mode === "absolute", onclick: () => { pt.mode = "absolute"; refreshPointUI(); } },
        { label: "Along Surface", icon: "pointSurface", big: true, on: pt.mode === "surface", onclick: () => { pt.mode = "surface"; refreshPointUI(); } },
      ] },
    ] };
  }
  function refreshPointUI() { updateRibbonContext(); renderProperties(); setPrompt(promptText()); }
  const promptText = () => Plan.tool === "topolabel" ? labelPrompt() : Plan.tool === "topoexcavate" ? excavatePrompt() : pt.mode === "surface"
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

  // ---- Modify Sub Elements and Add Split Line: shapeedit.js, with the toposolid as its host -----------------------
  const host = {
    noun: "toposolid",
    get: (id) => { const t = data().find((x) => x.id === id); return t && { rings: t.rings, points: t.points, lines: t.lines || [] }; },
    save: (id, points, lines) => run("set_toposolid_points", { id, points, lines }),
    addLine: (id, a, b) => run("add_toposolid_split_line", { id, a: [a.x, a.y], b: [b.x, b.y] }),
    done: (id) => select(id),
  };
  const startEdit = (id, mode = "edit") => ShapeEdit.start(host, id, mode);

  // ---- Excavate and Remove Excavation: click the floors, roofs or other toposolids that cut into it --------------
  // Merge Toposolids picks the same way: click the toposolid to make part of this one. So do the Site tab's
  // Sub-Divide and Graded Region with nothing selected (`pick`: what to do with the toposolid clicked)
  const exc = { id: null, remove: false, merge: false, pick: null };
  const excavators = () => data().find((t) => t.id === exc.id)?.excavated_by || [];
  function startExcavate(id, remove = false, merge = false) {
    if (remove && !data().find((t) => t.id === id)?.excavated_by?.length) { setPrompt("Nothing excavates this toposolid yet. Excavate picks the floors, roofs or toposolids that cut into it.", true); return; }
    exc.id = id; exc.remove = remove; exc.merge = merge; exc.pick = null;
    Plan.setTool("topoexcavate");
    updateRibbonContext(true);
    setPrompt(excavatePrompt());
  }
  // the Site tab's Sub-Divide and Graded Region: the selected toposolid, else the one clicked next
  async function pickToposolid(title, then) {
    const t = selected();
    if (t && t.host == null) { then(t.id); return; }
    if (!(await ensureFloorPlan())) return;
    if (!data().some((x) => x.host == null)) { setPrompt(`${title}: there is no toposolid in this view to pick.`, true); return; }
    exc.id = null; exc.remove = false; exc.merge = false; exc.pick = { title, then };
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool("topoexcavate");
    updateRibbonContext(true);
    setPrompt(excavatePrompt());
  }
  const excavatePrompt = () => exc.pick
    ? `${exc.pick.title}: click a toposolid. Esc cancels.`
    : exc.merge
    ? "Merge Toposolids: click a toposolid that overlaps this one or shares an edge with it. This one keeps its type and properties. Esc cancels."
    : exc.remove
    ? "Remove Excavation: click a floor, roof or toposolid outlined in orange to stop it excavating. Esc finishes."
    : "Excavate: click the floors, roofs or other toposolids that cut into the toposolid (outlined in orange once they do). Esc finishes.";
  function excavateContext() {
    return { auto: true, key: `topoexcavate:${exc.remove}:${exc.merge}:${exc.pick?.title}`, title: exc.pick ? exc.pick.title : exc.merge ? "Merge Toposolids" : exc.remove ? "Remove Excavation" : "Excavate", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stopExcavate() }] }] };
  }
  function stopExcavate() {
    const id = exc.id;
    exc.id = null; exc.pick = null;
    Plan.setTool("select");
    if (id != null) select(id);
  }
  async function excavateClick(id) {
    if (exc.pick) {
      const o = data().find((t) => t.id === id);
      if (!o) { setPrompt(`Click a toposolid. ${excavatePrompt()}`); return; }
      if (o.host != null) { setPrompt(`That is a sub-division: click the toposolid it is on. ${excavatePrompt()}`); return; }
      const { then } = exc.pick;
      stopExcavate();
      select(id);
      then(id);
      return;
    }
    if (exc.merge) {
      const o = data().find((t) => t.id === id);
      if (!o) { setPrompt(`Click a toposolid. ${excavatePrompt()}`); return; }
      if (o.host != null) { setPrompt(`That is a sub-division: click the toposolid it is on. ${excavatePrompt()}`); return; }
      const r = await run("merge_toposolids", { id: exc.id, other: id });
      if (!r) return;
      await refresh();
      stopExcavate();
      setPrompt("Toposolids merged.");
      return;
    }
    if (id == null) { setPrompt(`Click a floor, roof or toposolid. ${excavatePrompt()}`); return; }
    if (exc.remove && !excavators().includes(id)) { setPrompt(`That doesn't excavate the toposolid. ${excavatePrompt()}`); return; }
    const r = await run(exc.remove ? "remove_excavation" : "excavate_toposolid", { id: exc.id, by: [id] });
    if (!r) return;
    await refresh();
    if (exc.remove && !excavators().length) { stopExcavate(); setPrompt("Excavation removed. Nothing excavates the toposolid now."); return; }
    setPrompt(`${exc.remove ? "Excavation removed." : r.added ? "Excavated." : "That already excavates the toposolid."} ${excavatePrompt()}`);
  }
  function excavateKey(e) {
    if (e.key !== "Escape") return false;
    stopExcavate();
    return true;
  }
  // while picking, what already excavates the toposolid is outlined
  function drawExcavators(ctx, d, toScreen) {
    if (Plan.tool !== "topoexcavate" || exc.id == null || exc.merge || exc.pick) return;
    const all = [...(d?.floors || []), ...(d?.roofs || []), ...(d?.toposolids || [])];
    ctx.save();
    ctx.setLineDash([]); ctx.lineWidth = 3; ctx.strokeStyle = "#e07b00";
    for (const id of excavators()) {
      const e = all.find((x) => x.id === id);
      if (!e) continue;
      ctx.beginPath();
      for (const r of e.rings?.length ? e.rings : [e.points]) {
        r.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); });
        ctx.closePath();
      }
      ctx.stroke();
    }
    ctx.restore();
  }

  // ---- Label Contours: a line across the contours; each one it crosses is labelled there -------------------------
  const lab = { start: null, at: null };
  function startLabels() {
    if (!isFloorPlan()) { setPrompt("Contours are labelled in a floor plan.", true); return; }
    lab.start = null; lab.at = null;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool("topolabel");
    updateRibbonContext(true);
    setPrompt(labelPrompt());
  }
  const labelPrompt = () => lab.start ? "Label Contours: click the other end of the line. Esc cancels." : "Label Contours: click where the line across the contours starts. Esc finishes.";
  function labelContext() {
    return { auto: true, key: "topolabel", title: "Label Contours", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] }] };
  }
  function labelHover(p) { lab.at = p ? { x: p.x, y: p.y } : null; }
  async function labelClick(p) {
    if (!lab.start) { lab.start = { x: p.x, y: p.y }; setPrompt(labelPrompt()); return; }
    if (Math.hypot(p.x - lab.start.x, p.y - lab.start.y) < 1) return;
    const r = await run("add_contour_label", { view_id: app.view.viewId, a: [lab.start.x, lab.start.y], b: [p.x, p.y] });
    lab.start = null;
    if (!r) return;
    await refresh();
    const n = (Plan.viewData()?.contour_labels || []).find((l) => l.id === r.id)?.labels.length || 0;
    setPrompt(n ? `${n} contour${n === 1 ? "" : "s"} labelled. ${labelPrompt()}` : `The line crosses no contours; it labels them when it does. ${labelPrompt()}`);
  }
  function labelKey(e) {
    if (e.key !== "Escape") return false;
    if (lab.start) { lab.start = null; setPrompt(labelPrompt()); Plan.render(); return true; }
    Plan.setTool("select");
    updateRibbonContext();
    return true;
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

  // ---- Graded Region and Cut / Fill ------------------------------------------------------------------------------
  // the toposolid keeps its ground as it is now as the existing ground; cut and fill are measured from it
  function gradedRegion(id) {
    const t = data().find((x) => x.id === id);
    if (t?.graded) { setPrompt("This toposolid is already a graded region: its cut and fill are measured from the ground as it was when it was graded.", true); return; }
    openModal("Edit Graded Region", (body, foot, close) => {
      const choice = (value, text, on) => { const r = el("input", { type: "radio", name: "graded", value, checked: on }); return [r, el("label", { className: "new-row" }, [r, el("span", { textContent: text })])]; };
      const [all, allRow] = choice("all", "Keep the toposolid as it is (all its points)", true);
      const [, perRow] = choice("perimeter", "Keep only the points on its boundary (simple grading)", false);
      const msg = el("div", { className: "modal-msg", textContent: "The ground as it is now is kept as the existing ground. Reshape the toposolid (Modify Sub Elements, Add Point, Simplify) and Properties shows Cut, Fill and Net Cut/Fill under Other. Plans show the existing contours dashed." });
      body.append(allRow, perRow, msg);
      const ok = button("OK", async () => {
        const r = await Backend.call("grade_toposolid", { id, perimeter_only: !all.checked }).catch((e) => { msg.textContent = String(e.message || e); return null; });
        if (!r) return;
        close();
        await refresh();
        select(id);
        setPrompt("Graded region: reshape the toposolid; Properties shows its cut and fill against the existing ground.");
      }, true);
      foot.append(button("Cancel", close), ok);
    });
  }
  async function resetShape(id) {
    if (!(await run("reset_toposolid_shape", { id }))) return;
    await refresh();
    select(id);
    setPrompt("Shape reset: the toposolid is flat at its Height Offset From Level. Undo brings the points back.");
  }
  async function removeGraded(id) {
    if (!(await run("remove_graded_region", { id }))) return;
    await refresh();
    select(id);
    setPrompt("No longer a graded region: the existing ground is forgotten and the toposolid stays as it is.");
  }
  async function cutFillReport() {
    const r = await run("cut_fill_report", {});
    if (!r) return;
    openModal("Cut / Fill", (body, foot, close) => {
      if (!r.rows.length) {
        body.append(el("div", { className: "modal-msg", textContent: "No toposolid is a graded region yet. Select one and click Graded Region, then reshape it: its cut and fill show here and in its Properties." }));
      } else {
        const f = (v) => v.toFixed(3);
        const tot = r.rows.reduce((a, x) => ({ cut: a.cut + x.cut, fill: a.fill + x.fill }), { cut: 0, fill: 0 });
        const tr = (cells) => el("tr", {}, cells.map((c, i) => { const td = el("td", { textContent: c }); if (i) td.style.textAlign = "right"; return td; }));
        const th = (t, i) => { const h = el("th", { textContent: t }); if (i) h.style.textAlign = "right"; return h; };
        body.append(el("table", { className: "el-table" }, [
          el("tr", {}, ["Graded region", "Cut (m\u00b3)", "Fill (m\u00b3)", "Net Cut/Fill (m\u00b3)"].map(th)),
          ...r.rows.map((x) => tr([x.name, f(x.cut), f(x.fill), f(x.fill - x.cut)])),
          ...(r.rows.length > 1 ? [tr(["Total", f(tot.cut), f(tot.fill), f(tot.fill - tot.cut)])] : []),
        ]), el("div", { className: "modal-msg", textContent: "Net Cut/Fill is fill less cut: negative when more is dug out than brought in. Cut includes what excavations and sunken sub-divisions take out." }));
      }
      foot.append(button("Close", close, true));
    });
  }

  // ---- plan drawing --------------------------------------------------------------------------------------------------
  function elements(d) {
    const list = (d?.toposolids || []).filter((t) => t.id !== Sketch.editingId && t.rings.length);
    // sub-divisions pick before the toposolid they are on
    list.sort((a, b) => (b.host != null) - (a.host != null));
    return list.map((t) => ({ id: t.id, kind: "toposolid", category: "Topography", ring: t.rings[0], rings: t.rings, pinned: t.pinned, display: "normal" }));
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
      // a graded region's ground as it was: its contours dashed, under the new ones
      if (t.existing?.length) {
        if (tag) tag({ cat: "Topography", sub: "Existing Contours", kind: "projection" });
        ctx.beginPath();
        for (const c of t.existing) for (const s of c.segs) { const a = toScreen({ x: s[0], y: s[1] }), b = toScreen({ x: s[2], y: s[3] }); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); }
        ctx.strokeStyle = on ? "#6f9ac0" : "#9a8f80"; ctx.lineWidth = 0.8; ctx.setLineDash([6, 4]);
        ctx.stroke(); ctx.setLineDash([]);
      }
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
      if (ShapeEdit.editing(t.id)) ShapeEdit.draw(ctx, toScreen, t.id);
      else if (on || (Plan.tool === "topopoint" && pt.target === t.id)) ShapeEdit.drawPlain(ctx, host.get(t.id), toScreen);
    }
    if (tag) tag(null);
    ctx.restore();
  }

  // contour labels: each label's text along its contour, on a paper-coloured backing that breaks the line under it;
  // a selected label line shows dashed, and the line being drawn
  function drawLabels(ctx, d, toScreen, k, sel = new Set(), tag = null) {
    const s = d?.view?.scale || 100;
    ctx.save();
    if (tag) tag({ cat: "Topography", sub: null, kind: "annotation" });
    for (const l of d?.contour_labels || []) {
      const on = sel.has(l.id);
      const px = (l.size || 2) * s * k;
      ctx.font = `${px}px sans-serif`; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      for (const t of l.labels) {
        const q = toScreen({ x: t.at[0], y: t.at[1] });
        ctx.save();
        ctx.translate(q.x, q.y); ctx.rotate(-t.angle);
        const w = ctx.measureText(t.text).width;
        ctx.fillStyle = "#fff"; ctx.fillRect(-w / 2 - px * 0.25, -px * 0.6, w + px * 0.5, px * 1.2);
        ctx.fillStyle = on ? "#2e6f9e" : "#4d4033"; ctx.fillText(t.text, 0, 0);
        ctx.restore();
      }
      if (on) {
        const a = toScreen({ x: l.a[0], y: l.a[1] }), b = toScreen({ x: l.b[0], y: l.b[1] });
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.setLineDash([5, 4]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1; ctx.stroke(); ctx.setLineDash([]);
      }
    }
    if (Plan.tool === "topolabel" && lab.start && lab.at) {
      const a = toScreen(lab.start), b = toScreen(lab.at);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
      ctx.setLineDash([6, 4]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1; ctx.stroke();
    }
    if (tag) tag(null);
    ctx.restore();
  }
  // a label line picks along its length, and at its labels
  function labelElements(d) {
    return (d?.contour_labels || []).map((l) => ({ id: l.id, kind: "contourlabel", category: "Topography", ring: [l.a, l.b, l.a], line: [l.a, l.b], pinned: l.pinned, display: "normal" }));
  }

  // placing a new toposolid: its type and the height of its top above the level; a sub-division: its type and its
  // height above the toposolid; a split: nothing to set
  function placementProperties() {
    if (sk.mode === "split") return { types: [], title: "Split Toposolid", groups: [] };
    if (sk.mode === "sub") return {
      types: app.info.toposolid_types || [], current: typeId(),
      onType: (id) => { app.topoTypeId = id; renderProperties(); },
      title: "Create Sub-division Boundary",
      groups: [{ name: "Constraints", target: { apply: (patch) => { if (Number.isFinite(patch.offset)) app.topoSubOffset = patch.offset; renderProperties(); } }, rows: [
        { label: "Sub-divide Height", kind: "number", raw: subOffset(), value: String(subOffset()), path: "offset" },
      ] }],
    };
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
    startEdit, simplify, startExcavate, startSubSketch, gradedRegion, cutFillReport,
    subDivide: () => pickToposolid("Sub-Divide", (id) => startSubSketch(id, "sub")), grade: () => pickToposolid("Graded Region", gradedRegion), cancelSketch: () => { sk.mode = null; sk.host = null; }, excavateClick, excavateKey, excavateTarget: () => exc.id, drawExcavators, startLabels, labelClick, labelHover, labelKey, drawLabels, labelElements, labelSnap: () => ({ last: lab.start }) };
})();
