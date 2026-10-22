// Snapping, after the usual Snaps dialog. Object snaps work on the geometry the plan
// actually draws, so a wall's layer corners, joined outlines and material
// boundaries are all snap points, not just its location line. Dimension snaps
// step lengths and angles by increments that get finer as you zoom in.
// Extensions: a wall's faces and straight lines reach on past their ends, and the points given as `track` (the ends
// of a sketch's lines, a grip where it started, the picks so far) have horizontal and vertical lines through them. The
// cursor snaps to those lines, and harder to where two of them cross or one crosses a line; dashed guides show them.
// Settings last for the session and are shared by every view
// (they are not saved with the project).
const Snap = (() => {
  const DEFAULTS = {
    off: false,
    endpoint: true, midpoint: true, nearest: true, grid: false,
    quadrant: true, intersection: true, center: true, perpendicular: true, tangent: true, point: true, extension: true,
    lengthOn: true, lengths: [1000, 100, 20, 5, 1],
    angleOn: true, angles: [90, 45, 15, 5, 1],
    // lines drawn within this many degrees of horizontal or vertical go exactly horizontal or vertical
    hvOn: true, hvDeg: 2,
  };
  let s = { ...DEFAULTS };
  try {
    const saved = JSON.parse(localStorage.getItem("openarch.snaps") || "{}");
    // the work plane grid snap used to be on by default; it is off unless the person turns it on again
    if (!saved.gridSnapV2) saved.grid = false;
    // the length increments gained 1 mm; settings saved before then get it too
    if (!saved.mm1SnapV3 && Array.isArray(saved.lengths) && !saved.lengths.includes(1)) saved.lengths = [...saved.lengths, 1];
    Object.assign(s, saved);
  } catch { /* storage can be unavailable */ }
  const save = () => { try { localStorage.setItem("openarch.snaps", JSON.stringify({ ...s, gridSnapV2: true, mm1SnapV3: true })); } catch { /* ignore */ } };

  // what each kind is called under the cursor, and how it ranks when several are in reach
  const LABEL = { endpoint: "Endpoint", midpoint: "Midpoint", intersection: "Intersection", center: "Center", perpendicular: "Perpendicular", nearest: "Nearest", grid: "Work Plane Grid", quadrant: "Quadrant", tangent: "Tangent", extension: "Extension", extint: "Intersection" };
  const RANK = { endpoint: 0, intersection: 0, center: 1, quadrant: 1, midpoint: 1, extint: 1.5, tangent: 2, perpendicular: 2, extension: 3, grid: 3, nearest: 4 };
  // A segment's fifth item says what it is: 1 a wall's location line, 2 a piece of a curve (no ends or middle of its
  // own), 3 a face of a wall (or an edge of what an elevation draws), 4 a straight detail line, 5 a straight line of a
  // sketch (its ends come with the sketch's points). Faces and straight lines have extensions.
  const EXTENDS = new Set([3, 4, 5]);
  const EXT_TOL = 8; // pixels: how near an extension line the cursor must come
  const EXT_REACH = 600; // pixels: how far past its end a line reaches, and how far off a point can be to line up with
  const CELL = 1000;

  let segs = [];
  let extSegs = []; // the faces and straight lines in the index, for their extensions
  let index = new Map();
  let override = null; // one-shot override: a kind, or "off"

  const key = (cx, cy) => cx * 100003 + cy;
  // whose geometry the index holds: the plan, or the family editor while it sketches
  let owner = "plan";
  function build(list, who = "plan") {
    owner = who;
    segs = list;
    extSegs = list.filter((sg) => EXTENDS.has(sg[4]));
    index = new Map();
    list.forEach((sg, i) => {
      const x0 = Math.min(sg[0], sg[2]), x1 = Math.max(sg[0], sg[2]), y0 = Math.min(sg[1], sg[3]), y1 = Math.max(sg[1], sg[3]);
      const nx = Math.floor(x1 / CELL) - Math.floor(x0 / CELL) + 1, ny = Math.floor(y1 / CELL) - Math.floor(y0 / CELL) + 1;
      if (nx * ny > 400) { // very long diagonal: index its ends and middle only so it stays reachable
        for (const f of [0, 0.25, 0.5, 0.75, 1]) add(sg[0] + (sg[2] - sg[0]) * f, sg[1] + (sg[3] - sg[1]) * f, i);
        return;
      }
      for (let cx = Math.floor(x0 / CELL); cx <= Math.floor(x1 / CELL); cx++) for (let cy = Math.floor(y0 / CELL); cy <= Math.floor(y1 / CELL); cy++) push(cx, cy, i);
    });
  }
  const push = (cx, cy, i) => { const k = key(cx, cy); (index.get(k) || index.set(k, []).get(k)).push(i); };
  const add = (x, y, i) => push(Math.floor(x / CELL), Math.floor(y / CELL), i);

  /** Segments of a polygon ring given as [[x, y], ...], marked `flag` (a face) when given. */
  function ringSegs(ring, out, flag) {
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i], b = ring[(i + 1) % ring.length];
      if (Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) > 0.5) out.push(flag ? [a[0], a[1], b[0], b[1], flag] : [a[0], a[1], b[0], b[1]]);
    }
  }

  /** Snap geometry of a plan: everything drawn as an edge, plus wall location lines and detail lines. `held(path)`
   * leaves out the detail lines a sketch is holding. */
  function fromPlan(data, held) {
    const out = [];
    for (const g of data.wall_groups || []) {
      for (const shape of g.outline) for (const ring of shape) ringSegs(ring, out, 3);
      for (const f of g.fills) for (const shape of f.shapes) for (const ring of shape) ringSegs(ring, out);
    }
    for (const w of data.walls) {
      if (!w.layered) for (const q of w.pieces || [w.corners]) ringSegs(q, out, 3);
      out.push([w.start[0], w.start[1], w.end[0], w.end[1], 1]); // 5th item marks a location line
    }
    const det = data.detail, ds = (data.view && data.view.scale) || 100;
    if (det) for (const st of det.strokes) {
      if (held && held(st)) continue;
      const q = st.pts.map(([x, y]) => [x * ds, y * ds]), straight = q.length === 2;
      for (let i = 0; i + 1 < q.length; i++) out.push([...q[i], ...q[i + 1], straight ? 4 : 2]);
    }
    for (const f of data.floors || []) ringSegs(f.points, out);
    for (const r of data.roofs || []) {
      for (const ring of r.rings) ringSegs(ring, out);
      for (const [a, b] of r.ridges) out.push([a[0], a[1], b[0], b[1]]);
    }
    for (const i of data.inserts || []) if (i.quad) ringSegs(i.quad, out);
    return out;
  }

  function nearby(x, y, r) {
    const seen = new Set(), out = [];
    for (let cx = Math.floor((x - r) / CELL); cx <= Math.floor((x + r) / CELL); cx++) {
      for (let cy = Math.floor((y - r) / CELL); cy <= Math.floor((y + r) / CELL); cy++) {
        for (const i of index.get(key(cx, cy)) || []) if (!seen.has(i)) { seen.add(i); out.push(segs[i]); }
      }
    }
    return out;
  }

  const enabled = (kind) => {
    if (override === "off") return false;
    if (override) return kind === override;
    return !s.off && s[kind] !== false;
  };

  function segIntersect(a, b) {
    const d1x = a[2] - a[0], d1y = a[3] - a[1], d2x = b[2] - b[0], d2y = b[3] - b[1];
    const den = d1x * d2y - d1y * d2x;
    if (Math.abs(den) < 1e-9) return null;
    const t = ((b[0] - a[0]) * d2y - (b[1] - a[1]) * d2x) / den;
    const u = ((b[0] - a[0]) * d1y - (b[1] - a[1]) * d1x) / den;
    if (t < -1e-9 || t > 1 + 1e-9 || u < -1e-9 || u > 1 + 1e-9) return null;
    return { x: a[0] + d1x * t, y: a[1] + d1y * t };
  }

  /**
   * Snap candidates for the world point `w`, best first. `tol` is in mm, `last` the
   * previous point of the sketch (for perpendicular and dimension snaps), `extra`
   * segments of the sketch in progress, `zoom` pixels per mm.
   */
  function candidates(w, tol, last, extra, pts, tr) {
    const out = [];
    const near = nearby(w.x, w.y, tol).concat(extra || []);
    const seen = new Set();
    const consider = (x, y, kind, guides) => {
      if (!enabled(kind === "extint" ? "extension" : kind)) return;
      const d = Math.hypot(x - w.x, y - w.y);
      if (d > tol) return;
      const k = `${kind}|${Math.round(x)}|${Math.round(y)}`;
      if (seen.has(k)) return;
      seen.add(k);
      out.push(guides ? { x, y, kind, d, guides: guides.map((T) => ({ a: T.from, b: { x, y } })) } : { x, y, kind, d });
    };
    // exact points of curves (centres, quadrants, ends); `extra` segments marked 2 are only the
    // flattened shape of a curve, good for nearest, perpendicular and intersection but not for ends
    for (const q of pts || []) consider(q.x, q.y, q.kind);
    for (const sg of near) {
      if (sg[4] !== 2 && sg[4] !== 5) {
        consider(sg[0], sg[1], "endpoint");
        consider(sg[2], sg[3], "endpoint");
        consider((sg[0] + sg[2]) / 2, (sg[1] + sg[3]) / 2, "midpoint");
      }
      const dx = sg[2] - sg[0], dy = sg[3] - sg[1], l2 = dx * dx + dy * dy;
      if (l2 < 1e-9) continue;
      const t = Math.max(0, Math.min(1, ((w.x - sg[0]) * dx + (w.y - sg[1]) * dy) / l2));
      consider(sg[0] + dx * t, sg[1] + dy * t, "nearest");
      if (last) {
        const tp = ((last.x - sg[0]) * dx + (last.y - sg[1]) * dy) / l2;
        if (tp >= 0 && tp <= 1) consider(sg[0] + dx * tp, sg[1] + dy * tp, "perpendicular");
      }
    }
    if (enabled("intersection") && near.length < 60) {
      for (let i = 0; i < near.length; i++) for (let j = i + 1; j < near.length; j++) {
        const p = segIntersect(near[i], near[j]);
        if (p) consider(p.x, p.y, "intersection");
      }
    }
    // extension lines: the point on each nearest the cursor, where two cross, and where one crosses a line in reach
    // (the lines through the point drawing measures from count only where they cross another)
    for (const T of tr) if (!T.own) { const t = (w.x - T.p.x) * T.d.x + (w.y - T.p.y) * T.d.y; consider(T.p.x + T.d.x * t, T.p.y + T.d.y * t, "extension", [T]); }
    for (let i = 0; i < tr.length; i++) for (let j = i + 1; j < tr.length; j++) {
      if (tr[i].own && tr[j].own) continue;
      const p = crossing(tr[i], tr[j]);
      if (p) consider(p.x, p.y, "extint", [tr[i], tr[j]].filter((T) => !T.own || Math.hypot(p.x - T.from.x, p.y - T.from.y) > 1e-6));
    }
    for (const T of tr) {
      if (T.own) continue;
      for (const sg of near) {
        const p = crossing(T, { p: { x: sg[0], y: sg[1] }, d: { x: sg[2] - sg[0], y: sg[3] - sg[1] } });
        if (!p) continue;
        const dx = sg[2] - sg[0], dy = sg[3] - sg[1], l2 = dx * dx + dy * dy, t = l2 ? ((p.x - sg[0]) * dx + (p.y - sg[1]) * dy) / l2 : -1;
        if (t >= -1e-9 && t <= 1 + 1e-9) consider(p.x, p.y, "extint", [T]);
      }
    }
    out.sort((a, b) => RANK[a.kind] - RANK[b.kind] || a.d - b.d);
    // "nearest" is only worth showing when nothing sharper is in reach
    return out;
  }

  // where two lines { p, d } cross (null when they are within about a degree of parallel)
  function crossing(A, B) {
    const den = A.d.x * B.d.y - A.d.y * B.d.x, la = Math.hypot(A.d.x, A.d.y), lb = Math.hypot(B.d.x, B.d.y);
    if (!la || !lb || Math.abs(den) / (la * lb) < 0.017) return null;
    const t = ((B.p.x - A.p.x) * B.d.y - (B.p.y - A.p.y) * B.d.x) / den;
    return { x: A.p.x + A.d.x * t, y: A.p.y + A.d.y * t };
  }
  /**
   * The extension lines near the cursor `w` (zoom: pixels per unit): each straight face or line in the index or in
   * `extra` carried on past the end nearer the cursor, and horizontal and vertical lines through the `track` points
   * in reach. `last` (the point drawing measures from) adds its own two, marked `own`. Each is { p, d (a unit
   * direction), from (where its guide starts), own }.
   */
  function extensionLines(w, zoom, track, last, extra) {
    const out = [];
    if (!enabled("extension")) return out;
    const tol = EXT_TOL / zoom, reach = EXT_REACH / zoom, keys = new Map();
    // one line per place: collinear faces give the extension from the end nearest the cursor
    const add = (T) => {
      const ang = Math.round(((Math.atan2(T.d.y, T.d.x) + Math.PI) % Math.PI) * 1000), off = Math.round(T.p.x * -T.d.y + T.p.y * T.d.x);
      const k = `${ang}|${off}|${T.own ? 1 : 0}`, d = Math.hypot(T.from.x - w.x, T.from.y - w.y), was = keys.get(k);
      if (was && was.dist <= d) return;
      if (was) out.splice(out.indexOf(was.T), 1, T); else out.push(T);
      keys.set(k, { T, dist: d });
    };
    for (const sg of extSegs.concat((extra || []).filter((x) => EXTENDS.has(x[4])))) {
      const dx = sg[2] - sg[0], dy = sg[3] - sg[1], L = Math.hypot(dx, dy);
      if (L < 1e-6) continue;
      const ux = dx / L, uy = dy / L, t = (w.x - sg[0]) * ux + (w.y - sg[1]) * uy;
      if (t >= 0 && t <= L) continue; // alongside the line itself: Nearest covers that
      if ((t < 0 ? -t : t - L) > reach || Math.abs((w.x - sg[0]) * -uy + (w.y - sg[1]) * ux) > tol) continue;
      const from = t < 0 ? { x: sg[0], y: sg[1] } : { x: sg[2], y: sg[3] };
      add({ p: from, d: { x: ux, y: uy }, from });
    }
    for (const q of track || []) {
      if (Math.hypot(q.x - w.x, q.y - w.y) > reach) continue;
      if (Math.abs(w.y - q.y) <= tol) add({ p: q, d: { x: 1, y: 0 }, from: q });
      if (Math.abs(w.x - q.x) <= tol) add({ p: q, d: { x: 0, y: 1 }, from: q });
    }
    if (last) {
      add({ p: last, d: { x: 1, y: 0 }, from: last, own: true });
      add({ p: last, d: { x: 0, y: 1 }, from: last, own: true });
    }
    return out;
  }

  /** Largest increment whose on-screen size stays under 2 mm (96 dpi), as for dimension snaps. */
  function increment(list, pxPerUnit) {
    const sorted = [...list].sort((a, b) => b - a);
    for (const v of sorted) if (v * pxPerUnit < 7.56) return v;
    return sorted[sorted.length - 1];
  }

  /** Whether the line from `last` to `w` is within hvDeg of an axis: { horizontal } or null. */
  function hvAxis(w, last, deg) {
    const dx = w.x - last.x, dy = w.y - last.y;
    if (Math.hypot(dx, dy) < 1e-6) return null;
    const tol = Math.tan(((deg > 0 ? deg : s.hvDeg > 0 ? s.hvDeg : 2) * Math.PI) / 180);
    if (Math.abs(dy) <= Math.abs(dx) * tol) return { horizontal: true };
    if (Math.abs(dx) <= Math.abs(dy) * tol) return { horizontal: false };
    return null;
  }

  /**
   * Resolve a cursor position. Returns { x, y, kind, label, cands, index } where kind
   * is an object snap, "grid", "length" (dimension snap) or "free".
   */
  function resolve(w, o) {
    const { zoom, last, extra, points, cycle = 0, shift, gridStep } = o;
    // `lengths` replaces the dimension snap increments (an annotation family works in paper millimetres)
    const lengths = o.lengths || s.lengths;
    const tol = 12 / zoom;
    const tr = override === "off" || s.off || (override && override !== "extension") ? [] : extensionLines(w, zoom, o.track, last, extra);
    let cands = candidates(w, tol, last, extra, points, tr);
    // favour horizontal and vertical: within hvDeg of 0, 90, 180 or 270 the point is held on that axis.
    // Sharp object snaps still win; "nearest" does not, so a line can run exactly along an axis past a wall.
    // `hvDeg` widens the window for a line that should go level or plumb readily (a text note's leader)
    const hv = last && !s.off && override !== "off" && s.hvOn !== false && !shift ? hvAxis(w, last, o.hvDeg) : null;
    if (hv && !(cands.length && RANK[cands[0].kind] < RANK.extension)) cands = [];
    if (hv && !cands.length) {
      // an extension line crossing the axis near the cursor holds the point there
      const axis = { p: last, d: hv.horizontal ? { x: 1, y: 0 } : { x: 0, y: 1 } }, onAxis = hv.horizontal ? { x: w.x, y: last.y } : { x: last.x, y: w.y };
      let best = null;
      for (const T of tr) {
        if (T.own) continue;
        const p = crossing(axis, T), d = p ? Math.hypot(p.x - onAxis.x, p.y - onAxis.y) : Infinity;
        if (d <= tol && (!best || d < best.d)) best = { p, d, T };
      }
      if (best) return { x: best.p.x, y: best.p.y, kind: "extint", label: `${hv.horizontal ? "Horizontal" : "Vertical"} and Extension`, cands: [], index: 0, guides: [{ a: best.T.from, b: best.p }] };
      let len = Math.abs(hv.horizontal ? w.x - last.x : w.y - last.y);
      if (s.lengthOn) { const inc = increment(lengths, zoom); len = Math.max(inc, Math.round(len / inc) * inc); }
      const sign = hv.horizontal ? Math.sign(w.x - last.x) || 1 : Math.sign(w.y - last.y) || 1;
      const p = hv.horizontal ? { x: last.x + sign * len, y: last.y } : { x: last.x, y: last.y + sign * len };
      return { ...p, kind: "hv", label: hv.horizontal ? "Horizontal" : "Vertical", cands: [], index: 0, horizontal: hv.horizontal };
    }
    if (cands.length) {
      const i = ((cycle % cands.length) + cands.length) % cands.length;
      const c = cands[i];
      return { x: c.x, y: c.y, kind: c.kind, label: LABEL[c.kind], cands, index: i, guides: c.guides };
    }
    if (enabled("grid") && gridStep) {
      const gx = Math.round(w.x / gridStep) * gridStep, gy = Math.round(w.y / gridStep) * gridStep;
      if (Math.hypot(gx - w.x, gy - w.y) <= tol) return { x: gx, y: gy, kind: "grid", label: LABEL.grid, cands: [], index: 0 };
    }
    let p = { x: w.x, y: w.y };
    if (last && !s.off && override !== "off") {
      const dx = p.x - last.x, dy = p.y - last.y;
      let len = Math.hypot(dx, dy), ang = Math.atan2(dy, dx);
      if (s.angleOn || shift) {
        const inc = shift ? 45 : increment(s.angles, len * zoom * Math.PI / 180);
        const r = (inc * Math.PI) / 180;
        ang = Math.round(ang / r) * r;
      }
      if (s.lengthOn) { const inc = increment(lengths, zoom); len = Math.max(inc, Math.round(len / inc) * inc); }
      p = { x: last.x + Math.cos(ang) * len, y: last.y + Math.sin(ang) * len };
      return { ...p, kind: "length", label: null, cands: [], index: 0 };
    }
    const inc = s.lengthOn ? increment(lengths, zoom) : 1;
    return { x: Math.round(p.x / inc) * inc, y: Math.round(p.y / inc) * inc, kind: "free", label: null, cands: [], index: 0 };
  }

  /** Draw the snap marker for a result at screen position (sx, sy). */
  function glyph(ctx, kind, sx, sy) {
    ctx.save();
    ctx.strokeStyle = "#d4572f"; ctx.lineWidth = 1.6; ctx.setLineDash([]);
    ctx.beginPath();
    switch (kind) {
      case "endpoint": ctx.rect(sx - 5, sy - 5, 10, 10); break;
      case "midpoint": ctx.moveTo(sx - 6, sy + 5); ctx.lineTo(sx + 6, sy + 5); ctx.lineTo(sx, sy - 6); ctx.closePath(); break;
      case "intersection": ctx.moveTo(sx - 6, sy - 6); ctx.lineTo(sx + 6, sy + 6); ctx.moveTo(sx + 6, sy - 6); ctx.lineTo(sx - 6, sy + 6); break;
      case "perpendicular": ctx.moveTo(sx - 6, sy - 6); ctx.lineTo(sx - 6, sy + 6); ctx.lineTo(sx + 6, sy + 6); ctx.moveTo(sx - 6, sy); ctx.lineTo(sx, sy); ctx.lineTo(sx, sy + 6); break;
      case "nearest": ctx.moveTo(sx - 5, sy - 6); ctx.lineTo(sx + 5, sy - 6); ctx.lineTo(sx - 5, sy + 6); ctx.lineTo(sx + 5, sy + 6); ctx.closePath(); break;
      case "center": ctx.arc(sx, sy, 6, 0, Math.PI * 2); break;
      case "quadrant": ctx.moveTo(sx, sy - 6); ctx.lineTo(sx + 6, sy); ctx.lineTo(sx, sy + 6); ctx.lineTo(sx - 6, sy); ctx.closePath(); break;
      case "tangent": ctx.arc(sx, sy - 2, 4, 0, Math.PI * 2); ctx.moveTo(sx - 7, sy + 7); ctx.lineTo(sx + 7, sy + 7); break;
      case "grid": ctx.moveTo(sx - 6, sy); ctx.lineTo(sx + 6, sy); ctx.moveTo(sx, sy - 6); ctx.lineTo(sx, sy + 6); break;
      case "extint": ctx.moveTo(sx - 6, sy - 6); ctx.lineTo(sx + 6, sy + 6); ctx.moveTo(sx + 6, sy - 6); ctx.lineTo(sx - 6, sy + 6); break;
      case "extension": ctx.moveTo(sx - 4, sy - 4); ctx.lineTo(sx + 4, sy + 4); ctx.moveTo(sx + 4, sy - 4); ctx.lineTo(sx - 4, sy + 4); ctx.moveTo(sx - 7, sy); ctx.lineTo(sx + 7, sy); break;
      default: ctx.moveTo(sx - 5, sy); ctx.lineTo(sx + 5, sy); ctx.moveTo(sx, sy - 5); ctx.lineTo(sx, sy + 5);
    }
    ctx.stroke();
    ctx.restore();
  }

  /** The dashed guides of an extension snap: from where each line starts, through the point and a little past it. */
  function drawGuides(ctx, res, toScreen) {
    if (!res || !res.guides || !res.guides.length) return;
    ctx.save();
    ctx.strokeStyle = "#3aa7d8"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
    ctx.beginPath();
    for (const g of res.guides) {
      const a = toScreen(g.a), b = toScreen(g.b), L = Math.hypot(b.x - a.x, b.y - a.y) || 1;
      ctx.moveTo(a.x, a.y); ctx.lineTo(b.x + ((b.x - a.x) / L) * 24, b.y + ((b.y - a.y) / L) * 24);
    }
    ctx.stroke();
    ctx.restore();
  }

  const OVERRIDES = { SE: "endpoint", SM: "midpoint", SN: "nearest", SI: "intersection", SP: "perpendicular", SC: "center", SW: "grid", SO: "off", SS: null };

  return {
    LABEL, DEFAULTS,
    get settings() { return s; },
    update(patch) { s = { ...s, ...patch }; save(); },
    reset() { s = { ...DEFAULTS }; save(); },
    build, fromPlan, resolve, glyph, drawGuides,
    get owner() { return owner; },
    /** The length increment for placing along a line at this zoom: the dimension snap, never finer than a whole mm. */
    lengthStep(zoom) { return s.lengthOn && !s.off && override !== "off" ? Math.max(1, increment(s.lengths, zoom)) : 1; },
    /** Two-letter temporary overrides. They last for one pick. */
    setOverride(code) { if (code in OVERRIDES) { override = OVERRIDES[code]; return true; } return false; },
    clearOverride() { override = null; },
    get override() { return override; },
    isOverride: (code) => code in OVERRIDES,
  };
})();

