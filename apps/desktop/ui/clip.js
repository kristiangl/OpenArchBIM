// Clipboard (Modify > Clipboard): Copy to Clipboard (Ctrl+C), Cut (Ctrl+X), Paste (Ctrl+V: from the clipboard at a
// picked point, or aligned to selected levels, selected views, the current view or the same place) and Match Type
// Properties. The clipboard lives in the Rust session (crates/openarch-api/src/clipboard.rs).
const Clip = (() => {
  let info = { count: 0 };
  let match = null; // Match Type: the source element once picked
  const viewKind = () => (app.info.views || []).find((v) => v.id === app.view.viewId)?.view_kind;
  const inModelView = () => ["plan", "elevation", "3d"].includes(app.view.kind);

  async function load(update = true) { info = (await Backend.call("clipboard_info", {})) || { count: 0 }; if (update) updateRibbonContext(); }

  function enabled(id) {
    if (id === "clip-copy" || id === "clip-cut") return app.selection.length > 0 && inModelView();
    if (id === "match-type") return app.view.kind === "plan";
    if (!info.count || !info.same_project) return false;
    if (id === "paste") return app.view.kind === "plan";
    if (id === "paste-levels") return info.has_model;
    if (id === "paste-views") return !info.has_model && info.has_view_specific;
    if (id === "paste-current") return ["plan", "elevation"].includes(app.view.kind);
    return id === "paste-same";
  }

  const skippedText = (r) => { const n = (r.skipped || []).reduce((a, s) => a + s.count, 0); return n ? ` (${n} could not be copied: ${r.skipped.map((s) => s.kind).join(", ")})` : ""; };
  async function copy(cut = false) {
    if (!app.selection.length) { setPrompt("Select elements to copy.", true); return; }
    const r = await run(cut ? "cut" : "copy", { ids: app.selection, view_id: app.view.viewId });
    if (!r) return;
    if (cut) { app.selection = []; Plan.setSelection([]); app.vp?.setSelection([]); await refresh(); }
    await load();
    setPrompt(`${cut ? "Cut" : "Copied"} ${r.count} element${r.count === 1 ? "" : "s"} to the clipboard${skippedText(r)}. Paste with Ctrl+V.`);
  }

  async function done(r) {
    if (!r) return;
    await refresh();
    app.selection = r.ids; Plan.setSelection(r.ids); app.vp?.setSelection(r.ids); renderProperties(); updateRibbonContext();
    setPrompt(`Pasted ${r.ids.length} element${r.ids.length === 1 ? "" : "s"}.`);
  }

  // Paste from Clipboard: the copy follows the cursor until a click puts its middle there
  async function paste() {
    await load();
    if (!info.count) { setPrompt("The clipboard is empty: select elements and Copy (Ctrl+C) or Cut them first.", true); return; }
    if (!info.same_project) { setPrompt("The clipboard holds elements from another project. Copying between projects comes later.", true); return; }
    if (app.view.kind !== "plan") { if (!info.has_model) { pasteCurrent(); return; } setPrompt("Paste at a point in a plan view, or use Aligned to Same Place.", true); return; }
    Plan.setTool("paste");
    updateRibbonContext(true);
    setPrompt("Click to place the pasted elements (the middle of what was copied goes at the cursor). Esc cancels.");
  }
  async function place(p) {
    const r = await run("paste", { mode: "point", view_id: app.view.viewId, point: [p.x, p.y] });
    if (r) { Plan.setTool("select"); await done(r); }
  }
  function context() {
    return { auto: true, key: "paste", title: "Paste", groups: [{ label: "Mode", items: [{ label: "Cancel", ico: "✕", big: true, onclick: () => { Plan.setTool("select"); updateRibbonContext(); setPrompt(""); } }] }] };
  }
  function drawPreview(ctx, toScreen, at) {
    if (!at || !info.preview) return;
    const dx = at.x - info.origin[0], dy = at.y - info.origin[1];
    ctx.save();
    ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1.5; ctx.setLineDash([6, 4]);
    for (const l of info.preview) { ctx.beginPath(); l.forEach(([x, y], i) => { const q = toScreen({ x: x + dx, y: y + dy }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); }); ctx.stroke(); }
    ctx.restore();
  }

  const pasteCurrent = async () => done(await run("paste", { mode: "current_view", view_id: app.view.viewId }));
  const pasteSame = async () => done(await run("paste", { mode: "same_place" }));

  // Aligned to Selected Levels / Views: tick the levels or views to paste into
  function pick(title, items, mode, key) {
    openModal(title, (body, foot, close) => {
      const boxes = items.map((x) => { const c = el("input", { type: "checkbox" }); return { x, c, node: el("div", {}, [el("label", {}, [c, " ", x.name])]) }; });
      const msg = el("div", { className: "modal-msg" });
      body.append(...boxes.map((b) => b.node), msg);
      foot.append(button("Cancel", close), button("OK", async () => {
        const ids = boxes.filter((b) => b.c.checked).map((b) => b.x.id);
        if (!ids.length) { msg.textContent = "Tick at least one."; return; }
        const r = await run("paste", { mode, [key]: ids });
        if (r) { close(); await done(r); }
      }, true));
    });
  }
  async function pasteLevels() {
    await load();
    const levels = [...app.info.levels].sort((a, b) => a.elevation_mm - b.elevation_mm);
    pick("Select Levels", levels, "levels", "levels");
  }
  async function pasteViews() {
    await load();
    const kind = info.view_kind;
    const views = (app.info.views || []).filter((v) => v.view_kind === kind && v.id !== info.view_id);
    if (!views.length) { setPrompt("There is no other view of that kind to paste into.", true); return; }
    pick("Select Views", views, "views", "views");
  }

  // Match Type Properties: pick the element to match, then each element to change; Esc finishes
  function matchType() {
    match = app.selection.length === 1 ? app.selection[0] : null;
    Plan.setTool("matchtype");
    updateRibbonContext(true);
    setPrompt(match ? "Click elements to give them the selected element's type. Esc finishes." : "Click the element whose type to match.");
  }
  async function matchClick(id) {
    if (id == null) { setPrompt(match ? "Click an element of the same kind." : "Click the element whose type to match.", true); return; }
    if (match == null) { match = id; app.selection = [id]; Plan.setSelection([id]); renderProperties(); setPrompt("Now click elements to give them its type. Esc finishes."); return; }
    const r = await run("match_type", { source: match, targets: [id] });
    if (r) { await refresh(); setPrompt("Matched. Click more elements, or Esc to finish."); }
  }
  function matchContext() {
    return { auto: true, key: `matchtype:${match}`, title: "Match Type", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => { match = null; Plan.setTool("select"); updateRibbonContext(); } }] }] };
  }
  function endTool() { match = null; }

  return { load, enabled, copy, paste, place, context, drawPreview, pasteCurrent, pasteSame, pasteLevels, pasteViews, matchType, matchClick, matchContext, endTool, info: () => info };
})();
