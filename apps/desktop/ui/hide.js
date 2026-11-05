// Hiding in a view: Temporary Hide/Isolate (for this session only), Hide in View (elements and categories, saved
// with the view), Reveal Hidden Elements and Override Graphics in View by element. The Rust side folds the temporary
// state into each drawing (crates/openarch-api/src/hide.rs); this keeps the view's frame and the ribbon in step.
const Hide = (() => {
  let state = { temporary: false, isolated: false, reveal: false, revealed: [] };
  const viewOk = () => ["plan", "elevation", "3d"].includes(app.view.kind) && app.view.viewId != null;
  const sel = () => app.selection.slice();

  async function load() {
    state = viewOk() ? (await Backend.call("display_state", { view_id: app.view.viewId }).catch(() => null)) || state : { temporary: false, isolated: false, reveal: false, revealed: [] };
    frame();
  }

  // the view's frame: Temporary Hide/Isolate in cyan, Reveal Hidden Elements in magenta
  function frame() {
    const w = document.getElementById("viewport-wrap");
    const mode = viewOk() && state.reveal ? "reveal" : viewOk() && state.temporary ? "temp" : "";
    if (mode) { w.dataset.display = mode; w.dataset.label = mode === "reveal" ? "Reveal Hidden Elements" : "Temporary Hide/Isolate"; }
    else { delete w.dataset.display; delete w.dataset.label; }
    const t = document.getElementById("vb-temp"), r = document.getElementById("vb-reveal");
    if (t) { t.disabled = !viewOk(); t.classList.toggle("on", mode === "temp"); }
    if (r) { r.disabled = !viewOk(); r.classList.toggle("on", !!state.reveal); }
  }

  async function after(text) {
    app.selection = []; Plan.setSelection([]); Elev.setSelection([]); app.vp?.setSelection([]);
    await refresh();
    await load();
    renderProperties(); updateRibbonContext();
    if (text) setPrompt(text);
  }

  const needSel = () => { if (!app.selection.length) { setPrompt("Select elements first.", true); return false; } return true; };

  async function temp(op) {
    if (!viewOk()) return;
    if (!["reset", "apply"].includes(op) && !needSel()) return;
    const r = await run("temp_hide", { view_id: app.view.viewId, op, ids: sel() });
    if (!r) return;
    const said = { hide_elements: "Hidden for now.", isolate_elements: "Isolated for now.", hide_categories: "Category hidden for now.", isolate_categories: "Category isolated for now.", reset: "Temporary Hide/Isolate reset.", apply: "Applied to the view." }[op];
    await after(said);
  }

  async function hideInView(categories, hide = true) {
    if (!viewOk() || !needSel()) return;
    const ids = sel();
    if (categories) {
      const r = await run("hide_categories", { view_id: app.view.viewId, ids, hide });
      if (r) await after(`${hide ? "Hid" : "Showed"} ${r.categories.join(", ")} in this view.`);
    } else {
      const r = await run("hide_elements", { view_id: app.view.viewId, ids, hide });
      if (r) await after(`${hide ? "Hid" : "Showed"} ${ids.length} element${ids.length === 1 ? "" : "s"} in this view.`);
    }
  }

  async function reveal(on = !state.reveal) {
    if (!viewOk()) return;
    const r = await run("reveal_hidden", { view_id: app.view.viewId, on });
    if (r) await after(on ? "Hidden elements are shown in magenta. Select them and Unhide, or turn Reveal Hidden off." : "");
  }
  // the contextual panel while Reveal Hidden Elements is on
  function context() {
    if (!viewOk() || !state.reveal) return null;
    const some = app.selection.some((id) => state.revealed.includes(id));
    return { auto: true, key: `reveal:${some}`, title: "Reveal Hidden Elements", groups: [{ label: "Reveal Hidden Elements", items: [
      { label: "Unhide Element", icon: "revealHidden", disabled: !some, onclick: () => hideInView(false, false) },
      { label: "Unhide Category", icon: "revealHidden", disabled: !some, onclick: () => hideInView(true, false) },
      { label: "Toggle Reveal Hidden Elements Mode", icon: "revealHidden", big: true, onclick: () => reveal(false) },
    ] }] };
  }

  function tempMenu(x, y) {
    const none = !app.selection.length;
    showMenu(x, y, [
      { label: "Isolate Category", keys: "IC", disabled: none, run: () => temp("isolate_categories") },
      { label: "Hide Category", keys: "HC", disabled: none, run: () => temp("hide_categories") },
      { label: "Isolate Element", keys: "HI", disabled: none, run: () => temp("isolate_elements") },
      { label: "Hide Element", keys: "HH", disabled: none, run: () => temp("hide_elements") },
      { sep: true },
      { label: "Reset Temporary Hide/Isolate", keys: "HR", disabled: !state.temporary, run: () => temp("reset") },
      { label: "Apply Hide/Isolate to View", disabled: !state.temporary, run: () => temp("apply") },
    ]);
  }

  function enabled(id) {
    if (!viewOk()) return false;
    if (["temp-reset", "temp-apply"].includes(id)) return state.temporary;
    if (id === "reveal") return true;
    if (id === "override-cat") return true;
    return app.selection.length > 0;
  }

  // Override Graphics in View > By Element: View-Specific Element Graphics
  async function overrides() {
    if (!viewOk() || !needSel()) return;
    const ids = sel();
    const cur = await run("element_overrides", { view_id: app.view.viewId, ids });
    if (!cur) return;
    const std = await run("standards", {});
    const linePats = std ? std.line_patterns.map((p) => p.pattern.name) : ["Solid"];
    const fillPats = std ? std.fill_patterns.map((p) => p.pattern.name) : ["Solid fill"];
    const o = JSON.parse(JSON.stringify(cur.override));
    openModal("View-Specific Element Graphics", (body, foot, close) => {
      const msg = el("div", { className: "modal-msg" });
      const row = (label, ...ctl) => el("div", { className: "eg-row" }, [el("span", { textContent: label }), ...ctl]);
      const check = (v) => { const c = el("input", { type: "checkbox" }); c.checked = !!v; return c; };
      const select = (opts, v, first) => { const s = el("select"); s.add(new Option(first, "")); for (const p of opts) s.add(new Option(p, p, false, v === p)); return s; };
      const colour = (c, fallback) => { const use = check(!!c); const f = ColorPick.field(c ? rgb2hex(c) : fallback); f.onchange = () => { use.checked = true; }; return { use, f, get: () => (use.checked ? hex2rgb(f.value) : null) }; };
      const weight = (w) => { const s = el("select"); s.add(new Option("<By Category>", "")); for (let i = 1; i <= 16; i++) s.add(new Option(String(i), i, false, w === i)); return s; };
      const lines = (key, title) => {
        const l = o[key] || {};
        const pat = select(linePats, l.pattern, "<By Category>"), col = colour(l.color, "#000000"), wt = weight(l.weight);
        return { node: el("fieldset", {}, [el("legend", { textContent: title }), row("Pattern", pat), row("Colour", col.use, col.f), row("Weight", wt)]),
          read: () => { o[key] = { pattern: pat.value || null, color: col.get(), weight: wt.value ? Number(wt.value) : null }; } };
      };
      const fill = (key, title) => {
        const f = o[key] || { visible: true };
        const vis = check(f.visible !== false), pat = select(fillPats, f.pattern, "<By Category>"), col = colour(f.color, "#969ba2");
        return { node: el("fieldset", {}, [el("legend", { textContent: title }), row("Visible", vis), row("Pattern", pat), row("Colour", col.use, col.f)]),
          read: () => { o[key] = { visible: vis.checked, pattern: pat.value || null, color: col.get() }; } };
      };
      const visible = check(o.visible !== false), halftone = check(o.halftone);
      const trans = el("input", { type: "number", min: 0, max: 100, value: o.transparency || 0 });
      const parts = [lines("projection_lines", "Projection Lines"), fill("surface_foreground", "Surface Patterns: Foreground"), fill("surface_background", "Surface Patterns: Background"),
        lines("cut_lines", "Cut Lines"), fill("cut_foreground", "Cut Patterns: Foreground"), fill("cut_background", "Cut Patterns: Background")];
      body.append(el("p", { className: "note", textContent: `${ids.length} element${ids.length === 1 ? "" : "s"} (${[...new Set(cur.categories)].join(", ")}) in this view.` }),
        row("Visible", visible), row("Halftone", halftone), row("Surface Transparency", trans, " %"), el("div", { className: "eg-grid" }, parts.map((p) => p.node)), msg);
      const save = async (over) => {
        const err = await Backend.call("set_element_overrides", { view_id: app.view.viewId, ids, override: over }).then(() => null, (e) => e);
        if (err) { msg.textContent = err.message || String(err); return; }
        close(); await refresh(); setPrompt(over ? "Graphics overridden in this view." : "Overrides cleared.");
      };
      foot.append(button("Reset", () => save(null)), button("Cancel", close), button("OK", () => {
        parts.forEach((p) => p.read());
        o.visible = visible.checked; o.halftone = halftone.checked;
        o.transparency = Math.max(0, Math.min(100, parseInt(trans.value) || 0));
        save(o);
      }, true));
    }, { wide: true });
  }

  // the view bar's buttons
  document.getElementById("vb-temp")?.addEventListener("click", (e) => { const r = e.currentTarget.getBoundingClientRect(); tempMenu(r.left, r.top - 200); });
  document.getElementById("vb-reveal")?.addEventListener("click", () => reveal());

  return { load, frame, temp, hideInView, reveal, context, tempMenu, enabled, overrides, state: () => state };
})();
