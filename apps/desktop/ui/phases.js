// Phasing: Manage > Phasing. One box with three tabs: the project's phases in time order, the phase filters (how a
// view shows New, Existing, Demolished and Temporary elements) and the graphic override of each status. The core
// keeps the rules (phasing.rs); private notes: phasing.md.
const Phases = (() => {
  const SHOW = [["by_category", "By Category"], ["overridden", "Overridden"], ["not_displayed", "Not Displayed"]];
  const STATUS = ["new", "existing", "demolished", "temporary"];

  async function open(startTab = "phases") {
    let ph = await run("phasing", {});
    if (!ph) return;
    const std = await run("standards", {});
    const linePats = std ? std.line_patterns.map((p) => p.pattern.name) : ["Solid", "Dash"];
    let tab = startTab, picked = ph.phases[ph.phases.length - 1]?.id;
    const op = async (args) => {
      const r = await run("phase_op", args);
      if (r) { ph = r; await refresh(); }
      draw();
    };

    let host;
    const draw = () => {
      host.replaceChildren();
      if (tab === "phases") drawPhases(); else if (tab === "filters") drawFilters(); else drawOverrides();
    };

    function drawPhases() {
      const t = el("table", { className: "grid-table" });
      t.append(el("tr", {}, ["", "Name", "Description", "Built", "Demolished", "Views"].map((h) => el("th", { textContent: h }))));
      ph.phases.forEach((p, i) => {
        const name = el("input", { type: "text", value: p.name, onchange: () => op({ op: "rename", id: p.id, name: name.value }) });
        const desc = el("input", { type: "text", value: p.description, style: "width:200px", onchange: () => op({ op: "rename", id: p.id, description: desc.value }) });
        const row = el("tr", { className: p.id === picked ? "picked" : "", onclick: (e) => { if (e.target.tagName !== "INPUT") { picked = p.id; draw(); } } },
          [el("td", { textContent: String(i + 1) }), el("td", {}, [name]), el("td", {}, [desc]), el("td", { textContent: String(p.created) }), el("td", { textContent: String(p.demolished) }), el("td", { textContent: String(p.views) })]);
        t.append(row);
      });
      const at = ph.phases.findIndex((p) => p.id === picked);
      const ask = (after) => { const n = prompt("Name of the new phase", "New Phase"); if (n) op({ op: "insert", after, name: n }); };
      const bar = el("div", { className: "row" }, [
        button("Insert Before", () => ask(at > 0 ? ph.phases[at - 1].id : null)),
        button("Insert After", () => ask(picked ?? null)),
        button("Combine with Previous", () => { if (at > 0) op({ op: "combine", id: picked, into: ph.phases[at - 1].id }); else setPrompt("The first phase has none before it.", true); }),
        button("Combine with Next", () => { if (at >= 0 && at < ph.phases.length - 1) op({ op: "combine", id: picked, into: ph.phases[at + 1].id }); else setPrompt("The last phase has none after it.", true); }),
      ]);
      host.append(el("p", { className: "note", textContent: "Earliest at the top. Combining a phase moves what was built and demolished in it, and the views showing it, to the phase it joins." }), t, bar);
    }

    function drawFilters() {
      const t = el("table", { className: "grid-table" });
      t.append(el("tr", {}, ["Filter Name", "New", "Existing", "Demolished", "Temporary", ""].map((h) => el("th", { textContent: h }))));
      for (const f of ph.filters) {
        const name = el("input", { type: "text", value: f.name, onchange: () => op({ op: "set_filter", filter: { ...f, name: name.value } }) });
        const cells = STATUS.map((s) => {
          const sel = el("select", { onchange: () => op({ op: "set_filter", filter: { ...f, [s]: sel.value } }) });
          for (const [v, l] of SHOW) { const o = new Option(l, v); o.selected = f[s] === v; sel.add(o); }
          return el("td", {}, [sel]);
        });
        t.append(el("tr", {}, [el("td", {}, [name]), ...cells, el("td", {}, [button("Delete", () => op({ op: "delete_filter", id: f.id }))])]));
      }
      host.append(t, el("div", { className: "row" }, [button("New", () => { const n = prompt("Name of the new phase filter", "New Filter"); if (n) op({ op: "add_filter", name: n }); })]));
    }

    function drawOverrides() {
      const t = el("table", { className: "grid-table" });
      t.append(el("tr", {}, ["Phase Status", "Projection Lines", "Cut Lines", "Halftone", "Transparency"].map((h) => el("th", { textContent: h }))));
      for (const o of ph.overrides) {
        const ov = o.override;
        const pat = (key) => {
          const sel = el("select", { onchange: () => op({ op: "set_override", status: o.status, override: { ...ov, [key]: { ...ov[key], pattern: sel.value || null } } }) });
          sel.add(new Option("By category", ""));
          for (const n of linePats) { const x = new Option(n, n); x.selected = ov[key] && ov[key].pattern === n; sel.add(x); }
          return sel;
        };
        const half = el("input", { type: "checkbox", checked: !!ov.halftone, onchange: () => op({ op: "set_override", status: o.status, override: { ...ov, halftone: half.checked } }) });
        const tr = el("input", { type: "number", min: "0", max: "100", step: "10", value: ov.transparency || 0, style: "width:60px", onchange: () => op({ op: "set_override", status: o.status, override: { ...ov, transparency: Math.max(0, Math.min(100, parseInt(tr.value) || 0)) } }) });
        t.append(el("tr", {}, [el("td", { textContent: o.label }), el("td", {}, [pat("projection_lines")]), el("td", {}, [pat("cut_lines")]), el("td", {}, [half]), el("td", {}, [tr])]));
      }
      host.append(el("p", { className: "note", textContent: "Used where a view's phase filter says Overridden for that status." }), t);
    }

    openModal("Phasing", (body, foot, close) => {
      const tabs = el("div", { className: "modal-tabs" });
      host = el("div", {});
      host.style.cssText = "min-height:260px";
      const tabBtns = [["phases", "Project Phases"], ["filters", "Phase Filters"], ["overrides", "Graphic Overrides"]].map(([k, l]) => {
        const b = el("button", { textContent: l, className: k === tab ? "active" : "", onclick: () => { tab = k; tabBtns.forEach((x) => x.classList.toggle("active", x === b)); draw(); } });
        return b;
      });
      tabs.append(...tabBtns);
      body.append(tabs, host);
      foot.append(button("Close", close, true));
      draw();
    }, { wide: true });
  }

  // Demolish: the selected elements are taken down in the view's phase
  async function demolish() {
    if (!app.selection.length) { setPrompt("Select the elements to demolish, then click Demolish. They are demolished in this view's phase.", true); return; }
    if (app.view.viewId == null) return;
    if (await run("demolish", { view_id: app.view.viewId, ids: app.selection })) {
      await refresh();
      renderProperties();
      setPrompt(`Demolished ${app.selection.length} element${app.selection.length === 1 ? "" : "s"} in this view's phase. Phase Demolished in Properties undoes it.`);
    }
  }

  return { open, demolish };
})();
