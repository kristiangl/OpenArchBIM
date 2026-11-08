// Shape editing in a plan: Modify Sub Elements (pick points, boundary corners and split lines; drag them across with
// snapping; set their height; delete) and Add Split Line, for any host with a boundary, points and split lines.
// Toposolids use it now; floors and roofs are to (model side: shape.rs). A host is
//   { noun, get(id) -> { rings, points: [[x, y, z]], lines: [[i, j]] }, save(id, points, lines), addLine(id, a, b),
//     done(id) }
// with heights above the host's unmodified top. Private notes: site.md.
const ShapeEdit = (() => {
  // the host and element, the picked things (by key), a drag or window, and Add Split Line's first end and cursor
  const ed = { host: null, id: null, sel: new Set(), drag: null, box: null, start: null, at: null };
  const shape = () => (ed.host && ed.id != null ? ed.host.get(ed.id) : null);
  const editing = (id) => ed.id != null && (id == null || ed.id === id) && (Plan.tool === "shapeedit" || Plan.tool === "shapeline");

  // what can be picked: the points (by index), the boundary's corners that have no point on them (height 0), and the
  // split lines (by index, with their ends)
  function items() {
    const s = shape();
    if (!s) return [];
    const out = s.points.map((p, i) => ({ key: `p${i}`, i, x: p[0], y: p[1], z: p[2] }));
    s.rings.forEach((r, ri) => r.forEach((p, k) => {
      if (!s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1]) <= 1)) out.push({ key: `c${ri}_${k}`, i: null, x: p[0], y: p[1], z: 0 });
    }));
    (s.lines || []).forEach((l, k) => {
      const a = s.points[l[0]], b = s.points[l[1]];
      if (a && b) out.push({ key: `l${k}`, line: k, ends: l, a, b, x: (a[0] + b[0]) / 2, y: (a[1] + b[1]) / 2 });
    });
    return out;
  }
  const pointItems = () => items().filter((it) => it.line == null);
  // the points a change to the pick moves or raises: picked points and the ends of picked lines
  function touched(picked) {
    const out = new Set();
    for (const it of picked) { if (it.line != null) it.ends.forEach((i) => out.add(i)); else if (it.i != null) out.add(it.i); }
    return out;
  }

  const prompts = {
    shapeedit: () => ed.sel.size
      ? `${ed.sel.size} picked. Drag to move, set Elevation in Properties, or press Delete. Esc clears the pick.`
      : "Modify Sub Elements: click a point, a boundary corner or a split line, or drag a window round several (Ctrl adds, Shift removes). Esc finishes.",
    shapeline: () => ed.start ? "Add Split Line: click the next end (on the surface or a point). Esc ends the line." : `Add Split Line: click where the line starts on the ${ed.host?.noun || "surface"}. Esc finishes.`,
  };
  const promptText = () => (prompts[Plan.tool] || (() => ""))();
  function refreshUI() { renderProperties(); setPrompt(promptText()); Plan.render(); }

  // `mode` "edit" (Modify Sub Elements) or "line" (Add Split Line)
  function start(host, id, mode = "edit") {
    Object.assign(ed, { host, id, sel: new Set(), drag: null, box: null, start: null, at: null });
    app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]);
    Plan.setTool(mode === "line" ? "shapeline" : "shapeedit");
    refreshUI();
    updateRibbonContext();
  }
  function stop() {
    const { host, id } = ed;
    Object.assign(ed, { host: null, id: null, sel: new Set(), drag: null, box: null, start: null, at: null });
    Plan.setTool("select");
    if (host && id != null) host.done(id);
  }
  function setMode(mode) { const { host, id } = ed; if (host) start(host, id, mode); }

  function context() {
    const noun = ed.host?.noun || "surface";
    return { auto: true, key: `${Plan.tool}:${ed.id}`, title: Plan.tool === "shapeline" ? "Add Split Line" : "Modify Sub Elements", groups: [
      { label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => stop() }] },
      { label: "Shape Editing", items: [
        { label: "Modify Sub Elements", ico: "MS", big: true, on: Plan.tool === "shapeedit", onclick: () => setMode("edit") },
        { label: "Add Split Line", ico: "SL", big: true, on: Plan.tool === "shapeline", title: `Draw a line the ${noun} folds along`, onclick: () => setMode("line") },
      ] },
    ] };
  }

  // the whole lists with a change made: moved by `dx, dy`, or set to height `z`, or the picked things dropped (a
  // dropped line leaves its points; a dropped point takes its lines)
  async function commit(change) {
    const s = shape();
    if (!s) return;
    const pts = s.points.map((p) => p.slice());
    let lines = (s.lines || []).map((l) => l.slice());
    const picked = items().filter((it) => ed.sel.has(it.key));
    const moving = touched(picked);
    if (change.dx != null) for (const i of moving) { pts[i][0] += change.dx; pts[i][1] += change.dy; }
    if (change.z != null) {
      for (const i of moving) pts[i][2] = change.z;
      for (const it of picked) if (it.i == null && it.line == null) pts.push([it.x, it.y, change.z]);
    }
    let out = pts;
    if (change.remove) {
      const gone = new Set(picked.filter((it) => it.i != null).map((it) => it.i));
      const goneLines = new Set(picked.filter((it) => it.line != null).map((it) => it.line));
      const to = []; out = [];
      pts.forEach((p, i) => { if (!gone.has(i)) { to[i] = out.length; out.push(p); } });
      lines = lines.filter((l, k) => !goneLines.has(k) && !gone.has(l[0]) && !gone.has(l[1])).map((l) => [to[l[0]], to[l[1]]]);
    }
    const r = (v) => Math.round(v * 1000) / 1000;
    if (!(await ed.host.save(ed.id, out.map((p) => [r(p[0]), r(p[1]), r(p[2])]), lines))) return;
    // keep the same things picked, found again by where they now are (a point dropped on another replaces it, a
    // corner given a height becomes a point, and lines can be broken, so the lists' order can change)
    const shift = (x, y, i) => (change.dx != null && moving.has(i) ? [x + change.dx, y + change.dy] : [x, y]);
    const wherePts = change.remove ? [] : picked.filter((it) => it.line == null).map((it) => shift(it.x, it.y, it.i));
    const whereLines = change.remove ? [] : picked.filter((it) => it.line != null).map((it) => [shift(it.a[0], it.a[1], it.ends[0]), shift(it.b[0], it.b[1], it.ends[1])]);
    await refresh();
    const near = (p, x, y) => Math.hypot(p[0] - x, p[1] - y) < 0.01;
    ed.sel = new Set(items().filter((it) => it.line == null
      ? wherePts.some((w) => near(w, it.x, it.y))
      : whereLines.some(([a, b]) => (near(a, it.a[0], it.a[1]) && near(b, it.b[0], it.b[1])) || (near(b, it.a[0], it.a[1]) && near(a, it.b[0], it.b[1])))).map((it) => it.key));
    refreshUI();
  }

  // what is under the cursor: a point or corner within 7 px, else a split line within 5 px
  function hitAt(sx, sy, toScreen) {
    const its = items();
    const pt = its.filter((it) => it.line == null).map((it) => ({ it, d: Math.hypot(toScreen({ x: it.x, y: it.y }).x - sx, toScreen({ x: it.x, y: it.y }).y - sy) }))
      .filter((h) => h.d <= 7).sort((a, b) => a.d - b.d)[0];
    if (pt) return pt.it;
    const seg = (it) => {
      const a = toScreen({ x: it.a[0], y: it.a[1] }), b = toScreen({ x: it.b[0], y: it.b[1] });
      const dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy;
      const t = l2 ? Math.max(0, Math.min(1, ((sx - a.x) * dx + (sy - a.y) * dy) / l2)) : 0;
      return Math.hypot(sx - (a.x + t * dx), sy - (a.y + t * dy));
    };
    return its.filter((it) => it.line != null).map((it) => ({ it, d: seg(it) })).filter((h) => h.d <= 5).sort((a, b) => a.d - b.d)[0]?.it || null;
  }

  function down(sx, sy, ev, toScreen, toWorld) {
    if (Plan.tool !== "shapeedit" || ed.id == null) return false;
    const hit = hitAt(sx, sy, toScreen);
    if (hit) {
      const k = hit.key;
      if (ev.shiftKey) ed.sel.delete(k);
      else if (ev.ctrlKey) ed.sel.add(k);
      else if (!ed.sel.has(k)) ed.sel = new Set([k]);
      // a point is grabbed where it is; a line where it was clicked
      const from = hit.line != null ? toWorld(sx, sy) : { x: hit.x, y: hit.y };
      if (!ev.shiftKey) ed.drag = { from, to: null, sx, sy };
    } else {
      ed.box = { a: { x: sx, y: sy }, b: { x: sx, y: sy }, add: ev.ctrlKey, remove: ev.shiftKey };
    }
    refreshUI();
    return true;
  }
  // the points to snap to: the ones not being moved
  function snapPoints(skip = new Set()) {
    return pointItems().filter((it) => !(it.i != null && skip.has(it.i))).map((it) => ({ x: it.x, y: it.y, kind: "endpoint" }));
  }
  // `snapTo` snaps the cursor (to walls, lines, boundaries, the other points, and lengths and angles from where the
  // grab started); the grabbed point goes where it lands and the other picked things keep their offsets
  function move(sx, sy, snapTo) {
    if (Plan.tool !== "shapeedit") return false;
    if (ed.drag) {
      if (Math.hypot(sx - ed.drag.sx, sy - ed.drag.sy) > 3) {
        const picked = items().filter((it) => ed.sel.has(it.key));
        const w = snapTo({ last: ed.drag.from, points: snapPoints(touched(picked)) });
        ed.drag.to = { x: w.x, y: w.y };
      }
      return true;
    }
    if (ed.box) { ed.box.b = { x: sx, y: sy }; return true; }
    return false;
  }
  function up(toScreen) {
    if (Plan.tool !== "shapeedit") return false;
    if (ed.drag) {
      const d = ed.drag; ed.drag = null;
      if (d.to) commit({ dx: d.to.x - d.from.x, dy: d.to.y - d.from.y });
      return true;
    }
    if (ed.box) {
      const b = ed.box; ed.box = null;
      const [x0, x1] = [Math.min(b.a.x, b.b.x), Math.max(b.a.x, b.b.x)], [y0, y1] = [Math.min(b.a.y, b.b.y), Math.max(b.a.y, b.b.y)];
      const inBox = (x, y) => { const s = toScreen({ x, y }); return s.x >= x0 && s.x <= x1 && s.y >= y0 && s.y <= y1; };
      // points inside the window, and lines with both ends inside it
      const keys = items().filter((it) => (it.line == null ? inBox(it.x, it.y) : inBox(it.a[0], it.a[1]) && inBox(it.b[0], it.b[1]))).map((it) => it.key);
      if (!b.add && !b.remove) ed.sel = new Set();
      keys.forEach((k) => (b.remove ? ed.sel.delete(k) : ed.sel.add(k)));
      refreshUI();
      return true;
    }
    return false;
  }

  // Add Split Line: each click after the first adds a line from the last end, and the chain goes on from there
  const lineSnap = () => ({ last: ed.start, points: snapPoints() });
  function lineHover(p) { ed.at = p ? { x: p.x, y: p.y } : null; }
  async function lineClick(p) {
    if (Plan.tool !== "shapeline" || ed.id == null || !p) return;
    if (!ed.start) { ed.start = { x: p.x, y: p.y }; refreshUI(); return; }
    if (Math.hypot(p.x - ed.start.x, p.y - ed.start.y) < 1) return;
    if (!(await ed.host.addLine(ed.id, ed.start, p))) return;
    ed.start = { x: p.x, y: p.y };
    await refresh();
    refreshUI();
  }

  function key(e) {
    if (!editing()) return false;
    if (e.key === "Escape") {
      if (ed.drag || ed.box) { ed.drag = null; ed.box = null; Plan.render(); return true; }
      if (ed.start) { ed.start = null; refreshUI(); return true; }
      if (ed.sel.size) { ed.sel = new Set(); refreshUI(); return true; }
      stop();
      return true;
    }
    if (Plan.tool === "shapeedit" && (e.key === "Delete" || e.key === "Backspace") && ed.sel.size) {
      if (!items().some((it) => ed.sel.has(it.key) && (it.i != null || it.line != null))) { setPrompt(`Boundary corners stay; give them a height instead, or use Edit Boundary.`, true); return true; }
      commit({ remove: true });
      return true;
    }
    return false;
  }

  // the Properties pane while editing: the picked things' height above the unmodified top
  function properties() {
    const title = Plan.tool === "shapeline" ? "Add Split Line" : "Modify Sub Elements";
    const picked = items().filter((it) => ed.sel.has(it.key));
    if (!picked.length) return { title, groups: [{ name: "Points", rows: [{ label: "Picked", kind: "ro", value: "None" }] }] };
    const s = shape();
    const zs = [...touched(picked)].map((i) => s.points[i][2]).concat(picked.filter((it) => it.i == null && it.line == null).map((it) => it.z));
    const same = zs.every((z) => Math.abs(z - zs[0]) < 0.01);
    const z = Math.round(zs[0] * 100) / 100;
    return { title, groups: [{ name: "Points", target: { apply: (patch) => { if (Number.isFinite(patch.elevation)) commit({ z: patch.elevation }); } }, rows: [
      { label: "Picked", kind: "ro", value: String(picked.length) },
      { label: "Elevation", kind: "number", raw: same ? z : null, value: same ? String(z) : "", path: "elevation" },
    ] }] };
  }

  // a host's points and split lines when it is selected (small, plain)
  function drawPlain(ctx, s, toScreen, colour = "#2e6f9e") {
    ctx.save();
    ctx.strokeStyle = colour; ctx.lineWidth = 1; ctx.setLineDash([]);
    ctx.beginPath();
    for (const l of s.lines || []) {
      const a = s.points[l[0]], b = s.points[l[1]];
      if (!a || !b) continue;
      const p = toScreen({ x: a[0], y: a[1] }), q = toScreen({ x: b[0], y: b[1] });
      ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y);
    }
    ctx.stroke();
    ctx.fillStyle = colour;
    for (const p of s.points) { const q = toScreen({ x: p[0], y: p[1] }); ctx.fillRect(q.x - 2.5, q.y - 2.5, 5, 5); }
    ctx.restore();
  }

  // while editing: points, corners (hollow) and lines, picked ones orange, a drag shown where things will go, and
  // Add Split Line's line to the cursor
  function draw(ctx, toScreen, id) {
    if (!editing(id)) return;
    const s = shape();
    if (!s) return;
    const picked = items().filter((it) => ed.sel.has(it.key));
    const moving = ed.drag?.to ? touched(picked) : new Set();
    const dx = ed.drag?.to ? ed.drag.to.x - ed.drag.from.x : 0, dy = ed.drag?.to ? ed.drag.to.y - ed.drag.from.y : 0;
    const at = (i) => toScreen({ x: s.points[i][0] + (moving.has(i) ? dx : 0), y: s.points[i][1] + (moving.has(i) ? dy : 0) });
    ctx.save();
    ctx.setLineDash([]);
    for (const it of items()) {
      if (it.line == null) continue;
      const on = ed.sel.has(it.key);
      const a = at(it.ends[0]), b = at(it.ends[1]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = on ? "#d9480f" : "#2b8a3e"; ctx.lineWidth = on ? 2.5 : 1.5; ctx.stroke();
    }
    for (const it of items()) {
      if (it.line != null) continue;
      const on = ed.sel.has(it.key);
      const q = it.i != null ? at(it.i) : toScreen({ x: it.x, y: it.y });
      const r = on ? 4 : 3;
      ctx.lineWidth = 1.2;
      if (it.i == null) { ctx.strokeStyle = on ? "#d9480f" : "#2e6f9e"; ctx.fillStyle = "#fff"; ctx.fillRect(q.x - r, q.y - r, 2 * r, 2 * r); ctx.strokeRect(q.x - r, q.y - r, 2 * r, 2 * r); }
      else { ctx.fillStyle = on ? "#d9480f" : "#2e6f9e"; ctx.fillRect(q.x - r, q.y - r, 2 * r, 2 * r); }
    }
    if (Plan.tool === "shapeline" && ed.start && ed.at) {
      const a = toScreen(ed.start), b = toScreen(ed.at);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = "#2b8a3e"; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]); ctx.stroke();
    }
    if (ed.box) {
      ctx.setLineDash([4, 3]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1;
      ctx.strokeRect(ed.box.a.x, ed.box.a.y, ed.box.b.x - ed.box.a.x, ed.box.b.y - ed.box.a.y);
    }
    ctx.restore();
  }

  return { start, stop, context, down, move, up, key, properties, draw, drawPlain, editing, promptText, lineSnap, lineHover, lineClick,
    dragging: () => !!ed.drag?.to, id: () => ed.id };
})();
