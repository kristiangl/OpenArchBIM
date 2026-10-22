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
  function pattern(ctx, f, name, colour, trace, m, o, pats, modelScale) {
    const def = (pats || {})[name];
    if (!def || def.solid) { trace(); ctx.fillStyle = colour; ctx.fill("evenodd"); return; }
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const r of f.rings) for (const [x, y] of r) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    const corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    ctx.save();
    trace(); ctx.clip("evenodd");
    ctx.strokeStyle = colour; ctx.fillStyle = colour; ctx.lineWidth = Math.max(0.6, f.pen_mm * o.k); ctx.lineCap = "butt";
    let budget = 2000;
    // a model pattern's sizes are model mm: over the view's scale they are paper mm, as the drawing is
    const u = def.model ? 1 / (modelScale || 1) : 1;
    for (const L of def.lines) {
      const a = (L.angle * Math.PI) / 180, dx = Math.cos(a), dy = Math.sin(a), nx = -dy, ny = dx, sp = L.spacing * u;
      if (sp * o.k < 2) { ctx.globalAlpha *= 0.35; trace(); ctx.fill("evenodd"); break; } // too dense to resolve
      const v0 = (L.origin[0] * nx + L.origin[1] * ny) * u, u0 = (L.origin[0] * dx + L.origin[1] * dy) * u;
      const vs = corners.map(([x, y]) => x * nx + y * ny), us = corners.map(([x, y]) => x * dx + y * dy);
      const k0 = Math.ceil((Math.min(...vs) - v0) / sp), k1 = Math.floor((Math.max(...vs) - v0) / sp), umin = Math.min(...us), umax = Math.max(...us);
      budget -= k1 - k0 + 1;
      if (budget < 0) break;
      const dash = (L.dashes || []).map((d) => Math.max(d * u * o.k, 0.6)), period = dash.reduce((t, d) => t + d, 0);
      ctx.setLineDash(dash);
      for (let n = k0; n <= k1; n++) {
        const v = v0 + n * sp, p0 = m([umin * dx + v * nx, umin * dy + v * ny]), p1 = m([umax * dx + v * nx, umax * dy + v * ny]);
        if (period) ctx.lineDashOffset = ((((umin - (u0 + n * (L.shift || 0) * u)) * o.k) % period) + period) % period;
        ctx.beginPath(); ctx.moveTo(p0.x, p0.y); ctx.lineTo(p1.x, p1.y); ctx.stroke();
      }
    }
    ctx.restore();
  }

  // A symbol's paths. `col` draws everything in one colour (a selected or highlighted symbol), otherwise each path
  // keeps its own. `bg` is the view's background, which masking paints. opt: { pats (fill pattern definitions),
  // widen (line width factor), hidden (show what a project leaves out, greyed), only (a test on each path), thin (every
  // line one pixel, the view's Thin Lines), modelScale (a view's scale, for model patterns) }.
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
      if (f.background) pattern(ctx, f, f.background, col || rgb(f.background_color), trace, m, o, opt.pats, opt.modelScale);
      if (f.foreground) pattern(ctx, f, f.foreground, col || rgb(f.foreground_color), trace, m, o, opt.pats, opt.modelScale);
    }
    ctx.globalAlpha = 1;
    for (const s of p.strokes || []) {
      if (!keep(s) || s.pts.length < 2) continue;
      ctx.beginPath();
      s.pts.forEach((q, i) => { const a = m(q); i ? ctx.lineTo(a.x, a.y) : ctx.moveTo(a.x, a.y); });
      if (s.closed) ctx.closePath();
      ctx.strokeStyle = col || (s.hidden ? grey : rgb(s.color));
      ctx.lineWidth = (opt.thin ? 1 : Math.max(1, s.width_mm * o.k)) * widen;
      ctx.setLineDash(s.hidden ? [4, 3] : (s.dash || []).map((d) => Math.max(d * o.k, 0.8)));
      ctx.stroke();
    }
    ctx.restore();
  }

  // ---- labels and text (annotation.rs DrawnText): lines of pieces, wrapped in a box with the canvas's fonts
  const measure = document.createElement("canvas").getContext("2d");
  const fontOf = (st, px) => `${st.italic ? "italic " : ""}${st.bold ? "600 " : ""}${px}px ${st.font ? `"${st.font}", ` : ""}system-ui, sans-serif`;
  // Wrap a text's lines at its box width (between words, or only between a label's parameters) and size the box, in
  // paper mm: { lines, w, h, x0 (the box's left edge), top, lh (line spacing), wid (width of a string) }. The anchor
  // (x, y) is the box's left edge, middle or right edge and its top, middle or bottom, as the alignment says.
  function layout(t) {
    const st = t.style, s = st.size_mm, wf = st.width_factor || 1, f = fontOf(st, s * 10);
    const wid = (str) => { measure.font = f; return (measure.measureText(str).width / 10) * wf; };
    const out = [];
    for (const pieces of t.lines || []) {
      const atoms = t.wrap === "pieces" ? pieces : pieces.join("").split(/(?<= )/);
      if (!(t.width_mm > 0)) { out.push(atoms.join("").trimEnd()); continue; }
      let cur = "";
      for (const a of atoms) {
        if (cur && wid((cur + a).trimEnd()) > t.width_mm + 1e-6) { out.push(cur.trimEnd()); cur = a.trimStart(); }
        else cur += a;
      }
      out.push(cur.trimEnd());
    }
    const lh = s * 1.2, w = t.width_mm > 0 ? t.width_mm : Math.max(0, ...out.map(wid)), h = Math.max(1, out.length) * lh;
    const x0 = t.halign === "right" ? t.x - w : t.halign === "center" ? t.x - w / 2 : t.x;
    const top = t.valign === "top" ? t.y : t.valign === "bottom" ? t.y + h : t.y + h / 2;
    return { lines: out, w, h, x0, top, lh, wid };
  }

  // Labels and text, upright whatever the symbol's turn. `color` writes them all in one colour (selected), otherwise
  // each has its type's. opt: { bg (the view's background, for opaque text), hidden (show what a project leaves out) }.
  function labels(ctx, list, o, color, opt = {}) {
    const m = mapper({ ...o, rot: 0 });
    for (const t of list || []) {
      if (t.hidden && !opt.hidden) continue;
      const st = t.style, px = st.size_mm * o.k;
      if (px < 3) continue;
      const L = layout(t);
      // the box on screen; a flip mirrors it about the vertical reference line
      const left = o.flip ? m([L.x0 + L.w, 0]).x : m([L.x0, 0]).x, top = m([0, L.top]).y, W = L.w * o.k, H = L.h * o.k;
      const off = (st.border_offset_mm || 0) * o.k, ink = color || rgb(st.color);
      ctx.save();
      ctx.globalAlpha = t.hidden ? 0.4 : 1;
      if (st.opaque && opt.bg) { ctx.fillStyle = opt.bg; ctx.fillRect(left - off, top - off, W + 2 * off, H + 2 * off); }
      if (st.show_border) { ctx.strokeStyle = ink; ctx.lineWidth = Math.max(1, st.line_mm * o.k); ctx.setLineDash([]); ctx.strokeRect(left - off, top - off, W + 2 * off, H + 2 * off); }
      ctx.fillStyle = ink; ctx.strokeStyle = ink; ctx.font = fontOf(st, px); ctx.textBaseline = "top"; ctx.textAlign = "left";
      let ha = t.halign || "left";
      if (o.flip) ha = ha === "left" ? "right" : ha === "right" ? "left" : ha;
      L.lines.forEach((line, i) => {
        const y = top + (i * L.lh + (L.lh - st.size_mm) / 2) * o.k, lw = L.wid(line) * o.k;
        const x = ha === "left" ? left : ha === "right" ? left + W - lw : left + (W - lw) / 2;
        ctx.save(); ctx.translate(x, y); ctx.scale(st.width_factor || 1, 1); ctx.fillText(line, 0, 0); ctx.restore();
        if (st.underline) { ctx.lineWidth = Math.max(1, px * 0.06); ctx.beginPath(); ctx.moveTo(x, y + px * 1.02); ctx.lineTo(x + lw, y + px * 1.02); ctx.stroke(); }
      });
      ctx.restore();
    }
  }

  // a level head: its paths then its labels (in the head's colour, `color` when it is selected)
  function draw(ctx, head, o, color, bg, opt = {}) {
    paths(ctx, head.paths, o, opt.own ? null : color, bg, opt);
    labels(ctx, head.labels, o, opt.own ? null : color, { ...opt, bg });
  }

  // paper-mm bounds of a drawing: { paths (or body and pointer), labels (DrawnText) }
  function extent(fam) {
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    const add = (x, y) => { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); };
    for (const p of [fam.paths, fam.body, fam.pointer]) {
      if (!p) continue;
      for (const f of p.fills || []) for (const r of f.rings) for (const q of r) add(...q);
      for (const s of p.strokes || []) for (const q of s.pts) add(...q);
    }
    for (const t of fam.labels || []) { const L = layout(t); add(L.x0, L.top - L.h); add(L.x0 + L.w, L.top); }
    if (!Number.isFinite(x0)) return { minX: 0, maxX: 0, minY: 0, maxY: 0 };
    return { minX: x0, maxX: x1, minY: y0, maxY: y1 };
  }

  return { paths, labels, layout, draw, extent, mapper, rgb };
})();
