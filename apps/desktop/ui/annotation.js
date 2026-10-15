// Annotation families on a canvas: the shapes and labels of a level head or a marker, drawn at paper size.
// A family is drawn in paper millimetres round the crossing of its reference lines (x to the right, y up),
// `k` is the number of pixels in a paper millimetre (view scale times zoom), so a symbol keeps its printed size.
const Ann = (() => {
  // paper -> pixels. o: { x, y, k, flip, rot }  (flip mirrors about the vertical reference line; rot turns ccw, degrees)
  function mapper(o) {
    const rad = ((o.rot || 0) * Math.PI) / 180, c = Math.cos(rad), s = Math.sin(rad);
    return ([px, py]) => {
      const X = o.flip ? -px : px;
      return { x: o.x + (X * c - py * s) * o.k, y: o.y - (X * s + py * c) * o.k };
    };
  }
  const penPx = (fam, o) => Math.max(1, (fam.pen_mm || 0.25) * o.k);

  function shapes(ctx, list, o, color, bg, pen) {
    const m = mapper(o);
    ctx.save();
    ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = pen; ctx.lineJoin = "round"; ctx.lineCap = "round"; ctx.setLineDash([]);
    for (const sh of list || []) {
      ctx.beginPath();
      if (sh.type === "line") {
        const a = m(sh.a), b = m(sh.b);
        ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      } else if (sh.type === "circle") {
        const c = m(sh.c);
        ctx.arc(c.x, c.y, sh.r * o.k, 0, Math.PI * 2);
        if (sh.filled) ctx.fill(); else { if (bg) { ctx.save(); ctx.fillStyle = bg; ctx.fill(); ctx.restore(); } ctx.stroke(); }
      } else if (sh.type === "arc") {
        const n = Math.max(8, Math.ceil(Math.abs(sh.end_deg - sh.start_deg) / 6));
        for (let i = 0; i <= n; i++) {
          const t = ((sh.start_deg + ((sh.end_deg - sh.start_deg) * i) / n) * Math.PI) / 180;
          const p = m([sh.c[0] + sh.r * Math.cos(t), sh.c[1] + sh.r * Math.sin(t)]);
          i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y);
        }
        ctx.stroke();
      } else if (sh.type === "polygon") {
        sh.pts.forEach((p, i) => { const q = m(p); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); });
        ctx.closePath();
        if (sh.filled) ctx.fill(); else { if (bg) { ctx.save(); ctx.fillStyle = bg; ctx.fill(); ctx.restore(); } ctx.stroke(); }
      }
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

  // a head or marker: shapes then labels
  function draw(ctx, fam, o, color, bg) {
    shapes(ctx, fam.shapes, o, color, bg, penPx(fam, o));
    labels(ctx, fam.labels, o, color);
  }

  // paper-mm bounds of a family's drawing, labels measured roughly (0.6 em a character)
  function extent(fam) {
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    const add = (x, y) => { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); };
    for (const sh of fam.shapes || []) {
      if (sh.type === "line") { add(...sh.a); add(...sh.b); }
      else if (sh.type === "circle" || sh.type === "arc") { add(sh.c[0] - sh.r, sh.c[1] - sh.r); add(sh.c[0] + sh.r, sh.c[1] + sh.r); }
      else if (sh.type === "polygon") sh.pts.forEach((p) => add(...p));
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

  return { shapes, labels, draw, extent, mapper, penPx };
})();