// ------------------------------------------------------------------ Snaps dialog
function openSnaps() {
  openModal("Snaps", (body, foot, close) => {
    const cur = { ...Snap.settings };
    const h = (tag, props, ...kids) => { const e = document.createElement(tag); Object.assign(e, props || {}); e.append(...kids); return e; };
    const check = (label, key, code) => {
      const i = h("input", { type: "checkbox", checked: cur[key] !== false });
      i.onchange = () => { cur[key] = i.checked; };
      const l = h("label", { className: "snap-row" }, i, ` ${label}`, code ? h("span", { className: "snap-code", textContent: `(${code})` }) : "");
      return { l, i, key };
    };
    const off = check("Snaps Off", "off", "SO"); off.i.checked = !!cur.off; off.i.onchange = () => { cur.off = off.i.checked; };
    const lenOn = check("Length dimension snap increments", "lengthOn");
    const lens = h("input", { value: cur.lengths.join(" ; "), className: "snap-list" });
    const angOn = check("Angular dimension snap increments", "angleOn");
    const hvOn = check("Favour horizontal and vertical (within 2\u00b0)", "hvOn");
    const angs = h("input", { value: cur.angles.map((a) => a + "\u00b0").join(" ; "), className: "snap-list" });
    const rows = [
      check("Endpoints", "endpoint", "SE"), check("Intersections", "intersection", "SI"),
      check("Midpoints", "midpoint", "SM"), check("Centers", "center", "SC"),
      check("Nearest", "nearest", "SN"), check("Perpendicular", "perpendicular", "SP"),
      check("Work Plane Grid", "grid", "SW"), check("Tangents", "tangent", "ST"),
      check("Quadrants", "quadrant", "SQ"), check("Points", "point", "SX"),
      check("Extensions", "extension"),
    ];
    const grid = h("div", { className: "snap-grid" }, ...rows.map((r) => r.l));
    const all = button("Check All", () => rows.forEach((r) => { r.i.checked = true; cur[r.key] = true; }));
    const none = button("Check None", () => rows.forEach((r) => { r.i.checked = false; cur[r.key] = false; }));
    body.append(
      off.l,
      h("fieldset", {}, h("legend", { textContent: "Dimension Snaps" }),
        h("p", { className: "note", textContent: "Snaps adjust as the view is zoomed. The largest value that represents less than 2 mm on screen is used." }),
        lenOn.l, lens, angOn.l, angs, hvOn.l),
      h("fieldset", {}, h("legend", { textContent: "Object Snaps" }), grid, h("div", { className: "snap-btns" }, all, none)),
      h("p", { className: "note", textContent: "Temporary overrides apply to one pick: type the two letters shown, SS turns the override off. Tab cycles through the snap points in reach. Shift forces 45 degrees. Extensions carry wall faces and straight lines on past their ends, and line up with the ends of what is being drawn or edited horizontally and vertically." }),
    );
    const numbers = (text) => text.split(";").map((t) => parseFloat(t)).filter((v) => v > 0);
    foot.append(
      button("Restore Defaults", () => { Snap.reset(); close(); openSnaps(); }),
      button("Cancel", close),
      button("OK", () => {
        const l = numbers(lens.value), a = numbers(angs.value);
        Snap.update({ ...cur, lengths: l.length ? l : Snap.DEFAULTS.lengths, angles: a.length ? a : Snap.DEFAULTS.angles });
        Plan.refreshSnap();
        close();
      }, true),
    );
  });
}
window.openSnaps = openSnaps;
