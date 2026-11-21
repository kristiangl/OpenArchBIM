// Images (Insert > Image): PNG and JPEG pictures placed in a view or on a sheet, sized on paper. Plans, elevations,
// sections, drafting views, legends and sheets share this drawing and picking; the pictures are fetched once each
// (`image_data`) and kept by key, and the view is drawn again when one arrives (the core: image.rs).
const Images = (() => {
  const cache = new Map(); // key -> HTMLImageElement (loaded) or "loading"
  const scaleOf = (data) => data?.view?.scale || data?.scale || 1;

  function picture(img) {
    const got = cache.get(img.key);
    if (got && got !== "loading") return got;
    if (!got) {
      cache.set(img.key, "loading");
      Backend.call("image_data", { id: img.id }).then((r) => {
        const el = new Image();
        el.onload = () => { cache.set(img.key, el); redraw(); };
        el.onerror = () => cache.delete(img.key);
        el.src = `data:${r.mime};base64,${r.data}`;
      }).catch(() => cache.delete(img.key));
    }
    return null;
  }
  function redraw() {
    if (app.view.kind === "plan") Plan.render?.();
    else if (app.view.kind === "elevation") Elev.render?.();
    else if (app.view.kind === "sheet") SheetView.render?.();
  }

  // drawn centred on its point, its paper size times the view's scale; a frame while the picture loads
  function draw(ctx, data, toScreen, k, sel = new Set()) {
    const s = scaleOf(data);
    for (const img of data?.images || []) {
      const c = toScreen(img.at), w = img.width_mm * s * k, h = img.height_mm * s * k;
      const pic = picture(img);
      ctx.save();
      if (pic) ctx.drawImage(pic, c.x - w / 2, c.y - h / 2, w, h);
      else { ctx.strokeStyle = "#999"; ctx.setLineDash([4, 3]); ctx.strokeRect(c.x - w / 2, c.y - h / 2, w, h); }
      if (sel.has(img.id)) { ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 2; ctx.setLineDash([]); ctx.strokeRect(c.x - w / 2, c.y - h / 2, w, h); }
      ctx.restore();
    }
  }
  function elements(data) {
    const s = scaleOf(data);
    return (data?.images || []).map((img) => {
      const hw = (img.width_mm * s) / 2, hh = (img.height_mm * s) / 2, p = img.at;
      return { id: img.id, kind: "image", category: "Raster Images", ring: [[p.x - hw, p.y - hh], [p.x + hw, p.y - hh], [p.x + hw, p.y + hh], [p.x - hw, p.y + hh]], pinned: img.pinned, display: "normal" };
    });
  }

  // the tool: choose the file, then click where its centre goes
  let path = null;
  async function start() {
    if (!(await ensureDetailView("An image"))) return;
    const p = await Backend.fileDialog("open", { multiple: false, filters: [{ name: "Picture", extensions: ["png", "jpg", "jpeg"] }] });
    if (!p) return;
    path = Array.isArray(p) ? p[0] : p;
    noteView().setTool("image");
    setPrompt("Click where the picture's centre goes. Esc cancels.");
  }
  async function place(pt) {
    if (!path) return;
    const r = await run("add_image", { view_id: app.view.viewId, path, point: [Math.round(pt.x * 100) / 100, Math.round(pt.y * 100) / 100] });
    if (r) { path = null; noteView().setTool("select"); await refresh(); app.selection = [r.id]; noteView().setSelection?.([r.id]); renderProperties(); setPrompt("Image placed. Set its Width in Properties; drag it with Move."); }
  }
  // every picture of a list fetched and decoded (an export waits for this before it draws)
  async function preload(list) {
    await Promise.all((list || []).map(async (img) => {
      const got = cache.get(img.key);
      if (got && got !== "loading") return;
      try {
        const r = await Backend.call("image_data", { id: img.id });
        const el = new Image();
        await new Promise((ok) => { el.onload = ok; el.onerror = ok; el.src = `data:${r.mime};base64,${r.data}`; });
        if (el.naturalWidth) cache.set(img.key, el);
      } catch { /* drawn as a frame */ }
    }));
  }
  return { draw, elements, start, place, preload };
})();
