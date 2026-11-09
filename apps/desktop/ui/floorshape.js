// Shape editing of floors in a plan: Modify Sub Elements, Add Point, Add Split Line (shapeedit.js) and Reset Shape,
// with Edit Boundary, for one selected floor. A floor with a slope arrow is sloped, not shape edited.
const FloorShape = (() => {
  const floors = () => Plan.viewData()?.floors || [];
  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }
  const host = {
    noun: "floor",
    get: (id) => { const f = floors().find((x) => x.id === id); return f && { rings: f.rings, points: f.shape_points || [], lines: f.lines || [] }; },
    save: (id, points, lines) => run("set_floor_points", { id, points, lines }),
    addLine: (id, a, b) => run("add_floor_split_line", { id, a: [a.x, a.y], b: [b.x, b.y] }),
    addPoint: (id, p, z) => run("add_floor_point", { id, point: [p.x, p.y], z }),
    done: (id) => select(id),
  };
  const shaped = (f) => (f.shape_points || []).length > 0 || (f.lines || []).length > 0;

  async function reset(id) {
    if (!(await run("reset_floor_shape", { id }))) return;
    await refresh();
    select(id);
    setPrompt("Shape reset: the floor is flat again. Undo brings the points back.");
  }

  // one floor selected in a plan: Edit Boundary and the shape editing tools
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (ShapeEdit.editing() && floors().some((x) => x.id === ShapeEdit.id())) return ShapeEdit.context();
    if (Plan.tool !== "select" || app.selection.length !== 1) return null;
    const f = floors().find((x) => x.id === app.selection[0]);
    if (!f) return null;
    const why = f.sloped ? "A floor with a slope arrow is sloped, not shape edited" : "";
    const tool = (label, icon, onclick, big = false) => ({ label, icon, big, onclick, disabled: !!why, title: why });
    const flat = !shaped(f);
    return { auto: true, key: `floorshape:${f.id}:${f.sloped}:${flat}`, title: "Floors", groups: [
      { label: "Mode", labelled: true, items: [{ label: "Edit Boundary", icon: "editBoundary", big: true, onclick: () => startFloorSketch(f.id) }] },
      { label: "Shape Editing", labelled: true, items: [
        tool("Modify Sub Elements", "modifySub", () => ShapeEdit.start(host, f.id, "edit"), true),
        tool("Add Point", "topoPoints", () => ShapeEdit.start(host, f.id, "point")),
        tool("Add Split Line", "splitLine", () => ShapeEdit.start(host, f.id, "line")),
        { label: "Reset Shape", icon: "resetShape", big: true, disabled: flat, title: flat ? "The floor has no points or split lines" : "Remove its points and split lines: flat again", onclick: () => reset(f.id) },
      ] },
    ] };
  }

  // the floor being shape edited (its points and lines to work on), or a selected shaped floor (plainly)
  function draw(ctx, d, toScreen, sel) {
    for (const f of d?.floors || []) {
      if (ShapeEdit.editing(f.id)) ShapeEdit.draw(ctx, toScreen, f.id);
      else if (sel.has(f.id) && shaped(f)) ShapeEdit.drawPlain(ctx, host.get(f.id), toScreen);
    }
  }

  return { selectionContext, draw, host };
})();
