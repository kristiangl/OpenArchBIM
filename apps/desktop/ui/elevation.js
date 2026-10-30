// Elevation views: the model seen from an elevation marker, drawn back to front, with the level
// lines across it. Data comes from the `elevation` command; picking and editing go through callbacks
// so the rest of the app treats a click here like a click in plan. Sections are drawn here too.
// Detail lines, regions and text notes (details.js) live in the view's own frame: x along the view, y up. The sketch
// session (sketch.js) is hosted here while Detail Line or a region is drawn in the view.
const Elev = (() => {
  const canvas = document.getElementById("elev");
  let ctx = canvas.getContext("2d");
  // renderTo: drawing a view onto a sheet or a page, at another size, with nothing selected and no tool showing
  let printing = false, printSize = null;
  const cb = {
    select: () => {}, addLevel: () => {}, moveLevel: () => {}, setExtent: () => {}, toggleBubble: () => {},
    prompt: () => {}, cursor: () => {}, renameLevel: () => {}, editElevation: () => {},
    addGrid: () => {}, cropSection: () => {}, moveBy: () => {}, toggleLevelLock: () => {}, tempSettings: () => ({}),
    dimCall: () => null, dimRefresh: () => {}, dimText: () => {}, place: () => {}, placeSize: () => null,
    arrayOptions: () => ({ arrayCount: 2, arrayMoveTo: "second", arrayConstrain: false }), array: () => {},
    editGroup: () => {}, groupPick: () => {}, placeGroup: () => {}, curtainCall: () => {},
  };
  // groups (doc_info): a click on a member picks its group, Tab the member; Edit Group and Place Group as in plan
  let modelGroups = [], memberOf = new Map(), groupEdit = null, groupPlace = null;
  // a group's members, and those of the groups nested in it
  const expandIds = (ids) => {
    const out = new Set(ids);
    for (let grew = true; grew; ) { grew = false; for (const g of modelGroups) if (out.has(g.id)) for (const m of g.members) if (!out.has(m)) { out.add(m); grew = true; } }
    return out;
  };
  // the groups holding an element, innermost first
  const chainOf = (id) => { const out = []; for (let g = memberOf.get(id); g != null && !out.includes(g); g = memberOf.get(g)) out.push(g); return out; };
  // in Edit Group: the element or the nested group a pick takes (Add: what is outside the open group), or null
  const editUnit = (id) => {
    const ch = chainOf(id);
    if (groupEdit.mode === "add") return !ch.length ? id : ch.includes(groupEdit.id) ? null : ch[ch.length - 1];
    const k = ch.indexOf(groupEdit.id);
    return k < 0 ? null : k === 0 ? id : ch[k - 1];
  };
  function groupify(ids) {
    if (groupEdit) return [...new Set(ids.map(editUnit).filter((x) => x != null))];
    if (!memberOf.size) return ids;
    // each outermost group comes where its first member would, and Tab reaches what is in it next
    const out = [], byGroup = new Map();
    for (const id of ids) {
      const ch = chainOf(id);
      if (!ch.length) { out.push(id); continue; }
      const top = ch[ch.length - 1];
      if (!byGroup.has(top)) { byGroup.set(top, []); out.push(top); }
      const list = byGroup.get(top);
      for (const g of ch.slice(0, -1).reverse()) if (!list.includes(g)) list.push(g);
      list.push(id);
    }
    return out.flatMap((id) => (byGroup.has(id) ? [id, ...byGroup.get(id)] : [id]));
  }
  let data = null;
  let cam = { u: 0, z: 1500, s: 0.1 }; // centre (mm along the view's right axis, mm of height) and pixels per mm
  let tool = "select"; // select | level | grid | move | copy | align | text | noteleader | sketch | dimension | spot
  let sel = new Set();
  let hoverIds = null;
  let hoverLevel = null;
  let hoverGrid = null;
  let mouse = null;
  let pan = null;
  let down = null;
  let drag = null; // { id, kind: "move"|"end", end, z|u, start }
  let typed = "";
  let input = null;
  let ed = null; // the stage of a Modify tool: { stage: "pick"|"a"|"b"|"ref"|"tgt", base, ref }
  let dimDown = null;
  let candTab = 0, candKey = "";
  let boxes = new Map(); // id -> bounds of everything drawn for it
  let cands = []; // axis-aligned lines to align to
  let verts = [];
  let dimHits = [];
  let lastMove = null;
  let tabIdx = 0; // which of the things under the cursor Tab has stepped to
  let levelPins = []; // screen position of each padlock between aligned level ends
  let hover = null; // the snapped point under the cursor while a sketch or the Text tool is on
  let snapCycle = 0; // Tab steps through the snap points in reach
  let band = null; // a window or crossing box in the sketch's Select tool
  let detDrag = null; // selected detail items carried by a drag: { ids, p0, d }
  // curtain walls (data.curtains): the Curtain Grid tool's placement and the line it would place, the line placed in
  // All Except Picked waiting for its segments, the grid line whose segments are being added or removed (and the tool to
  // go back to), and a grid line placed by hand being dragged
  let cgSig = new Map(), cgMode = "all", cgHover = null, cgPending = null, segLine = null, segHover = null, segReturn = "select", cgDrag = null, joinHover = null;
  const PART_BASE = 2 ** 51;
  const partWall = (id) => (id >= PART_BASE && id < 2 * PART_BASE ? Math.floor((id - PART_BASE) / 2 ** 27) : null);
  // a part id's kind (0, 1 grid lines; 2 panel; 3, 4 mullions), line and segment
  const partOf = (id) => { const x = id - PART_BASE; return { kind: Math.floor(x / 2 ** 24) % 8, a: Math.floor(x / 4096) % 4096, b: x % 4096 }; };
  const faces = () => (data && data.curtains) || [];
  // the selected mullion run, alone in the selection, with its face
  function selMullion() {
    if (sel.size !== 1) return null;
    const id = [...sel][0];
    if (partWall(id) == null) return null;
    for (const f of faces()) { const m = (f.mullions || []).find((x) => x.id === id); if (m) return { f, m }; }
    return null;
  }
  // Toggle Mullion Join: a square at each crossing of the selected mullion, "+" where it stops (join it through), "-"
  // where it runs through (break it)
  const JOIN_PX = 7;
  function joinAt(sx, sy) {
    const s = selMullion();
    if (!s || tool !== "select") return null;
    for (const j of s.m.joins) { const p = toScreen(j.at[0], j.at[1]); if (Math.abs(sx - p.x) <= JOIN_PX + 2 && Math.abs(sy - p.y) <= JOIN_PX + 2) return { wall: s.f.wall, ...j }; }
    return null;
  }
  function drawJoinToggles(accent, viewBg) {
    const s = selMullion();
    if (!s || tool !== "select") return;
    ctx.save();
    for (const j of s.m.joins) {
      const p = toScreen(j.at[0], j.at[1]), r = JOIN_PX, hot = joinHover && joinHover.k === j.k && joinHover.j === j.j;
      ctx.fillStyle = hot ? accent : viewBg; ctx.strokeStyle = accent; ctx.lineWidth = 1.2;
      ctx.fillRect(p.x - r, p.y - r, 2 * r, 2 * r); ctx.strokeRect(p.x - r, p.y - r, 2 * r, 2 * r);
      ctx.strokeStyle = hot ? viewBg : accent; ctx.lineWidth = 1.6;
      ctx.beginPath(); ctx.moveTo(p.x - r + 3, p.y); ctx.lineTo(p.x + r - 3, p.y);
      if (!j.through) { ctx.moveTo(p.x, p.y - r + 3); ctx.lineTo(p.x, p.y + r - 3); }
      ctx.stroke();
    }
    ctx.restore();
  }
  const gridLine = (id) => { for (const f of faces()) { const l = f.lines.find((x) => x.id === id); if (l) return { f, l }; } return null; };
  // along a curtain wall from its start at the view's `u` (null off its ends), and the view's `u` along it
  function alongOf(f, u) {
    for (let k = 1; k < f.stops.length; k++) {
      const [d0, u0] = f.stops[k - 1], [d1, u1] = f.stops[k];
      if (Math.abs(u1 - u0) > 1e-6 && (u - u0) * (u - u1) <= 0) return d0 + ((u - u0) / (u1 - u0)) * (d1 - d0);
    }
    return null;
  }
  function uAlong(f, d) {
    for (let k = 1; k < f.stops.length; k++) {
      const [d0, u0] = f.stops[k - 1], [d1, u1] = f.stops[k];
      if (d <= d1 || k === f.stops.length - 1) return u0 + (d1 > d0 ? (d - d0) / (d1 - d0) : 0) * (u1 - u0);
    }
    return f.stops[0][1];
  }
  const near = (sx, sy, s, px) => segDist(sx, sy, toScreen(s.a[0], s.a[1]), toScreen(s.b[0], s.b[1])) <= px;
  // the interior grid lines of curtain walls by the cursor, by wall
  function curtainLinesAt(sx, sy) {
    const out = new Map();
    for (const f of faces()) for (const l of f.lines) {
      if (l.edge || !l.segs.some((s) => s.on && near(sx, sy, s, 5))) continue;
      if (!out.has(f.wall)) out.set(f.wall, []);
      out.get(f.wall).push(l.id);
    }
    return out;
  }
  // the panels and mullions of curtain walls under the cursor, nearest first, by wall
  function partsAt(sx, sy) {
    const p = toWorld(sx, sy), out = new Map();
    for (let i = data.items.length - 1; i >= 0; i--) {
      const it = data.items[i];
      if (it.part == null || !it.polys.some((poly) => inPoly(p, poly))) continue;
      if (!out.has(it.id)) out.set(it.id, []);
      if (!out.get(it.id).includes(it.part)) out.get(it.id).push(it.part);
    }
    return out;
  }
  // where the Curtain Grid tool would place a line on the curtain wall under the cursor: a vertical line when the cursor
  // is nearer the top or bottom of its cell, a horizontal one when nearer a side; One Segment keeps to that cell
  function curtainGridAt(sx, sy) {
    const p = toWorld(sx, sy), wall = [...partsAt(sx, sy).keys()][0];
    const f = faces().find((x) => x.wall === wall);
    if (!f) return null;
    const d = alongOf(f, p.u), h = p.z - f.z0;
    if (d == null) return null;
    const us = f.lines.filter((l) => l.vertical).map((l) => l.at), vs = f.lines.filter((l) => !l.vertical).map((l) => l.at);
    const i = Math.max(0, us.findIndex((x, k) => k + 1 < us.length && d >= x && d <= us[k + 1]));
    const j = Math.max(0, vs.findIndex((x, k) => k + 1 < vs.length && h >= x && h <= vs[k + 1]));
    const toSide = Math.min(Math.abs(uAlong(f, us[i]) - p.u), Math.abs(uAlong(f, us[i + 1]) - p.u));
    const toEnd = Math.min(Math.abs(h - vs[j]), Math.abs(h - vs[j + 1]));
    const vertical = toEnd < toSide, one = cgMode === "one";
    const at = Math.round((vertical ? d : h) / step()) * step();
    const [e0, e1] = vertical ? (one ? [vs[j], vs[j + 1]] : [0, vs[vs.length - 1]]) : one ? [us[i], us[i + 1]] : [0, us[us.length - 1]];
    const a = vertical ? [uAlong(f, at), f.z0 + e0] : [uAlong(f, e0), f.z0 + at], b = vertical ? [uAlong(f, at), f.z0 + e1] : [uAlong(f, e1), f.z0 + at];
    return { wall: f.wall, vertical, at, a, b, only: one ? (vertical ? (vs[j] + vs[j + 1]) / 2 : (us[i] + us[i + 1]) / 2) : null };
  }
  // the segment of the line in Add/Remove Segments under the cursor
  function segmentAt(sx, sy) {
    const g = gridLine(segLine);
    if (!g) return null;
    let best = null;
    g.l.segs.forEach((s, k) => { const d = segDist(sx, sy, toScreen(s.a[0], s.a[1]), toScreen(s.b[0], s.b[1])); if (d <= 8 && (!best || d < best.d)) best = { k, d }; });
    return best ? best.k : null;
  }
  // Curtain grid lines show while the Curtain Grid tool is on and on a curtain wall that is selected or hovered or has a
  // part selected: dashed, a segment taken out dotted, a selected or hovered line in the accent colour.
  function drawCurtainGrids(accent, text, viewBg) {
    const ids = [...sel, ...(hoverIds || [])];
    const line = (a, b) => { ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); };
    for (const f of faces()) {
      const show = tool === "curtaingrid" || tool === "cgsegments" || ids.some((i) => i === f.wall || partWall(i) === f.wall);
      if (!show) continue;
      for (const l of f.lines) {
        if (l.edge) continue;
        const on = sel.has(l.id) || (hoverIds || []).includes(l.id) || segLine === l.id;
        l.segs.forEach((g, s) => {
          ctx.save();
          ctx.strokeStyle = on ? accent : mix(text, viewBg, 0.55);
          ctx.lineWidth = on ? 1.6 : 1;
          ctx.setLineDash(g.on ? [8, 4] : [2, 4]);
          if (tool === "cgsegments" && segLine === l.id && segHover === s) { ctx.lineWidth = 3; ctx.setLineDash([]); }
          if (!(cgDrag && cgDrag.moved && cgDrag.id === l.id)) line(toScreen(g.a[0], g.a[1]), toScreen(g.b[0], g.b[1]));
          ctx.restore();
        });
      }
    }
    ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.6; ctx.setLineDash([8, 4]);
    if (tool === "curtaingrid" && cgHover) line(toScreen(cgHover.a[0], cgHover.a[1]), toScreen(cgHover.b[0], cgHover.b[1]));
    if (cgDrag && cgDrag.moved) {
      const f = cgDrag.f, len = f.stops[f.stops.length - 1][0];
      if (cgDrag.vertical) line(toScreen(uAlong(f, cgDrag.at), f.z0), toScreen(uAlong(f, cgDrag.at), f.z1));
      else line(toScreen(uAlong(f, 0), f.z0 + cgDrag.at), toScreen(uAlong(f, len), f.z0 + cgDrag.at));
    }
    ctx.restore();
  }
  const EDIT = ["move", "copy", "align", "array"];
  const isEdit = () => EDIT.includes(tool);

  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  // a colour `f` of the way from the background to `col`: depth cueing fades toward the background
  const rgbCache = new Map();
  function rgbOf(col) {
    if (!rgbCache.has(col)) {
      const c = document.createElement("canvas").getContext("2d");
      c.fillStyle = "#000"; c.fillStyle = col;
      const s = String(c.fillStyle);
      rgbCache.set(col, s[0] === "#" ? [1, 3, 5].map((i) => parseInt(s.slice(i, i + 2), 16)) : (s.match(/[\d.]+/g) || [0, 0, 0]).slice(0, 3).map(Number));
    }
    return rgbCache.get(col);
  }
  const mix = (col, bg, f) => { const a = rgbOf(col), b = rgbOf(bg); return `rgb(${a.map((x, i) => Math.round(b[i] + (x - b[i]) * f)).join(",")})`; };
  const size = () => printSize || { w: canvas.clientWidth, h: canvas.clientHeight };
  const toScreen = (u, z) => { const { w, h } = size(); return { x: (u - cam.u) * cam.s + w / 2, y: h / 2 - (z - cam.z) * cam.s }; };
  const toWorld = (sx, sy) => { const { w, h } = size(); return { u: (sx - w / 2) / cam.s + cam.u, z: (h / 2 - sy) / cam.s + cam.z }; };
  // annotation is drawn at its paper size: pixels in a paper millimetre are the view scale times the zoom
  const k = () => ((data && data.scale) || 100) * cam.s;
  // a grid head's radius on screen, from its type's Head Diameter on paper (8 mm when not given)
  const gridR = (g) => Math.max(5, (((g && g.head_dia_mm) || 8) / 2) * k());
  const headW = (l) => (l.head ? Math.max(Ann.extent(l.head).maxX, 0) * k() : 0); // how far a level's head reaches past its end

  function resize() {
    const r = window.devicePixelRatio || 1, { w, h } = size();
    if (canvas.width !== Math.round(w * r) || canvas.height !== Math.round(h * r)) { canvas.width = Math.round(w * r); canvas.height = Math.round(h * r); }
    ctx.setTransform(r, 0, 0, r, 0, 0);
  }

  const datum = () => (data && data.levels.length ? data.levels[0].z - data.levels[0].elev : 0);
  const fmtElev = (v) => String(Math.round(v) || 0);
  const snapZ = (z) => {
    // 10 mm steps, and a light pull towards existing levels
    for (const l of data.levels) if (Math.abs(l.z - z) * cam.s < 6) return l.z;
    return Math.round(z / 10) * 10;
  };

  function inPoly(p, poly) {
    let inside = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [xi, yi] = poly[i], [xj, yj] = poly[j];
      if ((yi > p.z) !== (yj > p.z) && p.u < ((xj - xi) * (p.z - yi)) / (yj - yi) + xi) inside = !inside;
    }
    return inside;
  }

  // ---- alignment of level ends: the alignment indicator and alignment pin ------------------------
  // `side` is 0 for the end at the left of this view and 1 for the right; the stored extent's own ends can be either way round
  const modelEnd = (l, side) => (side === 0 ? l.end_left : 1 - l.end_left);
  const sideOf = (l, mEnd) => (mEnd === l.end_left ? 0 : 1);
  const uOf = (l, side) => (side === 0 ? l.u0 : l.u1);
  // every level end locked to this one, directly or through others (including itself)
  function lockGroup(id, mEnd) {
    const out = [{ id, end: mEnd }];
    for (let i = 0; i < out.length; i++) {
      const l = data.levels.find((x) => x.id === out[i].id);
      for (const r of (l && l.locks ? l.locks[out[i].end] : []) || []) if (!out.some((q) => q.id === r.id && q.end === r.end) && data.levels.some((x) => x.id === r.id)) out.push({ id: r.id, end: r.end });
    }
    return out;
  }
  // the end of another level the dragged end lines up with, if any
  function levelAlign(l, side, u) {
    const skip = new Set(lockGroup(l.id, modelEnd(l, side)).map((q) => q.id));
    let best = null;
    for (const o of data.levels) {
      if (skip.has(o.id)) continue;
      for (const sd of [side, 1 - side]) {
        const d = Math.abs(uOf(o, sd) - u) * cam.s;
        if (d > 8) continue;
        const score = d + (sd === side ? 0 : 0.1);
        if (!best || score < best.score) best = { score, o, sd, u: uOf(o, sd) };
      }
    }
    return best ? { id: best.o.id, end: modelEnd(best.o, best.sd), u: best.u, custom: best.o.custom_extent, level: best.o } : null;
  }
  // a dashed vertical line through the ends that line up
  function alignIndicator(u, zs) {
    const a = toScreen(u, Math.max(...zs)), b = toScreen(u, Math.min(...zs));
    ctx.save(); ctx.strokeStyle = "#3aa7d8"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
    ctx.beginPath(); ctx.moveTo(a.x, a.y - 40); ctx.lineTo(b.x, b.y + 40); ctx.stroke(); ctx.restore();
  }
  const padlock = (x, y, locked) => drawPadlock(ctx, x, y, locked, css("--accent"));

  // padlocks for the selected levels: closed where the end is locked in line with another, open where the next
  // level up or down happens to line up
  function drawLevelPins() {
    levelPins = [];
    const done = new Set();
    const sorted = [...data.levels].sort((p, q) => p.z - q.z);
    for (const l of data.levels) {
      if (!sel.has(l.id)) continue;
      const i = sorted.findIndex((x) => x.id === l.id);
      for (const side of [0, 1]) {
        const mEnd = modelEnd(l, side), u = uOf(l, side);
        const pairs = [];
        for (const r of (l.locks && l.locks[mEnd]) || []) { const o = data.levels.find((x) => x.id === r.id); if (o) pairs.push({ o, oSide: sideOf(o, r.end), oEnd: r.end, locked: true }); }
        for (const o of [sorted[i - 1], sorted[i + 1]]) {
          if (!o || (!l.custom_extent && !o.custom_extent)) continue;
          if (Math.abs(uOf(o, side) - u) >= 1) continue;
          if (pairs.some((q) => q.o.id === o.id)) continue;
          pairs.push({ o, oSide: side, oEnd: modelEnd(o, side), locked: false });
        }
        for (const pr of pairs) {
          const key = [Math.min(l.id, pr.o.id), Math.max(l.id, pr.o.id), side].join("-");
          if (done.has(key)) continue;
          done.add(key);
          const a = toScreen(u, l.z), b = toScreen(uOf(pr.o, pr.oSide), pr.o.z);
          const m = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
          ctx.save(); ctx.strokeStyle = "#7a8aa0"; ctx.lineWidth = 0.8; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
          padlock(m.x, m.y, pr.locked);
          levelPins.push({ x: m.x, y: m.y, id: l.id, end: mEnd, other: pr.o.id, otherEnd: pr.oEnd, locked: pr.locked });
          alignIndicator(u, [l.z, pr.o.z]);
        }
      }
    }
  }

  function levelAt(sx, sy) {
    if (!data) return null;
    let best = null;
    for (const l of data.levels) {
      const a = toScreen(l.u0, l.z), b = toScreen(l.u1, l.z);
      const hw = headW(l) + 6;
      if (sx < a.x - hw || sx > b.x + hw) continue;
      const d = Math.abs(sy - a.y);
      if (d <= 5 && (!best || d < best.d)) best = { id: l.id, d };
    }
    return best ? best.id : null;
  }

  function gridAt(sx, sy) {
    if (!data) return null;
    for (const g of data.grids || []) {
      const a = toScreen(g.u, g.z1), b = toScreen(g.u, g.z0);
      if (Math.abs(sx - a.x) <= 5 && sy >= a.y - gridR(g) * 2 && sy <= b.y) return g.id;
    }
    return null;
  }

  // the items under the cursor, nearest first (items are stored far to near)
  function itemsAt(sx, sy) {
    const p = toWorld(sx, sy), out = [];
    for (let i = data.items.length - 1; i >= 0; i--) {
      const it = data.items[i];
      if (it.polys.some((poly) => inPoly(p, poly)) && !out.includes(it.id)) out.push(it.id);
    }
    return out;
  }

  // everything under the cursor that a click could pick, nearest first: elements drawn there, then a level (a grid crosses
  // it), then a grid. Tab steps through them.
  function candIds(sx, sy) { return groupify(rawCandIds(sx, sy)); }
  function rawCandIds(sx, sy) {
    // a curtain wall comes first; Tab reaches its grid lines, mullions and panels under the cursor next
    const parts = groupEdit ? new Map() : partsAt(sx, sy), lines = groupEdit ? new Map() : curtainLinesAt(sx, sy);
    const items = itemsAt(sx, sy).flatMap((id) => [id, ...(lines.get(id) || []), ...(parts.get(id) || [])]);
    const out = [...spotsAt(sx, sy), ...dimsAt(sx, sy), ...detailAt(sx, sy), ...items], lid = levelAt(sx, sy), gid = gridAt(sx, sy);
    if (lid !== null) out.push(lid);
    if (gid !== null) out.push(gid);
    return out;
  }
  // permanent dimensions under the cursor: their line, witness lines and texts
  function dimsAt(sx, sy) {
    if (!data || !(data.dimensions || []).length) return [];
    const p = W(sx, sy);
    return data.dimensions.filter((d) => EDims.hitRings(d).rings.some((r) => inRing(p, r))).map((d) => d.id);
  }
  function spotsAt(sx, sy) {
    if (!data || !(data.spots || []).length) return [];
    const p = W(sx, sy);
    return data.spots.filter((x) => ESpots.hitRings(x).rings.some((r) => inRing(p, r))).map((x) => x.id);
  }
  const hoverPick = (sx, sy) => { const c = candIds(sx, sy); return c.length ? c[tabIdx % c.length] : null; };

  // handles of the selected level: the end grips, and the bubble check boxes
  function levelParts(l) {
    const a = toScreen(l.u0, l.z), b = toScreen(l.u1, l.z);
    const hw = Math.max(headW(l), gridR() * 2) + 14;
    return { a, b, boxes: [{ end: 0, x: a.x - hw, y: a.y }, { end: 1, x: b.x + hw, y: a.y }] };
  }
  function gripAt(sx, sy) {
    if (!data) return null;
    for (const l of data.levels) {
      if (!sel.has(l.id)) continue;
      const { a, b, boxes } = levelParts(l);
      for (const bx of boxes) if (Math.abs(sx - bx.x) <= 7 && Math.abs(sy - bx.y) <= 7) return { id: l.id, kind: "bubble", end: bx.end, l };
      if (Math.hypot(sx - a.x, sy - a.y) <= 7) return { id: l.id, kind: "end", end: 0, l };
      if (Math.hypot(sx - b.x, sy - b.y) <= 7) return { id: l.id, kind: "end", end: 1, l };
    }
    return null;
  }

  function label(text, x, y, color, align = "left", bold = false) {
    ctx.font = `${bold ? "600 " : ""}11px system-ui, sans-serif`;
    ctx.textAlign = align; ctx.textBaseline = "middle"; ctx.fillStyle = color;
    ctx.fillText(text, x, y);
  }

  function polyPath(poly) {
    ctx.beginPath();
    poly.forEach(([u, z], k) => { const s = toScreen(u, z); k ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
    ctx.closePath();
  }

  // ---- the crop of a section view: its width is the section line, its height a range of levels. Drag a side to change it.
  let cropBoxes = [], cropDrag = null; // grips in screen pixels; { side, v, moved } while one is dragged
  function drawCrop() {
    cropBoxes = [];
    if (!data || !data.section || printing) return;
    const f = data.frame, accent = css("--accent");
    let u0 = 0, u1 = f.right_mm, z0 = f.z_min, z1 = f.z_max;
    if (cropDrag && cropDrag.moved) { if (cropDrag.side === "left") u0 = cropDrag.v; else if (cropDrag.side === "right") u1 = cropDrag.v; else if (cropDrag.side === "bottom") z0 = cropDrag.v; else z1 = cropDrag.v; }
    const a = toScreen(u0, z1), b = toScreen(u1, z0);
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([7, 4]);
    ctx.strokeRect(a.x, a.y, b.x - a.x, b.y - a.y);
    ctx.setLineDash([]);
    if (tool === "select" && !data.section.pinned) {
      const g = [["left", a.x, (a.y + b.y) / 2], ["right", b.x, (a.y + b.y) / 2], ["top", (a.x + b.x) / 2, a.y], ["bottom", (a.x + b.x) / 2, b.y]];
      ctx.fillStyle = css("--view-bg");
      for (const [side, x, y] of g) { ctx.fillRect(x - 4, y - 4, 8, 8); ctx.strokeRect(x - 4, y - 4, 8, 8); cropBoxes.push({ side, x, y }); }
    }
    ctx.restore();
  }

  // ---- section cuts ------------------------------------------------------------------------
  // What the cut plane passes through is drawn over everything else with the category's cut line and the material's
  // cut pattern (the API sends the resolved style). Openings are holes in the fill.
  function cutPath(it) {
    ctx.beginPath();
    for (const ring of [...it.polys, ...(it.holes || [])]) {
      ring.forEach(([u, z], i) => { const s = toScreen(u, z); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); });
      ctx.closePath();
    }
  }

  function hatchLines(it, f, col) {
    const def = (data.fill_patterns || {})[f.pattern];
    if (!def || def.solid) { ctx.fillStyle = col; const { w, h } = size(); ctx.fillRect(0, 0, w, h); return; }
    let u0 = Infinity, z0 = Infinity, u1 = -Infinity, z1 = -Infinity;
    for (const ring of it.polys) for (const [u, z] of ring) { u0 = Math.min(u0, u); u1 = Math.max(u1, u); z0 = Math.min(z0, z); z1 = Math.max(z1, z); }
    const unit = def.model ? 1 : (data.scale || 100); // model mm per pattern unit
    const corners = [[u0, z0], [u1, z0], [u1, z1], [u0, z1]];
    ctx.strokeStyle = col; ctx.lineWidth = 1; ctx.lineCap = "butt"; ctx.setLineDash([]);
    let budget = 3000;
    for (const L of def.lines) {
      const a = (L.angle * Math.PI) / 180, dx = Math.cos(a), dy = Math.sin(a), nx = -dy, ny = dx, sp = L.spacing * unit;
      if (sp * cam.s < 2.5) { ctx.globalAlpha = 0.35; ctx.fillStyle = col; const { w, h } = size(); ctx.fillRect(0, 0, w, h); ctx.globalAlpha = 1; break; }
      const v0 = L.origin[0] * unit * nx + L.origin[1] * unit * ny;
      const vs = corners.map(([x, y]) => x * nx + y * ny), us = corners.map(([x, y]) => x * dx + y * dy);
      const k0 = Math.ceil((Math.min(...vs) - v0) / sp), k1 = Math.floor((Math.max(...vs) - v0) / sp), umin = Math.min(...us), umax = Math.max(...us);
      budget -= k1 - k0 + 1;
      if (budget < 0) break;
      const dashPx = (L.dashes || []).map((d) => Math.max(d * unit * cam.s, 0.6));
      ctx.setLineDash(dashPx);
      ctx.beginPath();
      for (let n = k0; n <= k1; n++) {
        const v = v0 + n * sp, p0 = toScreen(umin * dx + v * nx, umin * dy + v * ny), p1 = toScreen(umax * dx + v * nx, umax * dy + v * ny);
        ctx.moveTo(p0.x, p0.y); ctx.lineTo(p1.x, p1.y);
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);
  }

  function drawCut(it, vstyle, viewBg, col, selected, pass) {
    const st = it.style || { line: { color: "#000000", width_mm: 0.5 }, fill: { visible: false } };
    ctx.save();
    if (pass === "line") {
      cutPath(it);
      ctx.strokeStyle = selected ? col : st.line.color; ctx.lineWidth = selected ? 2.5 : Math.max(1.2, st.line.width_mm * k()); ctx.lineJoin = "miter"; ctx.stroke();
      ctx.restore();
      return;
    }
    cutPath(it);
    if (vstyle === "hidden_line") { ctx.fillStyle = viewBg; ctx.fill("evenodd"); }
    else if (vstyle !== "wireframe") { ctx.fillStyle = `rgb(${it.fill[0]},${it.fill[1]},${it.fill[2]})`; ctx.fill("evenodd"); }
    if (vstyle !== "wireframe" && st.fill && st.fill.visible) {
      ctx.save();
      cutPath(it); ctx.clip("evenodd");
      if (st.fill.background) { ctx.fillStyle = st.fill.background; const { w, h } = size(); ctx.fillRect(0, 0, w, h); }
      hatchLines(it, st.fill, st.fill.color);
      ctx.restore();
    }
    ctx.restore();
  }

  function drawLevel(l, mode) {
    const text = css("--text"), accent = css("--accent");
    const { a, b } = levelParts(l);
    const col = mode === "sel" ? accent : mode === "hover" ? accent : text;
    ctx.save();
    ctx.strokeStyle = col; ctx.lineWidth = mode === "sel" ? 1.8 : 1;
    ctx.setLineDash([14, 4, 3, 4]);
    ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
    ctx.setLineDash([]);
    ctx.restore();
    // the head family at each end that shows one: Name and elevation as its labels say
    for (const end of [0, 1]) {
      if (!l.bubbles[end]) continue;
      const p = end === 0 ? a : b;
      if (l.head) Ann.draw(ctx, l.head, { x: p.x, y: p.y, k: k(), flip: end === 0 }, col, css("--view-bg"), { own: !mode, pats: data.fill_patterns });
    }
    // the name beside the line when no head carries it
    if (!l.bubbles[0] && !l.bubbles[1]) label(l.name, a.x + 6, a.y - 9, col);
    if (mode === "sel") {
      ctx.fillStyle = accent;
      for (const p of [a, b]) { ctx.beginPath(); ctx.arc(p.x, p.y, 4, 0, Math.PI * 2); ctx.fill(); }
      for (const bx of levelParts(l).boxes) {
        ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.strokeRect(bx.x - 5, bx.y - 5, 10, 10);
        if (l.bubbles[bx.end]) { ctx.beginPath(); ctx.moveTo(bx.x - 3, bx.y); ctx.lineTo(bx.x - 1, bx.y + 3); ctx.lineTo(bx.x + 4, bx.y - 3); ctx.stroke(); }
      }
    }
  }

  // grids seen end on: a vertical dash-dot line with its head above
  function drawGrids() {
    const text = css("--text"), accent = css("--accent");
    for (const g of data.grids || []) {
      const a = toScreen(g.u, g.z1), b = toScreen(g.u, g.z0);
      const on = sel.has(g.id) || hoverGrid === g.id, col = on ? accent : text;
      ctx.save();
      ctx.strokeStyle = col; ctx.lineWidth = sel.has(g.id) ? 1.8 : 1; ctx.setLineDash([14, 4, 3, 4]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      ctx.setLineDash([]);
      const R = gridR(g);
      ctx.beginPath(); ctx.arc(a.x, a.y - R, R, 0, Math.PI * 2); ctx.fillStyle = css("--view-bg"); ctx.fill(); ctx.stroke();
      ctx.restore();
      label(g.name, a.x, a.y - R, col, "center", true);
    }
  }


  // ---- geometry index: bounds, alignable lines and snap points ---------------------------------
  function index() {
    boxes = new Map(); cands = []; verts = [];
    data.items.forEach((it, rank) => {
      for (const poly of it.polys) {
        let b = boxes.get(it.id);
        for (let i = 0; i < poly.length; i++) {
          const [u, z] = poly[i], [u2, z2] = poly[(i + 1) % poly.length];
          if (!b) { b = { u0: u, u1: u, z0: z, z1: z }; boxes.set(it.id, b); }
          b.u0 = Math.min(b.u0, u); b.u1 = Math.max(b.u1, u); b.z0 = Math.min(b.z0, z); b.z1 = Math.max(b.z1, z);
          verts.push([u, z]);
          const du = u2 - u, dz = z2 - z;
          if (Math.hypot(du, dz) > 1) {
            if (Math.abs(du) < 0.5) cands.push({ owner: it.id, a: [u, z], b: [u2, z2], vertical: true, rank });
            else if (Math.abs(dz) < 0.5) cands.push({ owner: it.id, a: [u, z], b: [u2, z2], vertical: false, rank });
          }
        }
      }
    });
    for (const l of data.levels) cands.push({ owner: l.id, a: [l.u0, l.z], b: [l.u1, l.z], vertical: false, rank: -1 });
    for (const g of data.grids || []) cands.push({ owner: g.id, a: [g.u, g.z0], b: [g.u, g.z1], vertical: true, rank: -1 });
  }
  const kindOf = (id) => (data.levels.some((l) => l.id === id) ? "level" : (data.grids || []).some((g) => g.id === id) ? "grid" : "item");
  const boxOf = (id) => {
    const l = data.levels.find((x) => x.id === id);
    if (l) return { u0: l.u0, u1: l.u1, z0: l.z, z1: l.z };
    const g = (data.grids || []).find((x) => x.id === id);
    if (g) return { u0: g.u, u1: g.u, z0: g.z0, z1: g.z1 };
    return boxes.get(id) || null;
  };
  const step = () => (cam.s > 0.3 ? 1 : cam.s > 0.03 ? 10 : 100);
  const snapU = (u) => Math.round(u / step()) * step();

  function segDist(px, py, a, b) {
    const dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((px - a.x) * dx + (py - a.y) * dy) / l2)) : 0;
    return Math.hypot(px - (a.x + dx * t), py - (a.y + dy * t));
  }
  // the lines under the cursor, nearest first; Tab steps to the next one
  function candsAt(sx, sy) {
    const out = [];
    for (const c of cands) {
      const d = segDist(sx, sy, toScreen(c.a[0], c.a[1]), toScreen(c.b[0], c.b[1]));
      if (d <= 6) out.push({ c, d });
    }
    out.sort((x, y) => x.d - y.d || y.c.rank - x.c.rank);
    return out.map((o) => o.c);
  }
  // a point to move from or to: a corner of something drawn, else a round number
  function snapPt(sx, sy) {
    const p = toWorld(sx, sy);
    let best = null;
    for (const [u, z] of verts) {
      const s = toScreen(u, z), d = Math.hypot(s.x - sx, s.y - sy);
      if (d < 9 && (!best || d < best.d)) best = { d, u, z };
    }
    if (best) return { u: best.u, z: best.z, snap: true };
    for (const l of data.levels) if (Math.abs(toScreen(0, l.z).y - sy) < 6) return { u: snapU(p.u), z: l.z, snap: false };
    return { u: snapU(p.u), z: Math.round(p.z / step()) * step(), snap: false };
  }

  // ---- temporary dimensions of the selected level, grid or element -----------------------------
  // Each carries what typing a new value does: the selection moves so the dimension reads that value.
  function tempDims() {
    if (!data || tool !== "select" || sel.size !== 1) return [];
    const id = [...sel][0], k = kindOf(id), out = [];
    if (k === "level") {
      const l = data.levels.find((x) => x.id === id), others = data.levels.filter((x) => x.id !== id);
      const below = others.filter((x) => x.z < l.z - 0.5).sort((a, b) => b.z - a.z)[0];
      const above = others.filter((x) => x.z > l.z + 0.5).sort((a, b) => a.z - b.z)[0];
      if (below) out.push({ axis: "v", from: below.z, to: l.z, at: l.u0, off: 90, value: l.z - below.z, apply: (v) => ({ dz: below.z + v - l.z }) });
      if (above) out.push({ axis: "v", from: l.z, to: above.z, at: l.u0, off: 90, value: above.z - l.z, apply: (v) => ({ dz: above.z - v - l.z }) });
      return out;
    }
    if (k === "grid") {
      const g = data.grids.find((x) => x.id === id), others = data.grids.filter((x) => x.id !== id);
      const left = others.filter((x) => x.u < g.u - 0.5).sort((a, b) => b.u - a.u)[0];
      const right = others.filter((x) => x.u > g.u + 0.5).sort((a, b) => a.u - b.u)[0];
      if (left) out.push({ axis: "h", from: left.u, to: g.u, at: g.z1, off: 34, value: g.u - left.u, apply: (v) => ({ du: left.u + v - g.u }) });
      if (right) out.push({ axis: "h", from: g.u, to: right.u, at: g.z1, off: 34, value: right.u - g.u, apply: (v) => ({ du: right.u - v - g.u }) });
      return out;
    }
    return elementDims(id);
  }

  // ---- reference lines: what an element's dimensions measure from and to ---------------------------
  // The model gives the lines of walls, openings and floors (`data.refs`); levels and grids are added here.
  const refId = (r) => `${r.id}|${r.key}`;
  function allRefs(vertical) {
    const out = (data.refs || []).filter((r) => r.vertical === vertical);
    if (vertical) for (const g of data.grids || []) out.push({ id: g.id, key: "grid", label: "Grid", vertical: true, pos: g.u, lo: g.z0, hi: g.z1 });
    else for (const l of data.levels) out.push({ id: l.id, key: "level", label: "Level", vertical: false, pos: l.z, lo: l.u0, hi: l.u1 });
    return out;
  }
  // the lines of one element a dimension uses unless the person has picked another: the faces, centreline or core of a wall and
  // the edges or centre of an opening, as set in Temporary Dimension Properties
  function defaultsOf(refs, set) {
    const want = (labels, fallback) => { const r = refs.filter((x) => labels.includes(x.label)); return r.length ? r : refs.filter((x) => fallback.includes(x.label)); };
    const wr = set.wall_ref || "faces", or = set.opening_ref || "opening";
    const wallLabels = wr === "centreline" ? ["Centreline"] : wr === "core_centre" ? ["Core centre"] : wr === "core_faces" ? ["Core face"] : ["Face"];
    const out = [];
    out.push(...want([...wallLabels, "Wall end"], ["Face", "Wall end"]).filter((x) => ["Face", "Core face", "Core centre", "Centreline", "Wall end"].includes(x.label)));
    out.push(...want(or === "centreline" ? ["Centre"] : ["Opening edge"], ["Opening edge"]).filter((x) => ["Opening edge", "Centre", "Reveal", "Frame edge"].includes(x.label)));
    out.push(...refs.filter((x) => ["Edge", "Top", "Bottom", "Sill", "Head", "Level", "Grid"].includes(x.label)));
    return out;
  }
  const pins = new Map(); // `${id}|${slot}` -> { own, target } ref ids picked by dragging the ends of a dimension
  let tgrip = null; // { dim, role, x, y, cand } while an end of a dimension is dragged to another line

  function elementDims(id) {
    const b = boxOf(id);
    if (!b) return [];
    const set = cb.tempSettings();
    const out = [];
    const mine = (data.refs || []).filter((r) => r.id === id);
    for (const vertical of [true, false]) {
      const own = mine.filter((r) => r.vertical === vertical);
      if (!own.length) continue;
      const others = allRefs(vertical).filter((r) => r.id !== id);
      const byEl = new Map();
      for (const r of others) { if (!byEl.has(r.id)) byEl.set(r.id, []); byEl.get(r.id).push(r); }
      const defTargets = [];
      for (const refs of byEl.values()) defTargets.push(...defaultsOf(refs, set).filter((r) => vertical || r.label === "Level"));
      const ownDef = defaultsOf(own, set);
      const span = vertical ? [b.z0, b.z1] : [b.u0, b.u1];
      // a line only counts when it overlaps this one across the other direction (a grid or level always does)
      const reaches = (r) => r.label === "Grid" || r.label === "Level" || (r.hi >= span[0] + 1 && r.lo <= span[1] - 1);
      const slots = vertical ? [["L", -1], ["R", 1]] : [["D", -1], ["U", 1]];
      for (const [slot, dir] of slots) {
        const pin = pins.get(`${id}|${slot}`) || {};
        const pool = ownDef.length ? ownDef : own;
        let o = pin.own && own.find((r) => refId(r) === pin.own);
        if (!o) o = pool.reduce((a, r) => (dir < 0 ? (r.pos < a.pos ? r : a) : (r.pos > a.pos ? r : a)), pool[0]);
        let t = pin.target && others.find((r) => refId(r) === pin.target);
        if (!t) {
          const near = defTargets.filter((r) => reaches(r) && (dir < 0 ? r.pos <= o.pos - 0.5 : r.pos >= o.pos + 0.5));
          t = near.reduce((a, r) => (!a || (dir < 0 ? r.pos > a.pos : r.pos < a.pos) ? r : a), null);
        }
        if (!t) continue;
        const sign = o.pos >= t.pos ? 1 : -1;
        const value = Math.abs(o.pos - t.pos);
        const apply = vertical ? (v) => ({ du: t.pos + sign * v - o.pos }) : (v) => ({ dz: t.pos + sign * v - o.pos });
        out.push({ axis: vertical ? "h" : "v", from: Math.min(o.pos, t.pos), to: Math.max(o.pos, t.pos), at: vertical ? o.lo : o.hi, off: vertical ? 24 : 26, value, apply, slot, key: `${id}|${slot}`, own: o, target: t, ownList: own, vertical });
      }
    }
    return out;
  }

  let dimGrips = []; // the ends of the selected element's dimensions: { x, y, dim, role }
  function drawDims() {
    dimHits = []; dimGrips = [];
    const accent = css("--accent");
    ctx.save();
    ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.font = "11px system-ui, sans-serif"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
    for (const d of tempDims()) {
      const live = tgrip && tgrip.dim.key === d.key && tgrip.cand;
      const own = live && tgrip.role === "own" ? tgrip.cand : d.own, target = live && tgrip.role === "target" ? tgrip.cand : d.target;
      const a = d.own ? own.pos : d.from, b = d.own ? target.pos : d.to;
      const value = d.own ? Math.abs(a - b) : d.value;
      let pa, pb, ea, eb;
      if (d.axis === "h") {
        const y = toScreen(0, d.at).y + d.off, y0 = toScreen(0, d.at).y;
        pa = { x: toScreen(a, 0).x, y }; pb = { x: toScreen(b, 0).x, y }; ea = [{ x: pa.x, y: y0 }, pa]; eb = [{ x: pb.x, y: y0 }, pb];
      } else {
        const x = Math.max(50, Math.min(size().w - 70, toScreen(d.at, 0).x + d.off)), x0 = Math.max(50, Math.min(size().w - 70, toScreen(d.at, 0).x));
        pa = { x, y: toScreen(0, a).y }; pb = { x, y: toScreen(0, b).y }; ea = [{ x: x0, y: pa.y }, pa]; eb = [{ x: x0, y: pb.y }, pb];
      }
      ctx.beginPath();
      for (const [p, q] of [ea, eb, [pa, pb]]) { ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); }
      for (const p of [pa, pb]) { ctx.moveTo(p.x - 3, p.y + 3); ctx.lineTo(p.x + 3, p.y - 3); }
      ctx.stroke();
      const text = String(Math.round(value)), w = ctx.measureText(text).width + 10, cx = (pa.x + pb.x) / 2, cy = (pa.y + pb.y) / 2;
      ctx.fillStyle = css("--view-bg"); ctx.fillRect(cx - w / 2, cy - 8, w, 16); ctx.strokeRect(cx - w / 2 + 0.5, cy - 7.5, w - 1, 15);
      ctx.fillStyle = accent; ctx.fillText(text, cx, cy);
      dimHits.push({ x: cx - w / 2, y: cy - 8, w, h: 16, dim: { ...d, value, apply: d.apply } });
      if (d.own) {
        // the ends can be dragged to another line: the grips sit on the witness lines
        for (const [p, role] of [[pa, "own"], [pb, "target"]]) {
          const q = { x: (role === "own" ? ea : eb)[0].x, y: (role === "own" ? ea : eb)[0].y };
          ctx.beginPath(); ctx.arc(q.x, q.y, 3.5, 0, Math.PI * 2); ctx.fillStyle = accent; ctx.fill(); ctx.strokeStyle = css("--view-bg"); ctx.lineWidth = 1; ctx.stroke(); ctx.strokeStyle = accent;
          dimGrips.push({ x: q.x, y: q.y, dim: d, role });
          void p;
        }
      }
    }
    if (tgrip && tgrip.cand) {
      // the line the dragged end would take, named beside the cursor
      const c = tgrip.cand, A = c.vertical ? toScreen(c.pos, c.hi) : toScreen(c.lo, c.pos), B = c.vertical ? toScreen(c.pos, c.lo) : toScreen(c.hi, c.pos);
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 2; ctx.setLineDash([6, 3]); ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke(); ctx.restore();
      label(c.label, tgrip.x + 14, tgrip.y - 14, accent);
    }
    ctx.restore();
  }
  // the lines an end of a dimension can be dragged to: the element's own for its end, any other for the far end
  // a click on a dimension's end steps to the next line on the same element, outermost first
  function cycleRef(g) {
    const dim = g.dim, id = Number(dim.key.split("|")[0]);
    const cur = g.role === "own" ? dim.own : dim.target;
    let list = g.role === "own" ? dim.ownList : allRefs(dim.vertical).filter((r) => r.id === cur.id && r.id !== id);
    const low = dim.slot === "L" || dim.slot === "D"; // the dimension sits below or left of the element
    const sign = g.role === "own" ? (low ? -1 : 1) : (low ? 1 : -1);
    const sorted = [...list].sort((a, b) => sign * (b.pos - a.pos));
    const lines = sorted.filter((r, i) => i === 0 || Math.abs(r.pos - sorted[i - 1].pos) > 0.5);
    if (lines.length < 2) return null;
    let at = lines.findIndex((r) => refId(r) === refId(cur));
    if (at < 0) at = 0;
    return lines[(at + 1) % lines.length];
  }
  function gripCand(g, sx, sy) {
    const dim = g.dim, id = Number(dim.key.split("|")[0]);
    const list = g.role === "own" ? dim.ownList : allRefs(dim.vertical).filter((r) => r.id !== id);
    let best = null;
    for (const r of list) {
      const A = r.vertical ? toScreen(r.pos, r.hi) : toScreen(r.lo, r.pos), B = r.vertical ? toScreen(r.pos, r.lo) : toScreen(r.hi, r.pos);
      const d = segDist(sx, sy, A, B);
      if (d <= 10 && (!best || d < best.d)) best = { d, r };
    }
    return best ? best.r : null;
  }
  const dimAt = (sx, sy) => dimHits.find((h) => sx >= h.x && sx <= h.x + h.w && sy >= h.y && sy <= h.y + h.h) || null;

  // ---- Modify tools -----------------------------------------------------------------------------
  function newEd() {
    if (tool === "align") return { stage: "ref", ref: null };
    return { stage: sel.size ? "a" : "pick", base: null };
  }
  function drawGhost(du, dz) {
    const accent = css("--accent");
    ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.2; ctx.setLineDash([5, 3]);
    for (const id of sel) {
      const k = kindOf(id);
      if (k === "level") {
        const l = data.levels.find((x) => x.id === id), a = toScreen(l.u0, l.z + dz), b = toScreen(l.u1, l.z + dz);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      } else if (k === "grid") {
        const g = data.grids.find((x) => x.id === id), a = toScreen(g.u + du, g.z1), b = toScreen(g.u + du, g.z0);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      } else {
        for (const it of data.items) if (it.id === id) for (const poly of it.polys) { polyPath(poly.map(([u, z]) => [u + du, z + dz])); ctx.stroke(); }
        for (const e of Det.elements()) if (e.id === id) for (const r of e.rings || [e.ring]) { polyPath(r.map(([u, z]) => [u + du, z + dz])); ctx.stroke(); }
      }
    }
    ctx.restore();
  }
  function drawEdit() {
    if (!isEdit() || !ed) return;
    const accent = css("--accent");
    if (tool === "align") {
      const cur = mouse ? candsAt(mouse.x, mouse.y) : [];
      const key = cur.map((c) => `${c.owner}:${c.a}`).join("|");
      if (key !== candKey) { candKey = key; candTab = 0; }
      const hot = cur.length ? cur[candTab % cur.length] : null;
      for (const [c, w, dash] of [[ed.ref, 2.4, [6, 3]], [hot, 2.6, []]]) {
        if (!c) continue;
        const a = toScreen(c.a[0], c.a[1]), b = toScreen(c.b[0], c.b[1]);
        ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = w; ctx.setLineDash(dash);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
      }
      return; // Align shows the highlighted line only, no snap glyphs
    }
    if (!mouse || !data) return;
    const p = snapPt(mouse.x, mouse.y);
    const s = toScreen(p.u, p.z);
    if (p.snap) { ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.strokeRect(s.x - 5, s.y - 5, 10, 10); ctx.restore(); }
    if (ed.stage === "b" && ed.base && tool === "array") {
      const s = arrayStep(p), a = toScreen(ed.base.u, ed.base.z), b = toScreen(ed.base.u + s.du, ed.base.z + s.dz);
      for (let i = 1; i < s.n; i++) drawGhost(s.su * i, s.sz * i);
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
      label(`${Math.round(Math.hypot(s.du, s.dz))}   Number: ${s.n}`, b.x + 12, b.y - 12, accent, "left", true);
    } else if (ed.stage === "b" && ed.base) {
      let { u, z } = p;
      if (mouse.shift) { if (Math.abs(u - ed.base.u) > Math.abs(z - ed.base.z)) z = ed.base.z; else u = ed.base.u; }
      drawGhost(u - ed.base.u, z - ed.base.z);
      const a = toScreen(ed.base.u, ed.base.z), b = toScreen(u, z);
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
      label(`${Math.round(u - ed.base.u)}, ${Math.round(z - ed.base.z)}`, b.x + 12, b.y - 12, accent, "left", true);
    }
  }
  // An array from the start point to a cursor position: the vector picked (du, dz), level or plumb with Shift or
  // Constrain, and the step from one member to the next (su, sz) by Move To.
  function arrayStep(p) {
    const o = cb.arrayOptions(), n = Math.max(2, o.arrayCount | 0);
    let { u, z } = p;
    if (mouse.shift || o.arrayConstrain) { if (Math.abs(u - ed.base.u) > Math.abs(z - ed.base.z)) z = ed.base.z; else u = ed.base.u; }
    const du = u - ed.base.u, dz = z - ed.base.z, k = o.arrayMoveTo === "last" ? 1 / (n - 1) : 1;
    return { n, du, dz, su: du * k, sz: dz * k, last: o.arrayMoveTo === "last" };
  }
  function editPrompt() {
    const name = tool === "move" ? "Move" : tool === "copy" ? "Copy" : tool === "array" ? "Array" : "Align";
    if (!ed) return name;
    if (tool === "array" && ed.stage === "b") return `Array: click where the ${cb.arrayOptions().arrayMoveTo === "last" ? "last member goes (the rest spaced evenly between)" : "second member goes (the spacing)"}. Shift keeps it level or plumb.`;
    if (tool === "align") return ed.stage === "ref" ? "Align: click the line to align to. Tab cycles overlapping lines." : "Align: click the line of the element to move.";
    if (ed.stage === "pick") return `${name}: select the elements to ${tool}, then click to start.`;
    if (ed.stage === "a") return `${name}: click the start point.`;
    return `${name}: click the end point. Shift keeps it level or plumb.`;
  }
  function editClick(e) {
    if (tool === "align") {
      const list = candsAt(e.offsetX, e.offsetY);
      if (!list.length) return;
      const c = list[candTab % list.length];
      candTab = 0;
      if (ed.stage === "ref") { ed = { stage: "tgt", ref: c }; cb.prompt(editPrompt()); render(); return; }
      const r = ed.ref;
      if (r.vertical !== c.vertical) { cb.prompt("Only parallel lines can be aligned.", true); return; }
      if (r.owner === c.owner) { cb.prompt("Pick a line of another element.", true); return; }
      const du = r.vertical ? r.a[0] - c.a[0] : 0, dz = r.vertical ? 0 : r.a[1] - c.a[1];
      if (Math.abs(du) + Math.abs(dz) < 1e-6) { cb.prompt("Those lines are already aligned.", true); return; }
      ed = { stage: "ref", ref: null };
      cb.moveBy([c.owner], du, dz, false);
      cb.prompt(editPrompt());
      render();
      return;
    }
    if (ed.stage === "pick") { pick(e); if (sel.size) { ed = { stage: "a", base: null }; cb.prompt(editPrompt()); render(); } return; }
    const p = snapPt(e.offsetX, e.offsetY);
    if (ed.stage === "a") { ed = { stage: "b", base: p }; cb.prompt(editPrompt()); render(); return; }
    if (tool === "array") {
      const s = arrayStep({ u: p.u, z: p.z });
      if (Math.abs(s.du) + Math.abs(s.dz) < 1e-6) return;
      cb.array([...sel], s.du, s.dz);
      setTool("select");
      render();
      return;
    }
    let { u, z } = p;
    if (e.shiftKey) { if (Math.abs(u - ed.base.u) > Math.abs(z - ed.base.z)) z = ed.base.z; else u = ed.base.u; }
    const du = u - ed.base.u, dz = z - ed.base.z;
    if (Math.abs(du) + Math.abs(dz) < 1e-6) return;
    const copy = tool === "copy";
    cb.moveBy([...sel], du, dz, copy);
    if (copy) ed = { stage: "a", base: null }; else setTool("select");
    cb.prompt(copy ? "Copied. Click another start point or press Esc." : "Moved.");
    render();
  }

  function render() {
    if (!printing) resize();
    const { w, h } = size();
    if (!printing) {
      ctx.fillStyle = css("--view-bg");
      ctx.fillRect(0, 0, w, h);
    }
    if (!data) return;
    const text = css("--text"), accent = css("--accent");
    // ground
    const ground = toScreen(0, datum());
    ctx.strokeStyle = css("--line"); ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(0, ground.y + 0.5); ctx.lineTo(w, ground.y + 0.5); ctx.stroke();
    // Hidden Line draws every surface in the page colour with black linework, so what is behind shows nothing; Wireframe draws the
    // linework only; the other styles fill with the material colours.
    const style = data.visual_style || "hidden_line", viewBg = css("--view-bg");
    const selD = expandIds(sel), hovD = expandIds(hoverIds || []);
    for (const it of data.items) {
      const selected = selD.has(it.id) || selD.has(it.part), hovered = hovD.has(it.id) || hovD.has(it.part);
      if (it.cut) continue; // cuts are drawn after everything else, below
      // depth cueing: the model has worked out how far toward the background this item fades
      const f = it.fade == null ? 1 : it.fade, ink = f < 1 ? mix(text, viewBg, f) : text, fill = `rgb(${it.fill[0]},${it.fill[1]},${it.fill[2]})`;
      for (const poly of it.polys) {
        polyPath(poly);
        // holes (the openings in a wall's edited profile) go through the fill and are outlined with it
        for (const h of it.holes || []) { h.forEach(([u, z], i) => { const s = toScreen(u, z); i ? ctx.lineTo(s.x, s.y) : ctx.moveTo(s.x, s.y); }); ctx.closePath(); }
        if (style === "hidden_line") { ctx.fillStyle = viewBg; ctx.fill("evenodd"); }
        else if (style !== "wireframe") { ctx.fillStyle = f < 1 ? mix(fill, viewBg, f) : fill; ctx.fill("evenodd"); }
        ctx.strokeStyle = selected ? accent : hovered ? accent : ink; ctx.lineWidth = selected ? 2 : 1; if (it.stroke !== false || selected) ctx.stroke();
      }
      for (const [p, q] of it.lines) {
        const a = toScreen(p[0], p[1]), b = toScreen(q[0], q[1]);
        ctx.strokeStyle = ink; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
    }
    // What the plane cuts: every fill first, then every outline, so two elements that overlap (a wall and a slab that
    // clash) both show their outlines. Nothing is trimmed for the person; a clash stays a clash until they resolve it.
    const cuts = data.items.filter((it) => it.cut), cutCol = (it) => (selD.has(it.id) || selD.has(it.part) ? accent : hovD.has(it.id) || hovD.has(it.part) ? accent : text);
    for (const it of [...cuts.filter((i) => i.category !== "Walls"), ...cuts.filter((i) => i.category === "Walls")]) drawCut(it, style, viewBg, cutCol(it), selD.has(it.id), "fill");
    for (const it of cuts) drawCut(it, style, viewBg, cutCol(it), selD.has(it.id), "line");
    // a jogged section unrolls its runs side by side: a light fold line marks each jog
    for (const u of data.folds || []) {
      const a = toScreen(u, data.frame.z_max), b = toScreen(u, data.frame.z_min);
      ctx.save(); ctx.strokeStyle = mix(text, viewBg, 0.45); ctx.lineWidth = 1; ctx.setLineDash([10, 4, 2, 4]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
    }
    if (groupEdit) drawGroupFade(text);
    drawCurtainGrids(accent, text, viewBg);
    drawJoinToggles(accent, viewBg);
    const hov = new Set(tool === "select" && hoverIds ? [...hovD] : []);
    Det.drawItems(selD, hov, accent, tool === "select", sel);
    drawGroupOrigins(accent);
    if (tool === "groupplace") drawGroupGhost(accent);
    drawGrids();
    drawCrop();
    for (const l of data.levels) {
      let shown = l;
      if (drag && drag.id === l.id) {
        shown = { ...l, ...(drag.kind === "move" ? { z: drag.z, elev: drag.z - datum() } : {}) };
        if (drag.kind === "end") shown = { ...l, [drag.end === 0 ? "u0" : "u1"]: drag.u };
      } else if (drag && drag.kind === "end" && drag.group) {
        // the ends locked to the one being dragged go with it
        const m = drag.group.find((q) => q.id === l.id);
        if (m) shown = { ...l, [sideOf(l, m.end) === 0 ? "u0" : "u1"]: drag.u };
      }
      drawLevel(shown, sel.has(l.id) ? "sel" : hoverLevel === l.id ? "hover" : "");
    }
    drawLevelPins();
    if (drag && drag.kind === "end" && drag.align) alignIndicator(drag.u, [data.levels.find((x) => x.id === drag.id).z, drag.align.level.z]);
    if (tool === "level" && mouse) {
      const p = toWorld(mouse.x, mouse.y), z = typed ? parseFloat(typed) + datum() : snapZ(p.z);
      if (Number.isFinite(z)) {
        const a = toScreen(-data.frame.left_mm, z), b = toScreen(data.frame.right_mm, z);
        ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.4; ctx.setLineDash([14, 4, 3, 4]);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
        label(typed ? `${typed}_` : fmtElev(z - datum()), b.x + 8, a.y - 10, accent, "left", true);
        // distance to the level below and above
        const lv = data.levels.map((l) => l.z).sort((x, y) => x - y);
        const below = [...lv].reverse().find((v) => v < z - 0.5), above = lv.find((v) => v > z + 0.5);
        const dim = (other) => {
          const s0 = toScreen(0, other), s1 = toScreen(0, z), x = mouse.x + 28;
          ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(x, s0.y); ctx.lineTo(x, s1.y); ctx.stroke();
          label(String(Math.round(Math.abs(z - other))), x + 4, (s0.y + s1.y) / 2, accent);
        };
        if (below !== undefined) dim(below);
        if (above !== undefined) dim(above);
      }
    }
    if (tool === "grid" && mouse) {
      const u = snapU(toWorld(mouse.x, mouse.y).u), a = toScreen(u, data.frame.z_max), b = toScreen(u, data.frame.z_min);
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.4; ctx.setLineDash([14, 4, 3, 4]);
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke(); ctx.restore();
      label(String(Math.round(u)), a.x + 8, a.y + 12, accent, "left", true);
    }
    Det.drawNotes(selD, hov, accent, tool === "select", sel);
    if (tool === "text") Det.preview(hover);
    if (detDrag) drawGhost(detDrag.d.x, detDrag.d.y);
    drawEdit();
    EDims.draw(data.dimensions || [], sel, hoverIds);
    if (tool === "dimension") EDims.drawTool();
    if ((tool === "door" || tool === "window") && mouse) drawOpeningGhost(accent);
    ESpots.draw(data.spots || [], sel, hoverIds);
    if (tool === "spot") ESpots.drawTool();
    drawDims();
    if (tool === "sketch") Sketch.draw(ctx);
    if (band) {
      const x = Math.min(band.x0, band.x1), y = Math.min(band.y0, band.y1), win = band.x1 >= band.x0;
      ctx.save(); ctx.strokeStyle = win ? accent : "#3c965a"; ctx.lineWidth = 1.2; ctx.setLineDash(win ? [] : [6, 4]);
      ctx.strokeRect(x + 0.5, y + 0.5, Math.abs(band.x1 - band.x0), Math.abs(band.y1 - band.y0)); ctx.restore();
    }
    // the snap under the cursor, with its name
    if (hover && hover.label && (tool === "text" || tool === "noteleader" || (tool === "sketch" && Sketch.wantsSnap()))) {
      const s2 = toScreen(hover.x, hover.y);
      Snap.drawGuides(ctx, hover, (p) => toScreen(p.x, p.y));
      Snap.glyph(ctx, hover.kind, s2.x, s2.y);
      ctx.font = "11px system-ui, sans-serif";
      const tw = ctx.measureText(hover.label).width;
      ctx.fillStyle = "#fffbe6"; ctx.strokeStyle = "#999"; ctx.lineWidth = 1;
      ctx.fillRect(s2.x + 10, s2.y + 8, tw + 8, 16); ctx.strokeRect(s2.x + 10.5, s2.y + 8.5, tw + 8, 16);
      ctx.fillStyle = "#222"; ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.fillText(hover.label, s2.x + 14, s2.y + 20);
    }
    if (!printing) label(data.name, 10, 14, css("--muted"), "left", true);
  }

  // The opening about to be placed: its width centred on the cursor; a window's sill at the cursor, a door standing on
  // the level line below it. The model decides the wall and the exact place when clicked.
  function drawOpeningGhost(accent) {
    const size = cb.placeSize(tool);
    if (!size) return;
    const p = toWorld(mouse.x, mouse.y);
    const below = data.levels.map((l) => l.z).filter((z) => z <= p.z + 1).sort((a, b) => b - a)[0];
    const z0 = tool === "door" ? (below ?? p.z) : Math.round(p.z / 10) * 10;
    const a = toScreen(p.u - size.w / 2, z0 + size.h), c = toScreen(p.u + size.w / 2, z0);
    ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.4; ctx.setLineDash([6, 4]);
    ctx.strokeRect(a.x, a.y, c.x - a.x, c.y - a.y); ctx.restore();
    if (tool === "window") label(`Sill ${fmtElev(z0 - datum())}`, c.x + 6, c.y - 4, accent);
  }

  // Edit Group: everything fades but the group's members, drawn again in the ink colour
  function drawGroupFade(ink) {
    const mem = expandIds([groupEdit.id]);
    ctx.save(); ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.fillStyle = "rgba(255,255,255,0.62)"; ctx.fillRect(0, 0, canvas.width, canvas.height); ctx.restore();
    ctx.save(); ctx.strokeStyle = ink; ctx.lineWidth = 1.4;
    for (const it of data.items) if (mem.has(it.id)) for (const poly of it.polys) { polyPath(poly); ctx.stroke(); }
    ctx.restore();
  }
  // a selected detail group drawn in this view shows its origin
  function drawGroupOrigins(accent) {
    for (const g of modelGroups) {
      if (!sel.has(g.id) || g.view_id !== data.view_id) continue;
      const o = toScreen(g.origin.x, g.origin.y);
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.fillStyle = "#fff";
      ctx.fillRect(o.x - 5, o.y - 5, 10, 10); ctx.strokeRect(o.x - 5, o.y - 5, 10, 10);
      ctx.beginPath(); ctx.moveTo(o.x - 9, o.y); ctx.lineTo(o.x + 9, o.y); ctx.moveTo(o.x, o.y - 9); ctx.lineTo(o.x, o.y + 9); ctx.stroke(); ctx.restore();
    }
  }
  // Place Detail Group: an unmirrored instance in this view follows the cursor by its origin
  function drawGroupGhost(accent) {
    if (!groupPlace || !hover) return;
    const g = modelGroups.find((x) => x.type_id === groupPlace.typeId && !x.mirrored && x.view_id === data.view_id);
    if (!g) return;
    const a = (-g.angle_deg * Math.PI) / 180, c = Math.cos(a), sn = Math.sin(a), mem = expandIds([g.id]);
    const map = ([u, z]) => [hover.x + (u - g.origin.x) * c - (z - g.origin.y) * sn, hover.y + (u - g.origin.x) * sn + (z - g.origin.y) * c];
    ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1.4; ctx.setLineDash([6, 4]);
    for (const e of Det.elements()) if (mem.has(e.id)) for (const r of e.rings || [e.ring]) { polyPath(r.map(map)); ctx.stroke(); }
    ctx.restore();
  }

  // ---- inline editing of a level's name or elevation -------------------------------------
  function closeInput() { const i = input; input = null; if (i) { i.onblur = null; i.remove(); } }
  function openInput(x, y, value, onDone) {
    closeInput();
    input = document.createElement("input");
    input.className = "elev-input";
    input.value = value;
    input.style.left = `${x}px`; input.style.top = `${y}px`;
    canvas.parentElement.appendChild(input);
    input.focus(); input.select();
    const done = (ok) => { if (!input) return; const v = input.value; closeInput(); if (ok && v !== null && v !== undefined) onDone(v); };
    input.onkeydown = (e) => { e.stopPropagation(); if (e.key === "Enter") done(true); else if (e.key === "Escape") done(false); };
    input.onblur = () => done(false);
  }

  // ---- events ------------------------------------------------------------------------------
  function pick(ev) {
    // the nearest thing under the cursor, or the one Tab stepped to
    const h = hoverPick(ev.offsetX, ev.offsetY);
    const target = h !== null ? [h] : [];
    tabIdx = 0;
    // Edit Group's Add and Remove take what is clicked instead of selecting it
    if (groupEdit && groupEdit.mode) { if (h !== null) cb.groupPick([h]); render(); return; }
    if (ev.shiftKey) target.forEach((i) => sel.delete(i));
    else if (ev.ctrlKey) target.forEach((i) => sel.add(i));
    else sel = new Set(target);
    cb.select([...sel]);
    render();
  }

  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("pointerdown", (e) => {
    canvas.setPointerCapture(e.pointerId);
    if (e.button === 1 || e.button === 2) { pan = { x: e.clientX, y: e.clientY, moved: false, button: e.button }; return; }
    down = { x: e.offsetX, y: e.offsetY };
    if (tool === "sketch" && e.button === 0 && Sketch.gripDown(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && data && e.button === 0 && Det.gripDown(e.offsetX, e.offsetY)) { down = null; return; }
    if (data && e.button === 0 && EDims.lockDown(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && data && e.button === 0 && EDims.down(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && data && e.button === 0 && ESpots.down(e.offsetX, e.offsetY)) { down = null; return; }
    if (tool === "select" && data) {
      // pressing on a selected detail line, region or text note and moving carries the selected ones
      const dids = new Set(Det.elements().map((x) => x.id)), at = detailAt(e.offsetX, e.offsetY);
      if (at.some((i) => sel.has(i)) && [...sel].every((i) => dids.has(i))) down.drag = { ids: [...sel], p0: snap(e.offsetX, e.offsetY) };
      const cg = cropBoxes.find((q) => Math.abs(e.offsetX - q.x) <= 7 && Math.abs(e.offsetY - q.y) <= 7);
      if (cg) { cropDrag = { side: cg.side, v: 0, moved: false }; down = null; return; }
      const pin = levelPins.find((q) => padlockHit(e.offsetX, e.offsetY, q.x, q.y));
      if (pin) { down = null; cb.toggleLevelLock(pin); return; }
      const tg = dimGrips.find((q) => Math.hypot(e.offsetX - q.x, e.offsetY - q.y) <= 7);
      if (tg) { down = null; tgrip = { dim: tg.dim, role: tg.role, x: e.offsetX, y: e.offsetY, px: e.offsetX, py: e.offsetY, cand: null, grip: tg }; return; }
      const dh = dimAt(e.offsetX, e.offsetY);
      if (dh) { down = null; dimDown = dh; return; } // the box opens on release, so the click does not take its focus back
      const g = gripAt(e.offsetX, e.offsetY);
      if (g && g.kind === "bubble") { down = null; cb.toggleBubble(g.id, g.end, g.l.bubbles); return; }
      if (g && g.kind === "end") { drag = { id: g.id, kind: "end", end: g.end, u: g.end === 0 ? g.l.u0 : g.l.u1, moved: false, group: lockGroup(g.id, modelEnd(g.l, g.end)), align: null }; down = null; return; }
      // pressing on a selected curtain grid line placed by hand and moving drags it along the wall
      const sg = sel.size === 1 ? gridLine([...sel][0]) : null;
      if (sg && !sg.l.typed && !sg.l.edge && sg.l.segs.some((s) => near(e.offsetX, e.offsetY, s, 5))) cgDrag = { id: sg.l.id, f: sg.f, vertical: sg.l.vertical, start: sg.l.at, at: sg.l.at, x: e.offsetX, y: e.offsetY, moved: false };
      const lid = levelAt(e.offsetX, e.offsetY);
      if (lid !== null && sel.has(lid) && !itemsAt(e.offsetX, e.offsetY).length) {
        const l = data.levels.find((x) => x.id === lid);
        drag = { id: lid, kind: "move", z: l.z, start: l.z, moved: false };
        down = null;
      }
    }
  });
  canvas.addEventListener("pointermove", (e) => {
    mouse = { x: e.offsetX, y: e.offsetY, shift: e.shiftKey };
    if (pan) {
      const dx = e.clientX - pan.x, dy = e.clientY - pan.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) pan.moved = true;
      pan.x = e.clientX; pan.y = e.clientY;
      cam.u -= dx / cam.s; cam.z += dy / cam.s;
      render();
      return;
    }
    if (!data) return;
    if (Det.dragging) { Det.gripMove(e.offsetX, e.offsetY); return; }
    if (EDims.dragging) { EDims.dragMove(e.offsetX, e.offsetY); return; }
    if (ESpots.dragging) { ESpots.dragMove(e.offsetX, e.offsetY); return; }
    if (tool === "spot") { ESpots.move(e.offsetX, e.offsetY); return; }
    if (tool === "dimension") { const p = toWorld(e.offsetX, e.offsetY); cb.cursor(`${Math.round(p.u)}, ${fmtElev(p.z - datum())}`); EDims.move(e.offsetX, e.offsetY); return; }
    if (tool === "door" || tool === "window") { const p = toWorld(e.offsetX, e.offsetY); cb.cursor(`${Math.round(p.u)}, ${fmtElev(p.z - datum())}`); render(); return; }
    if (Sketch.gripDragging && tool === "sketch") { hover = snap(e.offsetX, e.offsetY); Sketch.gripMove(e.offsetX, e.offsetY, hover); return; }
    const moved = down && Math.hypot(e.offsetX - down.x, e.offsetY - down.y) > 5;
    if (moved && down.drag && !detDrag) detDrag = { ids: down.drag.ids, p0: down.drag.p0, d: { x: 0, y: 0 } };
    if (detDrag) {
      hover = snap(e.offsetX, e.offsetY);
      detDrag.d = { x: hover.x - detDrag.p0.x, y: hover.y - detDrag.p0.y };
      if (e.shiftKey) { if (Math.abs(detDrag.d.x) >= Math.abs(detDrag.d.y)) detDrag.d.y = 0; else detDrag.d.x = 0; }
      render();
      return;
    }
    if (moved && !band && tool === "sketch" && Sketch.canBand()) band = { x0: down.x, y0: down.y, x1: e.offsetX, y1: e.offsetY };
    if (band) { band.x1 = e.offsetX; band.y1 = e.offsetY; render(); return; }
    if (!lastMove || Math.hypot(e.offsetX - lastMove.x, e.offsetY - lastMove.y) > 3) snapCycle = 0;
    if (tool === "sketch" || tool === "text" || tool === "noteleader" || tool === "groupplace") {
      lastMove = { x: e.offsetX, y: e.offsetY };
      hover = tool === "sketch" ? snap(e.offsetX, e.offsetY) : Det.toolSnap(e.offsetX, e.offsetY);
      cb.cursor(`${Math.round(hover.x)}, ${fmtElev(hover.y - datum())}`);
      if (tool === "sketch") Sketch.move(hover, e.offsetX, e.offsetY);
      render();
      return;
    }
    if (cropDrag) {
      const w = toWorld(e.offsetX, e.offsetY), s = cropDrag.side;
      cropDrag.moved = true;
      cropDrag.v = s === "left" || s === "right" ? Math.round(w.u / 10) * 10 : Math.round(w.z / 10) * 10;
      render();
      return;
    }
    if (tgrip) { tgrip.x = e.offsetX; tgrip.y = e.offsetY; tgrip.cand = gripCand(tgrip.grip, e.offsetX, e.offsetY); canvas.style.cursor = tgrip.cand ? "crosshair" : "default"; render(); return; }
    const p = toWorld(e.offsetX, e.offsetY);
    cb.cursor(`${fmtElev(p.z - datum())}`);
    if (drag) {
      drag.moved = true;
      if (drag.kind === "move") drag.z = snapZ(p.z);
      else {
        // the end lines up with the end of another level: snap to it and, on release, lock to it
        const l = data.levels.find((x) => x.id === drag.id), al = levelAlign(l, drag.end, p.u);
        drag.align = al;
        drag.u = al ? al.u : Math.round(p.u / 10) * 10;
      }
      render();
      return;
    }
    if (tool === "curtaingrid") { cgHover = curtainGridAt(e.offsetX, e.offsetY); cb.prompt(promptText()); render(); return; }
    if (tool === "cgsegments") { segHover = segmentAt(e.offsetX, e.offsetY); render(); return; }
    const jh = !down && joinAt(e.offsetX, e.offsetY);
    if ((jh ? `${jh.k},${jh.j}` : "") !== (joinHover ? `${joinHover.k},${joinHover.j}` : "")) {
      joinHover = jh || null;
      cb.cursor(jh ? (jh.through ? "Break the mullion here" : "Join the mullion through here") : "");
      render();
    }
    if (cgDrag) {
      if (!cgDrag.moved && Math.hypot(e.offsetX - cgDrag.x, e.offsetY - cgDrag.y) < 4) return;
      cgDrag.moved = true;
      const v = cgDrag.vertical ? alongOf(cgDrag.f, p.u) : p.z - cgDrag.f.z0;
      if (v != null) cgDrag.at = Math.round(v / step()) * step();
      cb.cursor(`${Math.round(cgDrag.at)}`);
      render();
      return;
    }
    if (tool === "select") {
      if (!lastMove || Math.hypot(e.offsetX - lastMove.x, e.offsetY - lastMove.y) > 3) tabIdx = 0;
      lastMove = { x: e.offsetX, y: e.offsetY };
      const h = hoverPick(e.offsetX, e.offsetY);
      hoverIds = h !== null && kindOf(h) === "item" ? [h] : null;
      hoverLevel = h !== null && kindOf(h) === "level" ? h : null;
      hoverGrid = h !== null && kindOf(h) === "grid" ? h : null;
      const g = gripAt(e.offsetX, e.offsetY);
      canvas.style.cursor = g ? (g.kind === "bubble" ? "pointer" : "ew-resize") : hoverLevel !== null && sel.has(hoverLevel) ? "ns-resize" : "default";
    }
    render();
  });
  canvas.addEventListener("pointerup", (e) => {
    if (Det.dragging) { down = null; Det.gripUp(); return; }
    if (EDims.dragging) { down = null; EDims.up(e.offsetX, e.offsetY); return; }
    if (ESpots.dragging) { down = null; ESpots.up(e.offsetX, e.offsetY); return; }
    if (Sketch.gripDragging && tool === "sketch") { down = null; Sketch.gripUp(); return; }
    if (detDrag) {
      const d = detDrag; detDrag = null; down = null; hover = null;
      if (Math.hypot(d.d.x, d.d.y) > 0.5) cb.moveBy(d.ids, Math.round(d.d.x * 1000) / 1000, Math.round(d.d.y * 1000) / 1000, !!e.ctrlKey);
      render();
      return;
    }
    if (band) {
      const b = band; band = null; down = null;
      const a = W(b.x0, b.y0), c = W(b.x1, b.y1);
      Sketch.bandSelect({ x0: Math.min(a.x, c.x), y0: Math.min(a.y, c.y), x1: Math.max(a.x, c.x), y1: Math.max(a.y, c.y) }, b.x1 >= b.x0, e);
      render();
      return;
    }
    if (cropDrag) {
      const c = cropDrag; cropDrag = null;
      if (c.moved) cb.cropSection(c.side, c.v);
      render();
      return;
    }
    if (tgrip) {
      const g = tgrip; tgrip = null;
      let pick = g.cand;
      if (!pick && Math.hypot(e.offsetX - g.px, e.offsetY - g.py) < 4) pick = cycleRef(g);
      if (pick) {
        const k = g.dim.key, old = pins.get(k) || {};
        pins.set(k, { ...old, [g.role]: refId(pick) });
      }
      render();
      return;
    }
    if (dimDown) {
      const dh = dimDown; dimDown = null;
      const d = dh.dim, id = [...sel][0];
      openInput(dh.x, dh.y - 4, String(Math.round(d.value)), (v) => {
        const n = parseLengthMM(v);
        if (!Number.isFinite(n) || n < 0) return;
        const r = d.apply(n);
        cb.moveBy([id], r.du || 0, r.dz || 0, false);
      });
      return;
    }
    if (pan) {
      const wasClick = !pan.moved && pan.button === 2;
      pan = null;
      if (wasClick && tool === "sketch") { Sketch.rightClick(); return; }
      if (wasClick && tool === "dimension") { if (!EDims.cancel()) setTool("select"); cb.prompt(promptText()); render(); return; }
      if (wasClick && tool === "spot") { if (!ESpots.cancel()) setTool("select"); cb.prompt(promptText()); render(); return; }
      if (wasClick && tool !== "select") setTool("select");
      return;
    }
    if (cgDrag) {
      const g = cgDrag; cgDrag = null;
      if (g.moved) {
        down = null;
        if (Math.abs(g.at - g.start) > 0.5) cb.curtainCall("patch_curtain_part", { id: g.id, patch: { at: g.at } });
        render();
        return;
      }
    }
    if (drag) {
      const d = drag; drag = null;
      const l = data.levels.find((x) => x.id === d.id);
      if (d.moved && l) {
        if (d.kind === "move" && Math.abs(d.z - d.start) > 0.5) cb.moveLevel(d.id, d.z - datum());
        else if (d.kind === "end") cb.setExtent(d.id, d.end, d.u, l, d.align);
      }
      render();
      return;
    }
    if (down && Math.hypot(e.offsetX - down.x, e.offsetY - down.y) < 5) {
      if (tool === "sketch") {
        const p = snap(e.offsetX, e.offsetY);
        Snap.clearOverride(); snapCycle = 0; down = null;
        Sketch.click(p, e.offsetX, e.offsetY, e);
        return;
      }
      if (tool === "text" || tool === "noteleader") {
        const p = Det.toolSnap(e.offsetX, e.offsetY);
        Snap.clearOverride(); snapCycle = 0; down = null;
        if (tool === "noteleader") Det.leaderClick(p); else Det.toolClick(p);
        cb.prompt(promptText()); render();
        return;
      }
      if (tool === "dimension") { down = null; EDims.click(e.offsetX, e.offsetY, e); cb.prompt(promptText()); return; }
      if (tool === "groupplace") { const p = Det.toolSnap(e.offsetX, e.offsetY); down = null; if (groupPlace) cb.placeGroup(groupPlace.typeId, { x: p.x, y: p.y }); return; }
      if (tool === "door" || tool === "window") { const w = toWorld(e.offsetX, e.offsetY); down = null; cb.place(tool, Math.round(w.u), Math.round(w.z)); return; }
      if (tool === "spot") { down = null; ESpots.click(e.offsetX, e.offsetY, e); cb.prompt(promptText()); return; }
      if (tool === "curtaingrid") {
        down = null;
        const c = curtainGridAt(e.offsetX, e.offsetY);
        if (!c) return;
        if (cgMode === "except") cgPending = { wall: c.wall, vertical: c.vertical, at: c.at };
        cb.curtainCall("add_curtain_line", { wall: c.wall, vertical: c.vertical, at: c.at, ...(c.only != null ? { only: c.only } : {}) });
        return;
      }
      if (tool === "cgsegments") { down = null; const s = segmentAt(e.offsetX, e.offsetY); if (s != null) cb.curtainCall("toggle_curtain_segment", { id: segLine, s }); return; }
      const jt = joinAt(e.offsetX, e.offsetY);
      if (jt) { down = null; cb.curtainCall("toggle_curtain_join", { wall: jt.wall, k: jt.k, j: jt.j }); return; }
      if (tool === "grid") cb.addGrid(snapU(toWorld(e.offsetX, e.offsetY).u));
      else if (isEdit()) editClick(e);
      else if (tool === "level") {
        const z = typed ? parseFloat(typed) + datum() : snapZ(toWorld(e.offsetX, e.offsetY).z);
        typed = "";
        if (Number.isFinite(z)) cb.addLevel(z - datum());
      } else pick(e);
    }
    down = null;
  });
  canvas.addEventListener("dblclick", (e) => {
    if (tool === "sketch") { Sketch.dblclick(); return; }
    if (tool !== "select" || !data) return;
    if (EDims.dblclick(e.offsetX, e.offsetY)) return; // a dimension's text dialog is open
    // double-clicking a group opens it in Edit Group
    const gh = hoverPick(e.offsetX, e.offsetY);
    if (gh !== null && modelGroups.some((g) => g.id === gh)) { cb.editGroup(gh); return; }
    if (ESpots.dblclick(e.offsetX, e.offsetY)) return; // a bend came out of a spot's leader
    // a text note opens its text; a detail line, Detail Line with it selected; a region, Edit Boundary
    const did = detailAt(e.offsetX, e.offsetY)[0];
    if (did !== undefined) {
      if ((data.text_notes || []).some((x) => x.id === did)) cb.editNote(did);
      else if ((data.detail_lines || []).some((x) => x.id === did)) cb.editDetail(did);
      else if ((data.regions || []).some((x) => x.id === did)) cb.editRegion(did);
      return;
    }
    const lid = levelAt(e.offsetX, e.offsetY);
    if (lid === null || itemsAt(e.offsetX, e.offsetY).length) return;
    const l = data.levels.find((x) => x.id === lid);
    // double click near the head renames; anywhere on the line edits the elevation
    const b = toScreen(l.u1, l.z);
    if (e.offsetX > b.x) openInput(e.offsetX, e.offsetY, l.name, (v) => cb.renameLevel(lid, v.trim()));
    else openInput(e.offsetX, e.offsetY, String(Math.round(l.elev)), (v) => { const n = parseFloat(v); if (Number.isFinite(n)) cb.editElevation(lid, n); });
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const before = toWorld(e.offsetX, e.offsetY);
    cam.s = Math.min(1, Math.max(0.004, cam.s * Math.exp(-e.deltaY * 0.0015)));
    const after = toWorld(e.offsetX, e.offsetY);
    cam.u += before.u - after.u; cam.z += before.z - after.z;
    render();
  }, { passive: false });
  new ResizeObserver(render).observe(canvas);

  function setTool(t) {
    if (t !== "text") Det.clearPicks();
    if (t !== "noteleader") Det.stopLeader();
    if (t !== "dimension") EDims.stop();
    if (t !== "spot") ESpots.stop();
    tool = t; typed = ""; candTab = 0; hover = null; band = null; cgHover = null; segHover = null;
    if (t !== "cgsegments") segLine = null;
    ed = isEdit() ? newEd() : null;
    canvas.style.cursor = ["curtaingrid", "cgsegments", "level", "grid", "text", "noteleader", "sketch", "dimension", "spot", "door", "window", "groupplace"].includes(t) || isEdit() ? "crosshair" : "default";
    cb.prompt(promptText());
    if (typeof cb.tool === "function") cb.tool(t);
    render();
  }
  function promptText() {
    if (tool === "sketch") return Sketch.promptText();
    if (tool === "dimension") return EDims.prompt();
    if (tool === "door" || tool === "window") return `${tool === "door" ? "Door" : "Window"}: click on a wall seen in this view.${tool === "window" ? " The sill goes at the height clicked." : ""} Esc to finish.`;
    if (tool === "spot") return ESpots.prompt();
    if (tool === "text" || tool === "noteleader") return Det.promptText(tool);
    if (tool === "groupplace") return groupPlace ? `Click to place ${groupPlace.name} by its origin. Esc to stop.` : "Pick a group to place.";
    if (tool === "level") return typed ? `Elevation ${typed}. Press Enter to place the level, Esc to clear.` : "Level: click at the height of the new level, or type its elevation and press Enter. Esc to finish.";
    if (tool === "grid") return "Grid: click where the grid line goes. It runs away from the view. Esc to finish.";
    if (tool === "curtaingrid") {
      const how = { all: "All Segments", one: "One Segment", except: "All Except Picked" }[cgMode];
      return cgHover
        ? `Curtain Grid (${how}): click to place a ${cgHover.vertical ? "vertical" : "horizontal"} grid line ${Math.round(cgHover.at)} mm ${cgHover.vertical ? "from the wall's start" : "above its base"}. Esc to finish.`
        : `Curtain Grid (${how}): point at a curtain wall. Near the top or bottom of a panel the line runs vertical; near its side, horizontal. Esc to finish.`;
    }
    if (tool === "cgsegments") return "Add/Remove Segments: click a segment of the grid line to take it out or put it back. Esc to finish.";
    if (isEdit()) return editPrompt();
    return "Ready.";
  }

  // ---- detail items, snapping and the sketch's host ------------------------------------------------------------------
  const P = (p) => toScreen(p.x, p.y);
  const W = (sx, sy) => { const w = toWorld(sx, sy); return { x: w.u, y: w.z }; };
  // detail items a sketch here is holding: the view's detail lines while Detail Line is on, a region's edges in Edit Boundary
  function heldBySketch(x) {
    if (tool !== "sketch" || !Sketch.active) return false;
    if (Sketch.kind === "detail") return x.line != null;
    return Sketch.kind === "region" && Sketch.editingId != null && x.region === Sketch.editingId;
  }
  const inRing = (p, r) => {
    let inside = false;
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) if ((r[i][1] > p.y) !== (r[j][1] > p.y) && p.x < ((r[j][0] - r[i][0]) * (p.y - r[i][1])) / (r[j][1] - r[i][1]) + r[i][0]) inside = !inside;
    return inside;
  };
  // the detail lines, regions and text notes under the cursor (a region by its inside, holes left out)
  function detailAt(sx, sy) {
    if (!data) return [];
    const p = W(sx, sy), out = [];
    for (const e of Det.elements()) {
      const rings = e.rings || [e.ring];
      const hit = e.kind === "region" ? rings.reduce((n, r) => n + (inRing(p, r) ? 1 : 0), 0) % 2 === 1 : rings.some((r) => inRing(p, r));
      if (hit && !out.includes(e.id)) out.push(e.id);
    }
    return out.reverse(); // the last drawn is on top
  }
  // snapping: the edges of everything drawn, the levels and grids, the detail lines
  let snapPts = [];
  function buildSnap() {
    const segs = [];
    snapPts = [];
    if (!data) { Snap.build(segs, "elev"); return; }
    for (const it of data.items) {
      for (const poly of it.polys) for (let i = 0; i < poly.length; i++) { const a = poly[i], b = poly[(i + 1) % poly.length]; if (Math.hypot(b[0] - a[0], b[1] - a[1]) > 0.5) segs.push([a[0], a[1], b[0], b[1], 3]); }
      for (const [a, b] of it.lines || []) segs.push([a[0], a[1], b[0], b[1]]);
    }
    for (const l of data.levels) segs.push([l.u0, l.z, l.u1, l.z]);
    for (const g of data.grids || []) segs.push([g.u, g.z0, g.u, g.z1]);
    const det = data.detail, ds = data.scale || 100;
    if (det) for (const st of det.strokes) {
      if (heldBySketch(st)) continue;
      const q = st.pts.map(([x, y]) => [x * ds, y * ds]), straight = q.length === 2;
      for (let i = 0; i + 1 < q.length; i++) segs.push(straight ? [...q[i], ...q[i + 1], 4] : [...q[i], ...q[i + 1], 2]);
      if (!straight && !st.closed) snapPts.push({ x: q[0][0], y: q[0][1], kind: "endpoint" }, { x: q[q.length - 1][0], y: q[q.length - 1][1], kind: "endpoint" });
    }
    Snap.build(segs, "elev");
  }
  // the snapped point under the cursor; `o` asks for one measured from a point (a text note's leader)
  function snap(sx, sy, o) {
    if (Snap.owner !== "elev") buildSnap();
    const w = W(sx, sy), base = { zoom: cam.s, cycle: snapCycle, shift: !!(mouse && mouse.shift), gridStep: 0 };
    if (o) return Snap.resolve(w, { ...base, last: o.last || null, extra: [], points: snapPts, hvDeg: o.hvDeg, track: o.track });
    if (tool === "sketch" && Sketch.active) return Snap.resolve(w, { ...base, last: Sketch.last(), extra: Sketch.segs(), points: [...Sketch.points(), ...snapPts], track: Sketch.trackPoints() });
    return Snap.resolve(w, { ...base, last: null, extra: [], points: snapPts });
  }
  // the model's edges near the cursor, for Pick Lines
  function edgesNear(sx, sy) {
    const out = [];
    for (const it of data.items) for (const poly of it.polys) for (let i = 0; i < poly.length; i++) {
      const a = poly[i], b = poly[(i + 1) % poly.length], d = segDist(sx, sy, toScreen(a[0], a[1]), toScreen(b[0], b[1]));
      if (d <= 8) out.push({ d, a: { x: a[0], y: a[1] }, b: { x: b[0], y: b[1] } });
    }
    return out.sort((p, q) => p.d - q.d).map(({ a, b }) => ({ a, b }));
  }
  const Det = makeDetails({
    ctx: () => ctx, toScreen: P, toWorld: W, css, render: () => render(),
    faded: (id) => groupEdit != null && !chainOf(id).includes(groupEdit.id),
    zoom: () => cam.s,
    scale: () => (data && data.scale) || 100,
    data: () => data || {},
    thin: () => Plan.thinLines,
    held: (x) => heldBySketch(x),
    snap: (sx, sy, o) => snap(sx, sy, o),
    shift: () => !!(mouse && mouse.shift),
    options: () => DetailCallbacks.options(),
    cb: DetailCallbacks,
  });
  const elevHost = {
    get ctx() { return ctx; },
    get canvas() { return canvas; },
    size, toScreen: P, toWorld: W, css, render: () => render(),
    label: (text, at, color) => {
      ctx.font = "11px system-ui, sans-serif";
      const m = ctx.measureText(text);
      ctx.fillStyle = css("--panel"); ctx.fillRect(at.x - m.width / 2 - 3, at.y - 8, m.width + 6, 16);
      ctx.fillStyle = color; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillText(text, at.x, at.y);
    },
    scale: () => cam.s,
    wallAt: () => null, wallById: () => null, wallChain: (id) => [id],
    edgeAt: (sx, sy) => edgesNear(sx, sy)[0] || null,
    edgesNear: (sx, sy) => edgesNear(sx, sy),
    cycleSnap: (d) => { snapCycle += d; if (mouse) { hover = snap(mouse.x, mouse.y); Sketch.move(hover, mouse.x, mouse.y); } render(); },
  };

  // Aligned and Linear dimensions in this view, measured across it and up
  const EDims = makeDims();
  EDims.attach({
    ctx: () => ctx, toScreen: P, toWorld: W, label: elevHost.label, css, render: () => render(),
    scale: () => cam.s,
    paperPx: () => ((data && data.scale) || 100) * cam.s,
    thin: () => Plan.thinLines,
    dims: () => (data && data.dimensions) || [],
    selection: () => [...sel],
    select: (ids) => { sel = new Set(ids); cb.select([...sel]); render(); },
    viewId: () => data && data.view_id,
    settings: () => cb.tempSettings(),
    prompt: (t) => cb.prompt(t),
    call: (m, p) => cb.dimCall(m, p),
    refresh: () => cb.dimRefresh(),
    openText: (d, s2) => cb.dimText(d, s2),
    setTool: (t) => setTool(t),
    canvasRect: () => canvas.getBoundingClientRect(),
  });

  // spot elevations on the lines this view names
  const ESpots = makeSpots();
  ESpots.attach({
    elevation: true,
    ctx: () => ctx, toScreen: P, toWorld: W, label: elevHost.label, css, render: () => render(),
    scale: () => cam.s,
    paperPx: () => ((data && data.scale) || 100) * cam.s,
    thin: () => Plan.thinLines,
    spots: () => (data && data.spots) || [],
    selection: () => [...sel],
    viewId: () => data && data.view_id,
    prompt: (t) => cb.prompt(t),
    call: (m, p) => cb.dimCall(m, p),
    refresh: () => cb.dimRefresh(),
    setTool: (t) => setTool(t),
  });

  function fit() {
    if (!data) return;
    const f = data.frame, h = f.z_max - f.z_min;
    // leave room for the level heads: they stand past each end of the lines at their paper size
    const reach = Math.max(0, ...data.levels.map((l) => (l.head ? Ann.extent(l.head).maxX : 0))) * (data.scale || 100);
    const w = f.left_mm + f.right_mm + 2 * reach;
    const { w: pw, h: ph } = size();
    cam = { u: (f.right_mm - f.left_mm) / 2, z: (f.z_max + f.z_min) / 2, s: Math.min((pw - 80) / (w || 1), (ph - 80) / (h || 1)) || 0.1 };
    if (!(cam.s > 0)) cam.s = 0.1;
  }

  // Draw an elevation or section's data onto another canvas context (a sheet, a page): `o` is { data, cam, w, h }, the
  // camera along the view and up (mm) and pixels per mm. Nothing selected, no tool, no crop frame.
  function renderTo(target, o) {
    const keep = { ctx, data, cam, sel, hoverIds, tool, drag, mouse, hover, band };
    ctx = target; data = o.data; cam = o.cam; printSize = { w: o.w, h: o.h }; printing = true;
    sel = new Set(); hoverIds = null; tool = "select"; drag = null; mouse = null; hover = null; band = null;
    try { target.save(); render(); } finally {
      target.restore();
      ({ ctx, data, cam, sel, hoverIds, tool, drag, mouse, hover, band } = keep);
      printing = false; printSize = null;
    }
  }

  return {
    renderTo,
    callbacks: cb,
    get tool() { return tool; },
    setTool,
    setData(d, refit) {
      data = d;
      const ids = new Set([...d.items.map((i) => i.id), ...d.levels.map((l) => l.id), ...(d.grids || []).map((g) => g.id), ...[...(d.detail_lines || []), ...(d.regions || []), ...(d.text_notes || []), ...(d.dimensions || []), ...(d.spots || [])].map((x) => x.id)]);
      modelGroups.forEach((g) => ids.add(g.id));
      for (const it of d.items) if (it.part != null) ids.add(it.part);
      for (const f of d.curtains || []) for (const l of f.lines) ids.add(l.id);
      // a mullion joined to the one below it, or broken, carries on as the run holding its first segment
      let remapped = false;
      sel = new Set([...sel].map((i) => {
        if (ids.has(i) || partWall(i) == null) return i;
        const p = partOf(i), f = (d.curtains || []).find((x) => x.wall === partWall(i));
        const m = f && (f.mullions || []).find((x) => { const q = partOf(x.id); return q.kind === p.kind && q.a === p.a && q.b <= p.b && p.b < x.s1; });
        if (m) remapped = true;
        return m ? m.id : i;
      }).filter((i) => ids.has(i)));
      if (remapped) cb.select([...sel]);
      // a line added or taken away renumbers a curtain wall's grid, so its parts leave the selection
      const sig = new Map((d.curtains || []).map((f) => [f.wall, f.lines.length]));
      for (const [w, n] of cgSig) if (sig.has(w) && sig.get(w) !== n) sel = new Set([...sel].filter((i) => partWall(i) !== w));
      cgSig = sig;
      if (segLine != null && !ids.has(segLine)) setTool(segReturn);
      index();
      buildSnap();
      if (refit) fit();
      if (cgPending) {
        // All Except Picked: the line just placed waits for the segments to leave out
        const pend = cgPending; cgPending = null;
        const f = (d.curtains || []).find((x) => x.wall === pend.wall), l = f && f.lines.find((x) => x.vertical === pend.vertical && Math.abs(x.at - pend.at) < 1);
        if (l) { segReturn = "curtaingrid"; setTool("cgsegments"); segLine = l.id; sel = new Set([l.id]); cb.select([...sel]); cb.prompt(promptText()); }
      }
      render();
    },
    fit() { fit(); render(); },
    // curtain walls: the Curtain Grid tool, its placement, and Add/Remove Segments on a grid line
    startCurtainGrid() { setTool("curtaingrid"); },
    get curtainMode() { return cgMode; },
    setCurtainMode(m) { cgMode = m; cgHover = null; cb.prompt(promptText()); render(); },
    startSegments(id) { segReturn = "select"; setTool("cgsegments"); segLine = id; sel = new Set([id]); cb.select([...sel]); cb.prompt(promptText()); render(); },
    finishSegments() { const id = segLine, back = segReturn; setTool(back); if (back === "select" && id != null) { sel = new Set([id]); cb.select([...sel]); } render(); },
    mullionInfo(id) { const s = selMullion(); return s && s.m.id === id ? { id, wall: s.f.wall, joins: s.m.joins.length } : null; },
    gridLineInfo(id) { const g = gridLine(id); return g ? { id, wall: g.f.wall, vertical: g.l.vertical, typed: g.l.typed, edge: g.l.edge } : null; },
    getCamera() { return { ...cam }; },
    setCameraState(c) { cam = { ...c }; render(); },
    setSelection(ids) { sel = new Set(ids); render(); },
    setGroups(list, edit) { modelGroups = list || []; memberOf = new Map(); for (const g of modelGroups) for (const m of g.members) memberOf.set(m, g.id); groupEdit = edit || null; render(); },
    placeGroupType(t) { groupPlace = t; setTool("groupplace"); },
    selection: () => [...sel],
    render,
    // Detail Line and the regions sketch here when the view is open
    startSketch(o) {
      setTool("select");
      tool = "sketch";
      sel = new Set();
      Sketch.attach(elevHost);
      Sketch.begin(o);
      buildSnap();
      canvas.style.cursor = "crosshair";
      if (typeof cb.tool === "function") cb.tool("sketch");
      render();
    },
    endSketch() { Sketch.end(); setTool("select"); buildSnap(); render(); },
    get sketching() { return tool === "sketch"; },
    viewId: () => data && data.view_id,
    viewScale: () => (data && data.scale) || 100,
    paperPx: () => ((data && data.scale) || 100) * cam.s,
    screenOf: (x, y) => toScreen(x, y),
    canvasRect: () => canvas.getBoundingClientRect(),
    textNotes: () => (data && data.text_notes) || [],
    dimensions: () => (data && data.dimensions) || [],
    startDim(kind) { setTool("dimension"); EDims.start(kind); cb.prompt(promptText()); },
    startWitness(id) { setTool("dimension"); EDims.startWitness(id); cb.prompt(promptText()); },
    dimTool: () => EDims,
    spots: () => (data && data.spots) || [],
    startPlace(kind) { setTool(kind); },
    startSpot(kind) { setTool("spot"); ESpots.start(kind); cb.prompt(promptText()); },
    spotTool: () => ESpots,
    detailLines: () => (data && data.detail_lines) || [],
    regions: () => (data && data.regions) || [],
    setEditingNote(id) { Det.setEditing(id); },
    startNoteLeader(id, side, arc) { setTool("noteleader"); Det.startLeader(id, side, arc); sel = new Set([id]); cb.prompt(promptText()); render(); },
    noteOptionsChanged() { Det.clearPicks(); cb.prompt(promptText()); render(); },
    refreshSnap() { if (mouse && (tool === "sketch" || tool === "text")) hover = tool === "sketch" ? snap(mouse.x, mouse.y) : Det.toolSnap(mouse.x, mouse.y); render(); },
    key(e) {
      // Detail Line has no Finish: Esc in its select tool with nothing selected leaves it
      if (tool === "sketch" && e.key === "Escape" && Sketch.kind === "detail" && Sketch.idle) { cb.leaveDetail(); return true; }
      if (tool === "sketch") return Sketch.key(e);
      if (tool === "dimension" && EDims.key(e)) return true;
      if (tool === "spot" && ESpots.key(e)) return true;
      if (tool === "text" && e.key === " " && Det.flipArc()) return true;
      if (tool === "text" && e.key === "Escape") { if (Det.picking) { Det.clearPicks(); cb.prompt(promptText()); render(); } else setTool("select"); return true; }
      if (tool === "noteleader" && e.key === "Escape") { setTool("select"); return true; }
      if (tool === "groupplace" && e.key === "Escape") { setTool("select"); return true; }
      if (detDrag && e.key === "Escape") { detDrag = null; down = null; render(); return true; }
      if (e.key === "Escape" && Det.cancelDrag()) return true;
      if (tgrip && e.key === "Escape") { tgrip = null; render(); return true; }
      if (e.key === "Tab" && tool === "select" && lastMove && !drag && !pan && !input) {
        const c = candIds(lastMove.x, lastMove.y);
        if (c.length > 1) {
          tabIdx = (tabIdx + (e.shiftKey ? -1 : 1) + c.length * 4) % c.length;
          const h = c[tabIdx];
          hoverIds = kindOf(h) === "item" ? [h] : null; hoverLevel = kindOf(h) === "level" ? h : null; hoverGrid = kindOf(h) === "grid" ? h : null;
          render();
        }
        return true;
      }
      if (tool === "level") {
        if (/^[0-9.]$/.test(e.key) || (e.key === "-" && !typed)) { typed += e.key; cb.prompt(promptText()); render(); return true; }
        if (e.key === "Backspace" && typed) { typed = typed.slice(0, -1); cb.prompt(promptText()); render(); return true; }
        if (e.key === "Enter" && typed) {
          const v = parseFloat(typed); typed = "";
          if (Number.isFinite(v)) cb.addLevel(v);
          cb.prompt(promptText()); render();
          return true;
        }
        if (e.key === "Escape") { if (typed) { typed = ""; cb.prompt(promptText()); render(); } else setTool("select"); return true; }
      }
      if (isEdit()) {
        if (e.key === "Tab" && tool === "align") { candTab++; render(); return true; }
        if (e.key === "Escape") {
          if (ed && (ed.stage === "b" || ed.stage === "tgt" || (ed.stage === "a" && tool !== "align" && ed.base))) { ed = newEd(); cb.prompt(editPrompt()); render(); } else setTool("select");
          return true;
        }
      }
      if ((tool === "grid" || tool === "door" || tool === "window" || tool === "curtaingrid") && e.key === "Escape") { setTool("select"); return true; }
      if (tool === "cgsegments" && (e.key === "Escape" || e.key === "Enter")) {
        const id = segLine, back = segReturn;
        setTool(back);
        if (back === "select" && id != null) { sel = new Set([id]); cb.select([...sel]); render(); }
        return true;
      }
      if (e.key === "Escape" && tool === "select") { if (sel.size) { sel = new Set(); cb.select([]); render(); } return true; }
      return false;
    },
    // test hooks
    toScreen: (u, z) => toScreen(u, z),
    hoverSnap: () => hover,
    levelScreen: (id) => { const l = data.levels.find((x) => x.id === id); return l ? toScreen((l.u0 + l.u1) / 2, l.z) : null; },
    get data() { return data; },
    cropGrips: () => cropBoxes,
    get grips() { return dimGrips.map((g) => ({ x: g.x, y: g.y, role: g.role, slot: g.dim.slot })); },
    get dims() { return tempDims().map((d) => ({ slot: d.slot, value: d.value })); },
  };
})();
