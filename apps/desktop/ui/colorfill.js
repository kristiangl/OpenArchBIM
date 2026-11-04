// Colour fills: the coloured rooms or areas of a plan (behind or in front of the model), colour fill legends, Edit
// Color Scheme, and Area Schemes. Private notes: areas.md.
const ColorFill = (() => {
  const hexA = (h, a) => { const n = parseInt(h.slice(1), 16); return `rgba(${n >> 16},${(n >> 8) & 255},${n & 255},${a})`; };

  // the coloured faces: `layer` "background" draws before the model, "foreground" after it
  function drawFills(ctx, data, toScreen, layer) {
    if (!data?.color_fills?.length || (data.color_location || "background") !== layer) return;
    ctx.save();
    for (const f of data.color_fills) {
      ctx.beginPath();
      for (const r of f.rings) { r.forEach((p, i) => { const q = toScreen({ x: p[0], y: p[1] }); i ? ctx.lineTo(q.x, q.y) : ctx.moveTo(q.x, q.y); }); ctx.closePath(); }
      ctx.fillStyle = layer === "foreground" ? hexA(f.color, 0.75) : f.color;
      ctx.fill("evenodd");
    }
    ctx.restore();
  }

  // a legend's layout in paper mm: title, then a swatch and label per row
  const L = { title: 3.5, text: 2.5, row: 6, sw: 8, swh: 4.5, gap: 3, pad: 1 };
  function size(lg) { const w = Math.max(lg.title.length * L.title * 0.62, ...lg.rows.map((r) => L.sw + L.gap + r.label.length * L.text * 0.6), 30); return { w, h: L.title + 3 + lg.rows.length * L.row }; }
  function elements(data) {
    const s = data?.view?.scale || 100;
    return (data?.color_legends || []).map((lg) => {
      const { w, h } = size(lg), a = lg.at;
      return { id: lg.id, kind: "colorlegend", category: "Color Fill Legends", ring: [[a.x, a.y], [a.x + w * s, a.y], [a.x + w * s, a.y - h * s], [a.x, a.y - h * s]], pinned: lg.pinned, display: "normal" };
    });
  }
  function drawLegends(ctx, data, toScreen, k, sel = new Set()) {
    const s = data?.view?.scale || 100, px = (mm) => mm * s * k;
    for (const lg of data?.color_legends || []) {
      const o = toScreen(lg.at), col = sel.has(lg.id) ? "#2e6f9e" : "#222";
      ctx.save();
      ctx.textBaseline = "top"; ctx.fillStyle = col;
      ctx.font = `bold ${px(L.title)}px sans-serif`;
      ctx.fillText(lg.title, o.x, o.y);
      ctx.font = `${px(L.text)}px sans-serif`;
      lg.rows.forEach((r, i) => {
        const y = o.y + px(L.title + 3 + i * L.row);
        ctx.fillStyle = r.color; ctx.fillRect(o.x, y, px(L.sw), px(L.swh));
        ctx.strokeStyle = col; ctx.lineWidth = 1; ctx.strokeRect(o.x, y, px(L.sw), px(L.swh));
        ctx.fillStyle = col; ctx.fillText(r.label, o.x + px(L.sw + L.gap), y + px((L.swh - L.text) / 2));
      });
      if (!lg.rows.length) { ctx.font = `italic ${px(L.text)}px sans-serif`; ctx.fillText("(no coloured elements in the view)", o.x, o.y + px(L.title + 3)); }
      if (sel.has(lg.id)) { const { w, h } = size(lg); ctx.setLineDash([4, 3]); ctx.strokeStyle = "#2e6f9e"; ctx.strokeRect(o.x - 3, o.y - 3, px(w) + 6, px(h) + 6); }
      ctx.restore();
    }
  }

  // Color Fill Legend: click to place it; with no scheme in the view, choose one first
  async function start() {
    const v = (app.info.views || []).find((x) => x.id === app.view.viewId);
    if (app.view.kind !== "plan" || !v || !["floor_plan", "structural_plan", "area_plan", "callout"].includes(v.view_kind)) { setPrompt("A colour fill legend goes in a floor plan or an area plan.", true); return; }
    Plan.setTool("colorlegend");
    updateRibbonContext(true);
    setPrompt("Click to place the colour fill legend (its top left corner). Esc finishes.");
  }
  function context() {
    return { auto: true, key: "colorlegend", title: "Color Fill Legend", groups: [{ label: "Mode", items: [{ label: "Modify", ico: "Se", big: true, onclick: () => Plan.setTool("select") }] }] };
  }
  async function place(p) {
    const view = app.view.viewId;
    const list = await run("color_schemes", { id: view });
    if (!list) return;
    if (!list.current) {
      if (list.locked) { setPrompt("The view template controls this view's colour scheme, and it shows none.", true); return; }
      const id = await chooseScheme(list);
      if (!id) return;
      if (!(await run("set_view_color_scheme", { view_id: view, scheme_id: id }))) return;
    }
    if (await run("add_color_legend", { view_id: view, point: [p.x, p.y] })) { await refresh(); Plan.setTool("select"); updateRibbonContext(); }
  }
  function chooseScheme(list) {
    return new Promise((resolve) => openModal("Choose Color Scheme", (body, foot, close) => {
      const pick = el("select", {}, list.schemes.map((s) => new Option(s.name, s.id)));
      body.append(el("div", { className: "modal-msg", textContent: `The view has no colour scheme. Choose one for ${list.category}:` }), el("label", { className: "new-row" }, ["Color Scheme", pick]));
      body.closest(".modal-back").onEscape = () => { close(); resolve(null); };
      foot.append(button("Cancel", () => { close(); resolve(null); }), button("OK", () => { close(); resolve(Number(pick.value)); }, true));
    }));
  }

  // a selected legend: Edit Scheme
  function selectionContext() {
    if (app.view.kind !== "plan" || app.selection.length !== 1 || Plan.tool !== "select") return null;
    const lg = (Plan.viewData()?.color_legends || []).find((x) => x.id === app.selection[0]);
    if (!lg) return null;
    return { auto: true, key: `colorlegend:${lg.id}`, title: "Color Fill Legends", groups: [{ label: "Scheme", items: [{ label: "Edit Scheme", ico: "Cs", big: true, onclick: () => edit(lg.id) }] }] };
  }

  function askName(title, initial) {
    return new Promise((resolve) => openModal(title, (body, foot, close) => {
      const inp = el("input", { type: "text", value: initial || "" });
      const go = () => { close(); resolve(inp.value.trim() || null); };
      inp.onkeydown = (e) => { if (e.key === "Enter") go(); };
      body.append(el("label", { className: "new-row" }, ["Name", inp]));
      body.closest(".modal-back").onEscape = () => { close(); resolve(null); };
      foot.append(button("Cancel", () => { close(); resolve(null); }), button("OK", go, true));
      setTimeout(() => { inp.focus(); inp.select(); }, 0);
    }));
  }

  // Edit Color Scheme: the view's category's schemes on the left; the chosen one's title, parameter, by value or
  // range, and its colours on the right. OK shows the chosen scheme in the view (or none).
  async function edit(id) {
    let list = await run("color_schemes", { id });
    if (!list) return;
    const view = list.view_id;
    let chosen = list.current ?? null, draft = null;
    openModal(`Edit Color Scheme: ${list.category}`, (body, foot, close) => {
      const left = el("div", { className: "cs-list" }), right = el("div", { className: "cs-edit" }), msg = el("div", { className: "modal-msg" });
      body.append(el("div", { className: "cs-wrap" }, [left, right]), msg);
      body.style.minWidth = "640px";
      const scheme = () => list.schemes.find((s) => s.id === draft?.id);
      const take = (id) => { const s = list.schemes.find((x) => x.id === id); draft = s ? JSON.parse(JSON.stringify(s)) : null; };
      // apply the draft now, so a change of parameter brings that parameter's values
      async function save() { if (!draft) return true; const ok = await run("set_color_scheme", { id: draft.id, scheme: { name: draft.name, title: draft.title, parameter: draft.parameter, by_range: draft.by_range, rows: draft.rows.map((r) => ({ value: r.value, at_least: r.at_least, color: r.color, visible: r.visible })) } }); if (!ok) return false; await reload(draft.id); return true; }
      async function reload(keep) { const l = await run("color_schemes", { id: view }); if (l) { list = l; take(keep ?? draft?.id); paint(); } }
      function paint() {
        left.replaceChildren(el("div", { className: "ex-h", textContent: "Schemes" }),
          el("label", { className: "cs-item" }, [el("input", { type: "radio", name: "cs", checked: chosen == null, disabled: list.locked, onchange: () => { chosen = null; } }), " (none)"]),
          ...list.schemes.map((s) => el("label", { className: "cs-item" + (draft?.id === s.id ? " on" : "") }, [
            el("input", { type: "radio", name: "cs", checked: chosen === s.id, disabled: list.locked, onchange: () => { chosen = s.id; } }), " ",
            el("a", { href: "#", textContent: s.name, onclick: async (e) => { e.preventDefault(); if (await save()) { take(s.id); paint(); } } })])),
          el("div", { className: "cs-tools" }, [
            button("Duplicate", async () => { if (!draft || !(await save())) return; const n = await askName("Duplicate Color Scheme", `${draft.name} 2`); if (!n) return; const r = await run("duplicate_color_scheme", { id: draft.id, name: n }); if (r) await reload(r.id); }),
            button("Rename", async () => { if (!draft) return; const n = await askName("Rename Color Scheme", draft.name); if (!n) return; draft.name = n; await save(); }),
            button("Delete", async () => { if (!draft || list.schemes.length < 2) { msg.textContent = "A category keeps at least one colour scheme."; return; } if (!(await askConfirm(`Delete the colour scheme ${draft.name}? Views that show it will show none.`))) return; const gone = draft.id, keep = list.schemes.find((s) => s.id !== gone)?.id; if (await run("delete_color_scheme", { id: gone })) { if (chosen === gone) chosen = null; await reload(keep); } }),
          ]));
        if (list.locked) left.append(el("div", { className: "modal-msg", textContent: "The view template controls this view's colour scheme." }));
        right.replaceChildren();
        if (!draft) return;
        const title = el("input", { type: "text", value: draft.title, onchange: () => { draft.title = title.value; } });
        const param = el("select", { onchange: async () => { draft.parameter = param.value; if (!list.parameters.find((p) => p.name === param.value)?.numeric) draft.by_range = false; draft.rows = []; await save(); } }, list.parameters.map((p) => new Option(p.name, p.name, false, p.name === draft.parameter)));
        const numeric = list.parameters.find((p) => p.name === draft.parameter)?.numeric;
        const mode = (v, label) => el("label", {}, [el("input", { type: "radio", name: "csmode", checked: draft.by_range === v, disabled: v && !numeric, onchange: async () => { draft.by_range = v; draft.rows = []; await save(); } }), ` ${label}`]);
        right.append(el("label", { className: "new-row" }, ["Title", title]), el("label", { className: "new-row" }, ["Color", param]), el("div", { className: "new-row" }, [mode(false, "By value"), " ", mode(true, "By range")]));
        const table = el("table", { className: "vg cs-table" });
        table.append(el("tr", {}, [draft.by_range ? el("th", { textContent: "At least" }) : null, el("th", { textContent: "Value" }), el("th", { textContent: "Color" }), el("th", { textContent: "Visible" }), el("th", { textContent: "In use" }), draft.by_range ? el("th") : null].filter(Boolean)));
        draft.rows.forEach((r, i) => {
          const at = draft.by_range ? el("input", { type: "number", value: r.at_least ?? 0, style: "width:80px", onchange: () => { r.at_least = Number(at.value); } }) : null;
          table.append(el("tr", {}, [
            at ? el("td", {}, [at]) : null,
            el("td", { textContent: r.label }),
            el("td", {}, [el("input", { type: "color", value: r.color, onchange: (e) => { r.color = e.target.value; } })]),
            el("td", {}, [el("input", { type: "checkbox", checked: r.visible, onchange: (e) => { r.visible = e.target.checked; } })]),
            el("td", { textContent: r.in_use ? "Yes" : "No" }),
            draft.by_range ? el("td", {}, [button("Remove", async () => { draft.rows.splice(i, 1); if (!draft.rows.length) draft.rows.push({ at_least: 0, color: "#a6cee3", visible: true }); await save(); })]) : null,
          ].filter(Boolean)));
        });
        right.append(table);
        if (draft.by_range) right.append(button("Add Range", async () => { const last = draft.rows[draft.rows.length - 1]; draft.rows.push({ at_least: (last?.at_least ?? 0) + 10, color: "#b2df8a", visible: true }); await save(); }));
        else if (draft.rows.some((r) => !r.in_use)) right.append(button("Remove Unused Values", async () => { draft.rows = draft.rows.filter((r) => r.in_use); await save(); }));
      }
      take(chosen ?? list.schemes[0]?.id);
      paint();
      foot.append(button("Cancel", close), button("Apply", () => save()), button("OK", async () => {
        if (!(await save())) return;
        if (!list.locked && chosen !== (list.current ?? null) && !(await run("set_view_color_scheme", { view_id: view, scheme_id: chosen }))) return;
        close(); await refresh();
      }, true));
    }, { wide: true });
  }

  // Area Schemes: Gross Building stays; others are named, described, measured by Gross Building or Rentable rules,
  // and deleted with their area plans
  async function schemes() {
    if (!(app.info.area_schemes || []).length) { if (!(await run("ensure_area_schemes", {}))) return; await refresh(); }
    openModal("Area Schemes", (body, foot, close) => {
      const table = el("table", { className: "vg" }), msg = el("div", { className: "modal-msg" });
      body.append(table, msg);
      const rulesSel = (v, dis) => el("select", { disabled: dis }, [new Option("Gross Building", "gross_building", false, v === "gross_building"), new Option("Rentable", "rentable", false, v === "rentable")]);
      function paint() {
        table.replaceChildren(el("tr", {}, ["Name", "Description", "Rules", ""].map((h) => el("th", { textContent: h }))));
        for (const s of app.info.area_schemes) {
          const name = el("input", { type: "text", value: s.name, disabled: s.built_in }), desc = el("input", { type: "text", value: s.description || "", disabled: s.built_in, style: "width:260px" }), rules = rulesSel(s.rules, s.built_in || s.has_areas);
          const commit = async () => { if (await run("patch_area_scheme", { id: s.id, name: name.value, description: desc.value, rules: rules.value })) { await refresh(); } else paint(); };
          name.onchange = desc.onchange = rules.onchange = commit;
          table.append(el("tr", {}, [el("td", {}, [name]), el("td", {}, [desc]), el("td", {}, [rules]), el("td", {}, [s.built_in ? "" : button("Delete", async () => {
            const plans = (app.info.views || []).filter((v) => v.area_scheme === s.id).length;
            if (!(await askConfirm(`Delete the area scheme ${s.name}${plans ? ` and its ${plans} area plan${plans === 1 ? "" : "s"}, with their areas` : ""}?`))) return;
            if (await run("delete_area_scheme", { id: s.id })) { await refresh(); paint(); }
          })])]));
        }
      }
      paint();
      foot.append(button("New", async () => {
        const n = await askName("New Area Scheme", "Area Scheme");
        if (!n) return;
        if (await run("add_area_scheme", { name: n, description: "", rules: "rentable" })) { await refresh(); paint(); }
      }), button("Close", close, true));
    }, { wide: true });
  }

  return { drawFills, drawLegends, elements, start, context, place, selectionContext, edit, schemes };
})();
