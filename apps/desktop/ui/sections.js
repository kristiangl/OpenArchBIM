// Section lines in plan views: the line, its marker (head with the view name and numbers, an arrow for the side it
// looks at) and the grips of the selected section. The model owns the section (crates/openarch-model/src/section.rs);
// the plan scene brings each section with the corners of its line, its runs and jogs, the break in this plan, its
// marker family and labels worked out. Sizes in the marker family are paper millimetres, so they are scaled by the
// view scale and the zoom. A jogged line is runs joined by square jogs; a broken line shows a stretch at each end.

const Sections = (() => {
  let H = null; // host: set by plan.js
  let boxes = []; // grips of the selected section, in screen pixels
  // { id, type: "end0" | "end1" | "depth" | "flip" | "run" | "jog" | "gap0" | "gap1" | "break", index, sx, sy, moved, p, s }
  let drag = null;
  let split = null; // Split Segment: { id, at (where the line splits), preview (corners the model would give) }
  const attach = (h) => { H = h; };
  const r3 = (v) => Math.round(v * 1000) / 1000;
  const HEAD_MM = 4; // the head circle sits this far (paper mm) beyond the end of the line

  const P = (a) => ({ x: a[0], y: a[1] });
  const lookAngle = (look) => (Math.atan2(look[1], look[0]) * 180) / Math.PI;
  const len = (a, b) => Math.hypot(b.x - a.x, b.y - a.y);
  const cornersOf = (s) => (s.pts || [s.start, s.end]).map(P);
  const pathLen = (pts) => pts.slice(1).reduce((t, q, i) => t + len(pts[i], q), 0);
  const unit = (a, b) => { const l = len(a, b) || 1; return { x: (b.x - a.x) / l, y: (b.y - a.y) / l }; };

  // the stretch of a path from `d0` to `d1` along it
  function subPath(pts, d0, d1) {
    const out = [];
    let run = 0;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = pts[i], l = len(a, b);
      if (l > 0 && d1 >= run && d0 <= run + l) {
        const at = (d) => { const t = Math.max(0, Math.min(1, (d - run) / l)); return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }; };
        if (!out.length) out.push(at(d0));
        out.push(at(d1));
      }
      run += l;
    }
    return out;
  }
  const pointAt = (pts, d) => subPath(pts, d, d)[0] || pts[0];
  // how far along a path its nearest place to `p` is
  function along(pts, p) {
    let best = Infinity, at = 0, run = 0;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = pts[i], l = len(a, b);
      const t = l > 0 ? Math.max(0, Math.min(1, ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / (l * l))) : 0;
      const q = { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, dd = len(q, p);
      if (dd < best) { best = dd; at = run + t * l; }
      run += l;
    }
    return at;
  }

  // the section as it should be drawn now: a drag or a split shows the change before the model has it
  function shown(s) {
    if (split && split.id === s.id && split.preview) return { ...s, pts: split.preview };
    if (!drag || drag.id !== s.id || !drag.moved || !drag.p) return s;
    const pts = cornersOf(s), n = pts.length, p = drag.p;
    const t = unit(pts[0], pts[1]), nn = { x: -t.y, y: t.x };
    const dot = (v, w) => v.x * w.x + v.y * w.y;
    const moved = (q, v, k) => ({ x: q.x + v.x * k, y: q.y + v.y * k });
    if (drag.type === "end0" || drag.type === "end1") {
      if (n > 2) {
        // a jogged line's end slides along its own run
        const i = drag.type === "end0" ? 0 : n - 1, q = pts[i], k = dot({ x: p.x - q.x, y: p.y - q.y }, t);
        pts[i] = moved(q, t, k);
        return { ...s, pts: pts.map((c) => [c.x, c.y]) };
      }
      const start = drag.type === "end0" ? [p.x, p.y] : s.start, end = drag.type === "end1" ? [p.x, p.y] : s.end;
      const u = unit(P(start), P(end)), tt = s.flipped ? { x: -u.x, y: -u.y } : u;
      return { ...s, start, end, pts: [start, end], look: [-tt.y, tt.x] };
    }
    if (drag.type === "run") {
      const k = drag.index, off = dot({ x: p.x - pts[2 * k].x, y: p.y - pts[2 * k].y }, nn);
      pts[2 * k] = moved(pts[2 * k], nn, off); pts[2 * k + 1] = moved(pts[2 * k + 1], nn, off);
      return { ...s, pts: pts.map((c) => [c.x, c.y]) };
    }
    if (drag.type === "jog") {
      const j = drag.index, off = dot({ x: p.x - pts[2 * j + 1].x, y: p.y - pts[2 * j + 1].y }, t);
      pts[2 * j + 1] = moved(pts[2 * j + 1], t, off); pts[2 * j + 2] = moved(pts[2 * j + 2], t, off);
      return { ...s, pts: pts.map((c) => [c.x, c.y]) };
    }
    if (drag.type === "depth") return { ...s, depth: drag.depth, frame: s.frame && { ...s.frame, depth: drag.depth } };
    if (drag.type === "gap0" || drag.type === "gap1") return { ...s, gap: drag.gap };
    return s;
  }

  function mark(ctx, fam, labels, at, outward, look, o) {
    // the head (circle, divider line, labels) stays upright; the arrow turns with the side the section looks at
    const c = { x: at.x + outward.x * HEAD_MM * o.k, y: at.y + outward.y * HEAD_MM * o.k };
    const opt = { pats: H.pats && H.pats() };
    if (o.head) Ann.paths(ctx, fam.body, { x: c.x, y: c.y, k: o.k, rot: 0 }, o.hot, o.bg, opt);
    Ann.paths(ctx, fam.pointer, { x: c.x, y: c.y, k: o.k, rot: lookAngle(look) }, o.hot, o.bg, opt);
    if (o.head) Ann.labels(ctx, labels, { x: c.x, y: c.y, k: o.k }, o.col);
  }

  function strokePath(ctx, pts) {
    if (pts.length < 2) return;
    const S = pts.map((p) => H.toScreen(p));
    ctx.beginPath(); ctx.moveTo(S[0].x, S[0].y);
    for (const q of S.slice(1)) ctx.lineTo(q.x, q.y);
    ctx.stroke();
  }

  function drawOne(s0, selected, hovered) {
    const ctx = H.ctx(), s = shown(s0);
    const text = H.css("--text"), accent = H.css("--accent"), bg = H.css("--view-bg");
    const col = selected || hovered ? accent : text;
    const k = H.paperPx(), pts = cornersOf(s), S = pts.map((p) => H.toScreen(p)), L = pathLen(pts);
    if (pathLen(S) < 2) return;
    ctx.save();
    ctx.strokeStyle = col; ctx.lineWidth = selected ? 2 : Math.max(1, 0.35 * k); ctx.setLineDash([]); ctx.lineCap = "round"; ctx.lineJoin = "miter";
    if (s.gap) {
      // broken in this plan: a stretch at each end; the middle is a gap, a thin line, or dashed while selected
      strokePath(ctx, subPath(pts, 0, s.gap.head_mm));
      strokePath(ctx, subPath(pts, L - s.gap.tail_mm, L));
      const mid = subPath(pts, s.gap.head_mm, L - s.gap.tail_mm);
      if (s.broken_style === "continuous") { ctx.lineWidth = 1; strokePath(ctx, mid); }
      else if (selected) { ctx.lineWidth = 1; ctx.setLineDash([4, 4]); strokePath(ctx, mid); ctx.setLineDash([]); }
    } else {
      strokePath(ctx, pts);
    }
    const n = S.length;
    if (s.family) {
      const o = { k, col, hot: selected || hovered ? accent : null, bg }, out0 = unit(S[1], S[0]), out1 = unit(S[n - 2], S[n - 1]);
      mark(ctx, s.family, s.labels, S[0], out0, s.look, { ...o, head: true });
      mark(ctx, s.family, [], S[n - 1], out1, s.look, { ...o, head: false });
    }
    if (selected && !s.pinned) grips(ctx, s, pts, S, L, accent);
    ctx.restore();
  }

  // the box the view looks at, and the grips of a selected section
  function grips(ctx, s, pts, S, L, accent) {
    const lk = { x: s.look[0], y: -s.look[1] }, n = S.length;
    const f = s.frame;
    if (f && f.depth) {
      const o = P(f.o), at = (u, d) => H.toScreen({ x: o.x + f.right[0] * u + f.look[0] * d, y: o.y + f.right[1] * u + f.look[1] * d });
      const F0 = at(0, f.depth), F1 = at(f.width, f.depth);
      // the ends at the left and right of the view
      const [A, B] = s.flipped ? [S[n - 1], S[0]] : [S[0], S[n - 1]];
      ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
      ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(F0.x, F0.y); ctx.lineTo(F1.x, F1.y); ctx.lineTo(B.x, B.y); ctx.stroke();
      ctx.setLineDash([]);
      boxes.push({ id: s.id, type: "depth", x: (F0.x + F1.x) / 2, y: (F0.y + F1.y) / 2, r: 5, sq: true, lk });
    }
    boxes.push({ id: s.id, type: "end0", x: S[0].x, y: S[0].y, r: 5 }, { id: s.id, type: "end1", x: S[n - 1].x, y: S[n - 1].y, r: 5 });
    // the flip and break controls sit either side of the middle of the longest run, clear of the jogs
    let k0 = 0;
    for (let k = 1; k < n / 2; k++) if (len(pts[2 * k], pts[2 * k + 1]) > len(pts[2 * k0], pts[2 * k0 + 1])) k0 = k;
    const M = H.toScreen({ x: (pts[2 * k0].x + pts[2 * k0 + 1].x) / 2, y: (pts[2 * k0].y + pts[2 * k0 + 1].y) / 2 });
    boxes.push({ id: s.id, type: "flip", x: M.x + lk.x * 16, y: M.y + lk.y * 16, r: 7, lk });
    // the break control sits on the other side of the line
    boxes.push({ id: s.id, type: "break", x: M.x - lk.x * 16, y: M.y - lk.y * 16, r: 7, on: !!s.gap });
    if (n > 2) {
      // a jogged line: a grip on each run to move it across, one on each jog to slide it along
      for (let k = 0; k < n / 2; k++) {
        const a = S[2 * k], b = S[2 * k + 1];
        boxes.push({ id: s.id, type: "run", index: k, x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, r: 5, dia: true });
      }
      for (let j = 0; j + 1 < n / 2; j++) {
        const a = S[2 * j + 1], b = S[2 * j + 2];
        boxes.push({ id: s.id, type: "jog", index: j, x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, r: 4, sq: true });
      }
    }
    if (s.gap) {
      const g0 = H.toScreen(pointAt(pts, s.gap.head_mm)), g1 = H.toScreen(pointAt(pts, L - s.gap.tail_mm));
      boxes.push({ id: s.id, type: "gap0", x: g0.x, y: g0.y, r: 4 }, { id: s.id, type: "gap1", x: g1.x, y: g1.y, r: 4 });
    }
  }

  function drawGrips() {
    const ctx = H.ctx(), accent = H.css("--accent"), bg = H.css("--view-bg");
    ctx.save();
    for (const b of boxes) {
      ctx.strokeStyle = accent; ctx.fillStyle = bg; ctx.lineWidth = 1.5; ctx.setLineDash([]);
      if (b.type === "flip") {
        // two arrows, one each way across the line: the side the section looks at
        ctx.beginPath(); ctx.arc(b.x, b.y, b.r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
        const n = b.lk, t = { x: -n.y, y: n.x }, tip = (sg) => ({ x: b.x + n.x * 4.5 * sg, y: b.y + n.y * 4.5 * sg });
        ctx.beginPath(); ctx.moveTo(tip(-1).x, tip(-1).y); ctx.lineTo(tip(1).x, tip(1).y);
        for (const sg of [1, -1]) { const q = tip(sg); ctx.moveTo(q.x - n.x * 2.2 * sg + t.x * 2, q.y - n.y * 2.2 * sg + t.y * 2); ctx.lineTo(q.x, q.y); ctx.lineTo(q.x - n.x * 2.2 * sg - t.x * 2, q.y - n.y * 2.2 * sg - t.y * 2); }
        ctx.stroke();
      } else if (b.type === "break") {
        // a zigzag: click to break the line in this plan, or to join it again
        ctx.beginPath(); ctx.arc(b.x, b.y, b.r, 0, Math.PI * 2); if (b.on) { ctx.fillStyle = accent; } ctx.fill(); ctx.stroke();
        ctx.strokeStyle = b.on ? bg : accent; ctx.lineWidth = 1.2;
        ctx.beginPath(); ctx.moveTo(b.x - 4.5, b.y); ctx.lineTo(b.x - 1.5, b.y); ctx.lineTo(b.x - 0.5, b.y - 3); ctx.lineTo(b.x + 0.5, b.y + 3); ctx.lineTo(b.x + 1.5, b.y); ctx.lineTo(b.x + 4.5, b.y); ctx.stroke();
      } else if (b.dia) {
        ctx.beginPath(); ctx.moveTo(b.x, b.y - b.r); ctx.lineTo(b.x + b.r, b.y); ctx.lineTo(b.x, b.y + b.r); ctx.lineTo(b.x - b.r, b.y); ctx.closePath(); ctx.fill(); ctx.stroke();
      } else if (b.sq) {
        ctx.fillRect(b.x - b.r, b.y - b.r, 2 * b.r, 2 * b.r); ctx.strokeRect(b.x - b.r, b.y - b.r, 2 * b.r, 2 * b.r);
      } else {
        ctx.beginPath(); ctx.arc(b.x, b.y, b.r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      }
    }
    ctx.restore();
  }

  function draw(list, sel, hovered) {
    boxes = [];
    const selected = new Set(sel);
    for (const s of list || []) drawOne(s, selected.has(s.id) && !split, hovered && hovered.includes(s.id));
    drawGrips();
    if (split && split.at) {
      // where the line splits
      const ctx = H.ctx(), q = H.toScreen(split.at);
      ctx.save(); ctx.strokeStyle = H.css("--accent"); ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(q.x - 5, q.y - 5); ctx.lineTo(q.x + 5, q.y + 5); ctx.moveTo(q.x - 5, q.y + 5); ctx.lineTo(q.x + 5, q.y - 5); ctx.stroke();
      ctx.restore();
    }
  }

  // the line being drawn: from the first click to the cursor, looking to the left of the direction of drawing
  function drawPreview(a, b) {
    const ctx = H.ctx(), accent = H.css("--accent"), bg = H.css("--view-bg");
    const A = H.toScreen(a), B = H.toScreen(b), l = len(A, B);
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.setLineDash([8, 4]);
    ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke();
    ctx.setLineDash([]);
    if (l > 8 && H.family()) {
      const wl = Math.hypot(b.x - a.x, b.y - a.y), look = [-(b.y - a.y) / wl, (b.x - a.x) / wl], u = unit(A, B);
      const o = { k: H.paperPx(), col: accent, hot: accent, bg };
      mark(ctx, H.family(), [], A, { x: -u.x, y: -u.y }, look, { ...o, head: true });
      mark(ctx, H.family(), [], B, u, look, { ...o, head: false });
    }
    ctx.restore();
  }

  // world rings a click can hit: a band along each visible stretch of the line and a square round the head
  function hitRings(s) {
    const tol = 4 / H.scale(), m = H.paperPx() / H.scale();
    const pts = cornersOf(s), L = pathLen(pts);
    const parts = s.gap ? [subPath(pts, 0, s.gap.head_mm), subPath(pts, L - s.gap.tail_mm, L)] : [pts];
    const bands = [];
    for (const part of parts) {
      for (let i = 1; i < part.length; i++) {
        const a = part[i - 1], b = part[i], u = unit(a, b), nn = [-u.y, u.x];
        bands.push([[a.x + nn[0] * tol, a.y + nn[1] * tol], [b.x + nn[0] * tol, b.y + nn[1] * tol], [b.x - nn[0] * tol, b.y - nn[1] * tol], [a.x - nn[0] * tol, a.y - nn[1] * tol]]);
      }
    }
    const u = unit(pts[0], pts[1]), c = [pts[0].x - u.x * HEAD_MM * m, pts[0].y - u.y * HEAD_MM * m], r = HEAD_MM * m;
    const head = [[c[0] - r, c[1] - r], [c[0] + r, c[1] - r], [c[0] + r, c[1] + r], [c[0] - r, c[1] + r]];
    return { ring: bands[0] || head, rings: [...bands, head] };
  }

  // ---- grips ---------------------------------------------------------------------------------
  const selected = () => {
    const ids = H.selection();
    return ids.length === 1 ? H.sections().find((s) => s.id === ids[0]) : null;
  };

  function down(sx, sy) {
    const s = selected();
    if (!s || s.pinned || split) return false;
    const b = boxes.find((q) => q.id === s.id && Math.hypot(sx - q.x, sy - q.y) <= q.r + 3);
    if (!b) return false;
    drag = { id: s.id, type: b.type, index: b.index, sx, sy, moved: false, p: null, depth: s.depth, gap: s.gap, box: b, s };
    return true;
  }

  function dragMove(sx, sy) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(sx - drag.sx, sy - drag.sy) < 4) return;
    if (drag.type === "flip" || drag.type === "break") return;
    drag.moved = true;
    const s = drag.s, w = H.toWorld(sx, sy);
    if (drag.type === "end0" || drag.type === "end1" || drag.type === "run" || drag.type === "jog") {
      drag.p = H.snap(sx, sy);
    } else if (drag.type === "depth") {
      // distance of the cursor in front of the head run, along the side the section looks at
      const o = s.frame ? P(s.frame.o) : P(s.start);
      const d = (w.x - o.x) * s.look[0] + (w.y - o.y) * s.look[1];
      drag.depth = Math.max(100, Math.round(d / 10) * 10);
      drag.p = w;
    } else if (drag.type === "gap0" || drag.type === "gap1") {
      // the end of a stretch slides along the line; the two stretches never meet
      const pts = cornersOf(s), L = pathLen(pts), at = Math.round(along(pts, w)), g = { ...s.gap };
      if (drag.type === "gap0") g.head_mm = Math.max(0, Math.min(at, L - g.tail_mm - 10));
      else g.tail_mm = Math.max(0, Math.min(L - at, L - g.head_mm - 10));
      drag.gap = g;
      drag.p = w;
    }
    H.render();
  }

  async function up() {
    const d = drag;
    drag = null;
    if (!d) return;
    const id = d.id, pt = () => [r3(d.p.x), r3(d.p.y)];
    let ok = true;
    if (d.type === "flip") ok = await H.call("patch_section", { id, patch: { flipped: !d.s.flipped } });
    else if (d.type === "break") ok = await H.call("section_break", { id, view_id: H.viewId() });
    else if (!d.moved) { H.render(); return; }
    else if (d.type === "end0") ok = await H.call("patch_section", { id, patch: { start: pt() } });
    else if (d.type === "end1") ok = await H.call("patch_section", { id, patch: { end: pt() } });
    else if (d.type === "depth") ok = await H.call("patch_section", { id, patch: { far_clip_mm: d.depth, ...(d.s.far_clip === "no_clip" ? { far_clip: "with_line" } : {}) } });
    else if (d.type === "run") ok = await H.call("section_edit", { id, op: "move_run", index: d.index, to: pt() });
    else if (d.type === "jog") ok = await H.call("section_edit", { id, op: "move_jog", index: d.index, to: pt() });
    else if (d.type === "gap0" || d.type === "gap1") ok = await H.call("section_break", { id, view_id: H.viewId(), head_mm: d.gap.head_mm, tail_mm: d.gap.tail_mm });
    if (ok) await H.refresh(); else H.render();
  }

  // ---- Split Segment: click the line where it splits, then move the cursor across it and click -------
  function startSplit(id) { split = { id, at: null, preview: null }; }
  function stopSplit() { split = null; }
  const splitPrompt = () => (!split ? "" : split.at
    ? "Move the cursor to the side of the split that moves, and across the line by how far it moves; click to place it. Esc cancels."
    : "Click the section line where it should split.");

  let pending = null;
  async function splitMove(sx, sy) {
    if (!split || !split.at) return;
    const to = H.snap(sx, sy), want = { id: split.id, op: "split", at: [r3(split.at.x), r3(split.at.y)], to: [r3(to.x), r3(to.y)], preview: true };
    // one preview in flight at a time; the latest cursor position wins
    if (pending) { pending.next = want; return; }
    pending = { next: want };
    while (pending.next && split) {
      const q = pending.next;
      pending.next = null;
      const r = await H.preview("section_edit", q);
      if (split) split.preview = r ? r.pts : null;
      H.render();
    }
    pending = null;
  }

  async function splitClick(sx, sy) {
    if (!split) return false;
    const s = H.sections().find((x) => x.id === split.id);
    if (!s) { split = null; return false; }
    if (!split.at) {
      const pts = cornersOf(s), w = H.toWorld(sx, sy);
      split.at = pointAt(pts, along(pts, w));
      H.prompt(splitPrompt());
      H.render();
      return true;
    }
    const to = H.snap(sx, sy), at = split.at;
    const ok = await H.call("section_edit", { id: s.id, op: "split", at: [r3(at.x), r3(at.y)], to: [r3(to.x), r3(to.y)] });
    if (!ok) { split.preview = null; H.render(); return true; }
    split = null;
    await H.refresh();
    H.done();
    return true;
  }

  return {
    attach, draw, drawPreview, hitRings, down, dragMove, up, grips: () => boxes, get dragging() { return !!drag; },
    startSplit, stopSplit, splitMove, splitClick, splitPrompt, get splitting() { return !!split; },
  };
})();
