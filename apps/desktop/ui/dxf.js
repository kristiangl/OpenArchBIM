// DXF export: a sheet (or a view) as a drawing a CAD program can work in. The views go in model space at full size (a
// plan at its true coordinates, the other views beside it), the sheet is a paper space layout with the title block and
// a locked viewport onto each view at its scale. Lines sit on layers named from what they belong to and what they show
// (the category's layer stem and a status: cut, projection, pattern, ...), with colours, line weights and line types by
// layer (every entity takes its layer's colour); doors, windows and other families are blocks, dimensions are dimensions, fills and patterns are hatches.
//
// The drawing comes from the recording context in export.js, the same drawing the PDF gets, tagged by the viewers with
// what each piece belongs to. Lines are cut to the clip regions in force and lose what later fills cover, as on paper.
const Dxf = (() => {
  // ---- layers ------------------------------------------------------------------------------------------------------
  // A layer is a stem (discipline, major, minor) and a status. The status comes from what the piece shows, unless the
  // row gives its own. Colours are index colours; patterns (hatches) take the row's pattern colour.
  const STATUS = { projection: "OTLN", cut: "MCUT", pattern: "PATT", beyond: "MBND", underlay: "OTLN", hidden: "HDLN", overhead: "OVHD", iden: "IDEN", text: "TEXT", dims: "DIMS", symb: "SYMB", nplt: "NPLT", schd: "SCHD" };
  const row = (cat, layer, colour, pattern, sub, status) => ({ cat, sub: sub || "", layer, status: status || "", colour, pattern: pattern == null ? colour : pattern });
  const LAYERS = [
    row("Walls", "A-WALL-____", 113, 111), row("Curtain Panels", "A-GLAZ-CURT", 52, 8), row("Curtain Systems", "A-GLAZ-CURT", 52, 8),
    row("Curtain Wall Mullions", "A-GLAZ-CWMG", 51), row("Doors", "A-DOOR-____", 31, 8), row("Doors", "A-DOOR-FRAM", 31, 8, "Frame/Mullion"),
    row("Windows", "A-GLAZ-____", 4, 8), row("Windows", "A-GLAZ-FRAM", 4, 8, "Frame/Mullion"), row("Floors", "A-FLOR-____", 192, 8),
    row("Roofs", "A-ROOF-____", 132), row("Ceilings", "A-CLNG-____", 141, 8), row("Columns", "A-COLS-____", 1, 8),
    row("Structural Columns", "S-COLS-____", 1, 8), row("Structural Framing", "S-BEAM-____", 2, 8), row("Structural Foundations", "S-FNDN-____", 3, 8),
    row("Stairs", "S-STRS-____", 31), row("Railings", "A-FLOR-HRAL", 11), row("Ramps", "A-FLOR-RAMP", 31),
    row("Furniture", "I-FURN-____", 30), row("Furniture Systems", "I-FURN-SYST", 30), row("Casework", "Q-CASE-____", 31),
    row("Specialty Equipment", "Q-SPCQ-____", 91), row("Food Service Equipment", "Q-FOOD-____", 91), row("Plumbing Fixtures", "P-SANR-FIXT", 6),
    row("Mechanical Equipment", "M-EQPM-____", 50), row("Air Terminals", "M-HVAC-DIFF", 50), row("Mechanical Control Devices", "M-CONT-____", 50),
    row("Electrical Equipment", "E-ELEC-EQPM", 71), row("Electrical Fixtures", "E-POWR-DEVC", 71), row("Lighting Fixtures", "E-LITE-____", 2),
    row("Lighting Devices", "E-LITE-SWCH", 2), row("Communication Devices", "T-COMM-DEVC", 140), row("Data Devices", "T-DATA-DEVC", 140),
    row("Fire Alarm Devices", "F-ALRM-DEVC", 1), row("Fire Protection", "F-PROT-____", 1), row("Security Devices", "E-SERT-DEVC", 140),
    row("Generic Models", "A-GENM-____", 13), row("Mass", "A-MASS-____", 8), row("Entourage", "A-ENTR-____", 8), row("Signage", "A-SIGN-____", 31),
    row("Parking", "C-PKNG-____", 91), row("Topography", "C-TOPO-____", 7, 8), row("Site", "L-SITE-____", 91), row("Hardscape", "L-HARD-____", 91),
    row("Planting", "L-PLNT-____", 71), row("Rooms", "A-AREA-____", 32),
    row("Dimensions", "A-____-____", 211), row("Spot Elevations", "A-____-____", 211), row("Spot Coordinates", "A-____-____", 211), row("Spot Slopes", "A-____-____", 211),
    row("Text Notes", "A-____-NOTE", 131), row("Grids", "S-GRID-____", 1), row("Levels", "A-____-LEVL", 1),
    row("Elevations", "G-____-____", 111), row("Sections", "G-____-____", 111), row("Room Tags", "A-AREA-____", 32), row("Door Tags", "A-DOOR-____", 31),
    row("Window Tags", "A-GLAZ-____", 4), row("Wall Tags", "A-WALL-____", 113), row("Keynote Tags", "A-____-NOTE", 131), row("Generic Annotations", "G-ANNO-____", 131),
    row("Revision Clouds", "G-ANNO-REVC", 1), row("Detail Items", "A-DETL-GENF", 92, 8), row("Lines", "A-DETL-____", 90),
    row("Lines", "A-DETL-____", 151, null, "Thin Lines", "THIN"), row("Lines", "A-DETL-____", 90, null, "Hidden Lines", "HDLN"),
    row("Lines", "A-DETL-DEMO", 6, null, "Demolished"), row("Lines", "A-DETL-____", 1, null, "Overhead", "OVHD"), row("Lines", "A-DETL-____", 90, null, "Beyond", "MBND"),
    row("Title Blocks", "G-____-TTLB", 6), row("View Titles", "G-____-____", 231), row("Viewports", "G-____-____", 191), row("Schedules", "G-____-____", 231),
  ];
  const defaults = () => ({ layers: LAYERS.map((r) => ({ ...r })), status: { ...STATUS } });

  // ---- geometry ----------------------------------------------------------------------------------------------------
  const boxOf = (rings) => { let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity; for (const r of rings) for (const [x, y] of r) { if (x < x0) x0 = x; if (y < y0) y0 = y; if (x > x1) x1 = x; if (y > y1) y1 = y; } return [x0, y0, x1, y1]; };
  const meets = (a, b) => a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];
  const region = (subs, evenodd) => { const rings = subs.map((s) => s.pts).filter((r) => r.length > 2); return { rings, evenodd, box: boxOf(rings) }; };
  // inside a region: even-odd over all its rings, or (nonzero, as the viewers fill separate shapes) inside any of them
  function insideOf(r, x, y) {
    let any = false, odd = false;
    for (const ring of r.rings) {
      let c = false;
      for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
        const [xi, yi] = ring[i], [xj, yj] = ring[j];
        if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
      }
      if (c) { any = true; odd = !odd; }
    }
    return r.evenodd ? odd : any;
  }
  const segDist = (x, y, a, b) => { const dx = b[0] - a[0], dy = b[1] - a[1], l = dx * dx + dy * dy; let t = l ? ((x - a[0]) * dx + (y - a[1]) * dy) / l : 0; t = Math.max(0, Math.min(1, t)); return Math.hypot(x - a[0] - t * dx, y - a[1] - t * dy); };
  function nearEdge(r, x, y, eps) {
    for (const ring of r.rings) for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) if (segDist(x, y, ring[j], ring[i]) < eps) return true;
    return false;
  }
  // the parts of p->q (as [t0, t1] along it) inside a region (`keepInside`) or outside it; a part along the region's
  // edge counts as inside a clip and outside a fill, so an outline drawn on a clip's edge or beside a fill stays
  function split(p, q, r, keepInside, eps) {
    const ts = [0, 1], dx = q[0] - p[0], dy = q[1] - p[1];
    for (const ring of r.rings) {
      for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
        const a = ring[j], b = ring[i], ex = b[0] - a[0], ey = b[1] - a[1], den = dx * ey - dy * ex;
        if (Math.abs(den) < 1e-12) continue;
        const t = ((a[0] - p[0]) * ey - (a[1] - p[1]) * ex) / den, u = ((a[0] - p[0]) * dy - (a[1] - p[1]) * dx) / den;
        if (t > 1e-9 && t < 1 - 1e-9 && u >= -1e-9 && u <= 1 + 1e-9) ts.push(t);
      }
    }
    ts.sort((a, b) => a - b);
    const out = [];
    for (let i = 0; i + 1 < ts.length; i++) {
      const a = ts[i], b = ts[i + 1];
      if (b - a < 1e-9) continue;
      const m = (a + b) / 2, x = p[0] + dx * m, y = p[1] + dy * m;
      const inside = nearEdge(r, x, y, eps) ? keepInside : insideOf(r, x, y);
      if (inside === keepInside) { if (out.length && Math.abs(out[out.length - 1][1] - a) < 1e-9) out[out.length - 1][1] = b; else out.push([a, b]); }
    }
    return out;
  }
  const meet = (A, B) => { const out = []; for (const [a0, a1] of A) for (const [b0, b1] of B) { const s = Math.max(a0, b0), e = Math.min(a1, b1); if (e - s > 1e-9) out.push([s, e]); } return out; };
  // a ring cut to a convex region (Sutherland-Hodgman)
  function clipRing(ring, clip) {
    let pts = ring;
    const n = clip.length;
    let area = 0; for (let i = 0, j = n - 1; i < n; j = i++) area += (clip[j][0] - clip[i][0]) * (clip[j][1] + clip[i][1]);
    const sgn = area > 0 ? 1 : -1;
    for (let i = 0; i < n && pts.length; i++) {
      const a = clip[i], b = clip[(i + 1) % n];
      const side = (p) => sgn * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]));
      const out = [];
      for (let k = 0; k < pts.length; k++) {
        const P = pts[k], Q = pts[(k + 1) % pts.length], sp = side(P), sq = side(Q);
        if (sp >= 0) out.push(P);
        if ((sp >= 0) !== (sq >= 0)) { const t = sp / (sp - sq); out.push([P[0] + (Q[0] - P[0]) * t, P[1] + (Q[1] - P[1]) * t]); }
      }
      pts = out;
    }
    return pts;
  }
  const convex = (ring) => { let s = 0; for (let i = 0; i < ring.length; i++) { const a = ring[i], b = ring[(i + 1) % ring.length], c = ring[(i + 2) % ring.length]; const z = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]); if (Math.abs(z) < 1e-9) continue; if (s && Math.sign(z) !== s) return false; s = Math.sign(z); } return true; };

  // ---- colours, weights, line types --------------------------------------------------------------------------------
  // the index colours 1 to 255, for the nearest one to a true colour: 24 hues from 10 on, each in five shades, full
  // and pale, then six greys
  const ACI = (() => {
    const out = [null, [255, 0, 0], [255, 255, 0], [0, 255, 0], [0, 255, 255], [0, 0, 255], [255, 0, 255], [255, 255, 255], [128, 128, 128], [192, 192, 192]];
    const hsv = (h, s, v) => { const f = (n) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); }; return [f(5), f(3), f(1)].map(Math.round); };
    for (let i = 10; i < 250; i++) { const j = (i - 10) % 10; out.push(hsv(Math.floor((i - 10) / 10) * 15, j & 1 ? 0.5 : 1, [255, 204, 153, 127, 76][j >> 1])); }
    for (const g of [51, 91, 132, 173, 214, 255]) out.push([g, g, g]);
    return out;
  })();
  const nearestAci = (c) => { let best = 7, bd = Infinity; for (let i = 1; i < ACI.length; i++) { const q = ACI[i]; const d = (c[0] - q[0]) ** 2 + (c[1] - q[1]) ** 2 + (c[2] - q[2]) ** 2; if (d < bd) { bd = d; best = i; } } return best; };
  const LW = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
  const lineweight = (mm) => { const v = mm * 100; let best = 0; for (const w of LW) if (Math.abs(w - v) < Math.abs(best - v)) best = w; return best; };
  const isWhite = (c) => c[0] >= 245 && c[1] >= 245 && c[2] >= 245;
  const safeName = (s) => String(s).replace(/[<>/\\":;?*|,=`\x00-\x1f]/g, "_").trim().slice(0, 250) || "_";

  // ---- the drawing -------------------------------------------------------------------------------------------------
  // page: { paper [w, h] mm, K (page units to the mm, y down), ops, vps, blocks, dims, name, patterns }
  // opt: { space: "model" | "paper", layers, status }
  function build(page, opt) {
    const K = page.K, PH = page.paper[1], eps = 0.05 * K;
    const modelSpace = opt.space !== "paper";
    const rows = new Map();
    for (const r of opt.layers || LAYERS) rows.set(`${r.cat}|${r.sub || ""}`, r);
    const status = { ...STATUS, ...(opt.status || {}) };
    const layers = new Map(); // name -> { colour, plot, lws: Map, lts: Map }
    const layerOf = (tag) => {
      let name = "0", colour = 7, plot = true;
      if (tag && tag.cat) {
        const r = rows.get(`${tag.cat}|${tag.sub || ""}`) || rows.get(`${tag.cat}|`);
        const kind = tag.kind || "projection";
        if (r && r.layer) {
          name = safeName(`${r.layer}-${(r.sub && tag.sub && r.status) || status[kind] || "OTLN"}`);
          colour = kind === "pattern" ? r.pattern : r.colour;
        }
        plot = kind !== "nplt";
      }
      if (!layers.has(name)) layers.set(name, { colour: Number(colour) || 7, plot, lws: new Map(), lts: new Map() });
      return name;
    };

    // the views: page to model mm, before each is moved to its place in model space
    const vps = (page.vps || []).map((v) => {
      const [a, b, c, d, e, f] = v.m, det = a * d - b * c, L = v.local;
      const toModel = (p) => { const qx = (d * (p[0] - e) - c * (p[1] - f)) / det, qy = (-b * (p[0] - e) + a * (p[1] - f)) / det; return [(qx - L.w / 2) / L.s + L.cx, L.cy - (qy - L.h / 2) / L.s]; };
      return { ...v, toModel, unit: 1 / (L.s * Math.sqrt(Math.abs(det))), off: [0, 0], box: [Infinity, Infinity, -Infinity, -Infinity] };
    });
    const toPaper = (p) => [p[0] / K, PH - p[1] / K];
    const mapFor = (vp) => (modelSpace && vp != null && vps[vp] ? { F: vps[vp].toModel, u: vps[vp].unit, scale: vps[vp].scale } : { F: toPaper, u: 1 / K, scale: 1 });

    // the clip regions in force at each op, and the fills, which hide lines drawn before them
    const ops = page.ops, clipsAt = new Array(ops.length), fills = [];
    // everything is cut to the page, as the PDF is, whatever reaches past it
    const PW = page.paper[0], sheetEdge = region([{ pts: [[0, 0], [PW * K, 0], [PW * K, PH * K], [0, PH * K]] }], false);
    let clips = [sheetEdge]; const stack = [];
    ops.forEach((o, i) => {
      if (o.t === "save") stack.push(clips);
      else if (o.t === "restore") clips = stack.length ? stack.pop() : [sheetEdge];
      else if (o.t === "clip") clips = [...clips, region(o.subs, o.evenodd)];
      else { clipsAt[i] = clips; if (o.t === "fill" && o.subs.length) { const r = region(o.subs, o.evenodd); if (r.rings.length) fills.push({ i, ...r }); } }
    });
    const CELL = 25 * K, grid = new Map();
    const cells = (b, fn) => { for (let x = Math.floor(b[0] / CELL); x <= Math.floor(b[2] / CELL); x++) for (let y = Math.floor(b[1] / CELL); y <= Math.floor(b[3] / CELL); y++) fn(`${x},${y}`); };
    fills.forEach((f, n) => cells(f.box, (k) => { if (!grid.has(k)) grid.set(k, []); grid.get(k).push(n); }));
    const fillsOver = (b, after) => { const seen = new Set(), out = []; cells(b, (k) => { for (const n of grid.get(k) || []) if (!seen.has(n)) { seen.add(n); const f = fills[n]; if (f.i > after && meets(f.box, b)) out.push(f); } }); return out.sort((a, b2) => a.i - b2.i); };
    const visible = (p, q, i, cl) => {
      let iv = [[0, 1]];
      const b = [Math.min(p[0], q[0]) - eps, Math.min(p[1], q[1]) - eps, Math.max(p[0], q[0]) + eps, Math.max(p[1], q[1]) + eps];
      for (const r of cl) { if (!meets(r.box, b)) return []; iv = meet(iv, split(p, q, r, true, eps)); if (!iv.length) return iv; }
      for (const f of fillsOver(b, i)) { iv = meet(iv, split(p, q, f, false, eps)); if (!iv.length) break; }
      return iv;
    };
    const lerp = (p, q, t) => [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
    function pieces(sub, i, cl) {
      const pts = sub.closed ? [...sub.pts, sub.pts[0]] : sub.pts;
      const out = []; let cur = null, whole = true;
      for (let j = 0; j + 1 < pts.length; j++) {
        const p = pts[j], q = pts[j + 1];
        if (Math.abs(p[0] - q[0]) < 1e-9 && Math.abs(p[1] - q[1]) < 1e-9) continue;
        const iv = visible(p, q, i, cl);
        if (!(iv.length === 1 && iv[0][0] < 1e-9 && iv[0][1] > 1 - 1e-9)) whole = false;
        for (const [a, b] of iv) {
          if (cur && a < 1e-9 && cur.end === j) cur.pts.push(lerp(p, q, b));
          else { if (cur) out.push(cur.pts); cur = { pts: [lerp(p, q, a), lerp(p, q, b)] }; }
          cur.end = b > 1 - 1e-9 ? j + 1 : -1;
        }
      }
      if (cur) out.push(cur.pts);
      if (sub.closed && whole && out.length === 1) return [{ pts: out[0].slice(0, -1), closed: true }];
      return out.map((p) => ({ pts: p, closed: false }));
    }
    const clippedRings = (rings, cl) => {
      let out = rings;
      for (const r of cl) if (r.rings.length === 1 && convex(r.rings[0])) out = out.map((g) => clipRing(g, r.rings[0])).filter((g) => g.length > 2);
      return out;
    };
    const anchorIn = (x, y, cl) => cl.every((r) => insideOf(r, x, y) || nearEdge(r, x, y, eps));

    // what is drawn, in model or paper mm: { t, layer, colour, lw, lt, vp, blk, dim, ... }
    const ents = [];
    const lineTypes = new Map(); // name -> dashes (paper mm)
    const patternNames = page.patterns || {};
    const ltName = (dash) => {
      if (!dash || !dash.length || dash.every((d) => d <= 0)) return null;
      const mm = dash.map((d) => d / K);
      for (const [n, a] of Object.entries(patternNames)) {
        if (a && a.length === mm.length && a.every((v, k) => Math.abs(Math.max(v, 0.08) - mm[k]) <= Math.max(0.02, v * 0.02))) { if (!lineTypes.has(safeName(n))) lineTypes.set(safeName(n), a); return safeName(n); }
      }
      const n = `OA_${mm.map((v) => Math.round(v * 100) / 100).join("_")}`.replace(/\./g, "p");
      if (!lineTypes.has(n)) lineTypes.set(n, mm);
      return n;
    };
    ops.forEach((o, i) => {
      if (o.t !== "stroke" && o.t !== "fill" && o.t !== "text" && o.t !== "hatch") return;
      const cl = clipsAt[i] || [], M = mapFor(o.vp), base = { layer: layerOf(o.tag), vp: o.vp, blk: o.blk, dim: o.dim, scale: M.scale };
      // colour by layer, always: the screen's greys would hide lines in model space
      const colour = null;
      if (o.t === "stroke") {
        const lw = lineweight(o.width / K), lt = ltName(o.dash);
        for (const s of o.subs) for (const pc of pieces(s, i, cl)) {
          const pts = pc.pts.map(M.F);
          ents.push({ ...base, t: pts.length === 2 && !pc.closed ? "line" : "poly", pts, closed: pc.closed, colour, lw, lt });
        }
      } else if (o.t === "fill") {
        if (isWhite(o.colour)) return;
        const rings = clippedRings(o.subs.map((s) => s.pts).filter((r) => r.length > 2), cl);
        if (rings.length) ents.push({ ...base, t: "hatch", solid: true, rings: rings.map((r) => r.map(M.F)), colour, lw: 0 });
      } else if (o.t === "hatch") {
        const rings = clippedRings(o.subs.map((s) => s.pts).filter((r) => r.length > 2), cl);
        if (!rings.length) return;
        const V = (p, v) => { const a = M.F(p), b = M.F([p[0] + v[0], p[1] + v[1]]); return [b[0] - a[0], b[1] - a[1]]; };
        const lines = o.lines.map((L) => {
          const d = V(L.o, L.d);
          return { angle: (Math.atan2(d[1], d[0]) * 180) / Math.PI, base: M.F(L.o), off: V(L.o, L.off), dashes: L.dashes.map((x) => x * M.u) };
        });
        ents.push({ ...base, t: "hatch", solid: false, name: safeName(o.name || "OA_PATTERN"), rings: rings.map((r) => r.map(M.F)), lines, colour, lw: lineweight(o.width / K) });
      } else if (o.t === "text") {
        if (!anchorIn(o.x, o.y, cl)) return;
        const dir = [Math.cos(o.angle), Math.sin(o.angle)];
        const at = M.F([o.x, o.y]), d2 = M.F([o.x + dir[0], o.y + dir[1]]);
        const ang = Math.atan2(d2[1] - at[1], d2[0] - at[0]);
        const len = o.width * M.u, al = o.align === "center" ? 1 : o.align === "right" || o.align === "end" ? 2 : 0;
        const ap = al ? [at[0] + Math.cos(ang) * len * (al === 1 ? 0.5 : 1), at[1] + Math.sin(ang) * len * (al === 1 ? 0.5 : 1)] : null;
        ents.push({ ...base, t: "text", text: o.text, at, ap, al, h: o.size * M.u * 0.7, ang: (ang * 180) / Math.PI, stretch: o.stretch, bold: o.bold, italic: o.italic, colour });
      }
    });

    // blocks: each family drawn as a block in its own frame, the same block again where the drawing is the same
    const blocks = []; // { name, ents, anon }
    const byName = new Map();
    const pointsOf = (e) => (e.t === "hatch" ? e.rings.flat() : e.t === "text" ? [e.at] : e.pts || []);
    const moveEnt = (e, f, rot) => {
      const out = { ...e };
      if (e.pts) out.pts = e.pts.map(f);
      if (e.rings) out.rings = e.rings.map((r) => r.map(f));
      if (e.at) out.at = f(e.at);
      if (e.ap) out.ap = f(e.ap);
      if (e.t === "text") out.ang = e.ang + rot;
      if (e.lines) out.lines = e.lines.map((L) => ({ ...L, angle: L.angle + rot, base: f(L.base), off: turn(L.off, rot) }));
      return out;
    };
    const turn = (v, deg) => { const a = (deg * Math.PI) / 180, c = Math.cos(a), s = Math.sin(a); return [v[0] * c - v[1] * s, v[0] * s + v[1] * c]; };
    const round = (x) => Math.round(x * 100) / 100;
    const inserts = [];
    const grouped = new Map();
    for (const e of ents) if (e.blk != null && e.dim == null) { if (!grouped.has(e.blk)) grouped.set(e.blk, []); grouped.get(e.blk).push(e); }
    for (const [bi, list] of grouped) {
      const info = page.blocks[bi], M = mapFor(info.vp);
      const at = M.F(info.at), d = M.F([info.at[0] + info.dir[0], info.at[1] + info.dir[1]]);
      const rot = (Math.atan2(d[1] - at[1], d[0] - at[0]) * 180) / Math.PI;
      const local = list.map((e) => moveEnt(e, (p) => turn([p[0] - at[0], p[1] - at[1]], -rot), -rot));
      const key = JSON.stringify(local.map((e) => [e.t, e.layer, e.colour, e.lw, e.lt, e.closed, e.text, (e.pts || (e.rings && e.rings.flat()) || [e.at]).map((p) => p.map(round))]));
      const nm = safeName(info.name || "Block");
      if (!byName.has(nm)) byName.set(nm, []);
      const same = byName.get(nm);
      let b = same.find((x) => x.key === key);
      if (!b) { b = { name: same.length ? `${nm} (${same.length + 1})` : nm, ents: local.map((e) => ({ ...e, vp: null, blk: null })), key }; same.push(b); blocks.push(b); }
      inserts.push({ t: "insert", name: b.name, at, rot, layer: layerOf(info.tag), vp: info.vp, scale: M.scale });
    }
    // dimensions: each a dimension entity with its drawing as its block
    const dims = [];
    const dimGroups = new Map();
    for (const e of ents) if (e.dim != null) { if (!dimGroups.has(e.dim)) dimGroups.set(e.dim, []); dimGroups.get(e.dim).push(e); }
    for (const [di, list] of dimGroups) {
      const info = page.dims[di], M = mapFor(info.vp);
      const d = M.F([info.p13[0] + info.dir[0], info.p13[1] + info.dir[1]]), a = M.F(info.p13);
      const name = `*D${dims.length + 1}`;
      const style = `OpenArch ${Math.round(M.scale)}${info.tick === "arrow" ? "" : " Tick"}`;
      blocks.push({ name, anon: true, ents: list.map((e) => ({ ...e, blk: null, dim: null })), vp: info.vp });
      dims.push({ t: "dim", name, layer: layerOf(info.tag), vp: info.vp, p10: M.F(info.p10), p11: M.F(info.p11), p13: a, p14: M.F(info.p14), angle: (Math.atan2(d[1] - a[1], d[0] - a[0]) * 180) / Math.PI, value: info.value, text: info.text, style, txt: info.txt, tick: info.tick, gap: info.gap, ext: info.ext, scale: M.scale });
    }
    const loose = ents.filter((e) => e.blk == null && e.dim == null);

    // the views' places in model space: a plan stays at its coordinates, the others line up beside it
    if (modelSpace && vps.length) {
      const grow = (b, p) => { if (p[0] < b[0]) b[0] = p[0]; if (p[1] < b[1]) b[1] = p[1]; if (p[0] > b[2]) b[2] = p[0]; if (p[1] > b[3]) b[3] = p[1]; };
      for (const e of [...loose, ...inserts]) if (e.vp != null && vps[e.vp]) (e.t === "insert" ? [e.at] : pointsOf(e)).forEach((p) => grow(vps[e.vp].box, p));
      for (const dm of dims) if (dm.vp != null && vps[dm.vp]) [dm.p10, dm.p13, dm.p14].forEach((p) => grow(vps[dm.vp].box, p));
      const order = [...vps.keys()].sort((a, b) => (vps[b].kind === "plan") - (vps[a].kind === "plan") || a - b);
      let right = null, bottom = 0;
      for (const n of order) {
        const v = vps[n];
        if (!Number.isFinite(v.box[0])) continue;
        if (right == null) { right = v.box[2]; bottom = v.box[1]; continue; }
        const gap = Math.max(5000, 0.1 * (v.box[2] - v.box[0]));
        v.off = [right + gap - v.box[0], bottom - v.box[1]];
        right = v.box[2] + v.off[0];
      }
      const sh = (vp) => (p) => [p[0] + vps[vp].off[0], p[1] + vps[vp].off[1]];
      for (const e of loose) if (e.vp != null && vps[e.vp]) Object.assign(e, moveEnt(e, sh(e.vp), 0));
      for (const e of inserts) if (e.vp != null && vps[e.vp]) e.at = sh(e.vp)(e.at);
      for (const dm of dims) if (dm.vp != null && vps[dm.vp]) { const f = sh(dm.vp); dm.p10 = f(dm.p10); dm.p11 = f(dm.p11); dm.p13 = f(dm.p13); dm.p14 = f(dm.p14); }
      for (const b of blocks) if (b.anon && b.vp != null && vps[b.vp]) b.ents = b.ents.map((e) => moveEnt(e, sh(b.vp), 0));
    }

    // weights and line types by layer: each layer takes what most of its lines use
    const all = [...loose, ...blocks.flatMap((b) => b.ents)];
    for (const e of all) {
      if (e.t === "text" || e.t === "insert") continue;
      const L = layers.get(e.layer);
      if (e.lw != null && !(e.t === "hatch" && e.solid)) L.lws.set(e.lw, (L.lws.get(e.lw) || 0) + 1);
      if (e.t !== "hatch") L.lts.set(e.lt || "Continuous", (L.lts.get(e.lt || "Continuous") || 0) + 1);
    }
    const most = (m, dflt) => { let best = dflt, n = -1; for (const [k, c] of m) if (c > n) { n = c; best = k; } return best; };
    for (const L of layers.values()) { L.lw = most(L.lws, -3); L.lt = most(L.lts, "Continuous"); }
    for (const e of all) {
      const L = layers.get(e.layer);
      if (e.lw === L.lw) e.lw = -1;
      if (e.t !== "line" && e.t !== "poly") continue;
      if ((e.lt || "Continuous") === L.lt) e.lt = null;
      else if (!e.lt) e.lt = "Continuous";
    }

    // the viewports onto the views
    // (a turned view: its centre in the view's own axes, which turn with it)
    // (a view reaching past the page: its viewport stops at the page's edge)
    const viewports = modelSpace ? vps.map((v) => {
      const x0 = Math.max(0, v.centre[0] - v.paper_size[0] / 2), x1 = Math.min(PW, v.centre[0] + v.paper_size[0] / 2);
      const y0 = Math.max(0, v.centre[1] - v.paper_size[1] / 2), y1 = Math.min(PH, v.centre[1] + v.paper_size[1] / 2);
      if (x1 - x0 < 1e-3 || y1 - y0 < 1e-3) return null;
      const centre = [(x0 + x1) / 2, (y0 + y1) / 2];
      const c = v.toModel([centre[0] * K, (PH - centre[1]) * K]), w = [c[0] + v.off[0], c[1] + v.off[1]];
      return { centre, size: [x1 - x0, y1 - y0], view: turn(w, v.rotation || 0), height: (y1 - y0) * v.scale, twist: v.rotation || 0, layer: layerOf({ cat: "Viewports", kind: "nplt" }) };
    }).filter(Boolean) : [];
    const paperEnts = [], modelEnts = [];
    for (const e of [...loose, ...inserts, ...dims]) (modelSpace && (e.vp == null || !vps[e.vp]) ? paperEnts : modelEnts).push(e);
    return { paper: page.paper, name: safeName(page.name || "Sheet"), modelSpace, layers, lineTypes, blocks, model: modelEnts, paperEnts, viewports };
  }

  // ---- the file (DXF 2013, millimetres) ----------------------------------------------------------------------------
  function write(dw) {
    let seed = 0x20;
    const H = () => (seed++).toString(16).toUpperCase();
    const n = (v) => { const r = Math.round(v * 1e6) / 1e6; return Object.is(r, -0) ? "0" : String(r); };
    const body = [];
    const g = (c, v) => { body.push(String(c), typeof v === "number" ? n(v) : String(v)); };
    const xy = (c, p) => { g(c, p[0]); g(c + 10, p[1]); g(c + 20, 0); };
    // handles fixed up front, so tables and objects can point at each other
    const T = { vport: H(), ltype: H(), layer: H(), style: H(), view: H(), ucs: H(), appid: H(), dimstyle: H(), brec: H() };
    const O = { root: H(), group: H(), layout: H(), color: H(), material: H(), mleader: H(), mline: H(), plotset: H(), plotstyle: H(), placeholder: H(), scale: H(), tablestyle: H(), visual: H(), mlStd: H() };
    const ms = { brec: H(), block: H(), end: H(), layout: H() }, ps = { brec: H(), block: H(), end: H(), layout: H(), vp: H() };
    const vportActive = H();
    const styles = new Map([["Standard", { h: H(), font: "txt" }]]);
    const styleOf = (e) => {
      const nm = `Arial${e.bold ? " Bold" : ""}${e.italic ? " Italic" : ""}`;
      if (!styles.has(nm)) styles.set(nm, { h: H(), font: `arial${e.bold ? "bd" : ""}${e.italic ? "i" : ""}.ttf`.replace("bdi.", "bi.") });
      return nm;
    };
    const dimStyles = new Map([["Standard", { h: H(), scale: 1, txt: 2.5, tick: "arrow", gap: 0.625, ext: 1.25 }]]);
    const brecs = dw.blocks.map((b) => ({ ...b, brec: H(), begin: H(), end: H() }));
    const sx = (e) => {
      // common entity data: layer, colour (true colour with the nearest index colour), line type and scale, weight
      g(100, "AcDbEntity");
      if (e.paper) g(67, 1);
      g(8, e.layer);
      if (e.lt) g(6, e.lt);
      if (e.colour) { g(62, nearestAci(e.colour)); g(420, (e.colour[0] << 16) + (e.colour[1] << 8) + e.colour[2]); }
      if (e.lw != null && e.lw !== -1 && e.t !== "text") g(370, e.lw);
      if (e.lt && e.lt !== "Continuous" && e.scale && e.scale !== 1) g(48, e.scale);
    };
    const entity = (e, owner) => {
      if (e.t === "line") {
        g(0, "LINE"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbLine"); xy(10, e.pts[0]); xy(11, e.pts[1]);
      } else if (e.t === "poly") {
        g(0, "LWPOLYLINE"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbPolyline"); g(90, e.pts.length); g(70, e.closed ? 1 : 0); g(43, 0);
        for (const p of e.pts) { g(10, p[0]); g(20, p[1]); }
      } else if (e.t === "text") {
        g(0, "TEXT"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbText"); xy(10, e.at); g(40, Math.max(e.h, 1e-3)); g(1, e.text.replace(/[\r\n]+/g, " "));
        if (Math.abs(e.ang) > 1e-6) g(50, ((e.ang % 360) + 360) % 360);
        if (Math.abs(e.stretch - 1) > 0.01) g(41, e.stretch);
        g(7, styleOf(e));
        if (e.al) { g(72, e.al); xy(11, e.ap); }
        g(100, "AcDbText");
      } else if (e.t === "hatch") {
        g(0, "HATCH"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbHatch"); xy(10, [0, 0]); g(210, 0); g(220, 0); g(230, 1);
        g(2, e.solid ? "SOLID" : e.name); g(70, e.solid ? 1 : 0); g(71, 0); g(91, e.rings.length);
        e.rings.forEach((r, i) => { g(92, i === 0 ? 3 : 2); g(72, 0); g(73, 1); g(93, r.length); for (const p of r) { g(10, p[0]); g(20, p[1]); } g(97, 0); });
        g(75, 0); g(76, e.solid ? 1 : 2);
        if (!e.solid) {
          g(52, 0); g(41, 1); g(77, 0); g(78, e.lines.length);
          for (const L of e.lines) { g(53, L.angle); g(43, L.base[0]); g(44, L.base[1]); g(45, L.off[0]); g(46, L.off[1]); g(79, L.dashes.length); for (const d of L.dashes) g(49, d); }
        }
        g(98, 0);
      } else if (e.t === "insert") {
        g(0, "INSERT"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbBlockReference"); g(2, e.name); xy(10, e.at);
        if (Math.abs(e.rot) > 1e-6) g(50, ((e.rot % 360) + 360) % 360);
      } else if (e.t === "dim") {
        if (!dimStyles.has(e.style)) dimStyles.set(e.style, { h: H(), scale: e.scale, txt: e.txt, tick: e.tick, gap: e.gap, ext: e.ext });
        g(0, "DIMENSION"); g(5, H()); g(330, owner); sx(e); g(100, "AcDbDimension"); g(280, 0); g(2, e.name); xy(10, e.p10); xy(11, e.p11);
        g(70, 32 | 128); g(71, 5); g(72, 1); g(41, 1); g(42, e.value); g(1, e.text || ""); g(3, e.style);
        g(100, "AcDbAlignedDimension"); xy(13, e.p13); xy(14, e.p14); g(50, e.angle); g(100, "AcDbRotatedDimension");
      }
    };
    const viewport = (v, id, owner) => {
      g(0, "VIEWPORT"); g(5, id === 1 ? ps.vp : H()); g(330, owner); g(100, "AcDbEntity"); g(67, 1); g(8, v.layer || "0");
      g(100, "AcDbViewport"); xy(10, v.centre); g(40, v.size[0]); g(41, v.size[1]); g(68, id === 1 ? 1 : id); g(69, id);
      g(12, v.view[0]); g(22, v.view[1]); g(13, 0); g(23, 0); g(14, 10); g(24, 10); g(15, 10); g(25, 10); g(16, 0); g(26, 0); g(36, 1); g(17, 0); g(27, 0); g(37, 0);
      g(42, 50); g(43, 0); g(44, 0); g(45, v.height); g(50, 0); g(51, ((v.twist || 0) * Math.PI) / 180); g(72, 100);
      g(90, id === 1 ? 557088 : 16384 | 32 | 524288); g(1, ""); g(281, 0); g(71, 1); g(74, 0);
      g(110, 0); g(120, 0); g(130, 0); g(111, 1); g(121, 0); g(131, 0); g(112, 0); g(122, 1); g(132, 0); g(79, 0); g(146, 0); g(170, 0);
    };

    // entities first (they name the styles and dimension styles the tables need)
    const ents = [];
    const flush = () => body.splice(0);
    for (const e of dw.model) entity(e, ms.brec);
    const modelText = flush();
    if (dw.modelSpace) {
      viewport({ centre: [dw.paper[0] / 2, dw.paper[1] / 2], size: [dw.paper[0] * 1.1, dw.paper[1] * 1.1], view: [dw.paper[0] / 2, dw.paper[1] / 2], height: dw.paper[1] * 1.1 }, 1, ps.brec);
      dw.viewports.forEach((v, i) => viewport(v, i + 2, ps.brec));
      for (const e of dw.paperEnts) entity({ ...e, paper: true }, ps.brec);
    }
    const paperText = flush();
    const blockText = [];
    for (const b of brecs) {
      g(0, "BLOCK"); g(5, b.begin); g(330, b.brec); g(100, "AcDbEntity"); g(8, "0"); g(100, "AcDbBlockBegin"); g(2, b.name); g(70, b.anon ? 1 : 0); xy(10, [0, 0]); g(3, b.name); g(1, "");
      for (const e of b.ents) entity(e, b.brec);
      g(0, "ENDBLK"); g(5, b.end); g(330, b.brec); g(100, "AcDbEntity"); g(8, "0"); g(100, "AcDbBlockEnd");
      blockText.push(...flush());
    }
    ents.push(...modelText, ...paperText);

    // extents
    let mx = [Infinity, Infinity, -Infinity, -Infinity];
    const grow = (p) => { mx = [Math.min(mx[0], p[0]), Math.min(mx[1], p[1]), Math.max(mx[2], p[0]), Math.max(mx[3], p[1])]; };
    for (const e of dw.model) (e.pts || (e.rings && e.rings.flat()) || [e.at || e.p10]).forEach((p) => p && grow(p));
    if (!Number.isFinite(mx[0])) mx = [0, 0, dw.paper[0], dw.paper[1]];

    // tables
    const tables = [];
    const tg = (c, v) => tables.push(String(c), typeof v === "number" ? n(v) : String(v));
    const table = (name, h, count, fn, extra) => { tg(0, "TABLE"); tg(2, name); tg(5, h); tg(330, 0); tg(100, "AcDbSymbolTable"); tg(70, count); if (extra) extra(); fn(); tg(0, "ENDTAB"); };
    const rec = (type, h, owner, sub, hcode = 5) => { tg(0, type); tg(hcode, h); tg(330, owner); tg(100, "AcDbSymbolTableRecord"); tg(100, sub); };
    table("VPORT", T.vport, 1, () => {
      rec("VPORT", vportActive, T.vport, "AcDbViewportTableRecord"); tg(2, "*Active"); tg(70, 0);
      tg(10, 0); tg(20, 0); tg(11, 1); tg(21, 1); tg(12, (mx[0] + mx[2]) / 2); tg(22, (mx[1] + mx[3]) / 2); tg(13, 0); tg(23, 0); tg(14, 10); tg(24, 10); tg(15, 10); tg(25, 10);
      tg(16, 0); tg(26, 0); tg(36, 1); tg(17, 0); tg(27, 0); tg(37, 0); tg(40, Math.max(mx[3] - mx[1], 1) * 1.1); tg(41, 1.5); tg(42, 50); tg(43, 0); tg(44, 0); tg(50, 0); tg(51, 0);
      tg(71, 0); tg(72, 1000); tg(73, 1); tg(74, 3); tg(75, 0); tg(76, 0); tg(77, 0); tg(78, 0); tg(281, 0); tg(65, 1); tg(146, 0);
    });
    const lts = [["ByBlock", []], ["ByLayer", []], ["Continuous", []], ...dw.lineTypes];
    table("LTYPE", T.ltype, lts.length, () => {
      for (const [name, dashes] of lts) {
        rec("LTYPE", H(), T.ltype, "AcDbLinetypeTableRecord"); tg(2, name); tg(70, 0); tg(3, name === "Continuous" ? "Solid line" : ""); tg(72, 65);
        const els = dashes.map((d, i) => (i % 2 ? -Math.abs(d) : Math.abs(d)));
        tg(73, els.length); tg(40, els.reduce((t, d) => t + Math.abs(d), 0));
        for (const d of els) { tg(49, d); tg(74, 0); }
      }
    });
    const layerList = [["0", { colour: 7, plot: true, lw: -3, lt: "Continuous" }], ["Defpoints", { colour: 7, plot: false, lw: -3, lt: "Continuous" }], ...[...dw.layers].filter(([k]) => k !== "0")];
    table("LAYER", T.layer, layerList.length, () => {
      for (const [name, L] of layerList) {
        rec("LAYER", H(), T.layer, "AcDbLayerTableRecord"); tg(2, name); tg(70, 0); tg(62, L.colour); tg(6, L.lt || "Continuous");
        if (!L.plot) tg(290, 0);
        tg(370, L.lw ?? -3); tg(390, O.placeholder);
      }
    });
    table("STYLE", T.style, styles.size, () => {
      for (const [name, s] of styles) { rec("STYLE", s.h, T.style, "AcDbTextStyleTableRecord"); tg(2, name); tg(70, 0); tg(40, 0); tg(41, 1); tg(50, 0); tg(71, 0); tg(42, 2.5); tg(3, s.font); tg(4, ""); }
    });
    table("VIEW", T.view, 0, () => {});
    table("UCS", T.ucs, 0, () => {});
    table("APPID", T.appid, 1, () => { rec("APPID", H(), T.appid, "AcDbRegAppTableRecord"); tg(2, "ACAD"); tg(70, 0); });
    const arial = styles.get("Arial") || styles.get("Standard");
    table("DIMSTYLE", T.dimstyle, dimStyles.size, () => {
      for (const [name, s] of dimStyles) {
        rec("DIMSTYLE", s.h, T.dimstyle, "AcDbDimStyleTableRecord", 105); tg(2, name); tg(70, 0);
        tg(40, s.scale); tg(41, s.tick === "arrow" ? 2.5 : 1.5); tg(42, s.gap); tg(43, 3.75); tg(44, s.ext); tg(140, s.txt); tg(141, 0); tg(142, s.tick === "arrow" ? 0 : 1.5); tg(147, 0.8);
        tg(77, 1); tg(78, 8); tg(271, 0); tg(277, 2); tg(279, 2); tg(280, 0); tg(340, arial.h);
      }
    }, () => tg(100, "AcDbDimStyleTable"));
    const allRecs = [{ name: "*Model_Space", brec: ms.brec, layout: ms.layout }, { name: "*Paper_Space", brec: ps.brec, layout: ps.layout }, ...brecs];
    table("BLOCK_RECORD", T.brec, allRecs.length, () => {
      for (const b of allRecs) { rec("BLOCK_RECORD", b.brec, T.brec, "AcDbBlockTableRecord"); tg(2, b.name); tg(340, b.layout || 0); tg(70, 4); tg(280, 1); tg(281, 0); }
    });

    // blocks: the two spaces, then the blocks
    const blocks = [];
    const bg = (c, v) => blocks.push(String(c), typeof v === "number" ? n(v) : String(v));
    for (const [nm, r] of [["*Model_Space", ms], ["*Paper_Space", ps]]) {
      bg(0, "BLOCK"); bg(5, r.block); bg(330, r.brec); bg(100, "AcDbEntity"); if (r === ps) bg(67, 1); bg(8, "0"); bg(100, "AcDbBlockBegin"); bg(2, nm); bg(70, 0); bg(10, 0); bg(20, 0); bg(30, 0); bg(3, nm); bg(1, "");
      bg(0, "ENDBLK"); bg(5, r.end); bg(330, r.brec); bg(100, "AcDbEntity"); if (r === ps) bg(67, 1); bg(8, "0"); bg(100, "AcDbBlockEnd");
    }
    blocks.push(...blockText);

    // objects: the dictionaries a drawing carries, and the two layouts with their page setups
    const objs = [];
    const og = (c, v) => objs.push(String(c), typeof v === "number" ? n(v) : String(v));
    const dict = (h, owner, entries) => { og(0, "DICTIONARY"); og(5, h); og(330, owner); og(100, "AcDbDictionary"); og(281, 1); for (const [k, v] of entries) { og(3, k); og(350, v); } };
    const paperLayout = ps.layout;
    dict(O.root, 0, [["ACAD_COLOR", O.color], ["ACAD_GROUP", O.group], ["ACAD_LAYOUT", O.layout], ["ACAD_MATERIAL", O.material], ["ACAD_MLEADERSTYLE", O.mleader], ["ACAD_MLINESTYLE", O.mline], ["ACAD_PLOTSETTINGS", O.plotset], ["ACAD_PLOTSTYLENAME", O.plotstyle], ["ACAD_SCALELIST", O.scale], ["ACAD_TABLESTYLE", O.tablestyle], ["ACAD_VISUALSTYLE", O.visual]]);
    dict(O.color, O.root, []); dict(O.group, O.root, []);
    dict(O.layout, O.root, dw.modelSpace ? [["Model", ms.layout], [dw.name, paperLayout]] : [["Model", ms.layout], ["Layout1", paperLayout]]);
    dict(O.material, O.root, []); dict(O.mleader, O.root, []); dict(O.mline, O.root, [["Standard", O.mlStd]]); dict(O.plotset, O.root, []);
    og(0, "ACDBDICTIONARYWDFLT"); og(5, O.plotstyle); og(330, O.root); og(100, "AcDbDictionary"); og(281, 1); og(3, "Normal"); og(350, O.placeholder); og(100, "AcDbDictionaryWithDefault"); og(340, O.placeholder);
    og(0, "ACDBPLACEHOLDER"); og(5, O.placeholder); og(330, O.plotstyle);
    dict(O.scale, O.root, []); dict(O.tablestyle, O.root, []); dict(O.visual, O.root, []);
    og(0, "MLINESTYLE"); og(5, O.mlStd); og(330, O.mline); og(100, "AcDbMlineStyle"); og(2, "Standard"); og(70, 0); og(3, ""); og(62, 256); og(51, 90); og(52, 90); og(71, 2); og(49, 0.5); og(62, 256); og(6, "BYLAYER"); og(49, -0.5); og(62, 256); og(6, "BYLAYER");
    const [pw, ph] = dw.paper;
    const layout = (h, brec, name, tab, model, vp) => {
      og(0, "LAYOUT"); og(5, h); og(330, O.layout);
      og(100, "AcDbPlotSettings"); og(1, ""); og(2, "none_device"); og(4, model ? "" : `ISO_full_bleed_${pw}x${ph}_MM`); og(6, "");
      og(40, 0); og(41, 0); og(42, 0); og(43, 0); og(44, pw); og(45, ph); og(46, 0); og(47, 0); og(48, 0); og(49, 0); og(140, 0); og(141, 0); og(142, 1); og(143, 1);
      og(70, model ? 1024 : 688); og(72, 1); og(73, 0); og(74, 5); og(7, ""); og(75, 16); og(76, 0); og(77, 2); og(78, 300); og(147, 1); og(148, 0); og(149, 0);
      og(100, "AcDbLayout"); og(1, name); og(70, 1); og(71, tab); og(10, 0); og(20, 0); og(11, model ? 420 : pw); og(21, model ? 297 : ph); og(12, 0); og(22, 0); og(32, 0);
      og(14, model ? mx[0] : 0); og(24, model ? mx[1] : 0); og(34, 0); og(15, model ? mx[2] : pw); og(25, model ? mx[3] : ph); og(35, 0); og(146, 0);
      og(13, 0); og(23, 0); og(33, 0); og(16, 1); og(26, 0); og(36, 0); og(17, 0); og(27, 1); og(37, 0); og(76, 1); og(330, brec);
      if (vp) og(331, vp);
    };
    layout(ms.layout, ms.brec, "Model", 0, true, vportActive);
    layout(paperLayout, ps.brec, dw.modelSpace ? dw.name : "Layout1", 1, false, dw.modelSpace ? ps.vp : null);

    // the header, last, for the next free handle
    const hd = [];
    const hg = (c, v) => hd.push(String(c), typeof v === "number" ? n(v) : String(v));
    const v9 = (name, ...pairs) => { hg(9, name); for (let i = 0; i < pairs.length; i += 2) hg(pairs[i], pairs[i + 1]); };
    v9("$ACADVER", 1, "AC1027"); v9("$ACADMAINTVER", 70, 105); v9("$DWGCODEPAGE", 3, "ANSI_1252"); v9("$INSBASE", 10, 0, 20, 0, 30, 0);
    v9("$EXTMIN", 10, mx[0], 20, mx[1], 30, 0); v9("$EXTMAX", 10, mx[2], 20, mx[3], 30, 0); v9("$LIMMIN", 10, 0, 20, 0); v9("$LIMMAX", 10, pw, 20, ph);
    v9("$MIRRTEXT", 70, 0); v9("$LTSCALE", 40, 1); v9("$TEXTSIZE", 40, 2.5); v9("$TEXTSTYLE", 7, "Standard"); v9("$CLAYER", 8, "0"); v9("$CELTYPE", 6, "ByLayer");
    v9("$CECOLOR", 62, 256); v9("$CELTSCALE", 40, 1); v9("$DIMSTYLE", 2, "Standard"); v9("$LUNITS", 70, 2); v9("$LUPREC", 70, 0); v9("$AUNITS", 70, 0); v9("$AUPREC", 70, 2);
    v9("$HANDSEED", 5, (seed + 1).toString(16).toUpperCase()); v9("$TILEMODE", 70, 1); v9("$PLIMMIN", 10, 0, 20, 0); v9("$PLIMMAX", 10, pw, 20, ph);
    v9("$PEXTMIN", 10, 0, 20, 0, 30, 0); v9("$PEXTMAX", 10, pw, 20, ph, 30, 0); v9("$PSLTSCALE", 70, 0); v9("$MEASUREMENT", 70, 1); v9("$CELWEIGHT", 370, -1); v9("$LWDISPLAY", 290, 1);
    v9("$INSUNITS", 70, 4); v9("$PSTYLEMODE", 290, 1); v9("$DIMASSOC", 280, 2);

    const out = [];
    const sec = (name, lines) => { out.push("0", "SECTION", "2", name, ...lines, "0", "ENDSEC"); };
    sec("HEADER", hd);
    sec("CLASSES", ["0", "CLASS", "1", "ACDBDICTIONARYWDFLT", "2", "AcDbDictionaryWithDefault", "3", "ObjectDBX Classes", "90", "0", "91", "1", "280", "0", "281", "0",
      "0", "CLASS", "1", "ACDBPLACEHOLDER", "2", "AcDbPlaceHolder", "3", "ObjectDBX Classes", "90", "0", "91", "1", "280", "0", "281", "0",
      "0", "CLASS", "1", "LAYOUT", "2", "AcDbLayout", "3", "ObjectDBX Classes", "90", "0", "91", "2", "280", "0", "281", "0"]);
    sec("TABLES", tables);
    sec("BLOCKS", blocks);
    sec("ENTITIES", ents);
    sec("OBJECTS", objs);
    out.push("0", "EOF");
    return out.join("\r\n") + "\r\n";
  }

  return { build, write, defaults, STATUS, LAYERS };
})();
