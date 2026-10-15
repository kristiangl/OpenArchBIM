// Permanent dimensions in plan views: the Aligned, Linear, Angular, Radial, Diameter and Arc Length tools,
// drawing, and editing a selected dimension.
// The model owns the geometry and the constraints (crates/openarch-model/src/dimension.rs); this file picks
// references, draws what the model returns and turns clicks on a selected dimension into commands.
// Sizes in the dimension type are paper millimetres, so they are scaled by the view scale and the zoom.

const Dims = (() => {
  let H = null; // host: set by plan.js
  let kind = null; // placing tool: "aligned" | "linear" | "angular" | "radial" | "diameter" | "arclength"
  let wit = null; // Edit Witness Lines: { id }
  let refs = []; // picked so far: { spec, pick, label, a, b }
  let cands = [], idx = 0; // references under the cursor, best first; Tab steps through them
  let cur = null; // cursor in model mm
  let busy = false, again = false;
  let boxes = []; // hit boxes of the selected dimension, in screen pixels
  let drag = null; // { type: "text" | "line", id, seg, sx, sy, start, d, moved, base }
  let input = null;
  let witHover = -1; // witness line under the cursor in Edit Witness Lines
  let lastText = null; // the text just clicked: { id, seg, at, timer }
  let ghost = null, ghostBusy = false, ghostAgain = false; // the model's drawing of a round dimension about to be placed
  let reflexFlag = false; // angular: place the reflex angle
  // chains take any number of references; the round kinds take a fixed number and say what each pick is
  const KINDS = {
    aligned: { name: "Aligned", need: 0, api: "aligned" },
    linear: { name: "Linear", need: 0, api: "linear" },
    angular: { name: "Angular", need: 2, api: "angular", steps: ["Angular dimension: click the first straight reference (a wall face or centre, a grid line).", "Click the second reference. It must not be parallel to the first.", "Move to choose the angle (the quadrant of the cursor decides which), Tab for the reflex angle, then click to place it."] },
    radial: { name: "Radial", need: 1, api: "radial", steps: ["Radial dimension: click a curved wall face."] },
    diameter: { name: "Diameter", need: 1, api: "diameter", steps: ["Diameter dimension: click a curved wall face."] },
    arclength: { name: "Arc Length", need: 3, api: "arc_length", steps: ["Arc length dimension: click the arc to measure.", "Click the line where the measured part starts (a wall end, a wall face or a grid).", "Click the line where it ends.", "Move to set how far out the dimension arc sits, then click to place it. The side of the cursor chooses which part of the circle."] },
  };
  const wantNow = () => (kind === "radial" || kind === "diameter" || (kind === "arclength" && !refs.length) ? "arc" : "line");
  const isFull = () => !!kind && KINDS[kind].need > 0 && refs.length >= KINDS[kind].need;

  const r3 = (v) => Math.round(v * 1000) / 1000;
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1];
  const attach = (h) => { H = h; };
  const active = () => kind !== null || wit !== null;
  const tdef = { tick: "tick", tick_size_mm: 2, witness_gap_mm: 1.5, witness_extension_mm: 2, text_size_mm: 2.5, text_offset_mm: 0.8, text_background: false };

  // ---- tools ---------------------------------------------------------------------------------
  function start(k) { kind = k; wit = null; refs = []; cands = []; idx = 0; ghost = null; reflexFlag = false; closeInput(); H.prompt(prompt()); }
  function startWitness(id) { kind = null; wit = { id }; refs = []; cands = []; idx = 0; witHover = -1; closeInput(); H.prompt(prompt()); }
  function stop() { kind = null; wit = null; refs = []; cands = []; ghost = null; witHover = -1; drag = null; if (lastText) clearTimeout(lastText.timer); lastText = null; closeInput(); }
  function prompt() {
    if (wit) return "Edit Witness Lines: click a reference to add it, or a witness line to take it away. Esc to finish.";
    if (!kind) return "";
    const K = KINDS[kind];
    if (K.need) return K.steps[Math.min(refs.length, K.steps.length - 1)];
    const what = K.name;
    if (!refs.length) return `${what} dimension: click the first reference (a wall face, centre or end, a grid, a level). Tab steps through the lines under the cursor.`;
    if (refs.length === 1) return "Click the next reference.";
    return `${refs.length} references. Click another, or click an empty spot (or press Enter) to place the dimension line. Backspace takes the last one off.`;
  }
  // Esc or right-click: clears the picks first, then leaves the tool. Returns whether it consumed the key.
  function cancel() {
    if (wit) { const id = wit.id; stop(); H.setTool("select"); H.select([id]); return true; }
    if (!kind) return false;
    if (refs.length) { refs = []; cands = []; ghost = null; H.prompt(prompt()); H.render(); return true; }
    stop(); H.setTool("select");
    return true;
  }

  const used = (spec) => refs.some((r) => same(r.spec, spec)) || (wit && ((H.dims().find((d) => d.id === wit.id) || { refs: [] }).refs.some((r) => same(r.spec, spec))));
  const visibleCands = () => (isFull() ? [] : cands.filter((c) => !used(c.spec)));

  async function fetchCands() {
    if (busy) { again = true; return; }
    busy = true;
    do {
      again = false;
      const p = cur;
      if (!p) break;
      try {
        const r = await Backend.call("dimension_candidates", { view_id: H.viewId(), point: [r3(p.x), r3(p.y)], tol: 8 / H.scale(), settings: H.settings(), want: kind ? wantNow() : "line" });
        const next = r.candidates || [];
        if (!same(next.map((c) => c.spec), cands.map((c) => c.spec))) idx = 0;
        cands = next;
      } catch { cands = []; }
    } while (again);
    busy = false;
    fetchGhost();
    H.render();
  }

  // ---- round dimensions: the model draws the ghost, as it knows the geometry ---------------------------
  const mid = (a, b) => [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
  const decorate = (dim) => (dim ? { ...dim, style: { line: { color: "#000000", width_mm: 0.18, pattern: "solid" } } } : null);
  async function fetchGhost() {
    if (!kind || !KINDS[kind].need || !cur) { ghost = null; return; }
    if (ghostBusy) { ghostAgain = true; return; }
    ghostBusy = true;
    do {
      ghostAgain = false;
      let rs = null, lp = [r3(cur.x), r3(cur.y)];
      if (kind === "radial" || kind === "diameter") {
        const vis = visibleCands(), c = vis.length ? vis[idx % vis.length] : null;
        if (c && c.arc) { rs = [{ spec: c.spec, pick: [r3(cur.x), r3(cur.y)] }]; lp = roundDefault(c, cur); }
      } else if (isFull()) rs = refs.map(({ spec, pick }) => ({ spec, pick }));
      if (!rs) { ghost = null; break; }
      try {
        const r = await Backend.call("preview_dimension", { view_id: H.viewId(), kind: KINDS[kind].api, refs: rs, line_point: lp, reflex: reflexFlag });
        ghost = decorate(r.dimension);
      } catch { ghost = null; }
    } while (ghostAgain);
    ghostBusy = false;
    H.render();
  }
  // the text of a radius or diameter starts half way along the line from the centre to where the arc was clicked
  const roundDefault = (c, p) => { const cx = c.arc[0], cy = c.arc[1]; return [r3(cx + (p.x - cx) / 2), r3(cy + (p.y - cy) / 2)]; };

  function move(sx, sy) {
    cur = H.toWorld(sx, sy);
    if (kind || wit) {
      fetchCands();
      if (kind && isFull()) fetchGhost();
      if (wit) witHover = witnessAt(sx, sy);
    }
    H.render();
  }

  // ---- placing -------------------------------------------------------------------------------
  function anchorOf(r) {
    const a = r.a, b = r.b, l2 = (b[0] - a[0]) ** 2 + (b[1] - a[1]) ** 2;
    if (l2 < 1e-9) return a;
    const t = Math.max(0, Math.min(1, ((r.pick[0] - a[0]) * (b[0] - a[0]) + (r.pick[1] - a[1]) * (b[1] - a[1])) / l2));
    return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
  }
  const isLine = (r) => Math.hypot(r.b[0] - r.a[0], r.b[1] - r.a[1]) > 1e-6;
  function linearAxis(p) {
    const anchors = refs.map(anchorOf);
    const cx = anchors.reduce((s, q) => s + q[0], 0) / anchors.length, cy = anchors.reduce((s, q) => s + q[1], 0) / anchors.length;
    return Math.abs(p.y - cy) >= Math.abs(p.x - cx) ? [1, 0] : [0, 1];
  }
  // what the dimension would look like with the line at the cursor, in the shape the model draws
  function previewDraw(p) {
    if (refs.length < 2) return null;
    const anchors = refs.map(anchorOf);
    let u;
    if (kind === "linear") u = linearAxis(p);
    else {
      const L = refs.find(isLine);
      if (L) { const dx = L.b[0] - L.a[0], dy = L.b[1] - L.a[1], l = Math.hypot(dx, dy); u = [-dy / l, dx / l]; }
      else { const dx = anchors[anchors.length - 1][0] - anchors[0][0], dy = anchors[anchors.length - 1][1] - anchors[0][1], l = Math.hypot(dx, dy) || 1; u = [dx / l, dy / l]; }
    }
    const t = [-u[1], u[0]];
    let pos = anchors.map((q) => dot(q, u));
    if (pos[pos.length - 1] < pos[0]) { u = [-u[0], -u[1]]; pos = anchors.map((q) => dot(q, u)); }
    const tt = [-u[1], u[0]], lineAt = dot([p.x, p.y], tt);
    const at = (v) => [u[0] * v + tt[0] * lineAt, u[1] * v + tt[1] * lineAt];
    const order = pos.map((v, i) => i).sort((a, b) => pos[a] - pos[b]);
    const dr = order.map((i) => ({ pos: pos[i], anchor: anchors[i], at: at(pos[i]) }));
    const segments = [];
    for (let i = 0; i + 1 < dr.length; i++) {
      const a = dr[i].at, b = dr[i + 1].at, v = Math.abs(dr[i + 1].pos - dr[i].pos);
      segments.push({ index: i, a, b, value_mm: v, text: String(Math.round(v)), prefix: "", suffix: "", above: "", below: "", text_pos: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], moved: false, text_shift: [0, 0], locked: false, anchor: 0, replaced: false });
    }
    return { id: -1, u, t: tt, refs: dr, segments, equality: false, mid: at((pos[order[0]] + pos[order[order.length - 1]]) / 2), type: tdef, style: { line: { color: "#000000", width_mm: 0.18, pattern: "solid" } } };
  }

  async function place(lp) {
    const K = KINDS[kind];
    if (!cur || (K.need ? refs.length < K.need : refs.length < 2)) return;
    const at = lp || [r3(cur.x), r3(cur.y)];
    const r = await H.call("add_dimension", {
      view_id: H.viewId(), kind: K.api,
      refs: refs.map(({ spec, pick }) => ({ spec, pick })),
      line_point: at,
      axis: kind === "linear" ? linearAxis(cur) : null,
    });
    if (r && kind === "angular" && reflexFlag) await H.call("patch_dimension", { id: r.id, patch: { reflex: true } });
    refs = []; cands = []; idx = 0; ghost = null;
    if (r) { await H.refresh(); H.prompt("Dimension placed. Pick the next references, or Esc to stop."); }
    else H.render();
  }

  function click(sx, sy, ev) {
    cur = H.toWorld(sx, sy);
    if (wit) { witnessClick(sx, sy); return; }
    if (!kind) return;
    const K = KINDS[kind];
    const vis = visibleCands();
    if (vis.length && !(ev && ev.altKey)) {
      const c = vis[idx % vis.length];
      refs.push({ spec: c.spec, pick: [r3(cur.x), r3(cur.y)], label: c.label, a: c.a, b: c.b, arc: c.arc });
      cands = []; idx = 0;
      if (K.need === 1) { place(roundDefault(c, cur)); H.render(); return; }
      H.prompt(prompt());
      fetchCands();
    } else if (isFull() || (!K.need && refs.length >= 2)) place();
    else H.prompt(K.need ? "Click a reference first." : "Pick at least two references before placing the dimension line.");
    H.render();
  }

  // ---- Edit Witness Lines ----------------------------------------------------------------------
  function witnessAt(sx, sy) {
    const d = H.dims().find((x) => x.id === wit.id);
    if (!d) return -1;
    let best = -1, bd = 4;
    d.refs.forEach((r, i) => {
      const A = H.toScreen({ x: r.anchor[0], y: r.anchor[1] }), B = H.toScreen({ x: r.at[0], y: r.at[1] });
      const l2 = (B.x - A.x) ** 2 + (B.y - A.y) ** 2;
      const t = l2 < 1e-9 ? 0 : Math.max(0, Math.min(1, ((sx - A.x) * (B.x - A.x) + (sy - A.y) * (B.y - A.y)) / l2));
      const dd = Math.hypot(sx - (A.x + (B.x - A.x) * t), sy - (A.y + (B.y - A.y) * t));
      if (dd < bd) { bd = dd; best = i; }
    });
    return best;
  }
  async function witnessClick(sx, sy) {
    const d = H.dims().find((x) => x.id === wit.id);
    if (!d) return;
    witHover = witnessAt(sx, sy);
    const vis = visibleCands();
    let r;
    if (witHover >= 0) {
      // on a witness line: take it away (a line of the same wall next to it can still be added by clicking just off it)
      r = await H.call("edit_dimension_refs", { id: d.id, remove: witHover });
    } else if (vis.length) {
      const c = vis[idx % vis.length], v = c.a;
      const newPos = dot(v, d.u);
      const index = d.refs.filter((x) => x.pos < newPos - 1e-6).length;
      r = await H.call("edit_dimension_refs", { id: d.id, insert: { index, ref: { spec: c.spec, pick: [r3(cur.x), r3(cur.y)] } } });
    } else return;
    if (r) { cands = []; await H.refresh(); H.prompt("Witness lines changed. Click more, or Esc to finish."); }
  }

  // ---- keys ----------------------------------------------------------------------------------
  function key(e) {
    if (!active()) return false;
    if (e.key === "Escape") return cancel();
    if (e.key === "Tab" && kind === "angular" && isFull()) { reflexFlag = !reflexFlag; fetchGhost(); return true; }
    if (e.key === "Tab") { const n = visibleCands().length; if (n > 1) { idx = (idx + (e.shiftKey ? -1 : 1) + n * 4) % n; H.render(); } return true; }
    if (kind && e.key === "Backspace") { refs.pop(); ghost = null; H.prompt(prompt()); H.render(); return true; }
    if (kind && e.key === "Enter" && (isFull() || (!KINDS[kind].need && refs.length >= 2))) { place(); return true; }
    return false;
  }

  // ---- drawing -------------------------------------------------------------------------------
  function frame(d, s, k, ctx) {
    const ty = d.type || tdef;
    const size = ty.text_size_mm * k;
    const A = H.toScreen({ x: s.a[0], y: s.a[1] }), B = H.toScreen({ x: s.b[0], y: s.b[1] });
    let th = Math.atan2(B.y - A.y, B.x - A.x);
    if (th > Math.PI / 2 + 1e-6 || th <= -Math.PI / 2 + 1e-6) th += Math.PI;
    const e = { x: Math.cos(th), y: Math.sin(th) };
    let n = { x: Math.sin(th), y: -Math.cos(th) };
    // round dimensions keep their text on the outer side of the line whichever way it reads
    if (s.out && (s.out[0] || s.out[1])) n = { x: s.out[0], y: -s.out[1] };
    let T = H.toScreen({ x: s.text_pos[0], y: s.text_pos[1] });
    if (drag && drag.type === "text" && drag.moved && drag.id === d.id && drag.seg === s.index) T = { x: T.x + drag.d.x * H.scale(), y: T.y - drag.d.y * H.scale() };
    const off = ty.text_offset_mm * k;
    ctx.font = `${Math.max(1, size)}px system-ui, sans-serif`;
    const str = s.text ? `${s.prefix}${s.text}${s.suffix}` : "";
    const w = ctx.measureText(str).width;
    const c = { x: T.x + n.x * (off + size / 2), y: T.y + n.y * (off + size / 2) };
    return { A, B, th, e, n, T, c, w, h: size, size, str, off, home: { x: (A.x + B.x) / 2, y: (A.y + B.y) / 2 } };
  }

  function shifted(d, off) {
    const sh = (p) => [p[0] + d.t[0] * off, p[1] + d.t[1] * off];
    return { ...d, refs: d.refs.map((r) => ({ ...r, at: sh(r.at) })), segments: d.segments.map((s) => ({ ...s, a: sh(s.a), b: sh(s.b), text_pos: sh(s.text_pos) })), mid: sh(d.mid) };
  }

  function glyphLock(ctx, x, y, closed, col) {
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = 1.3;
    ctx.beginPath();
    if (closed) ctx.arc(x, y - 2, 3, Math.PI, 0); else ctx.arc(x + 1.5, y - 2.5, 3, Math.PI * 1.1, Math.PI * 1.9);
    ctx.stroke();
    ctx.fillRect(x - 4, y - 1, 8, 6);
    ctx.restore();
  }

  function tick(ctx, ty, k, p, e, n, col, side) {
    const sz = ty.tick_size_mm * k;
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col;
    if (ty.tick === "tick") {
      const vx = (e.x + n.x) * 0.7071 * sz, vy = (e.y + n.y) * 0.7071 * sz;
      ctx.lineWidth = Math.max(1.4, ctx.lineWidth);
      ctx.beginPath(); ctx.moveTo(p.x - vx, p.y - vy); ctx.lineTo(p.x + vx, p.y + vy); ctx.stroke();
    } else if (ty.tick === "dot") {
      ctx.beginPath(); ctx.arc(p.x, p.y, sz * 0.5, 0, Math.PI * 2); ctx.fill();
    } else if (ty.tick === "arrow") {
      // side: +1 the arrow sits at the start of a segment (points back along e), -1 at its end
      const L = sz * 1.6, hw = L / 4, dirx = e.x * -side, diry = e.y * -side;
      ctx.beginPath();
      ctx.moveTo(p.x, p.y);
      ctx.lineTo(p.x - dirx * L + n.x * hw, p.y - diry * L + n.y * hw);
      ctx.lineTo(p.x - dirx * L - n.x * hw, p.y - diry * L - n.y * hw);
      ctx.closePath(); ctx.fill();
    }
    ctx.restore();
  }

  // the text of one segment (and its leader when moved); returns the frame it was laid out in
  function paintSegText(d, s, o, k, col) {
    const ctx = H.ctx(), ty = d.type || tdef;
    const f = frame(d, s, k, ctx);
    ctx.save();
    ctx.font = `${Math.max(1, f.size)}px system-ui, sans-serif`;
    ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillStyle = col;
    const readable = f.size >= 4;
    if (readable) {
      const paint = (str, p, bg) => {
        if (!str) return;
        ctx.save(); ctx.translate(p.x, p.y); ctx.rotate(f.th);
        if (bg) { const tw = ctx.measureText(str).width; ctx.fillStyle = H.css("--view-bg"); ctx.fillRect(-tw / 2 - 1, -f.size / 2, tw + 2, f.size); ctx.fillStyle = col; }
        ctx.fillText(str, 0, 0.5);
        ctx.restore();
      };
      paint(f.str, f.c, ty.text_background);
      if (s.alt_text) paint(s.alt_text, { x: f.T.x - f.n.x * (f.off + f.size / 2), y: f.T.y - f.n.y * (f.off + f.size / 2) });
      paint(s.above, { x: f.c.x + f.n.x * f.size * 1.2, y: f.c.y + f.n.y * f.size * 1.2 });
      paint(s.below, { x: f.T.x - f.n.x * (f.off + f.size * (s.alt_text ? 1.7 : 0.5)), y: f.T.y - f.n.y * (f.off + f.size * (s.alt_text ? 1.7 : 0.5)) });
    }
    // a moved text keeps a thin leader to where it belongs
    if (s.moved && f.str && Math.hypot(f.c.x - f.home.x, f.c.y - f.home.y) > f.size) {
      ctx.strokeStyle = col; ctx.lineWidth = 0.8; ctx.globalAlpha = 0.7;
      ctx.beginPath(); ctx.moveTo(f.home.x, f.home.y); ctx.lineTo(f.c.x, f.c.y); ctx.stroke();
    }
    ctx.restore();
    if (o.live && f.str) boxes.push({ type: "text", id: d.id, seg: s.index, cx: f.c.x, cy: f.c.y, th: f.th, w: Math.max(f.w, 10) + 4, h: f.size + 4 });
    return f;
  }

  // angular, radial, diameter and arc length: the model gives the arc or line and where it ends
  function drawRound(d, o) {
    const ctx = H.ctx(), k = H.paperPx(), ty = d.type || tdef, R = d.round, col = o.col;
    const S = (p) => H.toScreen({ x: p[0], y: p[1] });
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.setLineDash([]);
    ctx.lineWidth = (H.thin() ? 1 : Math.max(1, d.style.line.width_mm * k)) + (o.selected ? 0.8 : 0);
    if (o.ghost) ctx.globalAlpha = 0.85;
    const gap = ty.witness_gap_mm * k, ext = ty.witness_extension_mm * k;
    ctx.beginPath();
    for (const r of d.refs) {
      const A = S(r.anchor), B = S(r.at), l = Math.hypot(B.x - A.x, B.y - A.y);
      if (l <= gap + 0.5) continue;
      const w = { x: (B.x - A.x) / l, y: (B.y - A.y) / l };
      ctx.moveTo(A.x + w.x * gap, A.y + w.y * gap); ctx.lineTo(B.x + w.x * ext, B.y + w.y * ext);
    }
    ctx.stroke();
    const C = S(R.centre), sc = H.scale();
    ctx.beginPath();
    if (R.arc) ctx.arc(C.x, C.y, R.arc_radius * sc, -R.arc[0], -(R.arc[0] + R.arc[1]), R.arc[1] > 0);
    if (R.line) { const A = S(R.line[0]), B = S(R.line[1]); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); }
    ctx.stroke();
    if (ty.tick !== "none") {
      for (const [p, dir] of R.ends) {
        const e = { x: dir[0], y: -dir[1] }, n = { x: e.y, y: -e.x };
        tick(ctx, ty, k, S(p), e, n, col, -1);
      }
    }
    if (R.centre_mark) {
      const h = 2.5 * k;
      ctx.beginPath(); ctx.moveTo(C.x - h, C.y); ctx.lineTo(C.x + h, C.y); ctx.moveTo(C.x, C.y - h); ctx.lineTo(C.x, C.y + h); ctx.stroke();
    }
    ctx.restore();
    for (const s of d.segments) paintSegText(d, s, o, k, col);
    if (o.live && o.selected) {
      if (R.arc) boxes.push({ type: "arc", id: d.id, cx: C.x, cy: C.y, r: R.arc_radius * sc, a0: R.arc[0], sweep: R.arc[1] });
      else if (R.line) boxes.push({ type: "line", id: d.id, a: S(R.line[0]), b: S(R.line[1]) });
    }
  }

  function drawOne(d, o) {
    const ctx = H.ctx(), k = H.paperPx();
    const ty = d.type || tdef;
    const S = (p) => H.toScreen({ x: p[0], y: p[1] });
    const col = o.col;
    if (d.invalid) {
      const m = S(d.mid);
      ctx.save();
      ctx.strokeStyle = col; ctx.fillStyle = "#c0392b"; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(m.x, m.y - 9); ctx.lineTo(m.x + 9, m.y + 7); ctx.lineTo(m.x - 9, m.y + 7); ctx.closePath(); ctx.stroke();
      ctx.fillRect(m.x - 1, m.y - 3, 2, 6); ctx.fillRect(m.x - 1, m.y + 4, 2, 2);
      ctx.font = "11px system-ui, sans-serif"; ctx.fillStyle = "#c0392b"; ctx.textAlign = "left"; ctx.textBaseline = "middle";
      ctx.fillText(d.invalid, m.x + 14, m.y);
      ctx.restore();
      return;
    }
    if (d.round) { drawRound(d, o); return; }
    if (!d.segments.length) return;
    const first = S(d.segments[0].a), last = S(d.segments[d.segments.length - 1].b);
    const len = Math.hypot(last.x - first.x, last.y - first.y);
    if (len < 1) return;
    const e = { x: (last.x - first.x) / len, y: (last.y - first.y) / len }, n = { x: e.y, y: -e.x };
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.setLineDash([]);
    ctx.lineWidth = (H.thin() ? 1 : Math.max(1, d.style.line.width_mm * k)) + (o.selected ? 0.8 : 0);
    if (o.ghost) ctx.globalAlpha = 0.85;
    // witness lines: a gap off the element, a little past the dimension line
    const gap = ty.witness_gap_mm * k, ext = ty.witness_extension_mm * k;
    ctx.beginPath();
    for (const r of d.refs) {
      const A = S(r.anchor), B = S(r.at), l = Math.hypot(B.x - A.x, B.y - A.y);
      if (l <= gap + 0.5) continue;
      const w = { x: (B.x - A.x) / l, y: (B.y - A.y) / l };
      ctx.moveTo(A.x + w.x * gap, A.y + w.y * gap); ctx.lineTo(B.x + w.x * ext, B.y + w.y * ext);
    }
    ctx.stroke();
    // the dimension line and its ends
    ctx.beginPath(); ctx.moveTo(first.x, first.y); ctx.lineTo(last.x, last.y); ctx.stroke();
    if (ty.tick !== "none") {
      if (ty.tick === "arrow") {
        for (const s of d.segments) { tick(ctx, ty, k, S(s.a), e, n, col, 1); tick(ctx, ty, k, S(s.b), e, n, col, -1); }
      } else {
        for (const r of d.refs) tick(ctx, ty, k, S(r.at), e, n, col, 0);
      }
    }
    ctx.restore();
    // text, padlocks, anchors and the Equality toggle
    for (const s of d.segments) {
      const f = paintSegText(d, s, o, k, col);
      if (o.live && (o.selected || s.locked)) {
        const lx = f.c.x + f.e.x * (f.w / 2 + 11), ly = f.c.y + f.e.y * (f.w / 2 + 11);
        glyphLock(ctx, lx, ly, s.locked, o.selected ? col : "#555");
        if (o.selected) boxes.push({ type: "lock", id: d.id, seg: s.index, x: lx, y: ly, r: 8, locked: s.locked });
      }
      if (o.live && o.selected) {
        // a small triangle at the end that stays put when a value is typed; click it to swap ends
        const sl = Math.hypot(f.B.x - f.A.x, f.B.y - f.A.y);
        if (sl > 50) {
          const A2 = S(s.a), B2 = S(s.b), ll = Math.hypot(B2.x - A2.x, B2.y - A2.y) || 1;
          const ue = { x: (B2.x - A2.x) / ll, y: (B2.y - A2.y) / ll };
          const at = s.anchor === 0 ? { x: A2.x + ue.x * 9, y: A2.y + ue.y * 9 } : { x: B2.x - ue.x * 9, y: B2.y - ue.y * 9 };
          const pt = s.anchor === 0 ? -1 : 1; // the point of the triangle faces its end
          const tx = ue.x * pt, ty2 = ue.y * pt, nx = -ty2, ny = tx;
          ctx.save(); ctx.fillStyle = col; ctx.globalAlpha = s.locked ? 0.35 : 0.9;
          ctx.beginPath(); ctx.moveTo(at.x + tx * 4, at.y + ty2 * 4); ctx.lineTo(at.x - tx * 3 + nx * 3, at.y - ty2 * 3 + ny * 3); ctx.lineTo(at.x - tx * 3 - nx * 3, at.y - ty2 * 3 - ny * 3); ctx.closePath(); ctx.fill();
          ctx.restore();
          boxes.push({ type: "anchor", id: d.id, seg: s.index, x: at.x, y: at.y, r: 7, anchor: s.anchor });
        }
      }
    }
    if (o.live && o.selected && d.refs.length >= 3) {
      const m = S(d.mid), bx = m.x - n.x * 20, by = m.y - n.y * 20;
      const ctx2 = ctx;
      ctx2.save();
      ctx2.font = "10px system-ui, sans-serif"; ctx2.textAlign = "center"; ctx2.textBaseline = "middle";
      ctx2.fillStyle = d.equality ? col : H.css("--view-bg"); ctx2.strokeStyle = col; ctx2.lineWidth = 1.2;
      ctx2.fillRect(bx - 11, by - 7, 22, 14); ctx2.strokeRect(bx - 10.5, by - 6.5, 22, 14);
      ctx2.fillStyle = d.equality ? H.css("--view-bg") : col; ctx2.fillText("EQ", bx, by + 0.5);
      if (!d.equality) { ctx2.beginPath(); ctx2.moveTo(bx - 11, by + 7); ctx2.lineTo(bx + 11, by - 7); ctx2.stroke(); }
      ctx2.restore();
      boxes.push({ type: "eq", id: d.id, x: bx, y: by, w: 22, h: 14, on: d.equality });
    }
    if (o.live && o.selected) boxes.push({ type: "line", id: d.id, a: first, b: last });
  }

  function draw(list, sel, hovered) {
    boxes = [];
    const accent = H.css("--accent"), text = H.css("--text");
    for (const d0 of list || []) {
      let d = d0;
      if (drag && (drag.type === "line" || drag.type === "arc") && drag.moved && drag.id === d.id && !d.invalid) {
        if (d.round) { if (drag.pv) d = { ...drag.pv, style: d.style }; }
        else d = shifted(d, dot([drag.d.x, drag.d.y], d.t));
      }
      const selected = sel.has(d.id), hov = hovered && hovered.includes(d.id);
      const base = d.style && d.style.line ? d.style.line.color : "#000000";
      const col = selected || hov ? accent : d.invalid ? "#c0392b" : base === "#000000" ? text : base;
      drawOne(d, { col, selected, live: true });
    }
  }

  // the picks so far, the line under the cursor and the dimension as it would be placed
  function drawTool() {
    if (!active()) return;
    const ctx = H.ctx(), accent = H.css("--accent");
    const S = (p) => H.toScreen({ x: p[0], y: p[1] });
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 2.5; ctx.globalAlpha = 0.85;
    const strokeCand = (c) => {
      ctx.beginPath();
      if (c.arc) { const C = S([c.arc[0], c.arc[1]]); ctx.arc(C.x, C.y, c.arc[2] * H.scale(), -c.arc[3], -(c.arc[3] + c.arc[4]), c.arc[4] > 0); }
      else { const A = S(c.a), B = S(c.b); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); }
      ctx.stroke();
    };
    for (const r of refs) strokeCand(r);
    const vis = wit && witHover >= 0 ? [] : visibleCands();
    if (vis.length) {
      const c = vis[idx % vis.length];
      ctx.setLineDash([8, 4]); ctx.lineWidth = 3;
      strokeCand(c);
      ctx.setLineDash([]);
      if (cur) {
        const q = H.toScreen(cur), text = vis.length > 1 ? `${c.label} (Tab for more)` : c.label;
        ctx.globalAlpha = 1;
        H.label(text, { x: q.x + 14 + ctx.measureText(text).width / 2, y: q.y - 16 }, accent);
      }
    }
    ctx.restore();
    if (wit && witHover >= 0) {
      const d = H.dims().find((x) => x.id === wit.id), r = d && d.refs[witHover];
      if (r) {
        const A = S(r.anchor), B = S(r.at);
        ctx.save(); ctx.strokeStyle = "#c0392b"; ctx.lineWidth = 3; ctx.globalAlpha = 0.8;
        ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke(); ctx.restore();
        if (cur) { const q = H.toScreen(cur); H.label("Click to remove this witness line", { x: q.x + 100, y: q.y - 16 }, "#c0392b"); }
      }
    }
    if (kind && cur && !KINDS[kind].need && refs.length >= 2) {
      const pv = previewDraw(cur);
      if (pv) drawOne(pv, { col: accent, ghost: true, live: false });
    }
    if (kind && ghost && KINDS[kind].need) drawOne(ghost, { col: accent, ghost: true, live: false });
  }

  // ---- hit testing and selection ---------------------------------------------------------------
  const inText = (b, sx, sy) => {
    const dx = sx - b.cx, dy = sy - b.cy, c = Math.cos(-b.th), s = Math.sin(-b.th);
    const x = dx * c - dy * s, y = dx * s + dy * c;
    return Math.abs(x) <= b.w / 2 && Math.abs(y) <= b.h / 2;
  };
  const segDistPx = (p, a, b) => {
    const l2 = (b.x - a.x) ** 2 + (b.y - a.y) ** 2;
    const t = l2 < 1e-9 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / l2));
    return Math.hypot(p.x - (a.x + (b.x - a.x) * t), p.y - (a.y + (b.y - a.y) * t));
  };
  function boxAt(sx, sy) {
    for (const b of boxes) {
      if ((b.type === "lock" || b.type === "anchor") && Math.hypot(sx - b.x, sy - b.y) <= b.r) return b;
      if (b.type === "eq" && Math.abs(sx - b.x) <= b.w / 2 && Math.abs(sy - b.y) <= b.h / 2) return b;
    }
    for (const b of boxes) if (b.type === "text" && inText(b, sx, sy)) return b;
    for (const b of boxes) if (b.type === "line" && segDistPx({ x: sx, y: sy }, b.a, b.b) <= 5) return b;
    for (const b of boxes) {
      if (b.type !== "arc" || Math.abs(Math.hypot(sx - b.cx, sy - b.cy) - b.r) > 5) continue;
      const ang = Math.atan2(-(sy - b.cy), sx - b.cx), tau = Math.PI * 2;
      const t = b.sweep >= 0 ? (((ang - b.a0) % tau) + tau) % tau : (((b.a0 - ang) % tau) + tau) % tau;
      if (t <= Math.abs(b.sweep) + 0.05) return b;
    }
    return null;
  }

  // outlines the Select tool can pick the dimension by: its line, its witness lines and its text
  function hitRings(d) {
    const ctx = H.ctx(), k = H.paperPx(), w = 4 / H.scale();
    const band = (a, b) => {
      const dx = b[0] - a[0], dy = b[1] - a[1], l = Math.hypot(dx, dy) || 1, nx = -dy / l * w, ny = dx / l * w;
      return [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]];
    };
    if (d.invalid || !d.segments.length) {
      const r = 12 / H.scale(), m = d.mid;
      const sq = [[m[0] - r, m[1] - r], [m[0] + r, m[1] - r], [m[0] + r, m[1] + r], [m[0] - r, m[1] + r]];
      return { ring: sq, rings: [sq] };
    }
    let line, rings;
    if (d.round) {
      const R = d.round;
      if (R.arc) {
        const n = Math.max(8, Math.ceil(Math.abs(R.arc[1]) / 0.08)), ro = R.arc_radius + w, ri = Math.max(0, R.arc_radius - w), outer = [], inner = [];
        for (let i = 0; i <= n; i++) {
          const a = R.arc[0] + R.arc[1] * i / n, c = Math.cos(a), sn = Math.sin(a);
          outer.push([R.centre[0] + ro * c, R.centre[1] + ro * sn]); inner.push([R.centre[0] + ri * c, R.centre[1] + ri * sn]);
        }
        line = outer.concat(inner.reverse());
      } else line = band(R.line[0], R.line[1]);
      rings = [line];
    } else { line = band(d.segments[0].a, d.segments[d.segments.length - 1].b); rings = [line]; }
    for (const r of d.refs) if (Math.hypot(r.at[0] - r.anchor[0], r.at[1] - r.anchor[1]) > 1e-6) rings.push(band(r.anchor, r.at));
    for (const s of d.segments) {
      const f = frame(d, s, k, ctx);
      if (!f.str) continue;
      const hw = Math.max(f.w, 10) / 2 + 2, hh = f.size / 2 + 2, c = Math.cos(f.th), sn = Math.sin(f.th);
      rings.push([[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]].map(([x, y]) => { const p = H.toWorld(f.c.x + x * c - y * sn, f.c.y + x * sn + y * c); return [p.x, p.y]; }));
    }
    return { ring: line, rings };
  }

  // ---- editing a selected dimension --------------------------------------------------------------
  const selectedDim = () => { const s = H.selection(); return s.length === 1 ? H.dims().find((d) => d.id === s[0]) : null; };

  function down(sx, sy) {
    if (active() || input) return false;
    const d = selectedDim();
    if (!d) return false;
    const b = boxAt(sx, sy);
    if (!b || b.id !== d.id) return false;
    if (b.type === "lock") { patch(d.id, { segments: [{ index: b.seg, locked: !b.locked }] }); return true; }
    if (b.type === "anchor") { patch(d.id, { segments: [{ index: b.seg, anchor: 1 - b.anchor }] }); return true; }
    if (b.type === "eq") { patch(d.id, { equality: !b.on }); return true; }
    const p = H.toWorld(sx, sy);
    drag = { type: b.type, id: d.id, seg: b.seg, sx, sy, start: p, d: { x: 0, y: 0 }, moved: false, box: b };
    return true;
  }
  function dragMove(sx, sy) {
    if (!drag) return false;
    if (!drag.moved && Math.hypot(sx - drag.sx, sy - drag.sy) > 5) drag.moved = true;
    if (drag.moved) {
      const p = H.toWorld(sx, sy); drag.d = { x: p.x - drag.start.x, y: p.y - drag.start.y };
      if (drag.type === "line" || drag.type === "arc") dragPreview(p);
      H.render();
    }
    return true;
  }
  // a round dimension's line follows the cursor: the model draws it, as the arc and angle depend on the geometry
  async function dragPreview(p) {
    const g = drag;
    if (!g || g.pbusy) { if (g) g.pagain = p; return; }
    const d = H.dims().find((x) => x.id === g.id);
    if (!d || !d.round) return;
    g.pbusy = true;
    let at = p;
    while (at && drag === g) {
      g.pagain = null;
      try { const r = await Backend.call("preview_dimension", { id: g.id, line_point: [r3(at.x), r3(at.y)] }); if (drag === g) g.pv = r.dimension; } catch { /* keep the last drawing */ }
      H.render();
      at = g.pagain;
    }
    g.pbusy = false;
  }
  function up(sx, sy) {
    if (!drag) return false;
    const g = drag; drag = null;
    const d = H.dims().find((x) => x.id === g.id);
    if (!d) { H.render(); return true; }
    if (g.moved) {
      if (g.type === "text") {
        const s = d.segments[g.seg], sh = s ? s.text_shift : [0, 0];
        patch(d.id, { segments: [{ index: g.seg, text_shift: [r3(sh[0] + g.d.x), r3(sh[1] + g.d.y)] }] });
      } else if (g.type === "line" || g.type === "arc") {
        const p = H.toWorld(sx, sy);
        patch(d.id, { line_point: [r3(p.x), r3(p.y)] });
      }
    } else if (g.type === "text") {
      // one click types a value, two clicks open the text dialog (the input would otherwise cover the second click)
      const now = Date.now();
      if (lastText && lastText.id === d.id && lastText.seg === g.seg && now - lastText.at < 450) {
        clearTimeout(lastText.timer); lastText = null;
        closeInput();
        H.openText(d, d.segments[g.seg]);
      } else if (d.round) {
        lastText = { id: d.id, seg: g.seg, at: now, timer: setTimeout(() => { lastText = null; }, 450) };
      } else {
        lastText = { id: d.id, seg: g.seg, at: now, timer: setTimeout(() => { lastText = null; const dd = H.dims().find((x) => x.id === g.id); if (dd) openValue(g.box, dd); }, 330) };
      }
    }
    H.render();
    return true;
  }
  function dblclick(sx, sy) {
    if (active()) return false;
    const d = selectedDim();
    if (!d) return false;
    const b = boxAt(sx, sy);
    // the second click already opened the text dialog; just keep the double click from doing anything else
    return !!(b && b.type === "text");
  }
  async function patch(id, p) {
    const r = await H.call("patch_dimension", { id, patch: p });
    await H.refresh();
    return r;
  }
  function closeInput() { const i = input; input = null; if (i && i.parentNode) i.parentNode.removeChild(i); }
  function openValue(b, d) {
    closeInput();
    const s = d.segments[b.seg];
    if (!s) return;
    const r = H.canvasRect();
    const inp = document.createElement("input");
    inp.type = "text"; inp.id = "dim-input"; inp.setAttribute("aria-label", "Dimension value");
    inp.value = String(Math.round(s.value_mm * 10) / 10);
    inp.style.cssText = `position:fixed;left:${r.left + b.cx - 40}px;top:${r.top + b.cy - 12}px;width:80px;height:24px;z-index:60;text-align:center;font:12px system-ui,sans-serif;border:1.5px solid #1d6fb8;border-radius:3px;background:#fff;color:#111;`;
    document.body.appendChild(inp);
    inp.focus(); inp.select();
    inp.onkeydown = async (e) => {
      e.stopPropagation();
      if (e.key === "Enter") {
        const v = parseLengthMM(inp.value);
        closeInput();
        if (!isNaN(v) && v > 0 && Math.abs(v - s.value_mm) > 0.001) { await H.call("set_dimension_value", { id: d.id, segment: b.seg, value: r3(v) }); await H.refresh(); }
      } else if (e.key === "Escape") closeInput();
    };
    inp.onblur = () => { if (input === inp) closeInput(); };
    input = inp;
  }

  return {
    attach, active, start, startWitness, stop, cancel, prompt, key, move, click, draw, drawTool, hitRings,
    down, dragMove, up, dblclick, closeInput,
    get kind() { return kind; },
    get witness() { return wit; },
    get dragging() { return !!drag; },
    wantsSnap: () => false,
    // test hooks
    boxes: () => boxes.map((b) => ({ ...b })),
    picks: () => refs.map((r) => ({ label: r.label })),
    candidates: () => visibleCands().map((c) => c.label),
  };
})();
