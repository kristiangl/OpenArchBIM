// Sheets: the paper (millimetres, origin at the title block's bottom left, y up), the title block drawn with the
// sheet's values (annotation.js), and each placed view drawn by its own viewer (Plan.renderTo, Elev.renderTo) inside its
// box at its scale, with its label below. `draw` paints a sheet onto any 2D context at `k` pixels per paper mm, which
// is what the screen, the PDF and the DXF writers use. Views are placed by dragging them from the Project Browser.
const SheetView = (() => {
  const canvas = document.getElementById("sheet");
  const ctx = canvas.getContext("2d");
  const cb = { select() {}, openView() {}, moveViewport() {}, placeView() {}, prompt() {} };
  let sheet = null; // the `sheet` call's answer
  let views = new Map(); // view id -> its plan or elevation data
  let cam = { x: 420, y: 297, s: 1 }; // paper mm at the centre, pixels per paper mm
  let sel = new Set();
  let drag = null; // { id, x0, y0, dx, dy, moved }
  let pan = null;

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
      if (d && v.drawing === "plan") Plan.renderTo(c, { data: d, cam: { x: v.view_centre[0], y: v.view_centre[1], s: s2 }, w: uw, h: uh });
      else if (d) Elev.renderTo(c, { data: d, cam: { u: v.view_centre[0], z: v.view_centre[1], s: s2 }, w: uw, h: uh });
      c.restore();
      label(c, v, at, k);
      if (opt.selected && opt.selected.has(v.id)) {
        c.save(); c.strokeStyle = opt.accent || "#2f6fd6"; c.lineWidth = 1.5; c.setLineDash([6, 4]);
        c.strokeRect(ctr.x - w / 2, ctr.y - h / 2, w, h); c.restore();
      }
    }
    if (s.title_block) Ann.draw(c, s.title_block, { x: at(0, 0).x, y: at(0, 0).y, k }, null, "#ffffff", { own: true });
    c.restore();
  }

  // a view's label under its box: the detail number in a circle, the title on a line, the scale under the line
  function label(c, v, at, k) {
    const left = v.centre[0] - v.paper_size[0] / 2, bottom = v.centre[1] - v.paper_size[1] / 2;
    const r = 4, cx = left + r + 1, cy = bottom - 8;
    const p = at(cx, cy);
    c.save();
    c.strokeStyle = "#000"; c.fillStyle = "#000"; c.lineWidth = Math.max(0.5, 0.25 * k);
    c.beginPath(); c.arc(p.x, p.y, r * k, 0, Math.PI * 2); c.stroke();
    c.textAlign = "center"; c.textBaseline = "middle"; c.font = `600 ${3.5 * k}px system-ui, sans-serif`;
    c.fillText(v.detail_number, p.x, p.y);
    c.textAlign = "left"; c.textBaseline = "alphabetic";
    const t0 = at(cx + r + 2, cy + 0.8);
    c.fillText(v.title.toUpperCase(), t0.x, t0.y);
    const tw = c.measureText(v.title.toUpperCase()).width / k;
    const l0 = at(cx + r + 2, cy), l1 = at(cx + r + 2 + Math.max(tw, 40), cy);
    c.lineWidth = Math.max(0.5, 0.35 * k);
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
    const shown = drag && drag.moved ? { ...sheet, views: sheet.views.map((v) => (v.id === drag.id ? { ...v, centre: [v.centre[0] + drag.dx, v.centre[1] + drag.dy] } : v)) } : sheet;
    // a shadow under the paper
    const o = toScreen(0, sheet.paper[1]);
    ctx.fillStyle = "rgba(0,0,0,0.12)";
    ctx.fillRect(o.x + 3, o.y + 3, sheet.paper[0] * cam.s, sheet.paper[1] * cam.s);
    draw(ctx, shown, views, toScreen, cam.s, { selected: sel, accent: css("--accent") });
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

  const viewportAt = (sx, sy) => {
    if (!sheet) return null;
    const p = toPaper(sx, sy);
    const hit = sheet.views.filter((v) => Math.abs(p.x - v.centre[0]) <= v.paper_size[0] / 2 && Math.abs(p.y - v.centre[1]) <= v.paper_size[1] / 2);
    return hit.length ? hit[hit.length - 1] : null;
  };

  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const before = toPaper(e.offsetX, e.offsetY);
    cam.s *= Math.exp(-e.deltaY * 0.0015);
    const after = toPaper(e.offsetX, e.offsetY);
    cam.x += before.x - after.x; cam.y += before.y - after.y;
    render();
  }, { passive: false });
  canvas.addEventListener("mousedown", (e) => {
    if (e.button === 1 || e.button === 2) { pan = { x: e.offsetX, y: e.offsetY, cx: cam.x, cy: cam.y }; e.preventDefault(); return; }
    if (e.button !== 0) return;
    const v = viewportAt(e.offsetX, e.offsetY);
    sel = new Set(v ? [v.id] : []);
    cb.select([...sel]);
    if (v) drag = { id: v.id, x0: e.offsetX, y0: e.offsetY, dx: 0, dy: 0, moved: false };
    render();
  });
  canvas.addEventListener("mousemove", (e) => {
    if (pan) { cam.x = pan.cx - (e.offsetX - pan.x) / cam.s; cam.y = pan.cy + (e.offsetY - pan.y) / cam.s; render(); return; }
    if (drag) {
      if (!drag.moved && Math.hypot(e.offsetX - drag.x0, e.offsetY - drag.y0) < 4) return;
      drag.moved = true; drag.dx = (e.offsetX - drag.x0) / cam.s; drag.dy = -(e.offsetY - drag.y0) / cam.s;
      render();
    }
  });
  window.addEventListener("mouseup", () => {
    pan = null;
    if (!drag) return;
    const d = drag; drag = null;
    const v = sheet && sheet.views.find((x) => x.id === d.id);
    if (d.moved && v) cb.moveViewport(d.id, [Math.round((v.centre[0] + d.dx) * 10) / 10, Math.round((v.centre[1] + d.dy) * 10) / 10]);
    render();
  });
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("dblclick", (e) => { const v = viewportAt(e.offsetX, e.offsetY); if (v) cb.openView(v.view); });
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
  };
})();
