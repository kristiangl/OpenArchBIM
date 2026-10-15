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
    const materials = std.materials.map((m) => m.material.name);
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
          tr.append(h("td", {}, select(materials, st.material || "", (v) => setField(cat, sub, "material", v || null), { blank: "" })));
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

  // ------------------------------------------------------------------ Materials
  async function openMaterials(selectName) {
    let std = await run("standards", {});
    if (!std) return;
    let current = selectName || (std.materials[0] && std.materials[0].material.name);
    let tab = "graphics";

    openModal("Materials", (body, foot, close) => {
      const msg = h("div", { className: "modal-msg" });
      const list = h("div", { className: "mat-list" });
      const pane = h("div", { className: "mat-pane" });
      const search = h("input", { placeholder: "Search" });
      const left = h("div", { className: "mat-left" }, search, list);
      body.append(h("div", { className: "mat-split" }, left, pane), msg);
      const find = () => std.materials.find((m) => m.material.name === current);
      const fills = () => std.fill_patterns.map((p) => p.pattern.name);

      async function patch(patchObj) {
        const m = find();
        const err = await Backend.call("patch_style_item", { id: m.id, patch: patchObj }).then(() => null, errText);
        msg.textContent = err || "";
        std = await Backend.call("standards");
        if (!err && patchObj.name) current = patchObj.name;
        await refresh();
        render();
      }

      const patternRow = (label, key) => {
        const g = find().material.graphics[key];
        return h("tr", {}, h("td", { textContent: label }),
          h("td", {}, select(fills(), g.pattern || "", (v) => patch({ graphics: { [key]: { pattern: v || null } } }), { blank: "<None>" })),
          h("td", {}, colorInput(g.color || [0, 0, 0], (c) => patch({ graphics: { [key]: { color: c } } }))));
      };

      function renderPane() {
        pane.replaceChildren();
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
        list.replaceChildren(...std.materials
          .filter((m) => m.material.name.toLowerCase().includes(q))
          .sort((a, b) => a.material.name.localeCompare(b.material.name))
          .map((m) => {
            const b = h("button", { className: "mat-item" + (m.material.name === current ? " active" : "") });
            b.append(h("span", { className: "mat-chip" }), m.material.name);
            b.firstChild.style.background = toHex(m.material.graphics.shading_color);
            b.onclick = () => { current = m.material.name; render(); };
            return b;
          }));
      }
      search.oninput = renderList;
      function render() { renderList(); renderPane(); }

      const dup = h("button", { textContent: "Duplicate" });
      dup.onclick = async () => {
        const m = find();
        if (!m) return;
        let n = `${m.material.name} copy`;
        while (std.materials.some((x) => x.material.name === n)) n += " 2";
        const r = await Backend.call("add_style_item", { copy_of: m.id, name: n }).then((x) => x, (e) => { msg.textContent = errText(e); return null; });
        if (r) { std = await Backend.call("standards"); current = n; msg.textContent = ""; render(); }
      };
      const del = h("button", { textContent: "Delete" });
      del.onclick = async () => {
        const m = find();
        if (!m) return;
        const err = await Backend.call("delete_style_item", { id: m.id }).then(() => null, errText);
        msg.textContent = err || "";
        if (!err) { std = await Backend.call("standards"); current = std.materials[0] && std.materials[0].material.name; render(); }
      };
      left.append(h("div", { className: "row" }, dup, del));
      foot.append(button("Close", close, true));
      render();
    }, { wide: true });
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
  async function openPatterns(kind) {
    let std = await run("standards", {});
    if (!std) return;
    const isLine = kind === "line";
    const key = isLine ? "line_patterns" : "fill_patterns";
    let current = std[key][0] && std[key][0].pattern.name;
    openModal(isLine ? "Line Patterns" : "Fill Patterns", (body, foot, close) => {
      const msg = h("div", { className: "modal-msg" });
      const list = h("div", { className: "mat-list" });
      const pane = h("div", { className: "mat-pane" });
      const left = h("div", { className: "mat-left" }, list);
      body.append(h("div", { className: "mat-split" }, left, pane), msg);
      const find = () => std[key].find((p) => p.pattern.name === current);

      async function patch(patchObj) {
        const p = find();
        const err = await Backend.call("patch_style_item", { id: p.id, patch: patchObj }).then(() => null, errText);
        msg.textContent = err || "";
        std = await Backend.call("standards");
        if (!err && patchObj.name) current = patchObj.name;
        await refresh();
        render();
      }

      function renderPane() {
        pane.replaceChildren();
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
          const typeSel = select([new Option("Drafting (fixed on paper)", "drafting"), new Option("Model (fixed in the model)", "model")], pat.pattern_type, (v) => patch({ pattern_type: v }));
          typeSel.disabled = ro;
          const solid = h("input", { type: "checkbox", checked: pat.solid, disabled: ro });
          solid.onchange = () => patch({ solid: solid.checked });
          pane.append(h("label", {}, "Type ", typeSel), h("label", {}, solid, " Solid fill"));
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
        list.replaceChildren(...std[key].map((p) => {
          const b = h("button", { className: "mat-item" + (p.pattern.name === current ? " active" : ""), textContent: p.pattern.name });
          b.onclick = () => { current = p.pattern.name; render(); };
          return b;
        }));
      }
      function render() { renderList(); renderPane(); }

      const dup = h("button", { textContent: "New (copy)" });
      dup.onclick = async () => {
        const p = find();
        if (!p) return;
        let n = `${p.pattern.name} copy`;
        while (std[key].some((x) => x.pattern.name === n)) n += " 2";
        const r = await Backend.call("add_style_item", { copy_of: p.id, name: n }).then((x) => x, (e) => { msg.textContent = errText(e); return null; });
        if (r) { std = await Backend.call("standards"); current = n; msg.textContent = ""; render(); }
      };
      const del = h("button", { textContent: "Delete" });
      del.onclick = async () => {
        const p = find();
        if (!p) return;
        const err = await Backend.call("delete_style_item", { id: p.id }).then(() => null, errText);
        msg.textContent = err || "";
        if (!err) { std = await Backend.call("standards"); current = std[key][0] && std[key][0].pattern.name; render(); }
      };
      left.append(h("div", { className: "row" }, dup, del));
      foot.append(button("Close", close, true));
      render();
    }, { wide: true });
  }

  Object.assign(window, { openObjectStyles, openMaterials, openLineWeights, openPatterns });
})();
