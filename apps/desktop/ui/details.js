// Detail items in a 2D view: detail lines, filled and masking regions, and text notes with their leaders. Plans,
// elevations and sections each make one with makeDetails(host). The host maps the view's own coordinates (model x and
// y in a plan; along the view and height in an elevation or section) to the screen. The core sends the drawing in
// paper mm round the view's origin (detail.rs); this draws it, picks it, and handles the text notes' grips and the
// Text tool's picks. Typing the text and talking to the core is main.js's part, through host.cb.

// What the views ask of the rest of the app (main.js fills these in): the Text tool's options, and placing and changing
// notes. One set serves every view.
const DetailCallbacks = {
  options: () => ({ leader: "none", left_attach: "top", right_attach: "bottom", halign: "left", valign: "top" }),
  placeNote() {}, patchNote() {}, addLeader() {},
};

// The geometry of a text note's leaders, in paper mm: where a leader leaves the text and the points it runs through.
const NoteGeom = (() => {
  // where a leader leaves the text: on its side of the box, at the height that side's attachment says (the middle of
  // the first line, of the box, or of the last line), the type's leader offset off the box
  function leaderStart(n, L, side) {
    const off = n.text.style.border_offset_mm || 0, att = side === "left" ? n.left_attach : n.right_attach;
    const y = att === "top" ? L.top - L.lh / 2 : att === "bottom" ? L.top - L.h + L.lh / 2 : L.top - L.h / 2;
    return [side === "left" ? L.x0 - off : L.x0 + L.w + off, y];
  }
  // an arc leader with no bend of its own bows out a fifth of its length, to the left going from the text to the end
  const arcBend = (s, e) => [(s[0] + e[0]) / 2 - (e[1] - s[1]) * 0.2, (s[1] + e[1]) / 2 + (e[0] - s[0]) * 0.2];
  // points along the circle through a, b and c, from a to c by way of b; nearly in line, the straight line
  function arcThrough(a, b, c, n = 24) {
    const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    const chord = Math.hypot(c[0] - a[0], c[1] - a[1]);
    if (Math.abs(d) < 1e-9 * Math.max(1, chord * chord)) return [a, c];
    const a2 = a[0] * a[0] + a[1] * a[1], b2 = b[0] * b[0] + b[1] * b[1], c2 = c[0] * c[0] + c[1] * c[1];
    const ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    const uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    const r = Math.hypot(a[0] - ux, a[1] - uy);
    if (r > chord * 1000) return [a, c];
    const TAU = Math.PI * 2, norm = (t) => ((t % TAU) + TAU) % TAU, ang = (p) => Math.atan2(p[1] - uy, p[0] - ux);
    const t0 = ang(a);
    let sweep = norm(ang(c) - t0);
    if (norm(ang(b) - t0) > sweep) sweep -= TAU; // b is the other way round: clockwise
    const out = [];
    for (let i = 0; i <= n; i++) { const t = t0 + (sweep * i) / n; out.push([ux + r * Math.cos(t), uy + r * Math.sin(t)]); }
    return out;
  }
  // the points a leader runs through, from the text to its end: straight (by its elbow if it has one) or an arc
  function leaderPath(n, L, l) {
    const s = leaderStart(n, L, l.side);
    if (l.arc) return arcThrough(s, l.elbow || arcBend(s, l.end), l.end);
    return l.elbow ? [s, l.elbow, l.end] : [s, l.end];
  }
  return { leaderStart, arcBend, arcThrough, leaderPath };
})();

