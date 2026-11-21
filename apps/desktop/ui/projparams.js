// Project Parameters: Manage > Project Parameters. The project's own instance parameters, each on one or more model
// categories (they show in those elements' Properties and can be scheduled): add, change and remove. Shared
// parameters come in through a schedule's Fields tab (Select Shared Parameter). The core keeps the rules
// (key_schedule.rs: names unique per category, a unit that cannot change once values are set, refusal while a key
// schedule drives one).
const ProjParams = (() => {
  const UNITS = [["text", "Text"], ["length", "Length"], ["area", "Area"], ["volume", "Volume"], ["angle", "Angle"], ["number", "Number"], ["integer", "Integer"], ["currency", "Currency"], ["yes_no", "Yes/No"]];
  const unitLabel = (u) => (UNITS.find((x) => x[0] === u) || [u, u])[1];

  async function open() {
    let st = await run("project_parameters", {});
    if (!st) return;
    openModal("Project Parameters", (body, foot, close) => {
      const list = el("div", {});
      const msg = el("div", { className: "modal-note" });
      const draw = () => {
        const t = el("table", { className: "grid-table" });
        t.append(el("tr", {}, ["Name", "Categories", "Type", "Group", "Values", ""].map((h) => el("th", { textContent: h }))));
        for (const p of st.parameters) {
          t.append(el("tr", {}, [
            el("td", { textContent: p.name + (p.shared_guid ? " (shared)" : "") }),
            el("td", { textContent: p.categories.join(", ") }),
            el("td", { textContent: unitLabel(p.unit) }),
            el("td", { textContent: p.group }),
            el("td", { textContent: String(p.values) }),
            el("td", {}, [
              button("Modify...", () => form(p)),
              button("Remove", async () => {
                if (!(await askConfirm(`Remove ${p.name}${p.values ? ` and its ${p.values} value${p.values === 1 ? "" : "s"}` : ""}?`))) return;
                try { st = await Backend.call("remove_project_parameter", { id: p.id }); msg.textContent = ""; await refresh(); draw(); } catch (e) { msg.textContent = String(e.message || e); }
              }),
            ]),
          ]));
        }
        list.replaceChildren(st.parameters.length ? t : el("p", { textContent: "The project has no project parameters yet." }));
      };
      // the Parameter Properties box: new (p null) or an existing one
      const form = (p) => openModal(p ? `Parameter Properties: ${p.name}` : "New Project Parameter", (b2, f2, close2) => {
        const name = el("input", { type: "text", value: p ? p.name : "" });
        const unit = el("select", {}, UNITS.map(([v, l]) => new Option(l, v, false, (p ? p.unit : "text") === v)));
        if (p && p.values) { unit.disabled = true; unit.title = "Elements have values for it, so its type cannot change."; }
        const group = el("input", { type: "text", value: p ? p.group : "Identity Data" });
        const chosen = new Set(p ? p.categories : []);
        const cats = el("div", { style: "max-height:220px;overflow:auto;border:1px solid var(--border,#ccc);padding:4px" },
          st.categories.map((c) => { const cb = el("input", { type: "checkbox", checked: chosen.has(c), onchange: () => (cb.checked ? chosen.add(c) : chosen.delete(c)) }); return el("label", { style: "display:block" }, [cb, " " + c]); }));
        const m2 = el("div", { className: "modal-note" });
        b2.append(
          el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]),
          el("label", { className: "new-row" }, [el("span", { textContent: "Type of Parameter" }), unit]),
          el("label", { className: "new-row" }, [el("span", { textContent: "Group Parameter Under" }), group]),
          el("div", { textContent: "Categories", style: "margin-top:8px" }), cats, m2,
        );
        f2.append(button("Cancel", close2), button("OK", async () => {
          const cs = st.categories.filter((c) => chosen.has(c));
          try {
            if (p) st = await Backend.call("edit_project_parameter", { id: p.id, name: name.value, categories: cs, group: group.value, unit: unit.disabled ? null : unit.value });
            else { await Backend.call("add_project_parameter", { name: name.value, categories: cs, unit: unit.value, group: group.value }); st = await Backend.call("project_parameters", {}); }
            close2(); msg.textContent = ""; await refresh(); draw();
          } catch (e) { m2.textContent = String(e.message || e); }
        }, true));
      });
      body.append(list, msg);
      foot.append(button("Add...", () => form(null)), button("Close", close, true));
      draw();
    }, { wide: true });
  }
  return { open };
})();
