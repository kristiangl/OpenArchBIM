// Property lines: Create by sketching or by entering distances and bearings (then placed by its start point), Edit
// Sketch, Edit Table, and how a floor plan draws them (the lines in the Property Lines style and each segment's
// bearing and distance). Bearings are whole circle from true north. Private notes: site.md.
const PropLines = (() => {
  const data = () => Plan.viewData()?.property_lines || [];
  // a table-based property line waiting to be placed: its rows, its lines from (0, 0), and where the cursor is
  const place = { rows: null, lines: null, at: null };

  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }

  async function ensureFloorPlan() {
    const isPlan = app.view.kind === "plan" && (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind === "floor_plan";
    if (isPlan) return true;
    const views = app.info.views || [];
    const v = views.find((x) => x.view_kind === "floor_plan" && x.name === "Site") || views.find((x) => x.view_kind === "floor_plan");
    if (!v) { setPrompt("Property lines are drawn in a floor plan, and this project has none.", true); return false; }
    await openView("plan", v.id);
    return true;
  }

  // Property Line: sketch it, or type the survey's distances and bearings
  async function start() {
    if (Sketch.active) return;
    if (!(await ensureFloorPlan())) return;
    openModal("Create Property Line", (body, foot, close) => {
      body.append(
        el("div", { className: "modal-msg", textContent: "Sketch the boundary, or type it from the survey: each side's distance and bearing, from a corner you click once the table is done." }),
      );
      foot.append(
        button("Cancel", close),
        button("Create by sketching", () => { close(); startSketch(null); }),
        button("Create by entering distances and bearings", () => { close(); tableDialog(null, [blankRow()]); }, true),
      );
    });
  }

  async function startSketch(id) {
    if (Sketch.active) return;
    let curves = [];
    if (id != null) {
      const r = await run("property_line", { id });
      if (!r) return;
      if (r.table) { setPrompt("This property line was made from distances and bearings: edit its table.", true); return; }
      curves = r.curves;
    }
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.startSketch({ kind: "propline", id, levelId: curLevelId(), curves, tool: id != null ? "select" : "line", closed: false, title: id != null ? "Edit Property Line" : "Create Property Line" });
    renderSketchUI();
    renderProperties();
    setPrompt("Property Line: sketch the boundary as one run of lines and arcs, then Finish. Close it to give it an area.");
  }

  async function finish(o) {
    const r = o.id ? await run("set_property_line_sketch", { id: o.id, curves: o.curves }) : await run("add_property_line", { curves: o.curves });
    if (!r) return false;
    const id = o.id || r.id;
    Plan.endSketch(); endSketchUI();
    await refresh();
    select(id);
    const pl = data().find((p) => p.id === id);
    setPrompt(pl && !closedLines(pl.lines) ? "Property line created. It isn't closed, so it has no area." : o.id ? "Property line updated." : "Property line created.");
    return true;
  }

  const closedLines = (lines) => {
    if (!lines?.length) return false;
    const a = lines[0][0], last = lines[lines.length - 1], b = last[last.length - 1];
    return lines.length > 1 && Math.hypot(a[0] - b[0], a[1] - b[1]) < 1;
  };

  // ---- the distances and bearings table --------------------------------------------------------------------------
  const blankRow = () => ({ distance: "", bearing: "", arc: false, radius: "", left: false });
  const fmt = (mm) => (mm == null ? "" : String(Math.round(mm) / 1000));
  const rowsFrom = (rows) => rows.map((r) => ({ distance: fmt(r.distance_mm), bearing: r.bearing, arc: r.radius_mm != null, radius: fmt(r.radius_mm), left: !!r.left }));
  const payload = (rows) => rows.map((r) => ({ distance_mm: Number(r.distance) * 1000, bearing: r.bearing, radius_mm: r.arc ? Number(r.radius) * 1000 : null, left: r.left }));

  // `id` null: a new one, placed by clicking its start once the table is done; otherwise Edit Table
  function tableDialog(id, start, sketched = false) {
    const rows = start.map((r) => ({ ...r }));
    let cur = 0;
    openModal(id == null ? "Property Lines" : "Edit Property Line Table", (body, foot, close) => {
      const table = el("table", { className: "el-table" });
      const msg = el("div", { className: "modal-msg" });
      const inp = (r, key, attrs = {}) => {
        const i = el("input", { type: "text", value: r[key], ...attrs });
        i.oninput = () => { r[key] = i.value; };
        return i;
      };
      const draw = () => {
        const head = el("tr", {}, ["", "Distance (m)", "Bearing", "Type", "Radius (m)", "L/R"].map((t) => el("th", { textContent: t })));
        const trs = rows.map((r, k) => {
          const type = el("select", {}, [el("option", { value: "line", textContent: "Line" }), el("option", { value: "arc", textContent: "Arc" })]);
          type.value = r.arc ? "arc" : "line";
          type.onchange = () => { r.arc = type.value === "arc"; draw(); };
          const side = el("select", { disabled: !r.arc }, [el("option", { value: "L", textContent: "L" }), el("option", { value: "R", textContent: "R" })]);
          side.value = r.left ? "L" : "R";
          side.onchange = () => { r.left = side.value === "L"; };
          const tr = el("tr", { className: k === cur ? "active" : "" }, [
            el("td", { textContent: String(k + 1) }),
            el("td", {}, [inp(r, "distance", { placeholder: "20.115" })]),
            el("td", {}, [inp(r, "bearing", { placeholder: "90°00'00\"" })]),
            el("td", {}, [type]),
            el("td", {}, [inp(r, "radius", { disabled: !r.arc })]),
            el("td", {}, [side]),
          ]);
          tr.onclick = () => { if (cur !== k) { cur = k; for (const [j, x] of [...table.querySelectorAll("tr")].slice(1).entries()) x.className = j === cur ? "active" : ""; } };
          return tr;
        });
        table.replaceChildren(head, ...trs);
      };
      draw();
      const move = (dk) => { const j = cur + dk; if (j < 0 || j >= rows.length) return; [rows[cur], rows[j]] = [rows[j], rows[cur]]; cur = j; draw(); };
      const tools = el("div", { className: "dlg-row" }, [
        button("Insert", () => { rows.splice(cur + 1, 0, blankRow()); cur += 1; draw(); }),
        button("Delete", () => { if (rows.length > 1) { rows.splice(cur, 1); cur = Math.min(cur, rows.length - 1); draw(); } }),
        button("Up", () => move(-1)),
        button("Down", () => move(1)),
      ]);
      body.append(
        el("div", { className: "modal-msg", textContent: "Bearings are whole circle from true north, as 82°30'00\" (or 82 30 00), or quadrant bearings such as N 82°30' E. An arc's distance and bearing are its chord's; L or R is the side it bulges to, going along it." + (sketched ? " Editing the table makes this a table-based property line: it can't then be sketched." : "") }),
        table, tools, msg,
      );
      const ok = button("OK", async () => {
        const p = payload(rows);
        const r = await Backend.call(id == null ? "property_line_preview" : "set_property_line_table", id == null ? { rows: p } : { id, rows: p }).catch((e) => { msg.textContent = String(e.message || e); return null; });
        if (!r) return;
        close();
        if (id == null) { startPlace(p, r); return; }
        await refresh();
        select(id);
        setPrompt("Property line table updated.");
      }, true);
      foot.append(button("Cancel", close), ok);
    }, { wide: true });
  }

  async function editTable(id) {
    const r = await run("property_line", { id });
    if (!r) return;
    tableDialog(id, rowsFrom(r.rows), !r.table);
  }

  // placing: the shape follows the cursor by its start point until a click puts it there
  function startPlace(rows, preview) {
    place.rows = rows; place.lines = preview.lines; place.at = null;
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool("propplace");
    updateRibbonContext(true);
    setPrompt(promptText());
  }
  const promptText = () => "Property Line: click where the first segment starts (a survey corner or a snapped point). Esc cancels.";
  async function placeClick(p) {
    if (!place.rows || !p) return;
    const r = await run("add_property_line_table", { at: [p.x, p.y], rows: place.rows });
    if (!r) return;
    stopPlace();
    await refresh();
    select(r.id);
    setPrompt("Property line created.");
  }
  function stopPlace() { place.rows = null; place.lines = null; place.at = null; Plan.setTool("select"); }
  function hover(p) { place.at = p; }
  function key(e) {
    if (e.key !== "Escape") return false;
    stopPlace();
    setPrompt("Property line cancelled.");
    return true;
  }

  // ---- a selected property line -----------------------------------------------------------------------------------
  function selectionContext() {
    if (app.view.kind !== "plan") return null;
    if (Plan.tool === "propplace") return { auto: true, key: "propplace", title: "Place Property Line", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stopPlace() }] }] };
    if (app.selection.length !== 1 || Plan.tool !== "select") return null;
    const pl = data().find((p) => p.id === app.selection[0]);
    if (!pl) return null;
    return { auto: true, key: `propline:${pl.id}:${pl.table}`, title: "Property Lines", groups: [
      { label: "Property Lines", labelled: true, items: [
        ...(pl.table ? [] : [{ label: "Edit Sketch", icon: "editBoundary", big: true, onclick: () => startSketch(pl.id) }]),
        { label: "Edit Table", icon: "editTable", big: true, onclick: () => editTable(pl.id) },
      ] },
    ] };
  }

  // ---- plan drawing --------------------------------------------------------------------------------------------------
  // an open run picks along its length: each piece goes there and back, so it has no inside
  function elements(d) {
    return (d?.property_lines || []).filter((p) => p.id !== Sketch.editingId).map((p) => {
      const rings = p.lines.map((l) => [...l, ...l.slice().reverse()]);
      return { id: p.id, kind: "propertyline", category: "Site", ring: rings[0], rings, pinned: p.pinned, display: "normal" };
    });
  }

  // the lines in the Property Lines style, each segment's label along it, and a table-based one being placed
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null, pen = null) {
    const s = d?.view?.scale || 100;
    ctx.save();
    for (const p of d?.property_lines || []) {
      if (p.id === Sketch.editingId) continue;
      const on = sel.has(p.id);
      const st = p.style;
      if (tag) tag({ cat: "Site", sub: "Property Lines", kind: "projection" });
      ctx.beginPath();
      for (const l of p.lines) l.forEach((q, i) => { const a = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(a.x, a.y) : ctx.moveTo(a.x, a.y); });
      ctx.strokeStyle = on ? "#2e6f9e" : pen ? pen.colour(st, st.line.color) : st.line.color;
      ctx.lineWidth = pen ? pen.lw(st) : 1.5;
      ctx.setLineDash(pen ? pen.dash(st) : [8, 3, 1, 3]);
      ctx.lineCap = "butt";
      ctx.stroke();
      ctx.setLineDash([]);
      // bearing and distance along each segment, on the side the text reads from
      const px = 2 * s * k;
      ctx.font = `${px}px sans-serif`; ctx.textAlign = "center"; ctx.textBaseline = "bottom";
      ctx.fillStyle = on ? "#2e6f9e" : "#333";
      for (const t of p.labels) {
        const q = toScreen({ x: t.at[0], y: t.at[1] });
        ctx.save(); ctx.translate(q.x, q.y); ctx.rotate(-t.angle); ctx.fillText(t.text, 0, -px * 0.35); ctx.restore();
      }
    }
    if (tag) tag(null);
    if (Plan.tool === "propplace" && place.lines && place.at) {
      ctx.beginPath();
      for (const l of place.lines) l.forEach((q, i) => { const a = toScreen({ x: q[0] + place.at.x, y: q[1] + place.at.y }); i ? ctx.lineTo(a.x, a.y) : ctx.moveTo(a.x, a.y); });
      ctx.setLineDash([6, 4]); ctx.strokeStyle = "#c0369a"; ctx.lineWidth = 1.5; ctx.stroke(); ctx.setLineDash([]);
    }
    ctx.restore();
  }

  return { start, startSketch, finish, editTable, selectionContext, elements, draw, promptText, placeClick, hover, key };
})();
