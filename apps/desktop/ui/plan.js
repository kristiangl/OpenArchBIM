// 2D floor plan view with the wall and floor tools. Coordinates are model
// millimetres, y up. All geometry comes from the Rust core; this file only
// draws it and turns mouse input into commands (callbacks set by main.js).

const Plan = (() => {
  const canvas = document.getElementById("plan");
  const ctx = canvas.getContext("2d");
  const cb = { editGroup() {}, groupPick() {}, placeGroup() {}, addSection() {}, openView() {}, editFloor() {}, editRoof() {}, tempEdit() {}, tempLines() {}, tempPin() {}, tempPromote() {}, dimCall() {}, dimRefresh() {}, dimText() {}, dimSettings: () => ({}), placeDims() {}, edit() {}, array() {}, offset() {}, trim() {}, align() {}, options() {}, addWall() {}, addWalls() {}, moveWallEnd() {}, moveWallEnds() {}, reshapeWall() {}, reshapePreview() {}, placeInstance() {}, placeComponent() {}, joinEnd() {}, placeMarker() {}, markerSlot() {}, addGrid() {}, gridFromLine() {}, patchGrid() {}, moveGridEnd() {}, toggleGridLock() {}, markerSlots: () => [true, false, false, false], select() {}, cursor() {}, tool() {}, prompt() {} };

  let cam = { x: 3000, y: 2000, s: 0.1 }; // centre in mm, pixels per mm
  let data = { walls: [], floors: [], roofs: [], inserts: [], view: {}, crop: {} };
  let tool = "select";
  let draw = null; // { pts: [{x,y}] } while a wall chain or floor outline is in progress
  let hover = null; // { x, y, kind } snapped cursor
  let sel = new Set();
  let extents = { left: 100, right: 100 }; // reach of the wall being drawn to each side of its location line
  let pan = null;
  let down = null;
  let shift = false;
  // status bar selection options (kept per user by the app, not in the project)
  let selOpts = { links: true, underlay: false, pinned: true, face: true, dragOnSel: true };
  let dragging = null; // Drag Elements on Selection: { ids, p0, d }
  let band = null; // rubber band in screen pixels: { x0, y0, x1, y1 }
  let selHover = null; // ids under the cursor in the Select tool (the Tab choice)
  let modelGroups = []; // model group instances (doc_info): { id, name, members, origin, ... }
  let memberOf = new Map(); // member element id to its group
  let groupEdit = null; // the group open in Edit Group: { id, mode: null | "add" | "remove" }
  let groupPlace = null; // Place Model Group: { typeId, name }
  let tabIdx = 0;
  let joinHover = null; // { id, end, p:{x,y} } wall end under the cursor while the Wall Joins tool is active
  let joinSel = []; // the wall ends whose joins are being edited, the last one is the primary
  let thin = false; // Thin Lines: every line draws one pixel wide whatever the view scale
  let place = { width: 900, label: "door" }; // size and name of the family being placed
  const hostedTool = (t) => t === "door" || t === "window" || t === "hosted"; // placed by clicking a wall
  let comp = null; // a free standing component being placed: { parts, footprint } in its own axes, and its turn { rot }
  let placeHover = null; // { wall, u, p:{x,y}, quad } while a door or window tool hovers a wall
  let tdims = []; // temporary dimensions of the single selected element, from the model
  let placeDims = [], placeKey = "", placeSeq = 0; // the same for the door or window about to be placed
  let tdimUI = []; // screen boxes of the drawn temporary dimensions: labels (click to edit) and witness grips
  let tgrip = null; // { dim, role, x, y, lines, cand } while an end of a dimension is dragged to another reference line
  // Wall tool draw shapes (the same shapes the sketch tools offer, without entering a sketch).
  // Straight walls only for now: curved shapes need curved walls in the model.
  let wallShape = "line", wallSides = 5;
  const SHAPE_TWO_CLICK = new Set(["rectangle", "polygon_in", "polygon_out"]);
  // curved shapes: the picks they take (n = 0 means open ended, Enter finishes) and the sketch tool the model
  // makes the curve with. A closed circle or ellipse goes round in two walls.
  const SHAPE_CURVE = {
    circle: { n: 2, core: "circle", prompts: ["Click the centre of the circle.", "Click a point on the circle."] },
    arc_3pt: { n: 3, core: "arc_3pt", prompts: ["Click the start of the arc.", "Click the end of the arc.", "Click a point on the arc."] },
    arc_centre: { n: 3, core: "arc_centre", prompts: ["Click the centre of the arc.", "Click the start of the arc (sets the radius).", "Click the end of the arc."] },
    ellipse: { n: 3, core: "ellipse", prompts: ["Click the centre of the ellipse.", "Click the end of the first axis.", "Click the end of the second axis."] },
    partial_ellipse: { n: 5, core: "partial_ellipse", prompts: ["Click the centre of the ellipse.", "Click the end of the first axis.", "Click the end of the second axis.", "Click where the wall starts.", "Click where the wall ends."] },
    spline: { n: 0, core: "spline", prompts: ["Click the first point of the curve.", "Click the next point. Enter finishes the wall."] },
  };
  // Grid tool: line, arcs, multi-segment chain, or Pick Lines (a wall or grid, with an offset)
  let gridShape = "line", gridOffset = 0;
  let ggrips = [], gboxes = [], gbubbles = []; // screen handles of the selected grids: end grips, head check boxes, heads
  let ggrip = null; // { id, end, g, cur, moved } while the end of a grid is dragged
  let gpins = []; // screen position of each padlock between aligned grid ends
  let gridAlignHint = null; // the grid end the cursor lines up with, while drawing or dragging a grid end
  let gridOpen = null; // the head pressed, opened as a name input on release
  let gridHover = null; // the wall or grid the Pick Lines option would use: { id, kind }
  const GRID_ARC = new Set(["arc_3pt", "arc_centre"]);
  let mgrips = []; // screen boxes of the pointer check boxes of the selected elevation marker
  let markerRot = null; // rotation of the marker about to be placed; null turns it to face the model
  let wallSeq = 0;
  // the curves the picks so far make, asked of the model (it draws previews and commits with the same call)
  async function wallCurves(pts, shape = wallShape) {
    const sc = SHAPE_CURVE[shape];
    const r = await Backend.call("sketch_make", { tool: sc.core, points: pts.map((q) => [Math.round(q.x * 1000) / 1000, Math.round(q.y * 1000) / 1000]), options: { ccw: true } });
    return r.curves || [];
  }
  async function refreshWallShape() {
    const sc = SHAPE_CURVE[wallShape];
    if (tool !== "wall" || !sc || !draw || !hover) return;
    const pts = [...draw.pts, hover], my = ++wallSeq;
    if (sc.n === 0 ? pts.length < 2 : draw.pts.length !== sc.n - 1) { draw.preview = null; return; }
    try { const c = await wallCurves(pts); if (my === wallSeq && draw) { draw.preview = c; render(); } }
    catch { if (my === wallSeq && draw) { draw.preview = null; render(); } }
  }
  async function finishCurveWall(extra) {
    const pts = extra ? [...draw.pts, extra] : draw.pts.slice();
    draw = null; cb.prompt(promptText()); render();
    try {
      const c = await wallCurves(pts);
      if (c.length) cb.addWalls(c.map((x) => ({ curve: x.curve })));
      else cb.prompt("Those points do not make a curve. Try again.");
    } catch (e) { cb.prompt(String(e.message || e)); }
  }
  // outline of a wall along a polyline: `l` to the left of it, `r` to the right
  function bandRing(poly, l, r) {
    const L = [], R = [];
    for (let i = 0; i < poly.length; i++) {
      const a = poly[Math.max(0, i - 1)], b = poly[Math.min(poly.length - 1, i + 1)];
      const d = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1, nx = -(b[1] - a[1]) / d, ny = (b[0] - a[0]) / d;
      L.push([poly[i][0] + nx * l, poly[i][1] + ny * l]); R.push([poly[i][0] - nx * r, poly[i][1] - ny * r]);
    }
    return [...L, ...R.reverse()];
  }
  // the wall segments of a shape from its first point and the cursor, drawn clockwise so the exterior
  // (left of the drawing direction) faces out
  function shapeSegments(a, b, shape, sides) {
    let pts = [];
    if (shape === "rectangle") {
      if (Math.abs(a.x - b.x) < 1 || Math.abs(a.y - b.y) < 1) return null;
      pts = [[a.x, a.y], [a.x, b.y], [b.x, b.y], [b.x, a.y]];
    } else if (shape === "polygon_in" || shape === "polygon_out") {
      const r = dist(a, b), n = Math.max(3, Math.min(64, Math.round(sides) || 5));
      if (r < 1) return null;
      // inscribed: the cursor is a corner; circumscribed: the cursor is the middle of a side
      const R = shape === "polygon_in" ? r : r / Math.cos(Math.PI / n), a0 = Math.atan2(b.y - a.y, b.x - a.x) + (shape === "polygon_out" ? Math.PI / n : 0);
      for (let k = 0; k < n; k++) { const t = a0 - (2 * Math.PI * k) / n; pts.push([a.x + R * Math.cos(t), a.y + R * Math.sin(t)]); }
    } else return null;
    const q = (v) => Math.round(v * 1000) / 1000;
    pts = pts.map(([x, y]) => [q(x), q(y)]);
    let area = 0;
    pts.forEach((p, i) => { const n = pts[(i + 1) % pts.length]; area += p[0] * n[1] - n[0] * p[1]; });
    if (area > 0) pts.reverse(); // counter-clockwise: turn it round
    return pts.map((p, i) => [p, pts[(i + 1) % pts.length]]);
  }
  const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
  let nudgePending = null, nudging = false;
  // presses that arrive while a nudge is being worked out are added up and sent as one move
  async function nudge(dx, dy) {
    nudgePending = nudgePending ? { dx: nudgePending.dx + dx, dy: nudgePending.dy + dy } : { dx, dy };
    if (nudging) return;
    nudging = true;
    try {
      while (nudgePending) {
        const n = nudgePending; nudgePending = null;
        await cb.edit({ kind: "move", ids: [...sel], dx: n.dx, dy: n.dy, copy: false, disjoin: false });
      }
    } finally { nudging = false; }
  }
  let wgrips = []; // screen positions of the end grips of the selected walls
  let wgrip = null; // { id, end, wall, fixed, cur, moved } while a wall end is dragged
  let tdimOpen = null; // the label pressed, opened as an input on release (opening on press loses focus to the canvas)
  let tdimInput = null; // the input box over a dimension being edited

  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const size = () => ({ w: canvas.clientWidth, h: canvas.clientHeight });
  const toScreen = (p) => { const { w, h } = size(); return { x: (p.x - cam.x) * cam.s + w / 2, y: h / 2 - (p.y - cam.y) * cam.s }; };
  const toWorld = (sx, sy) => { const { w, h } = size(); return { x: (sx - w / 2) / cam.s + cam.x, y: (h / 2 - sy) / cam.s + cam.y }; };
  const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);

  function resize() {
    const dpr = window.devicePixelRatio || 1;
    const { w, h } = size();
    if (canvas.width !== Math.floor(w * dpr) || canvas.height !== Math.floor(h * dpr)) {
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  let showWorkGrid = false;
  try { showWorkGrid = localStorage.getItem("openarch.showWorkGrid") === "1"; } catch { /* storage can be unavailable */ }
  function setShowWorkGrid(v) { showWorkGrid = !!v; try { localStorage.setItem("openarch.showWorkGrid", v ? "1" : "0"); } catch { /* ignore */ } render(); }

  function gridStep() {
    for (const s of [100, 500, 1000, 5000, 10000, 50000]) if (s * cam.s >= 8) return s;
    return 100000;
  }

  // ---- snapping -------------------------------------------------------------
  let lastMouse = null;
  let snapCycle = 0; // Tab steps through the snap points in reach
  function sketchSegs() {
    const out = [];
    if (!draw) return out;
    draw.pts.forEach((q, i) => { out.push([q.x, q.y, q.x, q.y]); if (i) out.push([draw.pts[i - 1].x, draw.pts[i - 1].y, q.x, q.y]); });
    return out;
  }
  // the points extension snaps line up with: the sketch's, the walls or grid being drawn, a grip where it started
  function trackPts() {
    if (Sketch.active) return Sketch.trackPoints();
    const out = [];
    if (draw) out.push(...draw.pts);
    if (wgrip && wgrip.start) out.push(wgrip.start);
    if (ggrip && ggrip.g.path) out.push(...ggrip.g.path.map(([x, y]) => ({ x, y })));
    if (dragging) out.push(dragging.p0);
    return out;
  }
  function snap(sx, sy, o) {
    // the family editor borrows the snap index while it sketches; take it back
    if (Snap.owner !== "plan") Snap.build(Snap.fromPlan(data, heldBySketch));
    const w = toWorld(sx, sy);
    if (o) return Snap.resolve(w, { zoom: cam.s, last: o.last || null, extra: [], cycle: snapCycle, shift, gridStep: gridStep(), hvDeg: o.hvDeg, track: o.track });
    const last = draw && draw.pts.length ? draw.pts[draw.pts.length - 1] : null;
    if (Sketch.active) return Snap.resolve(w, { zoom: cam.s, last: Sketch.last(), extra: Sketch.segs(), points: Sketch.points(), cycle: snapCycle, shift, gridStep: gridStep(), track: trackPts() });
    return Snap.resolve(w, { zoom: cam.s, last, extra: sketchSegs(), cycle: snapCycle, shift, gridStep: gridStep(), track: trackPts() });
  }

  // ---- hit testing ----------------------------------------------------------
  function inPoly(p, pts) {
    let inside = false;
    for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
      const a = pts[i], b = pts[j];
      if ((a[1] > p.y) !== (b[1] > p.y) && p.x < ((b[0] - a[0]) * (p.y - a[1])) / (b[1] - a[1]) + a[0]) inside = !inside;
    }
    return inside;
  }

  function segDist(p, a, b) {
    const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((p.x - a[0]) * dx + (p.y - a[1]) * dy) / l2)) : 0;
    return Math.hypot(p.x - (a[0] + t * dx), p.y - (a[1] + t * dy));
  }

  function polyEdgeDist(p, pts) {
    let d = Infinity;
    for (let i = 0; i < pts.length; i++) d = Math.min(d, segDist(p, pts[i], pts[(i + 1) % pts.length]));
    return d;
  }

  // distance from p to an arc { c, r, from, to } (degrees, counter-clockwise)
  function arcDist(p, a) {
    const dx = p.x - a.c[0], dy = p.y - a.c[1];
    const ang = (Math.atan2(dy, dx) * 180) / Math.PI;
    const norm = (v) => ((v % 360) + 360) % 360;
    if (norm(ang - a.from) <= norm(a.to - a.from)) return Math.abs(Math.hypot(dx, dy) - a.r);
    const end = (deg) => Math.hypot(p.x - (a.c[0] + a.r * Math.cos((deg * Math.PI) / 180)), p.y - (a.c[1] + a.r * Math.sin((deg * Math.PI) / 180)));
    return Math.min(end(a.from), end(a.to));
  }
  // points along an arc, for crossing selection and the highlight
  function arcPoints(a, n = 12) {
    const sweep = (((a.to - a.from) % 360) + 360) % 360;
    const out = [];
    for (let k = 0; k <= n; k++) { const t = ((a.from + (sweep * k) / n) * Math.PI) / 180; out.push([a.c[0] + a.r * Math.cos(t), a.c[1] + a.r * Math.sin(t)]); }
    return out;
  }
  // the parts of a door or window symbol that pick it besides its footprint in the wall: open leaves and swing arcs
  function insertParts(i) {
    return { polys: i.polys || [], arcs: (i.arcs || []).map((a) => ({ a, pts: arcPoints(a) })) };
  }

  // every selectable element as { id, kind, ring, pinned, category, display }
  function elements() {
    const out = [];
    for (const i of data.inserts) out.push({ id: i.id, kind: "insert", ring: i.footprint, parts: insertParts(i), pinned: i.pinned, category: i.category, display: i.display });
    for (const w of data.walls) out.push({ id: w.id, kind: "wall", ring: w.corners, pinned: w.pinned, category: "Walls", display: w.display });
    for (const f of data.floors) if (f.id !== Sketch.editingId) out.push({ id: f.id, kind: "floor", ring: f.points, rings: f.rings, pinned: f.pinned, category: "Floors", display: f.display });
    for (const r of data.roofs || []) if (r.id !== Sketch.editingId) out.push({ id: r.id, kind: "roof", ring: r.points, rings: r.rings, pinned: r.pinned, category: "Roofs", display: r.display });
    // elevation markers and the project base and survey points are annotation sized: a square round the symbol
    const sq = (x, y, r) => [[x - r, y - r], [x + r, y - r], [x + r, y + r], [x - r, y + r]];
    const rw = symR() * 1.7 / cam.s;
    const gt = 5 / cam.s;
    for (const g of data.grids || []) out.push({ id: g.id, kind: "grid", ring: bandRing(g.path, gt, gt), pinned: g.pinned, category: "Grids", display: "normal" });
    for (const m of data.markers || []) out.push({ id: m.id, kind: "marker", ring: sq(m.x, m.y, rw), pinned: m.pinned, category: "Elevations", display: "normal" });
    for (const d of data.dimensions || []) { const h = Dims.hitRings(d); out.push({ id: d.id, kind: "dimension", ring: h.ring, rings: h.rings, pinned: d.pinned, category: "Dimensions", display: "normal" }); }
    for (const x of data.sections || []) { const h = Sections.hitRings(x); out.push({ id: x.id, kind: "section", ring: h.ring, rings: h.rings, pinned: x.pinned, category: "Sections", display: "normal" }); }
    for (const x of data.spots || []) { const h = Spots.hitRings(x); out.push({ id: x.id, kind: "spot", ring: h.ring, rings: h.rings, pinned: x.pinned, category: x.category, display: "normal" }); }
    // detail lines, regions and text notes (details.js); what a sketch holds is picked there instead
    out.push(...Det.elements());
    const sp = data.site_points;
    if (sp) {
      if (sp.base) out.push({ id: sp.base.id, kind: "sitepoint", ring: sq(sp.base.x, sp.base.y, rw * 0.8), pinned: sp.base.pinned, category: "Project Base Point", display: "normal" });
      if (sp.survey) out.push({ id: sp.survey.id, kind: "sitepoint", ring: sq(sp.survey.x, sp.survey.y, rw * 0.8), pinned: sp.survey.pinned, category: "Survey Point", display: "normal" });
    }
    return out;
  }
  // a selection with groups in it, as the elements drawn for it
  function expandIds(ids) {
    const out = new Set(ids);
    for (const g of modelGroups) if (out.has(g.id)) g.members.forEach((m) => out.add(m));
    return out;
  }
  // what a pick may take while a group is open in Edit Group: its members, or with Add what is in no group
  function editPickable(id) { return groupEdit.mode === "add" ? !memberOf.has(id) : memberOf.get(id) === groupEdit.id; }
  // a click on a member picks its group; Tab reaches the member on its own
  function groupify(c) {
    if (groupEdit) return c.filter((ids) => ids.every(editPickable));
    if (!memberOf.size) return c;
    // each group comes where its first member would, and Tab reaches its members next
    const out = [], byGroup = new Map();
    for (const ids of c) {
      if (ids.length > 1) { const free = ids.filter((i) => !memberOf.has(i)); if (free.length > 1) out.push(free); continue; }
      const g = memberOf.get(ids[0]);
      if (g == null) { out.push(ids); continue; }
      if (!byGroup.has(g)) { byGroup.set(g, []); out.push([g]); }
      byGroup.get(g).push(ids);
    }
    return out.flatMap((ids) => (ids.length === 1 && byGroup.has(ids[0]) ? [ids, ...byGroup.get(ids[0])] : [ids]));
  }
  // a window takes a group when it holds every member drawn here, a crossing box when it touches one
  function groupBand(ids, window) {
    if (groupEdit) return ids.filter(editPickable);
    if (!memberOf.size) return ids;
    const drawn = new Set(elements().map((e) => e.id)), got = new Set(ids);
    const out = ids.filter((i) => !memberOf.has(i));
    for (const g of modelGroups) {
      const here = g.members.filter((m) => drawn.has(m));
      if (here.length && (window ? here.every((m) => got.has(m)) : here.some((m) => got.has(m)))) out.push(g.id);
    }
    return out;
  }
  // the members of a selected or hovered group light up, with the group's origin; in Edit Group the rest fades back
  function drawGroups() {
    const els = elements(), accent = css("--accent");
    const ringsOf = (e) => e.kind === "wall" ? (data.walls.find((w) => w.id === e.id).pieces || [e.ring]) : e.rings && e.rings.length ? e.rings : [e.ring];
    const paint = (ids, stroke, fill, width) => {
      ctx.strokeStyle = stroke; ctx.lineWidth = width;
      for (const e of els) {
        if (!ids.has(e.id)) continue;
        for (const q of ringsOf(e)) { polyPath(q); if (fill) { ctx.fillStyle = fill; ctx.fill(); } ctx.stroke(); }
        if (e.parts) strokeParts(e.parts);
      }
    };
    ctx.save();
    if (groupEdit) {
      ctx.save(); ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.fillStyle = "rgba(255,255,255,0.62)"; ctx.fillRect(0, 0, canvas.width, canvas.height); ctx.restore();
      const g = modelGroups.find((x) => x.id === groupEdit.id);
      // detail items fade on their own (details.js); model members are drawn again over the fade
      paint(new Set((g ? g.members : []).filter((m) => !Det.elements().some((e) => e.id === m))), "#202428", null, 1.4);
      paint(new Set([...sel].filter((i) => memberOf.get(i) === groupEdit.id || !memberOf.has(i))), accent, "rgba(46,111,158,0.45)", 2.5);
    }
    const hov = tool === "select" && selHover && !band ? selHover : [];
    for (const g of modelGroups) {
      const on = sel.has(g.id), over = !on && hov.includes(g.id);
      if (!on && !over) continue;
      paint(new Set(g.members), on ? accent : "#d4572f", on ? "rgba(46,111,158,0.35)" : null, on ? 2.5 : 2);
      if (on) {
        // the group's origin: a small square with a cross
        const o = toScreen(g.origin);
        ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.fillStyle = "#fff";
        ctx.fillRect(o.x - 5, o.y - 5, 10, 10); ctx.strokeRect(o.x - 5, o.y - 5, 10, 10);
        ctx.beginPath(); ctx.moveTo(o.x - 9, o.y); ctx.lineTo(o.x + 9, o.y); ctx.moveTo(o.x, o.y - 9); ctx.lineTo(o.x, o.y + 9); ctx.stroke();
      }
    }
    ctx.restore();
  }
  // detail items a sketch is holding: the view's detail lines while Detail Line is on, a region's edges in Edit Boundary
  function heldBySketch(x) {
    if (!Sketch.active) return false;
    if (Sketch.kind === "detail") return x.line != null;
    return Sketch.kind === "region" && Sketch.editingId != null && x.region === Sketch.editingId;
  }
  // radius of the marker and point symbols in pixels: 3.5 mm on paper at the view scale, so they keep their printed size against the model
  function symR() { const denom = (data.view && data.view.scale) || 100; return Math.max(2, 3.5 * denom * cam.s); }
  const selectable = (e) => !(e.display === "underlay" && !selOpts.underlay) && !(e.pinned && !selOpts.pinned);

  // walls joined end to end with this one, so Tab can pick the whole run
  function wallChain(id) {
    const by = new Map(data.walls.map((w) => [w.id, w]));
    const near = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]) <= 1;
    const seen = new Set([id]), stack = [id];
    while (stack.length) {
      const a = by.get(stack.pop());
      for (const w of data.walls) {
        if (seen.has(w.id) || w.display === "underlay") continue;
        if ([a.start, a.end].some((p) => [w.start, w.end].some((q) => near(p, q)))) { seen.add(w.id); stack.push(w.id); }
      }
    }
    return [...seen];
  }

  // what a click here can pick, best first: each choice is a list of ids. Tab steps through them
  // (overlapping elements, then the run of connected walls).
  function candidates(sx, sy) { return groupify(rawCandidates(sx, sy)); }
  function rawCandidates(sx, sy) {
    const p = toWorld(sx, sy);
    const tol = 4 / cam.s;
    const rank = { marker: -1, sitepoint: -1, section: -1, dimension: -1, spot: -1, note: -1, detail: 0, grid: 0, insert: 0, region: 1, wall: 1, roof: 2, floor: 3 };
    const found = [];
    for (const e of elements()) {
      if (!selectable(e)) continue;
      const all = e.rings && e.rings.length ? e.rings : [e.ring];
      const edge = Math.min(...all.map((r) => polyEdgeDist(p, r)));
      // a floor with an opening is inside only where the rings cover it an odd number of times
      const inside = all.filter((r) => inPoly(p, r)).length % 2 === 1;
      // with Select Elements by Face off only the edges pick
      let d = selOpts.face && inside ? 0 : edge <= tol ? edge : Infinity;
      if (e.parts) {
        // a door is also picked by its open leaf and its swing arc, not only by the opening in the wall
        for (const q of e.parts.polys) {
          const pe = polyEdgeDist(p, q);
          const pd = selOpts.face && inPoly(p, q) ? 0 : pe <= tol ? pe : Infinity;
          d = Math.min(d, pd);
        }
        for (const { a } of e.parts.arcs) { const ad = arcDist(p, a); if (ad <= tol) d = Math.min(d, ad); }
      }
      if (d !== Infinity) found.push({ e, d });
    }
    found.sort((a, b) => rank[a.e.kind] - rank[b.e.kind] || a.d - b.d);
    const out = found.map((f) => [f.e.id]);
    const firstWall = found.find((f) => f.e.kind === "wall");
    if (firstWall) {
      const chain = wallChain(firstWall.e.id);
      if (chain.length > 1) out.push(chain);
    }
    return out;
  }

  function hit(sx, sy) {
    const c = candidates(sx, sy);
    return c.length ? c[0][0] : null;
  }

  function segsCross(a, b, c, d) {
    const o = (p, q, r) => (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    return o(a, b, c) * o(a, b, d) < 0 && o(c, d, a) * o(c, d, b) < 0;
  }
  function worldRect(b) {
    const a = toWorld(b.x0, b.y0), c = toWorld(b.x1, b.y1);
    return { x0: Math.min(a.x, c.x), x1: Math.max(a.x, c.x), y0: Math.min(a.y, c.y), y1: Math.max(a.y, c.y) };
  }
  // Window (dragged left to right): fully inside. Crossing (right to left): touched.
  function bandPick(b) {
    const r = worldRect(b), window = b.x1 >= b.x0;
    const inR = (q) => q[0] >= r.x0 && q[0] <= r.x1 && q[1] >= r.y0 && q[1] <= r.y1;
    const rc = [[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]];
    const picked = elements().filter((e) => {
      if (!selectable(e)) return false;
      // a door counts its open leaves and swing arcs as part of itself
      const extra = e.parts ? [...e.parts.polys, ...e.parts.arcs.map((x) => x.pts)] : [];
      if (window) return e.ring.every(inR) && extra.every((q) => q.every(inR));
      const touches = (ring, closed) => {
        if (ring.some(inR) || (closed && rc.some((q) => inPoly({ x: q[0], y: q[1] }, ring)))) return true;
        const n = closed ? ring.length : ring.length - 1;
        for (let i = 0; i < n; i++) for (let k = 0; k < 4; k++) if (segsCross(ring[i], ring[(i + 1) % ring.length], rc[k], rc[(k + 1) % 4])) return true;
        return false;
      };
      return touches(e.ring, true) || (e.parts ? e.parts.polys.some((q) => touches(q, true)) || e.parts.arcs.some((x) => touches(x.pts, false)) : false);
    }).map((e) => e.id);
    return groupBand(picked, window);
  }

  // ---- drawing --------------------------------------------------------------
  // outline the open leaves and swing arcs of a door (hover and selection highlight)
  function strokeParts(parts) {
    for (const q of parts.polys) { polyPath(q); ctx.stroke(); }
    for (const { pts } of parts.arcs) {
      ctx.beginPath();
      pts.forEach((q, k) => { const s2 = toScreen({ x: q[0], y: q[1] }); k ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
      ctx.stroke();
    }
  }
  function polyPath(pts) {
    ctx.beginPath();
    pts.forEach((q, i) => { const s = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
    ctx.closePath();
  }

  function label(text, at, color) {
    ctx.font = "11px system-ui, sans-serif";
    const m = ctx.measureText(text);
    ctx.fillStyle = css("--panel");
    ctx.fillRect(at.x - m.width / 2 - 3, at.y - 8, m.width + 6, 16);
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(text, at.x, at.y);
  }


  // ---- temporary dimensions -----------------------------------------------------------------
  const fmtMM = (v) => (Math.abs(v - Math.round(v)) < 0.05 ? String(Math.round(v)) : v.toFixed(1));
  // a dimension with the end being dragged moved onto the line it would snap to
  function draggedDim(d) {
    if (!tgrip || !tgrip.lines || !tgrip.cand || tgrip.dim.key !== d.key) return d;
    const { origin, axis } = tgrip.lines, along = (q) => (q[0] - origin[0]) * axis[0] + (q[1] - origin[1]) * axis[1];
    const moveTo = (q) => { const k = tgrip.cand.pos - along(q); return [q[0] + axis[0] * k, q[1] + axis[1] * k]; };
    const e = { ...d };
    if (tgrip.role === "own") e.a = moveTo(d.a); else e.b = moveTo(d.b);
    e.value = Math.abs(along(e.b) - along(e.a));
    return e;
  }

  // The line a click on a dimension's end moves it to next: the lines of that element in order from the
  // outside in (the selected element's outermost line first, the neighbour's nearest line first).
  function cycleCand(g) {
    const L = g.lines;
    if (!L) return null;
    const along = (q) => (q[0] - L.origin[0]) * L.axis[0] + (q[1] - L.origin[1]) * L.axis[1];
    let list = g.role === "own" ? L.own : L.targets;
    if (g.role !== "own" && g.dim.to != null) {
      const same = list.filter((c) => c.spec.id === g.dim.to);
      if (same.length) list = same;
    }
    const s = Math.sign(g.dim.normal[0] * L.axis[0] + g.dim.normal[1] * L.axis[1]) || 1;
    const k = g.role === "own" ? -s : s;
    const sorted = [...list].sort((a, b) => k * (a.pos - b.pos));
    const lines = sorted.filter((c, i) => i === 0 || Math.abs(c.pos - sorted[i - 1].pos) > 0.5);
    if (lines.length < 2) return null;
    const cur = along(g.role === "own" ? g.dim.a : g.dim.b);
    let at = 0;
    lines.forEach((c, i) => { if (Math.abs(c.pos - cur) < Math.abs(lines[at].pos - cur)) at = i; });
    return lines[(at + 1) % lines.length];
  }

  function drawTempDims(list, live) {
    const col = live ? "#1d6fb8" : "rgba(29,111,184,0.7)";
    ctx.save();
    ctx.lineWidth = 1;
    ctx.font = "11px system-ui, sans-serif";
    if (live && tgrip && tgrip.cand) {
      // the line the dragged end will snap to
      const c = tgrip.cand, P = toScreen({ x: c.a[0], y: c.a[1] }), Q = toScreen({ x: c.b[0], y: c.b[1] });
      ctx.strokeStyle = col; ctx.lineWidth = 2.5; ctx.globalAlpha = 0.8; ctx.setLineDash([8, 4]);
      ctx.beginPath(); ctx.moveTo(P.x, P.y); ctx.lineTo(Q.x, Q.y); ctx.stroke();
      ctx.setLineDash([]); ctx.globalAlpha = 1; ctx.lineWidth = 1;
      label(c.label, { x: tgrip.x + 14 + ctx.measureText(c.label).width / 2, y: tgrip.y - 16 }, col);
    }
    for (const d0 of list) {
      const d = draggedDim(d0);
      const A = toScreen({ x: d.a[0], y: d.a[1] }), B = toScreen({ x: d.b[0], y: d.b[1] });
      const straight = d.key === "up" || d.key === "down";
      // a length dimension clears the body of a thick wall as well as the usual gap
      const wall = d.key === "length" ? data.walls.find((w) => sel.has(w.id)) : null;
      const clear = wall && wall.extents ? Math.min(120, Math.max(Math.abs(wall.extents[0]), Math.abs(wall.extents[1])) * cam.s) : 0;
      const off = straight ? 0 : 30 + clear, nx = d.normal[0], ny = -d.normal[1];
      const A2 = { x: A.x + nx * off, y: A.y + ny * off }, B2 = { x: B.x + nx * off, y: B.y + ny * off };
      const len = Math.hypot(B2.x - A2.x, B2.y - A2.y);
      // zoomed out until the figure would cover the dimension itself: leave it out until the view comes in
      const label0 = fmtMM(d.value);
      if (len < ctx.measureText(label0).width * 0.75) continue;
      ctx.strokeStyle = col; ctx.fillStyle = col; ctx.setLineDash([]);
      if (off) {
        ctx.globalAlpha = 0.55; ctx.beginPath();
        ctx.moveTo(A.x, A.y); ctx.lineTo(A2.x + nx * 5, A2.y + ny * 5);
        ctx.moveTo(B.x, B.y); ctx.lineTo(B2.x + nx * 5, B2.y + ny * 5);
        ctx.stroke(); ctx.globalAlpha = 1;
      } else { ctx.setLineDash([5, 3]); }
      ctx.beginPath(); ctx.moveTo(A2.x, A2.y); ctx.lineTo(B2.x, B2.y); ctx.stroke(); ctx.setLineDash([]);
      const ux = (B2.x - A2.x) / len, uy = (B2.y - A2.y) / len, tx = (ux - uy) * 0.7071 * 5, ty = (ux + uy) * 0.7071 * 5;
      ctx.beginPath();
      for (const q of [A2, B2]) { ctx.moveTo(q.x - tx, q.y - ty); ctx.lineTo(q.x + tx, q.y + ty); }
      ctx.stroke();
      // the value, in a box that opens an input when clicked
      const text = fmtMM(d.value), tw = ctx.measureText(text).width + 12, cx = (A2.x + B2.x) / 2, cy = (A2.y + B2.y) / 2;
      ctx.fillStyle = "#ffffff"; ctx.strokeStyle = col;
      ctx.fillRect(cx - tw / 2, cy - 9, tw, 18); ctx.strokeRect(cx - tw / 2 + 0.5, cy - 8.5, tw, 18);
      ctx.fillStyle = col; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(text, cx, cy + 0.5);
      if (live) {
        if (d.edit) tdimUI.push({ type: "label", x: cx, y: cy, w: tw, h: 18, dim: d0 });
        // a small dimension glyph beside the figure makes the temporary dimension permanent
        const px = cx + tw / 2 + 11;
        ctx.fillStyle = "#ffffff"; ctx.strokeStyle = col; ctx.lineWidth = 1;
        ctx.fillRect(px - 8, cy - 8, 16, 16); ctx.strokeRect(px - 7.5, cy - 7.5, 16, 16);
        ctx.beginPath(); ctx.moveTo(px - 5, cy - 4); ctx.lineTo(px - 5, cy + 4); ctx.moveTo(px + 5, cy - 4); ctx.lineTo(px + 5, cy + 4); ctx.moveTo(px - 5, cy); ctx.lineTo(px + 5, cy); ctx.stroke();
        tdimUI.push({ type: "promote", x: px, y: cy, r: 9, dim: d0 });
        if (["up", "down", "left", "right"].includes(d.key)) {
          // a grip on each end: the one on the selected element picks which of its lines is measured from,
          // the other which line is measured to
          for (const [role, q] of [["own", A2], ["target", B2]]) {
            const g = tgrip && tgrip.dim.key === d.key && tgrip.role === role ? { x: tgrip.x, y: tgrip.y } : q;
            ctx.fillStyle = "#ffffff"; ctx.strokeStyle = col; ctx.lineWidth = 1.5;
            ctx.fillRect(g.x - 4, g.y - 4, 8, 8); ctx.strokeRect(g.x - 4, g.y - 4, 8, 8);
            ctx.lineWidth = 1;
            tdimUI.push({ type: "grip", role, x: q.x, y: q.y, r: 8, dim: d0 });
          }
        }
      }
    }
    ctx.restore();
  }

  function tdimHit(sx, sy) {
    for (const u of tdimUI) {
      if (u.type === "label" && Math.abs(sx - u.x) <= u.w / 2 && Math.abs(sy - u.y) <= u.h / 2) return u;
      if ((u.type === "grip" || u.type === "promote") && Math.hypot(sx - u.x, sy - u.y) <= u.r) return u;
    }
    return null;
  }

  function closeTdimInput() {
    const i = tdimInput;
    tdimInput = null;
    if (i && i.parentNode) i.parentNode.removeChild(i);
  }

  function openTdimInput(box) {
    closeTdimInput();
    const r = canvas.getBoundingClientRect();
    const inp = document.createElement("input");
    inp.type = "text"; inp.value = fmtMM(box.dim.value); inp.setAttribute("aria-label", "Dimension value");
    inp.id = "tdim-input";
    inp.style.cssText = `position:fixed;left:${r.left + box.x - 38}px;top:${r.top + box.y - 12}px;width:76px;height:24px;z-index:60;text-align:center;font:12px system-ui,sans-serif;border:1.5px solid #1d6fb8;border-radius:3px;background:#fff;color:#111;`;
    document.body.appendChild(inp);
    inp.focus(); inp.select();
    inp.onkeydown = (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { const v = parseLengthMM(inp.value); closeTdimInput(); if (!isNaN(v)) cb.tempEdit(box.dim, Math.round(v * 1000) / 1000); }
      else if (e.key === "Escape") closeTdimInput();
    };
    inp.onblur = () => { if (tdimInput === inp) closeTdimInput(); };
    tdimInput = inp;
  }

  // click a head of a selected grid to type a new name
  function openGridName(b) {
    closeTdimInput();
    const g = (data.grids || []).find((x) => x.id === b.id);
    if (!g) return;
    const r = canvas.getBoundingClientRect();
    const inp = document.createElement("input");
    inp.type = "text"; inp.value = g.name; inp.setAttribute("aria-label", "Grid name"); inp.id = "grid-name-input";
    inp.style.cssText = `position:fixed;left:${r.left + b.x - 36}px;top:${r.top + b.y - 12}px;width:72px;height:24px;z-index:60;text-align:center;font:12px system-ui,sans-serif;border:1.5px solid #1d6fb8;border-radius:3px;background:#fff;color:#111;`;
    document.body.appendChild(inp);
    inp.focus(); inp.select();
    inp.onkeydown = (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { const v = inp.value.trim(); closeTdimInput(); if (v && v !== g.name) cb.patchGrid(g.id, { name: v }); }
      else if (e.key === "Escape") closeTdimInput();
    };
    inp.onblur = () => { if (tdimInput === inp) closeTdimInput(); };
    tdimInput = inp;
  }

  // dimensions of the door or window about to be placed follow the cursor along its wall
  function updatePlaceDims() {
    if (!placeHover) { placeDims = []; placeKey = ""; return; }
    const key = placeHover.wall + ":" + Math.round(placeHover.u);
    if (key === placeKey) return;
    placeKey = key;
    const seq = ++placeSeq;
    Promise.resolve(cb.placeDims(placeHover.wall, { x: placeHover.p.x, y: placeHover.p.y })).then((d) => { if (seq === placeSeq) { placeDims = d || []; render(); } });
  }


  // ---- grids ------------------------------------------------------------------------------
  // Pick Lines: the wall or grid under the cursor, and the grid the click would make (straight lines only)
  function gridPickAt(sx, sy) {
    const p = toWorld(sx, sy), tol = 10 / cam.s;
    let best = null;
    const consider = (id, kind, a, b, d) => { if (d <= tol && (!best || d < best.d)) best = { id, kind, d, line: a && b ? [a, b] : null }; };
    for (const w of data.walls) {
      if (w.display === "underlay") continue;
      if (!w.curve) consider(w.id, "wall", w.start, w.end, segDist(p, w.start, w.end));
      else if (inPoly(p, w.corners)) consider(w.id, "wall", null, null, 0);
    }
    for (const g of data.grids || []) {
      if (g.path.length === 2) consider(g.id, "grid", g.path[0], g.path[1], segDist(p, g.path[0], g.path[1]));
      else if (polyEdgeDist(p, g.path) <= tol) consider(g.id, "grid", null, null, 0);
    }
    if (best && best.line && gridOffset > 0) {
      // the new line sits to the side of the picked one the cursor is on
      const [a, b] = best.line, dx = b[0] - a[0], dy = b[1] - a[1], L = Math.hypot(dx, dy) || 1;
      const side = ((p.x - a[0]) * -dy + (p.y - a[1]) * dx) >= 0 ? 1 : -1, nx = (-dy / L) * side * gridOffset, ny = (dx / L) * side * gridOffset;
      best.result = [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny]];
    } else if (best && best.line) best.result = best.line;
    return best;
  }
  async function refreshGridShape() {
    if (tool !== "grid" || !GRID_ARC.has(gridShape) || !draw || !hover) return;
    const sc = SHAPE_CURVE[gridShape], pts = [...draw.pts, hover], my = ++wallSeq;
    if (draw.pts.length !== sc.n - 1) { draw.preview = null; return; }
    try { const c = await wallCurves(pts, gridShape); if (my === wallSeq && draw) { draw.preview = c; render(); } }
    catch { if (my === wallSeq && draw) { draw.preview = null; render(); } }
  }
  async function finishGridArc(extra) {
    const pts = [...draw.pts, extra];
    draw = null; cb.prompt(promptText()); render();
    try {
      const c = await wallCurves(pts, gridShape);
      if (c.length) cb.addGrid([c[0].curve]);
      else cb.prompt("Those points do not make an arc. Try again.");
    } catch (e) { cb.prompt(String(e.message || e)); }
  }
  function finishGridChain() {
    const pts = draw ? draw.pts : [];
    draw = null; cb.prompt(promptText()); render();
    if (pts.length < 2) return;
    const q = (v) => Math.round(v * 1000) / 1000, curves = [];
    for (let i = 1; i < pts.length; i++) curves.push({ type: "line", a: { x: q(pts[i - 1].x), y: q(pts[i - 1].y) }, b: { x: q(pts[i].x), y: q(pts[i].y) } });
    cb.addGrid(curves);
  }
  function gridClick(p, sx, sy) {
    const q = (v) => Math.round(v * 1000) / 1000;
    if (gridShape === "pick") {
      const h = gridPickAt(sx, sy);
      if (h) cb.gridFromLine(h.id, gridOffset, { x: q(toWorld(sx, sy).x), y: q(toWorld(sx, sy).y) });
    } else if (gridShape === "line") {
      if (!draw) draw = { pts: [p], startAlign: p.align || null };
      else if (dist(draw.pts[0], p) > 1) {
        const locks = [];
        if (draw.startAlign) locks.push({ end: 0, id: draw.startAlign.id, otherEnd: draw.startAlign.end });
        if (p.align) locks.push({ end: 1, id: p.align.id, otherEnd: p.align.end });
        cb.addGrid([{ type: "line", a: { x: q(draw.pts[0].x), y: q(draw.pts[0].y) }, b: { x: q(p.x), y: q(p.y) } }], locks);
        draw = null; gridAlignHint = null;
      }
    } else if (gridShape === "multi") {
      if (!draw) draw = { pts: [p] };
      else if (dist(draw.pts[draw.pts.length - 1], p) > 1) draw.pts.push(p);
    } else if (GRID_ARC.has(gridShape)) {
      const sc = SHAPE_CURVE[gridShape];
      if (!draw) draw = { pts: [p] };
      else if (draw.pts.length === sc.n - 1) { finishGridArc(p); return; }
      else draw.pts.push(p);
    }
    cb.prompt(promptText());
    render();
  }
  // ---- alignment of grid ends: the alignment indicator and alignment pin ------------------------
  const straightGrids = () => (data.grids || []).filter((g) => g.path && g.path.length === 2 && !g.is_arc);
  const gUnit = (g) => { const [a, b] = g.path, l = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1; return [(b[0] - a[0]) / l, (b[1] - a[1]) / l]; };
  const PARALLEL = 0.026; // sine of about 1.5 degrees
  // Where a point lines up with an end of another straight grid. With `dir` only grids parallel to it count, and
  // with `from` the new line is held exactly parallel to the grid it lines up with.
  function gridAlign(p, o = {}) {
    const tol = 8 / cam.s;
    let best = null;
    for (const g of straightGrids()) {
      if (o.exclude && o.exclude.has(g.id)) continue;
      const u = gUnit(g);
      if (o.dir && Math.abs(o.dir[0] * u[1] - o.dir[1] * u[0]) > PARALLEL) continue;
      for (const end of [0, 1]) {
        const e = g.path[end];
        const dt = (p.x - e[0]) * u[0] + (p.y - e[1]) * u[1];
        if (Math.abs(dt) > tol) continue;
        const score = Math.abs(dt) + (o.end === end ? 0 : tol * 0.01);
        if (!best || score < best.score) best = { score, g, end, u, e, dt };
      }
    }
    if (!best) return null;
    const { g, end, u, e, dt } = best;
    let q;
    if (o.from) { const t = (e[0] - o.from.x) * u[0] + (e[1] - o.from.y) * u[1]; q = { x: o.from.x + u[0] * t, y: o.from.y + u[1] * t }; }
    else q = { x: p.x - u[0] * dt, y: p.y - u[1] * dt };
    return { ...q, kind: "align", label: "Aligned with grid " + g.name, cands: [], index: 0, align: { id: g.id, end }, u, e };
  }
  // the snapped cursor for the grid tool: object snaps first, then lining up with the end of another grid
  function gridSnap(sx, sy) {
    const h = snap(sx, sy);
    if (!["free", "length", "hv", "grid"].includes(h.kind) || (gridShape !== "line" && gridShape !== "multi")) { gridAlignHint = null; return h; }
    const from = draw && draw.pts.length ? draw.pts[draw.pts.length - 1] : null;
    let dir = null;
    if (from) { const l = Math.hypot(h.x - from.x, h.y - from.y); if (l > 1) dir = [(h.x - from.x) / l, (h.y - from.y) / l]; }
    const a = gridAlign(h, { dir, from: gridShape === "line" ? from : null });
    gridAlignHint = a;
    return a || h;
  }
  // the dashed line through the ends that line up, the padlocks between them
  function alignLine(e, u, extraPts) {
    const n = [-u[1], u[0]];
    const ends = [];
    for (const g of straightGrids()) {
      const gu = gUnit(g);
      if (Math.abs(gu[0] * u[1] - gu[1] * u[0]) > PARALLEL) continue;
      for (const end of [0, 1]) { const q = g.path[end]; if (Math.abs((q[0] - e[0]) * u[0] + (q[1] - e[1]) * u[1]) < 1) ends.push(q); }
    }
    const ns = [...ends, ...extraPts].map((q) => (q[0] - e[0]) * n[0] + (q[1] - e[1]) * n[1]);
    if (!ns.length) return;
    const pad = 36 / cam.s, lo = Math.min(...ns) - pad, hi = Math.max(...ns) + pad;
    const a = toScreen({ x: e[0] + n[0] * lo, y: e[1] + n[1] * lo }), b = toScreen({ x: e[0] + n[0] * hi, y: e[1] + n[1] * hi });
    ctx.save(); ctx.strokeStyle = "#3aa7d8"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
    ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
  }
  const padlock = (x, y, locked) => drawPadlock(ctx, x, y, locked, css("--accent"));

  function drawGridAlign() {
    if (ggrip && ggrip.moved && gridAlignHint) alignLine(gridAlignHint.e, gridAlignHint.u, [[hover.x, hover.y]]);
    else if (tool === "grid" && gridAlignHint && hover) alignLine(gridAlignHint.e, gridAlignHint.u, [[hover.x, hover.y]]);
  }
  // padlocks for the selected grids: a closed one where the end is locked in line with another, an open one
  // where the nearest neighbour on each side happens to line up
  function drawGridPins() {
    gpins = [];
    const gs = straightGrids(), done = new Set();
    for (const g of gs) {
      if (!sel.has(g.id)) continue;
      const u = gUnit(g), n = [-u[1], u[0]];
      for (const end of [0, 1]) {
        const locks = ((g.end_locks || [[], []])[end] || []).filter((r) => gs.some((o) => o.id === r.id));
        const near = { 1: null, [-1]: null };
        for (const o of gs) {
          if (o.id === g.id) continue;
          const uo = gUnit(o);
          if (Math.abs(u[0] * uo[1] - u[1] * uo[0]) > PARALLEL) continue;
          for (const oe of [0, 1]) {
            const a = g.path[end], b = o.path[oe];
            if (Math.abs((a[0] - b[0]) * u[0] + (a[1] - b[1]) * u[1]) >= 1) continue;
            const side = ((b[0] - a[0]) * n[0] + (b[1] - a[1]) * n[1]) > 0 ? 1 : -1;
            const off = Math.abs((b[0] - a[0]) * n[0] + (b[1] - a[1]) * n[1]);
            if (!near[side] || off < near[side].off) near[side] = { o, oe, off };
          }
        }
        const pairs = [];
        for (const r of locks) pairs.push({ o: gs.find((x) => x.id === r.id), oe: r.end, locked: true });
        for (const side of [1, -1]) {
          const nb = near[side];
          if (nb && !locks.some((r) => r.id === nb.o.id && r.end === nb.oe)) pairs.push({ o: nb.o, oe: nb.oe, locked: false });
        }
        for (const pr of pairs) {
          const key = [Math.min(g.id, pr.o.id), Math.max(g.id, pr.o.id), end].join("-") + (pr.locked ? "L" : "U");
          if (done.has(key)) continue;
          done.add(key);
          const a = toScreen({ x: g.path[end][0], y: g.path[end][1] }), b = toScreen({ x: pr.o.path[pr.oe][0], y: pr.o.path[pr.oe][1] });
          const m = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
          // keep the padlock off the line itself so it can be clicked
          ctx.save(); ctx.strokeStyle = "#7a8aa0"; ctx.lineWidth = 0.8; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
          padlock(m.x, m.y, pr.locked);
          gpins.push({ x: m.x, y: m.y, id: g.id, end, other: pr.o.id, otherEnd: pr.oe, locked: pr.locked });
          alignLine(g.path[end], u, [pr.o.path[pr.oe]]);
        }
      }
    }
  }

  // the part of a screen polyline between two distances along it
  function polySlice(pts, s0, s1) {
    const out = [];
    let at = 0;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = pts[i], L = Math.hypot(b.x - a.x, b.y - a.y);
      if (L < 1e-9) continue;
      const lo = Math.max(s0, at), hi = Math.min(s1, at + L);
      if (hi > lo) {
        const p = (t) => ({ x: a.x + ((b.x - a.x) * (t - at)) / L, y: a.y + ((b.y - a.y) * (t - at)) / L });
        if (!out.length) out.push(p(lo));
        out.push(p(hi));
      }
      at += L;
    }
    return out;
  }
  const polyLen = (pts) => pts.reduce((m, q, i) => (i ? m + Math.hypot(q.x - pts[i - 1].x, q.y - pts[i - 1].y) : 0), 0);
  function strokePoly(pts) { if (pts.length < 2) return; ctx.beginPath(); pts.forEach((q, i) => (i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y))); ctx.stroke(); }

  // where the head of a grid end sits: its centre, radius and the direction the line leaves in
  function gridHead(g, pts, end) {
    const P = end === 0 ? pts[0] : pts[pts.length - 1];
    const Q = end === 0 ? pts[Math.min(1, pts.length - 1)] : pts[Math.max(0, pts.length - 2)];
    const L = Math.hypot(P.x - Q.x, P.y - Q.y) || 1, u = { x: (P.x - Q.x) / L, y: (P.y - Q.y) / L };
    const denom = (data.view && data.view.scale) || 100;
    const R = Math.min(30, Math.max(9, (g.bubble_dia_mm / 2) * denom * cam.s));
    const off = g.head_offsets && g.head_offsets[end];
    const c = { x: P.x + u.x * R + (off ? off[0] * cam.s : 0), y: P.y + u.y * R - (off ? off[1] * cam.s : 0) };
    return { P, u, R, c, offset: !!off };
  }

  function drawGridHead(g, pts, end, col, selected) {
    const h = gridHead(g, pts, end);
    ctx.save();
    ctx.strokeStyle = col; ctx.lineWidth = selected ? 2 : 1.2; ctx.setLineDash([]);
    if (h.offset) { ctx.beginPath(); ctx.moveTo(h.P.x, h.P.y); ctx.lineTo(h.c.x, h.c.y); ctx.stroke(); }
    ctx.beginPath(); ctx.arc(h.c.x, h.c.y, h.R, 0, Math.PI * 2); ctx.fillStyle = css("--view-bg"); ctx.fill(); ctx.stroke();
    let size = Math.round(h.R * 0.95);
    ctx.font = `600 ${size}px system-ui, sans-serif`;
    const w = ctx.measureText(g.name).width;
    if (w > h.R * 1.7) { size = Math.max(6, Math.floor((size * h.R * 1.7) / w)); ctx.font = `600 ${size}px system-ui, sans-serif`; }
    ctx.fillStyle = col; ctx.textAlign = "center"; ctx.textBaseline = "middle";
    ctx.fillText(g.name, h.c.x, h.c.y + 0.5);
    ctx.restore();
    return h;
  }

  function drawGrids() {
    ggrips = []; gboxes = []; gbubbles = [];
    const text = css("--text"), accent = css("--accent");
    const denom = (data.view && data.view.scale) || 100, paperPx = denom * cam.s;
    for (const g of data.grids || []) {
      let path = g.path;
      // an end being dragged shows where it will land
      if (ggrip && ggrip.id === g.id && ggrip.moved && ggrip.preview) path = ggrip.preview;
      const pts = path.map((q) => toScreen({ x: q[0], y: q[1] }));
      if (pts.length < 2) continue;
      const selected = sel.has(g.id), hovered = (selHover && selHover.includes(g.id)) || (gridHover && gridHover.id === g.id);
      const base = g.style.line.color;
      const col = selected || hovered ? accent : base === "#000000" ? text : base;
      const dashArr = (((data.line_patterns || {})[g.style.line.pattern]) || []).map((v) => Math.max(v * paperPx, 0.8));
      ctx.save();
      ctx.strokeStyle = col; ctx.lineWidth = (thin ? 1 : Math.max(1, g.style.line.width_mm * paperPx)) + (selected ? 0.8 : 0); ctx.lineJoin = "round";
      if (g.centre_segment === "none") { ctx.setLineDash(dashArr); strokePoly(pts); }
      else {
        // patterned ends round a solid middle (Center Segment)
        const total = polyLen(pts), e = Math.min(g.end_segment_mm * paperPx, total / 2);
        ctx.setLineDash(dashArr); strokePoly(polySlice(pts, 0, e)); strokePoly(polySlice(pts, total - e, total));
        ctx.setLineDash([]); strokePoly(polySlice(pts, e, total - e));
      }
      ctx.restore();
      for (const end of [0, 1]) {
        if (g.bubbles[end]) {
          const h = drawGridHead(g, pts, end, col, selected);
          gbubbles.push({ id: g.id, end, x: h.c.x, y: h.c.y, r: h.R });
        }
        if (selected) {
          const h = gridHead(g, pts, end);
          // the end grip, and a check box beyond the head to show or hide it
          ctx.fillStyle = accent; ctx.beginPath(); ctx.arc(h.P.x, h.P.y, 4, 0, Math.PI * 2); ctx.fill();
          const bx = h.P.x + h.u.x * (h.R * (g.bubbles[end] ? 2 : 0) + 16), by = h.P.y + h.u.y * (h.R * (g.bubbles[end] ? 2 : 0) + 16);
          ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.fillStyle = css("--view-bg");
          ctx.fillRect(bx - 5, by - 5, 10, 10); ctx.strokeRect(bx - 5, by - 5, 10, 10);
          if (g.bubbles[end]) { ctx.beginPath(); ctx.moveTo(bx - 3, by); ctx.lineTo(bx - 1, by + 3); ctx.lineTo(bx + 4, by - 3); ctx.stroke(); }
          ggrips.push({ id: g.id, end, x: h.P.x, y: h.P.y, g });
          gboxes.push({ id: g.id, end, x: bx, y: by, on: g.bubbles[end], g });
        }
      }
    }
    drawGridPins();
  }

  // the grid being drawn: the picks so far and what the next click would make
  function drawGridPreview() {
    const accent = css("--accent");
    ctx.save();
    ctx.strokeStyle = accent; ctx.fillStyle = accent; ctx.lineWidth = 1.5; ctx.setLineDash([14, 4, 3, 4]);
    if (gridShape === "pick") {
      if (gridHover && gridHover.line) {
        ctx.setLineDash([]); ctx.lineWidth = 2.5; ctx.globalAlpha = 0.5;
        const [a, b] = gridHover.line.map((q) => toScreen({ x: q[0], y: q[1] }));
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
        ctx.globalAlpha = 1;
        if (gridHover.result) {
          ctx.setLineDash([14, 4, 3, 4]); ctx.lineWidth = 1.5;
          const [c, d] = gridHover.result.map((q) => toScreen({ x: q[0], y: q[1] }));
          ctx.beginPath(); ctx.moveTo(c.x, c.y); ctx.lineTo(d.x, d.y); ctx.stroke();
        }
      }
    } else if (draw && draw.pts.length && hover) {
      const last = draw.pts[draw.pts.length - 1];
      if (GRID_ARC.has(gridShape)) {
        ctx.setLineDash([4, 4]); ctx.lineWidth = 1;
        ctx.beginPath(); [...draw.pts, hover].forEach((q, i) => { const sp = toScreen(q); i ? ctx.lineTo(sp.x, sp.y) : ctx.moveTo(sp.x, sp.y); }); ctx.stroke();
        ctx.setLineDash([14, 4, 3, 4]); ctx.lineWidth = 1.5;
        for (const c of draw.preview || []) { polyPath(c.poly); ctx.stroke(); }
      } else {
        const pts = [...draw.pts, hover].map((q) => toScreen(q));
        strokePoly(pts);
        ctx.setLineDash([]);
        const tl = parseFloat(draw.typed);
        label(draw.typed ? `${draw.typed}|` : `${Math.round(dist(last, hover))}`, { x: (pts[pts.length - 1].x + pts[pts.length - 2].x) / 2, y: (pts[pts.length - 1].y + pts[pts.length - 2].y) / 2 - 12 }, accent);
      }
    }
    ctx.restore();
  }

  // elevation markers (a circle with a pointer for each view it makes) and the project base and survey points
  function drawMarkers() {
    mgrips = [];
    const text = css("--text"), accent = css("--accent"), bg = css("--view-bg");
    const r = symR();
    const dir = (m, k) => { const l = m.slots[k].look; return { x: l[0], y: -l[1] }; };
    const drawOne = (m, selected, hovered, ghost) => {
      const c = ghost ? toScreen(ghost) : toScreen({ x: m.x, y: m.y });
      const hot = selected || hovered || ghost ? accent : null;
      ctx.save();
      const fam = m.family, denom = (data.view && data.view.scale) || 100, kk = denom * cam.s, pats = data.fill_patterns;
      // the body and a pointer for each view come from the marker's annotation family, at paper size
      if (fam) Ann.paths(ctx, fam.body, { x: c.x, y: c.y, k: kk, rot: m.rotation_deg }, hot, bg, { pats, widen: selected ? 1.6 : 1 });
      for (let k = 0; k < 4; k++) {
        const d = dir(m, k), n = { x: -d.y, y: d.x };
        if (m.slots[k].view_id !== null && m.slots[k].view_id !== undefined && fam) {
          Ann.paths(ctx, fam.pointer, { x: c.x, y: c.y, k: kk, rot: (Math.atan2(-d.y, d.x) * 180) / Math.PI }, hot, bg, { pats });
        }
        if (selected && !ghost) {
          // check boxes to make or drop the view of each pointer
          const bx = c.x + d.x * r * 3.1, by = c.y + d.y * r * 3.1, on = m.slots[k].view_id !== null && m.slots[k].view_id !== undefined;
          ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.fillStyle = bg;
          ctx.fillRect(bx - 5, by - 5, 10, 10); ctx.strokeRect(bx - 5, by - 5, 10, 10);
          if (on) { ctx.beginPath(); ctx.moveTo(bx - 3, by); ctx.lineTo(bx - 1, by + 3); ctx.lineTo(bx + 4, by - 3); ctx.stroke(); }
          mgrips.push({ id: m.id, slot: k, on, x: bx, y: by });
        }
      }
      ctx.restore();
    };
    for (const m of data.markers || []) drawOne(m, sel.has(m.id), selHover && selHover.includes(m.id));
    if (tool === "elevation" && hover) {
      const rot = markerRotAt(hover);
      const slots = cb.markerSlots();
      const probe = { id: 0, rotation_deg: rot, family: data.marker_family, slots: [0, 1, 2, 3].map((k) => { const a = (rot + [270, 90, 180, 0][k]) * Math.PI / 180; return { view_id: slots[k] ? 1 : null, look: [Math.cos(a), Math.sin(a)] }; }) };
      drawOne(probe, false, false, hover);
    }
    const sp = data.site_points;
    if (sp) {
      const cross = (c, col, lw) => { ctx.strokeStyle = col; ctx.lineWidth = lw; ctx.beginPath(); ctx.moveTo(c.x - r * 1.5, c.y); ctx.lineTo(c.x + r * 1.5, c.y); ctx.moveTo(c.x, c.y - r * 1.5); ctx.lineTo(c.x, c.y + r * 1.5); ctx.stroke(); };
      ctx.save();
      if (sp.base) {
        const base = toScreen({ x: sp.base.x, y: sp.base.y });
        const bsel = sel.has(sp.base.id), bcol = bsel || (selHover && selHover.includes(sp.base.id)) ? accent : text;
        // project base point: a circle with a cross, the cross turned by the angle to true north
        ctx.beginPath(); ctx.arc(base.x, base.y, r * 0.8, 0, Math.PI * 2); ctx.fillStyle = bg; ctx.fill(); ctx.strokeStyle = bcol; ctx.lineWidth = bsel ? 2 : 1.3; ctx.stroke();
        cross(base, bcol, bsel ? 2 : 1.3);
        ctx.beginPath(); ctx.moveTo(base.x, base.y); ctx.arc(base.x, base.y, r * 0.8, 0, -Math.PI / 2, true); ctx.closePath(); ctx.fillStyle = bcol; ctx.fill();
        if (bsel) label("Project Base Point", { x: base.x + r * 2.2, y: base.y - r }, accent);
      }
      if (sp.survey) {
        const sv = toScreen({ x: sp.survey.x, y: sp.survey.y });
        const ssel = sel.has(sp.survey.id), scol = ssel || (selHover && selHover.includes(sp.survey.id)) ? accent : text;
        // survey point: a triangle with a cross
        ctx.beginPath(); ctx.moveTo(sv.x, sv.y - r); ctx.lineTo(sv.x + r * 0.9, sv.y + r * 0.6); ctx.lineTo(sv.x - r * 0.9, sv.y + r * 0.6); ctx.closePath(); ctx.fillStyle = bg; ctx.fill(); ctx.strokeStyle = scol; ctx.lineWidth = ssel ? 2 : 1.3; ctx.stroke();
        cross(sv, scol, ssel ? 2 : 1.3);
        if (ssel) label("Survey Point", { x: sv.x + r * 2.2, y: sv.y - r }, accent);
      }
      ctx.restore();
    }
  }
  // the rotation a marker placed here would get: turned to face the model unless Space has set it
  function markerRotAt(p) {
    if (markerRot !== null) return markerRot;
    const pts = [...data.walls.flatMap((w) => w.corners), ...data.floors.flatMap((f) => f.points), ...(data.roofs || []).flatMap((r) => r.points)];
    if (!pts.length) return 0;
    const xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
    const cx = (Math.min(...xs) + Math.max(...xs)) / 2, cy = (Math.min(...ys) + Math.max(...ys)) / 2;
    const ang = (Math.atan2(cy - p.y, cx - p.x) * 180) / Math.PI;
    return (((Math.round((ang - 270) / 90) * 90) % 360) + 360) % 360;
  }

  function render() {
    resize();
    tdimUI = [];
    wgrips = [];
    const { w, h } = size();
    const text = css("--text"), accent = css("--accent"), line = css("--line");
    ctx.fillStyle = css("--view-bg");
    ctx.fillRect(0, 0, w, h);

    // work plane grid (Work Plane > Show). Off by default.
    const g = gridStep();
    const tl = toWorld(0, 0), br = toWorld(w, h);
    ctx.lineWidth = 1;
    if (showWorkGrid) for (let x = Math.floor(tl.x / g) * g; x <= br.x; x += g) {
      const major = Math.round(x / g) % 5 === 0;
      ctx.strokeStyle = major ? line : css("--panel-2");
      const sx = Math.round(toScreen({ x, y: 0 }).x) + 0.5;
      ctx.beginPath(); ctx.moveTo(sx, 0); ctx.lineTo(sx, h); ctx.stroke();
    }
    if (showWorkGrid) for (let y = Math.floor(br.y / g) * g; y <= tl.y; y += g) {
      const major = Math.round(y / g) % 5 === 0;
      ctx.strokeStyle = major ? line : css("--panel-2");
      const sy = Math.round(toScreen({ x: 0, y }).y) + 0.5;
      ctx.beginPath(); ctx.moveTo(0, sy); ctx.lineTo(w, sy); ctx.stroke();
    }
    // origin
    const o = toScreen({ x: 0, y: 0 });
    ctx.strokeStyle = accent; ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.moveTo(o.x - 8, o.y); ctx.lineTo(o.x + 8, o.y); ctx.moveTo(o.x, o.y - 8); ctx.lineTo(o.x, o.y + 8); ctx.stroke();

    // crop region: clip the model to it and optionally draw its edge
    const crop = data.crop || {};
    ctx.save();
    if (crop.enabled && crop.region) {
      const [c0, c1] = crop.region;
      const a = toScreen({ x: c0[0], y: c1[1] }), b = toScreen({ x: c1[0], y: c0[1] });
      ctx.beginPath(); ctx.rect(a.x, a.y, b.x - a.x, b.y - a.y); ctx.clip();
    }

    const bg = css("--view-bg");
    // line weights and pattern lengths are paper sizes: pixels = paper mm x scale x zoom
    const denom = (data.view && data.view.scale) || 100;
    const paperPx = denom * cam.s;
    const lw = (st) => (thin ? 1 : Math.max(1, st.line.width_mm * paperPx));
    const mix = (hex, amount) => { // halftone: blend towards the background
      const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
      const [a, b] = [rgb(hex), rgb(bg.startsWith("#") && bg.length === 7 ? bg : "#fbfbfc")];
      return "#" + a.map((v, i) => Math.round(v * (1 - amount) + b[i] * amount).toString(16).padStart(2, "0")).join("");
    };
    const colour = (st, hex) => (st.halftone ? mix(hex, 0.55) : hex);
    const alpha = (st) => 1 - (st.transparency || 0) / 100;
    const dash = (st) => {
      const a = (data.line_patterns || {})[st.line.pattern];
      return a && a.length ? a.map((v) => Math.max(v * paperPx, 0.8)) : [];
    };

    // fill polygons (arrays of [x, y] model points) with the style's pattern:
    // a solid colour, or hatch lines clipped to the shape and anchored to the
    // model origin so neighbouring shapes line up
    const fillShapes = (polys, st, skipBackground, evenodd, flat) => {
      const f = st.fill;
      if (!f.visible || !polys.length) return;
      const def = (data.fill_patterns || {})[f.pattern];
      const col = colour(st, f.color);
      ctx.save();
      ctx.globalAlpha = alpha(st);
      const addRing = (q) => {
        q.forEach((pt, i) => { const s2 = toScreen({ x: pt[0], y: pt[1] }); i ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
        ctx.closePath();
      };
      // rings of one region (outline and holes) fill even-odd; separate shapes fill one by one
      const solid = (c) => {
        ctx.fillStyle = c;
        if (evenodd) { ctx.beginPath(); for (const q of polys) addRing(q); ctx.fill("evenodd"); return; }
        for (const q of polys) { polyPath(q); ctx.fill(); }
      };
      if (f.background && !skipBackground) solid(colour(st, f.background));
      if (!def || def.solid) { solid(col); ctx.restore(); return; }
      if (flat) { ctx.restore(); return; } // hatch lines belong to cut material only
      ctx.beginPath();
      for (const q of polys) addRing(q);
      ctx.clip(evenodd ? "evenodd" : "nonzero");
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const q of polys) for (const [x, y] of q) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
      const unit = def.model ? 1 : denom; // model mm per pattern unit
      const corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
      ctx.strokeStyle = col; ctx.lineWidth = 1; ctx.lineCap = "butt";
      let budget = 4000;
      for (const L of def.lines) {
        const a = (L.angle * Math.PI) / 180, dx = Math.cos(a), dy = Math.sin(a), nx = -dy, ny = dx;
        const sp = L.spacing * unit;
        if (sp * cam.s < 2.5) { ctx.globalAlpha = alpha(st) * 0.35; solid(col); break; } // too dense to resolve
        const ox = L.origin[0] * unit, oy = L.origin[1] * unit;
        const v0 = ox * nx + oy * ny, u0 = ox * dx + oy * dy;
        const vs = corners.map(([x, y]) => x * nx + y * ny), us = corners.map(([x, y]) => x * dx + y * dy);
        const k0 = Math.ceil((Math.min(...vs) - v0) / sp), k1 = Math.floor((Math.max(...vs) - v0) / sp);
        const umin = Math.min(...us), umax = Math.max(...us);
        budget -= k1 - k0 + 1;
        if (budget < 0) break;
        const dashPx = (L.dashes || []).map((d) => Math.max(d * unit * cam.s, 0.6));
        const period = dashPx.reduce((m, d) => m + d, 0);
        ctx.setLineDash(dashPx.length ? dashPx : []);
        if (!dashPx.length) ctx.beginPath();
        for (let k = k0; k <= k1; k++) {
          const v = v0 + k * sp;
          const p0 = toScreen({ x: umin * dx + v * nx, y: umin * dy + v * ny }), p1 = toScreen({ x: umax * dx + v * nx, y: umax * dy + v * ny });
          if (dashPx.length) {
            ctx.beginPath();
            const into = (umin - (u0 + k * L.shift * unit)) * cam.s;
            ctx.lineDashOffset = ((into % period) + period) % period;
          }
          ctx.moveTo(p0.x, p0.y); ctx.lineTo(p1.x, p1.y);
          if (dashPx.length) ctx.stroke();
        }
        if (!dashPx.length) ctx.stroke();
      }
      ctx.restore();
    };
    const order = { underlay: 0, beyond: 1, projection: 2, cut: 3 };

    // floors (below walls)
    for (const f of [...data.floors].sort((a, b) => order[a.display] - order[b.display])) {
      if (f.id === Sketch.editingId) continue; // its sketch is drawn instead
      const st = f.style;
      fillShapes(f.rings, st, false, true);
      ctx.strokeStyle = colour(st, st.line.color); ctx.lineWidth = lw(st); ctx.setLineDash(dash(st));
      for (const r of f.rings) { polyPath(r); ctx.stroke(); }
      ctx.setLineDash([]);
    }

    // walls. Walls that share a look are drawn as one shape: stroke every outline
    // at double width, then fill over it. The fill hides the strokes where the
    // walls join and leaves only the outer half of the outline. Which walls share
    // a shape follows the view's Wall Join Display: all walls, or same type only.
    const sameType = data.view && data.view.wall_join_display === "clean_same_type";
    const groups = new Map();
    for (const wl of data.walls.filter((w) => !w.layered).sort((a, b) => order[a.display] - order[b.display])) {
      // a "don't clean" join keeps its seams, so that wall is drawn on its own
      const key = [wl.display, JSON.stringify(wl.style), sameType ? wl.type : "", wl.dont_clean ? wl.id : ""].join("|");
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(wl);
    }
    for (const group of groups.values()) {
      const st = group[0].style, line = colour(st, st.line.color), w1 = lw(st);
      ctx.lineJoin = "round";
      if (st.fill.visible && !dash(st).length) {
        ctx.strokeStyle = line; ctx.lineWidth = 2 * w1 + 1;
        for (const wl of group) for (const q of wl.pieces) { polyPath(q); ctx.stroke(); }
        const def = (data.fill_patterns || {})[st.fill.pattern];
        const hatched = def && !def.solid;
        // the base fill hides the strokes where walls join; hatching goes on top
        ctx.globalAlpha = hatched ? 1 : alpha(st);
        ctx.fillStyle = ctx.strokeStyle = hatched ? colour(st, st.fill.background || bg) : colour(st, st.fill.color); ctx.lineWidth = 1;
        for (const wl of group) for (const q of wl.pieces) { polyPath(q); ctx.fill(); ctx.stroke(); }
        ctx.globalAlpha = 1;
        if (hatched) fillShapes(group.flatMap((wl) => wl.pieces), st, true);
      } else {
        ctx.strokeStyle = line; ctx.lineWidth = w1; ctx.setLineDash(dash(st));
        for (const wl of group) for (const q of wl.pieces) { polyPath(q); ctx.stroke(); }
        ctx.setLineDash([]);
      }
    }
    // roofs: the outline and the hips, ridges and valleys, drawn over the walls they cover
    for (const r of [...(data.roofs || [])].sort((a, b) => order[a.display] - order[b.display])) {
      if (r.id === Sketch.editingId) continue; // its sketch is drawn instead
      const st = r.style;
      if (st.fill && st.fill.visible) fillShapes(r.rings, st, false, true);
      ctx.strokeStyle = colour(st, st.line.color); ctx.lineWidth = lw(st); ctx.setLineDash(dash(st));
      for (const q of r.rings) { polyPath(q); ctx.stroke(); }
      ctx.lineWidth = Math.max(1, lw(st) * 0.75);
      for (const [a, b] of r.ridges) { const s1 = toScreen({ x: a[0], y: a[1] }), s2 = toScreen({ x: b[0], y: b[1] }); ctx.beginPath(); ctx.moveTo(s1.x, s1.y); ctx.lineTo(s2.x, s2.y); ctx.stroke(); }
      ctx.setLineDash([]);
    }
    // walls drawn layer by layer: material fills, thin lines between materials, then the heavy outline
    for (const g of [...(data.wall_groups || [])].sort((a, b) => order[a.display] - order[b.display])) {
      for (const f of g.fills) {
        if (!f.style || !f.style.fill || !f.style.fill.visible) continue;
        fillShapes(f.shapes.flat(), { fill: f.style.fill, transparency: f.style.transparency, halftone: f.style.halftone, line: g.line }, false, true, g.display !== "cut");
      }
      ctx.lineJoin = "round";
      if (g.edge) {
        const est = { line: g.edge, halftone: g.halftone };
        ctx.strokeStyle = colour(est, est.line.color); ctx.lineWidth = lw(est); ctx.setLineDash(dash(est));
        for (const f of g.fills) for (const shape of f.shapes) for (const ring of shape) { polyPath(ring); ctx.stroke(); }
      }
      const ost = { line: g.line, halftone: g.halftone };
      ctx.strokeStyle = colour(ost, g.line.color); ctx.lineWidth = lw(ost); ctx.setLineDash(dash(ost));
      for (const shape of g.outline) for (const ring of shape) { polyPath(ring); ctx.stroke(); }
      ctx.setLineDash([]);
    }
    // end grips of the selected walls; ends of selected walls that meet share one grip, which moves them together
    const gripGroups = [];
    if (tool === "select" && sel.size <= 20) {
      for (const w of data.walls) {
        if (!sel.has(w.id) || w.pinned) continue;
        for (const end of [0, 1]) {
          const q = end ? w.end : w.start;
          let g = gripGroups.find((o) => Math.hypot(o.x - q[0], o.y - q[1]) <= 1);
          if (!g) { g = { x: q[0], y: q[1], members: [] }; gripGroups.push(g); }
          g.members.push({ id: w.id, end, wall: w });
        }
      }
    }
    for (const wl of data.walls) {
      if (!sel.has(wl.id)) continue;
      for (const q of wl.shape && wl.shape.length ? wl.shape : wl.pieces) {
        polyPath(q);
        ctx.fillStyle = "rgba(46,111,158,0.55)"; ctx.fill();
        ctx.strokeStyle = accent; ctx.lineWidth = 2.5; ctx.stroke();
      }
      // the ends of the location line, as drag grips
      if (tool === "select" && !wl.pinned && sel.size <= 20) {
        for (const end of [0, 1]) {
          const grp = gripGroups.find((o) => o.members.some((m) => m.id === wl.id && m.end === end));
          if (!grp || grp.members[0].id !== wl.id || grp.members[0].end !== end) continue; // the first of the walls at this point draws it
          const dragged = wgrip && wgrip.members && wgrip.members.some((m) => m.id === wl.id && m.end === end);
          const sp = toScreen(dragged ? wgrip.cur : { x: grp.x, y: grp.y });
          const shared = grp.members.length > 1;
          ctx.beginPath(); ctx.arc(sp.x, sp.y, shared ? 7 : 5.5, 0, Math.PI * 2);
          ctx.fillStyle = "#ffffff"; ctx.fill(); ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.stroke();
          if (shared) { ctx.beginPath(); ctx.arc(sp.x, sp.y, 3, 0, Math.PI * 2); ctx.fillStyle = accent; ctx.fill(); }
          wgrips.push({ id: wl.id, end, x: sp.x, y: sp.y, wall: wl, members: grp.members });
        }
        // a curved wall also drags from its middle (an arc's radius) or from the fit points of a spline
        (wl.grips || []).forEach((g, k) => {
          const q = wgrip && wgrip.pt && wgrip.id === wl.id && wgrip.k === k ? wgrip.cur : { x: g[0], y: g[1] };
          const sp = toScreen(q);
          ctx.beginPath(); ctx.rect(sp.x - 5, sp.y - 5, 10, 10);
          ctx.fillStyle = "#ffffff"; ctx.fill(); ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.stroke();
          wgrips.push({ id: wl.id, pt: true, k, x: sp.x, y: sp.y, wall: wl });
        });
      }
      // the curve as it would be with a point dragged
      if (wgrip && wgrip.pt && wgrip.id === wl.id && wgrip.poly) {
        polyPath(bandRing(wgrip.poly, wl.extents[0], wl.extents[1]));
        ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill(); ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.stroke();
      }
      // the wall as it would be with the dragged end moved
      const gm = wgrip && !wgrip.pt && wgrip.members && wgrip.members.find((m) => m.id === wl.id);
      if (gm && dist(gm.fixed, wgrip.cur) > 1) {
        const a = gm.fixed, b = wgrip.cur, L = dist(a, b), ux = -(b.y - a.y) / L, uy = (b.x - a.x) / L;
        // the wall's own left/right reach is measured looking from its start to its end
        const [l, r] = gm.end === 1 ? wl.extents : [wl.extents[1], wl.extents[0]];
        if (wl.curve) {
          // a curved wall keeps its shape: its outline turned and scaled with the chord
          const [o0, o1] = gm.end === 1 ? [wl.start, wl.end] : [wl.end, wl.start];
          const k = L / (Math.hypot(o1[0] - o0[0], o1[1] - o0[1]) || 1), rot = Math.atan2(b.y - a.y, b.x - a.x) - Math.atan2(o1[1] - o0[1], o1[0] - o0[0]), cs = Math.cos(rot), sn = Math.sin(rot);
          polyPath(wl.corners.map(([x, y]) => [a.x + k * ((x - o0[0]) * cs - (y - o0[1]) * sn), a.y + k * ((x - o0[0]) * sn + (y - o0[1]) * cs)]));
        } else polyPath([[a.x - ux * r, a.y - uy * r], [b.x - ux * r, b.y - uy * r], [b.x + ux * l, b.y + uy * l], [a.x + ux * l, a.y + uy * l]]);
        ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill(); ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.stroke();
        label(`${Math.round(L)}`, toScreen({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }), accent);
      }
    }
    // doors and windows: each part draws in its subcategory's style
    for (const ins of [...data.inserts].sort((a, b) => order[a.display] - order[b.display])) {
      const sub = (name) => (ins.sub_styles && ins.sub_styles[name]) || ins.style;
      const pen = (st) => { ctx.strokeStyle = colour(st, st.line.color); ctx.lineWidth = lw(st); ctx.setLineDash(dash(st)); ctx.lineJoin = "round"; };
      // a family with a stored body: the cut through its solids and its plan symbols, each in its subcategory's style
      for (const part of ins.parts || []) {
        const ps = sub(part.sub);
        pen(ps);
        if (part.masking) { polyPath(part.rings[0]); ctx.fillStyle = "#ffffff"; ctx.fill(); }
        if (part.filled) fillShapes(part.rings, ps, false, true, !part.cut);
        for (const ring of part.rings) {
          ctx.beginPath();
          ring.forEach((q, i) => { const s2 = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
          if (part.closed) ctx.closePath();
          ctx.stroke();
        }
      }
      let st = sub("Panel");
      pen(st);
      for (const q of ins.polys) { fillShapes([q], st); polyPath(q); ctx.stroke(); }
      st = sub("Frame/Mullion");
      pen(st);
      // the frame in section: its outline at Medium, the steel solid at Fine
      for (const q of ins.frame || []) {
        polyPath(q);
        if (ins.frame_solid) { ctx.fillStyle = ctx.strokeStyle; ctx.fill(); }
        ctx.stroke();
      }
      for (const [a, b] of ins.lines) {
        const sa = toScreen({ x: a[0], y: a[1] }), sb = toScreen({ x: b[0], y: b[1] });
        ctx.beginPath(); ctx.moveTo(sa.x, sa.y); ctx.lineTo(sb.x, sb.y); ctx.stroke();
      }
      st = sub("Glass");
      pen(st);
      for (const [a, b] of ins.glass || []) {
        const sa = toScreen({ x: a[0], y: a[1] }), sb = toScreen({ x: b[0], y: b[1] });
        ctx.beginPath(); ctx.moveTo(sa.x, sa.y); ctx.lineTo(sb.x, sb.y); ctx.stroke();
      }
      st = sub("Plan Swing");
      pen(st);
      for (const a of ins.arcs) {
        const c = toScreen({ x: a.c[0], y: a.c[1] });
        ctx.beginPath(); ctx.arc(c.x, c.y, a.r * cam.s, (-a.from * Math.PI) / 180, (-a.to * Math.PI) / 180, true); ctx.stroke();
      }
      ctx.setLineDash([]);
      if (sel.has(ins.id)) {
        polyPath(ins.footprint);
        ctx.fillStyle = "rgba(46,111,158,0.35)"; ctx.fill();
        ctx.strokeStyle = accent; ctx.lineWidth = 2.5; ctx.stroke();
        strokeParts(insertParts(ins));
      }
    }
    for (const f of data.floors) {
      if (!sel.has(f.id) || f.id === Sketch.editingId) continue;
      ctx.beginPath();
      for (const r of f.rings) {
        r.forEach((q, i) => { const s2 = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
        ctx.closePath();
      }
      ctx.fillStyle = "rgba(46,111,158,0.35)"; ctx.fill("evenodd");
      ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.stroke();
    }
    for (const rf of data.roofs || []) {
      if (!sel.has(rf.id) || rf.id === Sketch.editingId) continue;
      ctx.beginPath();
      for (const r of rf.rings) {
        r.forEach((q, i) => { const s2 = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s2.x, s2.y) : ctx.moveTo(s2.x, s2.y); });
        ctx.closePath();
      }
      ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill("evenodd");
      ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.stroke();
      ctx.lineWidth = 1.5;
      for (const [a, b] of rf.ridges) { const s1 = toScreen({ x: a[0], y: a[1] }), s2 = toScreen({ x: b[0], y: b[1] }); ctx.beginPath(); ctx.moveTo(s1.x, s1.y); ctx.lineTo(s2.x, s2.y); ctx.stroke(); }
    }
    drawGroups();
    const hov = new Set(tool === "select" && selHover ? selHover : []);
    Det.drawItems(sel, hov, accent, tool === "select");
    Det.drawNotes(sel, hov, accent, tool === "select");
    if (tool === "text") Det.preview(hover);
    ctx.restore();
    drawGrids();
    drawMarkers();
    if (tool === "grid") drawGridPreview();
    drawGridAlign();

    // crop region outline (not clipped)
    if (crop.enabled && crop.visible && crop.region) {
      const [c0, c1] = crop.region;
      const a = toScreen({ x: c0[0], y: c1[1] }), b = toScreen({ x: c1[0], y: c0[1] });
      ctx.strokeStyle = "#7a3fa0"; ctx.lineWidth = 1.2; ctx.setLineDash([8, 4]);
      ctx.strokeRect(a.x, a.y, b.x - a.x, b.y - a.y); ctx.setLineDash([]);
    }

    // Wall Joins tool: boxes on wall ends, the edited join in the accent colour
    if (tool === "joins") {
      const box = (e, col, w) => { const s = toScreen(e.p); ctx.strokeStyle = col; ctx.lineWidth = w; ctx.strokeRect(s.x - 9, s.y - 9, 18, 18); };
      const isSel = (e) => joinSel.some((j) => j.id === e.id && j.end === e.end);
      // a grey square at every wall end, so every join (corner, T and free end) can be picked
      for (const w of data.walls) {
        for (const end of [0, 1]) {
          const q = end === 0 ? w.start : w.end;
          if (!isSel({ id: w.id, end })) box({ p: { x: q[0], y: q[1] } }, "rgba(120,128,138,0.8)", 1);
        }
      }
      if (joinHover && !isSel(joinHover)) box(joinHover, "#d4572f", 1.5);
      for (const j of joinSel) box(j, accent, 2.5);
    }

    // Select tool: pre-highlight what a click would pick (Tab steps through the choices)
    if (tool === "select" && selHover && !band) {
      const ids = new Set(selHover);
      ctx.strokeStyle = "#d4572f"; ctx.lineWidth = 2;
      for (const e of elements()) {
        if (!ids.has(e.id) || sel.has(e.id)) continue;
        const rings = e.kind === "wall" ? (data.walls.find((w) => w.id === e.id).pieces || [e.ring]) : [e.ring];
        for (const q of rings) { polyPath(q); ctx.stroke(); }
        if (e.parts) strokeParts(e.parts);
      }
    }
    // pushpin on pinned elements that are selected or under the cursor
    for (const e of elements()) {
      if (!e.pinned || !(sel.has(e.id) || (selHover && selHover.includes(e.id)))) continue;
      const cx = e.ring.reduce((a, q) => a + q[0], 0) / e.ring.length, cy = e.ring.reduce((a, q) => a + q[1], 0) / e.ring.length;
      const c = toScreen({ x: cx, y: cy });
      ctx.fillStyle = "#c0392b"; ctx.strokeStyle = "#fff"; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.arc(c.x, c.y - 5, 5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      ctx.strokeStyle = "#c0392b"; ctx.lineWidth = 2;
      ctx.beginPath(); ctx.moveTo(c.x, c.y); ctx.lineTo(c.x, c.y + 6); ctx.stroke();
    }
    drawEdit();
    if (Sketch.active) Sketch.draw(ctx);
    // rubber band: solid for window (left to right), dashed for crossing (right to left)
    if (band) {
      const x = Math.min(band.x0, band.x1), y = Math.min(band.y0, band.y1), bw = Math.abs(band.x1 - band.x0), bh = Math.abs(band.y1 - band.y0);
      const win = band.x1 >= band.x0;
      ctx.fillStyle = win ? "rgba(46,111,158,0.12)" : "rgba(60,150,90,0.12)";
      ctx.fillRect(x, y, bw, bh);
      ctx.strokeStyle = win ? accent : "#3c965a"; ctx.lineWidth = 1.2;
      ctx.setLineDash(win ? [] : [6, 4]);
      ctx.strokeRect(x + 0.5, y + 0.5, bw, bh);
      ctx.setLineDash([]);
    }

    // in-progress drawing
    if (draw && draw.pts.length && hover) {
      ctx.strokeStyle = accent; ctx.fillStyle = accent; ctx.lineWidth = 1.5;
      const last = draw.pts[draw.pts.length - 1];
      if (tool === "wall" && SHAPE_TWO_CLICK.has(wallShape)) {
        const segs = shapeSegments(draw.pts[0], hover, wallShape, wallSides);
        if (segs) {
          const [l, r] = [extents.left, extents.right];
          for (const [a0, b0] of segs) {
            const [a, b] = [{ x: a0[0], y: a0[1] }, { x: b0[0], y: b0[1] }], L = dist(a, b);
            if (L < 1) continue;
            const ux = -(b.y - a.y) / L, uy = (b.x - a.x) / L;
            polyPath([[a.x - ux * r, a.y - uy * r], [b.x - ux * r, b.y - uy * r], [b.x + ux * l, b.y + uy * l], [a.x + ux * l, a.y + uy * l]]);
            ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill(); ctx.stroke();
          }
          const c = wallShape === "rectangle" ? { x: (draw.pts[0].x + hover.x) / 2, y: (draw.pts[0].y + hover.y) / 2 } : draw.pts[0];
          label(wallShape === "rectangle" ? `${Math.round(Math.abs(hover.x - draw.pts[0].x))} x ${Math.round(Math.abs(hover.y - draw.pts[0].y))}` : `R ${Math.round(dist(draw.pts[0], hover))}`, toScreen(c), accent);
        }
      } else if (tool === "wall" && SHAPE_CURVE[wallShape]) {
        // the picks so far, then the wall the model makes from them
        ctx.save(); ctx.setLineDash([4, 4]); ctx.lineWidth = 1;
        ctx.beginPath();
        [...draw.pts, hover].forEach((q, i) => { const sp = toScreen(q); i ? ctx.lineTo(sp.x, sp.y) : ctx.moveTo(sp.x, sp.y); });
        ctx.stroke(); ctx.restore();
        for (const c of draw.preview || []) {
          polyPath(bandRing(c.poly, extents.left, extents.right));
          ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill(); ctx.stroke();
        }
        if (wallShape === "circle") label(`R ${Math.round(dist(draw.pts[0], hover))}`, toScreen(draw.pts[0]), accent);
      } else if (tool === "wall") {
        const a = last, L = dist(a, hover), tl = parseFloat(draw.typed);
        const b = !isNaN(tl) && L > 1e-6 ? { x: a.x + ((hover.x - a.x) / L) * tl, y: a.y + ((hover.y - a.y) / L) * tl } : hover;
        if (L > 1) {
          const ux = -(b.y - a.y) / L, uy = (b.x - a.x) / L; // left normal
          const [l, r] = [extents.left, extents.right];
          polyPath([[a.x - ux * r, a.y - uy * r], [b.x - ux * r, b.y - uy * r], [b.x + ux * l, b.y + uy * l], [a.x + ux * l, a.y + uy * l]]);
          ctx.fillStyle = "rgba(46,111,158,0.25)"; ctx.fill(); ctx.stroke();
          label(draw.typed ? `${draw.typed}|` : `${Math.round(dist(a, b))}`, toScreen({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }), accent);
          let deg = (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI; if (deg < 0) deg += 360;
          const sa = toScreen(a);
          label(`${deg.toFixed(1)}\u00b0`, { x: sa.x + 34, y: sa.y - 16 }, accent);
        }
      }
    }

    // Place Model Group: an instance of the type drawn here, unmirrored, follows the cursor by its origin
    if (tool === "groupplace" && groupPlace && hover) {
      const g = modelGroups.find((x) => x.type_id === groupPlace.typeId && !x.mirrored && x.members.some((m) => elements().some((e) => e.id === m)));
      if (g) {
        const a = (-g.angle_deg * Math.PI) / 180, c = Math.cos(a), sn = Math.sin(a);
        ghost((q) => ({ x: hover.x + (q.x - g.origin.x) * c - (q.y - g.origin.y) * sn, y: hover.y + (q.x - g.origin.x) * sn + (q.y - g.origin.y) * c }), new Set([g.id]));
      }
    }
    // door / window placement preview
    if (tool === "component" && comp && hover) {
      // the component's plan follows the cursor, turned by Space
      const a = (comp.rot * Math.PI) / 180, c = Math.cos(a), sn = Math.sin(a);
      const T = (q) => toScreen({ x: hover.x + q[0] * c - q[1] * sn, y: hover.y + q[0] * sn + q[1] * c });
      ctx.save();
      ctx.strokeStyle = accent; ctx.lineWidth = 1.2; ctx.fillStyle = "rgba(46,111,158,0.16)";
      for (const pt of comp.parts || []) for (const ring of pt.rings) {
        ctx.beginPath();
        ring.forEach((q, i) => { const v = T(q); i ? ctx.lineTo(v.x, v.y) : ctx.moveTo(v.x, v.y); });
        if (pt.closed) { ctx.closePath(); if (pt.filled) ctx.fill(); }
        ctx.stroke();
      }
      if (comp.footprint && comp.footprint.length) {
        ctx.setLineDash([5, 4]); ctx.beginPath();
        comp.footprint.forEach((q, i) => { const v = T(q); i ? ctx.lineTo(v.x, v.y) : ctx.moveTo(v.x, v.y); });
        ctx.closePath(); ctx.stroke(); ctx.setLineDash([]);
      }
      ctx.restore();
    }
    if (hostedTool(tool) && placeHover) {
      polyPath(placeHover.quad);
      ctx.fillStyle = "rgba(46,111,158,0.30)"; ctx.fill();
      ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.stroke();
      label(`${Math.round(placeHover.u)}`, toScreen(placeHover.p), accent);
    }

    // permanent dimensions, and the tool that places them
    Dims.draw(data.dimensions, sel, selHover);
    if (tool === "dimension") Dims.drawTool();
    Spots.draw(data.spots, sel, selHover);
    if (tool === "spot") Spots.drawTool();
    Sections.draw(data.sections, [...sel], selHover);
    if (tool === "section" && draw && hover) Sections.drawPreview(draw.pts[0], hover);

    // temporary dimensions: of the selected element, and of the door or window being placed
    if (tool === "select" && tdims.length && !band && !dragging) drawTempDims(tdims, true);
    if (hostedTool(tool) && placeHover && placeDims.length) drawTempDims(placeDims, false);

    // snap marker: the glyph for the kind of snap, with its name beside it
    if (hover && snapGlyphWanted()) {
      const s = toScreen(hover);
      if (hover.label) {
        Snap.drawGuides(ctx, hover, toScreen);
        Snap.glyph(ctx, hover.kind, s.x, s.y);
        ctx.font = "11px system-ui, sans-serif";
        const text = hover.cands && hover.cands.length > 1 ? `${hover.label} (Tab for more)` : hover.label;
        const tw = ctx.measureText(text).width;
        ctx.fillStyle = "#fffbe6"; ctx.strokeStyle = "#999"; ctx.lineWidth = 1;
        ctx.fillRect(s.x + 10, s.y + 8, tw + 8, 16); ctx.strokeRect(s.x + 10.5, s.y + 8.5, tw + 8, 16);
        ctx.fillStyle = "#222"; ctx.fillText(text, s.x + 14, s.y + 20);
      } else {
        ctx.strokeStyle = accent; ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.moveTo(s.x - 6, s.y); ctx.lineTo(s.x + 6, s.y); ctx.moveTo(s.x, s.y - 6); ctx.lineTo(s.x, s.y + 6); ctx.stroke();
      }
    }
  }

  // ---- tools ----------------------------------------------------------------
  function setTool(t) {
    if (t !== "dimension") Dims.stop();
    if (t !== "spot") Spots.stop();
    if (t !== "secsplit") Sections.stopSplit();
    if (t !== "text") Det.clearPicks();
    if (t !== "noteleader") Det.stopLeader();
    tool = t;
    draw = null;
    markerRot = null; gridHover = null; ggrip = null;
    ed = isEditTool(t) ? newEdit(t) : null;
    edHover = null;
    placeHover = null;
    if (t !== "component") comp = null;
    placeDims = []; placeKey = ""; closeTdimInput();
    joinHover = null;
    if (t !== "joins") joinSel = [];
    canvas.style.cursor = t === "select" ? "default" : "crosshair";
    cb.tool(t);
    cb.prompt(promptText());
    render();
  }

  function promptText() {
    if (tool === "sketch") return Sketch.promptText();
    if (tool === "dimension") return Dims.prompt();
    if (tool === "spot") return Spots.prompt();
    if (tool === "text" || tool === "noteleader") return Det.promptText(tool);
    if (tool === "wall" && SHAPE_TWO_CLICK.has(wallShape)) {
      const what = { rectangle: "rectangle", polygon_in: "inscribed polygon", polygon_out: "circumscribed polygon" }[wallShape];
      if (!draw) return `Click the ${wallShape === "rectangle" ? "first corner" : "centre"} of the ${what}.`;
      return wallShape === "rectangle" ? "Click the opposite corner. Esc cancels." : `Click ${wallShape === "polygon_in" ? "a corner" : "the middle of a side"}. Esc cancels.`;
    }
    if (tool === "wall" && SHAPE_CURVE[wallShape]) {
      const ps = SHAPE_CURVE[wallShape].prompts, k = draw ? draw.pts.length : 0;
      return (ps[Math.min(k, ps.length - 1)] || "") + " Esc cancels.";
    }
    if (tool === "grid") {
      if (gridShape === "pick") return gridOffset > 0 ? "Click a wall or grid line: the new grid goes to the side of it you click, set back by the offset. Esc to stop." : "Click a wall or grid line to put a grid on it. Set an offset in the options bar to place it beside the line. Esc to stop.";
      if (gridShape === "multi") return draw ? "Click the next point. Enter or double-click finishes the grid. Esc cancels." : "Click the first point of the multi-segment grid.";
      if (GRID_ARC.has(gridShape)) { const ps = SHAPE_CURVE[gridShape].prompts, k = draw ? draw.pts.length : 0; return (ps[Math.min(k, ps.length - 1)] || "") + " Esc cancels."; }
      return draw ? "Click the end of the grid. Type a length and press Enter. Shift keeps to 45 degrees. Esc cancels." : "Click the start of the grid.";
    }
    if (tool === "secsplit") return Sections.splitPrompt();
    if (tool === "section") return draw ? "Click the end of the section line. Shift keeps to 45 degrees. The view looks to the left of the direction you draw; flip it after. Esc cancels." : "Click the start of the section line.";
    if (tool === "elevation") return "Click to place the elevation marker. Space turns it 90 degrees. Tick the pointers you want views for in the options bar. Esc to stop.";
    if (tool === "wall") return draw ? "Click to place the next wall point. Esc finishes the chain. Hold Shift for 45 degree angles." : "Click to start a wall. Esc or right-click to stop.";
    if (ed) return editPrompt();
    if (tool === "joins") return "Click a wall end to edit its join. Use Next / Previous to step through the configurations. Esc to stop.";
    if (hostedTool(tool)) return `Click on a wall to place the ${place.label}. ${tool === "hosted" ? "It faces the side of the wall you click." : "The swing follows the side of the wall you click."} Esc to stop.`;
    if (tool === "groupplace") return groupPlace ? `Click to place ${groupPlace.name} by its origin. Esc to stop.` : "Pick a group to place.";
    if (tool === "component") return comp ? "Click to place the component. Space turns it 90 degrees. Esc to stop." : "Pick a component in the type selector.";
    return "Click to select. Shift adds to the selection. Delete removes. Wall: WA, Door: DR, Window: WN";
  }

  function editPrompt() {
    const st = ed.stage;
    if (st === "pick") return "Select the elements to modify, then press Enter. Esc cancels.";
    const T = tool;
    if (T === "move" || T === "copy") return st === "a" ? `Click the start point to ${T}. Hold Shift (or turn on Constrain) to keep to one direction.` : "Click the end point, or type a distance and press Enter.";
    if (T === "rotate") return eo.centrePlace ? "Click to place the centre of rotation." : st === "a" ? "Click to set the start ray, or type an angle and press Enter." : "Click to set the end ray, or type an angle and press Enter. Shift snaps to 15 degrees.";
    if (T === "array" && eo.arrayKind === "radial") return eo.centrePlace ? "Click to place the centre of the array." : st === "a" ? "Click to set the start ray (drag the centre to move it), or type an angle and press Enter." : `Click to set the ${eo.arrayMoveTo === "last" ? "last member's" : "second member's"} angle, or type it and press Enter. Shift snaps to 15 degrees.`;
    if (T === "array") return st === "a" ? "Click the start point of the array. Hold Shift (or turn on Constrain) to keep to one direction." : `Click where the ${eo.arrayMoveTo === "last" ? "last member goes (the rest spaced evenly between)" : "second member goes (the spacing)"}, or type a distance and press Enter.`;
    if (T === "mirror") return eo.mirrorMode === "pick" ? "Pick the line to mirror about (a wall face, centreline or edge). Tab cycles lines." : st === "a" ? "Click the first point of the mirror axis." : "Click the second point of the mirror axis.";
    if (T === "offset") return st === "a" ? (eo.offsetMode === "numerical" ? "Move the cursor to the side to offset to, then click the wall. Tab picks the whole chain. Ctrl copies." : "Click the wall to offset. Tab picks the whole chain.") : "Move to set the distance and click, or type it and press Enter.";
    if (T === "trim") return eo.trimMode === "corner" ? (st === "a" ? "Click the first wall on the part to keep." : "Click the second wall on the part to keep.") : st === "a" ? "Click the wall to trim or extend to." : "Click the wall to trim or extend, on the part to keep.";
    if (T === "align") return st === "a" ? "Click the line to align to. Tab cycles overlapping lines." : "Click the line of the element to move. Ctrl keeps the same reference.";
    return "";
  }

  // the wall under the cursor and where a door or window of the current width would sit
  function placementAt(sx, sy) {
    const p = toWorld(sx, sy);
    let best = null;
    for (const w of data.walls) {
      if (w.display === "underlay") continue;
      if (!inPoly(p, w.corners) && polyEdgeDist(p, w.corners) > 14 / cam.s) continue; // a little slack around the wall
      const [ax, ay, bx, by] = [w.start[0], w.start[1], w.end[0], w.end[1]];
      const len = Math.hypot(bx - ax, by - ay);
      if (len < place.width) continue;
      const ux = (bx - ax) / len, uy = (by - ay) / len;
      // distance along the wall steps by the snap increment (never finer than a whole mm), as drawing does
      const step = Snap.lengthStep(cam.s);
      const raw = Math.round(((p.x - ax) * ux + (p.y - ay) * uy) / step) * step;
      const along = Math.min(Math.floor(len - place.width / 2), Math.max(Math.ceil(place.width / 2), raw));
      // the opening across the wall's body, square to the wall (the drawn outline is mitred at joins, so slicing it slants the ends)
      const [el, er] = w.extents || [w.thickness / 2, w.thickness / 2];
      const at = (u, side) => [ax + ux * u - uy * side, ay + uy * u + ux * side];
      const quad = [at(along - place.width / 2, -er), at(along + place.width / 2, -er), at(along + place.width / 2, el), at(along - place.width / 2, el)];
      best = { wall: w.id, u: along, p: { x: ax + ux * along, y: ay + uy * along }, quad };
      break;
    }
    return best;
  }

  // The snap glyph shows only for tools that place a point; picking a line, wall or element needs none
  function snapGlyphWanted() {
    // a dragged grip or element snaps, whatever the tool
    if ((wgrip && wgrip.moved) || (ggrip && ggrip.moved) || dragging || Det.dragging) return true;
    if (tool === "sketch") return Sketch.wantsSnap();
    if (tool === "dimension" || tool === "spot") return false;
    if (["select", "door", "window", "hosted", "joins", "elevation"].includes(tool)) return false;
    if (tool === "grid" && gridShape === "pick") return false;
    if (ed) {
      if (tool === "align" || tool === "trim" || ed.stage === "pick") return false;
      if (tool === "offset" && eo.offsetMode === "numerical") return false;
      if (tool === "mirror" && eo.mirrorMode === "pick" && ed.stage === "a") return false;
    }
    return true;
  }

  function endAt(sx, sy) {
    const r = 14 / cam.s;
    const m = toWorld(sx, sy);
    let best = null;
    for (const w of data.walls) {
      for (const end of [0, 1]) {
        const q = end === 0 ? w.start : w.end;
        const d = Math.hypot(q[0] - m.x, q[1] - m.y);
        if (d <= r && (!best || d < best.d)) best = { id: w.id, end, p: { x: q[0], y: q[1] }, d };
      }
    }
    return best;
  }

  // ---- modify tools: Move, Copy, Rotate, Mirror, Offset, Trim/Extend, Align -------------
  // The model does the work (one command, one undo step). This part gathers the picks, shows a
  // preview and sends the command through cb.edit, cb.offset, cb.trim and cb.align.
  const EDIT_TOOLS = ["move", "copy", "rotate", "mirror", "offset", "trim", "align", "array"];
  const NEEDS_SELECTION = new Set(["move", "copy", "rotate", "mirror", "array"]);
  let eo = { constrain: false, disjoin: false, multiple: false, copy: false, mirrorMode: "pick", mirrorCopy: true, offsetMode: "numerical", offsetDist: 300, offsetCopy: false, trimMode: "corner", prefer: "all", centrePlace: false, angle: "",
    arrayKind: "linear", arrayCount: 2, arrayMoveTo: "second", arrayConstrain: false };
  let cdrag = false; // the centre of rotation (Rotate, a radial Array) being dragged
  const radialTool = () => tool === "rotate" || (tool === "array" && eo.arrayKind === "radial");
  let ed = null; // { stage, p0, centre, typed, ... }
  let edHover = null; // what the cursor would pick: { kind:"edge", a, b } or { kind:"wall", id, side }
  const isEditTool = (t = tool) => EDIT_TOOLS.includes(t);

  function newEdit(t) {
    return { stage: NEEDS_SELECTION.has(t) && !sel.size ? "pick" : "a", typed: "", chain: false };
  }

  function selBounds() {
    // walls count by their location line, so the default centre of rotation is not pulled by mitred corners
    const all = expandIds(sel);
    const pts = elements().filter((e) => all.has(e.id)).flatMap((e) => { const w = e.kind === "wall" ? data.walls.find((x) => x.id === e.id) : null; return w ? [w.start, w.end] : e.ring; });
    if (!pts.length) return null;
    const xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
    return { x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys) };
  }
  function centreOfRotation() {
    if (ed && ed.centre) return ed.centre;
    const b = selBounds();
    return b ? { x: (b.x0 + b.x1) / 2, y: (b.y0 + b.y1) / 2 } : { x: 0, y: 0 };
  }

  // straight edges anyone can pick: every wall face, layer boundary, core face and centreline, floor and opening edges
  function edges(filter) {
    const out = [];
    const wantFace = (k) => !filter || filter === "all" || (filter === "face" && k === "face") || (filter === "centre" && k === "centre") || (filter === "core" && k === "core");
    for (const w of data.walls) {
      if (w.display === "underlay" && !selOpts.underlay) continue;
      const dx = w.end[0] - w.start[0], dy = w.end[1] - w.start[1], len = Math.hypot(dx, dy) || 1;
      const nx = -dy / len, ny = dx / len;
      for (const f of w.faces || [{ off: 0, kind: "centre" }]) {
        if (!wantFace(f.kind)) continue;
        out.push({ kind: "wall", owner: w.id, face: f.kind, a: [w.start[0] + nx * f.off, w.start[1] + ny * f.off], b: [w.end[0] + nx * f.off, w.end[1] + ny * f.off] });
      }
    }
    if (!filter || filter === "all") {
      for (const f of data.floors) for (let i = 0; i < f.points.length; i++) out.push({ kind: "floor", owner: f.id, a: f.points[i], b: f.points[(i + 1) % f.points.length] });
      for (const rf of data.roofs || []) {
        if (rf.id === Sketch.editingId) continue;
        for (const ring of rf.rings) for (let i = 0; i < ring.length; i++) out.push({ kind: "roof", owner: rf.id, a: ring[i], b: ring[(i + 1) % ring.length] });
        for (const [a, b] of rf.ridges) out.push({ kind: "roof", owner: rf.id, a, b });
      }
      for (const ins of data.inserts) for (let i = 0; i < ins.footprint.length; i++) out.push({ kind: "insert", owner: ins.id, a: ins.footprint[i], b: ins.footprint[(i + 1) % ins.footprint.length] });
    }
    return out;
  }
  function edgeCands(sx, sy, filter) {
    const p = toWorld(sx, sy), tol = 8 / cam.s;
    const found = [];
    for (const e of edges(filter)) { const d = segDist(p, e.a, e.b); if (d <= tol) found.push({ e, d }); }
    found.sort((x, y) => x.d - y.d);
    return found.map((f) => f.e);
  }
  function wallAt(sx, sy) {
    const p = toWorld(sx, sy), tol = 4 / cam.s;
    let best = null, bd = Infinity;
    for (const w of data.walls) {
      if (w.display === "underlay") continue;
      const d = inPoly(p, w.corners) ? 0 : polyEdgeDist(p, w.corners);
      if (d <= tol && d < bd) { bd = d; best = w; }
    }
    return best;
  }
  const wallDir = (w) => { const dx = w.end[0] - w.start[0], dy = w.end[1] - w.start[1], l = Math.hypot(dx, dy) || 1; return { x: dx / l, y: dy / l }; };
  // signed distance of a point from a wall's line, positive on the left of its drawing direction
  function sideOf(w, p) { const d = wallDir(w); return (p.x - w.start[0]) * -d.y + (p.y - w.start[1]) * d.x; }

  // the translation a Move or Copy would make for a cursor position
  function moveDelta(p) {
    let v = { x: p.x - ed.p0.x, y: p.y - ed.p0.y };
    if ((tool === "array" ? eo.arrayConstrain : eo.constrain) || shift) {
      const only = [...sel];
      const w = only.length === 1 ? data.walls.find((x) => x.id === only[0]) : null;
      const ax = w ? wallDir(w) : { x: 1, y: 0 }, ay = { x: -ax.y, y: ax.x };
      const a = v.x * ax.x + v.y * ax.y, b = v.x * ay.x + v.y * ay.y;
      v = Math.abs(a) >= Math.abs(b) ? { x: ax.x * a, y: ax.y * a } : { x: ay.x * b, y: ay.y * b };
    }
    const typed = parseFloat(ed.typed);
    if (!isNaN(typed)) { const l = Math.hypot(v.x, v.y); v = l > 1e-9 ? { x: (v.x / l) * typed, y: (v.y / l) * typed } : { x: typed, y: 0 }; }
    return v;
  }
  function rotateAngle(p) {
    const c = centreOfRotation();
    const typed = parseFloat(ed.typed);
    if (!isNaN(typed)) return typed;
    let a = (Math.atan2(p.y - c.y, p.x - c.x) - Math.atan2(ed.p0.y - c.y, ed.p0.x - c.x)) * 180 / Math.PI;
    a = ((a + 540) % 360) - 180;
    return shift ? Math.round(a / 15) * 15 : Math.round(a * 100) / 100;
  }
  // The array a cursor position would make: the step from one member to the next, as a point mapping per member.
  function arrayStep(p) {
    const n = Math.max(2, eo.arrayCount | 0), k = eo.arrayMoveTo === "last" ? 1 / (n - 1) : 1;
    if (eo.arrayKind === "radial") {
      const full = eo.arrayMoveTo === "last" && Math.abs(Math.abs(rotateAngle(p)) - 360) < 1e-6;
      const a = rotateAngle(p), step = full ? a / n : a * k, c = centreOfRotation();
      return { n, angle: a, map: (i) => { const r = (step * i * Math.PI) / 180, cs = Math.cos(r), sn = Math.sin(r); return (q) => ({ x: c.x + (q.x - c.x) * cs - (q.y - c.y) * sn, y: c.y + (q.x - c.x) * sn + (q.y - c.y) * cs }); } };
    }
    const v = moveDelta(p);
    return { n, v, map: (i) => (q) => ({ x: q.x + v.x * k * i, y: q.y + v.y * k * i }) };
  }
  function sendArray(p) {
    const ids = [...sel], s = arrayStep(p), r = (x) => Math.round(x * 1000) / 1000;
    if (eo.arrayKind === "radial") {
      const c = centreOfRotation();
      cb.array({ kind: "radial", ids, cx: r(c.x), cy: r(c.y), angle_deg: Math.round(s.angle * 100) / 100, count: s.n, move_to: eo.arrayMoveTo });
    } else cb.array({ kind: "linear", ids, dx: r(s.v.x), dy: r(s.v.y), count: s.n, move_to: eo.arrayMoveTo });
  }

  function offsetTarget() { return ed.target ? data.walls.find((w) => w.id === ed.target) : null; }

  function finishEdit(keepTool) {
    selHover = null; edHover = null;
    if (keepTool) { ed = newEdit(tool); } else { setTool("select"); return; }
    cb.prompt(promptText());
    render();
  }

  function editClick(sx, sy, ev) {
    if (!ed) return false;
    if (ed.stage === "pick") { selectClick(sx, sy, ev); return true; }
    const p = snap(sx, sy);
    Snap.clearOverride();
    snapCycle = 0;
    const ids = [...sel];
    if (tool === "move" || tool === "copy") {
      if (ed.stage === "a") { ed.p0 = p; ed.stage = "b"; }
      else {
        const d = moveDelta(p);
        cb.edit({ kind: "move", ids, dx: Math.round(d.x * 1000) / 1000, dy: Math.round(d.y * 1000) / 1000, copy: tool === "copy" || !!ev.ctrlKey, disjoin: tool === "move" && eo.disjoin });
        if (tool === "copy" && eo.multiple) { ed.typed = ""; } else finishEdit(false);
      }
    } else if (tool === "rotate") {
      if (eo.centrePlace) { ed.centre = p; eo.centrePlace = false; cb.options(); }
      else if (ed.stage === "a") { ed.p0 = p; ed.stage = "b"; }
      else {
        const c = centreOfRotation();
        cb.edit({ kind: "rotate", ids, cx: c.x, cy: c.y, angle_deg: rotateAngle(p), copy: eo.copy, disjoin: eo.disjoin });
        finishEdit(false);
      }
    } else if (tool === "array") {
      if (eo.arrayKind === "radial" && eo.centrePlace) { ed.centre = p; eo.centrePlace = false; cb.options(); }
      else if (ed.stage === "a") { ed.p0 = p; ed.stage = "b"; }
      else {
        if (eo.arrayKind === "linear" && dist(ed.p0, p) < 1 && isNaN(parseFloat(ed.typed))) return true;
        sendArray(p);
        finishEdit(false);
      }
    } else if (tool === "mirror") {
      if (eo.mirrorMode === "pick") {
        const e = edgeCands(sx, sy, "all")[tabIdx % Math.max(1, edgeCands(sx, sy, "all").length)];
        if (!e) { cb.prompt("Pick a line to mirror about: a wall face, centreline or edge."); return true; }
        cb.edit({ kind: "mirror", ids, a: e.a, b: e.b, copy: eo.mirrorCopy });
        finishEdit(false);
      } else if (ed.stage === "a") { ed.p0 = p; ed.stage = "b"; }
      else {
        if (dist(ed.p0, p) < 1) return true;
        cb.edit({ kind: "mirror", ids, a: [ed.p0.x, ed.p0.y], b: [p.x, p.y], copy: eo.mirrorCopy });
        finishEdit(false);
      }
    } else if (tool === "offset") {
      if (ed.stage === "a") {
        const w = wallAt(sx, sy);
        if (!w) { cb.prompt("Pick a wall to offset."); return true; }
        const pt = toWorld(sx, sy);
        const chain = ed.chain ? wallChain(w.id) : [w.id];
        const side = sideOf(w, pt) >= 0 ? 1 : -1;
        if (eo.offsetMode === "numerical") {
          if (!(eo.offsetDist > 0)) { cb.prompt("Type an offset distance in the options bar."); return true; }
          cb.offset({ ids: [w.id, ...chain.filter((i) => i !== w.id)], dist: side * eo.offsetDist, copy: eo.offsetCopy || !!ev.ctrlKey });
          ed.target = null;
        } else { ed.target = w.id; ed.p0 = pt; ed.stage = "b"; ed.chainIds = chain; }
      } else {
        const w = offsetTarget();
        if (!w) { ed.stage = "a"; return true; }
        const typed = parseFloat(ed.typed);
        const dd = !isNaN(typed) ? Math.abs(typed) * (sideOf(w, p) >= 0 ? 1 : -1) : sideOf(w, p);
        if (Math.abs(dd) < 1) return true;
        cb.offset({ ids: [w.id, ...ed.chainIds.filter((i) => i !== w.id)], dist: dd, copy: eo.offsetCopy || !!ev.ctrlKey });
        ed.stage = "a"; ed.target = null; ed.typed = "";
      }
    } else if (tool === "trim") {
      const w = wallAt(sx, sy), pt = toWorld(sx, sy);
      if (!w) { cb.prompt("Pick a wall."); return true; }
      if (eo.trimMode === "corner") {
        if (ed.stage === "a") { ed.first = { id: w.id, pick: [pt.x, pt.y] }; ed.stage = "b"; }
        else { cb.trim({ mode: "corner", a: ed.first.id, pick_a: ed.first.pick, b: w.id, pick_b: [pt.x, pt.y] }); ed.stage = "a"; ed.first = null; }
      } else if (ed.stage === "a") { ed.boundary = w.id; ed.stage = "b"; }
      else { cb.trim({ mode: "single", wall: w.id, pick: [pt.x, pt.y], boundary: ed.boundary }); if (eo.trimMode === "single") { ed.stage = "a"; ed.boundary = null; } }
    } else if (tool === "align") {
      const c = edgeCands(sx, sy, eo.prefer)[tabIdx % Math.max(1, edgeCands(sx, sy, eo.prefer).length)];
      if (!c) { cb.prompt(ed.stage === "a" ? "Pick the line to align to." : "Pick the line of the element to move."); return true; }
      if (ed.stage === "a") { ed.ref = c; ed.stage = "b"; }
      else {
        cb.align({ ids: [c.owner], reference: [ed.ref.a, ed.ref.b], target: [c.a, c.b] });
        if (eo.multiple || ev.ctrlKey) { /* stay on the same reference */ } else { ed.stage = "a"; ed.ref = null; }
      }
    }
    tabIdx = 0;
    cb.prompt(promptText());
    render();
    return true;
  }

  function editMove(sx, sy) {
    if (!ed) return;
    edHover = null;
    if (tool === "mirror" && eo.mirrorMode === "pick" && ed.stage === "a") {
      const c = edgeCands(sx, sy, "all"); const e = c[tabIdx % Math.max(1, c.length)]; if (e) edHover = { kind: "edge", a: e.a, b: e.b };
    } else if (tool === "align") {
      const c = edgeCands(sx, sy, eo.prefer); const e = c[tabIdx % Math.max(1, c.length)]; if (e) edHover = { kind: "edge", a: e.a, b: e.b, owner: e.owner };
    } else if (tool === "offset" && ed.stage === "a") {
      const w = wallAt(sx, sy); if (w) edHover = { kind: "wall", id: w.id, side: sideOf(w, toWorld(sx, sy)) >= 0 ? 1 : -1 };
    } else if (tool === "trim") {
      const w = wallAt(sx, sy); if (w) edHover = { kind: "wall", id: w.id, pick: toWorld(sx, sy) };
    }
  }

  // outlines of the selection (or of walls) pushed through a point mapping, drawn as a dashed preview
  function ghost(map, ids) {
    ids = expandIds(ids);
    ctx.save();
    ctx.strokeStyle = css("--accent"); ctx.lineWidth = 1.6; ctx.setLineDash([6, 4]);
    ctx.fillStyle = "rgba(46,111,158,0.12)";
    for (const e of elements()) {
      if (!ids.has(e.id)) continue;
      const rings = e.kind === "wall" ? (data.walls.find((w) => w.id === e.id).pieces || [e.ring]) : [e.ring];
      for (const r of rings) {
        ctx.beginPath();
        r.forEach((q, i) => { const m = map({ x: q[0], y: q[1] }), s = toScreen(m); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
        ctx.closePath(); ctx.fill(); ctx.stroke();
      }
    }
    ctx.restore();
  }
  function lineOnScreen(a, b, col, w, dashed) {
    const sa = toScreen({ x: a[0], y: a[1] }), sb = toScreen({ x: b[0], y: b[1] });
    ctx.save(); ctx.strokeStyle = col; ctx.lineWidth = w; if (dashed) ctx.setLineDash([8, 5]);
    ctx.beginPath(); ctx.moveTo(sa.x, sa.y); ctx.lineTo(sb.x, sb.y); ctx.stroke(); ctx.restore();
  }

  // the centre of rotation: a circle with a cross, which can be dragged
  function drawCentre() {
    const sc = toScreen(centreOfRotation());
    ctx.save(); ctx.strokeStyle = css("--accent"); ctx.fillStyle = "#fff"; ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.arc(sc.x, sc.y, 6, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(sc.x - 10, sc.y); ctx.lineTo(sc.x + 10, sc.y); ctx.moveTo(sc.x, sc.y - 10); ctx.lineTo(sc.x, sc.y + 10); ctx.stroke(); ctx.restore();
  }

  function drawEdit() {
    if (dragging) {
      const d = dragging.d;
      ghost((q) => ({ x: q.x + d.x, y: q.y + d.y }), new Set(sel));
      const s0 = toScreen(dragging.p0), s1 = toScreen({ x: dragging.p0.x + d.x, y: dragging.p0.y + d.y });
      ctx.save(); ctx.strokeStyle = css("--accent"); ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(s0.x, s0.y); ctx.lineTo(s1.x, s1.y); ctx.stroke(); ctx.restore();
      label(`${Math.round(Math.hypot(d.x, d.y))}`, { x: (s0.x + s1.x) / 2, y: (s0.y + s1.y) / 2 - 12 }, css("--accent"));
    }
    if (!ed) return;
    const accent = css("--accent");
    const ids = new Set(sel);
    if (tool === "move" || tool === "copy") {
      if (ed.stage === "b" && hover) {
        const d = moveDelta(hover);
        ghost((q) => ({ x: q.x + d.x, y: q.y + d.y }), ids);
        const s0 = toScreen(ed.p0), s1 = toScreen({ x: ed.p0.x + d.x, y: ed.p0.y + d.y });
        ctx.save(); ctx.strokeStyle = accent; ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(s0.x, s0.y); ctx.lineTo(s1.x, s1.y); ctx.stroke(); ctx.restore();
        label(`${Math.round(Math.hypot(d.x, d.y))}`, { x: (s0.x + s1.x) / 2, y: (s0.y + s1.y) / 2 - 12 }, accent);
      }
    } else if (tool === "array") {
      if (eo.arrayKind === "radial") drawCentre();
      if (ed.stage === "b" && hover) {
        const s = arrayStep(hover);
        for (let i = 1; i < s.n; i++) ghost(s.map(i), ids);
        const s0 = toScreen(ed.p0), s1 = toScreen(hover);
        ctx.save(); ctx.strokeStyle = accent; ctx.setLineDash([3, 3]); ctx.beginPath();
        if (eo.arrayKind === "radial") { const sc = toScreen(centreOfRotation()); ctx.moveTo(sc.x, sc.y); ctx.lineTo(s0.x, s0.y); ctx.moveTo(sc.x, sc.y); ctx.lineTo(s1.x, s1.y); }
        else { const e = toScreen({ x: ed.p0.x + s.v.x, y: ed.p0.y + s.v.y }); ctx.moveTo(s0.x, s0.y); ctx.lineTo(e.x, e.y); }
        ctx.stroke(); ctx.restore();
        const txt = eo.arrayKind === "radial" ? `${s.angle.toFixed(1)}°` : `${Math.round(Math.hypot(s.v.x, s.v.y))}`;
        label(`${txt}   Number: ${s.n}`, { x: s1.x, y: s1.y - 22 }, accent);
      }
    } else if (tool === "rotate") {
      const c = centreOfRotation(), sc = toScreen(c);
      drawCentre();
      if (ed.stage === "b" && hover) {
        const a = rotateAngle(hover) * Math.PI / 180, cs = Math.cos(a), sn = Math.sin(a);
        ghost((q) => ({ x: c.x + (q.x - c.x) * cs - (q.y - c.y) * sn, y: c.y + (q.x - c.x) * sn + (q.y - c.y) * cs }), ids);
        const s0 = toScreen(ed.p0), s1 = toScreen(hover);
        ctx.save(); ctx.strokeStyle = accent; ctx.setLineDash([3, 3]);
        ctx.beginPath(); ctx.moveTo(sc.x, sc.y); ctx.lineTo(s0.x, s0.y); ctx.moveTo(sc.x, sc.y); ctx.lineTo(s1.x, s1.y); ctx.stroke(); ctx.restore();
        label(`${(a * 180 / Math.PI).toFixed(Math.abs(a) < 1e-9 ? 0 : 1)}°`, { x: sc.x, y: sc.y - 22 }, accent);
      }
    } else if (tool === "mirror") {
      if (edHover && edHover.kind === "edge") lineOnScreen(edHover.a, edHover.b, accent, 2.5, true);
      if (ed.stage === "b" && eo.mirrorMode === "draw" && hover) {
        lineOnScreen([ed.p0.x, ed.p0.y], [hover.x, hover.y], accent, 1.5, true);
        const m = { ax: ed.p0.x, ay: ed.p0.y, bx: hover.x, by: hover.y }, dx = m.bx - m.ax, dy = m.by - m.ay, l2 = dx * dx + dy * dy;
        if (l2 > 1) ghost((q) => { const t = ((q.x - m.ax) * dx + (q.y - m.ay) * dy) / l2; return { x: 2 * (m.ax + t * dx) - q.x, y: 2 * (m.ay + t * dy) - q.y }; }, ids);
      }
    } else if (tool === "offset") {
      if (ed.stage === "a" && edHover && edHover.kind === "wall") {
        const w = data.walls.find((x) => x.id === edHover.id), d = wallDir(w);
        const chain = ed.chain ? wallChain(w.id) : [w.id];
        const k = eo.offsetMode === "numerical" ? edHover.side * (eo.offsetDist || 0) : 0;
        ghost((q) => ({ x: q.x - d.y * k, y: q.y + d.x * k }), new Set(chain));
      } else if (ed.stage === "b" && hover) {
        const w = offsetTarget();
        if (w) { const typed = parseFloat(ed.typed), k = !isNaN(typed) ? Math.abs(typed) * (sideOf(w, hover) >= 0 ? 1 : -1) : sideOf(w, hover), d = wallDir(w); ghost((q) => ({ x: q.x - d.y * k, y: q.y + d.x * k }), new Set(ed.chainIds)); label(`${Math.round(Math.abs(k))}`, toScreen(hover), accent); }
      }
    } else if (tool === "trim") {
      const mark = (id, col) => { const w = data.walls.find((x) => x.id === id); if (w) { ctx.save(); ctx.strokeStyle = col; ctx.lineWidth = 2.5; (w.pieces || [w.corners]).forEach((r) => { ctx.beginPath(); r.forEach((q, i) => { const s = toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); }); ctx.closePath(); ctx.stroke(); }); ctx.restore(); } };
      if (ed.first) mark(ed.first.id, accent);
      if (ed.boundary) mark(ed.boundary, "#3c965a");
      if (edHover) mark(edHover.id, "#d4572f");
    } else if (tool === "align") {
      if (ed.ref) lineOnScreen(ed.ref.a, ed.ref.b, "#3c965a", 2.5, false);
      if (edHover) lineOnScreen(edHover.a, edHover.b, "#d4572f", 2.5, true);
    }
  }

  // typed values (the keyboard entry): digits then Enter
  function editKey(e) {
    if (!ed || ed.stage === "pick") return false;
    if (/^[0-9.]$/.test(e.key) || (e.key === "-" && !ed.typed)) { ed.typed += e.key; cb.prompt(`Typed: ${ed.typed}. Press Enter to apply, Esc to clear.`); render(); return true; }
    if (e.key === "Backspace" && ed.typed) { ed.typed = ed.typed.slice(0, -1); render(); return true; }
    if (e.key === "Enter" && ed.typed !== "" && hover) {
      const v = parseFloat(ed.typed);
      if (isNaN(v)) { ed.typed = ""; return true; }
      if (tool === "rotate" && ed.stage === "a") {
        const c = centreOfRotation();
        cb.edit({ kind: "rotate", ids: [...sel], cx: c.x, cy: c.y, angle_deg: v, copy: eo.copy, disjoin: eo.disjoin });
        finishEdit(false);
        return true;
      }
      // a radial array takes a typed angle straight away (from the start ray at zero)
      if (tool === "array" && eo.arrayKind === "radial" && ed.stage === "a") { ed.p0 = hover; sendArray(hover); finishEdit(false); return true; }
      if ((tool === "move" || tool === "copy" || tool === "rotate" || tool === "offset" || tool === "array") && ed.stage === "b") { editClick(lastMouse.x, lastMouse.y, { ctrlKey: false }); return true; }
    }
    return false;
  }

  function selectClick(sx, sy, ev) {
    const c = candidates(sx, sy);
    const ids = c.length ? c[tabIdx % c.length] : null;
    tabIdx = 0;
    // Edit Group's Add and Remove take what is clicked instead of selecting it
    if (groupEdit && groupEdit.mode) { if (ids) cb.groupPick(ids); selHover = null; render(); return; }
    // Ctrl adds to the selection and Shift removes from it
    if (!ids) { if (!ev.ctrlKey && !ev.shiftKey) sel = new Set(); }
    else if (ev.shiftKey) ids.forEach((i) => sel.delete(i));
    else if (ev.ctrlKey) ids.forEach((i) => sel.add(i));
    else sel = new Set(ids);
    selHover = null;
    cb.select([...sel]);
    render();
  }

  function click(sx, sy, ev) {
    if (tool === "sketch") {
      const p = snap(sx, sy);
      Snap.clearOverride();
      snapCycle = 0;
      Sketch.click(p, sx, sy, ev);
      return;
    }
    if (ed && editClick(sx, sy, ev)) return;
    if (tool === "dimension") { Dims.click(sx, sy, ev); cb.prompt(promptText()); return; }
    if (tool === "spot") { Spots.click(sx, sy, ev); cb.prompt(promptText()); return; }
    if (tool === "text" || tool === "noteleader") {
      const p = Det.toolSnap(sx, sy);
      Snap.clearOverride(); snapCycle = 0;
      if (tool === "noteleader") Det.leaderClick(p); else Det.toolClick(p);
      cb.prompt(promptText()); render();
      return;
    }
    if (tool === "groupplace") {
      const p = hover || toWorld(sx, sy);
      if (groupPlace) cb.placeGroup(groupPlace.typeId, { x: p.x, y: p.y });
      return;
    }
    if (tool === "component") {
      const p = hover || toWorld(sx, sy);
      if (comp) cb.placeComponent({ x: p.x, y: p.y }, comp.rot);
      return;
    }
    if (hostedTool(tool)) {
      const ph = placementAt(sx, sy);
      if (ph) cb.placeInstance(ph.wall, { x: toWorld(sx, sy).x, y: toWorld(sx, sy).y });
      return;
    }
    if (tool === "joins") {
      const e = endAt(sx, sy);
      if (!e) { if (!(ev.ctrlKey || ev.shiftKey)) joinSel = []; }
      else {
        const same = (j) => j.id === e.id && j.end === e.end;
        if (ev.ctrlKey || ev.shiftKey) joinSel = joinSel.some(same) ? joinSel.filter((j) => !same(j)) : [...joinSel, { id: e.id, end: e.end, p: e.p }];
        else joinSel = [{ id: e.id, end: e.end, p: e.p }];
      }
      cb.joinEnd(joinSel.map((j) => ({ id: j.id, end: j.end })));
      render();
      return;
    }
    const p = tool === "grid" ? gridSnap(sx, sy) : snap(sx, sy);
    Snap.clearOverride();
    snapCycle = 0;
    if (tool === "select") {
      selectClick(sx, sy, ev);
    } else if (tool === "grid") {
      gridClick(p, sx, sy);
      return;
    } else if (tool === "secsplit") {
      Sections.splitClick(sx, sy);
      return;
    } else if (tool === "section") {
      if (!draw) draw = { pts: [p] };
      else if (dist(draw.pts[0], p) > 100) { const a = draw.pts[0]; draw = null; cb.addSection([Math.round(a.x * 1000) / 1000, Math.round(a.y * 1000) / 1000], [Math.round(p.x * 1000) / 1000, Math.round(p.y * 1000) / 1000]); return; }
    } else if (tool === "elevation") {
      cb.placeMarker({ x: Math.round(p.x * 1000) / 1000, y: Math.round(p.y * 1000) / 1000 }, markerRotAt(p));
    } else if (tool === "wall" && SHAPE_TWO_CLICK.has(wallShape)) {
      if (!draw) draw = { pts: [p] };
      else {
        const segs = shapeSegments(draw.pts[0], p, wallShape, wallSides);
        if (segs) { cb.addWalls(segs); draw = null; }
      }
    } else if (tool === "wall" && SHAPE_CURVE[wallShape]) {
      const sc = SHAPE_CURVE[wallShape];
      if (!draw) draw = { pts: [p] };
      else if (sc.n === 0) draw.pts.push(p);
      else if (draw.pts.length === sc.n - 1) { finishCurveWall(p); return; }
      else draw.pts.push(p);
    } else if (tool === "wall") {
      if (!draw) draw = { pts: [p] };
      else {
        const a = draw.pts[0];
        if (dist(a, p) > 1) { cb.addWall([a.x, a.y], [p.x, p.y]); draw = { pts: [p] }; }
      }
    }
    cb.prompt(promptText());
    render();
  }

  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("pointerdown", (e) => {
    canvas.setPointerCapture(e.pointerId);
    if (e.button === 1 || e.button === 2) { pan = { x: e.clientX, y: e.clientY, moved: false, button: e.button }; return; }
    down = { x: e.offsetX, y: e.offsetY };
    if (e.button === 0 && Dims.lockDown(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && e.button === 0 && Dims.down(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && e.button === 0 && Spots.down(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && e.button === 0 && Det.gripDown(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && e.button === 0 && Sections.down(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "sketch" && e.button === 0 && Sketch.gripDown(e.offsetX, e.offsetY)) { down = null; return; }
    // the centre of rotation can be dragged to a new place
    if (e.button === 0 && ed && ed.stage !== "pick" && radialTool() && !eo.centrePlace) {
      const sc = toScreen(centreOfRotation());
      if (Math.hypot(e.offsetX - sc.x, e.offsetY - sc.y) <= 9) { cdrag = true; down = null; return; }
    }
    if (tool === "select" && e.button === 0) {
      const gp = gpins.find((q) => padlockHit(e.offsetX, e.offsetY, q.x, q.y));
      if (gp) { down = null; cb.toggleGridLock(gp); return; }
      const gb = gboxes.find((q) => Math.abs(e.offsetX - q.x) <= 7 && Math.abs(e.offsetY - q.y) <= 7);
      if (gb) { down = null; const b = gb.g.bubbles.slice(); b[gb.end] = !b[gb.end]; cb.patchGrid(gb.id, { bubbles: b }); return; }
      const gg = ggrips.find((q) => Math.hypot(e.offsetX - q.x, e.offsetY - q.y) <= 8);
      if (gg) { ggrip = { id: gg.id, end: gg.end, g: gg.g, cur: null, moved: false, px: e.offsetX, py: e.offsetY, preview: null }; down = null; return; }
      const hb = gbubbles.find((q) => sel.has(q.id) && Math.hypot(e.offsetX - q.x, e.offsetY - q.y) <= q.r);
      if (hb) { down = null; gridOpen = hb; return; }
    }
    if (tool === "select" && e.button === 0) {
      const mg = mgrips.find((q) => Math.abs(e.offsetX - q.x) <= 7 && Math.abs(e.offsetY - q.y) <= 7);
      if (mg) { down = null; cb.markerSlot(mg.id, mg.slot, !mg.on); return; }
    }
    if (tool === "select" && e.button === 0) {
      const g = wgrips.find((q) => Math.hypot(e.offsetX - q.x, e.offsetY - q.y) <= 9);
      if (g && g.pt) {
        wgrip = { id: g.id, pt: true, k: g.k, wall: g.wall, cur: { x: g.wall.grips[g.k][0], y: g.wall.grips[g.k][1] }, start: { x: g.wall.grips[g.k][0], y: g.wall.grips[g.k][1] }, moved: false, px: e.offsetX, py: e.offsetY, poly: null };
        down = null;
        return;
      }
      if (g) {
        const w = g.wall, fixed = g.end === 1 ? w.start : w.end, own = g.end === 1 ? w.end : w.start;
        const members = (g.members || [{ id: g.id, end: g.end, wall: w }]).map((m) => { const f = m.end === 1 ? m.wall.start : m.wall.end; return { id: m.id, end: m.end, wall: m.wall, fixed: { x: f[0], y: f[1] } }; });
        wgrip = { id: g.id, end: g.end, wall: w, members, fixed: { x: fixed[0], y: fixed[1] }, cur: { x: own[0], y: own[1] }, start: { x: own[0], y: own[1] }, moved: false, px: e.offsetX, py: e.offsetY };
        down = null;
        return;
      }
    }
    if (tool === "select" && e.button === 0 && tdims.length) {
      const t = tdimHit(e.offsetX, e.offsetY);
      if (t) {
        if (t.type === "label") { down = null; tdimOpen = t; return; }
        if (t.type === "promote") { down = null; cb.tempPromote(t.dim, 55 / cam.s); return; }
        tgrip = { dim: t.dim, role: t.role, x: t.x, y: t.y, px: e.offsetX, py: e.offsetY, lines: null, cand: null };
        const g = tgrip;
        g.ready = Promise.resolve(cb.tempLines()).then((l) => { g.lines = l; });
        down = null;
        return;
      }
    }
    // Drag Elements on Selection: pressing on an element and moving carries it, without selecting it first
    if (tool === "select" && selOpts.dragOnSel && e.button === 0) {
      const c = candidates(e.offsetX, e.offsetY);
      if (c.length) down.drag = { ids: c[0], p0: snap(e.offsetX, e.offsetY) };
    }
  });
  canvas.addEventListener("pointermove", (e) => {
    shift = e.shiftKey;
    if (pan) {
      const dx = e.clientX - pan.x, dy = e.clientY - pan.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) pan.moved = true;
      pan.x = e.clientX; pan.y = e.clientY;
      cam.x -= dx / cam.s; cam.y += dy / cam.s;
      render();
      return;
    }
    if (cdrag) { const p = snap(e.offsetX, e.offsetY); hover = p; ed.centre = { x: p.x, y: p.y }; cb.cursor(p); render(); return; }
    if (ggrip) {
      if (Math.hypot(e.offsetX - ggrip.px, e.offsetY - ggrip.py) > 3) ggrip.moved = true;
      if (ggrip.moved) {
        let p = snap(e.offsetX, e.offsetY);
        // the end lines up with the end of a parallel grid (not one already locked to it)
        const path = ggrip.g.path;
        gridAlignHint = null;
        if (path.length === 2 && !ggrip.g.is_arc && ["free", "length", "hv", "grid"].includes(p.kind)) {
          const skip = new Set([ggrip.id, ...((ggrip.g.end_locks || [[], []])[ggrip.end] || []).map((r) => r.id)]);
          const u = gUnit(ggrip.g);
          const a = gridAlign(p, { dir: u, exclude: skip, end: ggrip.end });
          if (a) { p = a; gridAlignHint = a; }
        }
        hover = p; ggrip.cur = p; ggrip.align = gridAlignHint ? gridAlignHint.align : null; cb.cursor(p);
        // a straight grid slides along its own line; the preview shows where the end lands
        if (path.length === 2) {
          const fixed = ggrip.end === 0 ? path[1] : path[0], mv = ggrip.end === 0 ? path[0] : path[1];
          const dx = mv[0] - fixed[0], dy = mv[1] - fixed[1], L = Math.hypot(dx, dy) || 1, u = [dx / L, dy / L];
          const t = Math.max(10, (p.x - fixed[0]) * u[0] + (p.y - fixed[1]) * u[1]);
          const np = [fixed[0] + u[0] * t, fixed[1] + u[1] * t];
          ggrip.preview = ggrip.end === 0 ? [np, fixed] : [fixed, np];
        }
      }
      render();
      return;
    }
    if (wgrip) {
      if (Math.hypot(e.offsetX - wgrip.px, e.offsetY - wgrip.py) > 3) wgrip.moved = true;
      if (wgrip.moved && wgrip.pt) {
        const p = snap(e.offsetX, e.offsetY);
        hover = p; wgrip.cur = { x: p.x, y: p.y };
        cb.cursor(hover);
        const my = (wgrip.seq = (wgrip.seq || 0) + 1), g = wgrip;
        Promise.resolve(cb.reshapePreview(g.id, g.k, [Math.round(p.x * 1000) / 1000, Math.round(p.y * 1000) / 1000])).then((poly) => { if (wgrip === g && g.seq === my) { g.poly = poly || null; render(); } });
      } else if (wgrip.moved) {
        // snap as when drawing from the other end (Shift keeps to 45 degrees)
        const keep = draw; draw = wgrip.members.length > 1 ? null : { pts: [wgrip.fixed] };
        const p = snap(e.offsetX, e.offsetY);
        draw = keep;
        hover = p; wgrip.cur = { x: p.x, y: p.y };
        cb.cursor(hover);
      }
      render();
      return;
    }
    if (tgrip) {
      tgrip.x = e.offsetX; tgrip.y = e.offsetY;
      tgrip.cand = null;
      const L = tgrip.lines;
      if (L) {
        // the nearest line across the direction the dimension runs, within a few pixels
        const p = toWorld(e.offsetX, e.offsetY), posc = (p.x - L.origin[0]) * L.axis[0] + (p.y - L.origin[1]) * L.axis[1];
        let best = Infinity;
        for (const c of tgrip.role === "own" ? L.own : L.targets) {
          const gap = Math.abs(c.pos - posc) * cam.s;
          if (gap > 14) continue;
          // among lines at the same place, the one whose stretch is nearest the cursor
          const [ax, ay, bx, by] = [c.a[0], c.a[1], c.b[0], c.b[1]], vx = bx - ax, vy = by - ay, l2 = vx * vx + vy * vy || 1;
          const t = Math.max(0, Math.min(1, ((p.x - ax) * vx + (p.y - ay) * vy) / l2));
          const score = gap + Math.hypot(p.x - (ax + vx * t), p.y - (ay + vy * t)) * cam.s * 0.02;
          if (score < best) { best = score; tgrip.cand = c; }
        }
      }
      render();
      return;
    }
    if (Dims.dragging) { Dims.dragMove(e.offsetX, e.offsetY); return; }
    if (Spots.dragging) { Spots.dragMove(e.offsetX, e.offsetY); return; }
    if (Det.dragging) { Det.gripMove(e.offsetX, e.offsetY); return; }
    if (Sections.dragging) { Sections.dragMove(e.offsetX, e.offsetY); return; }
    if (Sketch.gripDragging) { lastMouse = { x: e.offsetX, y: e.offsetY }; hover = snap(e.offsetX, e.offsetY); cb.cursor(hover); Sketch.gripMove(e.offsetX, e.offsetY, hover); return; }
    const moved = down && Math.hypot(e.offsetX - down.x, e.offsetY - down.y) > 5;
    if (moved && down.drag && !dragging) {
      dragging = { ids: new Set(down.drag.ids), p0: down.drag.p0, d: { x: 0, y: 0 } };
      if (![...dragging.ids].every((i) => sel.has(i))) { sel = new Set(dragging.ids); cb.select([...sel]); }
    }
    if (dragging) {
      hover = snap(e.offsetX, e.offsetY);
      dragging.d = { x: hover.x - dragging.p0.x, y: hover.y - dragging.p0.y };
      if (shift) { Math.abs(dragging.d.x) >= Math.abs(dragging.d.y) ? (dragging.d.y = 0) : (dragging.d.x = 0); }
      cb.cursor(hover);
      render();
      return;
    }
    if (moved && !band && (tool === "select" || tool === "joins" || (ed && ed.stage === "pick") || (tool === "sketch" && Sketch.canBand()))) band = { x0: down.x, y0: down.y, x1: e.offsetX, y1: e.offsetY };
    if (band) { band.x1 = e.offsetX; band.y1 = e.offsetY; render(); return; }
    if (tool === "dimension") Dims.move(e.offsetX, e.offsetY);
    if (tool === "spot") Spots.move(e.offsetX, e.offsetY);
    if (tool === "secsplit") Sections.splitMove(e.offsetX, e.offsetY);
    if (tool === "joins") { joinHover = endAt(e.offsetX, e.offsetY); canvas.style.cursor = joinHover ? "pointer" : "crosshair"; }
    if (tool === "grid" && gridShape === "pick") { gridHover = gridPickAt(e.offsetX, e.offsetY); canvas.style.cursor = gridHover ? "pointer" : "crosshair"; }
    if (hostedTool(tool)) { placeHover = placementAt(e.offsetX, e.offsetY); canvas.style.cursor = placeHover ? "crosshair" : "not-allowed"; updatePlaceDims(); }
    if (!lastMouse || Math.hypot(e.offsetX - lastMouse.x, e.offsetY - lastMouse.y) > 3) { snapCycle = 0; tabIdx = 0; }
    editMove(e.offsetX, e.offsetY);
    if (tool === "select") { const c = candidates(e.offsetX, e.offsetY); selHover = c.length ? c[tabIdx % c.length] : null; }
    lastMouse = { x: e.offsetX, y: e.offsetY };
    hover = tool === "grid" ? gridSnap(e.offsetX, e.offsetY) : tool === "text" || tool === "noteleader" ? Det.toolSnap(e.offsetX, e.offsetY) : snap(e.offsetX, e.offsetY);
    cb.cursor(hover);
    if (Sketch.active) Sketch.move(hover, e.offsetX, e.offsetY);
    if (tool === "wall" && draw && SHAPE_CURVE[wallShape]) refreshWallShape();
    if (tool === "grid" && draw && GRID_ARC.has(gridShape)) refreshGridShape();
    render();
  });
  canvas.addEventListener("pointerup", (e) => {
    if (Dims.dragging) { down = null; Dims.up(e.offsetX, e.offsetY); return; }
    if (Spots.dragging) { down = null; Spots.up(e.offsetX, e.offsetY); return; }
    if (Det.dragging) { down = null; Det.gripUp(); return; }
    if (Sections.dragging) { down = null; Sections.up(e.offsetX, e.offsetY); return; }
    if (Sketch.gripDragging) { down = null; Sketch.gripUp(); return; }
    if (cdrag) { cdrag = false; down = null; Snap.clearOverride(); render(); return; }
    if (tdimOpen) { const t = tdimOpen; tdimOpen = null; openTdimInput(t); return; }
    if (gridOpen) { const b = gridOpen; gridOpen = null; openGridName(b); return; }
    if (ggrip) {
      const g = ggrip; ggrip = null; hover = null;
      const r = (v) => Math.round(v * 1000) / 1000;
      gridAlignHint = null;
      if (g.moved && g.cur) cb.moveGridEnd(g.id, g.end, [r(g.cur.x), r(g.cur.y)], g.align ? { end: g.end, id: g.align.id, otherEnd: g.align.end } : null);
      render();
      return;
    }
    if (wgrip) {
      const g = wgrip; wgrip = null; hover = null;
      const r = (v) => Math.round(v * 1000) / 1000;
      if (g.pt) { if (g.moved && dist(g.cur, g.start) > 1) cb.reshapeWall(g.id, g.k, [r(g.cur.x), r(g.cur.y)]); render(); return; }
      if (g.moved && dist(g.cur, g.start) > 1) {
        if (g.members.length > 1) cb.moveWallEnds(g.members.map((m) => ({ id: m.id, end: m.end })), [r(g.cur.x), r(g.cur.y)]);
        else cb.moveWallEnd(g.id, g.end, [r(g.cur.x), r(g.cur.y)]);
      }
      render();
      return;
    }
    if (tgrip) {
      const g = tgrip; tgrip = null;
      if (Math.hypot(e.offsetX - g.px, e.offsetY - g.py) < 4) {
        // a click on the grip steps to the next line on that element (outer face, centre, inner face, core ...)
        (g.ready || Promise.resolve()).then(() => { const next = cycleCand(g); if (next) cb.tempPin(g.dim.key, g.role, next.spec); render(); });
        render();
        return;
      }
      // dropped on a line: measure from or to it; dropped on empty space: back to the default
      cb.tempPin(g.dim.key, g.role, g.cand ? g.cand.spec : null);
      render();
      return;
    }
    if (pan) {
      const wasClick = !pan.moved && pan.button === 2;
      pan = null;
      if (wasClick && Sketch.active) { Sketch.rightClick(); return; }
      if (wasClick && tool === "spot") { if (!Spots.cancel()) setTool("select"); cb.prompt(promptText()); render(); return; }
      if (wasClick && tool === "dimension") { if (!Dims.cancel()) setTool("select"); cb.prompt(promptText()); render(); return; }
      if (wasClick) { if (draw) { draw = null; cb.prompt(promptText()); render(); } else if (tool !== "select") setTool("select"); }
      return;
    }
    if (dragging) {
      const d = dragging; dragging = null; down = null;
      if (Math.hypot(d.d.x, d.d.y) > 1) cb.edit({ kind: "move", ids: [...sel], dx: Math.round(d.d.x * 1000) / 1000, dy: Math.round(d.d.y * 1000) / 1000, copy: !!e.ctrlKey, disjoin: false });
      render();
      return;
    }
    if (band) {
      const b = band; band = null; down = null;
      if (tool === "sketch") {
        Sketch.bandSelect(worldRect(b), b.x1 >= b.x0, e);
      } else if (tool === "joins") {
        const r = worldRect(b);
        const picked = [];
        for (const w of data.walls) for (const end of [0, 1]) {
          const q = end === 0 ? w.start : w.end;
          if (q[0] >= r.x0 && q[0] <= r.x1 && q[1] >= r.y0 && q[1] <= r.y1) picked.push({ id: w.id, end, p: { x: q[0], y: q[1] } });
        }
        const key = (j) => j.id + ":" + j.end;
        joinSel = e.ctrlKey || e.shiftKey ? [...joinSel.filter((j) => !picked.some((q) => key(q) === key(j))), ...picked] : picked;
        cb.joinEnd(joinSel.map((j) => ({ id: j.id, end: j.end })));
      } else {
        const ids = bandPick(b);
        if (e.shiftKey) ids.forEach((i) => sel.delete(i));
        else if (e.ctrlKey) ids.forEach((i) => sel.add(i));
        else sel = new Set(ids);
        cb.select([...sel]);
      }
      render();
      return;
    }
    if (down && Math.hypot(e.offsetX - down.x, e.offsetY - down.y) < 5) click(e.offsetX, e.offsetY, e);
    down = null;
  });
  canvas.addEventListener("dblclick", (e) => {
    if (tool === "sketch") Sketch.dblclick();
    else if (tool === "select" && Dims.dblclick(e.offsetX, e.offsetY)) { /* the text dialog opened */ }
    else if (tool === "select" && Spots.dblclick(e.offsetX, e.offsetY)) { /* a bend came out of the leader */ }
    else if (tool === "grid" && gridShape === "multi" && draw) finishGridChain();
    else if (tool === "select") {
      // double-clicking a section opens the view it makes
      const c = candidates(e.offsetX, e.offsetY), sec = c.length ? (data.sections || []).find((s) => c[0].includes(s.id)) : null;
      // double-clicking a group opens it in Edit Group
      const grp = c.length && c[0].length === 1 ? modelGroups.find((g) => g.id === c[0][0]) : null;
      if (grp) { cb.editGroup(grp.id); return; }
      if (sec && sec.view_id) { cb.openView(sec.view_id); return; }
      const id = hit(e.offsetX, e.offsetY); const f = id && data.floors.find((x) => x.id === id); if (f) cb.editFloor(f.id); else { const rf = id && (data.roofs || []).find((x) => x.id === id); if (rf) cb.editRoof(rf.id); }
      // a detail line opens Detail Line with it selected; a region, Edit Boundary; a text note, its text
      if (id && (data.text_notes || []).some((x) => x.id === id)) cb.editNote(id);
      else if (id && (data.detail_lines || []).some((x) => x.id === id)) cb.editDetail(id);
      else if (id && (data.regions || []).some((x) => x.id === id)) cb.editRegion(id); }
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const before = toWorld(e.offsetX, e.offsetY);
    cam.s = Math.min(2, Math.max(0.005, cam.s * Math.exp(-e.deltaY * 0.0015)));
    const after = toWorld(e.offsetX, e.offsetY);
    cam.x += before.x - after.x; cam.y += before.y - after.y;
    render();
  }, { passive: false });
  new ResizeObserver(render).observe(canvas);

  Dims.attach({
    ctx: () => ctx, toScreen, toWorld, label, css, render: () => render(),
    scale: () => cam.s,
    paperPx: () => ((data.view && data.view.scale) || 100) * cam.s,
    thin: () => thin,
    dims: () => data.dimensions || [],
    selection: () => [...sel],
    select: (ids) => { sel = new Set(ids); cb.select([...sel]); render(); },
    viewId: () => (data.view && data.view.id),
    settings: () => cb.dimSettings(),
    prompt: (t) => cb.prompt(t),
    call: (m, p) => cb.dimCall(m, p),
    refresh: () => cb.dimRefresh(),
    openText: (d, s) => cb.dimText(d, s),
    setTool: (t) => setTool(t),
    canvasRect: () => canvas.getBoundingClientRect(),
  });

  Sections.attach({
    ctx: () => ctx, toScreen, toWorld, css, render: () => render(),
    scale: () => cam.s,
    paperPx: () => ((data.view && data.view.scale) || 100) * cam.s,
    sections: () => data.sections || [],
    family: () => data.section_family,
    pats: () => data.fill_patterns,
    selection: () => [...sel],
    snap: (sx, sy) => snap(sx, sy),
    call: (m, p) => cb.dimCall(m, p),
    // a preview asks the model without reporting errors: a cursor still on the line is not a mistake
    preview: (m, p) => Backend.call(m, p).catch(() => null),
    refresh: () => cb.dimRefresh(),
    viewId: () => data.view && data.view.id,
    prompt: (t) => cb.prompt(t),
    done: () => setTool("select"),
  });

  Spots.attach({
    ctx: () => ctx, toScreen, toWorld, label, css, render: () => render(),
    scale: () => cam.s,
    paperPx: () => ((data.view && data.view.scale) || 100) * cam.s,
    thin: () => thin,
    spots: () => data.spots || [],
    selection: () => [...sel],
    viewId: () => (data.view && data.view.id),
    prompt: (t) => cb.prompt(t),
    call: (m, p) => cb.dimCall(m, p),
    refresh: () => cb.dimRefresh(),
    setTool: (t) => setTool(t),
  });

  const Det = makeDetails({
    ctx: () => ctx, toScreen, toWorld, css, render: () => render(),
    zoom: () => cam.s,
    scale: () => (data.view && data.view.scale) || 100,
    data: () => data,
    thin: () => thin,
    held: (x) => heldBySketch(x),
    faded: (id) => groupEdit != null && memberOf.get(id) !== groupEdit.id,
    snap: (sx, sy, o) => snap(sx, sy, o),
    shift: () => shift,
    options: () => DetailCallbacks.options(),
    cb: DetailCallbacks,
  });

  const planHost = {
    get ctx() { return ctx; },
    get canvas() { return canvas; },
    size, toScreen, toWorld, label, css, render: () => render(),
    scale: () => cam.s,
    wallAt: (sx, sy) => wallAt(sx, sy),
    wallById: (id) => data.walls.find((w) => w.id === id),
    wallChain: (id) => wallChain(id),
    edgeAt: (sx, sy, skipFloor) => { const c = edgeCands(sx, sy, "all").find((x) => !(x.kind === "floor" && x.owner === skipFloor)); return c ? { a: { x: c.a[0], y: c.a[1] }, b: { x: c.b[0], y: c.b[1] } } : null; },
    edgesNear: (sx, sy, skipFloor) => {
      const p = toWorld(sx, sy), tol = 8 / cam.s;
      const out = edgeCands(sx, sy, "all").filter((x) => !(x.kind === "floor" && x.owner === skipFloor)).map((c) => ({ a: { x: c.a[0], y: c.a[1] }, b: { x: c.b[0], y: c.b[1] } }));
      for (const g of data.grids || []) for (let i = 0; i + 1 < (g.path || []).length; i++) if (segDist(p, g.path[i], g.path[i + 1]) <= tol) out.push({ a: { x: g.path[i][0], y: g.path[i][1] }, b: { x: g.path[i + 1][0], y: g.path[i + 1][1] } });
      return out;
    },
    cycleSnap: (d) => { snapCycle += d; if (lastMouse) { hover = snap(lastMouse.x, lastMouse.y); Sketch.move(hover, lastMouse.x, lastMouse.y); } render(); },
  };
  Sketch.attach(planHost);

  return {
    callbacks: cb,
    setTool,
    get tool() { return tool; },
    get sketching() { return tool === "sketch"; },
    // Sketch mode: the sketch is drawn over the plan; the floor being edited is hidden meanwhile
    startSketch(o) {
      setTool("select");
      tool = "sketch";
      if (o.id) Snap.build(Snap.fromPlan({ ...data, floors: data.floors.filter((f) => f.id !== o.id), roofs: (data.roofs || []).filter((r) => r.id !== o.id) }, heldBySketch));
      sel = new Set();
      Sketch.attach(planHost);
      Sketch.begin(o);
      canvas.style.cursor = "crosshair";
      cb.tool("sketch");
      render();
    },
    endSketch() {
      Sketch.end();
      Snap.build(Snap.fromPlan(data, heldBySketch));
      setTool("select");
    },
    setData(d) {
      data = { crop: {}, view: {}, inserts: [], ...d };
      for (const w of data.walls) w.pieces = w.pieces && w.pieces.length ? w.pieces : [w.corners];
      Snap.build(Snap.fromPlan(data, heldBySketch));
      const ids = new Set([...data.walls, ...data.floors, ...(data.roofs || []), ...data.inserts, ...(data.markers || []), ...(data.sections || []), ...(data.grids || []), ...(data.dimensions || []), ...(data.spots || []), ...(data.detail_lines || []), ...(data.regions || []), ...(data.text_notes || []), ...(data.site_points ? [data.site_points.base, data.site_points.survey].filter(Boolean) : []), ...modelGroups].map((e) => e.id)); sel = new Set([...sel].filter((i) => ids.has(i))); render(); },
    setGroups(list, edit) {
      modelGroups = list || [];
      memberOf = new Map();
      for (const g of modelGroups) for (const m of g.members) memberOf.set(m, g.id);
      groupEdit = edit || null;
      render();
    },
    groupOf: (id) => memberOf.get(id),
    viewData: () => data,
    placeGroupType(t) { groupPlace = t; setTool("groupplace"); },
    setSelection(ids) { sel = new Set(ids); render(); },
    gridOptions: () => ({ shape: gridShape, offset: gridOffset }),
    setGridOptions(o) { if (o.shape) { gridShape = o.shape; draw = null; gridHover = null; } if (o.offset !== undefined) gridOffset = Math.max(0, o.offset); cb.prompt(promptText()); render(); },
    wallOptions: () => ({ shape: wallShape, sides: wallSides }),
    setWallOptions(o) { if (o.shape) { wallShape = o.shape; draw = null; } if (o.sides) wallSides = o.sides; cb.prompt(promptText()); render(); },
    startDim(kind) { setTool("dimension"); Dims.start(kind); },
    startWitness(id) { setTool("dimension"); Dims.startWitness(id); },
    startSection() { setTool("section"); cb.prompt(promptText()); render(); },
    // Split Segment on the selected section: the split point first, then the side that moves and how far
    startSplitSegment(id) { setTool("secsplit"); Sections.startSplit(id); cb.prompt(promptText()); render(); },
    startSpot(kind) { setTool("spot"); Spots.start(kind); },
    spots: () => data.spots || [],
    sections: () => data.sections || [],
    detailLines: () => data.detail_lines || [],
    textNotes: () => data.text_notes || [],
    paperPx: () => ((data.view && data.view.scale) || 100) * cam.s,
    viewScale: () => (data.view && data.view.scale) || 100,
    // the Text tool's options changed (main.js keeps them): its picks start again
    noteOptionsChanged() { Det.clearPicks(); cb.prompt(promptText()); render(); },
    setEditingNote(id) { Det.setEditing(id); },
    startNoteLeader(id, side, arc) { setTool("noteleader"); Det.startLeader(id, side, arc); sel = new Set([id]); cb.prompt(promptText()); render(); },
    canvasRect: () => canvas.getBoundingClientRect(),
    viewId: () => data.view && data.view.id,
    dimensions: () => data.dimensions || [],
    tempGrips2: () => tdimUI.filter((u) => u.type === "promote").map((u) => ({ x: u.x, y: u.y, key: u.dim.key })),
    setTempDims(list) { tdims = list || []; if (!tdims.length) closeTdimInput(); render(); },
    tempDims: () => tdims,
    tempGrips: () => tdimUI.filter((u) => u.type === "grip" || u.type === "label" || u.type === "promote").map((u) => ({ type: u.type, role: u.role, key: u.dim.key, x: u.x, y: u.y })),
    setJoinEnd(list) {
      joinSel = (list || []).map((e) => { const w = data.walls.find((x) => x.id === e.id); const q = w ? (e.end === 0 ? w.start : w.end) : [0, 0]; return { ...e, p: { x: q[0], y: q[1] } }; });
      render();
    },
    toScreenPt(x, y) { return toScreen({ x, y }); },
    get showWorkGrid() { return showWorkGrid; },
    setShowWorkGrid,
    refreshSnap() { if (lastMouse) hover = snap(lastMouse.x, lastMouse.y); render(); },
    get thinLines() { return thin; },
    setThinLines(v) { thin = !!v; render(); },
    selection: () => [...sel],
    isEditTool: () => isEditTool(),
    editOptions: () => ({ ...eo }),
    setEditOptions(o) { eo = { ...eo, ...o }; if (ed) cb.prompt(promptText()); render(); },
    editCentre() { return centreOfRotation(); },
    resetEditCentre() { if (ed) ed.centre = null; render(); },
    /** A radial array by a typed angle, from the options in the ribbon. */
    arrayByAngle(v) {
      if (!ed || tool !== "array" || ed.stage === "pick" || !sel.size) return false;
      const c = centreOfRotation();
      ed.p0 = ed.p0 || { x: c.x + 1000, y: c.y };
      ed.typed = String(v);
      sendArray(ed.p0);
      finishEdit(false);
      return true;
    },
    setSelectOptions(o) { selOpts = { ...selOpts, ...o }; if (lastMouse && tool === "select") { const c = candidates(lastMouse.x, lastMouse.y); selHover = c.length ? c[0] : null; } render(); },
    selectOptions: () => ({ ...selOpts }),
    // category counts of the current selection, for the Filter dialog
    selectionInfo() {
      const by = new Map(elements().map((e) => [e.id, e]));
      const out = {};
      for (const id of sel) { const c = by.get(id)?.category || "Other"; out[c] = (out[c] || []).concat(id); }
      return out;
    },
    isPinned: (id) => elements().some((e) => e.id === id && e.pinned),
    setExtents(left, right) { extents = { left, right }; render(); },
    setPlacement(width, label) { place = { width, label }; placeHover = null; render(); },
    // the component about to be placed: its plan in its own axes, from the core
    setComponent(c) { comp = c ? { parts: c.parts || [], footprint: c.footprint || [], rot: comp ? comp.rot : 0 } : null; cb.prompt(promptText()); render(); },
    rotateComponent() { if (comp) { comp.rot = (comp.rot + 90) % 360; render(); return true; } return false; },
    bounds() {
      const pts = [...data.walls.flatMap((w) => w.corners), ...data.floors.flatMap((f) => f.points), ...(data.roofs || []).flatMap((r) => r.points)];
      if (!pts.length) return null;
      const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
      return [[Math.min(...xs), Math.min(...ys)], [Math.max(...xs), Math.max(...ys)]];
    },
    render,
    key(e) {
      // Detail Line has no Finish: Esc in its select tool with nothing selected leaves it
      if (tool === "sketch" && e.key === "Escape" && Sketch.kind === "detail" && Sketch.idle) { cb.leaveDetail(); return true; }
      if (tool === "sketch") return Sketch.key(e);
      if (tool === "text" && e.key === "Escape" && Det.picking) { Det.clearPicks(); cb.prompt(promptText()); render(); return true; }
      if (tool === "text" && e.key === " " && Det.flipArc()) return true;
      if (tool === "select" && sel.size && !wgrip && !dragging && !down && !e.ctrlKey && !e.metaKey && !e.altKey && ARROWS[e.key]) {
        // arrow keys nudge the selection: one press moves about 8 screen pixels, rounded down to a plain step; Shift moves ten times as far
        const [ax, ay] = ARROWS[e.key], raw = 8 / cam.s, step = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000].filter((v) => v <= raw).pop() || 1, d = e.shiftKey ? step * 10 : step;
        nudge(ax * d, ay * d);
        return true;
      }
      if (tool === "dimension" && Dims.key(e)) return true;
      if (tool === "spot" && Spots.key(e)) return true;
      if (ed && ed.stage === "pick" && e.key === "Enter") { if (sel.size) { ed.stage = "a"; cb.prompt(promptText()); render(); } return true; }
      if (ed && editKey(e)) return true;
      if (ed && e.key === "Tab" && lastMouse) {
        if (tool === "offset") { ed.chain = !ed.chain; cb.prompt(ed.chain ? "Chain on: the whole run of connected walls will be offset." : "Chain off: the one wall will be offset."); editMove(lastMouse.x, lastMouse.y); render(); return true; }
        tabIdx += e.shiftKey ? -1 : 1; if (tabIdx < 0) tabIdx = 99; editMove(lastMouse.x, lastMouse.y); render(); return true;
      }
      if (ed && e.key === "Escape") {
        if (ed.typed) { ed.typed = ""; cb.prompt(promptText()); render(); return true; }
        if (ed.stage === "b" || (tool === "align" && ed.ref) || ed.first || ed.boundary) { ed = newEdit(tool); if (ed.stage === "pick" && sel.size) ed.stage = "a"; cb.prompt(promptText()); render(); return true; }
        setTool("select"); return true;
      }
      if (e.key === "Escape" && wgrip) { wgrip = null; hover = null; render(); return true; }
      if (e.key === "Escape" && Det.cancelDrag()) return true;
      if (e.key === "Escape" && draw && draw.typed) { draw.typed = ""; cb.prompt(promptText()); render(); return true; }
      if (e.key === "Escape") { if (draw) { draw = null; } else if (tool !== "select") { setTool("select"); return true; } else { sel = new Set(); cb.select([]); } cb.prompt(promptText()); render(); return true; }
      if (e.key === "Tab" && tool === "select" && lastMouse) {
        const c = candidates(lastMouse.x, lastMouse.y);
        if (c.length > 1) { tabIdx = (tabIdx + (e.shiftKey ? -1 : 1) + c.length * 4) % c.length; selHover = c[tabIdx]; render(); }
        return true;
      }
      if (tool === "grid" && gridShape === "multi" && draw && e.key === "Enter") { finishGridChain(); return true; }
      if (tool === "grid" && gridShape === "line" && draw && draw.pts.length && hover) {
        if (/^[0-9.]$/.test(e.key)) { draw.typed = (draw.typed || "") + e.key; cb.prompt(`Length ${draw.typed}. Press Enter to place the grid, Esc to clear.`); render(); return true; }
        if (e.key === "Backspace" && draw.typed) { draw.typed = draw.typed.slice(0, -1); render(); return true; }
        if (e.key === "Enter" && draw.typed) {
          const tl = parseFloat(draw.typed), a = draw.pts[0], L = dist(a, hover);
          draw.typed = "";
          if (!isNaN(tl) && tl > 0) {
            const b = L > 1e-6 ? { x: a.x + ((hover.x - a.x) / L) * tl, y: a.y + ((hover.y - a.y) / L) * tl } : { x: a.x + tl, y: a.y };
            const r = (v) => Math.round(v * 1000) / 1000;
            cb.addGrid([{ type: "line", a: { x: r(a.x), y: r(a.y) }, b: { x: r(b.x), y: r(b.y) } }]);
            draw = null; cb.prompt(promptText());
          }
          render();
          return true;
        }
      }
      if (tool === "elevation" && e.key === " ") { markerRot = (markerRotAt(hover || { x: 0, y: 0 }) + 90) % 360; render(); return true; }
      if (tool === "wall" && wallShape === "spline" && draw && e.key === "Enter") {
        if (draw.pts.length >= 2) finishCurveWall(); else { draw = null; cb.prompt(promptText()); render(); }
        return true;
      }
      if (tool === "wall" && wallShape === "line" && draw && draw.pts.length && hover) {
        if (/^[0-9.]$/.test(e.key)) { draw.typed = (draw.typed || "") + e.key; cb.prompt(`Length ${draw.typed}. Press Enter to place the wall, Esc to clear.`); render(); return true; }
        if (e.key === "Backspace" && draw.typed) { draw.typed = draw.typed.slice(0, -1); render(); return true; }
        if (e.key === "Enter" && draw.typed) {
          const tl = parseFloat(draw.typed), a = draw.pts[draw.pts.length - 1], L = dist(a, hover);
          draw.typed = "";
          if (!isNaN(tl) && tl > 0) {
            const b = L > 1e-6 ? { x: a.x + ((hover.x - a.x) / L) * tl, y: a.y + ((hover.y - a.y) / L) * tl } : { x: a.x + tl, y: a.y };
            const r = (v) => Math.round(v * 1000) / 1000;
            cb.addWall([a.x, a.y], [r(b.x), r(b.y)]);
            draw = { pts: [{ x: r(b.x), y: r(b.y) }] };
            cb.prompt(promptText());
          }
          render();
          return true;
        }
      }
      if (e.key === "Tab" && tool === "wall" && hover) { snapCycle += e.shiftKey ? -1 : 1; if (lastMouse) hover = snap(lastMouse.x, lastMouse.y); render(); return true; }
      return false;
    },
    fit() {
      // the elevation markers and project points are part of what the view shows
      const extra = [...(data.markers || []).map((m) => [m.x, m.y]), ...(data.sections || []).flatMap((s) => [s.start, s.end]), ...(data.site_points && data.site_points.base ? [[data.site_points.base.x, data.site_points.base.y]] : [])];
      const pts = [...data.walls.flatMap((w) => w.corners), ...data.floors.flatMap((f) => f.points), ...(data.roofs || []).flatMap((r) => r.points), ...extra];
      if (!pts.length) { cam = { x: 3000, y: 2000, s: Math.min(size().w / 12000, size().h / 8000) || 0.1 }; render(); return; }
      const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
      const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
      cam = { x: (x0 + x1) / 2, y: (y0 + y1) / 2, s: Math.min(size().w / ((x1 - x0) * 1.3 + 2000), size().h / ((y1 - y0) * 1.3 + 2000)) };
      render();
    },
    // test hooks: where a model point appears on screen (relative to the canvas), and the snap under the cursor
    screenOf(x, y) { return toScreen({ x, y }); },
    hoverSnap: () => hover,
    setCamera(x, y, s2) { cam = { x, y, s: s2 }; render(); },
    getCamera() { return { ...cam }; },
  };
})();
