// Symbols (Annotate > Symbol, North Arrow): generic annotation families placed in a view or on a sheet. Drawn at
// their paper size whatever the view's scale, turned by their rotation; plans, elevations, sections, drafting views,
// legends and sheets share this drawing and picking (the core: annsymbol.rs).
const Symbols = (() => {
  // view mm per paper mm: the view's scale, 1 on a sheet
  const scaleOf = (data) => data?.view?.scale || data?.scale || 1;
  const turn = (s) => (s.rotation_deg * Math.PI) / 180;

  function draw(ctx, data, toScreen, k, sel = new Set()) {
    const list = data?.symbols || [];
    if (!list.length) return;
    const s = scaleOf(data);
    for (const sym of list) {
      const p = toScreen(sym.at);
      const col = sel.has(sym.id) ? "#2e6f9e" : null;
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.rotate(-turn(sym));
      const base = { x: 0, y: 0, k: k * s };
      if (sym.family) Ann.paths(ctx, sym.family.body, base, col, "#fff");
      Ann.labels(ctx, sym.labels || [], base, col);
      ctx.restore();
    }
  }

  // each symbol picked by its drawn box, turned with it (view mm)
  function elements(data) {
    const out = [], s = scaleOf(data);
    for (const sym of data?.symbols || []) {
      const e = sym.family ? Ann.extent({ body: sym.family.body, labels: sym.labels }) : { minX: -3, maxX: 3, minY: -3, maxY: 3 };
      const pad = 0.8, a = turn(sym), c = Math.cos(a), sn = Math.sin(a);
      const corners = [[e.minX - pad, e.minY - pad], [e.maxX + pad, e.minY - pad], [e.maxX + pad, e.maxY + pad], [e.minX - pad, e.maxY + pad]];
      const ring = corners.map(([x, y]) => [sym.at.x + (x * c - y * sn) * s, sym.at.y + (x * sn + y * c) * s]);
      out.push({ id: sym.id, kind: "symbol", category: "Generic Annotations", ring, pinned: sym.pinned, display: "normal" });
    }
    return out;
  }

  // the tool: `family` null places the project's North Arrow (made the first time)
  let family = null;
  async function start(north) {
    if (!(await ensureDetailView(north ? "A north arrow" : "A symbol"))) return;
    if (north) family = ((await run("symbol_families", {})) || { families: [] }).families.find((f) => f.name === "North Arrow")?.id ?? null;
    else {
      const r = await run("symbol_families", {});
      const fams = (r && r.families) || [];
      if (fams.length > 1) {
        const picked = await new Promise((resolve) => openModal("Symbol", (body, foot, close) => {
          const sel = el("select", {}, fams.map((f) => new Option(f.name, String(f.id), false, f.id === family)));
          body.append(el("label", { className: "new-row" }, [el("span", { textContent: "Family" }), sel]));
          foot.append(button("Cancel", () => { close(); resolve(undefined); }), button("OK", () => { close(); resolve(Number(sel.value)); }, true));
        }));
        if (picked === undefined) return;
        family = picked;
      } else family = fams[0]?.id ?? null;
    }
    noteView().setTool("symbol");
    setPrompt("Click where the symbol goes; rotate it afterwards with Rotate or its Properties. Esc finishes.");
  }
  async function place(p) {
    const r = await run("add_symbol", { view_id: app.view.viewId, family_id: family, point: [Math.round(p.x * 100) / 100, Math.round(p.y * 100) / 100] });
    if (r) { await refresh(); setPrompt("Placed. Click to place another, Esc finishes."); }
  }
  return { draw, elements, start, place };
})();
