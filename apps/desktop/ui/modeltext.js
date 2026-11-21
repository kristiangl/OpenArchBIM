// Model Text in the project (Architecture > Model Text): the text, its size and depth and whether it stands upright,
// asked first, then a click in a plan places it (the core: modeltext.rs). Plans draw flat text's letters and the strip
// upright text stands on; each is picked by the box round it.
const ModelText = (() => {
  const opts = { text: "", size_mm: 300, depth_mm: 50, upright: false };

  function draw(ctx, data, toScreen, k, sel = new Set()) {
    for (const t of data?.model_texts || []) {
      ctx.save();
      ctx.beginPath();
      for (const r of t.rings) { r.forEach(([x, y], i) => { const q = toScreen({ x, y }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); }); ctx.closePath(); }
      const on = sel.has(t.id);
      ctx.fillStyle = on ? "rgba(46,111,158,0.35)" : "rgba(120,120,120,0.35)"; ctx.fill("evenodd");
      ctx.strokeStyle = on ? "#2e6f9e" : "#444"; ctx.lineWidth = on ? 1.5 : 0.8; ctx.stroke();
      ctx.restore();
    }
  }
  function box(rings) {
    let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
    for (const r of rings) for (const [x, y] of r) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    return [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
  }
  const elements = (data) => (data?.model_texts || []).filter((t) => t.rings.length).map((t) => ({ id: t.id, kind: "modeltext", category: "Generic Models", ring: t.upright ? t.rings[0] : box(t.rings), pinned: t.pinned, display: "normal" }));

  async function start() {
    if (!(await ensurePlan("Model text"))) return;
    openModal("Model Text", (body, foot, close) => {
      const txt = el("textarea", { rows: 2, style: "width:100%", value: opts.text, placeholder: "Text" });
      const n = (label, key) => { const i = el("input", { type: "number", min: "1", step: "10", value: opts[key], style: "width:90px", onchange: () => { const v = parseFloat(i.value); if (v > 0) opts[key] = v; } }); return el("label", { className: "new-row" }, [el("span", { textContent: label }), i]); };
      const up = el("input", { type: "checkbox", checked: opts.upright, onchange: () => { opts.upright = up.checked; } });
      body.append(txt, n("Size (mm)", "size_mm"), n("Depth (mm)", "depth_mm"), el("label", { className: "new-row" }, [el("span", { textContent: "Upright (stands on the level, facing down the plan when unturned)" }), up]));
      foot.append(button("Cancel", close), button("OK", () => {
        if (!txt.value.trim()) { txt.focus(); return; }
        opts.text = txt.value;
        close();
        Plan.setTool("mtext");
        setPrompt("Click where the text starts. Esc finishes.");
      }, true));
      setTimeout(() => txt.focus(), 0);
    });
  }
  async function place(p) {
    const level = (app.info.views || []).find((v) => v.id === app.view.viewId)?.level_id;
    if (level == null) { setPrompt("Model text is placed in a floor plan.", true); return; }
    const r = await run("add_model_text", { level_id: level, point: [p.x, p.y], ...opts });
    if (r) { await refresh(); setPrompt("Model text placed. Click to place it again, or Esc."); }
  }
  return { draw, elements, start, place };
})();
