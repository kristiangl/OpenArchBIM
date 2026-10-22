// Graphics standards dialogs: Object Styles, Materials, Line Weights, Line
// Patterns and Fill Patterns (Manage tab). Every edit goes to the core as a
// merge patch, which validates it and records one undo step.
(() => {
  const h = (tag, props, ...kids) => {
    const e = document.createElement(tag);
    Object.assign(e, props || {});
    e.append(...kids.filter((k) => k !== null && k !== undefined));
    return e;
  };
  const toRgb = (hx) => [1, 3, 5].map((i) => parseInt(hx.slice(i, i + 2), 16));
  const toHex = (c) => "#" + c.map((x) => x.toString(16).padStart(2, "0")).join("");
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const load = () => Backend.call("standards");
  const errText = (e) => (e && e.message) || String(e);

  function select(options, value, onchange, { blank } = {}) {
    const s = h("select");
    if (blank !== undefined) s.add(new Option(blank, ""));
    for (const o of options) s.add(o instanceof Option ? o : new Option(o, o));
    s.value = value ?? "";
    s.onchange = () => onchange(s.value);
    return s;
  }

  function colorInput(rgb, onchange) {
    const i = h("input", { type: "color", value: toHex(rgb || [0, 0, 0]) });
    i.onchange = () => onchange(toRgb(i.value));
    return i;
  }

  function numberInput(value, onchange, { min, max, step, width } = {}) {
    const i = h("input", { type: "number", value, step: step ?? "any" });
    if (min !== undefined) i.min = min;
    if (max !== undefined) i.max = max;
    if (width) i.style.width = width;
    i.onchange = () => { const v = parseFloat(i.value); if (!Number.isNaN(v)) onchange(v); };
    return i;
  }

  const weights = () => Array.from({ length: 16 }, (_, i) => new Option(String(i + 1), String(i + 1)));

  // Changes in the Materials and pattern windows go into the model as they are made, inside an edit session (as in
  // Type Properties): Apply keeps them as one undo step and carries on, OK keeps them and closes, Cancel or Escape
  // undoes everything since the last Apply. Opened from another dialog (to pick something), the views are left for
  // that dialog to redraw.
  async function editSession(name, redraw) {
    let mark;
    try { mark = (await Backend.call("edit_begin", {})).mark; } catch (e) { return null; }
    return {
      async apply() { mark = (await Backend.call("edit_commit", { mark, name })).mark; if (redraw) await refresh(); },
      async cancel() { try { await Backend.call("edit_cancel", { mark }); } catch (e) { /* nothing to undo */ } if (redraw) await refresh(); },
    };
  }

  /** A field that shows a value and opens a picker when clicked (a material, a fill pattern). */
  function pickerField(text, onclick, { swatch } = {}) {
    const b = h("button", { className: "picker-field", title: "Click to choose" });
    if (swatch) { const s = h("span", { className: "mat-chip" }); s.style.background = swatch; b.append(s); }
    b.append(h("span", { textContent: text }), h("span", { className: "picker-dots", textContent: "..." }));
    b.onclick = onclick;
    return b;
  }

  /**
   * Choose a material in the Materials window (where materials can also be made and edited). Resolves to the
   * name picked with OK, or null when cancelled. `extras` are choices that are not materials, listed first
   * (<By Category>, <Air gap>).
   */
  const pickMaterial = (current, extras = []) => openMaterials(current, { pick: true, extras });

  /** Choose a fill pattern; resolves to its name, "" for <No pattern>, or null when cancelled. */
  // `only` keeps the choice to one tab ("drafting" for the regions of an annotation family)
  const pickFillPattern = (current, preferType, only) => openPatterns("fill", { pick: true, select: current, tab: only || preferType, only });

  /** Tiny canvas that shows a dash array (paper mm) as a line. */
  function linePreview(dash) {
    const c = h("canvas", { width: 160, height: 14 });
    const g = c.getContext("2d");
    g.strokeStyle = getComputedStyle(document.body).color || "#222";
    g.lineWidth = 2;
    g.setLineDash(dash.length ? dash.map((d) => Math.max(d * 4, 0.8)) : []);
    g.beginPath(); g.moveTo(2, 7); g.lineTo(158, 7); g.stroke();
    return c;
  }

  // ------------------------------------------------------------------ Object Styles
  async function openObjectStyles() {
    const std = await run("standards", {});
    if (!std) return;
    const os = clone(std.object_styles);
    const patterns = std.line_patterns.map((p) => p.pattern.name);
    const draft = {}; // merge patch of what changed
    const tabs = { model: "Model Objects", annotation: "Annotation Objects", imported: "Imported Objects" };
    let tab = "model";
    const open = new Set();

    openModal("Object Styles", (body, foot, close) => {
      const bar = h("div", { className: "modal-tabs" });
      const host = h("div", { className: "vg-host" });
      const msg = h("div", { className: "modal-msg" });
      body.append(bar, host, msg);

      // write a field into both the working copy and the patch
      const setField = (cat, sub, field, value) => {
        const t = (draft[tab] ||= {});
        const c = (t[cat] ||= {});
        const row = sub ? ((c.subcategories ||= {})[sub] ||= {}) : c;
        row[field] = value;
        const live = sub ? os[tab][cat].subcategories[sub] : os[tab][cat];
        live[field] = value;
      };

      const rowFor = (cat, sub, st, indent) => {
        const tr = h("tr");
        const name = sub || cat;
        const first = h("td");
        first.style.paddingLeft = `${6 + indent * 18}px`;
        if (!sub && Object.keys(st.subcategories).length) {
          const t = h("button", { className: "twisty", textContent: open.has(cat) ? "▾" : "▸" });
          t.onclick = () => { open.has(cat) ? open.delete(cat) : open.add(cat); render(); };
          first.append(t, " ", name);
        } else first.append(name);
        if (sub && !st.built_in) {
          const x = h("button", { className: "twisty", textContent: "✕", title: "Delete subcategory" });
          x.onclick = () => {
            ((draft[tab] ||= {})[cat] ||= {}).subcategories ||= {};
            draft[tab][cat].subcategories[sub] = null;
            delete os[tab][cat].subcategories[sub];
            render();
          };
          first.append(" ", x);
        }
        tr.append(first);
        tr.append(h("td", {}, select(weights(), String(st.projection_weight), (v) => setField(cat, sub, "projection_weight", Number(v)))));
        tr.append(h("td", {}, st.cut_weight == null
          ? h("span", { className: "muted", textContent: "-" })
          : select(weights(), String(st.cut_weight), (v) => setField(cat, sub, "cut_weight", Number(v)))));
        tr.append(h("td", {}, colorInput(st.color, (c) => setField(cat, sub, "color", c))));
        tr.append(h("td", {}, select(patterns, st.pattern, (v) => setField(cat, sub, "pattern", v))));
        if (tab === "model") {
          // the Materials window picks the material, so a new one can be made on the way
          tr.append(h("td", {}, pickerField(st.material || "<None>", async () => {
            const name = await pickMaterial(st.material || "", ["<None>"]);
            if (name == null) return;
            setField(cat, sub, "material", name === "<None>" ? null : name);
            render();
          })));
        }
        return tr;
      };

      function render() {
        bar.replaceChildren(...Object.keys(tabs).map((k) => {
          const b = h("button", { textContent: tabs[k], className: k === tab ? "active" : "" });
          b.onclick = () => { tab = k; render(); };
          return b;
        }));
        host.replaceChildren();
        const table = h("table", { className: "vg" });
        const head = table.createTHead().insertRow();
        const cols = ["Category", "Projection", "Cut", "Line Color", "Line Pattern", ...(tab === "model" ? ["Material"] : [])];
        cols.forEach((t) => head.append(h("th", { textContent: t })));
        const tb = table.createTBody();
        const cats = Object.keys(os[tab]);
        if (!cats.length) {
          const td = tb.insertRow().insertCell();
          td.colSpan = cols.length; td.className = "empty";
          td.textContent = tab === "imported" ? "Imported layers appear here once a CAD file is linked or imported." : "Nothing here";
        }
        for (const cat of cats) {
          tb.append(rowFor(cat, null, os[tab][cat], 0));
          if (open.has(cat)) {
            for (const sub of Object.keys(os[tab][cat].subcategories)) tb.append(rowFor(cat, sub, os[tab][cat].subcategories[sub], 1));
          }
        }
        host.append(table);
        if (tab !== "imported") {
          const catSel = select(cats, cats[0] || "", () => {});
          const nameIn = h("input", { placeholder: "New subcategory name" });
          const add = h("button", { textContent: "New subcategory" });
          add.onclick = () => {
            const n = nameIn.value.trim();
            if (!n) return;
            const parent = os[tab][catSel.value];
            if (parent.subcategories[n]) { msg.textContent = `${catSel.value} already has '${n}'.`; return; }
            const sub = { projection_weight: 1, cut_weight: null, color: [29, 34, 41], pattern: "Solid", material: null, subcategories: {}, built_in: false };
            parent.subcategories[n] = sub;
            (((draft[tab] ||= {})[catSel.value] ||= {}).subcategories ||= {})[n] = clone(sub);
            open.add(catSel.value);
            msg.textContent = "";
            render();
          };
          host.append(h("div", { className: "addrow" }, "Add to ", catSel, nameIn, add));
        }
      }

      const save = async (andClose) => {
        if (Object.keys(draft).length) {
          const err = await Backend.call("patch_object_styles", { patch: draft }).then(() => null, errText);
          if (err) { msg.textContent = err; return; }
          for (const k of Object.keys(draft)) delete draft[k];
          await refresh();
        }
        if (andClose) close();
      };
      foot.append(button("OK", () => save(true), true), button("Cancel", close), button("Apply", () => save(false)));
      render();
    }, { wide: true });
  }

  // ------------------------------------------------------------------ Line Styles
  // Manage > Additional Settings > Line Styles: the Lines category of Object Styles and its subcategories, the line
  // styles that detail lines and region edges are drawn with. Changes go into the model as they are made, inside an
  // edit session: OK and Apply keep them as one undo step, Cancel undoes them. New adds a style; Delete and Rename
  // work on the ticked rows (Rename on one). Built-in styles cannot be deleted or renamed; the lines drawn with a
  // deleted style take another one.
  async function openLineStyles() {
    let std = await run("standards", {});
    if (!std) return;
    const ses = await editSession("Line Styles", true);
    if (!ses) return;
    let usage = await Backend.call("line_style_usage").catch(() => ({}));
    const patterns = std.line_patterns.map((p) => p.pattern.name);
    const ticked = new Set();
    let search = "", expanded = true;
    openModal("Line Styles", (body, foot, close) => {
      const searchIn = h("input", { className: "ls-search", value: "" });
      const host = h("div", { className: "vg-host ls-host" }), msg = h("div", { className: "modal-msg" });
      const lines = () => std.object_styles.annotation.Lines || { subcategories: {} };
      const reload = async () => { std = await Backend.call("standards"); usage = await Backend.call("line_style_usage").catch(() => ({})); render(); };
      const patch = async (p) => {
        const err = await Backend.call("patch_object_styles", { patch: { annotation: { Lines: p } } }).then(() => null, errText);
        msg.textContent = err || "";
        await reload();
      };
      const call = async (m, p) => { const err = await Backend.call(m, p).then(() => null, errText); msg.textContent = err || ""; await reload(); return !err; };
      const finish = async (how) => {
        if (how === "cancel") { await ses.cancel(); close(); return; }
        await ses.apply();
        if (how === "ok") close();
      };
      body.closest(".modal-back").onEscape = () => finish("cancel");
      searchIn.oninput = () => { search = searchIn.value.trim().toLowerCase(); render(); };

      const row = (name, st, sub) => {
        const tr = h("tr", { className: sub && ticked.has(name) ? "on" : "" });
        const first = h("td");
        if (sub) {
          first.style.paddingLeft = "24px";
          const tick = h("input", { type: "checkbox", checked: ticked.has(name), title: "Tick to delete or rename" });
          tick.onchange = () => { if (tick.checked) ticked.add(name); else ticked.delete(name); render(); };
          first.append(h("label", {}, tick, name));
        } else {
          const t = h("button", { className: "twisty", textContent: expanded ? "▾" : "▸" });
          t.onclick = () => { expanded = !expanded; render(); };
          first.append(t, " ", name);
        }
        const set = (field, v) => patch(sub ? { subcategories: { [name]: { [field]: v } } } : { [field]: v });
        tr.append(first);
        tr.append(h("td", {}, select(weights(), String(st.projection_weight), (v) => set("projection_weight", Number(v)))));
        tr.append(h("td", {}, colorInput(st.color, (c) => set("color", c))));
        tr.append(h("td", {}, select(patterns, st.pattern, (v) => set("pattern", v))));
        return tr;
      };
      function render() {
        const L = lines();
        for (const n of [...ticked]) if (!L.subcategories[n]) ticked.delete(n);
        const table = h("table", { className: "vg" });
        const head = table.createTHead();
        const h1 = head.insertRow(), h2 = head.insertRow();
        h1.append(h("th", { textContent: "Category", rowSpan: 2 }), h("th", { textContent: "Line Weight" }), h("th", { textContent: "Line Color", rowSpan: 2 }), h("th", { textContent: "Line Pattern", rowSpan: 2 }));
        h2.append(h("th", { textContent: "Projection" }));
        const tb = table.createTBody();
        tb.append(row("Lines", L, false));
        if (expanded) {
          for (const [n, st] of Object.entries(L.subcategories)) {
            if (search && !n.toLowerCase().includes(search)) continue;
            tb.append(row(n, st, true));
          }
        }
        host.replaceChildren(table);
        del.disabled = !ticked.size || [...ticked].some((n) => L.subcategories[n].built_in);
        ren.disabled = ticked.size !== 1 || L.subcategories[[...ticked][0]].built_in;
      }
      // a small dialog over this one: its fields, and OK runs `ok` (which says whether it worked)
      const ask = (title, fields, ok) => openModal(title, (b2, f2, close2) => {
        b2.append(...fields.map(([label, ctl]) => h("div", { className: "new-row" }, h("span", { textContent: label }), ctl)));
        f2.append(button("Cancel", close2), button("OK", async () => { if (await ok()) close2(); }, true));
        setTimeout(() => { const i = b2.querySelector("input"); if (i) { i.focus(); i.select(); } }, 0);
      });
      const others = (except) => Object.keys(lines().subcategories).filter((n) => !except.includes(n));
      const fallback = (except) => (others(except).includes("Thin Lines") ? "Thin Lines" : others(except)[0]);
      const add = h("button", { textContent: "New" }), del = h("button", { textContent: "Delete" }), ren = h("button", { textContent: "Rename" });
      add.onclick = () => {
        const name = h("input", { value: "New line style" });
        ask("New Subcategory", [["Name", name], ["Subcategory of", h("span", { textContent: "Lines" })]], async () => {
          const n = name.value.trim();
          if (!n) { msg.textContent = "A line style needs a name."; return false; }
          if (lines().subcategories[n] || n === "<Invisible lines>") { msg.textContent = `A line style named '${n}' already exists.`; return false; }
          await patch({ subcategories: { [n]: { projection_weight: 1, cut_weight: null, color: [0, 0, 0], pattern: "Solid", material: null, subcategories: {}, built_in: false } } });
          return true;
        });
      };
      del.onclick = async () => {
        const names = [...ticked], used = names.filter((n) => usage[n]);
        const go = async (to) => { for (const n of names) if (!(await call("delete_line_style", { name: n, replace_with: to }))) return false; ticked.clear(); render(); return true; };
        if (!used.length) { await go(fallback(names)); return; }
        // lines are drawn with them: they take the style picked here
        const to = select(others(names), fallback(names), () => {});
        const count = used.reduce((t, n) => t + usage[n], 0);
        ask("Delete Line Style", [[`${count} line${count === 1 ? "" : "s"} drawn with ${used.map((n) => `'${n}'`).join(", ")} will use`, to]], () => go(to.value));
      };
      ren.onclick = () => {
        const from = [...ticked][0], to = h("input", { value: from });
        ask("Rename", [["From", h("span", { textContent: from })], ["To", to]], async () => {
          const ok = await call("rename_line_style", { from, to: to.value });
          if (ok) { ticked.clear(); ticked.add(to.value.trim()); render(); }
          return ok;
        });
      };
      const pick = (fn) => () => { const L = lines(); fn(Object.keys(L.subcategories).filter((n) => !search || n.toLowerCase().includes(search))); render(); };
      const selAll = h("button", { textContent: "Select All" }), selNone = h("button", { textContent: "Select None" }), invert = h("button", { textContent: "Invert" });
      selAll.onclick = pick((ns) => ns.forEach((n) => ticked.add(n)));
      selNone.onclick = pick(() => ticked.clear());
      invert.onclick = pick((ns) => ns.forEach((n) => (ticked.has(n) ? ticked.delete(n) : ticked.add(n))));
      body.append(
        h("label", { className: "ls-searchrow" }, "Category name search: ", searchIn),
        host,
        h("div", { className: "ls-bottom" },
          h("div", { className: "row" }, selAll, selNone, invert),
          h("fieldset", { className: "ls-modify" }, h("legend", { textContent: "Modify Subcategories" }), add, del, ren)),
        msg,
      );
      foot.append(button("OK", () => finish("ok"), true), button("Cancel", () => finish("cancel")), button("Apply", () => finish("apply")));
      render();
    }, { wide: true });
  }

  // ------------------------------------------------------------------ Materials
  // Opened on its own (Manage > Materials) it edits the project's materials. Opened to pick one (`pick`), OK also
  // hands back the material selected; `extras` are choices that are not materials, listed first.
  async function openMaterials(selectName, { pick = false, extras = [] } = {}) {
    let std = await run("standards", {});
    if (!std) return null;
    const ses = await editSession("Materials", !pick);
    if (!ses) return null;
    let current = selectName || (std.materials[0] && std.materials[0].material.name);
    let tab = "graphics";
    let done;
    const result = new Promise((r) => { done = r; });

    openModal(pick ? "Material Browser" : "Materials", (body, foot, close) => {
      const msg = h("div", { className: "modal-msg" });
      const list = h("div", { className: "mat-list" });
      const pane = h("div", { className: "mat-pane" });
      const search = h("input", { placeholder: "Search" });
      const left = h("div", { className: "mat-left" }, search, list);
      body.append(h("div", { className: "mat-split" }, left, pane), msg);
      const find = () => std.materials.find((m) => m.material.name === current);
      const finish = async (how) => {
        if (how === "cancel") { await ses.cancel(); close(); done(null); return; }
        await ses.apply();
        if (how === "ok") { close(); done(pick ? current : null); }
      };
      body.closest(".modal-back").onEscape = () => finish("cancel");

      async function patch(patchObj) {
        const m = find();
        const err = await Backend.call("patch_style_item", { id: m.id, patch: patchObj }).then(() => null, errText);
        msg.textContent = err || "";
        std = await Backend.call("standards");
        if (!err && patchObj.name) current = patchObj.name;
        render();
      }

      // a surface or cut pattern is chosen in the Fill Patterns window, on the tab it belongs to
      const patternRow = (label, key) => {
        const g = find().material.graphics[key];
        const kind = std.fill_patterns.find((p) => p.pattern.name === g.pattern);
        const prefer = kind ? kind.pattern.pattern_type : key.startsWith("surface") ? "model" : "drafting";
        return h("tr", {}, h("td", { textContent: label }),
          h("td", {}, pickerField(g.pattern || "<None>", async () => {
            const name = await pickFillPattern(g.pattern || "", prefer);
            if (name == null) return;
            std = await Backend.call("standards"); // the pattern may be new
            patch({ graphics: { [key]: { pattern: name || null } } });
          })),
          h("td", {}, colorInput(g.color || [0, 0, 0], (c) => patch({ graphics: { [key]: { color: c } } }))));
      };

      function renderPane() {
        pane.replaceChildren();
        if (extras.includes(current)) {
          pane.append(h("p", { className: "note", textContent: current === "<By Category>" ? "The layer takes the material of its category in Object Styles." : current === "<Air gap>" ? "A cavity: the layer is left empty and carries no material." : "No material." }));
          return;
        }
        const m = find();
        if (!m) { pane.append(h("p", { className: "note", textContent: "No material selected." })); return; }
        const mat = m.material;
        const nameIn = h("input", { value: mat.name });
        nameIn.onchange = () => nameIn.value.trim() && patch({ name: nameIn.value.trim() });
        pane.append(h("label", { className: "mat-name" }, "Name ", nameIn));
        const bar = h("div", { className: "modal-tabs" }, ...["identity", "graphics", "physical"].map((k) => {
          const b = h("button", { textContent: k[0].toUpperCase() + k.slice(1), className: k === tab ? "active" : "" });
          b.onclick = () => { tab = k; renderPane(); };
          return b;
        }));
        pane.append(bar);
        const t = h("table", { className: "range" });
        if (tab === "graphics") {
          const g = mat.graphics;
          t.append(h("tr", { className: "sect" }, h("td", { colSpan: 3, textContent: "Shading" })));
          t.append(h("tr", {}, h("td", { textContent: "Color" }), h("td", { colSpan: 2 }, colorInput(g.shading_color, (c) => patch({ graphics: { shading_color: c } })))));
          t.append(h("tr", {}, h("td", { textContent: "Transparency" }), h("td", { colSpan: 2 }, numberInput(g.shading_transparency, (v) => patch({ graphics: { shading_transparency: Math.max(0, Math.min(100, Math.round(v))) } }), { min: 0, max: 100, width: "70px" }))));
          t.append(h("tr", { className: "sect" }, h("td", { colSpan: 3, textContent: "Surface Pattern" })));
          t.append(patternRow("Foreground", "surface_foreground"), patternRow("Background", "surface_background"));
          t.append(h("tr", { className: "sect" }, h("td", { colSpan: 3, textContent: "Cut Pattern" })));
          t.append(patternRow("Foreground", "cut_foreground"), patternRow("Background", "cut_background"));
        } else if (tab === "identity") {
          const id = mat.identity;
          t.append(h("tr", {}, h("td", { textContent: "Class" }), h("td", {}, (() => { const i = h("input", { value: mat.class }); i.onchange = () => patch({ class: i.value }); return i; })())));
          for (const [k, label] of [["description", "Description"], ["comments", "Comments"], ["manufacturer", "Manufacturer"], ["model", "Model"], ["url", "URL"], ["keynote", "Keynote"], ["mark", "Mark"]]) {
            const i = h("input", { value: id[k] });
            i.onchange = () => patch({ identity: { [k]: i.value } });
            t.append(h("tr", {}, h("td", { textContent: label }), h("td", {}, i)));
          }
          t.append(h("tr", {}, h("td", { textContent: "Cost" }), h("td", {}, numberInput(id.cost, (v) => patch({ identity: { cost: v } }), { min: 0, width: "100px" }))));
        } else {
          const p = mat.physical;
          for (const [k, label, unit] of [["density_kg_m3", "Density", "kg/m3"], ["conductivity_w_mk", "Thermal conductivity", "W/(m.K)"], ["specific_heat_j_kgk", "Specific heat", "J/(kg.K)"]]) {
            const i = h("input", { type: "number", step: "any", value: p[k] ?? "" });
            i.onchange = () => patch({ physical: { [k]: i.value === "" ? null : parseFloat(i.value) } });
            t.append(h("tr", {}, h("td", { textContent: label }), h("td", {}, i), h("td", { className: "muted", textContent: unit })));
          }
        }
        pane.append(t);
        const used = m.used_by;
        pane.append(h("p", { className: "note", textContent: used.length ? `Used by: ${used.join(", ")}` : "Not used by anything yet." }));
      }

      function renderList() {
        const q = search.value.toLowerCase();
        const extraItems = extras.filter((x) => x.toLowerCase().includes(q)).map((x) => {
          const b = h("button", { className: "mat-item mat-extra" + (x === current ? " active" : ""), textContent: x });
          b.onclick = () => { current = x; render(); };
          b.ondblclick = () => pick && finish("ok");
          return b;
        });
        list.replaceChildren(...extraItems, ...std.materials
          .filter((m) => m.material.name.toLowerCase().includes(q))
          .sort((a, b) => a.material.name.localeCompare(b.material.name))
          .map((m) => {
            const b = h("button", { className: "mat-item" + (m.material.name === current ? " active" : "") });
            b.append(h("span", { className: "mat-chip" }), m.material.name);
            b.firstChild.style.background = toHex(m.material.graphics.shading_color);
            b.onclick = () => { current = m.material.name; render(); };
            b.ondblclick = () => pick && finish("ok");
            return b;
          }));
      }
      search.oninput = renderList;
      function render() { renderList(); renderPane(); }

      const unique = (base) => { let n = base, k = 2; while (std.materials.some((x) => x.material.name === n)) n = `${base} ${k++}`; return n; };
      const made = async (r, n) => { if (r) { std = await Backend.call("standards"); current = n; msg.textContent = ""; render(); } };
      const add = h("button", { textContent: "New", title: "Create a new material" });
      add.onclick = async () => {
        const n = unique("New Material");
        made(await Backend.call("add_style_item", { item: { kind: "material", name: n, class: "Generic" } }).then((x) => x, (e) => { msg.textContent = errText(e); return null; }), n);
      };
      const dup = h("button", { textContent: "Duplicate" });
      dup.onclick = async () => {
        const m = find();
        if (!m) return;
        const n = unique(`${m.material.name} copy`);
        made(await Backend.call("add_style_item", { copy_of: m.id, name: n }).then((x) => x, (e) => { msg.textContent = errText(e); return null; }), n);
      };
      const del = h("button", { textContent: "Delete" });
      del.onclick = async () => {
        const m = find();
        if (!m) return;
        const err = await Backend.call("delete_style_item", { id: m.id }).then(() => null, errText);
        msg.textContent = err || "";
        if (!err) { std = await Backend.call("standards"); current = std.materials[0] && std.materials[0].material.name; render(); }
      };
      left.append(h("div", { className: "row" }, add, dup, del));
      foot.append(button("OK", () => finish("ok"), true), button("Cancel", () => finish("cancel")), button("Apply", () => finish("apply")));
      render();
    }, { wide: true });
    return result;
  }

  // ------------------------------------------------------------------ Line Weights
  async function openLineWeights() {
    const std = await run("standards", {});
    if (!std) return;
    const lw = clone(std.line_weights);
    let tab = "model";
    openModal("Line Weights", (body, foot, close) => {
      const bar = h("div", { className: "modal-tabs" });
      const host = h("div", { className: "vg-host" });
      const msg = h("div", { className: "modal-msg" });
      body.append(bar, host, h("p", { className: "note", textContent: "Widths are paper millimetres. Model lines use the column for the view's scale; other scales use the nearest column." }), msg);
      const titles = { model: "Model Line Weights", perspective: "Perspective Line Weights", annotation: "Annotation Line Weights" };
      function render() {
        bar.replaceChildren(...Object.keys(titles).map((k) => {
          const b = h("button", { textContent: titles[k], className: k === tab ? "active" : "" });
          b.onclick = () => { tab = k; render(); };
          return b;
        }));
        const t = h("table", { className: "vg" });
        const head = t.createTHead().insertRow();
        head.append(h("th", { textContent: "Weight" }));
        if (tab === "model") lw.scales.forEach((s) => head.append(h("th", { textContent: `1:${s}` })));
        else head.append(h("th", { textContent: "Width (mm)" }));
        const tb = t.createTBody();
        for (let i = 0; i < 16; i++) {
          const tr = tb.insertRow();
          tr.insertCell().textContent = String(i + 1);
          if (tab === "model") lw.model[i].forEach((v, j) => tr.insertCell().append(numberInput(v, (x) => { lw.model[i][j] = x; }, { min: 0.01, max: 20, step: 0.01, width: "62px" })));
          else tr.insertCell().append(numberInput(lw[tab][i], (x) => { lw[tab][i] = x; }, { min: 0.01, max: 20, step: 0.01, width: "70px" }));
        }
        host.replaceChildren(t);
      }
      const save = async () => {
        const err = await Backend.call("patch_line_weights", { patch: { model: lw.model, perspective: lw.perspective, annotation: lw.annotation } }).then(() => null, errText);
        if (err) { msg.textContent = err; return; }
        await refresh();
        close();
      };
      foot.append(button("OK", save, true), button("Cancel", close));
      render();
    }, { wide: true });
  }

  // ------------------------------------------------------------------ Line and fill patterns
  // Fill patterns sit on two tabs, Drafting (fixed on paper, for cut patterns) and Model (fixed in the model, as real
  // bricks or tiles are, for surfaces); Solid fill is on both. A new pattern is of the tab it is made on. Opened to
  // pick a fill pattern (`pick`), OK hands back the one selected ("" for <No pattern>).
  const NO_PATTERN = "<No pattern>";
  async function openPatterns(kind, { pick = false, select: chosen, tab: firstTab, only } = {}) {
    let std = await run("standards", {});
    if (!std) return null;
    const isLine = kind === "line";
    const key = isLine ? "line_patterns" : "fill_patterns";
    const ses = await editSession(isLine ? "Line Patterns" : "Fill Patterns", !pick);
    if (!ses) return null;
    const known = std[key].find((p) => p.pattern.name === chosen);
    let tab = isLine ? null : only || (known && !known.pattern.solid ? known.pattern.pattern_type : firstTab || "drafting");
    const onTab = (p) => isLine || p.pattern.solid || p.pattern.pattern_type === tab;
    let current = pick && chosen === "" ? NO_PATTERN : known ? chosen : (std[key].find(onTab) || {}).pattern?.name;
    let done;
    const result = new Promise((r) => { done = r; });
    openModal(isLine ? "Line Patterns" : "Fill Patterns", (body, foot, close) => {
      const msg = h("div", { className: "modal-msg" });
      const list = h("div", { className: "mat-list" });
      const pane = h("div", { className: "mat-pane" });
      const tabs = h("div", { className: "modal-tabs" });
      const left = h("div", { className: "mat-left" }, ...(isLine ? [] : [tabs]), list);
      body.append(h("div", { className: "mat-split" }, left, pane), msg);
      const find = () => std[key].find((p) => p.pattern.name === current);
      const finish = async (how) => {
        if (how === "cancel") { await ses.cancel(); close(); done(null); return; }
        await ses.apply();
        if (how === "ok") { close(); done(pick ? (current === NO_PATTERN ? "" : current) : null); }
      };
      body.closest(".modal-back").onEscape = () => finish("cancel");

      async function patch(patchObj) {
        const p = find();
        const err = await Backend.call("patch_style_item", { id: p.id, patch: patchObj }).then(() => null, errText);
        msg.textContent = err || "";
        std = await Backend.call("standards");
        if (!err && patchObj.name) current = patchObj.name;
        render();
      }

      function renderPane() {
        pane.replaceChildren();
        if (current === NO_PATTERN) { pane.append(h("p", { className: "note", textContent: "No pattern: the surface or cut is left unfilled." })); return; }
        const p = find();
        if (!p) return;
        const pat = p.pattern;
        const ro = pat.built_in;
        const nameIn = h("input", { value: pat.name, disabled: ro });
        nameIn.onchange = () => nameIn.value.trim() && patch({ name: nameIn.value.trim() });
        pane.append(h("label", { className: "mat-name" }, "Name ", nameIn));
        if (ro) pane.append(h("p", { className: "note", textContent: "Built-in. Duplicate it to make changes." }));
        if (isLine) {
          pane.append(linePreview(p.dash));
          const t = h("table", { className: "range" });
          pat.segments.forEach((s, i) => {
            const tr = h("tr");
            tr.append(h("td", {}, select(["dash", "space", "dot"], s.kind, (v) => { const segs = clone(pat.segments); segs[i].kind = v; patch({ segments: segs }); })));
            tr.lastChild.firstChild.disabled = ro;
            const len = numberInput(s.length_mm, (v) => { const segs = clone(pat.segments); segs[i].length_mm = v; patch({ segments: segs }); }, { min: 0.1, step: 0.1, width: "70px" });
            len.disabled = ro || s.kind === "dot";
            tr.append(h("td", {}, len, " mm"));
            if (!ro) {
              const x = h("button", { textContent: "✕" });
              x.onclick = () => { const segs = clone(pat.segments); segs.splice(i, 1); patch({ segments: segs }); };
              tr.append(h("td", {}, x));
            }
            t.append(tr);
          });
          pane.append(t);
          if (!ro) {
            const add = h("button", { textContent: "Add segment" });
            add.onclick = () => { const segs = clone(pat.segments); segs.push({ kind: segs.length % 2 ? "space" : "dash", length_mm: 2 }); patch({ segments: segs }); };
            pane.append(add);
          }
        } else {
          const solid = h("input", { type: "checkbox", checked: pat.solid, disabled: ro });
          solid.onchange = () => patch({ solid: solid.checked });
          pane.append(h("p", { className: "note", textContent: pat.solid ? "Solid fill: used on both tabs." : pat.pattern_type === "model" ? "Model pattern: fixed in the model, as real bricks or tiles are." : "Drafting pattern: fixed on paper, the same at every scale." }), h("label", {}, solid, " Solid fill"));
          const t = h("table", { className: "vg" });
          const head = t.createTHead().insertRow();
          ["Angle", "Origin X", "Origin Y", "Shift", "Spacing", "Dashes"].forEach((c) => head.append(h("th", { textContent: c })));
          pat.lines.forEach((l, i) => {
            const tr = t.insertRow();
            const edit = (field, v) => { const ls = clone(pat.lines); ls[i][field] = v; patch({ lines: ls }); };
            const cell = (el) => { el.disabled = ro; tr.insertCell().append(el); };
            cell(numberInput(l.angle_deg, (v) => edit("angle_deg", v), { width: "60px" }));
            cell(numberInput(l.origin[0], (v) => { const ls = clone(pat.lines); ls[i].origin[0] = v; patch({ lines: ls }); }, { width: "60px" }));
            cell(numberInput(l.origin[1], (v) => { const ls = clone(pat.lines); ls[i].origin[1] = v; patch({ lines: ls }); }, { width: "60px" }));
            cell(numberInput(l.shift, (v) => edit("shift", v), { width: "60px" }));
            cell(numberInput(l.spacing, (v) => edit("spacing", v), { min: 0.05, width: "60px" }));
            const d = h("input", { value: l.dashes.join(", "), title: "dash, space, dash, space ..." });
            d.onchange = () => edit("dashes", d.value.split(",").map((x) => parseFloat(x)).filter((x) => !Number.isNaN(x)));
            cell(d);
          });
          pane.append(t);
          if (!ro) {
            const add = h("button", { textContent: "Add line" });
            add.onclick = () => { const ls = clone(pat.lines); ls.push({ angle_deg: 45, origin: [0, 0], shift: 0, spacing: 3, dashes: [] }); patch({ lines: ls }); };
            pane.append(add);
          }
          pane.append(h("p", { className: "note", textContent: `Lengths are ${pat.pattern_type === "model" ? "model" : "paper"} millimetres.` }));
        }
        const used = p.used_by;
        pane.append(h("p", { className: "note", textContent: used.length ? `Used by: ${used.join(", ")}` : "Not used by anything yet." }));
      }

      function renderList() {
        if (!isLine) {
          tabs.replaceChildren(...[["drafting", "Drafting"], ["model", "Model"]].map(([k, label]) => {
            const b = h("button", { textContent: label, className: k === tab ? "active" : "", disabled: !!only && k !== only, title: only && k !== only ? `Only ${only} patterns can be used here` : "" });
            b.onclick = () => {
              tab = k;
              if (current !== NO_PATTERN && !(find() && onTab(find()))) current = (std[key].find(onTab) || {}).pattern?.name;
              render();
            };
            return b;
          }));
        }
        const item = (name, onclick) => {
          const b = h("button", { className: "mat-item" + (name === current ? " active" : ""), textContent: name });
          b.onclick = onclick;
          b.ondblclick = () => pick && finish("ok");
          return b;
        };
        list.replaceChildren(
          ...(pick && !isLine ? [item(NO_PATTERN, () => { current = NO_PATTERN; render(); })] : []),
          ...std[key].filter(onTab).map((p) => item(p.pattern.name, () => { current = p.pattern.name; render(); })),
        );
      }
      function render() { renderList(); renderPane(); }

      const unique = (base) => { let n = base, k = 2; while (std[key].some((x) => x.pattern.name === n)) n = `${base} ${k++}`; return n; };
      const made = async (r, n) => { if (r) { std = await Backend.call("standards"); current = n; msg.textContent = ""; render(); } };
      const add = h("button", { textContent: "New", title: isLine ? "A new line pattern" : "A new pattern on this tab" });
      add.onclick = async () => {
        const n = unique(isLine ? "New Line Pattern" : tab === "model" ? "New Model Pattern" : "New Drafting Pattern");
        const item = isLine
          ? { kind: "line_pattern", name: n, segments: [{ kind: "dash", length_mm: 3 }, { kind: "space", length_mm: 1.5 }] }
          : { kind: "fill_pattern", name: n, pattern_type: tab, lines: [{ angle_deg: 45, origin: [0, 0], shift: 0, spacing: tab === "model" ? 100 : 3, dashes: [] }] };
        made(await Backend.call("add_style_item", { item }).then((x) => x, (e) => { msg.textContent = errText(e); return null; }), n);
      };
      const dup = h("button", { textContent: "Duplicate" });
      dup.onclick = async () => {
        const p = find();
        if (!p) return;
        const n = unique(`${p.pattern.name} copy`);
        made(await Backend.call("add_style_item", { copy_of: p.id, name: n }).then((x) => x, (e) => { msg.textContent = errText(e); return null; }), n);
      };
      const del = h("button", { textContent: "Delete" });
      del.onclick = async () => {
        const p = find();
        if (!p) return;
        const err = await Backend.call("delete_style_item", { id: p.id }).then(() => null, errText);
        msg.textContent = err || "";
        if (!err) { std = await Backend.call("standards"); current = (std[key].find(onTab) || {}).pattern?.name; render(); }
      };
      left.append(h("div", { className: "row" }, add, dup, del));
      foot.append(button("OK", () => finish("ok"), true), button("Cancel", () => finish("cancel")), button("Apply", () => finish("apply")));
      render();
    }, { wide: true });
    return result;
  }

  Object.assign(window, { openObjectStyles, openLineStyles, openMaterials, openLineWeights, openPatterns, pickMaterial, pickFillPattern, pickerField });
})();
