// Snapping, after the usual Snaps dialog. Object snaps work on the geometry the plan
// actually draws, so a wall's layer corners, joined outlines and material
// boundaries are all snap points, not just its location line. Dimension snaps
// step lengths and angles by increments that get finer as you zoom in.
// Settings last for the session and are shared by every view
// (they are not saved with the project).
const Snap = (() => {
  const DEFAULTS = {
    off: false,
    endpoint: true, midpoint: true, nearest: true, grid: false,
    quadrant: true, intersection: true, center: true, perpendicular: true, tangent: true, point: true,
    lengthOn: true, lengths: [1000, 100, 20, 5],
    angleOn: true, angles: [90, 45, 15, 5, 1],
    // lines drawn within this many degrees of horizontal or vertical go exactly horizontal or vertical
    hvOn: true, hvDeg: 2,
  };
  let s = { ...DEFAULTS };
  try {
    const saved = JSON.parse(localStorage.getItem("openarch.snaps") || "{}");
    // the work plane grid snap used to be on by default; it is off unless the person turns it on again
    if (!saved.gridSnapV2) saved.grid = false;
    Object.assign(s, saved);
  } catch { /* storage can be unavailable */ }
  const save = () => { try { localStorage.setItem("openarch.snaps", JSON.stringify({ ...s, gridSnapV2: true })); } catch { /* ignore */ } };

  // what each kind is called under the cursor, and how it ranks when several are in reach
  const LABEL = { endpoint: "Endpoint", midpoint: "Midpoint", intersection: "Intersection", center: "Center", perpendicular: "Perpendicular", nearest: "Nearest", grid: "Work Plane Grid", quadrant: "Quadrant", tangent: "Tangent" };
  const RANK = { endpoint: 0, intersection: 0, center: 1, quadrant: 1, midpoint: 1, tangent: 2, perpendicular: 2, grid: 3, nearest: 4 };
  const CELL = 1000;

  let segs = [];
  let index = new Map();
  let override = null; // one-shot override: a kind, or "off"

  const key = (cx, cy) => cx * 100003 + cy;
  // whose geometry the index holds: the plan, or the family editor while it sketches
  let owner = "plan";
  function build(list, who = "plan") {
    owner = who;
    segs = list;
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

  /** Segments of a polygon ring given as [[x, y], ...]. */
  function ringSegs(ring, out) {
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i], b = ring[(i + 1) % ring.length];
      if (Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) > 0.5) out.push([a[0], a[1], b[0], b[1]]);
    }
  }

  /** Snap geometry of a plan: everything drawn as an edge, plus wall location lines. */
  function fromPlan(data) {
    const out = [];
    for (const g of data.wall_groups || []) {
      for (const shape of g.outline) for (const ring of shape) ringSegs(ring, out);
      for (const f of g.fills) for (const shape of f.shapes) for (const ring of shape) ringSegs(ring, out);
    }
    for (const w of data.walls) {
      if (!w.layered) for (const q of w.pieces || [w.corners]) ringSegs(q, out);
      out.push([w.start[0], w.start[1], w.end[0], w.end[1], 1]); // 5th item marks a location line
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
  function candidates(w, tol, last, extra, pts) {
    const out = [];
    const near = nearby(w.x, w.y, tol).concat(extra || []);
    const seen = new Set();
    const consider = (x, y, kind) => {
      if (!enabled(kind)) return;
      const d = Math.hypot(x - w.x, y - w.y);
      if (d > tol) return;
      const k = `${kind}|${Math.round(x)}|${Math.round(y)}`;
      if (seen.has(k)) return;
      seen.add(k);
      out.push({ x, y, kind, d });
    };
    // exact points of curves (centres, quadrants, ends); `extra` segments marked 2 are only the
    // flattened shape of a curve, good for nearest, perpendicular and intersection but not for ends
    for (const q of pts || []) consider(q.x, q.y, q.kind);
    for (const sg of near) {
      if (sg[4] !== 2) {
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
    out.sort((a, b) => RANK[a.kind] - RANK[b.kind] || a.d - b.d);
    // "nearest" is only worth showing when nothing sharper is in reach
    return out;
  }

  /** Largest increment whose on-screen size stays under 2 mm (96 dpi), as for dimension snaps. */
  function increment(list, pxPerUnit) {
    const sorted = [...list].sort((a, b) => b - a);
    for (const v of sorted) if (v * pxPerUnit < 7.56) return v;
    return sorted[sorted.length - 1];
  }

  /** Whether the line from `last` to `w` is within hvDeg of an axis: { horizontal } or null. */
  function hvAxis(w, last) {
    const dx = w.x - last.x, dy = w.y - last.y;
    if (Math.hypot(dx, dy) < 1e-6) return null;
    const tol = Math.tan(((s.hvDeg > 0 ? s.hvDeg : 2) * Math.PI) / 180);
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
    const tol = 12 / zoom;
    let cands = candidates(w, tol, last, extra, points);
    // favour horizontal and vertical: within hvDeg of 0, 90, 180 or 270 the point is held on that axis.
    // Sharp object snaps still win; "nearest" does not, so a line can run exactly along an axis past a wall.
    const hv = last && !s.off && override !== "off" && s.hvOn !== false && !shift ? hvAxis(w, last) : null;
    if (hv && !(cands.length && RANK[cands[0].kind] < RANK.nearest)) cands = [];
    if (hv && !cands.length) {
      let len = Math.abs(hv.horizontal ? w.x - last.x : w.y - last.y);
      if (s.lengthOn) { const inc = increment(s.lengths, zoom); len = Math.max(inc, Math.round(len / inc) * inc); }
      const sign = hv.horizontal ? Math.sign(w.x - last.x) || 1 : Math.sign(w.y - last.y) || 1;
      const p = hv.horizontal ? { x: last.x + sign * len, y: last.y } : { x: last.x, y: last.y + sign * len };
      return { ...p, kind: "hv", label: hv.horizontal ? "Horizontal" : "Vertical", cands: [], index: 0, horizontal: hv.horizontal };
    }
    if (cands.length) {
      const i = ((cycle % cands.length) + cands.length) % cands.length;
      const c = cands[i];
      return { x: c.x, y: c.y, kind: c.kind, label: LABEL[c.kind], cands, index: i };
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
      if (s.lengthOn) { const inc = increment(s.lengths, zoom); len = Math.max(inc, Math.round(len / inc) * inc); }
      p = { x: last.x + Math.cos(ang) * len, y: last.y + Math.sin(ang) * len };
      return { ...p, kind: "length", label: null, cands: [], index: 0 };
    }
    const inc = s.lengthOn ? increment(s.lengths, zoom) : 1;
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
      default: ctx.moveTo(sx - 5, sy); ctx.lineTo(sx + 5, sy); ctx.moveTo(sx, sy - 5); ctx.lineTo(sx, sy + 5);
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
    build, fromPlan, resolve, glyph,
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
      h("p", { className: "note", textContent: "Temporary overrides apply to one pick: type the two letters shown, SS turns the override off. Tab cycles through the snap points in reach. Shift forces 45 degrees." }),
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
