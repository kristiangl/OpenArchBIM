// Sheets: the paper (millimetres, origin at the title block's bottom left, y up), the title block drawn with the
// sheet's values (annotation.js), and each placed view drawn by its own viewer (Plan.renderTo, Elev.renderTo) inside its
// box at its scale, with its label below. `draw` paints a sheet onto any 2D context at `k` pixels per paper mm, which
// is what the screen, the PDF and the DXF writers use. Views are placed by dragging them from the Project Browser.
// Activate View (a double-click on a viewport): the view's own viewer works over the viewport's box (main.js places
// it there), and the rest of the sheet is dimmed and still until it is deactivated.
// Schedules on the sheet: each placed schedule drawn as a table in its segments side by side (schedLayout), the
// rows flowing from one segment to the next; a segment drags to move, right-click splits or joins.
const SheetView = (() => {
  const canvas = document.getElementById("sheet");
  const ctx = canvas.getContext("2d");
  const cb = { editLabel() {}, titleLine() {}, moveViewports() {}, select() {}, openView() {}, moveViewport() {}, placeView() {}, prompt() {}, activate() {}, deactivate() {}, menu() {}, activeBox() {}, moveSegment() {}, scheduleMenu() {}, openSchedule() {} };
  let sheet = null; // the `sheet` call's answer
  let views = new Map(); // view id -> its plan or elevation data
  let cam = { x: 420, y: 297, s: 1 }; // paper mm at the centre, pixels per paper mm
  let sel = new Set();
  let drag = null; // { id, x0, y0, dx, dy, moved }, and seg for a schedule's segment
  let pan = null;
  let band = null; // a box dragged on the paper to pick views (screen px)
  let tdrag = null, titleGrip = null; // a title line's length dragged; the grip of the one picked view
  let active = null; // the activated viewport

  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const size = () => ({ w: canvas.clientWidth, h: canvas.clientHeight });
  const toScreen = (x, y) => { const { w, h } = size(); return { x: (x - cam.x) * cam.s + w / 2, y: h / 2 - (y - cam.y) * cam.s }; };
  const toPaper = (sx, sy) => { const { w, h } = size(); return { x: (sx - w / 2) / cam.s + cam.x, y: (h / 2 - sy) / cam.s + cam.y }; };

  // ---- drawing, onto any context: `at(x, y)` maps paper mm to the context's pixels, `k` pixels per mm
  function draw(c, s, data, at, k, opt = {}) {
    const [pw, ph] = s.paper;
    const o = at(0, ph), e = at(pw, 0);
    c.save();
    c.fillStyle = "#ffffff";
    c.fillRect(o.x, o.y, e.x - o.x, e.y - o.y);
    for (const v of s.views) {
      const d = data.get(v.view);
      const ctr = at(v.centre[0], v.centre[1]), w = v.paper_size[0] * k, h = v.paper_size[1] * k;
      c.save();
      c.beginPath(); c.rect(ctr.x - w / 2, ctr.y - h / 2, w, h); c.clip();
      // the view, turned with its viewport: drawn unturned about the box's middle
      const uw = v.rotation ? h : w, uh = v.rotation ? w : h;
      c.translate(ctr.x, ctr.y);
      if (v.rotation) c.rotate((-v.rotation * Math.PI) / 180);
      c.translate(-uw / 2, -uh / 2);
      const s2 = k / v.scale;
      if (c.oaViewport) c.oaViewport({ view: v.view, kind: v.drawing, scale: v.scale, rotation: v.rotation || 0, centre: v.centre, paper_size: v.paper_size, title: v.title, local: { s: s2, cx: v.view_centre[0], cy: v.view_centre[1], w: uw, h: uh } });
      if (d && v.drawing === "plan") Plan.renderTo(c, { data: d, cam: { x: v.view_centre[0], y: v.view_centre[1], s: s2 }, w: uw, h: uh });
      else if (d) Elev.renderTo(c, { data: d, cam: { u: v.view_centre[0], z: v.view_centre[1], s: s2 }, w: uw, h: uh });
      if (c.oaViewport) c.oaViewport(null);
      c.restore();
      label(c, v, at, k);
      if (opt.selected && opt.selected.has(v.id)) {
        c.save(); c.strokeStyle = opt.accent || "#2f6fd6"; c.lineWidth = 1.5; c.setLineDash([6, 4]);
        c.strokeRect(ctr.x - w / 2, ctr.y - h / 2, w, h); c.restore();
      }
    }
    for (const p of s.schedules || []) drawSchedule(c, p, at, k, opt.selected && opt.selected.has(p.id) ? opt.accent || "#2f6fd6" : null);
    if (s.title_block) {
      const o0 = at(0, 0), o1 = at(1, 0);
      if (c.oaTag) c.oaTag({ cat: "Title Blocks", sub: null, kind: "projection" });
      if (c.oaBlock) c.oaBlock({ name: s.title_block.name || "Title Block", at: o0, dir: { x: o1.x - o0.x, y: o1.y - o0.y } });
      Ann.draw(c, s.title_block, { x: o0.x, y: o0.y, k }, null, "#ffffff", { own: true });
      if (c.oaBlock) c.oaBlock(null);
    }
    c.restore();
  }

  // ---- schedules on the sheet ---------------------------------------------------------------------------------------
  // Sizes in paper mm: the title, the headings, each row; text 2.5 mm (the title 3.5 mm), 1.5 mm in from the lines.
  const S = { title: 8, head: 6.5, row: 5.5, text: 2.5, titleText: 3.5, pad: 1.5 };
  const measurer = document.createElement("canvas").getContext("2d");
  const textMm = (t, size, bold) => { measurer.font = `${bold ? "600 " : ""}${size * 10}px system-ui, sans-serif`; return measurer.measureText(String(t)).width / 10; };
  // A placed schedule laid out: its columns' widths (the same in every segment) and each segment's place, size and
  // rows. Segment i takes its number of rows, the last the rest.
  function schedLayout(p) {
    const t = p.table || { columns: [], rows: [], title: "", show_title: false, show_headers: false };
    const cols = t.columns.map((c, i) => {
      let w = textMm(c.heading, S.text, true);
      for (const r of t.rows) if (r.kind !== "header" && r.cells[i]) w = Math.max(w, textMm(r.cells[i].text, S.text, r.kind !== "item"));
      return Math.max(10, w + 2 * S.pad);
    });
    const total = cols.reduce((a, b) => a + b, 0);
    const need = t.show_title ? textMm(t.title, S.titleText, true) + 2 * S.pad : 0;
    if (cols.length && need > total) cols[cols.length - 1] += need - total;
    const width = Math.max(cols.reduce((a, b) => a + b, 0), need, 20);
    let cursor = 0;
    const segs = p.segments.map((g, i) => {
      const last = i === p.segments.length - 1;
      const n = last ? Infinity : g.rows || 0;
      const rows = t.rows.slice(cursor, cursor + n).map((r, j) => ({ ...r, index: cursor + j }));
      const start = cursor;
      cursor += rows.length;
      const title = i === 0 && t.show_title, head = t.show_headers && (i === 0 || p.repeat_headers);
      const h = (title ? S.title : 0) + (head ? S.head : 0) + rows.length * S.row;
      return { i, x: g.at[0], top: g.at[1], w: width, h: Math.max(h, S.row), title, head, rows, start };
    });
    return { cols, segs, table: t };
  }
  function drawSchedule(c, p, at, k, accent) {
    const L = schedLayout(p), t = L.table;
    c.save();
    if (c.oaTag) c.oaTag({ cat: "Schedules", sub: null, kind: "schd" });
    c.strokeStyle = "#000"; c.fillStyle = "#000"; c.lineCap = "butt";
    const line = (x0, y0, x1, y1, w) => { const a = at(x0, y0), b = at(x1, y1); c.lineWidth = Math.max(0.5, Pen.w(w, k)); c.beginPath(); c.moveTo(a.x, a.y); c.lineTo(b.x, b.y); c.stroke(); };
    const text = (s, x, yTop, hRow, size, bold, align, italic) => {
      if (s == null || s === "") return;
      c.font = `${italic ? "italic " : ""}${bold ? "600 " : ""}${size * k}px system-ui, sans-serif`;
      c.textAlign = align === "right" ? "right" : align === "centre" ? "center" : "left"; c.textBaseline = "middle";
      const q = at(x, yTop - hRow / 2);
      c.fillText(String(s), q.x, q.y);
    };
    const cellX = (i, align) => { let x = 0; for (let j = 0; j < i; j++) x += L.cols[j]; return align === "right" ? x + L.cols[i] - S.pad : align === "centre" ? x + L.cols[i] / 2 : x + S.pad; };
    for (const g of L.segs) {
      let y = g.top;
      const x0 = g.x, x1 = g.x + g.w;
      const verticals = (yTop, yBot) => { let x = x0; for (let j = 0; j + 1 < L.cols.length; j++) { x += L.cols[j]; line(x, yTop, x, yBot, 0.13); } };
      if (g.title) { text(t.title, x0 + g.w / 2, y, S.title, S.titleText, true, "centre"); y -= S.title; line(x0, y, x1, y, 0.13); }
      if (g.head) {
        t.columns.forEach((col, i) => text(col.heading, x0 + cellX(i, "centre"), y, S.head, S.text, true, "centre"));
        verticals(y, y - S.head);
        y -= S.head; line(x0, y, x1, y, 0.25);
      }
      for (const r of g.rows) {
        const top = y;
        // conditional formats the schedule shows on sheets
        if (r.kind === "item") r.cells.forEach((cell, i) => {
          if (!cell.bg || !t.columns[i] || !t.columns[i].highlight_on_sheets) return;
          let x = x0; for (let j = 0; j < i; j++) x += L.cols[j];
          const a = at(x, top), b = at(x + L.cols[i], top - S.row);
          c.save(); c.fillStyle = cell.bg; c.fillRect(a.x, a.y, b.x - a.x, b.y - a.y); c.restore();
        });
        if (r.kind === "header") text(r.cells[0].text, x0 + S.pad + 3 * (r.level || 0), y, S.row, S.text, true, "left");
        else if (r.kind !== "blank") {
          r.cells.forEach((cell, i) => text(cell.text, x0 + cellX(i, t.columns[i] ? t.columns[i].align : "left"), y, S.row, S.text, r.kind !== "item", t.columns[i] ? t.columns[i].align : "left", r.kind === "footer"));
          verticals(top, top - S.row);
        }
        y -= S.row;
        line(x0, y, x1, y, r.kind === "grand" || (r.index + 1 < t.rows.length && t.rows[r.index + 1].kind === "grand") ? 0.25 : 0.13);
      }
      // the outline
      const bottom = g.top - g.h;
      line(x0, g.top, x1, g.top, 0.35); line(x0, bottom, x1, bottom, 0.35); line(x0, g.top, x0, bottom, 0.35); line(x1, g.top, x1, bottom, 0.35);
      if (accent) {
        const a = at(x0, g.top), b = at(x1, bottom);
        c.save(); c.strokeStyle = accent; c.lineWidth = 1.5; c.setLineDash([6, 4]); c.strokeRect(a.x - 2, a.y - 2, b.x - a.x + 4, b.y - a.y + 4); c.restore();
      }
    }
    c.restore();
  }
  // the placed schedule, segment and row under a point on the screen
  function scheduleAt(sx, sy) {
    if (!sheet) return null;
    const q = toPaper(sx, sy);
    for (const p of [...(sheet.schedules || [])].reverse()) {
      const L = schedLayout(p);
      for (const g of L.segs) {
        if (q.x < g.x || q.x > g.x + g.w || q.y > g.top || q.y < g.top - g.h) continue;
        const into = g.top - q.y - (g.title ? S.title : 0) - (g.head ? S.head : 0);
        const row = into >= 0 ? g.rows[Math.floor(into / S.row)] : null;
        return { p, seg: g.i, row: row ? row.index : null, layout: L };
      }
    }
    return null;
  }

  // where a view's title line runs on the paper: its start, its end and its height
  const titleLen = (v, tw) => (v.title_line_mm > 0 ? v.title_line_mm : Math.max(tw, 40));
  function titleLine(v) {
    const left = v.centre[0] - v.paper_size[0] / 2, bottom = v.centre[1] - v.paper_size[1] / 2;
    const x0 = left + 4 + 1 + 4 + 2;
    return { x0, x1: x0 + titleLen(v, textMm(v.title.toUpperCase(), 3.5, true)), y: bottom - 8 };
  }
  // a view's label under its box: the detail number in a circle, the title on a line, the scale under the line
  function label(c, v, at, k) {
    const left = v.centre[0] - v.paper_size[0] / 2, bottom = v.centre[1] - v.paper_size[1] / 2;
    const r = 4, cx = left + r + 1, cy = bottom - 8;
    const p = at(cx, cy);
    c.save();
    if (c.oaTag) c.oaTag({ cat: "View Titles", sub: null, kind: "text" });
    c.strokeStyle = "#000"; c.fillStyle = "#000"; c.lineWidth = Math.max(0.5, Pen.w(0.25, k));
    c.beginPath(); c.arc(p.x, p.y, r * k, 0, Math.PI * 2); c.stroke();
    c.textAlign = "center"; c.textBaseline = "middle"; c.font = `600 ${3.5 * k}px system-ui, sans-serif`;
    c.fillText(v.detail_number, p.x, p.y);
    c.textAlign = "left"; c.textBaseline = "alphabetic";
    const t0 = at(cx + r + 2, cy + 0.8);
    c.fillText(v.title.toUpperCase(), t0.x, t0.y);
    const tw = c.measureText(v.title.toUpperCase()).width / k;
    const l0 = at(cx + r + 2, cy), l1 = at(cx + r + 2 + titleLen(v, tw), cy);
    c.lineWidth = Math.max(0.5, Pen.w(0.35, k));
    c.beginPath(); c.moveTo(l0.x, l0.y); c.lineTo(l1.x, l1.y); c.stroke();
    c.font = `${2.5 * k}px system-ui, sans-serif`; c.textBaseline = "top";
    const s0 = at(cx + r + 2, cy - 1);
    c.fillText(`1 : ${v.scale}`, s0.x, s0.y);
    c.restore();
  }

  function render() {
    const dpr = window.devicePixelRatio || 1, { w, h } = size();
    if (canvas.width !== Math.floor(w * dpr) || canvas.height !== Math.floor(h * dpr)) { canvas.width = Math.floor(w * dpr); canvas.height = Math.floor(h * dpr); }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = css("--panel-2") || "#e8e9ec";
    ctx.fillRect(0, 0, w, h);
    if (!sheet) return;
    const moved = (p) => ({ ...p, segments: p.segments.map((g, i) => (i === drag.seg ? { ...g, at: [g.at[0] + drag.dx, g.at[1] + drag.dy] } : g)) });
    const shown = drag && drag.moved
      ? { ...sheet, views: sheet.views.map((v) => (drag.seg == null && (drag.ids || [drag.id]).includes(v.id) ? { ...v, centre: [v.centre[0] + drag.dx, v.centre[1] + drag.dy] } : v)), schedules: (sheet.schedules || []).map((p) => (p.id === drag.id && drag.seg != null ? moved(p) : p)) }
      : tdrag ? { ...sheet, views: sheet.views.map((v) => (v.id === tdrag.id ? { ...v, title_line_mm: tdrag.len } : v)) } : sheet;
    // a shadow under the paper
    const o = toScreen(0, sheet.paper[1]);
    ctx.fillStyle = "rgba(0,0,0,0.12)";
    ctx.fillRect(o.x + 3, o.y + 3, sheet.paper[0] * cam.s, sheet.paper[1] * cam.s);
    draw(ctx, shown, views, toScreen, cam.s, { selected: active ? new Set() : sel, accent: css("--accent") });
    const accent = css("--accent");
    // the guides a dragged view lines up on, across the paper
    if (drag && drag.moved && drag.guides) {
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
      for (const g of drag.guides) {
        const a = g.x != null ? toScreen(g.x, 0) : toScreen(0, g.y), b = g.x != null ? toScreen(g.x, sheet.paper[1]) : toScreen(sheet.paper[0], g.y);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
      }
      ctx.restore();
    }
    // the box dragged to pick views
    if (band) {
      ctx.save(); ctx.strokeStyle = accent; ctx.lineWidth = 1; ctx.setLineDash(band.x1 < band.x0 ? [5, 3] : []);
      ctx.fillStyle = "rgba(47,111,214,0.06)";
      ctx.fillRect(band.x0, band.y0, band.x1 - band.x0, band.y1 - band.y0); ctx.strokeRect(band.x0, band.y0, band.x1 - band.x0, band.y1 - band.y0);
      ctx.restore();
    }
    // one view picked: a grip at the end of its title line sets the line's length
    titleGrip = null;
    if (!active && sel.size === 1) {
      const v = shown.views.find((x) => sel.has(x.id));
      if (v) {
        const t = titleLine(v), p = toScreen(t.x1, t.y);
        ctx.save(); ctx.fillStyle = css("--view-bg") || "#fff"; ctx.strokeStyle = accent; ctx.lineWidth = 1.2;
        ctx.beginPath(); ctx.arc(p.x, p.y, 4.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); ctx.restore();
        titleGrip = { id: v.id, x: p.x, y: p.y, x0: t.x0 };
      }
    }
    // an activated view: everything else on the sheet fades behind it
    const b = active && screenBox(active);
    if (b) {
      ctx.save();
      ctx.beginPath(); ctx.rect(0, 0, size().w, size().h); ctx.rect(b.x, b.y, b.w, b.h);
      ctx.fillStyle = "rgba(236,238,241,0.62)"; ctx.fill("evenodd");
      ctx.strokeStyle = css("--accent"); ctx.lineWidth = 1.5; ctx.strokeRect(b.x - 1, b.y - 1, b.w + 2, b.h + 2);
      ctx.restore();
      cb.activeBox(b);
    }
  }

  // a viewport's box on the screen (CSS pixels in the canvas)
  function screenBox(id) {
    const v = sheet && sheet.views.find((x) => x.id === id);
    if (!v) return null;
    const a = toScreen(v.centre[0] - v.paper_size[0] / 2, v.centre[1] + v.paper_size[1] / 2);
    return { x: a.x, y: a.y, w: v.paper_size[0] * cam.s, h: v.paper_size[1] * cam.s };
  }

  function fit() {
    if (!sheet) return;
    const { w, h } = size();
    cam = { x: sheet.paper[0] / 2, y: sheet.paper[1] / 2, s: Math.min((w - 40) / sheet.paper[0], (h - 40) / sheet.paper[1]) };
  }

  // the sheet and the data of every view on it
  async function load(id, refit) {
    const s = await Backend.call("sheet", { id });
    const data = new Map();
    for (const v of s.views) {
      try { data.set(v.view, await Backend.call(v.drawing === "plan" ? "plan" : "elevation", { view_id: v.view })); } catch (e) { /* a view that cannot be drawn shows as an empty box */ }
    }
    const first = !sheet || sheet.id !== s.id;
    sheet = s; views = data;
    if (first || refit) fit();
    render();
  }

  // What a view lines up by on the paper: its box's sides and middle, its title line, and where the model's origin
  // falls (so two plans, or two elevations at the same scale, show the same place or height side by side).
  function features(v, dx = 0, dy = 0) {
    const c = [v.centre[0] + dx, v.centre[1] + dy], [w, h] = v.paper_size, t = titleLine(v);
    const xs = [c[0] - w / 2, c[0], c[0] + w / 2], ys = [c[1] - h / 2, c[1], c[1] + h / 2, t.y + dy];
    if (!v.rotation && v.scale) {
      if (v.drawing === "plan") xs.push(c[0] - v.view_centre[0] / v.scale);
      ys.push(c[1] - v.view_centre[1] / v.scale);
    }
    return { xs, ys };
  }
  // the move that puts a dragged view on the nearest line of another, and the lines it is on
  function align(ids, dx, dy) {
    const tol = 5 / cam.s, moving = sheet.views.filter((v) => ids.includes(v.id)), still = sheet.views.filter((v) => !ids.includes(v.id));
    if (!moving.length || !still.length) return { dx, dy, guides: [] };
    let bx = null, by = null;
    for (const m of moving) {
      const f = features(m, dx, dy);
      for (const o of still) {
        const g = features(o);
        for (const a of f.xs) for (const b of g.xs) if (Math.abs(b - a) <= tol && (!bx || Math.abs(b - a) < Math.abs(bx.d))) bx = { d: b - a, x: b };
        for (const a of f.ys) for (const b of g.ys) if (Math.abs(b - a) <= tol && (!by || Math.abs(b - a) < Math.abs(by.d))) by = { d: b - a, y: b };
      }
    }
    const guides = [];
    if (bx) { dx += bx.d; guides.push({ x: bx.x }); }
    if (by) { dy += by.d; guides.push({ y: by.y }); }
    return { dx, dy, guides };
  }

  const viewportAt = (sx, sy) => {
    if (!sheet) return null;
    const p = toPaper(sx, sy);
    const hit = sheet.views.filter((v) => Math.abs(p.x - v.centre[0]) <= v.paper_size[0] / 2 && Math.abs(p.y - v.centre[1]) <= v.paper_size[1] / 2);
    return hit.length ? hit[hit.length - 1] : null;
  };

  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    if (active) return; // the sheet stays still while a view is activated
    const before = toPaper(e.offsetX, e.offsetY);
    cam.s *= Math.exp(-e.deltaY * 0.0015);
    const after = toPaper(e.offsetX, e.offsetY);
    cam.x += before.x - after.x; cam.y += before.y - after.y;
    render();
  }, { passive: false });
  canvas.addEventListener("mousedown", (e) => {
    if (active) return;
    if (e.button === 1 || e.button === 2) { pan = { x: e.offsetX, y: e.offsetY, cx: cam.x, cy: cam.y }; e.preventDefault(); return; }
    if (e.button !== 0) return;
    if (titleGrip && Math.hypot(e.offsetX - titleGrip.x, e.offsetY - titleGrip.y) <= 7) { tdrag = { id: titleGrip.id, x0: titleGrip.x0, len: null }; return; }
    const hs = scheduleAt(e.offsetX, e.offsetY);
    if (hs) {
      sel = new Set([hs.p.id]); cb.select([hs.p.id]);
      drag = { id: hs.p.id, seg: hs.seg, x0: e.offsetX, y0: e.offsetY, dx: 0, dy: 0, moved: false };
      render();
      return;
    }
    const v = viewportAt(e.offsetX, e.offsetY);
    const add = e.ctrlKey || e.shiftKey;
    // nothing there: a box picks the views (left to right, those wholly inside; right to left, those it touches)
    if (!v) { if (!add) { sel = new Set(); cb.select([]); } band = { x0: e.offsetX, y0: e.offsetY, x1: e.offsetX, y1: e.offsetY, add }; render(); return; }
    if (add) { if (sel.has(v.id)) sel.delete(v.id); else sel.add(v.id); }
    else if (!sel.has(v.id)) sel = new Set([v.id]);
    cb.select([...sel]);
    // a picked view drags every picked view with it
    const ids = sheet.views.filter((x) => sel.has(x.id)).map((x) => x.id);
    if (sel.has(v.id)) drag = { id: v.id, ids, x0: e.offsetX, y0: e.offsetY, dx: 0, dy: 0, moved: false, add };
    render();
  });
  canvas.addEventListener("mousemove", (e) => {
    if (pan) { cam.x = pan.cx - (e.offsetX - pan.x) / cam.s; cam.y = pan.cy + (e.offsetY - pan.y) / cam.s; render(); return; }
    if (tdrag) { tdrag.len = Math.max(5, Math.round(toPaper(e.offsetX, e.offsetY).x - tdrag.x0)); render(); return; }
    if (band) { band.x1 = e.offsetX; band.y1 = e.offsetY; render(); return; }
    if (drag) {
      if (!drag.moved && Math.hypot(e.offsetX - drag.x0, e.offsetY - drag.y0) < 4) return;
      drag.moved = true;
      const raw = { dx: (e.offsetX - drag.x0) / cam.s, dy: -(e.offsetY - drag.y0) / cam.s };
      // views line up with each other (not schedule segments); Shift drags freely
      const a = drag.seg == null && !e.shiftKey ? align(drag.ids || [drag.id], raw.dx, raw.dy) : { ...raw, guides: [] };
      drag.dx = a.dx; drag.dy = a.dy; drag.guides = a.guides;
      render();
    }
  });
  window.addEventListener("mouseup", () => {
    pan = null;
    if (tdrag) { const t = tdrag; tdrag = null; if (t.len != null) cb.titleLine(t.id, t.len); render(); return; }
    if (band) {
      const b = band; band = null;
      const a = toPaper(Math.min(b.x0, b.x1), Math.max(b.y0, b.y1)), z = toPaper(Math.max(b.x0, b.x1), Math.min(b.y0, b.y1)), crossing = b.x1 < b.x0;
      const picked = sheet ? sheet.views.filter((v) => {
        const l = v.centre[0] - v.paper_size[0] / 2, r = v.centre[0] + v.paper_size[0] / 2, bo = v.centre[1] - v.paper_size[1] / 2, t = v.centre[1] + v.paper_size[1] / 2;
        return crossing ? r >= a.x && l <= z.x && t >= a.y && bo <= z.y : l >= a.x && r <= z.x && bo >= a.y && t <= z.y;
      }).map((v) => v.id) : [];
      if (Math.hypot(b.x1 - b.x0, b.y1 - b.y0) >= 4) { sel = new Set(b.add ? [...sel, ...picked] : picked); cb.select([...sel]); }
      render();
      return;
    }
    if (!drag) return;
    const d = drag; drag = null;
    if (d.seg != null) {
      const p = sheet && (sheet.schedules || []).find((x) => x.id === d.id);
      if (d.moved && p) { const g = p.segments[d.seg]; cb.moveSegment(p, d.seg, [Math.round((g.at[0] + d.dx) * 10) / 10, Math.round((g.at[1] + d.dy) * 10) / 10]); }
      render();
      return;
    }
    // a click (no drag) on one of several picked views picks that one alone
    if (!d.moved && !d.add && d.seg == null && sel.size > 1) { sel = new Set([d.id]); cb.select([d.id]); }
    if (d.moved) {
      const moves = sheet.views.filter((x) => (d.ids || [d.id]).includes(x.id)).map((v) => [v.id, [Math.round((v.centre[0] + d.dx) * 10) / 10, Math.round((v.centre[1] + d.dy) * 10) / 10]]);
      if (moves.length === 1) cb.moveViewport(...moves[0]); else if (moves.length) cb.moveViewports(moves);
    }
    render();
  });
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    if (active || (pan && (pan.x !== e.offsetX || pan.y !== e.offsetY))) return;
    const hs = scheduleAt(e.offsetX, e.offsetY);
    if (hs) { sel = new Set([hs.p.id]); cb.select([hs.p.id]); render(); cb.scheduleMenu(hs, e.clientX, e.clientY); return; }
    const v = viewportAt(e.offsetX, e.offsetY);
    if (v) { sel = new Set([v.id]); cb.select([v.id]); render(); cb.menu(v, e.clientX, e.clientY); }
  });
  // the title block's label under a point on the screen, with the parameters it shows
  function labelAt(sx, sy) {
    if (!sheet || !sheet.title_block) return null;
    const q = toPaper(sx, sy);
    for (const t of sheet.title_block.labels || []) {
      if (!t.params || !t.params.length) continue;
      const L = Ann.layout(t), pad = 1;
      if (q.x >= L.x0 - pad && q.x <= L.x0 + Math.max(L.w, 4) + pad && q.y <= L.top + pad && q.y >= L.top - L.h - pad) return t;
    }
    return null;
  }
  // a double-click on a title block's label edits it; on a viewport, activates its view; on the dimmed sheet,
  // deactivates it
  canvas.addEventListener("dblclick", (e) => {
    if (active) { cb.deactivate(); return; }
    const lb = labelAt(e.offsetX, e.offsetY);
    if (lb) { cb.editLabel(lb.params); return; }
    const hs = scheduleAt(e.offsetX, e.offsetY);
    if (hs) { cb.openSchedule(hs.p.schedule); return; }
    const v = viewportAt(e.offsetX, e.offsetY);
    if (v) cb.activate(v.id);
  });
  // a view dragged from the Project Browser goes where it is dropped
  canvas.addEventListener("dragover", (e) => { if (sheet && e.dataTransfer.types.includes("text/x-openarch-view")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; } });
  canvas.addEventListener("drop", (e) => {
    const id = Number(e.dataTransfer.getData("text/x-openarch-view"));
    if (!sheet || !id) return;
    e.preventDefault();
    const p = toPaper(e.offsetX, e.offsetY);
    cb.placeView(sheet.id, id, [Math.round(p.x), Math.round(p.y)]);
  });
  new ResizeObserver(() => render()).observe(canvas);

  return {
    callbacks: cb,
    load,
    render,
    fit() { fit(); render(); },
    draw,
    get sheet() { return sheet; },
    get viewData() { return views; },
    setSelection(ids) { sel = new Set(ids); render(); },
    selection: () => [...sel],
    getCamera: () => ({ ...cam }),
    setCamera(c) { cam = { ...c }; render(); },
    setActive(id) { active = id; if (id) { sel = new Set(); drag = null; } render(); },
    get active() { return active; },
    placed: (id) => (sheet && sheet.views.find((x) => x.id === id)) || null,
    screenBox,
    schedLayout,
    scheduleAt,
  };
})();
