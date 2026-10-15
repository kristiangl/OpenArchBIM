// Section lines in plan views: the line, its marker (head with the view name and numbers, an arrow for the side it
// looks at) and the grips of the selected section. The model owns the section (crates/openarch-model/src/section.rs);
// the plan scene brings each section with its marker family and labels worked out. Sizes in the marker family are
// paper millimetres, so they are scaled by the view scale and the zoom.

const Sections = (() => {
  let H = null; // host: set by plan.js
  let boxes = []; // grips of the selected section, in screen pixels
  let drag = null; // { id, type: "end0" | "end1" | "depth" | "flip", sx, sy, moved, p }
  const attach = (h) => { H = h; };
  const r3 = (v) => Math.round(v * 1000) / 1000;
  const HEAD_MM = 4; // the head circle sits this far (paper mm) beyond the end of the line

  const toS = (p) => H.toScreen({ x: p[0], y: p[1] });
  const lookAngle = (look) => (Math.atan2(look[1], look[0]) * 180) / Math.PI;

  // the section as it should be drawn now: a drag shows the new ends before the model has them
  function shown(s) {
    if (!drag || drag.id !== s.id || !drag.moved || !drag.p) return s;
    if (drag.type === "end0" || drag.type === "end1") {
      const start = drag.type === "end0" ? [drag.p.x, drag.p.y] : s.start, end = drag.type === "end1" ? [drag.p.x, drag.p.y] : s.end;
      const dx = end[0] - start[0], dy = end[1] - start[1], l = Math.hypot(dx, dy) || 1;
      const t = s.flipped ? [-dx / l, -dy / l] : [dx / l, dy / l];
      return { ...s, start, end, look: [-t[1], t[0]] };
    }
    if (drag.type === "depth") return { ...s, depth: drag.depth };
    return s;
  }

  function mark(ctx, fam, labels, at, outward, look, o) {
    // the head (circle, divider line, labels) stays upright; the arrow turns with the side the section looks at
    const c = { x: at.x + outward.x * HEAD_MM * o.k, y: at.y + outward.y * HEAD_MM * o.k };
    const pen = Ann.penPx(fam, { k: o.k });
    if (o.head) Ann.shapes(ctx, fam.shapes, { x: c.x, y: c.y, k: o.k, rot: 0 }, o.col, o.bg, pen);
    Ann.shapes(ctx, fam.pointer, { x: c.x, y: c.y, k: o.k, rot: lookAngle(look) }, o.col, o.bg, pen);
    if (o.head) Ann.labels(ctx, labels, { x: c.x, y: c.y, k: o.k }, o.col);
  }

  function drawOne(s0, selected, hovered) {
    const ctx = H.ctx(), s = shown(s0);
    const text = H.css("--text"), accent = H.css("--accent"), bg = H.css("--view-bg");
    const col = selected || hovered ? accent : text;
    const k = H.paperPx(), A = toS(s.start), B = toS(s.end);
    const len = Math.hypot(B.x - A.x, B.y - A.y);
    if (len < 2) return;
    const u = { x: (B.x - A.x) / len, y: (B.y - A.y) / len };
    ctx.save();
    ctx.strokeStyle = col; ctx.lineWidth = selected ? 2 : Math.max(1, 0.35 * k); ctx.setLineDash([]); ctx.lineCap = "round";
    ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke();
    if (s.family) {
      const o = { k, col, bg };
      mark(ctx, s.family, s.labels, A, { x: -u.x, y: -u.y }, s.look, { ...o, head: true });
      mark(ctx, s.family, [], B, u, s.look, { ...o, head: false });
    }
    if (selected && !s.pinned) {
      // the box the view looks at, and its grips
      const lk = { x: s.look[0], y: -s.look[1] };
      if (s.depth) {
        const d = s.depth * H.scale(), F0 = { x: A.x + lk.x * d, y: A.y + lk.y * d }, F1 = { x: B.x + lk.x * d, y: B.y + lk.y * d };
        ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
        ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(F0.x, F0.y); ctx.lineTo(F1.x, F1.y); ctx.lineTo(B.x, B.y); ctx.stroke();
        ctx.setLineDash([]);
        boxes.push({ id: s.id, type: "depth", x: (F0.x + F1.x) / 2, y: (F0.y + F1.y) / 2, r: 5, sq: true, lk });
      }
      const M = { x: (A.x + B.x) / 2, y: (A.y + B.y) / 2 };
      boxes.push({ id: s.id, type: "end0", x: A.x, y: A.y, r: 5 }, { id: s.id, type: "end1", x: B.x, y: B.y, r: 5 });
      boxes.push({ id: s.id, type: "flip", x: M.x + lk.x * 16, y: M.y + lk.y * 16, r: 7, lk });
    }
    ctx.restore();
  }

  function drawGrips() {
    const ctx = H.ctx(), accent = H.css("--accent"), bg = H.css("--view-bg");
    ctx.save();
    for (const b of boxes) {
      ctx.strokeStyle = accent; ctx.fillStyle = bg; ctx.lineWidth = 1.5; ctx.setLineDash([]);
      if (b.type === "flip") {
        // two arrows, one each way across the line: the side the section looks at
        ctx.beginPath(); ctx.arc(b.x, b.y, b.r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
        // a double-headed arrow across the line
        const n = b.lk, t = { x: -n.y, y: n.x }, tip = (sg) => ({ x: b.x + n.x * 4.5 * sg, y: b.y + n.y * 4.5 * sg });
        ctx.beginPath(); ctx.moveTo(tip(-1).x, tip(-1).y); ctx.lineTo(tip(1).x, tip(1).y);
        for (const sg of [1, -1]) { const q = tip(sg); ctx.moveTo(q.x - n.x * 2.2 * sg + t.x * 2, q.y - n.y * 2.2 * sg + t.y * 2); ctx.lineTo(q.x, q.y); ctx.lineTo(q.x - n.x * 2.2 * sg - t.x * 2, q.y - n.y * 2.2 * sg - t.y * 2); }
        ctx.stroke();
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
    for (const s of list || []) drawOne(s, selected.has(s.id), hovered && hovered.includes(s.id));
    drawGrips();
  }

  // the line being drawn: from the first click to the cursor, looking to the left of the direction of drawing
  function drawPreview(a, b) {
    const ctx = H.ctx(), accent = H.css("--accent"), bg = H.css("--view-bg");
    const A = H.toScreen(a), B = H.toScreen(b), len = Math.hypot(B.x - A.x, B.y - A.y);
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.setLineDash([8, 4]);
    ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke();
    ctx.setLineDash([]);
    if (len > 8 && H.family()) {
      const wl = Math.hypot(b.x - a.x, b.y - a.y), look = [-(b.y - a.y) / wl, (b.x - a.x) / wl], u = { x: (B.x - A.x) / len, y: (B.y - A.y) / len };
      const o = { k: H.paperPx(), col: accent, bg };
      mark(ctx, H.family(), [], A, { x: -u.x, y: -u.y }, look, { ...o, head: true });
      mark(ctx, H.family(), [], B, u, look, { ...o, head: false });
    }
    ctx.restore();
  }

  // world rings a click can hit: a band along the line and a square round the head
  function hitRings(s) {
    const tol = 4 / H.scale(), m = H.paperPx() / H.scale();
    const dx = s.end[0] - s.start[0], dy = s.end[1] - s.start[1], l = Math.hypot(dx, dy) || 1;
    const u = [dx / l, dy / l], n = [-u[1], u[0]];
    const band = [[s.start[0] + n[0] * tol, s.start[1] + n[1] * tol], [s.end[0] + n[0] * tol, s.end[1] + n[1] * tol], [s.end[0] - n[0] * tol, s.end[1] - n[1] * tol], [s.start[0] - n[0] * tol, s.start[1] - n[1] * tol]];
    const c = [s.start[0] - u[0] * HEAD_MM * m, s.start[1] - u[1] * HEAD_MM * m], r = HEAD_MM * m;
    const head = [[c[0] - r, c[1] - r], [c[0] + r, c[1] - r], [c[0] + r, c[1] + r], [c[0] - r, c[1] + r]];
    return { ring: band, rings: [band, head] };
  }

  // ---- grips ---------------------------------------------------------------------------------
  const selected = () => {
    const ids = H.selection();
    return ids.length === 1 ? H.sections().find((s) => s.id === ids[0]) : null;
  };

  function down(sx, sy) {
    const s = selected();
    if (!s || s.pinned) return false;
    const b = boxes.find((q) => q.id === s.id && Math.hypot(sx - q.x, sy - q.y) <= q.r + 3);
    if (!b) return false;
    drag = { id: s.id, type: b.type, sx, sy, moved: false, p: null, depth: s.depth, box: b, s };
    return true;
  }

  function dragMove(sx, sy) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(sx - drag.sx, sy - drag.sy) < 4) return;
    drag.moved = true;
    if (drag.type === "end0" || drag.type === "end1") {
      drag.p = H.snap(sx, sy);
    } else if (drag.type === "depth") {
      // distance of the cursor in front of the line, along the side the section looks at
      const w = H.toWorld(sx, sy), s = drag.s;
      const d = (w.x - s.start[0]) * s.look[0] + (w.y - s.start[1]) * s.look[1];
      drag.depth = Math.max(100, Math.round(d / 10) * 10);
      drag.p = w;
    }
    H.render();
  }

  async function up() {
    const d = drag;
    drag = null;
    if (!d) return;
    const patch = {};
    if (d.type === "flip") patch.flipped = !d.s.flipped;
    else if (!d.moved) { H.render(); return; }
    else if (d.type === "end0") patch.start = [r3(d.p.x), r3(d.p.y)];
    else if (d.type === "end1") patch.end = [r3(d.p.x), r3(d.p.y)];
    else if (d.type === "depth") { patch.far_clip_mm = d.depth; if (d.s.far_clip === "no_clip") patch.far_clip = "with_line"; }
    await H.call("patch_section", { id: d.id, patch });
    await H.refresh();
  }

  return { attach, draw, drawPreview, hitRings, down, dragMove, up, grips: () => boxes, get dragging() { return !!drag; } };
})();
