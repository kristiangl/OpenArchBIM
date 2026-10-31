// Schedules: the table a schedule view shows (worked out by the core from the live model each time), its cells
// edited in place (an edit goes to the element, or to its type, as the Properties palette would send it), the rows'
// commands (Highlight in Model, Delete Row), export to CSV or tab-separated text, New Schedule (a category or
// Multi-Category, a material takeoff, a sheet list or a view list) and the Schedule Properties dialog (Fields, Filter,
// Sorting/Grouping, Formatting with conditional formats, Appearance).
const Sched = (() => {
  let table = null; // the schedule_table answer
  let picked = new Set(); // index of the selected rows
  const TABS = [["fields", "Fields"], ["filter", "Filter"], ["sorting", "Sorting/Grouping"], ["formatting", "Formatting"], ["appearance", "Appearance"]];
  const OPS = [["equal", "equals"], ["not_equal", "does not equal"], ["greater", "is greater than"], ["greater_or_equal", "is greater than or equal to"], ["less", "is less than"], ["less_or_equal", "is less than or equal to"], ["contains", "contains"], ["not_contains", "does not contain"], ["begins_with", "begins with"], ["not_begins_with", "does not begin with"], ["ends_with", "ends with"], ["not_ends_with", "does not end with"], ["has_value", "has a value"], ["has_no_value", "has no value"], ["has_parameter", "has parameter"], ["associated_with_global", "is associated with global parameter"], ["not_associated_with_global", "is not associated with global parameter"]];
  const GLOBAL_OPS = ["associated_with_global", "not_associated_with_global"];
  const NO_VALUE = ["has_value", "has_no_value", "has_parameter"];
  // what a schedule's rows are, for the count under the table
  const noun = (t) => (t.def && t.def.takeoff ? "material rows" : t.category === "Multi-Category" ? "elements" : t.category.toLowerCase());
  const hex = (c) => "#" + c.map((x) => x.toString(16).padStart(2, "0")).join("");
  const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const UNITS = [["number", "Number"], ["integer", "Integer"], ["length", "Length"], ["area", "Area"], ["volume", "Volume"], ["angle", "Angle"], ["currency", "Currency"], ["text", "Text"], ["yes_no", "Yes/No"]];
  const TOTALS = [["none", "No calculation"], ["totals", "Calculate totals"], ["min", "Calculate minimum"], ["max", "Calculate maximum"], ["min_max", "Calculate minimum and maximum"]];
  const numeric = (u) => !["text", "yes_no"].includes(u);

  // ---- the table ---------------------------------------------------------------------------------------------------
  async function render(id) {
    const host = $("#schedule-view");
    let t;
    try { t = await Backend.call("schedule_table", { id }); } catch (e) { host.textContent = String(e.message || e); return; }
    if (!table || table.id !== id) picked = new Set();
    table = t;
    const cols = t.columns, n = Math.max(1, cols.length);
    const bar = el("div", { className: "sched-bar" }, [
      button("Highlight in Model", () => highlight(), false),
      button("Delete Row", () => deleteRows(), false),
      el("span", { className: "spacer" }),
      button("Export...", () => exportDialog(), false),
      button("Schedule Properties...", () => edit(id, "fields"), false),
    ]);
    const tbl = el("table", { className: "sched2" });
    if (t.show_title) tbl.append(el("tr", { className: "s-title" }, [el("th", { colSpan: n, textContent: t.title })]));
    if (t.show_headers) tbl.append(el("tr", { className: "s-head" }, cols.map((c) => el("th", { textContent: c.heading, className: `a-${c.align}` }))));
    t.rows.forEach((r, i) => {
      if (r.kind === "header") { tbl.append(el("tr", { className: "s-group" }, [el("td", { colSpan: n, textContent: r.cells[0].text, style: `padding-left:${8 + 12 * (r.level || 0)}px` })])); return; }
      if (r.kind === "blank") { tbl.append(el("tr", { className: "s-blank" }, [el("td", { colSpan: n, innerHTML: "&nbsp;" })])); return; }
      const tr = el("tr", { className: `s-${r.kind}${picked.has(i) ? " picked" : ""}` }, r.cells.map((c, k) => {
        const td = el("td", { textContent: c.text, className: `a-${cols[k] ? cols[k].align : "left"}${c.row ? " editable" : ""}` });
        if (c.bg) td.style.background = c.bg;
        if (c.row) td.ondblclick = (e) => { e.stopPropagation(); editCell(td, c); };
        return td;
      }));
      if (r.kind === "item") tr.onclick = (e) => pick(i, e.ctrlKey || e.shiftKey);
      tbl.append(tr);
    });
    if (!t.rows.length) tbl.append(el("tr", {}, [el("td", { colSpan: n, className: "empty", textContent: cols.length ? `No ${noun(t)} to list.` : "No fields yet: add some in Schedule Properties." })]));
    const wrap = el("div", { className: "sched-wrap sched-live" }, [bar, tbl, el("div", { className: "sched-count", textContent: `${t.count} ${noun(t)}` })]);
    host.replaceChildren(wrap);
  }

  // a row picked selects its elements, so Properties shows them
  function pick(i, add) {
    if (!add) picked = new Set();
    if (picked.has(i)) picked.delete(i); else picked.add(i);
    app.selection = [...new Set([...picked].flatMap((k) => table.rows[k].ids))];
    renderProperties();
    render(table.id);
  }
  const pickedIds = () => [...new Set([...picked].flatMap((k) => (table.rows[k] ? table.rows[k].ids : [])))];

  // a cell edited where it is: the palette's editor for its row; the patch goes to every target (each element, or
  // each type, the row stands for)
  function editCell(td, c) {
    const r = c.row;
    const send = async (patch) => {
      const q = c.type ? `Are you sure you want to change the type "${c.type.name}"?\nThis will update ${c.type.instances} instance${c.type.instances === 1 ? "" : "s"} of it.`
        : c.targets.length > 1 ? `This will change ${c.targets.length} elements. Go ahead?` : null;
      if (q && !(await askConfirm(q, "Change Value"))) { render(table.id); return; }
      for (const t of c.targets) { try { await Backend.call(t.method, { id: t.id, patch, ...(t.extra || {}) }); } catch (e) { setPrompt(String(e.message || e), true); break; } }
      await refresh();
    };
    let inp;
    if (r.kind === "select") {
      inp = el("select", {}, r.options.map((o, i) => new Option(o.label, i, false, o.selected)));
      inp.onchange = () => send(r.options[Number(inp.value)].patch);
    } else if (r.kind === "bool") {
      inp = el("input", { type: "checkbox", checked: !!r.raw });
      inp.onchange = () => send(nestPatch(r.path, inp.checked));
    } else {
      inp = el("input", { type: r.kind === "number" ? "number" : "text", value: r.kind === "number" ? r.raw : r.value });
      if (r.kind === "number") inp.step = "any";
      const commit = () => { const v = r.kind === "number" ? parseFloat(inp.value) : inp.value; if (r.kind === "number" && Number.isNaN(v)) return render(table.id); send(nestPatch(r.path, v)); };
      inp.onkeydown = (e) => { if (e.key === "Enter") inp.blur(); if (e.key === "Escape") { inp.onblur = null; render(table.id); } };
      inp.onblur = commit;
    }
    td.replaceChildren(inp);
    inp.focus();
    if (inp.select) inp.select();
  }

  // Highlight in Model: a plan showing the picked elements, zoomed to them and with them selected; a sheet or view
  // list opens the sheet or view
  async function highlight() {
    const ids = pickedIds();
    if (!ids.length) { setPrompt("Pick a row first.", true); return; }
    if (table.category === "Sheets") { await openView("sheet", ids[0]); return; }
    if (table.category === "Views") { const v = app.info.views.find((x) => x.id === ids[0]); if (v) await openView(viewerOf(v.view_kind), v.id); return; }
    const plans = app.info.views.filter((v) => v.view_kind === "floor_plan");
    const order = [app.lastPlanId, ...plans.map((v) => v.id)].filter((x, i, a) => x != null && a.indexOf(x) === i);
    for (const vid of order) {
      await openView("plan", vid);
      if (Plan.frame(ids)) { app.selection = ids; Plan.setSelection(ids); renderProperties(); setPrompt(`${ids.length} element${ids.length === 1 ? "" : "s"} highlighted.`); return; }
    }
    setPrompt("No plan shows these elements.", true);
  }

  async function deleteRows() {
    const ids = pickedIds();
    if (!ids.length) { setPrompt("Pick a row first.", true); return; }
    const what = table.category === "Sheets" ? "sheet" : table.category === "Views" ? "view" : "element";
    if (!(await askConfirm(`Delete ${ids.length} ${what}${ids.length === 1 ? "" : "s"} from the model?`))) return;
    if (await run("delete", { ids })) { picked = new Set(); app.selection = []; await refresh(); }
  }

  // ---- export as delimited text ------------------------------------------------------------------------------------
  function text(t, o) {
    const q = o.qualifier === "double" ? '"' : o.qualifier === "single" ? "'" : "";
    const cell = (s) => (q ? q + String(s).split(q).join(q + q) + q : String(s));
    const lines = [];
    if (o.title && t.show_title) lines.push(cell(t.title));
    if (o.headers) lines.push(t.columns.map((c) => cell(c.heading)).join(o.delimiter));
    for (const r of t.rows) {
      if ((r.kind === "header" || r.kind === "footer") && !o.groups) continue;
      if (r.kind === "blank" && !o.blanks) continue;
      if (r.kind === "header") lines.push(cell(r.cells[0].text));
      else lines.push(r.cells.map((c) => cell(c.text)).join(o.delimiter));
    }
    return lines.join("\r\n") + "\r\n";
  }
  function exportDialog() {
    const t = table;
    openModal("Export Schedule", (body, foot, close) => {
      const o = { delimiter: ",", qualifier: "double", title: true, headers: true, groups: true, blanks: false };
      const radio = (key, opts) => el("span", { className: "ex-radios" }, opts.map(([v, l]) => { const r = el("input", { type: "radio", name: `sx-${key}`, checked: o[key] === v }); r.onchange = () => { o[key] = v; }; return el("label", {}, [r, ` ${l}`]); }));
      const chk = (key, l) => { const c = el("input", { type: "checkbox", checked: o[key] }); c.onchange = () => { o[key] = c.checked; }; return el("label", {}, [c, ` ${l}`]); };
      body.append(
        el("div", { className: "ex-row2" }, [el("span", { textContent: "Delimiter:" }), radio("delimiter", [[",", "Comma (CSV)"], ["\t", "Tab"]])]),
        el("div", { className: "ex-row2" }, [el("span", { textContent: "Text qualifier:" }), radio("qualifier", [["double", '"'], ["single", "'"], ["none", "None"]])]),
        el("div", { className: "ex-bar" }, [chk("title", "Title"), chk("headers", "Column headers"), chk("groups", "Group headers and footers"), chk("blanks", "Blank lines")]),
      );
      foot.append(button("Cancel", close), button("Export...", async () => {
        const ext = o.delimiter === "\t" ? "txt" : "csv";
        const path = await Backend.pickSaveCsv(`${t.name}.${ext}`);
        if (!path) return;
        try { await Backend.call("write_file", { path, text: text(t, o) }); close(); setPrompt(`Exported ${t.name} to ${path}.`); } catch (e) { setPrompt(String(e.message || e), true); }
      }, true));
    });
  }

  // ---- New Schedule ------------------------------------------------------------------------------------------------
  // kind: "schedule" (a category or Multi-Category), "takeoff" (a material takeoff), "sheets" or "views" (a list)
  async function newSchedule(kind = "schedule") {
    const list = kind === "sheets" || kind === "views";
    let cats = list ? [] : await run("schedule_categories", {});
    if (!cats) return;
    if (kind === "takeoff") cats = cats.filter((c) => c.takeoff);
    const title = { schedule: "New Schedule", takeoff: "New Material Takeoff", sheets: "New Sheet List", views: "New View List" }[kind];
    const nameFor = (c) => (kind === "takeoff" ? `${c.replace(/s$/, "")} Material Takeoff` : `${c.replace(/s$/, "")} Schedule`);
    openModal(title, (body, foot, close) => {
      const pick = el("select", { size: 14, className: "sched-cats" }, cats.map((c) => new Option(`${c.name === "Multi-Category" ? "<Multi-Category>" : c.name}${c.count ? `  (${c.count})` : ""}`, c.name)));
      const first = cats.find((c) => c.count && c.name !== "Multi-Category") || cats[0];
      if (first) pick.value = first.name;
      const name = el("input", { type: "text", value: list ? (kind === "sheets" ? "Sheet List" : "View List") : nameFor(first.name) });
      pick.onchange = () => { name.value = nameFor(pick.value); };
      const msg = el("div", { className: "modal-msg" });
      if (!list) body.append(el("div", { className: "new-row" }, [el("span", { textContent: "Category" }), pick]));
      body.append(el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]), msg);
      const ok = async () => {
        try {
          const category = kind === "sheets" ? "Sheets" : kind === "views" ? "Views" : pick.value;
          const r = await Backend.call("add_schedule", { category, name: name.value.trim(), takeoff: kind === "takeoff" });
          close(); await refresh(); await openView("schedule", r.id); edit(r.id, "fields");
        } catch (e) { msg.textContent = String(e.message || e); }
      };
      pick.ondblclick = ok;
      foot.append(button("Cancel", close), button("OK", ok, true));
      if (list) name.focus();
    });
  }

  // ---- Schedule Properties -----------------------------------------------------------------------------------------
  async function edit(id, tab = "fields") {
    const t = await run("schedule_table", { id });
    if (!t) return;
    const def = JSON.parse(JSON.stringify(t.def));
    const avail = (await run("schedule_fields", { category: def.category, takeoff: !!def.takeoff })) || [];
    const globals = ((await run("global_params", {})) || { params: [] }).params.map((g) => g.name);
    const unitOf = (f) => (f.kind === "count" ? "integer" : f.kind === "formula" ? f.unit : f.kind === "percentage" ? "number" : f.kind === "combined" ? "text" : ((avail.find((a) => a.name === f.name) || {}).unit || "text"));
    const blank = (name, kind) => ({ name, kind, heading: "", hidden: false, totals: "none", align: "left", formula: "", unit: "number", of: "", parts: [], highlights: [], highlight_on_sheets: false });
    // a filter's or highlight's value: a global parameter's name for the global tests, nothing for the presence
    // tests, else text
    const valueBox = (op, value, onchange) => {
      const box = el("span", { className: "sched-val" });
      const draw = (o) => {
        if (GLOBAL_OPS.includes(o)) {
          const s = el("select", {}, [new Option("(any)", "", false, !value), ...globals.map((g) => new Option(g, g, false, g === value))]);
          s.onchange = () => { value = s.value; onchange(value); };
          box.replaceChildren(s);
        } else if (NO_VALUE.includes(o)) {
          box.replaceChildren();
        } else {
          const i = el("input", { type: "text", value });
          i.oninput = () => { value = i.value; onchange(value); };
          box.replaceChildren(i);
        }
      };
      draw(op);
      return { box, redraw: (o) => { if (GLOBAL_OPS.includes(o) || NO_VALUE.includes(o)) { value = ""; onchange(value); } draw(o); } };
    };
    openModal(`Schedule Properties: ${t.name}`, (body, foot, close) => {
      body.classList.add("ex-body", "sched-props");
      const bar = el("div", { className: "ex-tabs" }), pane = el("div", { className: "ex-tab" }), msg = el("div", { className: "modal-msg" });
      const opts = (pairs, cur) => pairs.map(([v, l]) => new Option(l, v, false, v === cur));
      const fieldOpts = (cur, none, noneLabel = "(none)") => [...(none ? [new Option(noneLabel, "", false, !cur)] : []), ...def.fields.map((f) => new Option(f.name, f.name, false, f.name === cur))];
      const draw = () => {
        bar.replaceChildren(...TABS.map(([k, l]) => { const b = el("button", { textContent: l, className: tab === k ? "on" : "" }); b.onclick = () => { tab = k; draw(); }; return b; }));
        pane.replaceChildren(...({ fields, filter, sorting, formatting, appearance })[tab]());
      };
      // Fields: what the category offers, and the schedule's columns in order
      let selAvail = null, selField = null;
      function fields() {
        const search = el("input", { type: "search", placeholder: "Search..." });
        const left = el("select", { size: 16, multiple: true, className: "sched-list" }), right = el("select", { size: 16, multiple: true, className: "sched-list" });
        const fillLeft = () => {
          const q = search.value.trim().toLowerCase();
          const all = [{ name: "Count", kind: "count", unit: "integer" }, ...avail];
          const tag = (a) => (def.category === "Multi-Category" || a.kind !== "type" ? "" : "  (type)");
          left.replaceChildren(...all.filter((a) => !def.fields.some((f) => f.name === a.name) && (!q || a.name.toLowerCase().includes(q))).map((a) => new Option(`${a.name}${tag(a)}`, a.name, false, a.name === selAvail)));
        };
        const fillRight = () => right.replaceChildren(...def.fields.map((f) => new Option(`${f.name}${f.kind === "formula" ? "  (formula)" : f.kind === "percentage" ? "  (percentage)" : f.kind === "combined" ? "  (combined)" : ""}`, f.name, false, f.name === selField)));
        search.oninput = fillLeft;
        left.onchange = () => { selAvail = left.value; };
        right.onchange = () => { selField = right.value; };
        const add = () => {
          const names = [...left.selectedOptions].map((o) => o.value);
          for (const nm of names) { const a = nm === "Count" ? { name: "Count", kind: "count" } : avail.find((x) => x.name === nm); if (a) def.fields.push(blank(a.name, a.kind)); }
          fillLeft(); fillRight();
        };
        const remove = () => {
          const names = [...right.selectedOptions].map((o) => o.value);
          def.fields = def.fields.filter((f) => !names.includes(f.name));
          def.sort = def.sort.filter((k) => !names.includes(k.field));
          def.filters = def.filters.filter((k) => !names.includes(k.field));
          fillLeft(); fillRight();
        };
        const move = (d) => { const i = def.fields.findIndex((f) => f.name === right.value); const j = i + d; if (i < 0 || j < 0 || j >= def.fields.length) return; [def.fields[i], def.fields[j]] = [def.fields[j], def.fields[i]]; selField = def.fields[j].name; fillRight(); };
        left.ondblclick = add; right.ondblclick = remove;
        fillLeft(); fillRight();
        return [el("div", { className: "sched-fields" }, [
          el("div", {}, [el("div", { className: "ex-h", textContent: "Available fields" }), search, left]),
          el("div", { className: "sched-mid" }, [button("Add →", add), button("← Remove", remove)]),
          el("div", {}, [el("div", { className: "ex-h", textContent: "Scheduled fields (in order)" }), right]),
          el("div", { className: "sched-mid" }, [button("Move Up", () => move(-1)), button("Move Down", () => move(1)), button("Add Calculated...", () => calculated(null, () => { fillRight(); })), button("Combine Parameters...", () => combined(() => { fillRight(); })), button("Edit...", () => { const f = def.fields.find((x) => x.name === right.value); if (f && (f.kind === "formula" || f.kind === "percentage")) calculated(f, fillRight); else if (f && f.kind === "combined") combined(fillRight, f); })]),
        ])];
      }
      // a formula or percentage field
      function calculated(f, done) {
        openModal(f ? `Calculated Value: ${f.name}` : "Calculated Value", (b2, f2, close2) => {
          const v = f ? { ...f } : blank("", "formula");
          const name = el("input", { type: "text", value: v.name });
          const kind = el("span", { className: "ex-radios" }, [["formula", "Formula"], ["percentage", "Percentage"]].map(([k, l]) => { const r = el("input", { type: "radio", name: "calc-kind", checked: v.kind === k }); r.onchange = () => { v.kind = k; show(); }; return el("label", {}, [r, ` ${l}`]); }));
          const unit = el("select", {}, opts(UNITS, v.unit));
          const formula = el("input", { type: "text", value: v.formula, placeholder: "[Width] * [Height]" });
          const insert = el("select", {}, [new Option("Insert a field...", ""), ...def.fields.filter((x) => x.name !== v.name).map((x) => new Option(x.name, x.name))]);
          insert.onchange = () => { if (insert.value) { formula.value += `[${insert.value}]`; insert.value = ""; formula.focus(); } };
          const of = el("select", {}, def.fields.filter((x) => numeric(unitOf(x)) && x.name !== v.name).map((x) => new Option(x.name, x.name, false, x.name === v.of)));
          const m2 = el("div", { className: "modal-msg" });
          const rowF = el("div", {}, [el("label", { className: "new-row" }, [el("span", { textContent: "Type" }), unit]), el("label", { className: "new-row" }, [el("span", { textContent: "Formula" }), formula]), el("label", { className: "new-row" }, [el("span", { textContent: "" }), insert])]);
          const rowP = el("label", { className: "new-row" }, [el("span", { textContent: "Of" }), of]);
          const show = () => { rowF.hidden = v.kind !== "formula"; rowP.hidden = v.kind !== "percentage"; };
          b2.append(el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]), el("div", { className: "new-row" }, [el("span", { textContent: "" }), kind]), rowF, rowP, el("div", { className: "modal-note", textContent: "Lengths are in metres inside a formula, so a length times a length is an area in m²." }), m2);
          show();
          f2.append(button("Cancel", close2), button("OK", () => {
            const nm = name.value.trim();
            if (!nm) { m2.textContent = "It needs a name."; return; }
            if (def.fields.some((x) => x.name === nm && x !== f)) { m2.textContent = `${nm} is already a field.`; return; }
            if (f) for (const k of [...def.sort, ...def.filters]) if (k.field === f.name) k.field = nm;
            Object.assign(v, { name: nm, unit: unit.value, formula: formula.value.trim(), of: of.value || "" });
            if (f) Object.assign(f, v); else def.fields.push(v);
            close2(); done();
          }, true));
          name.focus();
        });
      }
      // a combined field: parts with a prefix, suffix and separator each
      function combined(done, f) {
        openModal("Combine Parameters", (b2, f2, close2) => {
          const v = f ? JSON.parse(JSON.stringify(f)) : { ...blank("Combined", "combined"), parts: [] };
          const name = el("input", { type: "text", value: v.name });
          const list = el("div", { className: "sched-parts" });
          const drawParts = () => list.replaceChildren(el("div", { className: "sched-part ex-head" }, ["Field", "Prefix", "Suffix", "Separator", ""].map((x) => el("span", { textContent: x }))), ...v.parts.map((p, i) => {
            const fs = el("select", {}, def.fields.filter((x) => x.kind !== "combined").map((x) => new Option(x.name, x.name, false, x.name === p.field)));
            fs.onchange = () => { p.field = fs.value; };
            const inp = (k) => { const x = el("input", { type: "text", value: p[k] || "" }); x.oninput = () => { p[k] = x.value; }; return x; };
            return el("div", { className: "sched-part" }, [fs, inp("prefix"), inp("suffix"), inp("separator"), button("×", () => { v.parts.splice(i, 1); drawParts(); })]);
          }));
          drawParts();
          const m2 = el("div", { className: "modal-msg" });
          b2.append(el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]), list, button("Add Part", () => { const first = def.fields.find((x) => x.kind !== "combined"); if (first) { v.parts.push({ field: first.name, prefix: "", suffix: "", separator: v.parts.length ? "" : " " }); drawParts(); } }), m2);
          f2.append(button("Cancel", close2), button("OK", () => {
            const nm = name.value.trim();
            if (!nm || !v.parts.length) { m2.textContent = "It needs a name and a part."; return; }
            if (def.fields.some((x) => x.name === nm && x !== f)) { m2.textContent = `${nm} is already a field.`; return; }
            v.name = nm;
            if (f) Object.assign(f, v); else def.fields.push(v);
            close2(); done();
          }, true));
        }, { wide: true });
      }
      // Filter: up to eight rules, all of which a row must pass
      function filter() {
        const rows = [];
        const sync = () => {
          const list = [];
          rows.forEach((r) => { if (r.fs.value) list.push({ field: r.fs.value, op: r.op.value, value: r.value }); });
          def.filters = list;
        };
        for (let i = 0; i < 8; i++) {
          const fl = def.filters[i] || { field: "", op: "equal", value: "" };
          const r = { fs: el("select", {}, fieldOpts(fl.field, true)), op: el("select", {}, opts(OPS, fl.op)), value: fl.value };
          const v = valueBox(fl.op, fl.value, (x) => { r.value = x; sync(); });
          r.box = v.box;
          r.fs.onchange = sync;
          r.op.onchange = () => { v.redraw(r.op.value); sync(); };
          rows.push(r);
        }
        const out = [el("div", { className: "modal-note", style: "margin:0 0 6px", textContent: "A row is listed when it passes every rule. Numbers compare in the field's units (mm, m²)." }), ...rows.map((r, i) => el("div", { className: "sched-rule" }, [el("span", { textContent: i ? "And:" : "Filter by:" }), r.fs, r.op, r.box]))];
        if (def.category === "Sheets" || def.category === "Views") return out;
        const bySheet = el("input", { type: "checkbox", checked: !!def.filter_by_sheet });
        bySheet.onchange = () => { def.filter_by_sheet = bySheet.checked; };
        return [...out, el("div", { className: "sched-rule" }, [el("span"), el("label", {}, [bySheet, " Filter by sheet"])]),
          el("div", { className: "modal-note", textContent: "On a sheet, only the elements the sheet's views show are listed." })];
      }
      // Conditional Format: rules tested in order, the first a row passes colours the field's cell
      function conditional(f, done) {
        openModal(`Conditional Formatting: ${f.name}`, (b2, f2, close2) => {
          const rules = JSON.parse(JSON.stringify(f.highlights || []));
          const list = el("div", { className: "sched-parts" });
          const drawRules = () => list.replaceChildren(el("div", { className: "sched-hl ex-head" }, ["Field", "Test", "Value", "Colour", ""].map((x) => el("span", { textContent: x }))), ...rules.map((h, i) => {
            const fs = el("select", {}, fieldOpts(h.field, true, `(${f.name})`));
            fs.onchange = () => { h.field = fs.value; };
            const op = el("select", {}, opts(OPS, h.op));
            const v = valueBox(h.op, h.value, (x) => { h.value = x; });
            op.onchange = () => { h.op = op.value; v.redraw(h.op); };
            const col = el("input", { type: "color", value: hex(h.colour) });
            col.oninput = () => { h.colour = rgb(col.value); };
            return el("div", { className: "sched-hl" }, [fs, op, v.box, col, button("×", () => { rules.splice(i, 1); drawRules(); })]);
          }));
          drawRules();
          const onSheets = el("input", { type: "checkbox", checked: !!f.highlight_on_sheets });
          b2.append(list, button("Add Rule", () => { rules.push({ field: "", op: "less", value: "", colour: [255, 205, 205] }); drawRules(); }),
            el("div", { className: "modal-note", textContent: "Rules are tested in order; the first a row passes colours the cell. Numbers compare in the field's units." }),
            el("div", { className: "new-row" }, [el("span"), el("label", {}, [onSheets, " Show conditional format on sheets"])]));
          f2.append(button("Cancel", close2), button("OK", () => { f.highlights = rules; f.highlight_on_sheets = onSheets.checked; close2(); done(); }, true));
        }, { wide: true });
      }
      // Sorting/Grouping: up to four keys, the grand totals and itemising
      function sorting() {
        const out = [];
        for (let i = 0; i < 4; i++) {
          const k = def.sort[i] || { field: "", descending: false, header: false, footer: false, blank_line: false };
          const fs = el("select", {}, fieldOpts(k.field, true));
          const dir = el("span", { className: "ex-radios" }, [[false, "Ascending"], [true, "Descending"]].map(([d, l]) => { const r = el("input", { type: "radio", name: `sort-${i}`, checked: !!k.descending === d }); r.onchange = () => { k.descending = d; }; return el("label", {}, [r, ` ${l}`]); }));
          const chk = (key, l) => { const c = el("input", { type: "checkbox", checked: !!k[key] }); c.onchange = () => { k[key] = c.checked; }; return el("label", {}, [c, ` ${l}`]); };
          fs.onchange = () => {
            k.field = fs.value;
            const list = def.sort.slice();
            list[i] = k;
            def.sort = list.filter((x) => x && x.field);
          };
          if (def.sort[i]) def.sort[i] = k;
          out.push(el("div", { className: "sched-rule" }, [el("span", { textContent: i ? "Then by:" : "Sort by:" }), fs, dir]), el("div", { className: "sched-rule sub" }, [el("span"), chk("header", "Header"), chk("footer", "Footer (title, count and totals)"), chk("blank_line", "Blank line")]));
        }
        const gt = el("input", { type: "checkbox", checked: def.grand_totals }), title = el("input", { type: "text", value: def.grand_title });
        gt.onchange = () => { def.grand_totals = gt.checked; }; title.oninput = () => { def.grand_title = title.value; };
        const it = el("input", { type: "checkbox", checked: def.itemize });
        it.onchange = () => { def.itemize = it.checked; };
        out.push(el("div", { className: "sched-rule" }, [el("span"), el("label", {}, [gt, " Grand totals"]), title]), el("div", { className: "sched-rule" }, [el("span"), el("label", {}, [it, " Itemize every instance"])]));
        return out;
      }
      // Formatting: each field's heading, alignment, hidden and totals
      function formatting() {
        const list = el("select", { size: 14, className: "sched-list" }, def.fields.map((f, i) => new Option(f.name, i, false, i === 0)));
        const form = el("div", { className: "sched-format" });
        const show = () => {
          const f = def.fields[Number(list.value)];
          if (!f) { form.replaceChildren(el("div", { className: "modal-note", textContent: "No fields yet." })); return; }
          const heading = el("input", { type: "text", value: f.heading, placeholder: f.name });
          heading.oninput = () => { f.heading = heading.value; };
          const align = el("select", {}, opts([["left", "Left"], ["centre", "Centre"], ["right", "Right"]], f.align));
          align.onchange = () => { f.align = align.value; };
          const hidden = el("input", { type: "checkbox", checked: f.hidden });
          hidden.onchange = () => { f.hidden = hidden.checked; };
          const totals = el("select", {}, opts(TOTALS, f.totals));
          totals.onchange = () => { f.totals = totals.value; };
          totals.disabled = !numeric(unitOf(f)) || f.kind === "count";
          const n = (f.highlights || []).length;
          const cond = button(n ? `Conditional Format... (${n})` : "Conditional Format...", () => conditional(f, show));
          form.replaceChildren(
            el("label", { className: "new-row" }, [el("span", { textContent: "Heading" }), heading]),
            el("label", { className: "new-row" }, [el("span", { textContent: "Alignment" }), align]),
            el("label", { className: "new-row" }, [el("span", { textContent: "Hidden field" }), hidden]),
            el("label", { className: "new-row" }, [el("span", { textContent: "Totals" }), totals]),
            el("div", { className: "new-row" }, [el("span", { textContent: "" }), cond]),
            el("div", { className: "modal-note", textContent: f.kind === "count" ? "Count always totals." : `Measures: ${(UNITS.find((u) => u[0] === unitOf(f)) || [0, "Text"])[1]}` }),
          );
        };
        list.onchange = show;
        show();
        return [el("div", { className: "sched-fields two" }, [list, form])];
      }
      function appearance() {
        const chk = (key, l) => { const c = el("input", { type: "checkbox", checked: !!def[key] }); c.onchange = () => { def[key] = c.checked; }; return el("label", {}, [c, ` ${l}`]); };
        return [el("div", { className: "sched-rule" }, [el("span"), chk("show_title", "Show title")]), el("div", { className: "sched-rule" }, [el("span"), chk("show_headers", "Show headers")])];
      }
      body.append(bar, pane, msg);
      draw();
      foot.append(button("Cancel", close), button("OK", async () => {
        try { await Backend.call("set_schedule", { id, def }); close(); await refresh(); } catch (e) { msg.textContent = String(e.message || e); }
      }, true));
    }, { wide: true });
  }

  return { render, edit, newSchedule, text, get table() { return table; } };
})();
