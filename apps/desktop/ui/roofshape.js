// Shape editing of flat roofs in a plan: Modify Sub Elements, Add Point, Add Split Line (shapeedit.js) and Reset
// Shape, with Edit Footprint, for one selected roof. A roof with edges that define slope is pitched, not shape edited.
const RoofShape = (() => {
  const roofs = () => Plan.viewData()?.roofs || [];
  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }
  const host = {
    noun: "roof",
    get: (id) => { const r = roofs().find((x) => x.id === id); return r && { rings: r.rings, points: r.shape_points || [], lines: r.lines || [] }; },
    save: (id, points, lines) => run("set_roof_points", { id, points, lines }),
    addLine: (id, a, b) => run("add_roof_split_line", { id, a: [a.x, a.y], b: [b.x, b.y] }),
    addPoint: (id, p, z) => run("add_roof_point", { id, point: [p.x, p.y], z }),
    done: (id) => select(id),
  };
  const shaped = (r) => (r.shape_points || []).length > 0 || (r.lines || []).length > 0;

  async function reset(id) {
    if (!(await run("reset_roof_shape", { id }))) return;
    await refresh();
    select(id);
    setPrompt("Shape reset: the roof is flat again. Undo brings the points back.");
  }

  // one roof selected in a plan: Edit Footprint and the shape editing tools
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (ShapeEdit.editing() && roofs().some((x) => x.id === ShapeEdit.id())) return ShapeEdit.context();
    if (Plan.tool !== "select" || app.selection.length !== 1) return null;
    const r = roofs().find((x) => x.id === app.selection[0]);
    if (!r) return null;
    const why = r.pitched ? "A roof with edges that define slope is pitched, not shape edited" : "";
    const tool = (label, icon, onclick, big = false) => ({ label, icon, big, onclick, disabled: !!why, title: why });
    const flat = !shaped(r);
    return { auto: true, key: `roofshape:${r.id}:${r.pitched}:${flat}`, title: "Roofs", groups: [
      { label: "Mode", labelled: true, items: [{ label: "Edit Footprint", icon: "editBoundary", big: true, onclick: () => startRoofSketch(r.id) }] },
      { label: "Shape Editing", labelled: true, items: [
        tool("Modify Sub Elements", "modifySub", () => ShapeEdit.start(host, r.id, "edit"), true),
        tool("Add Point", "topoPoints", () => ShapeEdit.start(host, r.id, "point")),
        tool("Add Split Line", "splitLine", () => ShapeEdit.start(host, r.id, "line")),
        { label: "Reset Shape", icon: "resetShape", big: true, disabled: flat, title: flat ? "The roof has no points or split lines" : "Remove its points and split lines: flat again", onclick: () => reset(r.id) },
      ] },
    ] };
  }

  // the roof being shape edited (its points and lines to work on), or a selected shaped roof (plainly)
  function draw(ctx, d, toScreen, sel) {
    for (const r of d?.roofs || []) {
      if (ShapeEdit.editing(r.id)) ShapeEdit.draw(ctx, toScreen, r.id);
      else if (sel.has(r.id) && shaped(r)) ShapeEdit.drawPlain(ctx, host.get(r.id), toScreen);
    }
  }

  return { selectionContext, draw, host };
})();
