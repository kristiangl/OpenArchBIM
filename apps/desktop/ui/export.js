// Export: sheets or views to PDF and DXF, and the model to IFC, in one run. The workflow follows Kristian's own sheet
// exporter: pick the sheets (or views), then the settings (formats, the output folder and file names from templates,
// a tab for each format), then Export. Settings are remembered per user.
//
// Drawing: a sheet is drawn by SheetView.draw (the views on it by Plan.renderTo and Elev.renderTo) onto a recording
// context, a stand-in for a canvas that keeps what is drawn as paths, clips and text in page units. The PDF writer
// turns that into page content (vector lines, fills and Helvetica text); the DXF writer into R12 polylines, lines and
// text on paper in millimetres.
const ExportFlow = (() => {
  const PT = 72 / 25.4; // PDF points in a millimetre
  // The viewers draw no line thinner than one unit, so pages are drawn finely and scaled back: a PDF page at four units
  // to the point, a DXF page at ten to the millimetre.
  const PDF_K = PT * 4, DXF_K = 10;
  const PAPERS = { A0: [1189, 841], A1: [841, 594], A2: [594, 420], A3: [420, 297], A4: [297, 210] };
  const DEFAULTS = {
    folder: "{ProjectDir}\\Exports\\{Date}_",
    dateFormat: "yyMMdd",
    pdf: true, dxf: true, ifc: false,
    pdfName: "{ProjectNumber}_{ProjectName}_{Date}",
    pdfCombine: true, pdfPaper: "default", pdfOrientation: "landscape", pdfZoom: "fit", pdfColour: "colour",
    dxfName: "{ProjectNumber}_{ProjectName}_DXF_{Date}_{SheetNumber}_{SheetName}",
    dxfColours: "index",
    ifcName: "{ProjectNumber}_{ProjectName}_IFC_{Date}",
  };
  const load = () => { try { return { ...DEFAULTS, ...JSON.parse(localStorage.getItem("openarch.export") || "{}") }; } catch { return { ...DEFAULTS }; } };
  const store = (s) => { try { localStorage.setItem("openarch.export", JSON.stringify(s)); } catch { /* storage can be unavailable */ } };

  // ---- the recording context ---------------------------------------------------------------------------------------
  const measurer = document.createElement("canvas").getContext("2d");
  function parseColour(c, alpha) {
    if (typeof c !== "string") return null;
    let r = 0, g = 0, b = 0, a = 1;
    const s = c.trim();
    if (s[0] === "#") {
      const h = s.length === 4 ? [...s.slice(1)].map((x) => x + x).join("") : s.slice(1, 7);
      [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    } else if (/^rgba?\(/.test(s)) {
      const v = s.replace(/^rgba?\(|\)$/g, "").split(",").map((x) => parseFloat(x));
      [r, g, b] = v; a = v.length > 3 ? v[3] : 1;
    } else if (s === "white") [r, g, b] = [255, 255, 255];
    else if (s === "transparent") return null;
    a *= alpha;
    if (a <= 0.01) return null;
    // see-through over white paper
    return [r, g, b].map((x) => Math.round(255 - (255 - x) * a));
  }
  class Recorder {
    constructor(w, h) {
      this.canvas = { width: w, height: h, clientWidth: w, clientHeight: h };
      this.ops = [];
      this.st = { m: [1, 0, 0, 1, 0, 0], fillStyle: "#000", strokeStyle: "#000", lineWidth: 1, dash: [], font: "10px sans-serif", textAlign: "start", textBaseline: "alphabetic", globalAlpha: 1, lineCap: "butt", lineJoin: "miter", lineDashOffset: 0 };
      this.stack = [];
      this.path = [];
    }
    // state
    save() { this.stack.push({ ...this.st, m: [...this.st.m], dash: [...this.st.dash] }); this.ops.push({ t: "save" }); }
    restore() { if (this.stack.length) { this.st = this.stack.pop(); this.ops.push({ t: "restore" }); } }
    get fillStyle() { return this.st.fillStyle; } set fillStyle(v) { this.st.fillStyle = v; }
    get strokeStyle() { return this.st.strokeStyle; } set strokeStyle(v) { this.st.strokeStyle = v; }
    get lineWidth() { return this.st.lineWidth; } set lineWidth(v) { this.st.lineWidth = v; }
    get font() { return this.st.font; } set font(v) { this.st.font = v; }
    get textAlign() { return this.st.textAlign; } set textAlign(v) { this.st.textAlign = v; }
    get textBaseline() { return this.st.textBaseline; } set textBaseline(v) { this.st.textBaseline = v; }
    get globalAlpha() { return this.st.globalAlpha; } set globalAlpha(v) { this.st.globalAlpha = v; }
    get lineCap() { return this.st.lineCap; } set lineCap(v) { this.st.lineCap = v; }
    get lineJoin() { return this.st.lineJoin; } set lineJoin(v) { this.st.lineJoin = v; }
    get lineDashOffset() { return this.st.lineDashOffset; } set lineDashOffset(v) { this.st.lineDashOffset = v; }
    setLineDash(d) { this.st.dash = [...(d || [])]; }
    getLineDash() { return [...this.st.dash]; }
    // transforms
    setTransform(a, b, c, d, e, f) { this.st.m = [a, b, c, d, e, f]; }
    resetTransform() { this.st.m = [1, 0, 0, 1, 0, 0]; }
    transform(a, b, c, d, e, f) { const [A, B, C, D, E, F] = this.st.m; this.st.m = [A * a + C * b, B * a + D * b, A * c + C * d, B * c + D * d, A * e + C * f + E, B * e + D * f + F]; }
    translate(x, y) { this.transform(1, 0, 0, 1, x, y); }
    scale(x, y) { this.transform(x, 0, 0, y, 0, 0); }
    rotate(a) { const c = Math.cos(a), s = Math.sin(a); this.transform(c, s, -s, c, 0, 0); }
    pt(x, y) { const [a, b, c, d, e, f] = this.st.m; return [a * x + c * y + e, b * x + d * y + f]; }
    get k() { const [a, b, c, d] = this.st.m; return Math.sqrt(Math.abs(a * d - b * c)); }
    // paths, kept in page units
    beginPath() { this.path = []; }
    moveTo(x, y) { this.path.push({ pts: [this.pt(x, y)], closed: false }); }
    lineTo(x, y) { if (!this.path.length) this.moveTo(x, y); else this.path[this.path.length - 1].pts.push(this.pt(x, y)); }
    closePath() { const s = this.path[this.path.length - 1]; if (s) { s.closed = true; this.path.push({ pts: [s.pts[0]], closed: false }); } }
    rect(x, y, w, h) { this.moveTo(x, y); this.lineTo(x + w, y); this.lineTo(x + w, y + h); this.lineTo(x, y + h); this.closePath(); }
    arc(x, y, r, a0, a1, ccw) { this.ellipse(x, y, r, r, 0, a0, a1, ccw); }
    ellipse(x, y, rx, ry, rot, a0, a1, ccw) {
      let sweep = a1 - a0;
      if (!ccw && sweep < 0) sweep += Math.PI * 2 * Math.ceil(-sweep / (Math.PI * 2));
      if (ccw && sweep > 0) sweep -= Math.PI * 2 * Math.ceil(sweep / (Math.PI * 2));
      if (Math.abs(a1 - a0) >= Math.PI * 2 - 1e-9) sweep = ccw ? -Math.PI * 2 : Math.PI * 2;
      const n = Math.max(8, Math.ceil(Math.abs(sweep) / (Math.PI / 24)));
      const cr = Math.cos(rot), sr = Math.sin(rot);
      for (let i = 0; i <= n; i++) {
        const t = a0 + (sweep * i) / n, ex = rx * Math.cos(t), ey = ry * Math.sin(t);
        const px = x + ex * cr - ey * sr, py = y + ex * sr + ey * cr;
        if (i === 0 && this.path.length && this.path[this.path.length - 1].pts.length) this.lineTo(px, py);
        else if (i === 0) this.moveTo(px, py); else this.lineTo(px, py);
      }
    }
    arcTo(x1, y1, x2, y2) { this.lineTo(x1, y1); this.lineTo(x2, y2); }
    quadraticCurveTo(cx, cy, x, y) { this.bezierCurveTo(cx, cy, cx, cy, x, y); }
    bezierCurveTo(c1x, c1y, c2x, c2y, x, y) {
      const s = this.path[this.path.length - 1]; if (!s) return;
      const [a, b, c, d, e, f] = this.st.m, det = a * d - b * c;
      const p0 = s.pts[s.pts.length - 1], X = (px, py) => [(d * (px - e) - c * (py - f)) / det, (-b * (px - e) + a * (py - f)) / det];
      const [x0, y0] = X(p0[0], p0[1]);
      for (let i = 1; i <= 12; i++) {
        const t = i / 12, u = 1 - t;
        this.lineTo(u * u * u * x0 + 3 * u * u * t * c1x + 3 * u * t * t * c2x + t * t * t * x, u * u * u * y0 + 3 * u * u * t * c1y + 3 * u * t * t * c2y + t * t * t * y);
      }
    }
    subpaths() { return this.path.filter((s) => s.pts.length > 1).map((s) => ({ pts: s.pts.map((p) => [...p]), closed: s.closed })); }
    fill(rule) { const c = parseColour(this.st.fillStyle, this.st.globalAlpha); if (c) this.ops.push({ t: "fill", subs: this.subpaths(), colour: c, evenodd: rule === "evenodd" }); }
    stroke() {
      const c = parseColour(this.st.strokeStyle, this.st.globalAlpha);
      if (c) this.ops.push({ t: "stroke", subs: this.subpaths(), colour: c, width: this.st.lineWidth * this.k, dash: this.st.dash.map((x) => x * this.k), cap: this.st.lineCap, join: this.st.lineJoin });
    }
    clip(rule) { this.ops.push({ t: "clip", subs: this.subpaths(), evenodd: rule === "evenodd" }); }
    fillRect(x, y, w, h) { const keep = this.path; this.beginPath(); this.rect(x, y, w, h); this.fill(); this.path = keep; }
    strokeRect(x, y, w, h) { const keep = this.path; this.beginPath(); this.rect(x, y, w, h); this.stroke(); this.path = keep; }
    clearRect() {}
    // text, as Helvetica in the page: where its baseline starts, which way it runs, how big
    measureText(t) { measurer.font = this.st.font; return measurer.measureText(t); }
    fillText(text, x, y) {
      const c = parseColour(this.st.fillStyle, this.st.globalAlpha);
      if (!c || text == null || text === "") return;
      const f = this.st.font, size = parseFloat((f.match(/([\d.]+)px/) || [0, 10])[1]);
      const bold = /\b(bold|[6-9]00)\b/.test(f), italic = /\bitalic\b/.test(f);
      const w = this.measureText(String(text)).width;
      const al = this.st.textAlign, bl = this.st.textBaseline;
      const dx = al === "center" ? -w / 2 : al === "right" || al === "end" ? -w : 0;
      const dy = bl === "top" || bl === "hanging" ? size * 0.78 : bl === "middle" ? size * 0.3 : bl === "bottom" || bl === "ideographic" ? -size * 0.22 : 0;
      const [a, b, cc, d] = this.st.m;
      const o = this.pt(x + dx, y + dy);
      const sx = Math.hypot(a, b), sy = Math.hypot(cc, d);
      this.ops.push({ t: "text", text: String(text), x: o[0], y: o[1], size: size * sy, stretch: sy ? sx / sy : 1, angle: Math.atan2(b, a), bold, italic, colour: c, width: w * sx });
    }
    strokeText(text, x, y) { this.fillText(text, x, y); }
    createPattern() { return null; }
    drawImage() {}
  }

  // the sheets or views as pages: { name, number, paper:[w,h] mm, ops (page points, y down) }
  async function pageOf(item, k) {
    const s = item.kind === "sheet" ? await Backend.call("sheet", { id: item.id }) : await Backend.call("view_print", { view_id: item.id });
    const data = new Map();
    for (const v of s.views) {
      try { data.set(v.view, await Backend.call(v.drawing === "plan" ? "plan" : "elevation", { view_id: v.view })); } catch { /* drawn empty */ }
    }
    const [pw, ph] = s.paper;
    const rec = new Recorder(pw * k, ph * k);
    SheetView.draw(rec, s, data, (x, y) => ({ x: x * k, y: (ph - y) * k }), k);
    return { paper: s.paper, ops: rec.ops };
  }

  // ---- PDF -----------------------------------------------------------------------------------------------------------
  const num = (v) => (Math.round(v * 1000) / 1000).toString();
  function grey(c, mode) {
    if (mode === "colour") return c;
    const l = Math.round(0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]);
    if (mode === "grey") return [l, l, l];
    return l > 250 ? [255, 255, 255] : [0, 0, 0];
  }
  const rgb = (c) => c.map((x) => num(x / 255)).join(" ");
  const pdfText = (s) => "(" + [...s].map((ch) => { const n = ch.charCodeAt(0); if (ch === "(" || ch === ")" || ch === "\\") return "\\" + ch; return n < 32 || n > 255 ? "?" : ch; }).join("") + ")";
  // one page's content: page points with y up (`h` tall), offset and scaled for the paper it goes on
  function pdfContent(ops, h, place, mode) {
    const out = [`q ${num(place.s)} 0 0 ${num(place.s)} ${num(place.x)} ${num(place.y)} cm`];
    const P = (p) => `${num(p[0])} ${num(h - p[1])}`;
    const path = (subs) => subs.map((s) => `${P(s.pts[0])} m ${s.pts.slice(1).map((p) => `${P(p)} l`).join(" ")}${s.closed ? " h" : ""}`).join(" ");
    for (const o of ops) {
      if (o.t === "save") out.push("q");
      else if (o.t === "restore") out.push("Q");
      else if (o.t === "clip") out.push(`${path(o.subs) || "0 0 m"} ${o.evenodd ? "W*" : "W"} n`);
      else if (o.t === "fill" && o.subs.length) out.push(`${rgb(grey(o.colour, mode))} rg ${path(o.subs)} ${o.evenodd ? "f*" : "f"}`);
      else if (o.t === "stroke" && o.subs.length) {
        const cap = o.cap === "round" ? 1 : o.cap === "square" ? 2 : 0, join = o.join === "round" ? 1 : o.join === "bevel" ? 2 : 0;
        out.push(`${rgb(grey(o.colour, mode))} RG ${num(Math.max(o.width, 0.1))} w ${cap} J ${join} j [${o.dash.map(num).join(" ")}] 0 d ${path(o.subs)} S`);
      } else if (o.t === "text") {
        const c = Math.cos(o.angle), s = Math.sin(o.angle);
        // the baseline runs (c, -s) in the page (y up); the glyphs stand up at right angles to it
        out.push(`BT /${o.bold ? "F2" : "F1"} 1 Tf ${rgb(grey(o.colour, mode))} rg ${num(c * o.size * o.stretch)} ${num(-s * o.size * o.stretch)} ${num(s * o.size)} ${num(c * o.size)} ${P([o.x, o.y])} Tm ${pdfText(o.text)} Tj ET`);
      }
    }
    out.push("Q");
    return out.join("\n");
  }
  async function deflate(bytes) {
    if (typeof CompressionStream === "undefined") return null;
    const cs = new CompressionStream("deflate");
    const buf = await new Response(new Blob([bytes]).stream().pipeThrough(cs)).arrayBuffer();
    return new Uint8Array(buf);
  }
  // a PDF of pages: each { paper (mm), ops (points), place }
  async function pdf(pages, set) {
    const enc = new TextEncoder();
    const objs = []; // byte arrays, numbered from 1
    const add = (parts) => { objs.push(parts); return objs.length; };
    const fonts = [add([enc.encode("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")]), add([enc.encode("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>")])];
    const pagesId = objs.length + 1; objs.push(null);
    const kids = [];
    for (const pg of pages) {
      const [W, H] = pageSize(pg.paper, set).map((x) => x * PT);
      const sc = pageScale(pg.paper, set);
      const place = { s: (sc * PT) / PDF_K, x: (W - pg.paper[0] * PT * sc) / 2, y: (H - pg.paper[1] * PT * sc) / 2 };
      const raw = enc.encode(pdfContent(pg.ops, pg.paper[1] * PDF_K, place, set.pdfColour));
      const z = await deflate(raw);
      const body = z || raw;
      const content = add([enc.encode(`<< /Length ${body.length}${z ? " /Filter /FlateDecode" : ""} >>\nstream\n`), body, enc.encode("\nendstream")]);
      kids.push(add([enc.encode(`<< /Type /Page /Parent ${pagesId} 0 R /MediaBox [0 0 ${num(W)} ${num(H)}] /Resources << /Font << /F1 ${fonts[0]} 0 R /F2 ${fonts[1]} 0 R >> >> /Contents ${content} 0 R >>`)]));
    }
    objs[pagesId - 1] = [enc.encode(`<< /Type /Pages /Kids [${kids.map((k) => `${k} 0 R`).join(" ")}] /Count ${kids.length} >>`)];
    const catalog = add([enc.encode(`<< /Type /Catalog /Pages ${pagesId} 0 R >>`)]);
    const info = add([enc.encode(`<< /Producer (OpenArch BIM) /Creator (OpenArch BIM) >>`)]);
    const chunks = [enc.encode("%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")];
    let at = chunks[0].length;
    const offs = [];
    objs.forEach((parts, i) => {
      offs.push(at);
      const head = enc.encode(`${i + 1} 0 obj\n`), tail = enc.encode("\nendobj\n");
      for (const p of [head, ...parts, tail]) { chunks.push(p); at += p.length; }
    });
    const xref = [`xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`, ...offs.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`)].join("");
    chunks.push(enc.encode(`${xref}trailer\n<< /Size ${objs.length + 1} /Root ${catalog} 0 R /Info ${info} 0 R >>\nstartxref\n${at}\n%%EOF\n`));
    const total = chunks.reduce((n, c) => n + c.length, 0), outb = new Uint8Array(total);
    let p = 0; for (const c of chunks) { outb.set(c, p); p += c.length; }
    return outb;
  }
  function pageSize(paper, set) {
    if (set.pdfPaper === "default" || !PAPERS[set.pdfPaper]) return paper;
    const [a, b] = PAPERS[set.pdfPaper];
    return set.pdfOrientation === "portrait" ? [b, a] : [a, b];
  }
  function pageScale(paper, set) {
    if (set.pdfPaper === "default" || set.pdfZoom !== "fit") return set.pdfPaper === "default" ? 1 : (Number(set.pdfZoomPct) || 100) / 100;
    const [W, H] = pageSize(paper, set);
    return Math.min(W / paper[0], H / paper[1]);
  }

  // ---- DXF (R12, millimetres on paper) -------------------------------------------------------------------------------
  const ACI = [[1, [255, 0, 0]], [2, [255, 255, 0]], [3, [0, 255, 0]], [4, [0, 255, 255]], [5, [0, 0, 255]], [6, [255, 0, 255]], [8, [128, 128, 128]], [9, [192, 192, 192]], [7, [0, 0, 0]]];
  const aci = (c) => { const l = 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]; if (l < 60) return 7; let best = 7, bd = Infinity; for (const [i, q] of ACI) { const d = (c[0] - q[0]) ** 2 + (c[1] - q[1]) ** 2 + (c[2] - q[2]) ** 2; if (d < bd) { bd = d; best = i; } } return best; };
  // a polyline cut to a rectangle (the viewport it is drawn in): its pieces inside
  function clipPolyline(pts, r) {
    if (!r) return [pts];
    const out = []; let cur = [];
    const inside = (p) => p[0] >= r[0] - 1e-6 && p[0] <= r[2] + 1e-6 && p[1] >= r[1] - 1e-6 && p[1] <= r[3] + 1e-6;
    for (let i = 0; i + 1 < pts.length; i++) {
      let [x0, y0] = pts[i], [x1, y1] = pts[i + 1];
      let t0 = 0, t1 = 1; const dx = x1 - x0, dy = y1 - y0;
      let ok = true;
      for (const [p, q] of [[-dx, x0 - r[0]], [dx, r[2] - x0], [-dy, y0 - r[1]], [dy, r[3] - y0]]) {
        if (Math.abs(p) < 1e-12) { if (q < 0) { ok = false; break; } continue; }
        const t = q / p;
        if (p < 0) { if (t > t1) { ok = false; break; } if (t > t0) t0 = t; } else { if (t < t0) { ok = false; break; } if (t < t1) t1 = t; }
      }
      if (!ok) { if (cur.length > 1) out.push(cur); cur = []; continue; }
      const a = [x0 + dx * t0, y0 + dy * t0], b = [x0 + dx * t1, y0 + dy * t1];
      if (!cur.length || t0 > 1e-9) { if (cur.length > 1) out.push(cur); cur = [a]; }
      cur.push(b);
      if (t1 < 1 - 1e-9) { out.push(cur); cur = []; }
    }
    if (cur.length > 1) out.push(cur);
    return out.filter((c) => inside(c[0]) || c.length > 1);
  }
  function dxf(page, set) {
    const H = page.paper[1];
    const L = [];
    const g = (code, v) => L.push(String(code), String(v));
    // page units (DXF_K to the millimetre, y down) to paper millimetres, y up
    const X = (p) => [Math.round((p[0] / DXF_K) * 1000) / 1000, Math.round((H - p[1] / DXF_K) * 1000) / 1000];
    // the clip rectangles in force (axis-aligned boxes of the clip paths), as a stack with save and restore
    const clips = [null];
    const box = (subs) => { const ps = subs.flatMap((s) => s.pts); return [Math.min(...ps.map((p) => p[0])), Math.min(...ps.map((p) => p[1])), Math.max(...ps.map((p) => p[0])), Math.max(...ps.map((p) => p[1]))]; };
    const meet = (a, b) => (!a ? b : !b ? a : [Math.max(a[0], b[0]), Math.max(a[1], b[1]), Math.min(a[2], b[2]), Math.min(a[3], b[3])]);
    g(0, "SECTION"); g(2, "HEADER"); g(9, "$ACADVER"); g(1, "AC1009"); g(9, "$INSUNITS"); g(70, 4); g(9, "$EXTMIN"); g(10, 0); g(20, 0); g(9, "$EXTMAX"); g(10, page.paper[0]); g(20, H); g(0, "ENDSEC");
    g(0, "SECTION"); g(2, "TABLES"); g(0, "TABLE"); g(2, "LAYER"); g(70, 2);
    for (const [name, col] of [["OA-LINES", 7], ["OA-TEXT", 7]]) { g(0, "LAYER"); g(2, name); g(70, 0); g(62, col); g(6, "CONTINUOUS"); }
    g(0, "ENDTAB"); g(0, "ENDSEC");
    g(0, "SECTION"); g(2, "ENTITIES");
    for (const o of page.ops) {
      if (o.t === "save") clips.push(clips[clips.length - 1]);
      else if (o.t === "restore") { if (clips.length > 1) clips.pop(); }
      else if (o.t === "clip") clips[clips.length - 1] = meet(clips[clips.length - 1], o.subs.length ? box(o.subs) : null);
      else if (o.t === "stroke") {
        const col = aci(o.colour);
        for (const s of o.subs) {
          const pts = s.closed ? [...s.pts, s.pts[0]] : s.pts;
          for (const piece of clipPolyline(pts, clips[clips.length - 1])) {
            const ps = piece.map(X), wmm = o.width / DXF_K;
            if (ps.length === 2) { g(0, "LINE"); g(8, "OA-LINES"); g(62, col); g(10, ps[0][0]); g(20, ps[0][1]); g(11, ps[1][0]); g(21, ps[1][1]); continue; }
            g(0, "POLYLINE"); g(8, "OA-LINES"); g(62, col); g(66, 1); g(10, 0); g(20, 0); g(70, 0);
            if (wmm >= 0.25) { g(40, Math.round(wmm * 100) / 100); g(41, Math.round(wmm * 100) / 100); }
            for (const p of ps) { g(0, "VERTEX"); g(8, "OA-LINES"); g(10, p[0]); g(20, p[1]); }
            g(0, "SEQEND"); g(8, "OA-LINES");
          }
        }
      } else if (o.t === "text") {
        const r = clips[clips.length - 1];
        if (r && (o.x < r[0] || o.x > r[2] || o.y < r[1] || o.y > r[3])) continue;
        const p = X([o.x, o.y]), h = o.size / DXF_K;
        g(0, "TEXT"); g(8, "OA-TEXT"); g(62, aci(o.colour)); g(10, p[0]); g(20, p[1]); g(40, Math.round(h * 1000) / 1000); g(1, o.text.replace(/[\r\n]/g, " "));
        const ang = (-o.angle * 180) / Math.PI;
        if (Math.abs(ang) > 0.01) g(50, Math.round(ang * 100) / 100);
        if (Math.abs(o.stretch - 1) > 0.01) g(41, Math.round(o.stretch * 1000) / 1000);
      }
    }
    g(0, "ENDSEC"); g(0, "EOF");
    return L.join("\r\n") + "\r\n";
  }

  // ---- templates -----------------------------------------------------------------------------------------------------
  const clean = (s) => String(s || "").replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").trim();
  function date(fmt) {
    const d = new Date(), p = (n, w = 2) => String(n).padStart(w, "0");
    return fmt.replace(/yyyy|yy|MM|dd|HH|mm/g, (t) => ({ yyyy: d.getFullYear(), yy: p(d.getFullYear() % 100), MM: p(d.getMonth() + 1), dd: p(d.getDate()), HH: p(d.getHours()), mm: p(d.getMinutes()) })[t]);
  }
  function fill(template, set, ctx, item) {
    const pr = app.info.project || {};
    let s = template
      .replace(/\{ProjectDir\}/g, ctx.dir)
      .replace(/\{ProjectNumber\}/g, clean(pr.number || "Project"))
      .replace(/\{ProjectName\}/g, clean(pr.name || "Untitled"))
      .replace(/\{Date\}/g, date(set.dateFormat));
    if (item) s = s.replace(/\{SheetNumber\}/g, clean(item.number)).replace(/\{SheetName\}/g, clean(item.name));
    return s.replace(/\{SheetNumber\}|\{SheetName\}/g, "");
  }
  const join = (dir, name) => (/[\\/]$/.test(dir) ? dir + name : `${dir}${dir.includes("/") && !dir.includes("\\") ? "/" : "\\"}${name}`);
  async function projectDir() {
    const p = app.info.path;
    if (p) return p.replace(/[\\/][^\\/]*$/, "");
    try { return (await Backend.call("default_dir", {})).path; } catch { return "."; }
  }
  const toBase64 = (bytes) => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s); };

  // ---- the run -------------------------------------------------------------------------------------------------------
  async function run(items, set, report) {
    const ctx = { dir: await projectDir() };
    const folder = fill(set.folder, set, ctx);
    const written = [];
    const out = async (name, payload) => { const r = await Backend.call("write_file", { path: join(folder, name), ...payload }); written.push(r.path); };
    const pages = [];
    for (const [i, it] of items.entries()) {
      report(`Drawing ${it.number ? `${it.number} ` : ""}${it.name} (${i + 1} of ${items.length})...`);
      pages.push({ item: it, pdf: set.pdf ? await pageOf(it, PDF_K) : null, dxf: set.dxf ? await pageOf(it, DXF_K) : null });
    }
    if (set.pdf) {
      report("Writing PDF...");
      if (set.pdfCombine) await out(`${fill(set.pdfName, set, ctx)}.pdf`, { base64: toBase64(await pdf(pages.map((p) => p.pdf), set)) });
      else for (const p of pages) await out(`${fill(set.pdfName, set, ctx, p.item)}${set.pdfName.includes("{Sheet") ? "" : `_${clean(p.item.number || p.item.name)}`}.pdf`, { base64: toBase64(await pdf([p.pdf], set)) });
    }
    if (set.dxf) {
      report("Writing DXF...");
      for (const p of pages) await out(`${fill(set.dxfName, set, ctx, p.item)}.dxf`, { text: dxf(p.dxf, set) });
    }
    if (set.ifc) {
      report("Writing IFC...");
      const r = await Backend.call("export_ifc", { path: join(folder, `${fill(set.ifcName, set, ctx)}.ifc`) });
      written.push(r.path);
    }
    return { folder, written };
  }

  // ---- the windows ---------------------------------------------------------------------------------------------------
  function selectWindow(onNext) {
    openModal("Export: Select Sheets", (body, foot, close) => {
      let mode = "sheets";
      const picked = new Set();
      const rSheets = el("input", { type: "radio", name: "ex-show", checked: true }), rViews = el("input", { type: "radio", name: "ex-show" });
      const listOnly = el("input", { type: "checkbox", checked: true });
      const search = el("input", { type: "search", placeholder: "Search..." });
      const table = el("div", { className: "ex-list" });
      const count = el("span", { className: "ex-count" });
      const items = () => mode === "sheets"
        ? (app.info.sheets || []).filter((s) => !listOnly.checked || s.appears_in_list).map((s) => ({ kind: "sheet", id: s.id, number: s.number, name: s.name }))
        : app.info.views.filter((v) => ["floor_plan", "ceiling_plan", "elevation", "section"].includes(v.view_kind)).map((v) => ({ kind: "view", id: v.id, number: "", name: v.name, type: v.view_kind.replace("_", " ") }));
      const shown = () => { const q = search.value.trim().toLowerCase(); return items().filter((i) => !q || `${i.number} ${i.name}`.toLowerCase().includes(q)); };
      const key = (i) => `${i.kind}:${i.id}`;
      const exportBtn = button("Export...", () => { const chosen = items().filter((i) => picked.has(key(i))); close(); onNext(chosen); }, true);
      const draw = () => {
        listOnly.disabled = mode !== "sheets";
        const head = el("div", { className: "ex-row ex-head" }, [el("span"), el("span", { textContent: mode === "sheets" ? "Sheet Number" : "View Type" }), el("span", { textContent: mode === "sheets" ? "Sheet Name" : "View Name" })]);
        table.replaceChildren(head, ...shown().map((i) => {
          const cb = el("input", { type: "checkbox", checked: picked.has(key(i)) });
          cb.onchange = () => { if (cb.checked) picked.add(key(i)); else picked.delete(key(i)); draw(); };
          const row = el("label", { className: "ex-row" }, [cb, el("span", { textContent: mode === "sheets" ? i.number : i.type }), el("span", { textContent: i.name })]);
          return row;
        }));
        const n = items().filter((i) => picked.has(key(i))).length;
        count.textContent = `${n} ${mode === "sheets" ? "sheet" : "view"}${n === 1 ? "" : "s"} selected`;
        exportBtn.disabled = n === 0;
      };
      rSheets.onchange = () => { mode = "sheets"; picked.clear(); draw(); };
      rViews.onchange = () => { mode = "views"; picked.clear(); draw(); };
      listOnly.onchange = draw; search.oninput = draw;
      const all = button("Select All", () => { for (const i of shown()) picked.add(key(i)); draw(); });
      const none = button("Deselect All", () => { for (const i of shown()) picked.delete(key(i)); draw(); });
      body.classList.add("ex-body");
      body.append(
        el("div", { className: "ex-bar" }, [el("span", { textContent: "Show:" }), el("label", {}, [rSheets, " Sheets"]), el("label", {}, [rViews, " Views"])]),
        el("div", { className: "ex-bar" }, [el("label", {}, [listOnly, " Show in sheet list only"]), search, all, none]),
        table,
      );
      foot.append(count, el("span", { className: "spacer" }), exportBtn, button("Cancel", close));
      draw();
    }, { wide: true });
  }

  function settingsWindow(items, onDone) {
    const set = load();
    openModal("Export Settings", (body, foot, close) => {
      body.classList.add("ex-body");
      const inp = (key, attrs = {}) => { const i = el("input", { type: "text", value: set[key] ?? "", ...attrs }); i.oninput = () => { set[key] = i.value; preview(); }; return i; };
      const chk = (key, label) => { const c = el("input", { type: "checkbox", checked: !!set[key] }); c.onchange = () => { set[key] = c.checked; tabsDraw(); }; return el("label", {}, [c, ` ${label}`]); };
      const sel = (key, opts) => { const s = el("select", {}, opts.map(([v, l]) => new Option(l, v, false, set[key] === v))); s.onchange = () => { set[key] = s.value; preview(); }; return s; };
      const radio = (key, opts) => el("span", { className: "ex-radios" }, opts.map(([v, l]) => { const r = el("input", { type: "radio", name: `ex-${key}`, checked: set[key] === v }); r.onchange = () => { set[key] = v; preview(); }; return el("label", {}, [r, ` ${l}`]); }));
      const pv = (t) => el("div", { className: "ex-preview", textContent: t });
      const folderPrev = pv(""), pdfPrev = pv(""), dxfPrev = pv(""), ifcPrev = pv("");
      const sample = items[0] || { number: "A1.01", name: "Site Plan" };
      let dir = app.info.path ? app.info.path.replace(/[\\/][^\\/]*$/, "") : "Documents";
      const preview = () => {
        const c = { dir };
        folderPrev.textContent = `Preview: ${fill(set.folder, set, c)}`;
        pdfPrev.textContent = `Preview: ${fill(set.pdfName, set, c, sample)}.pdf`;
        dxfPrev.textContent = `Preview: ${fill(set.dxfName, set, c, sample)}.dxf`;
        ifcPrev.textContent = `Preview: ${fill(set.ifcName, set, c)}.ifc`;
      };
      projectDir().then((d) => { dir = d; preview(); });
      const folder = inp("folder");
      const browse = button("Browse...", async () => { const d = await Backend.pickFolder(); if (d) { folder.value = `${d}\\{Date}_`; set.folder = folder.value; preview(); } });
      const tabsBar = el("div", { className: "ex-tabs" });
      const tabBody = el("div", { className: "ex-tab" });
      let tab = "pdf";
      const tabsDraw = () => {
        tabsBar.replaceChildren(...[["pdf", "PDF Settings"], ["dxf", "DWG/DXF Settings"], ["ifc", "IFC Settings"]].map(([k, l]) => { const b = el("button", { textContent: l, className: tab === k ? "on" : "" }); b.onclick = () => { tab = k; tabsDraw(); }; return b; }));
        const row = (l, c) => el("div", { className: "ex-row2" }, [el("span", { textContent: l }), c]);
        if (tab === "pdf") {
          const combine = el("span", { className: "ex-radios" }, [[true, "Combined into a single file"], [false, "Create separate files"]].map(([v, l]) => { const r = el("input", { type: "radio", name: "ex-combine", checked: set.pdfCombine === v }); r.onchange = () => { set.pdfCombine = v; }; return el("label", {}, [r, ` ${l}`]); }));
          tabBody.replaceChildren(
            row("Filename template:", inp("pdfName")), pdfPrev,
            el("div", { className: "ex-note", textContent: "Tags: {ProjectNumber} {ProjectName} {Date} {SheetNumber} {SheetName}" }),
            row("File:", combine),
            row("Paper size:", sel("pdfPaper", [["default", "Default (sheet size)"], ["A0", "A0"], ["A1", "A1"], ["A2", "A2"], ["A3", "A3"], ["A4", "A4"]])),
            row("Orientation:", radio("pdfOrientation", [["portrait", "Portrait"], ["landscape", "Landscape"]])),
            row("Zoom:", radio("pdfZoom", [["fit", "Fit to page"], ["zoom", "Zoom 100%"]])),
            row("Colours:", sel("pdfColour", [["colour", "Colour"], ["grey", "Greyscale"], ["bw", "Black and white"]])),
            el("div", { className: "ex-note", textContent: "Views print as vector lines and text. Crop boundaries, work planes and selection never print." }),
          );
        } else if (tab === "dxf") tabBody.replaceChildren(
          row("Filename template:", inp("dxfName")), dxfPrev,
          el("div", { className: "ex-note", textContent: "One file per sheet, paper space in millimetres (DXF R12, which every CAD program opens). Line weights are kept as polyline widths; fills are left out." }),
          row("Colours:", sel("dxfColours", [["index", "Index colours"]])),
        );
        else tabBody.replaceChildren(
          row("Filename template:", inp("ifcName")), ifcPrev,
          el("div", { className: "ex-note", textContent: "The whole model as IFC4, once per export." }),
        );
        preview();
      };
      const formats = el("div", { className: "ex-bar" }, [chk("pdf", "PDF"), chk("dxf", "DWG/DXF"), chk("ifc", "IFC")]);
      body.append(
        el("div", { className: "ex-h" , textContent: "Export Formats" }), formats,
        el("div", { className: "ex-h", textContent: "Output Location" }),
        el("div", { className: "ex-row2" }, [el("span", { textContent: "Folder template:" }), folder, browse]), folderPrev,
        el("div", { className: "ex-row2" }, [el("span", { textContent: "Date format:" }), sel("dateFormat", [["yyMMdd", "yyMMdd (e.g. 261030)"], ["yyyyMMdd", "yyyyMMdd"], ["yyyy-MM-dd", "yyyy-MM-dd"], ["ddMMyy", "ddMMyy"]])]),
        el("div", { className: "ex-note", textContent: "Available tags: {ProjectDir} {ProjectNumber} {ProjectName} {Date}" }),
        tabsBar, tabBody,
      );
      tabsDraw();
      const saveSettings = button("Save Settings...", async () => { const p = await Backend.pickSaveJson("Export Settings.json"); if (p) await call("write_file", { path: p, text: JSON.stringify(set, null, 2) }); });
      const loadSettings = button("Load Settings...", async () => { const p = await Backend.pickOpenJson(); if (!p) return; const r = await call("read_file", { path: p }); if (r) { try { Object.assign(set, JSON.parse(r.text)); store(set); close(); settingsWindow(items, onDone); } catch (e) { setPrompt(`Not export settings: ${e.message}`, true); } } });
      foot.append(loadSettings, saveSettings, el("span", { className: "spacer" }), button("Export", () => { if (!set.pdf && !set.dxf && !set.ifc) { setPrompt("Pick at least one format.", true); return; } store(set); close(); onDone(set); }, true), button("Cancel", close));
    }, { wide: true });
  }
  const call = async (m, p) => { try { return await Backend.call(m, p); } catch (e) { setPrompt(String(e.message || e), true); return null; } };

  // Export > Select Sheets > Export Settings > Export
  function start() {
    selectWindow((items) => settingsWindow(items, async (set) => {
      try {
        const r = await run(items, set, (t) => setPrompt(t));
        setPrompt(`Exported ${r.written.length} file${r.written.length === 1 ? "" : "s"} to ${r.folder}.`);
      } catch (e) { setPrompt(`Export failed: ${String(e.message || e)}`, true); }
    }));
  }

  return { start, run, pdf, dxf, pageOf, Recorder, fill, load };
})();
