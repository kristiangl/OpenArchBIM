// View filters: View > Filters. One box with two tabs. "In This View" is the ordered list of filters the view
// applies, each with Enable, Visibility and its graphic overrides (the filter higher in the list wins a property
// two of them set). "All Filters" makes and edits the project's filters: a rule filter (categories and a tree of
// "all of" and "any of" groups over parameter rules) or a selection filter (the elements selected when it is made).
// The core works out what each filter matches (viewfilter.rs). Private notes: view-templates-filters.md.
const Filters = (() => {
  const clone = (o) => JSON.parse(JSON.stringify(o));
  const blankRule = () => ({ node: "rule", field: "", op: "equal", value: "", not: false, match_case: false });
  const NO_VALUE = new Set(["has_value", "has_no_value"]);

  async function open(startTab = "view") {
    const v = curView();
    const std = await run("standards", {});
    const linePats = std ? std.line_patterns.map((p) => p.pattern.name) : ["Solid"];
    const fillPats = std ? std.fill_patterns.map((p) => p.pattern.name) : ["Solid fill"];
    let all = await run("view_filters", {});
    if (!all) return;
    const isView = v && ["floor_plan", "ceiling_plan", "three_d", "elevation", "section", "area_plan"].includes(v.view_kind);
    const inView = isView ? await run("view_filter_list", { view_id: v.id }) : null;
    let list = inView ? clone(inView.filters) : [];
    let tab = isView ? startTab : "all";
    let picked = all.filters[0]?.id ?? null; // the filter being edited in All Filters
    let draft = null, fields = { fields: [], ops: [] };

    openModal("Filters", (body, foot, close) => {
      const tabs = el("div", { className: "modal-tabs" });
      const host = el("div", {});
      host.style.cssText = "min-width:820px;min-height:340px";
      const msg = el("div", { className: "modal-msg" });
      body.append(tabs, host, msg);
      const say = (t) => { msg.textContent = t || ""; };
      const nameOf = (id) => all.filters.find((f) => f.id === id)?.name || "(deleted filter)";

      // ---- a small box under a button, for a line or a fill override ----
      function popover(anchor, nodes, onOk) {
        document.querySelectorAll(".popover").forEach((p) => p.remove());
        const pop = el("div", { className: "popover" });
        const r = anchor.getBoundingClientRect();
        pop.style.left = `${Math.min(r.left, window.innerWidth - 260)}px`;
        pop.style.top = `${r.bottom + 2}px`;
        pop.append(...nodes, button("OK", () => { onOk(); pop.remove(); render(); }, true), button("Cancel", () => pop.remove()));
        document.body.append(pop);
      }
      const field = (label, node) => { const l = el("label", { textContent: label }); l.append(node); return l; };
      const lineButton = (ov, key, locked) => {
        const o = ov[key] || {};
        const b = el("button", { className: "swatch", disabled: locked, title: "Click to override" });
        b.innerHTML = `<span class="sw-line" style="${o.color ? `background:${rgb2hex(o.color)}` : ""}"></span><span>${o.weight || ""}</span>`;
        b.onclick = () => {
          const cur = ov[key] || (ov[key] = {});
          const weight = el("select", {}, [new Option("No override", ""), ...Array.from({ length: 16 }, (_, i) => new Option(String(i + 1), i + 1, false, cur.weight === i + 1))]);
          const use = el("input", { type: "checkbox", checked: !!cur.color });
          const color = ColorPick.field(cur.color ? rgb2hex(cur.color) : "#000000");
          color.onchange = () => { use.checked = true; };
          const pattern = el("select", {}, ["", ...linePats].map((p) => new Option(p || "No override", p, false, (cur.pattern || "") === p)));
          popover(b, [field("Weight", weight), field("Override colour", use), field("Colour", color), field("Pattern", pattern)], () => {
            cur.weight = weight.value ? Number(weight.value) : null; cur.color = use.checked ? hex2rgb(color.value) : null; cur.pattern = pattern.value || null;
          });
        };
        return b;
      };
      const fillButton = (ov, key, locked) => {
        const o = ov[key] || {};
        const b = el("button", { className: "swatch", disabled: locked, title: "Click to override" });
        b.innerHTML = `<span class="sw-fill" style="${o.color ? `background:${rgb2hex(o.color)}` : ""}"></span><span>${o.pattern || ""}</span>`;
        b.onclick = () => {
          const cur = ov[key] || (ov[key] = { visible: true });
          const vis = el("input", { type: "checkbox", checked: cur.visible !== false });
          const use = el("input", { type: "checkbox", checked: !!cur.color });
          const color = ColorPick.field(cur.color ? rgb2hex(cur.color) : "#969ba2");
          color.onchange = () => { use.checked = true; };
          const pattern = el("select", {}, ["", ...fillPats].map((p) => new Option(p || "No override", p, false, (cur.pattern || "") === p)));
          popover(b, [field("Visible", vis), field("Override colour", use), field("Colour", color), field("Pattern", pattern)], () => {
            cur.visible = vis.checked; cur.color = use.checked ? hex2rgb(color.value) : null; cur.pattern = pattern.value || null;
          });
        };
        return b;
      };

      // ---- In This View ----
      function renderView() {
        const locked = !!inView.locked;
        if (locked) host.append(el("p", { className: "note", textContent: "The filters of this view are controlled by its view template." }));
        const table = el("table", { className: "vg" });
        const head = table.createTHead().insertRow();
        for (const t of ["Enable", "Visibility", "Filter", "Projection/Surface: Lines", "Patterns", "Transparency", "Cut: Lines", "Patterns", "Halftone", ""]) head.append(el("th", { textContent: t }));
        const tb = table.createTBody();
        if (!list.length) { const td = tb.insertRow().insertCell(); td.colSpan = 10; td.className = "empty"; td.textContent = "No filters are applied to this view. Add one below."; }
        list.forEach((a, i) => {
          const ov = (a.overrides ||= {});
          const tr = tb.insertRow();
          const tick = (key, on) => { const c = el("input", { type: "checkbox", checked: on, disabled: locked }); c.onchange = () => { a[key] = c.checked; }; return c; };
          tr.insertCell().append(tick("enabled", a.enabled !== false));
          tr.insertCell().append(tick("visible", a.visible !== false));
          tr.insertCell().append(el("span", { textContent: nameOf(a.filter) }));
          tr.insertCell().append(lineButton(ov, "projection_lines", locked));
          tr.insertCell().append(fillButton(ov, "surface_foreground", locked));
          const tp = el("input", { type: "number", min: 0, max: 100, value: ov.transparency || 0, disabled: locked });
          tp.style.width = "56px";
          tp.onchange = () => { ov.transparency = Math.max(0, Math.min(100, parseInt(tp.value) || 0)); };
          tr.insertCell().append(tp);
          tr.insertCell().append(lineButton(ov, "cut_lines", locked));
          tr.insertCell().append(fillButton(ov, "cut_foreground", locked));
          const ht = el("input", { type: "checkbox", checked: !!ov.halftone, disabled: locked });
          ht.onchange = () => { ov.halftone = ht.checked; };
          tr.insertCell().append(ht);
          const move = (d) => () => { const j = i + d; if (j < 0 || j >= list.length) return; [list[i], list[j]] = [list[j], list[i]]; render(); };
          const acts = tr.insertCell();
          acts.style.whiteSpace = "nowrap";
          for (const [label, title, fn] of [["▲", "Move up: it wins over those below", move(-1)], ["▼", "Move down", move(1)], ["✕", "Remove from this view", () => { list.splice(i, 1); render(); }]]) {
            const b = button(label, fn); b.title = title; b.disabled = locked; acts.append(b);
          }
        });
        host.append(table);
        const free = all.filters.filter((f) => !list.some((a) => a.filter === f.id));
        const pick = el("select", { disabled: locked || !free.length }, free.length ? free.map((f) => new Option(f.name, f.id)) : [new Option(all.filters.length ? "Every filter is already in the list" : "No filters yet: make one in All Filters", "")]);
        const add = button("Add", () => { if (pick.value) { list.push({ filter: Number(pick.value), visible: true, enabled: true, overrides: {} }); render(); } });
        add.disabled = locked || !free.length;
        const bar = el("div", {}, [el("span", { textContent: "Add a filter: " }), pick, add]);
        bar.style.cssText = "display:flex;gap:6px;align-items:center;margin-top:8px";
        host.append(bar, el("p", { className: "note", textContent: "Where two filters set the same thing for an element, the one higher in the list wins. Visibility off hides what the filter matches; Enable off skips the filter and keeps its settings." }));
      }
      async function saveView() {
        if (!isView || inView.locked) return true;
        try { await Backend.call("set_view_filter_list", { view_id: v.id, filters: list }); } catch (e) { say(String(e.message || e)); return false; }
        await refresh(true);
        return true;
      }

      // ---- All Filters ----
      async function loadFields() {
        const cats = draft && draft.kind === "rules" ? draft.categories : [];
        fields = cats.length ? (await run("filter_fields", { categories: cats })) || { fields: [], ops: [] } : { fields: [], ops: fields.ops };
      }
      async function pick(id) {
        picked = id;
        const f = all.filters.find((x) => x.id === id);
        draft = f ? clone(f) : null;
        await loadFields();
        render();
      }
      function ruleRow(node, parent, i) {
        const fl = el("select", {}, [new Option("(parameter)", ""), ...fields.fields.map((f) => new Option(f.name, f.name)), ...(node.field && !fields.fields.some((f) => f.name === node.field) ? [new Option(`${node.field} (not shared by the categories)`, node.field)] : [])]);
        fl.value = node.field || "";
        fl.onchange = () => { node.field = fl.value; };
        const op = el("select", {}, (fields.ops.length ? fields.ops : [{ op: node.op, label: node.op }]).map((o) => new Option(o.label, o.op)));
        op.value = node.op;
        const val = el("input", { type: "text", value: node.value || "" });
        val.style.width = "120px";
        val.oninput = () => { node.value = val.value; };
        val.disabled = NO_VALUE.has(node.op);
        op.onchange = () => { node.op = op.value; val.disabled = NO_VALUE.has(node.op); };
        const not = el("input", { type: "checkbox", checked: !!node.not, title: "Turn the test round: elements that do not pass it" });
        not.onchange = () => { node.not = not.checked; };
        const mc = el("input", { type: "checkbox", checked: !!node.match_case, title: "Text must match capitals too" });
        mc.onchange = () => { node.match_case = mc.checked; };
        const row = el("div", {}, [fl, op, val, el("label", {}, [not, " Not"]), el("label", {}, [mc, " Match case"]), button("✕", () => { parent.of.splice(i, 1); render(); })]);
        row.style.cssText = "display:flex;gap:6px;align-items:center;margin:3px 0;flex-wrap:wrap";
        return row;
      }
      function groupBox(node, parent, i) {
        const how = el("select", {}, [new Option("All of these (AND)", "and"), new Option("Any of these (OR)", "or")]);
        how.value = node.node;
        how.onchange = () => { node.node = how.value; };
        const head = el("div", {}, [how, button("+ Rule", () => { node.of.push(blankRule()); render(); }), button("+ Group", () => { node.of.push({ node: "or", of: [blankRule()] }); render(); }),
          ...(parent ? [button("✕", () => { parent.of.splice(i, 1); render(); })] : [])]);
        head.style.cssText = "display:flex;gap:6px;align-items:center";
        const box = el("div", {}, [head, ...node.of.map((c, k) => (c.node === "rule" ? ruleRow(c, node, k) : groupBox(c, node, k)))]);
        box.style.cssText = "border-left:3px solid var(--line);padding:4px 0 4px 8px;margin:4px 0";
        return box;
      }
      function renderAll() {
        const names = el("select", { size: 14 }, all.filters.map((f) => new Option(`${f.name}${f.kind === "selection" ? "  (selection)" : ""}`, f.id, false, f.id === picked)));
        names.style.cssText = "width:230px";
        names.onchange = () => pick(Number(names.value));
        const make = async (filter) => {
          const r = await Backend.call("add_view_filter", { filter }).catch((e) => { say(String(e.message || e)); return null; });
          if (!r) return;
          all = await run("view_filters", {});
          await pick(r.id);
        };
        const fresh = (base) => { let n = base, k = 2; while (all.filters.some((f) => f.name.toLowerCase() === n.toLowerCase())) n = `${base} ${k++}`; return n; };
        const sel = app.selection || [];
        const fromSel = button("New from Selection", () => make({ name: fresh("Selection Filter"), kind: "selection", ids: sel }));
        fromSel.disabled = !sel.length;
        fromSel.title = sel.length ? `A filter of the ${sel.length} selected element${sel.length === 1 ? "" : "s"}` : "Select elements first";
        const left = el("div", {}, [names, el("div", {}, [
          button("New", () => make({ name: fresh("Filter"), kind: "rules", categories: ["Walls"], root: { node: "and", of: [] } })),
          fromSel,
          button("Duplicate", () => { const f = all.filters.find((x) => x.id === picked); if (f) { const c = clone(f); delete c.id; delete c.used_in; c.name = fresh(`${f.name} copy`); make(c); } }),
          button("Delete", async () => {
            const f = all.filters.find((x) => x.id === picked);
            if (!f) return;
            if (f.used_in && !(await askConfirm(`${f.name} is applied in ${f.used_in} view${f.used_in === 1 ? "" : "s"}. Delete it and take it out of them?`))) return;
            if (!(await run("delete_view_filter", { id: f.id }))) return;
            list = list.filter((a) => a.filter !== f.id);
            all = await run("view_filters", {});
            await pick(all.filters[0]?.id ?? null);
            await refresh(true);
          }),
        ])]);
        left.lastChild.style.cssText = "display:flex;gap:4px;flex-wrap:wrap;margin-top:6px;width:230px";
        const right = el("div", {});
        right.style.cssText = "flex:1;min-width:0";
        if (!draft) right.append(el("p", { className: "note", textContent: "No filters yet. New makes a rule filter; New from Selection makes one of the selected elements." }));
        else {
          const name = el("input", { type: "text", value: draft.name });
          name.style.width = "260px";
          name.oninput = () => { draft.name = name.value; };
          right.append(el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]));
          if (draft.kind === "selection") {
            right.append(el("p", { className: "note", textContent: `A selection filter of ${draft.ids.length} element${draft.ids.length === 1 ? "" : "s"}.` }));
            const again = button("Set to the Current Selection", () => { draft.ids = [...sel]; render(); });
            again.disabled = !sel.length;
            const show = button("Select Them", async () => { close(); app.selection = draft.ids.slice(); (app.view.kind === "plan" ? Plan : app.vp)?.setSelection(app.selection); renderProperties(); });
            right.append(el("div", {}, [again, show]));
          } else {
            const box = el("div", {});
            box.style.cssText = "max-height:120px;overflow:auto;border:1px solid var(--line);padding:4px 8px;columns:3";
            for (const c of all.categories) {
              const t = el("input", { type: "checkbox", checked: draft.categories.includes(c) });
              t.onchange = async () => { draft.categories = all.categories.filter((x) => (x === c ? t.checked : draft.categories.includes(x))); await loadFields(); render(); };
              const l = el("label", {}, [t, " " + c]);
              l.style.display = "block";
              box.append(l);
            }
            right.append(el("div", { className: "modal-sub", textContent: "Categories" }), box,
              el("div", { className: "modal-sub", textContent: "Rules (the parameters every ticked category has)" }), groupBox(draft.root, null, 0));
          }
          const count = el("span", {});
          const bar = el("div", {}, [
            button("Save Filter", async () => {
              const filter = clone(draft); delete filter.id; delete filter.used_in;
              try { await Backend.call("set_view_filter", { id: draft.id, filter }); } catch (e) { say(String(e.message || e)); return; }
              say("");
              all = await run("view_filters", {});
              await pick(draft.id);
              await refresh(true);
              setPrompt(`Filter ${filter.name} saved.`);
            }, true),
            button("Count Matches", async () => {
              const filter = clone(draft); delete filter.id; delete filter.used_in;
              const r = await Backend.call("view_filter_matches", { filter }).catch((e) => { say(String(e.message || e)); return null; });
              if (r) count.textContent = r.count === 1 ? "1 element matches now" : `${r.count} elements match now`;
            }), count]);
          bar.style.cssText = "display:flex;gap:8px;align-items:center;margin-top:8px";
          right.append(bar);
        }
        const wrap = el("div", {}, [left, right]);
        wrap.style.cssText = "display:flex;gap:14px;align-items:flex-start";
        host.append(wrap);
        for (const s of host.querySelectorAll(".modal-sub")) { s.style.fontWeight = "600"; s.style.margin = "8px 0 2px"; }
      }

      function render() {
        say("");
        const tabNames = { ...(isView ? { view: `In This View: ${v.name}` } : {}), all: "All Filters" };
        tabs.replaceChildren(...Object.entries(tabNames).map(([k, label]) => { const b = el("button", { textContent: label, className: k === tab ? "active" : "" }); b.onclick = () => { tab = k; render(); }; return b; }));
        host.replaceChildren();
        if (tab === "view") renderView(); else renderAll();
      }
      foot.append(button("OK", async () => { if (await saveView()) close(); }, true), button("Cancel", close), button("Apply", saveView));
      pick(picked);
    }, { wide: true });
  }

  return { open };
})();