// host: { ctx(), toScreen({ x, y }), toWorld(sx, sy), zoom() (pixels in a view mm), scale() (the view's scale),
//   data() (the view's data), thin() (Thin Lines), css(name), render(), held(path) (a sketch holds it),
//   snap(sx, sy, { last, hvDeg }), options() (the Text tool's: leader, left_attach, right_attach, halign, valign),
//   cb: { placeNote(points, options), patchNote(id, patch), addLeader(id, side, point, arc) } (DetailCallbacks) }
function makeDetails(host) {
  let pts = []; // the Text tool's picks so far (view mm): the leader's end, its elbow, then where it meets the text
  let leaderFor = null; // { id, side, arc } while Add Leader waits for the new leader's end
  let editing = null; // the note being typed in, left out of the drawing meanwhile
  let drag = null; // a grip of the selected note being dragged: { id, g, n (a copy that changes as it goes), L, moved }
  let grips = []; // the selected note's grips on screen
  const ARROW_MM = 2.5, ARROW_HALF = 0.26; // a filled arrowhead, 2.5 mm long on paper and 30 degrees across
  const LEADER_HV = 8; // a leader's segments go horizontal or vertical within this many degrees
  const PICKS = { none: 1, one: 2, two: 3, arc: 2 };

  const data = () => host.data() || {};
  const ds = () => host.scale() || 100;
  const base = () => { const o = host.toScreen({ x: 0, y: 0 }); return { x: o.x, y: o.y, k: ds() * host.zoom() }; };
  const notes = () => data().text_notes || [];
  const paper = (p) => [p.x / ds(), p.y / ds()];
  const viewPt = (q) => ({ x: q[0] * ds(), y: q[1] * ds() });

  // a band either side of a polyline ([[x, y], ...]), for picking thin things
  function band(poly, t) {
    const L = [], R = [];
    for (let i = 0; i < poly.length; i++) {
      const a = poly[Math.max(0, i - 1)], b = poly[Math.min(poly.length - 1, i + 1)];
      const d = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1, nx = -(b[1] - a[1]) / d, ny = (b[0] - a[0]) / d;
      L.push([poly[i][0] + nx * t, poly[i][1] + ny * t]); R.push([poly[i][0] - nx * t, poly[i][1] - ny * t]);
    }
    return [...L, ...R.reverse()];
  }

  const { leaderStart, arcBend, arcThrough, leaderPath } = NoteGeom;

  // A leader on screen (q: from the text to the end) with its arrowhead. The arrow's tip is the end; the line stops
  // inside the arrowhead, so neither its width nor its cap shows past the tip.
  function drawLeader(c, q, k, ink, width) {
    const len = Math.max(6, ARROW_MM * k), e = q[q.length - 1];
    // the point `dist` back along the leader from its end, and the index of the point before it
    const back = (dist) => {
      let acc = 0;
      for (let i = q.length - 1; i > 0; i--) {
        const a = q[i - 1], b = q[i], sl = Math.hypot(b.x - a.x, b.y - a.y);
        if (sl > 0 && acc + sl >= dist) { const t = (dist - acc) / sl; return { x: b.x + (a.x - b.x) * t, y: b.y + (a.y - b.y) * t, i }; }
        acc += sl;
      }
      return { x: q[0].x, y: q[0].y, i: 1 };
    };
    const axis = back(len), stop = back(len * 0.7), ang = Math.atan2(e.y - axis.y, e.x - axis.x);
    c.save();
    c.strokeStyle = ink; c.fillStyle = ink; c.lineWidth = width; c.lineCap = "butt"; c.lineJoin = "round"; c.setLineDash([]);
    c.beginPath(); c.moveTo(q[0].x, q[0].y);
    for (let i = 1; i < stop.i; i++) c.lineTo(q[i].x, q[i].y);
    c.lineTo(stop.x, stop.y); c.stroke();
    c.beginPath(); c.moveTo(e.x, e.y);
    c.lineTo(e.x - len * Math.cos(ang - ARROW_HALF), e.y - len * Math.sin(ang - ARROW_HALF));
    c.lineTo(e.x - len * Math.cos(ang + ARROW_HALF), e.y - len * Math.sin(ang + ARROW_HALF));
    c.closePath(); c.fill();
    c.restore();
  }

  // ---- drawing -----------------------------------------------------------------------------------------------------
  // detail lines and regions are paper sized (line widths, patterns, dashes): drawn round the view's origin at its
  // scale. sel: the selection; hov: what a click would pick.
  function drawItems(sel, hov, accent) {
    const det = data().detail;
    if (!det || (!det.strokes.length && !det.fills.length)) return;
    const c = host.ctx(), b = base(), thin = host.thin(), toS = ([x, y]) => host.toScreen(viewPt([x, y]));
    Ann.paths(c, det, b, null, host.css("--view-bg"), { pats: data().fill_patterns, modelScale: ds(), thin, only: (x) => !host.held(x) });
    // selected regions show as floors do; selected and hovered lines in the accent or the hover colour
    for (const f of det.fills) {
      if (!sel.has(f.region) || host.held(f)) continue;
      c.beginPath();
      for (const r of f.rings) { r.forEach((q, i) => { const s = toS(q); i ? c.lineTo(s.x, s.y) : c.moveTo(s.x, s.y); }); c.closePath(); }
      c.fillStyle = "rgba(46,111,158,0.30)"; c.fill("evenodd"); c.strokeStyle = accent; c.lineWidth = 2; c.stroke();
    }
    Ann.paths(c, { strokes: det.strokes }, b, accent, null, { thin, widen: 1.6, only: (x) => !host.held(x) && x.line != null && sel.has(x.line) });
    if (hov.size) Ann.paths(c, { strokes: det.strokes }, b, "#d4572f", null, { thin, widen: 1.6, only: (x) => !host.held(x) && ((x.line != null && hov.has(x.line) && !sel.has(x.line)) || (x.region != null && hov.has(x.region) && !sel.has(x.region))) });
  }
  // text notes: the text in its box (annotation.js lays it out), the leaders; the selected note's box and grips
  function drawNotes(sel, hov, accent, selecting) {
    grips = [];
    const list = notes();
    if (!list.length) return;
    const c = host.ctx(), b = base(), m = Ann.mapper(b), bg = host.css("--view-bg");
    for (const n0 of list) {
      if (n0.id === editing) continue;
      const n = drag && drag.id === n0.id ? drag.n : n0;
      const col = sel.has(n.id) || (leaderFor && leaderFor.id === n.id) ? accent : hov.has(n.id) ? "#d4572f" : null;
      Ann.labels(c, [n.text], b, col, { bg });
      const L = Ann.layout(n.text), ink = col || Ann.rgb(n.text.style.color), w = host.thin() ? 1 : Math.max(1, n.text.style.line_mm * b.k);
      for (const l of n.leaders) drawLeader(c, leaderPath(n, L, l).map(m), b.k, ink, w);
      if (!(sel.has(n.id) && selecting && sel.size === 1)) continue;
      // the box, the grips at its ends (its width), and on each leader its end and its elbow (or bend); a straight
      // leader's middle grip puts an elbow in
      c.save();
      const a = m([L.x0, L.top]), z = m([L.x0 + L.w, L.top - L.h]);
      c.setLineDash([3, 3]); c.lineWidth = 1; c.strokeStyle = accent; c.strokeRect(a.x, a.y, z.x - a.x, z.y - a.y); c.setLineDash([]);
      const grip = (q, g) => { c.fillStyle = "#ffffff"; c.strokeStyle = accent; c.lineWidth = 1.5; c.beginPath(); c.arc(q.x, q.y, 4.5, 0, Math.PI * 2); c.fill(); c.stroke(); grips.push({ x: q.x, y: q.y, id: n.id, ...g }); };
      grip({ x: a.x, y: (a.y + z.y) / 2 }, { kind: "width", edge: 0 });
      grip({ x: z.x, y: (a.y + z.y) / 2 }, { kind: "width", edge: 1 });
      n.leaders.forEach((l, i) => {
        const s = leaderStart(n, L, l.side);
        grip(m(l.end), { kind: "end", i });
        if (l.elbow || l.arc) grip(m(l.elbow || arcBend(s, l.end)), { kind: "elbow", i });
        else grip(m([(s[0] + l.end[0]) / 2, (s[1] + l.end[1]) / 2]), { kind: "elbow", i });
      });
      c.restore();
    }
  }
  // the Text tool's picks so far, to the cursor
  function preview(hover) {
    if (!pts.length || !hover) return;
    const c = host.ctx(), arc = host.options().leader === "arc";
    let q = [...pts, hover];
    if (arc && q.length === 2) { const a = paper(q[0]), e = paper(q[1]); q = arcThrough(e, arcBend(e, a), a).reverse().map(viewPt); }
    c.save(); c.strokeStyle = host.css("--accent"); c.lineWidth = 1.5; c.setLineDash([5, 4]);
    c.beginPath();
    q.forEach((p, i) => { const s = host.toScreen(p); i ? c.lineTo(s.x, s.y) : c.moveTo(s.x, s.y); });
    c.stroke(); c.restore();
  }

  // ---- picking -----------------------------------------------------------------------------------------------------
  // everything pickable as { id, kind, ring, rings, pinned, category, display } in view mm: detail lines (a band either
  // side), regions (inside), text notes (the box and a band along each leader)
  function elements() {
    const out = [], det = data().detail, t = 4 / host.zoom(), mm = (r) => r.map(([x, y]) => [x * ds(), y * ds()]);
    if (det) {
      const pinned = new Map([...(data().detail_lines || []), ...(data().regions || [])].map((x) => [x.id, x.pinned]));
      for (const s of det.strokes) {
        if (s.line == null || host.held(s)) continue;
        const q = mm(s.closed ? [...s.pts, s.pts[0]] : s.pts);
        out.push({ id: s.line, kind: "detail", ring: band(q, t), pinned: pinned.get(s.line), category: "Lines", display: "normal" });
      }
      for (const f of det.fills) if (!host.held(f)) { const rings = f.rings.map(mm); out.push({ id: f.region, kind: "region", ring: rings[0], rings, pinned: pinned.get(f.region), category: "Detail Items", display: "normal" }); }
    }
    for (const n of notes()) {
      if (n.id === editing) continue;
      const L = Ann.layout(n.text);
      const box = mm([[L.x0, L.top], [L.x0 + L.w, L.top], [L.x0 + L.w, L.top - L.h], [L.x0, L.top - L.h]]);
      out.push({ id: n.id, kind: "note", ring: box, rings: [box, ...n.leaders.map((l) => band(mm(leaderPath(n, L, l)), t))], pinned: n.pinned, category: "Text Notes", display: "normal" });
    }
    return out;
  }

  // ---- the selected note's grips -----------------------------------------------------------------------------------
  function gripDown(sx, sy) {
    const g = grips.find((q) => Math.hypot(q.x - sx, q.y - sy) <= 7);
    if (!g) return false;
    const n = notes().find((x) => x.id === g.id);
    if (!n) return false;
    drag = { id: n.id, g, n: JSON.parse(JSON.stringify(n)), L: Ann.layout(n.text), moved: false };
    return true;
  }
  function gripMove(sx, sy) {
    const d = drag;
    d.moved = true;
    if (d.g.kind === "width") {
      const p = paper(host.toWorld(sx, sy)), L = d.L, t = d.n.text;
      const e0 = d.g.edge === 0 ? Math.min(p[0], L.x0 + L.w - 2) : L.x0, e1 = d.g.edge === 1 ? Math.max(p[0], L.x0 + 2) : L.x0 + L.w;
      t.width_mm = Math.round((e1 - e0) * 10) / 10;
      t.x = t.halign === "right" ? e1 : t.halign === "center" ? (e0 + e1) / 2 : e0;
    } else {
      // an end snaps with its angle measured from the elbow (or the text); an elbow or bend from where the leader
      // leaves the text, so that piece goes level easily
      const l = d.n.leaders[d.g.i], s = leaderStart(d.n, d.L, l.side);
      const from = d.g.kind === "end" && l.elbow && !l.arc ? l.elbow : s;
      const p = host.snap(sx, sy, { last: viewPt(from), hvDeg: LEADER_HV });
      if (d.g.kind === "end") l.end = paper(p); else l.elbow = paper(p);
    }
    host.render();
  }
  function gripUp() {
    const d = drag;
    drag = null;
    if (!d || !d.moved) { host.render(); return; }
    const pt = (q) => ({ x: Math.round(q[0] * ds() * 1000) / 1000, y: Math.round(q[1] * ds() * 1000) / 1000 });
    const patch = d.g.kind === "width"
      ? { width_mm: d.n.text.width_mm, at: pt([d.n.text.x, d.n.text.y]) }
      : { leaders: d.n.leaders.map((l) => ({ side: l.side, end: pt(l.end), elbow: l.elbow ? pt(l.elbow) : null, arc: !!l.arc })) };
    host.cb.patchNote(d.id, patch);
  }

  // ---- the Text tool and Add Leader --------------------------------------------------------------------------------
  // The leader's end snaps as anything placed does; its elbow, and where it meets the text, keep level or plumb with
  // the pick before more readily than other lines do.
  function toolSnap(sx, sy) {
    const last = pts.length ? pts[pts.length - 1] : null;
    return host.snap(sx, sy, last ? { last, hvDeg: LEADER_HV } : {});
  }
  function toolClick(p) {
    pts.push({ x: p.x, y: p.y });
    const o = host.options();
    if (pts.length >= (PICKS[o.leader] || 1)) { const done = pts; pts = []; host.cb.placeNote(done, o); }
  }
  function leaderClick(p) { if (leaderFor) host.cb.addLeader(leaderFor.id, leaderFor.side, { x: p.x, y: p.y }, !!leaderFor.arc); }
  function promptText(tool) {
    if (tool === "noteleader") return `Click where the new ${leaderFor && leaderFor.arc ? "arc " : ""}leader points. Esc finishes.`;
    const end = "Click where the leader points.", meet = "Click where the leader meets the text, then type.";
    const steps = { none: ["Click where the text goes, then type."], one: [end, meet], two: [end, "Click the leader's elbow.", meet], arc: [end, meet] }[host.options().leader] || ["Click where the text goes, then type."];
    return `${steps[Math.min(pts.length, steps.length - 1)]} Esc leaves the tool.`;
  }

  return {
    drawItems, drawNotes, preview, elements, leaderStart, leaderPath, arcBend,
    gripDown, gripMove, gripUp,
    get dragging() { return !!drag; },
    toolSnap, toolClick, leaderClick, promptText,
    get picking() { return pts.length > 0; },
    clearPicks() { pts = []; },
    startLeader(id, side, arc) { leaderFor = { id, side, arc: !!arc }; },
    stopLeader() { leaderFor = null; },
    setEditing(id) { editing = id; host.render(); },
  };
}
