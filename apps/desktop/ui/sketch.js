// Sketch mode, after Edit Boundary / Create Floor Boundary. One session at a time,
// drawn on top of the plan view. Floors and roofs use it now; ceilings and openings reuse it.
// Roof footprints keep a per-edge `attr` ({ defines_slope, slope_deg, overhang_mm }) on each curve entry.
// The geometry (curves, offsets, trims, Pick Walls) is computed by the Rust core through
// sketch_make / sketch_edit / pick_walls / sketch_check. This file collects picks, shows
// previews and keeps the list of curves.
//
// A curve entry is what the core returns: { curve, poly, closed, snaps } plus a local uid.
// `curve` is the stored shape (type line | arc | circle | ellipse | spline).

// makeSketch() gives an independent sketch session with its own host: the project's plan uses
// `Sketch`, the family editor makes its own for sketches of forms, an annotation family one for its lines and regions.
// A host can work in units other than millimetres: `host.unit` sketch units make one unit shown and typed (an
// annotation family sketches at 100 units to the paper millimetre, so the core's tolerances suit it).
// Curves can carry attributes (`attr`): a roof edge's slope, an annotation line's style. begin({ attr }) gives
// the attributes of a new curve.
function makeSketch() {
  const cb = { finish: async () => false, cancel() {}, change() {}, prompt() {}, slope() {} };
  let host = null;
  let S = null;
  let uidSeq = 0, seq = 0;

  const MAGENTA = "#b0309c", ORANGE = "#d4572f";
  const call = (m, p) => Backend.call(m, p);
  const P = (p) => [Math.round(p.x * 1000) / 1000, Math.round(p.y * 1000) / 1000];
  const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);
  const near1 = (a, b) => dist(a, b) < 1;
  // sketch units per unit shown; lengths show in whole millimetres, or to 0.01 of a paper millimetre
  const U = () => (host && host.unit) || 1;
  const fmt = (v) => (U() === 1 ? String(Math.round(v)) : String(Math.round((v / U()) * 100) / 100));
  const typedLen = () => parseFloat(S.typed) * U();

  // ---- tool tables ---------------------------------------------------------------
  // n: picks a drawing tool needs (0 = open ended). core: the sketch_make tool name.
  const DRAW = [
    { id: "pick_walls", label: "Pick Walls", ico: "PW" },
    { id: "pick_lines", label: "Pick Lines", ico: "PL" },
    { id: "line", label: "Line", ico: "Ln", n: 2, core: "line" },
    { id: "rectangle", label: "Rectangle", ico: "Rc", n: 2, core: "rectangle" },
    { id: "polygon_in", label: "Inscribed Polygon", ico: "Pi", n: 2, core: "polygon_in" },
    { id: "polygon_out", label: "Circumscribed Polygon", ico: "Po", n: 2, core: "polygon_out" },
    { id: "circle", label: "Circle", ico: "Ci", n: 2, core: "circle" },
    { id: "arc_3pt", label: "Start-End-Radius Arc", ico: "Ar", n: 3, core: "arc_3pt" },
    { id: "arc_centre", label: "Center-ends Arc", ico: "Ac", n: 3, core: "arc_centre" },
    { id: "arc_tangent", label: "Tangent End Arc", ico: "At", n: 2, core: "arc_tangent" },
    { id: "fillet", label: "Fillet Arc", ico: "Fa" },
    { id: "points", label: "Polygon by Points", ico: "Pp", n: 0, core: "polyline" },
    { id: "spline", label: "Spline", ico: "Sp", n: 0, core: "spline" },
    { id: "ellipse", label: "Ellipse", ico: "El", n: 3, core: "ellipse" },
    { id: "partial_ellipse", label: "Partial Ellipse", ico: "Pe", n: 5, core: "partial_ellipse" },
  ];
  const MODIFY = [
    { id: "select", label: "Select", ico: "Se" },
    { id: "move", label: "Move", ico: "Mv" },
    { id: "copy", label: "Copy", ico: "Cp" },
    { id: "rotate", label: "Rotate", ico: "Ro" },
    { id: "mirror", label: "Mirror", ico: "Mi" },
    { id: "align", label: "Align", ico: "Al" },
    { id: "offset", label: "Offset", ico: "Of" },
    { id: "trim_corner", label: "Trim/Extend to Corner", ico: "Tc" },
    { id: "trim_single", label: "Trim/Extend Single Element", ico: "Ts" },
    { id: "split", label: "Split Element", ico: "Sl" },
    { id: "delete", label: "Delete", ico: "De" },
  ];
  const ANNOTATE = [
    { id: "slope", label: "Slope Arrow", ico: "Sa", n: 2 },
    { id: "span", label: "Span Direction", ico: "Sd", n: 2 },
  ];
  const TOOLS = Object.fromEntries([...DRAW, ...MODIFY, ...ANNOTATE].map((t) => [t.id, t]));
  const NEEDS_SELECTION = new Set(["move", "copy", "rotate", "mirror"]);

  const OPTS = {
    chain: true, cornerRadius: 0, sides: 5, radiusOn: false, radius: 1000, ccw: true,
    filletRadius: 500, wallOffset: 0, toCore: false, wallFace: "exterior",
    offsetDist: 300, copy: false, offsetChain: false, multiple: false,
    mirrorMode: "draw", mirrorCopy: true,
    definesSlope: true, slopeDeg: 22.5,
  };

  // ---- session ---------------------------------------------------------------------
  // roof edges carry what the roof does along them; a new edge starts with the options bar settings
  const isRoof = () => !!S && S.kind === "roof";
  const roofAttr = (extra) => ({ defines_slope: !!S.opts.definesSlope, slope_deg: S.opts.slopeDeg, overhang_mm: 0, ...(extra || {}) });
  const wrap = (d, extra) => ({ uid: ++uidSeq, ...d, ...(S && S.attrFn ? { attr: d.attr ? { ...d.attr } : S.attrFn(extra) } : {}) });
  function begin(o) {
    S = {
      kind: o.kind || "floor", id: o.id ?? null, levelId: o.levelId, typeId: o.typeId, title: o.title || "Create Floor Boundary",
      curves: [], slope: o.slope || null, span: o.span || null,
      hist: [], redo: [], tool: "select", pts: [], preview: [], sel: new Set(), mod: {}, typed: "", err: null, hover: null,
      hoverCurve: null, chainWalls: false, wallIds: null, cancelArmed: false, opts: { ...OPTS, ...(o.opts || {}) },
      attrFn: o.attr || (o.kind === "roof" ? roofAttr : null),
      // fade: the rest of the view fades while sketching; ink: the colour of the curves (or a function of the curve)
      fade: o.fade !== false, ink: o.ink || MAGENTA,
      // closed: Finish first checks the curves make closed loops (a path or an axis line is open: the host checks those)
      closed: o.closed !== false,
    };
    S.curves = (o.curves || []).map((d) => wrap(d));
    setTool(o.tool || "line");
  }
  function end() { closeTdInput(); S = null; seq++; }

  const snapshot = () => { S.hist.push({ curves: S.curves.slice(), slope: S.slope, span: S.span }); S.redo = []; S.cancelArmed = false; S.err = null; };
  function undo() {
    if (!S || !S.hist.length) return false;
    S.redo.push({ curves: S.curves.slice(), slope: S.slope, span: S.span });
    const h = S.hist.pop();
    S.curves = h.curves; S.slope = h.slope; S.span = h.span;
    S.sel = new Set([...S.sel].filter((u) => S.curves.some((c) => c.uid === u)));
    S.pts = []; S.preview = []; S.err = null;
    cb.change(); cb.prompt("Undone."); host.render();
    return true;
  }
  function redo() {
    if (!S || !S.redo.length) return false;
    S.hist.push({ curves: S.curves.slice(), slope: S.slope, span: S.span });
    const h = S.redo.pop();
    S.curves = h.curves; S.slope = h.slope; S.span = h.span;
    S.sel = new Set([...S.sel].filter((u) => S.curves.some((c) => c.uid === u)));
    S.pts = []; S.preview = []; S.err = null;
    cb.change(); cb.prompt("Redone."); host.render();
    return true;
  }
  // `inherit` is a list of attr objects, one per new curve (copies of selected edges), or one object for all
  function addCurves(descs, inherit) {
    snapshot();
    S.curves.push(...descs.map((d, i) => wrap(d, Array.isArray(inherit) ? inherit[i] : inherit)));
    cb.change();
  }
  function swap(remove, descs) {
    snapshot();
    const gone = new Set(remove);
    const removed = S.curves.filter((c) => gone.has(c.uid));
    S.curves = S.curves.filter((c) => !gone.has(c.uid));
    // a trimmed, offset or moved edge keeps the settings of the edge it replaces (the nearest one)
    const nearestAttr = (d) => {
      if (!removed.length) return undefined;
      const q = d.poly[Math.floor(d.poly.length / 2)];
      let best = null, bd = Infinity;
      for (const r of removed) { const dd = polyDist({ x: q[0], y: q[1] }, r.poly); if (dd < bd) { bd = dd; best = r; } }
      return best && best.attr;
    };
    const fresh = descs.map((d) => wrap({ ...d, ...(S.attrFn && nearestAttr(d) ? { attr: nearestAttr(d) } : {}) }));
    S.curves.push(...fresh);
    S.sel = new Set([...S.sel].filter((u) => !gone.has(u)));
    cb.change();
    return fresh;
  }

  function setTool(id) {
    if (!S) return;
    S.tool = id; S.pts = []; S.preview = []; S.mod = {}; S.typed = ""; S.wallIds = null; S.cancelArmed = false;
    if (NEEDS_SELECTION.has(id)) S.mod.stage = S.sel.size ? "a" : "pick";
    else S.mod.stage = "a";
    if (id === "rotate" && S.sel.size) S.mod.stage = "centre";
    cb.change();
    cb.prompt(promptText());
    host.render();
  }
  function setOpts(patch) { if (!S) return; Object.assign(S.opts, patch); S.preview = []; cb.change(); cb.prompt(promptText()); refreshPreview(); host.render(); }

  // ---- text ------------------------------------------------------------------------------
  function promptText() {
    if (!S) return "";
    const t = S.tool, n = S.pts.length, st = S.mod.stage;
    const pick = "Select the curves to modify, then press Enter.";
    switch (t) {
      case "select": return "Click a curve to select it. Shift or Ctrl adds or removes. Delete removes the selection. Esc leaves the tool.";
      case "pick_walls": {
        const face = { exterior: "exterior face", interior: "interior face", centre: "centreline" }[S.opts.wallFace] || "exterior face";
        return S.chainWalls ? `Tab chain is on: click to pick the ${face} of the whole run of connected walls. Tab turns it off.` : `Click a wall to take its ${face} as a boundary line. Tab picks the whole run of connected walls.`;
      }
      case "pick_lines": return "Click an edge of a wall, floor or other element to use it as a boundary line.";
      case "line": return n ? "Click the end point, or type a length and press Enter. Esc ends the chain." : "Click the start point.";
      case "rectangle": return n ? "Click the opposite corner." : "Click the first corner.";
      case "polygon_in": return n ? "Click a corner of the polygon." : "Click the centre.";
      case "polygon_out": return n ? "Click the middle of one side." : "Click the centre.";
      case "circle": return n ? "Click a point on the circle, or type a radius and press Enter." : "Click the centre.";
      case "arc_3pt": return n === 0 ? "Click the start point." : n === 1 ? "Click the end point." : S.opts.radiusOn ? "Click the side the arc should bulge to." : "Click a point on the arc.";
      case "arc_centre": return n === 0 ? "Click the centre." : n === 1 ? "Click the start point (sets the radius)." : "Click the end point (sets the end angle).";
      case "arc_tangent": return n === 0 ? "Click the end of an existing curve to start from." : "Click the end point of the arc.";
      case "fillet": return st === "a" ? "Click the first line, on the part to keep." : "Click the second line, on the part to keep.";
      case "points": return n ? (n >= 3 ? "Click the next point. Enter or double-click closes the outline. Click the first point to close it." : "Click the next point.") : "Click the first corner of the outline.";
      case "spline": return n ? "Click the next point. Enter or double-click finishes. Click the first point to close it." : "Click the first point.";
      case "ellipse": return ["Click the centre.", "Click the end of the first axis.", "Click the end of the second axis."][n] || "";
      case "partial_ellipse": return ["Click the centre.", "Click the end of the first axis.", "Click the end of the second axis.", "Click where the arc starts.", "Click where the arc ends."][n] || "";
      case "slope": return n ? "Click the head of the slope arrow." : "Click the tail of the slope arrow. It replaces the one there.";
      case "span": return n ? "Click the other end of the span direction." : "Click one end of the span direction.";
      case "move": case "copy":
        return st === "pick" ? pick : st === "a" ? `Click the point to ${t} from.` : "Click the point to move to, or type a distance and press Enter.";
      case "rotate":
        return st === "pick" ? pick : st === "centre" ? "Click the centre of rotation." : st === "a" ? "Click the start ray, or type an angle and press Enter." : "Click the end ray.";
      case "mirror":
        return st === "pick" ? pick : S.opts.mirrorMode === "pick" ? "Click a line to mirror about." : st === "a" ? "Click the first point of the axis." : "Click the second point of the axis.";
      case "offset": return S.opts.offsetChain ? "Tab chain is on: click to offset the whole run of connected lines. Tab turns it off." : "Move the cursor to the side to offset to and click the line. Tab offsets the whole run of connected lines.";
      case "align": return st === "a" ? "Click the line to align to: a sketch line or any line in the plan. Tab cycles overlapping lines." : "Click the sketch line to move onto it. Ctrl keeps the same reference.";
      case "trim_corner": return st === "a" ? "Click the first line, on the part to keep." : "Click the second line, on the part to keep.";
      case "trim_single": return st === "a" ? "Click the line to trim or extend to." : "Click the line to trim or extend, on the part to keep.";
      case "split": return "Click a line or arc where it should be split.";
      case "delete": return "Click a curve to delete it.";
      default: return "";
    }
  }

  // ---- hit testing ----------------------------------------------------------------------
  function segDist(p, a, b) {
    const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((p.x - a[0]) * dx + (p.y - a[1]) * dy) / l2)) : 0;
    return Math.hypot(p.x - (a[0] + t * dx), p.y - (a[1] + t * dy));
  }
  function polyDist(p, poly) {
    let d = Infinity;
    for (let i = 0; i + 1 < poly.length; i++) d = Math.min(d, segDist(p, poly[i], poly[i + 1]));
    return d;
  }
  function curveAt(w, exclude) {
    const tol = 8 / host.scale();
    let best = null, bd = tol;
    for (const c of S.curves) {
      if (exclude && exclude.has(c.uid)) continue;
      const d = polyDist(w, c.poly);
      if (d <= bd) { bd = d; best = c; }
    }
    return best;
  }
  const byUid = (u) => S.curves.find((c) => c.uid === u);

  // ---- snapping data for the plan ---------------------------------------------------------
  function segs() {
    if (!S) return [];
    const out = [], held = gripHeld();
    // a straight line is marked 5 (it has extensions); a curve's pieces 2 (they do not). Their ends come with points().
    for (const c of S.curves) { if (held.has(c.uid)) continue; const f = isLine(c) ? 5 : 2; for (let i = 0; i + 1 < c.poly.length; i++) out.push([c.poly[i][0], c.poly[i][1], c.poly[i + 1][0], c.poly[i + 1][1], f]); }
    for (let i = 0; i + 1 < S.pts.length; i++) out.push([S.pts[i].x, S.pts[i].y, S.pts[i + 1].x, S.pts[i + 1].y]);
    return out;
  }
  // The points extension snaps line up with horizontally and vertically: the ends of the sketch's lines and curves
  // (for a line whose end is being dragged, its other end), the grip being dragged where it started, the picks so far.
  function trackPoints() {
    if (!S) return [];
    const out = [], held = gripHeld();
    for (const c of S.curves) {
      const n = c.poly.length;
      if (!n) continue;
      if (held.has(c.uid)) {
        const m = S.grip.g.members.find((q) => q.uid === c.uid), o = m && (m.end === "a" ? c.curve.b : c.curve.a);
        if (o) out.push({ x: o.x, y: o.y });
        continue;
      }
      out.push({ x: c.poly[0][0], y: c.poly[0][1] });
      if (!c.closed) out.push({ x: c.poly[n - 1][0], y: c.poly[n - 1][1] });
    }
    if (S.grip && S.grip.moved) out.push({ x: S.grip.g.x, y: S.grip.g.y });
    for (const q of S.pts) out.push({ x: q.x, y: q.y });
    return out;
  }
  function points() {
    if (!S) return [];
    const out = [], held = gripHeld();
    for (const c of S.curves) { if (held.has(c.uid)) continue; for (const q of c.snaps || []) out.push(q); }
    for (const q of S.pts) out.push({ x: q.x, y: q.y, kind: "endpoint" });
    return out;
  }
  // while one end grip is dragged the other end of that line is the point angles and lengths are measured from
  const last = () => {
    if (!S) return null;
    if (S.grip && S.grip.moved && S.grip.g.members.length === 1) { const m = S.grip.g.members[0], c = byUid(m.uid); return c ? (m.end === "a" ? c.curve.b : c.curve.a) : null; }
    return S.pts.length ? S.pts[S.pts.length - 1] : null;
  };

  // ---- the points a drawing tool will use ----------------------------------------------------
  // Tangent End Arc starts at the end of a curve and leaves along it.
  function tangentFrom(start) {
    for (const c of S.curves) {
      const poly = c.poly, n = poly.length;
      if (c.closed || n < 2) continue;
      const a = { x: poly[0][0], y: poly[0][1] }, b = { x: poly[n - 1][0], y: poly[n - 1][1] };
      if (near1(a, start)) return { x: start.x + (poly[0][0] - poly[1][0]), y: start.y + (poly[0][1] - poly[1][1]) };
      if (near1(b, start)) return { x: start.x + (poly[n - 1][0] - poly[n - 2][0]), y: start.y + (poly[n - 1][1] - poly[n - 2][1]) };
    }
    return null;
  }
  // the point `len` from `a` in the direction of `toward` (along +x when they coincide)
  function alongTo(a, toward, len) {
    const L = dist(a, toward);
    return L > 1e-6 ? { x: a.x + ((toward.x - a.x) / L) * len, y: a.y + ((toward.y - a.y) / L) * len } : { x: a.x + len, y: a.y };
  }
  function drawPoints(withHover) {
    const t = TOOLS[S.tool];
    let pts = S.pts.slice();
    if (withHover && S.hover) {
      let h = { x: S.hover.x, y: S.hover.y };
      // a typed length or radius sets the distance of the second pick along the cursor direction
      const typed = typedLen();
      if (t.n === 2 && pts.length === 1 && !isNaN(typed) && typed > 0) h = alongTo(pts[0], h, typed);
      pts.push(h);
    }
    if (S.tool === "arc_tangent") {
      if (pts.length < 2) return null;
      const dir = tangentFrom(pts[0]);
      if (!dir) return null;
      return [pts[0], dir, pts[1]];
    }
    return pts;
  }
  function makeOptions() {
    const o = S.opts;
    return { radius: (S.tool === "arc_3pt" ? o.radius : o.cornerRadius) * U(), sides: o.sides, ccw: o.ccw, closed: false };
  }
  function coreTool() {
    if (S.tool === "arc_3pt" && S.opts.radiusOn) return "arc_ser";
    return TOOLS[S.tool].core;
  }

  // ---- preview -------------------------------------------------------------------------------
  async function refreshPreview() {
    if (!S || !S.hover) return;
    const my = ++seq;
    const t = TOOLS[S.tool];
    let curves = [];
    try {
      if (t.core) {
        const pts = drawPoints(true);
        if (pts && (t.n === 0 ? pts.length >= 2 : S.pts.length === t.n - 1)) {
          const r = await call("sketch_make", { tool: coreTool(), points: pts.map(P), options: makeOptions() });
          curves = r.curves;
        }
      } else if (S.tool === "pick_walls") {
        const w = S.hoverWall;
        if (w) {
          const ids = S.chainWalls ? host.wallChain(w.id) : [w.id];
          S.wallIds = ids;
          const r = await call("pick_walls", { ids, pick: P(S.cursor || S.hover), offset: S.opts.wallOffset * U(), to_core: S.opts.toCore, face: S.opts.wallFace });
          curves = r.curves;
        } else S.wallIds = null;
      } else if (S.tool === "offset" && S.hoverCurve) {
        curves = await offsetCurves(byUid(S.hoverCurve), S.cursor || S.hover);
      } else if (["move", "copy"].includes(S.tool) && S.mod.stage === "b") {
        const d = moveDelta();
        curves = await transformSel({ kind: "move", dx: d.x, dy: d.y });
      } else if (S.tool === "rotate" && S.mod.stage === "b") {
        curves = await transformSel({ kind: "rotate", cx: S.mod.centre.x, cy: S.mod.centre.y, angle_deg: rotateAngle() });
      } else if (S.tool === "mirror" && S.mod.stage === "b" && S.opts.mirrorMode === "draw") {
        if (dist(S.mod.a, S.hover) > 1) curves = await transformSel({ kind: "mirror", a: P(S.mod.a), b: P(S.hover) });
      }
    } catch { curves = []; }
    if (my === seq) { S.preview = curves; host.render(); }
  }
  function moveDelta() {
    const typed = typedLen();
    let v = { x: S.hover.x - S.mod.a.x, y: S.hover.y - S.mod.a.y };
    if (!isNaN(typed)) { const l = Math.hypot(v.x, v.y); v = l > 1e-9 ? { x: (v.x / l) * typed, y: (v.y / l) * typed } : { x: typed, y: 0 }; }
    return v;
  }
  function rotateAngle() {
    const typed = parseFloat(S.typed);
    if (!isNaN(typed)) return typed;
    const c = S.mod.centre;
    let a = (Math.atan2(S.hover.y - c.y, S.hover.x - c.x) - Math.atan2(S.mod.a.y - c.y, S.mod.a.x - c.x)) * 180 / Math.PI;
    a = ((a + 540) % 360) - 180;
    return Math.round(a * 100) / 100;
  }
  async function transformSel(p) {
    const curves = [...S.sel].map((u) => byUid(u)).filter(Boolean).map((c) => c.curve);
    if (!curves.length) return [];
    const r = await call("sketch_edit", { op: "transform", curves, ...(p.kind === "mirror" ? { kind: "mirror", a: p.a, b: p.b } : p) });
    return r.curves;
  }

  // lines joined head to tail with the picked one, oriented along one direction of travel
  function chainOf(entry) {
    if (entry.curve.type !== "line") return null;
    const lines = S.curves.filter((c) => c.curve.type === "line");
    const used = new Set([entry.uid]);
    const seg = [{ e: entry, a: entry.curve.a, b: entry.curve.b }];
    const grow = (forward) => {
      for (;;) {
        const tip = forward ? seg[seg.length - 1].b : seg[0].a;
        const nx = lines.filter((l) => !used.has(l.uid) && (near1(l.curve.a, tip) || near1(l.curve.b, tip)));
        if (nx.length !== 1) return;
        const l = nx[0]; used.add(l.uid);
        const startsHere = near1(l.curve.a, tip);
        if (forward) seg.push(startsHere ? { e: l, a: l.curve.a, b: l.curve.b } : { e: l, a: l.curve.b, b: l.curve.a });
        else seg.unshift(startsHere ? { e: l, a: l.curve.b, b: l.curve.a } : { e: l, a: l.curve.a, b: l.curve.b });
      }
    };
    grow(true); grow(false);
    return seg;
  }

  async function offsetCurves(entry, towards) {
    if (!entry) return [];
    const dist0 = S.opts.offsetDist * U();
    if (!(dist0 > 0)) return [];
    const chain = S.opts.offsetChain ? chainOf(entry) : null;
    if (chain && chain.length > 1) {
      // the side is taken against the picked line, then restated for the first line of the run
      const me = chain.find((s) => s.e.uid === entry.uid);
      const d = { x: me.b.x - me.a.x, y: me.b.y - me.a.y };
      const side = d.x * (towards.y - me.a.y) - d.y * (towards.x - me.a.x) >= 0 ? 1 : -1;
      const f = chain[0], fd = { x: f.b.x - f.a.x, y: f.b.y - f.a.y }, fl = Math.hypot(fd.x, fd.y) || 1;
      const mid = { x: (f.a.x + f.b.x) / 2 - (fd.y / fl) * side * 10, y: (f.a.y + f.b.y) / 2 + (fd.x / fl) * side * 10 };
      const r = await call("sketch_edit", { op: "offset", chain: chain.map((s) => ({ type: "line", a: s.a, b: s.b })), towards: P(mid), dist: dist0 });
      return r.curves;
    }
    const r = await call("sketch_edit", { op: "offset", curve: entry.curve, towards: P(towards), dist: dist0 });
    return r.curves;
  }

  // Offset takes the line under the cursor; with Tab chain on, the whole run it belongs to
  function updateHoverChain() {
    S.hoverChain = null;
    if (S.tool !== "offset" || !S.opts.offsetChain || !S.hoverCurve) return;
    const ch = chainOf(byUid(S.hoverCurve));
    if (ch && ch.length > 1) S.hoverChain = new Set(ch.map((x) => x.e.uid));
  }

  // ---- Align -------------------------------------------------------------------------------------
  // Lines Align can use as the reference: the sketch's own lines and every line of the plan outside it
  // (wall faces and centrelines, floor and roof edges, openings, grids), nearest first.
  function alignCands(w, sx, sy) {
    const tol = 8 / host.scale();
    const out = [];
    for (const c of S.curves) {
      if (c.curve.type !== "line") continue;
      const d = segDist(w, [c.curve.a.x, c.curve.a.y], [c.curve.b.x, c.curve.b.y]);
      if (d <= tol) out.push({ d, a: { x: c.curve.a.x, y: c.curve.a.y }, b: { x: c.curve.b.x, y: c.curve.b.y }, uid: c.uid });
    }
    for (const e of host.edgesNear(sx, sy, S.id)) {
      const d = segDist(w, [e.a.x, e.a.y], [e.b.x, e.b.y]);
      out.push({ d, a: e.a, b: e.b });
    }
    out.sort((p, q) => p.d - q.d);
    return out;
  }
  function alignHover(w, sx, sy) {
    const c = alignCands(w, sx, sy);
    const key = c.map((x) => `${Math.round(x.a.x)},${Math.round(x.a.y)},${Math.round(x.b.x)},${Math.round(x.b.y)}`).join("|");
    if (key !== S.alignKey) { S.alignKey = key; S.alignTab = 0; }
    S.alignList = c;
    S.alignHover = c.length ? c[((S.alignTab % c.length) + c.length) % c.length] : null;
  }
  // intersection of two infinite lines given as point + direction
  function lineMeet(p1, d1, p2, d2) {
    const den = d1.x * d2.y - d1.y * d2.x;
    if (Math.abs(den) < 1e-9) return null;
    const t = ((p2.x - p1.x) * d2.y - (p2.y - p1.y) * d2.x) / den;
    return { x: p1.x + d1.x * t, y: p1.y + d1.y * t };
  }
  // Move a sketch line onto the reference line. The lines joined to its ends stretch or shrink to meet it.
  async function alignLine(ref, c) {
    const L = c.curve;
    if (L.type !== "line") { cb.prompt("Align moves straight lines. Pick a straight line to move.", true); return false; }
    const unit = (a, b) => { const l = Math.hypot(b.x - a.x, b.y - a.y) || 1; return { x: (b.x - a.x) / l, y: (b.y - a.y) / l }; };
    const rd = unit(ref.a, ref.b), ld = unit(L.a, L.b);
    if (Math.abs(rd.x * ld.y - rd.y * ld.x) > 1e-4) { cb.prompt("Only parallel lines can be aligned.", true); return false; }
    if (ref.uid === c.uid) { cb.prompt("Pick a different line to align to.", true); return false; }
    const n = { x: -rd.y, y: rd.x };
    const d = (ref.a.x - L.a.x) * n.x + (ref.a.y - L.a.y) * n.y;
    if (Math.abs(d) < 1e-6) { cb.prompt("Those lines are already aligned.", true); return false; }
    const ends = { a: { x: L.a.x + n.x * d, y: L.a.y + n.y * d }, b: { x: L.b.x + n.x * d, y: L.b.y + n.y * d } };
    const changed = new Map(); // uid -> { entry, a, b }
    for (const end of ["a", "b"]) {
      for (const o of S.curves) {
        if (o.uid === c.uid || o.curve.type !== "line") continue;
        const oe = near1(o.curve.a, L[end]) ? "a" : near1(o.curve.b, L[end]) ? "b" : null;
        if (!oe) continue;
        const od = unit(o.curve.a, o.curve.b);
        const q = lineMeet(ref.a, rd, o.curve[oe], od);
        if (!q) continue;
        ends[end] = q;
        const cur = changed.get(o.uid) || { entry: o, a: o.curve.a, b: o.curve.b };
        cur[oe] = q;
        changed.set(o.uid, cur);
        break;
      }
    }
    const lines = [{ entry: c, a: ends.a, b: ends.b }, ...changed.values()];
    const made = [];
    for (const ln of lines) {
      const r = await call("sketch_make", { tool: "line", points: [P(ln.a), P(ln.b)] });
      if (!r.curves.length) { cb.prompt("That alignment leaves a line with no length.", true); return false; }
      made.push(...r.curves);
    }
    swap(lines.map((l) => l.entry.uid), made);
    return true;
  }

  // ---- end grips, joins and nudging of straight lines ------------------------------------------------
  // The ends of selected lines are drag grips that change only that line. Ends of selected lines that meet share one
  // grip, which moves both. A line that is moved keeps its direction: its ends slide to the lines they were joined to,
  // and those lines get longer or shorter without turning (the top of a rectangle moved up stretches the sides).
  const isLine = (c) => c && c.curve && c.curve.type === "line";
  const gripHeld = () => (S && S.grip && S.grip.moved ? new Set(S.grip.g.members.map((m) => m.uid)) : new Set());
  function gripGroups() {
    const groups = [];
    for (const c of S.curves) {
      if (!S.sel.has(c.uid) || !isLine(c)) continue;
      for (const end of ["a", "b"]) {
        const q = c.curve[end];
        let g = groups.find((o) => dist(o, q) < 0.5);
        if (!g) { g = { x: q.x, y: q.y, members: [] }; groups.push(g); }
        g.members.push({ uid: c.uid, end });
      }
    }
    return groups;
  }
  // where the line through a, b meets the line through c, d (null when they are within a degree of parallel)
  function meet(a, b, c, d) {
    const r = { x: b.x - a.x, y: b.y - a.y }, t = { x: d.x - c.x, y: d.y - c.y };
    const lr = Math.hypot(r.x, r.y), lt = Math.hypot(t.x, t.y);
    if (lr < 1e-9 || lt < 1e-9 || Math.abs(r.x * t.y - r.y * t.x) / (lr * lt) < 0.017) return null;
    const u = ((c.x - a.x) * t.y - (c.y - a.y) * t.x) / (r.x * t.y - r.y * t.x);
    return { x: a.x + u * r.x, y: a.y + u * r.y };
  }
  const pt = (q) => ({ x: Math.round(q.x * 1000) / 1000, y: Math.round(q.y * 1000) / 1000 });
  async function lineEntry(a, b) {
    const r = await call("sketch_make", { tool: "line", points: [P(a), P(b)] });
    return r.curves[0];
  }
  // After lines `uids` become `out` (their moved copies, same order): their joined ends slide to the lines they met.
  // Returns the lines to use and the changes to the lines that stay, as { uid: { a, b } }.
  function fixJoins(uids, out) {
    const moved = new Set(uids), edits = new Map();
    const fixed = out.map((d, i) => {
      const old = byUid(uids[i]);
      if (!isLine(old) || d.curve.type !== "line") return d;
      const nl = { a: { ...d.curve.a }, b: { ...d.curve.b } };
      for (const end of ["a", "b"]) {
        const P0 = old.curve[end];
        let hit = null;
        for (const n of S.curves) {
          if (moved.has(n.uid) || !isLine(n)) continue;
          const cur = edits.get(n.uid) || { a: n.curve.a, b: n.curve.b };
          for (const ne of ["a", "b"]) {
            if (dist(n.curve[ne], P0) > 0.5) continue;
            const x = meet(d.curve.a, d.curve.b, n.curve.a, n.curve.b);
            if (!x) continue;
            edits.set(n.uid, { ...cur, [ne]: pt(x) });
            hit = hit || x;
          }
        }
        if (!hit) {
          // an end resting on the middle of a line that stays slides along that line
          for (const n of S.curves) {
            if (moved.has(n.uid) || !isLine(n)) continue;
            if (polyDist(P0, n.poly) > 0.5) continue;
            const x = meet(d.curve.a, d.curve.b, n.curve.a, n.curve.b);
            if (x) { hit = x; break; }
          }
        }
        if (hit) nl[end] = pt(hit);
      }
      if (dist(nl.a, nl.b) < 1) return d;
      return { ...d, curve: { ...d.curve, ...nl }, poly: [[nl.a.x, nl.a.y], [nl.b.x, nl.b.y]], fixedLine: true };
    });
    // a line that comes out where it started is not a change
    for (const [uid, e] of [...edits]) { const n = byUid(uid); if (dist(e.a, n.curve.a) < 0.01 && dist(e.b, n.curve.b) < 0.01) edits.delete(uid); }
    return { out: fixed, edits };
  }
  async function rebuildLines(map) {
    const done = new Map();
    for (const [uid, l] of map) done.set(uid, await lineEntry(l.a, l.b));
    return done;
  }
  // the entries of the moved lines the core has drawn again after their ends were joined up
  async function redrawLines(out) {
    const res = [];
    for (const d of out) res.push(d.fixedLine ? { ...(await lineEntry(d.curve.a, d.curve.b)) } : d);
    return res;
  }

  function gripDown(sx, sy) {
    if (!S) return false;
    const box = tdHit(sx, sy);
    if (box) { openTdInput(box); return true; }
    if (S.tool !== "select" || !S.gripBoxes) return false;
    const b = S.gripBoxes.find((q) => Math.hypot(sx - q.sx, sy - q.sy) <= 9);
    if (!b) return false;
    S.grip = { g: b.g, moved: false, sx, sy, cur: { x: b.g.x, y: b.g.y } };
    return true;
  }
  function gripMove(sx, sy, p) {
    if (!S || !S.grip) return;
    if (!S.grip.moved && Math.hypot(sx - S.grip.sx, sy - S.grip.sy) < 4) return;
    S.grip.moved = true;
    S.grip.cur = { x: p.x, y: p.y };
    host.render();
  }
  async function gripUp() {
    const d = S && S.grip;
    if (!S) return;
    S.grip = null;
    if (!d || !d.moved || dist(d.cur, d.g) < 1) { host.render(); return; }
    try {
      const map = new Map();
      for (const m of d.g.members) {
        const c = byUid(m.uid), l = { a: c.curve.a, b: c.curve.b };
        l[m.end] = pt(d.cur);
        if (dist(l.a, l.b) < 1) { cb.prompt("That would leave a line with no length.", true); host.render(); return; }
        map.set(m.uid, l);
      }
      const made = await rebuildLines(map);
      snapshot();
      S.curves = S.curves.map((c) => (made.has(c.uid) ? { ...made.get(c.uid), uid: c.uid, attr: c.attr } : c));
      cb.change(); cb.prompt("Done.");
    } catch (e) { cb.prompt(String(e.message || e), true); }
    host.render();
  }
  const gripPoly = (c) => {
    if (!S.grip || !S.grip.moved) return null;
    const m = S.grip.g.members.find((q) => q.uid === c.uid);
    if (!m) return null;
    const o = m.end === "a" ? c.curve.b : c.curve.a, cur = S.grip.cur;
    return m.end === "a" ? [[cur.x, cur.y], [o.x, o.y]] : [[o.x, o.y], [cur.x, cur.y]];
  };

  // arrow keys nudge the selected lines: about 8 screen pixels per press, rounded down to a plain step; Shift moves ten times as far
  let nudgeQ = Promise.resolve();
  function nudge(ax, ay, shift) {
    const raw = 8 / host.scale(), step = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000].filter((v) => v <= raw).pop() || 1, d = shift ? step * 10 : step;
    nudgeQ = nudgeQ.then(async () => {
      if (!S || !S.sel.size) return;
      try {
        const out = await transformSel({ kind: "move", dx: ax * d, dy: ay * d });
        await applyTransform(out, false);
      } catch (e) { cb.prompt(String(e.message || e), true); }
      host.render();
    });
  }

  // ---- temporary dimensions -------------------------------------------------------------------
  // While drawing: a line's length and its angle from horizontal, a rectangle's two sides, a circle's or polygon's
  // radius, drawn as dimensions that follow the cursor (a typed length shows in the box). With one straight line
  // selected: its length and its distance to the nearest parallel sketch line on each side. Click a value to type a
  // new one: the length moves the line's far end (lines joined there follow it); a distance moves the line.
  const TD_BLUE = "#1d6fb8";
  function tempDims() {
    const out = [];
    if (!S) return out;
    const t = S.tool;
    if (S.pts.length === 1 && S.hover) {
      const a = S.pts[0], b = S.hover, typed = S.typed ? `${S.typed}|` : null;
      if (t === "line" && dist(a, b) > 1e-6) out.push({ a, b, value: dist(a, b), text: typed, offset: 22, angle: true });
      if (t === "rectangle") {
        const x0 = Math.min(a.x, b.x), x1 = Math.max(a.x, b.x), y0 = Math.min(a.y, b.y), y1 = Math.max(a.y, b.y);
        if (x1 - x0 > 1e-6) out.push({ a: { x: x0, y: y1 }, b: { x: x1, y: y1 }, value: x1 - x0, offset: 22, side: { x: 0, y: 1 } });
        if (y1 - y0 > 1e-6) out.push({ a: { x: x1, y: y0 }, b: { x: x1, y: y1 }, value: y1 - y0, offset: 22, side: { x: 1, y: 0 } });
      }
      if (["circle", "polygon_in", "polygon_out"].includes(t) && dist(a, b) > 1e-6) out.push({ a, b, value: dist(a, b), text: typed ? `R ${typed}` : null, prefix: "R ", offset: 0 });
    }
    if (t === "select" && S.sel.size === 1 && !S.grip && !S.pts.length) {
      const c = byUid([...S.sel][0]);
      if (isLine(c)) {
        const a = c.curve.a, b = c.curve.b, L = dist(a, b);
        if (L > 1e-6) {
          const u = { x: (b.x - a.x) / L, y: (b.y - a.y) / L }, nrm = { x: -u.y, y: u.x }, m = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
          // the length stands outside the sketch: on the side away from the middle of everything drawn
          const all = S.curves.flatMap((x) => x.poly);
          const cen = all.length ? { x: all.reduce((t, q) => t + q[0], 0) / all.length, y: all.reduce((t, q) => t + q[1], 0) / all.length } : m;
          const inSide = (cen.x - m.x) * nrm.x + (cen.y - m.y) * nrm.y;
          out.push({ a, b, value: L, offset: 22, edit: "length", uid: c.uid, side: inSide > 0 ? { x: -nrm.x, y: -nrm.y } : nrm });
          const best = { 1: null, [-1]: null };
          for (const n of S.curves) {
            if (n.uid === c.uid || !isLine(n)) continue;
            const ln = dist(n.curve.a, n.curve.b);
            if (ln < 1e-6) continue;
            const un = { x: (n.curve.b.x - n.curve.a.x) / ln, y: (n.curve.b.y - n.curve.a.y) / ln };
            if (Math.abs(u.x * un.y - u.y * un.x) > 0.0087) continue; // not within half a degree of parallel
            const sd = (n.curve.a.x - a.x) * nrm.x + (n.curve.a.y - a.y) * nrm.y;
            if (Math.abs(sd) < 0.5) continue;
            const side = sd > 0 ? 1 : -1;
            if (!best[side] || Math.abs(sd) < Math.abs(best[side])) best[side] = sd;
          }
          for (const side of [1, -1]) {
            const sd = best[side];
            if (sd == null) continue;
            out.push({ a: m, b: { x: m.x + nrm.x * sd, y: m.y + nrm.y * sd }, value: Math.abs(sd), offset: 0, edit: "offset", uid: c.uid, dir: { x: nrm.x * side, y: nrm.y * side } });
          }
        }
      }
    }
    return out;
  }
  function drawTempDims(ctx) {
    S.tdBoxes = [];
    const list = tempDims();
    if (!list.length) return;
    ctx.save();
    ctx.font = "11px system-ui, sans-serif"; ctx.lineWidth = 1; ctx.setLineDash([]);
    for (const d of list) {
      const A = host.toScreen(d.a), B = host.toScreen(d.b);
      const len = Math.hypot(B.x - A.x, B.y - A.y);
      if (len < 2) continue;
      const ux = (B.x - A.x) / len, uy = (B.y - A.y) / len;
      // the dimension stands off to one side (screen left of a to b, or the side asked for), with witness lines back
      let nx = uy, ny = -ux;
      if (d.side) { const sw = host.toScreen({ x: d.a.x + d.side.x, y: d.a.y + d.side.y }); const sx = sw.x - A.x, sy = sw.y - A.y; if (nx * sx + ny * sy < 0) { nx = -nx; ny = -ny; } }
      const off = d.offset || 0;
      const A2 = { x: A.x + nx * off, y: A.y + ny * off }, B2 = { x: B.x + nx * off, y: B.y + ny * off };
      ctx.strokeStyle = TD_BLUE; ctx.fillStyle = TD_BLUE;
      if (off) {
        ctx.globalAlpha = 0.55; ctx.beginPath();
        ctx.moveTo(A.x + nx * 3, A.y + ny * 3); ctx.lineTo(A2.x + nx * 5, A2.y + ny * 5);
        ctx.moveTo(B.x + nx * 3, B.y + ny * 3); ctx.lineTo(B2.x + nx * 5, B2.y + ny * 5);
        ctx.stroke(); ctx.globalAlpha = 1;
      } else ctx.setLineDash([5, 3]);
      ctx.beginPath(); ctx.moveTo(A2.x, A2.y); ctx.lineTo(B2.x, B2.y); ctx.stroke(); ctx.setLineDash([]);
      const tx = (ux - uy) * 0.7071 * 5, ty = (ux + uy) * 0.7071 * 5;
      ctx.beginPath();
      for (const q of [A2, B2]) { ctx.moveTo(q.x - tx, q.y - ty); ctx.lineTo(q.x + tx, q.y + ty); }
      ctx.stroke();
      if (d.angle) {
        // the angle from horizontal, measured the short way round
        let deg = (Math.atan2(d.b.y - d.a.y, d.b.x - d.a.x) * 180) / Math.PI;
        const ref = Math.abs(deg) > 90 ? 180 : 0; // the horizontal on the side the line goes
        const sweep = ref === 0 ? deg : deg > 0 ? deg - 180 : deg + 180;
        const r = 30, a0 = (-ref * Math.PI) / 180, a1 = (-(ref + sweep) * Math.PI) / 180;
        ctx.globalAlpha = 0.55; ctx.setLineDash([3, 3]);
        ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(A.x + Math.cos(a0) * (r + 10), A.y + Math.sin(a0) * (r + 10)); ctx.stroke();
        ctx.setLineDash([]); ctx.globalAlpha = 1;
        ctx.beginPath(); ctx.arc(A.x, A.y, r, a0, a1, sweep > 0); ctx.stroke();
        const am = (a0 + a1) / 2, at = `${Math.abs(sweep).toFixed(1)}\u00b0`, aw = ctx.measureText(at).width + 8;
        const ax = A.x + Math.cos(am) * (r + 16), ay = A.y + Math.sin(am) * (r + 16);
        ctx.fillStyle = "#ffffff"; ctx.fillRect(ax - aw / 2, ay - 8, aw, 16);
        ctx.fillStyle = TD_BLUE; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillText(at, ax, ay + 0.5);
      }
      const text = d.text || `${d.prefix || ""}${fmt(d.value)}`;
      const tw = ctx.measureText(text).width + 12, cx = (A2.x + B2.x) / 2, cy = (A2.y + B2.y) / 2;
      ctx.fillStyle = "#ffffff"; ctx.strokeStyle = TD_BLUE;
      ctx.fillRect(cx - tw / 2, cy - 9, tw, 18); ctx.strokeRect(cx - tw / 2 + 0.5, cy - 8.5, tw, 18);
      ctx.fillStyle = TD_BLUE; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(text, cx, cy + 0.5);
      if (d.edit) S.tdBoxes.push({ x: cx, y: cy, w: tw, h: 18, d });
    }
    ctx.restore();
  }
  function tdHit(sx, sy) {
    return (S && S.tdBoxes || []).find((u) => Math.abs(sx - u.x) <= u.w / 2 && Math.abs(sy - u.y) <= u.h / 2) || null;
  }
  let tdInput = null;
  function closeTdInput() { const i = tdInput; tdInput = null; if (i && i.parentNode) i.parentNode.removeChild(i); }
  function openTdInput(box) {
    closeTdInput();
    const cv = host.canvas;
    if (!cv) return;
    const inp = document.createElement("input");
    const r = cv.getBoundingClientRect();
    inp.type = "text"; inp.id = "sk-tdim-input"; inp.setAttribute("aria-label", "Dimension value");
    inp.value = fmt(box.d.value);
    inp.style.cssText = `position:fixed;left:${r.left + box.x - 38}px;top:${r.top + box.y - 12}px;width:76px;height:24px;z-index:60;text-align:center;font:12px system-ui,sans-serif;border:1.5px solid ${TD_BLUE};border-radius:3px;background:#fff;color:#111;`;
    document.body.appendChild(inp);
    tdInput = inp;
    const d = box.d;
    inp.onkeydown = (e) => {
      e.stopPropagation();
      if (e.key === "Escape") { closeTdInput(); host.render(); }
      if (e.key === "Enter") {
        const v = U() === 1 ? parseLengthMM(inp.value) : parseFloat(inp.value) * U();
        closeTdInput();
        if (!(v > 0)) { cb.prompt(U() === 1 ? "Type a length, such as 2400 or 2.4m." : "Type a length in millimetres, such as 2.5.", true); host.render(); return; }
        applyTdEdit(d, v);
      }
    };
    inp.onblur = () => { if (tdInput === inp) closeTdInput(); };
    setTimeout(() => { inp.focus(); inp.select(); }, 0);
  }
  async function applyTdEdit(d, v) {
    try {
      const c = byUid(d.uid);
      if (!c) return;
      if (d.edit === "length") {
        const a = c.curve.a, b = c.curve.b, L = dist(a, b);
        const nb = pt({ x: a.x + ((b.x - a.x) / L) * v, y: a.y + ((b.y - a.y) / L) * v });
        const map = new Map([[c.uid, { a, b: nb }]]);
        // the lines joined at the end that moves keep hold of it
        for (const n of S.curves) {
          if (n.uid === c.uid || !isLine(n)) continue;
          for (const e of ["a", "b"]) {
            if (dist(n.curve[e], b) >= 0.5) continue;
            const l = map.get(n.uid) || { a: n.curve.a, b: n.curve.b };
            l[e] = nb;
            if (dist(l.a, l.b) < 1) { cb.prompt("That would leave a joined line with no length.", true); return; }
            map.set(n.uid, l);
          }
        }
        const made = await rebuildLines(map);
        snapshot();
        S.curves = S.curves.map((x) => (made.has(x.uid) ? { ...made.get(x.uid), uid: x.uid, attr: x.attr } : x));
        cb.change(); cb.prompt(`Length set to ${fmt(v)}.`);
      } else if (d.edit === "offset") {
        const delta = v - d.value;
        if (Math.abs(delta) < 1e-6) return;
        const out = await transformSel({ kind: "move", dx: -d.dir.x * delta, dy: -d.dir.y * delta });
        await applyTransform(out, false);
      }
    } catch (e) { cb.prompt(String(e.message || e), true); }
    host.render();
  }

  // ---- input ------------------------------------------------------------------------------------
  function move(h, sx, sy) {
    if (!S) return;
    S.hover = h;
    const w = host.toWorld(sx, sy);
    S.cursor = w; // the raw cursor: sides are judged against it, not against the snapped point
    const t = S.tool;
    S.hoverCurve = null; S.hoverWall = null; S.hoverChain = null; S.alignHover = null;
    if (t === "pick_walls") S.hoverWall = host.wallAt(sx, sy);
    else if (t === "align" && S.mod.stage === "a") alignHover(w, sx, sy);
    else if (t === "pick_lines") S.hoverEdge = host.edgeAt(sx, sy, S.id);
    else if (["select", "offset", "split", "delete", "fillet", "trim_corner", "trim_single", "mirror", "align"].includes(t) || S.mod.stage === "pick") {
      const c = curveAt(w);
      S.hoverCurve = c ? c.uid : null;
      updateHoverChain();
    }
    refreshPreview();
    host.render();
  }

  async function click(p, sx, sy, ev) {
    if (!S) return;
    S.err = null; S.cancelArmed = false; S.sx = sx; S.sy = sy;
    const t = S.tool, w = host.toWorld(sx, sy);
    const spec = TOOLS[t];
    // choosing curves for Move, Copy, Rotate and Mirror when none are selected yet
    if (S.mod.stage === "pick") { toggleSelect(curveAt(w), ev, true); return; }

    if (t === "select") { toggleSelect(curveAt(w), ev, false); return; }
    if (t === "pick_walls") {
      if (S.preview.length) { addCurves(S.preview, { overhang_mm: S.opts.wallOffset }); S.preview = []; cb.prompt("Picked. Click another wall, or Finish Edit Mode."); }
      else cb.prompt("Click on a wall.", true);
      host.render();
      return;
    }
    if (t === "pick_lines") {
      const e = host.edgeAt(sx, sy, S.id);
      if (!e) { cb.prompt("Click on an edge.", true); return; }
      const r = await call("sketch_make", { tool: "line", points: [P(e.a), P(e.b)] });
      if (r.curves.length) addCurves(r.curves);
      host.render();
      return;
    }
    if (t === "slope" || t === "span") {
      S.pts.push(p);
      if (S.pts.length === 2) {
        const [a, b] = S.pts; S.pts = [];
        if (dist(a, b) < 1) { cb.prompt("The two points are the same.", true); return; }
        snapshot();
        if (t === "slope") {
          const keep = S.slope && S.slope.spec;
          S.slope = { tail: { x: a.x, y: a.y }, head: { x: b.x, y: b.y }, spec: keep || { by: "slope", tail_offset_mm: 0, percent: 2 }, tail_level: null, head_level: null };
          cb.change();
          cb.slope();
        } else {
          S.span = { a: { x: a.x, y: a.y }, b: { x: b.x, y: b.y } };
          cb.change();
        }
        setTool("select");
        return;
      }
      cb.prompt(promptText()); host.render();
      return;
    }
    if (t === "spline") {
      if (S.pts.length >= 3 && dist(S.pts[0], p) < 10 / host.scale()) { await commitSpline(true); return; }
      S.pts.push(p); cb.prompt(promptText()); host.render();
      return;
    }
    if (t === "points") {
      if (S.pts.length >= 3 && dist(S.pts[0], p) < 10 / host.scale()) { await commitPoints(); return; }
      S.pts.push(p); cb.prompt(promptText()); host.render();
      return;
    }
    if (spec.core) { // fixed number of picks
      const start = S.pts.length === 0;
      S.pts.push(p);
      if (start && S.tool === "line") S.chainStart = p;
      if (S.pts.length >= spec.n) await commitDraw(); else { cb.prompt(promptText()); host.render(); }
      return;
    }
    await modifyClick(p, w, ev);
  }

  function toggleSelect(c, ev, forModify) {
    if (!c) { if (!ev.ctrlKey && !ev.shiftKey && !forModify) S.sel = new Set(); }
    else if (ev.shiftKey) S.sel.delete(c.uid);
    else if (ev.ctrlKey || forModify) { S.sel.has(c.uid) && forModify ? S.sel.delete(c.uid) : S.sel.add(c.uid); }
    else S.sel = new Set([c.uid]);
    cb.change(); host.render();
  }

  async function commitDraw() {
    // a typed length replaces the distance of the second pick
    let use = S.pts.slice();
    const t = TOOLS[S.tool];
    const typed = typedLen();
    if (t.n === 2 && !isNaN(typed) && typed > 0 && S.hover) use[1] = alongTo(use[0], use[1], typed);
    if (S.tool === "arc_tangent") {
      const dir = tangentFrom(use[0]);
      if (!dir) { S.pts = []; cb.prompt("Start from the end of an existing curve.", true); host.render(); return; }
      use = [use[0], dir, use[1]];
    }
    S.pts = []; S.typed = ""; S.preview = [];
    let r;
    try { r = await call("sketch_make", { tool: coreTool(), points: use.map(P), options: makeOptions() }); }
    catch (e) { cb.prompt(String(e.message || e), true); host.render(); return; }
    if (!r.curves.length) { cb.prompt("Those points do not make a shape. Try again.", true); host.render(); return; }
    addCurves(r.curves);
    if (S.tool === "line" && S.opts.chain) {
      const endPt = use[1];
      // a chain that returns to where it began is closed, so it stops
      S.pts = S.chainStart && dist(endPt, S.chainStart) < 1 ? [] : [{ x: endPt.x, y: endPt.y }];
    }
    cb.prompt(promptText());
    host.render();
  }

  // a closed outline of straight lines through the clicked points (replaces the old Floor by Points tool)
  async function commitPoints() {
    const pts = S.pts.slice();
    S.pts = []; S.preview = []; S.typed = "";
    if (pts.length < 3) { cb.prompt("An outline needs at least three points.", true); host.render(); return; }
    try {
      const r = await call("sketch_make", { tool: "polyline", points: pts.map(P), options: { closed: true } });
      if (r.curves.length) addCurves(r.curves);
    } catch (e) { cb.prompt(String(e.message || e), true); }
    cb.prompt(promptText()); host.render();
  }

  async function commitSpline(closed) {
    const pts = S.pts.slice();
    S.pts = []; S.preview = [];
    if (pts.length < 2) { host.render(); return; }
    try {
      const r = await call("sketch_make", { tool: "spline", points: pts.map(P), options: { closed } });
      if (r.curves.length) addCurves(r.curves);
    } catch (e) { cb.prompt(String(e.message || e), true); }
    cb.prompt(promptText()); host.render();
  }

  async function modifyClick(p, w, ev) {
    const t = S.tool, st = S.mod.stage;
    const c = curveAt(w);
    const need = () => { cb.prompt("Click on a curve.", true); };
    try {
      switch (t) {
        case "move": case "copy": {
          if (st === "a") { S.mod.a = p; S.mod.stage = "b"; }
          else {
            const d = moveDelta();
            const out = await transformSel({ kind: "move", dx: d.x, dy: d.y });
            await applyTransform(out, t === "copy");
            if (t === "copy" && S.opts.multiple) { S.mod.stage = "b"; } else { setTool("select"); return; }
          }
          break;
        }
        case "rotate": {
          if (st === "centre") { S.mod.centre = p; S.mod.stage = "a"; }
          else if (st === "a") { S.mod.a = p; S.mod.stage = "b"; }
          else {
            const out = await transformSel({ kind: "rotate", cx: S.mod.centre.x, cy: S.mod.centre.y, angle_deg: rotateAngle() });
            await applyTransform(out, S.opts.copy);
            setTool("select"); return;
          }
          break;
        }
        case "mirror": {
          if (S.opts.mirrorMode === "pick") {
            if (!c || c.curve.type !== "line" || S.sel.has(c.uid)) { cb.prompt("Click a line that is not selected.", true); return; }
            const out = await transformSel({ kind: "mirror", a: [c.curve.a.x, c.curve.a.y], b: [c.curve.b.x, c.curve.b.y] });
            await applyTransform(out, S.opts.mirrorCopy);
            setTool("select"); return;
          }
          if (st === "a") { S.mod.a = p; S.mod.stage = "b"; }
          else {
            if (dist(S.mod.a, p) < 1) { cb.prompt("The axis needs two different points.", true); return; }
            const out = await transformSel({ kind: "mirror", a: P(S.mod.a), b: P(p) });
            await applyTransform(out, S.opts.mirrorCopy);
            setTool("select"); return;
          }
          break;
        }
        case "align": {
          if (st === "a") {
            const list = alignCands(w, S.sx, S.sy);
            const ref = list.length ? list[((S.alignTab % list.length) + list.length) % list.length] : null;
            if (!ref) { cb.prompt("Click a line to align to.", true); return; }
            S.mod = { stage: "b", ref };
            S.alignTab = 0; S.alignKey = "";
            break;
          }
          if (!c) { cb.prompt("Click the sketch line to move.", true); return; }
          const ok = await alignLine(S.mod.ref, c);
          if (!ok) return;
          if (!(S.opts.multiple || ev.ctrlKey)) S.mod = { stage: "a" };
          S.hoverChain = null;
          break;
        }
        case "offset": {
          if (!c) return need();
          const out = await offsetCurves(c, w);
          if (!out.length) { cb.prompt("Nothing to offset. Check the distance.", true); return; }
          const chain = S.opts.offsetChain ? chainOf(c) : null;
          if (S.opts.copy) addCurves(out);
          else swap(chain && chain.length > 1 ? chain.map((s) => s.e.uid) : [c.uid], out);
          break;
        }
        case "trim_corner": {
          if (!c) return need();
          if (st === "a") { S.mod.first = { c, pick: p }; S.mod.stage = "b"; break; }
          if (c.uid === S.mod.first.c.uid) { cb.prompt("Pick a different line.", true); return; }
          const r = await call("sketch_edit", { op: "trim_corner", a: S.mod.first.c.curve, b: c.curve, pick_a: P(S.mod.first.pick), pick_b: P(p) });
          swap([S.mod.first.c.uid, c.uid], r.curves);
          S.mod = { stage: "a" };
          break;
        }
        case "trim_single": {
          if (!c) return need();
          if (st === "a") { S.mod.boundary = c; S.mod.stage = "b"; break; }
          if (c.uid === S.mod.boundary.uid) { cb.prompt("Pick a line other than the boundary.", true); return; }
          const r = await call("sketch_edit", { op: "trim_to", curve: c.curve, boundary: S.mod.boundary.curve, pick: P(p) });
          swap([c.uid], r.curves);
          if (!S.opts.multiple) S.mod = { stage: "a" };
          break;
        }
        case "fillet": {
          if (!c) return need();
          if (st === "a") { S.mod.first = { c, pick: p }; S.mod.stage = "b"; break; }
          if (c.uid === S.mod.first.c.uid) { cb.prompt("Pick a different line.", true); return; }
          const r = await call("sketch_edit", { op: "fillet", a: S.mod.first.c.curve, b: c.curve, pick_a: P(S.mod.first.pick), pick_b: P(p), radius: S.opts.filletRadius * U() });
          swap([S.mod.first.c.uid, c.uid], r.curves);
          S.mod = { stage: "a" };
          break;
        }
        case "split": {
          if (!c) return need();
          const r = await call("sketch_edit", { op: "split", curve: c.curve, at: P(p) });
          swap([c.uid], r.curves);
          break;
        }
        case "delete": {
          if (!c) return need();
          swap([c.uid], []);
          break;
        }
        default: break;
      }
    } catch (e) { cb.prompt(String(e.message || e), true); host.render(); return; }
    S.preview = [];
    cb.prompt(promptText());
    host.render();
  }

  async function applyTransform(out, copy) {
    const uids = [...S.sel];
    const attrs = uids.map((u) => byUid(u)?.attr);
    let edits = null, made = null;
    if (!copy && out.length === uids.length) {
      const f = fixJoins(uids, out);
      out = await redrawLines(f.out);
      made = await rebuildLines(f.edits);
      // a move that leaves every line where it was (along a line that is joined at both ends) changes nothing
      const same = (a, b) => dist(a, b) < 0.01;
      if (!made.size && out.every((d, i) => { const o = byUid(uids[i]); return isLine(o) && d.curve.type === "line" && same(o.curve.a, d.curve.a) && same(o.curve.b, d.curve.b); })) { cb.prompt("Nothing moves: the line is joined at both ends and keeps its place."); return; }
    }
    const fresh = copy ? (addCurves(out, attrs.length === out.length ? attrs : undefined), S.curves.slice(-out.length)) : swap(uids, out);
    // the lines that stay change in the same undo step
    if (made && made.size) S.curves = S.curves.map((c) => (made.has(c.uid) ? { ...made.get(c.uid), uid: c.uid, attr: c.attr } : c));
    S.sel = new Set(fresh.map((c) => c.uid));
    cb.change();
  }

  function dblclick() {
    if (S && S.tool === "spline") { commitSpline(false); return true; }
    if (S && S.tool === "points") { commitPoints(); return true; }
    return false;
  }

  // Returns true when the key was used.
  function key(e) {
    if (!S) return false;
    if (e.key === "Escape") {
      if (S.grip) { S.grip = null; host.render(); return true; }
      if (S.typed) { S.typed = ""; host.render(); return true; }
      if (S.tool === "spline" && S.pts.length >= 2) { commitSpline(false); return true; }
      if (S.pts.length || (S.mod.stage && !["a", "pick"].includes(S.mod.stage)) || S.mod.first || S.mod.boundary || S.mod.ref) { S.pts = []; S.preview = []; S.mod = { stage: NEEDS_SELECTION.has(S.tool) ? (S.sel.size ? "a" : "pick") : "a" }; if (S.tool === "rotate" && S.sel.size) S.mod.stage = "centre"; cb.prompt(promptText()); host.render(); return true; }
      if (S.tool !== "select") { setTool("select"); return true; }
      if (S.sel.size) { S.sel = new Set(); cb.change(); host.render(); return true; }
      return true;
    }
    if (e.key === "Enter") {
      if (S.mod.stage === "pick") {
        if (!S.sel.size) { cb.prompt("Select at least one curve first.", true); return true; }
        S.mod.stage = S.tool === "rotate" ? "centre" : "a"; cb.prompt(promptText()); host.render(); return true;
      }
      if (S.tool === "spline") { commitSpline(false); return true; }
      if (S.tool === "points") { commitPoints(); return true; }
      if (S.typed && S.tool === "line" && S.pts.length === 1 && S.hover) { S.pts.push(S.hover); commitDraw(); return true; }
      if (S.typed && ["circle", "polygon_in", "polygon_out"].includes(S.tool) && S.pts.length === 1 && S.hover) { S.pts.push(S.hover); commitDraw(); return true; }
      if (S.typed && ["move", "copy"].includes(S.tool) && S.mod.stage === "b") { click(S.hover, 0, 0, {}).catch(() => {}); return true; }
      if (S.typed && S.tool === "rotate" && S.mod.stage === "a") { S.mod.a = { x: S.mod.centre.x + 1, y: S.mod.centre.y }; S.mod.stage = "b"; click(S.hover, 0, 0, {}).catch(() => {}); return true; }
      return true;
    }
    if (S.tool === "select" && S.sel.size && !e.ctrlKey && !e.metaKey && !e.altKey && /^Arrow(Left|Right|Up|Down)$/.test(e.key)) {
      nudge(e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0, e.key === "ArrowUp" ? 1 : e.key === "ArrowDown" ? -1 : 0, e.shiftKey);
      return true;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      if (e.key === "Backspace" && S.typed) { S.typed = S.typed.slice(0, -1); refreshPreview(); host.render(); return true; }
      if (S.sel.size) { swap([...S.sel], []); S.sel = new Set(); cb.prompt("Deleted."); host.render(); }
      return true;
    }
    if (e.key === "Tab") {
      if (S.tool === "pick_walls") { S.chainWalls = !S.chainWalls; cb.prompt(promptText()); refreshPreview(); host.render(); return true; }
      if (S.tool === "offset") { setOpts({ offsetChain: !S.opts.offsetChain }); updateHoverChain(); host.render(); return true; }
      if (S.tool === "align" && S.mod.stage === "a") {
        S.alignTab = (S.alignTab || 0) + (e.shiftKey ? -1 : 1);
        const n = (S.alignList || []).length;
        if (n) { S.alignTab = ((S.alignTab % n) + n) % n; S.alignHover = S.alignList[S.alignTab]; }
        host.render(); return true;
      }
      host.cycleSnap(e.shiftKey ? -1 : 1);
      return true;
    }
    if (/^[0-9.]$/.test(e.key) && (S.pts.length || S.mod.stage === "b" || S.mod.stage === "a")) {
      const typedOk = (S.pts.length === 1 && ["line", "circle", "polygon_in", "polygon_out"].includes(S.tool)) || (["move", "copy"].includes(S.tool) && S.mod.stage === "b") || (S.tool === "rotate" && S.mod.stage === "a");
      if (typedOk) { S.typed += e.key; cb.prompt(`Typed ${S.typed}. Press Enter to apply, Esc to clear.`); refreshPreview(); host.render(); return true; }
    }
    return false;
  }

  // Which tools use the snap glyph: the ones that place a point. Picking a line or a wall does not.
  function wantsSnap() {
    if (!S) return false;
    if (S.grip && S.grip.moved) return true; // a dragged end snaps whatever the tool
    const t = S.tool;
    if (["pick_walls", "pick_lines", "fillet", "select", "delete", "offset", "trim_corner", "trim_single", "split", "align"].includes(t)) return false;
    if (NEEDS_SELECTION.has(t) && S.mod.stage === "pick") return false;
    if (t === "mirror" && S.opts.mirrorMode === "pick") return false;
    return true;
  }
  // Drag a box to pick lines: Select, and the pick stage of Move, Copy, Rotate and Mirror
  function canBand() { return !!S && (S.tool === "select" || S.tool === "delete" || (NEEDS_SELECTION.has(S.tool) && S.mod.stage === "pick")); }
  function bandSelect(r, win, ev) {
    if (!S) return;
    const inR = (q) => q[0] >= r.x0 && q[0] <= r.x1 && q[1] >= r.y0 && q[1] <= r.y1;
    const rc = [[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]];
    const ccw = (a, b, c) => (c[1] - a[1]) * (b[0] - a[0]) > (b[1] - a[1]) * (c[0] - a[0]);
    const cross = (a, b, c, d) => ccw(a, c, d) !== ccw(b, c, d) && ccw(a, b, c) !== ccw(a, b, d);
    const hits = S.curves.filter((c) => {
      const ring = c.poly;
      if (!ring || !ring.length) return false;
      if (win) return ring.every(inR);
      if (ring.some(inR)) return true;
      const n = c.closed ? ring.length : ring.length - 1;
      for (let i = 0; i < n; i++) for (let k = 0; k < 4; k++) if (cross(ring[i], ring[(i + 1) % ring.length], rc[k], rc[(k + 1) % 4])) return true;
      return false;
    }).map((c) => c.uid);
    if (ev && ev.shiftKey) hits.forEach((u) => S.sel.delete(u));
    else if (ev && ev.ctrlKey) hits.forEach((u) => S.sel.add(u));
    else S.sel = new Set(hits);
    cb.change(); host.render();
  }
  // Undo walks back through the sketch. When the sketch has no steps left it is abandoned (nothing is lost, it was unchanged)
  // so Undo carries on into the model.
  function undoOrLeave() {
    if (!S) return "none";
    if (S.hist.length) { undo(); return "sketch"; }
    cb.cancel();
    return "left";
  }

  function rightClick() {
    if (!S) return;
    if (S.pts.length || S.mod.first || S.mod.boundary || S.mod.ref) { key({ key: "Escape" }); return; }
    if (S.tool !== "select") setTool("select");
  }

  // ---- finishing ------------------------------------------------------------------------------------
  async function check() {
    if (!S) return null;
    const curves = S.curves.map((c) => c.curve);
    if (!curves.length) { cb.prompt("Draw a closed boundary first.", true); return { ok: false }; }
    let r;
    try { r = await call("sketch_check", { curves }); } catch (e) { cb.prompt(String(e.message || e), true); return { ok: false }; }
    S.err = r.ok ? null : r.error;
    if (!r.ok) cb.prompt(r.error.message, true);
    host.render();
    return r;
  }
  async function finish() {
    if (!S) return false;
    if (!S.curves.length) { cb.prompt("Draw something first.", true); return false; }
    if (S.closed) {
      const r = await check();
      if (!r || !r.ok) return false;
    }
    const out = { kind: S.kind, id: S.id, levelId: S.levelId, typeId: S.typeId, curves: S.curves.map((c) => c.curve), slope: S.slope, span: S.span };
    if (S.attrFn) out.edges = S.curves.map((c) => ({ curve: c.curve, ...c.attr }));
    return cb.finish(out);
  }
  function cancel() {
    if (!S) return false;
    if (S.hist.length && !S.cancelArmed) { S.cancelArmed = true; cb.prompt("There are unsaved changes. Press Cancel Edit Mode again to discard them.", true); return false; }
    cb.cancel();
    return true;
  }

  // Attributes of the selected curves (a roof edge's Defines Slope and slope, an annotation line's style): change them
  // in one undo step, or read them (a value the selected curves do not share reads null)
  function patchSelected(patch) {
    if (!S || !S.attrFn || !S.sel.size) return;
    snapshot();
    S.curves = S.curves.map((c) => (S.sel.has(c.uid) ? { ...c, attr: { ...c.attr, ...patch } } : c));
    cb.change(); host.render();
  }
  function selectedAttr() {
    if (!S || !S.attrFn || !S.sel.size) return null;
    const list = S.curves.filter((c) => S.sel.has(c.uid)).map((c) => c.attr);
    const out = { count: list.length };
    for (const k of Object.keys(list[0])) out[k] = list.every((a) => a[k] === list[0][k]) ? list[0][k] : null;
    return out;
  }

  function setSlope(spec) { if (!S || !S.slope) return; snapshot(); S.slope = { ...S.slope, spec }; cb.change(); host.render(); }
  function removeSlope() { if (!S || !S.slope) return; snapshot(); S.slope = null; cb.change(); host.render(); }
  function removeSpan() { if (!S || !S.span) return; snapshot(); S.span = null; cb.change(); host.render(); }

  // ---- drawing ------------------------------------------------------------------------------------------
  function stroke(ctx, poly, close) {
    ctx.beginPath();
    poly.forEach((q, i) => { const s = host.toScreen({ x: q[0], y: q[1] }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
    if (close) ctx.closePath();
    ctx.stroke();
  }
  function arrow(ctx, a, b, colour, head = 12) {
    const A = host.toScreen(a), B = host.toScreen(b);
    const ang = Math.atan2(B.y - A.y, B.x - A.x);
    ctx.strokeStyle = colour; ctx.fillStyle = colour;
    ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(B.x, B.y);
    ctx.lineTo(B.x - head * Math.cos(ang - 0.4), B.y - head * Math.sin(ang - 0.4));
    ctx.lineTo(B.x - head * Math.cos(ang + 0.4), B.y - head * Math.sin(ang + 0.4));
    ctx.closePath(); ctx.fill();
  }
  const slopeText = (sp) => (sp.spec.by === "slope" ? `${sp.spec.percent}% from ${sp.spec.tail_offset_mm}` : `${sp.spec.tail_offset_mm} to ${sp.spec.head_offset_mm}`);

  // the slope symbol drawn beside a slope-defining line: a small triangle pointing up the slope
  // (toward the middle of the footprint) and the slope beside it
  function slopeSymbol(ctx, c, centre, colour) {
    const poly = c.poly;
    if (poly.length < 2) return;
    // the middle of the curve and its direction there
    const m = Math.floor((poly.length - 1) / 2);
    const a = poly[m], b = poly[m + 1] || poly[m];
    const A = host.toScreen({ x: a[0], y: a[1] }), B = host.toScreen({ x: b[0], y: b[1] });
    const mid = { x: (A.x + B.x) / 2, y: (A.y + B.y) / 2 };
    const L = Math.hypot(B.x - A.x, B.y - A.y) || 1;
    let nx = -(B.y - A.y) / L, ny = (B.x - A.x) / L;
    if (centre) { const C = host.toScreen(centre); if ((C.x - mid.x) * nx + (C.y - mid.y) * ny < 0) { nx = -nx; ny = -ny; } }
    const tx = (B.x - A.x) / L, ty = (B.y - A.y) / L, h = 9;
    ctx.save();
    ctx.strokeStyle = colour; ctx.fillStyle = colour; ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(mid.x - tx * h * 0.7 + nx * 3, mid.y - ty * h * 0.7 + ny * 3);
    ctx.lineTo(mid.x + tx * h * 0.7 + nx * 3, mid.y + ty * h * 0.7 + ny * 3);
    ctx.lineTo(mid.x + nx * (3 + h * 1.3), mid.y + ny * (3 + h * 1.3));
    ctx.closePath(); ctx.stroke();
    host.label(`${Math.round(c.attr.slope_deg * 100) / 100}\u00b0`, { x: mid.x + nx * (3 + h * 2.6), y: mid.y + ny * (3 + h * 2.6) }, colour);
    ctx.restore();
  }

  function draw(ctx) {
    if (!S) return;
    const accent = host.css("--accent");
    const { w, h } = host.size();
    // the rest of the model fades while sketching
    if (S.fade) {
      ctx.save();
      ctx.globalAlpha = 0.55; ctx.fillStyle = host.css("--view-bg"); ctx.fillRect(0, 0, w, h);
      ctx.restore();
    }
    ctx.save();
    ctx.lineJoin = "round"; ctx.lineCap = "round";
    // a roof edge that does not define a slope is a gable end: drawn dashed, without a slope symbol
    const centre = (() => {
      if (!isRoof()) return null;
      const all = S.curves.flatMap((c) => c.poly);
      return all.length ? { x: all.reduce((s, q) => s + q[0], 0) / all.length, y: all.reduce((s, q) => s + q[1], 0) / all.length } : null;
    })();
    for (const c of S.curves) {
      const sel = S.sel.has(c.uid), hot = S.hoverCurve === c.uid || (S.hoverChain && S.hoverChain.has(c.uid)) || (S.alignHover && S.alignHover.uid === c.uid);
      const ink = typeof S.ink === "function" ? S.ink(c) : S.ink;
      ctx.strokeStyle = sel ? accent : hot ? ORANGE : ink;
      ctx.lineWidth = sel ? 3.5 : hot ? 3 : 2;
      const gable = isRoof() && c.attr && !c.attr.defines_slope;
      if (gable) ctx.setLineDash([8, 5]);
      const poly = gripPoly(c) || c.poly;
      stroke(ctx, poly, c.closed);
      if (gable) ctx.setLineDash([]);
      if (isRoof() && c.attr && c.attr.defines_slope) slopeSymbol(ctx, c, centre, sel ? accent : MAGENTA);
      if (!c.closed) {
        ctx.fillStyle = sel ? accent : ink;
        for (const q of [poly[0], poly[poly.length - 1]]) { const s = host.toScreen({ x: q[0], y: q[1] }); ctx.fillRect(s.x - 2.5, s.y - 2.5, 5, 5); }
      }
    }
    // end grips of the selected lines
    S.gripBoxes = [];
    if (S.tool === "select" && S.sel.size) {
      for (const g of gripGroups()) {
        // the groups are worked out afresh each time: the one being dragged is the one where the drag started
        const at = S.grip && S.grip.moved && dist(S.grip.g, g) < 0.5 ? S.grip.cur : g;
        const sp = host.toScreen(at), shared = g.members.length > 1;
        ctx.setLineDash([]); ctx.lineWidth = 2; ctx.strokeStyle = accent; ctx.fillStyle = "#ffffff";
        ctx.beginPath(); ctx.arc(sp.x, sp.y, shared ? 7 : 5.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
        if (shared) { ctx.fillStyle = accent; ctx.beginPath(); ctx.arc(sp.x, sp.y, 3, 0, Math.PI * 2); ctx.fill(); }
        S.gripBoxes.push({ sx: sp.x, sy: sp.y, g });
      }
      if (S.grip && S.grip.moved && S.grip.g.members.length === 1) {
        const m = S.grip.g.members[0], c = byUid(m.uid);
        if (c) { const o = m.end === "a" ? c.curve.b : c.curve.a, cur = S.grip.cur; host.label(fmt(dist(o, cur)), host.toScreen({ x: (o.x + cur.x) / 2, y: (o.y + cur.y) / 2 }), accent); }
      }
    }
    // pick previews
    ctx.setLineDash([6, 4]); ctx.lineWidth = 2;
    ctx.strokeStyle = S.tool === "pick_walls" || S.tool === "pick_lines" ? ORANGE : accent;
    for (const c of S.preview) stroke(ctx, c.poly, c.closed);
    ctx.setLineDash([]);
    if (S.tool === "pick_lines" && S.hoverEdge) { ctx.strokeStyle = ORANGE; ctx.lineWidth = 3; stroke(ctx, [[S.hoverEdge.a.x, S.hoverEdge.a.y], [S.hoverEdge.b.x, S.hoverEdge.b.y]], false); }
    if (S.tool === "pick_walls" && S.hoverWall && S.wallIds) {
      ctx.strokeStyle = ORANGE; ctx.lineWidth = 1.5;
      for (const id of S.wallIds) { const wl = host.wallById(id); if (wl) for (const q of wl.pieces || [wl.corners]) { ctx.beginPath(); q.forEach((pt, i) => { const s = host.toScreen({ x: pt[0], y: pt[1] }); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); }); ctx.closePath(); ctx.stroke(); } }
    }
    // Align: the line under the cursor, then the reference once it is chosen
    if (S.tool === "align") {
      const line = (l, col, wd) => { ctx.strokeStyle = col; ctx.lineWidth = wd; stroke(ctx, [[l.a.x, l.a.y], [l.b.x, l.b.y]], false); };
      if (S.mod.stage === "a" && S.alignHover) line(S.alignHover, ORANGE, 3);
      if (S.mod.ref) line(S.mod.ref, "#3c965a", 2.5);
    }
    // guide for a shape still being picked
    if (S.pts.length && S.hover) {
      ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([3, 3]);
      const g = [...S.pts, S.hover].map((q) => [q.x, q.y]);
      if (!S.preview.length || S.tool === "spline" || S.tool === "points") stroke(ctx, g, false);
      ctx.setLineDash([]);
      ctx.fillStyle = accent;
      for (const q of S.pts) { const s = host.toScreen(q); ctx.fillRect(s.x - 3, s.y - 3, 6, 6); }
    }
    // the transform in progress: the reference line
    if ((["move", "copy"].includes(S.tool) && S.mod.stage === "b") || (S.tool === "rotate" && S.mod.stage === "b") || (S.tool === "mirror" && S.mod.stage === "b" && S.opts.mirrorMode === "draw")) {
      const from = S.tool === "rotate" ? S.mod.centre : S.mod.a;
      if (from && S.hover) {
        ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([3, 3]);
        stroke(ctx, [[from.x, from.y], [S.hover.x, S.hover.y]], false);
        ctx.setLineDash([]);
        if (S.tool === "rotate") host.label(`${rotateAngle().toFixed(1)}°`, host.toScreen(S.hover), accent);
        if (S.tool === "move" || S.tool === "copy") { const d = moveDelta(); host.label(fmt(Math.hypot(d.x, d.y)), host.toScreen(S.hover), accent); }
      }
    }
    // slope arrow and span direction
    if (S.slope) {
      ctx.lineWidth = 2;
      arrow(ctx, S.slope.tail, S.slope.head, MAGENTA);
      const t = host.toScreen(S.slope.tail);
      ctx.beginPath(); ctx.arc(t.x, t.y, 4, 0, Math.PI * 2); ctx.stroke();
      host.label(slopeText(S.slope), host.toScreen({ x: (S.slope.tail.x + S.slope.head.x) / 2, y: (S.slope.tail.y + S.slope.head.y) / 2 }), MAGENTA);
    }
    if (S.span) {
      ctx.lineWidth = 1.5; ctx.strokeStyle = MAGENTA;
      arrow(ctx, S.span.a, S.span.b, MAGENTA, 9);
      arrow(ctx, S.span.b, S.span.a, MAGENTA, 9);
      host.label("Span", host.toScreen({ x: (S.span.a.x + S.span.b.x) / 2, y: (S.span.a.y + S.span.b.y) / 2 }), MAGENTA);
    }
    drawTempDims(ctx);
    // where the check found trouble
    if (S.err && S.err.at) {
      const s = host.toScreen(S.err.at);
      ctx.strokeStyle = "#c0392b"; ctx.lineWidth = 2.5;
      ctx.beginPath(); ctx.arc(s.x, s.y, 14, 0, Math.PI * 2); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(s.x - 9, s.y - 9); ctx.lineTo(s.x + 9, s.y + 9); ctx.moveTo(s.x + 9, s.y - 9); ctx.lineTo(s.x - 9, s.y + 9); ctx.stroke();
    }
    ctx.restore();
  }

  return {
    callbacks: cb,
    attach(h) { host = h; },
    DRAW, MODIFY, ANNOTATE, TOOLS,
    begin, end, setTool, setOpts, finish, cancel, check, undo, redo, key, click, move, dblclick, rightClick, draw,
    setSlope, removeSlope, removeSpan, patchSelected, selectedAttr,
    gripDown, gripMove, gripUp, nudge, trackPoints,
    get gripDragging() { return !!(S && S.grip); },
    // the select tool with nothing selected or in progress: Esc there leaves a session that has no Finish
    get idle() { return !!S && S.tool === "select" && !S.sel.size && !S.pts.length && !S.grip && !S.typed; },
    segs, points, last, promptText, wantsSnap, canBand, bandSelect, undoOrLeave,
    get active() { return !!S; },
    get tool() { return S ? S.tool : null; },
    get opts() { return S ? S.opts : OPTS; },
    get slope() { return S ? S.slope : null; },
    get span() { return S ? S.span : null; },
    get title() { return S ? S.title : ""; },
    get editingId() { return S ? S.id : null; },
    get curveCount() { return S ? S.curves.length : 0; },
    get selectedCount() { return S ? S.sel.size : 0; },
    get changed() { return !!(S && S.hist.length); },
    get canUndo() { return !!(S && S.hist.length); },
    get canRedo() { return !!(S && S.redo.length); },
    get kind() { return S ? S.kind : null; },
    get isRoof() { return isRoof(); },
    // test hook
    state: () => S,
  };
}
const Sketch = makeSketch();
