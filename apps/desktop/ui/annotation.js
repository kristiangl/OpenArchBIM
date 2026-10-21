// Annotation families on a canvas: the lines, regions and labels of a level head or a mark, drawn at paper size.
// A family is drawn in paper millimetres round the crossing of its reference lines (x to the right, y up),
// `k` is the number of pixels in a paper millimetre (view scale times zoom), so a symbol keeps its printed size.
// The core sends the drawing as paths (annotation.rs SymbolPaths): fills, then strokes over them.
const Ann = (() => {
  // paper -> pixels. o: { x, y, k, flip, rot }  (flip mirrors about the vertical reference line; rot turns ccw, degrees)
  function mapper(o) {
    const rad = ((o.rot || 0) * Math.PI) / 180, c = Math.cos(rad), s = Math.sin(rad);
    return ([px, py]) => {
      const X = o.flip ? -px : px;
      return { x: o.x + (X * c - py * s) * o.k, y: o.y - (X * s + py * c) * o.k };
    };
  }
  const rgb = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

  // A fill pattern over a region: solid, or its pattern lines (paper mm, anchored to the family's origin and turned
  // with it) clipped to the region.
  function pattern(ctx, f, name, colour, trace, m, o, pats) {
    const def = (pats || {})[name];
    if (!def || def.solid) { trace(); ctx.fillStyle = colour; ctx.fill("evenodd"); return; }
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const r of f.rings) for (const [x, y] of r) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    const corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    ctx.save();
    trace(); ctx.clip("evenodd");
    ctx.strokeStyle = colour; ctx.fillStyle = colour; ctx.lineWidth = Math.max(0.6, f.pen_mm * o.k); ctx.lineCap = "butt";
    let budget = 2000;
    for (const L of def.lines) {
      const a = (L.angle * Math.PI) / 180, dx = Math.cos(a), dy = Math.sin(a), nx = -dy, ny = dx, sp = L.spacing;
      if (sp * o.k < 2) { ctx.globalAlpha *= 0.35; trace(); ctx.fill("evenodd"); break; } // too dense to resolve
      const v0 = L.origin[0] * nx + L.origin[1] * ny, u0 = L.origin[0] * dx + L.origin[1] * dy;
      const vs = corners.map(([x, y]) => x * nx + y * ny), us = corners.map(([x, y]) => x * dx + y * dy);
      const k0 = Math.ceil((Math.min(...vs) - v0) / sp), k1 = Math.floor((Math.max(...vs) - v0) / sp), umin = Math.min(...us), umax = Math.max(...us);
      budget -= k1 - k0 + 1;
      if (budget < 0) break;
      const dash = (L.dashes || []).map((d) => Math.max(d * o.k, 0.6)), period = dash.reduce((t, d) => t + d, 0);
      ctx.setLineDash(dash);
      for (let n = k0; n <= k1; n++) {
        const v = v0 + n * sp, p0 = m([umin * dx + v * nx, umin * dy + v * ny]), p1 = m([umax * dx + v * nx, umax * dy + v * ny]);
        if (period) ctx.lineDashOffset = ((((umin - (u0 + n * (L.shift || 0))) * o.k) % period) + period) % period;
        ctx.beginPath(); ctx.moveTo(p0.x, p0.y); ctx.lineTo(p1.x, p1.y); ctx.stroke();
      }
    }
    ctx.restore();
  }

  // A symbol's paths. `col` draws everything in one colour (a selected or highlighted symbol), otherwise each path
  // keeps its own. `bg` is the view's background, which masking paints. opt: { pats (fill pattern definitions),
  // widen (line width factor), hidden (show what a project leaves out, greyed), only (a test on each path) }.
  function paths(ctx, p, o, col, bg, opt = {}) {
    if (!p) return;
    const m = mapper(o), widen = opt.widen || 1, grey = "#9aa0a8";
    const keep = (x) => (!x.hidden || opt.hidden) && (!opt.only || opt.only(x));
    ctx.save();
    ctx.lineJoin = "round"; ctx.lineCap = "round";
    for (const f of p.fills || []) {
      if (!keep(f)) continue;
      const trace = () => {
        ctx.beginPath();
        for (const r of f.rings) { r.forEach((q, i) => { const s = m(q); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); }); ctx.closePath(); }
      };
      ctx.globalAlpha = f.hidden ? 0.35 : 1;
      if (f.masking && bg) { trace(); ctx.fillStyle = bg; ctx.fill("evenodd"); }
      if (f.background) pattern(ctx, f, f.background, col || rgb(f.background_color), trace, m, o, opt.pats);
      if (f.foreground) pattern(ctx, f, f.foreground, col || rgb(f.foreground_color), trace, m, o, opt.pats);
    }
    ctx.globalAlpha = 1;
    for (const s of p.strokes || []) {
      if (!keep(s) || s.pts.length < 2) continue;
      ctx.beginPath();
      s.pts.forEach((q, i) => { const a = m(q); i ? ctx.lineTo(a.x, a.y) : ctx.moveTo(a.x, a.y); });
      if (s.closed) ctx.closePath();
      ctx.strokeStyle = col || (s.hidden ? grey : rgb(s.color));
      ctx.lineWidth = Math.max(1, s.width_mm * o.k) * widen;
      ctx.setLineDash(s.hidden ? [4, 3] : (s.dash || []).map((d) => Math.max(d * o.k, 0.8)));
      ctx.stroke();
    }
    ctx.restore();
  }

  // labels: each is { x, y, halign, valign, size_mm, bold, lines: [string] } (lines already worked out)
  function labels(ctx, list, o, color) {
    ctx.save();
    ctx.fillStyle = color; ctx.textBaseline = "top";
    for (const lb of list || []) {
      const lines = lb.lines || [];
      if (!lines.length) continue;
      const px = lb.size_mm * o.k;
      if (px < 3) continue;
      const p = mapper({ ...o, rot: 0 })([lb.x, lb.y]);
      ctx.font = `${lb.bold ? "600 " : ""}${px}px system-ui, sans-serif`;
      const h = px * 1.2, total = h * lines.length;
      let ha = lb.halign || "left";
      if (o.flip) ha = ha === "left" ? "right" : ha === "right" ? "left" : ha;
      ctx.textAlign = ha;
      const top = lb.valign === "top" ? p.y : lb.valign === "bottom" ? p.y - total : p.y - total / 2;
      lines.forEach((t, i) => ctx.fillText(t, p.x, top + i * h + (h - px) / 2));
    }
    ctx.restore();
  }

  // a level head: its paths then its labels (in the head's colour, `color` when it is selected)
  function draw(ctx, head, o, color, bg, opt = {}) {
    paths(ctx, head.paths, o, opt.own ? null : color, bg, opt);
    labels(ctx, head.labels, o, color);
  }

  // paper-mm bounds of a drawing: { paths (or body and pointer), labels }, labels measured roughly (0.6 em a character)
  function extent(fam) {
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    const add = (x, y) => { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); };
    for (const p of [fam.paths, fam.body, fam.pointer]) {
      if (!p) continue;
      for (const f of p.fills || []) for (const r of f.rings) for (const q of r) add(...q);
      for (const s of p.strokes || []) for (const q of s.pts) add(...q);
    }
    for (const lb of fam.labels || []) {
      const lines = lb.lines || [];
      const text = lines.map((l) => (typeof l === "string" ? l : `${l.prefix || ""}${l.sample || ""}${l.suffix || ""}`));
      const w = Math.max(0, ...text.map((t) => t.length)) * lb.size_mm * 0.6, h = Math.max(1, text.length) * lb.size_mm * 1.2;
      const lx = lb.halign === "right" ? lb.x - w : lb.halign === "center" ? lb.x - w / 2 : lb.x;
      const ly = lb.valign === "top" ? lb.y - h : lb.valign === "bottom" ? lb.y : lb.y - h / 2;
      add(lx, ly); add(lx + w, ly + h);
    }
    if (!Number.isFinite(x0)) return { minX: 0, maxX: 0, minY: 0, maxY: 0 };
    return { minX: x0, maxX: x1, minY: y0, maxY: y1 };
  }

  return { paths, labels, draw, extent, mapper, rgb };
})();
