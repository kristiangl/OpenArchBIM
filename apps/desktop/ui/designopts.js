// Design options: Manage > Design Options (option sets, each with its options and a primary), Add to Set, Pick to
// Edit, and the status bar's option being edited and Exclude Options. The core keeps the rules (designopt.rs);
// private notes: design-options.md.
const DesignOpts = (() => {
  let state = { sets: [], active: null, member: {}, outside: [] };
  let outside = new Set();
  let exclude = true;
  try { exclude = localStorage.getItem("openarch.excludeOptions") !== "0"; } catch {}
  let picking = false;
  // datums, views and their markers stay in the main model and can always be picked
  const FREE = new Set(["Grids", "Levels", "Elevations", "Sections", "Callouts", "Views", "Scope Boxes", "Project Base Point", "Survey Point"]);

  async function load() {
    try { state = (await Backend.call("design_options", {})) || state; } catch {}
    outside = new Set(state.outside || []);
    renderBar();
  }

  // Whether an element cannot be picked: while an option is edited, everything outside it; in the main model, what
  // is in an option while Exclude Options is on (spec 3.1).
  function locked(id, category) {
    if (picking || !state.sets.length || FREE.has(category)) return false;
    if (state.active != null) return outside.has(id);
    return exclude && state.member[String(id)] != null;
  }

  const label = (s, o) => `${s.name}: ${o.name}${o.primary ? " (primary)" : ""}`;

  async function setActive(option) {
    const r = await run("set_active_option", { option });
    if (!r) return;
    state = r;
    renderBar();
    await refresh(true);
    setPrompt(option == null ? "Editing the main model." : `Editing ${state.active_label}. New elements go into it; everything else is greyed out and cannot be picked.`);
  }

  function renderBar() {
    const box = document.getElementById("design-opts");
    if (!box) return;
    if (!state.sets.length) { box.replaceChildren(el("span", { textContent: "Main Model", title: "Manage > Design Options makes option sets" })); return; }
    const sel = el("select", { title: "The design option being edited", onchange: () => setActive(sel.value ? Number(sel.value) : null) });
    sel.add(new Option("Main Model", ""));
    for (const s of state.sets) {
      const g = el("optgroup", { label: s.name });
      for (const o of s.options) { const x = new Option(label(s, o), String(o.id)); x.selected = o.id === state.active; g.append(x); }
      sel.append(g);
    }
    const ex = el("button", { className: exclude ? "on" : "", textContent: "Exclude Options", title: `Exclude Options: ${exclude ? "on" : "off"}. While the main model is edited, elements in design options cannot be picked.`,
      onclick: () => { exclude = !exclude; try { localStorage.setItem("openarch.excludeOptions", exclude ? "1" : "0"); } catch {} renderBar(); } });
    box.replaceChildren(sel, ex);
  }

  // Manage > Design Options
  async function open() {
    let d = await run("design_options", {});
    if (!d) return;
    let picked = d.active ?? d.sets[0]?.options[0]?.id ?? null;
    const setOf = (id) => d.sets.find((s) => s.id === id || s.options.some((o) => o.id === id));
    const op = async (args) => {
      const r = await run("design_option_op", args);
      if (r) { d = r; state = r; renderBar(); await refresh(true); }
      draw();
    };
    let host;
    const draw = () => {
      host.replaceChildren();
      const t = el("div", { className: "opt-tree" });
      if (!d.sets.length) t.append(el("p", { className: "note", textContent: "No option sets yet. An option set is one design decision (an entry canopy, a kitchen layout); its options are the alternatives, of which one is primary." }));
      for (const s of d.sets) {
        t.append(el("div", { className: "opt-set" + (s.id === picked ? " picked" : ""), textContent: s.name, onclick: () => { picked = s.id; draw(); } }));
        for (const o of s.options) {
          const editing = o.id === d.active ? "  (editing)" : "";
          t.append(el("div", { className: "opt-option" + (o.id === picked ? " picked" : ""), textContent: `${o.name}${o.primary ? " (primary)" : ""}  ${o.count} element${o.count === 1 ? "" : "s"}${editing}`, onclick: () => { picked = o.id; draw(); } }));
        }
      }
      const s = setOf(picked);
      const isSet = s && s.id === picked;
      const opt = s && !isSet ? s.options.find((o) => o.id === picked) : null;
      const ask = (q, def) => { const n = prompt(q, def); return n && n.trim() ? n.trim() : null; };
      const col = el("div", { className: "opt-buttons" }, [
        el("div", { className: "opt-head", textContent: "Edit" }),
        button(d.active != null && opt && d.active === opt.id ? "Finish Editing" : "Edit Selected", () => {
          if (d.active != null && opt && d.active === opt.id) { setActive(null).then(() => run("design_options", {}).then((r) => { if (r) { d = r; draw(); } })); return; }
          if (!opt) { setPrompt("Pick an option to edit.", true); return; }
          setActive(opt.id).then(() => run("design_options", {}).then((r) => { if (r) { d = r; draw(); } }));
        }),
        el("div", { className: "opt-head", textContent: "Option Set" }),
        button("New", () => { const n = ask("Name of the new option set", `Option Set ${d.sets.length + 1}`); if (n) op({ op: "add_set", name: n }); }),
        button("Rename", () => { if (!isSet) { setPrompt("Pick an option set to rename.", true); return; } const n = ask("Option set name", s.name); if (n) op({ op: "rename", id: s.id, name: n }); }),
        button("Delete", () => { if (!isSet) { setPrompt("Pick an option set to delete.", true); return; } if (confirm(`Delete '${s.name}' and every element in all its options?`)) op({ op: "delete_set", id: s.id }); }),
        button("Accept Primary", () => {
          if (!s) { setPrompt("Pick an option set.", true); return; }
          const p = s.options.find((o) => o.primary);
          const others = s.options.filter((o) => !o.primary).reduce((n, o) => n + o.count, 0);
          if (confirm(`Accept '${p.name}' for '${s.name}'? Its elements join the main model; the other options and their ${others} element${others === 1 ? "" : "s"} are deleted, and the set goes.`)) op({ op: "accept_primary", id: s.id });
        }),
        el("div", { className: "opt-head", textContent: "Option" }),
        button("New", () => { if (!s) { setPrompt("Pick an option set first.", true); return; } op({ op: "add_option", set: s.id }); }),
        button("Rename", () => { if (!opt) { setPrompt("Pick an option to rename.", true); return; } const n = ask("Option name", opt.name); if (n) op({ op: "rename", id: opt.id, name: n }); }),
        button("Make Primary", () => { if (!opt) { setPrompt("Pick an option.", true); return; } op({ op: "make_primary", id: opt.id }); }),
        button("Delete", () => { if (!opt) { setPrompt("Pick an option to delete.", true); return; } if (confirm(`Delete '${opt.name}' and its ${opt.count} element${opt.count === 1 ? "" : "s"}?`)) op({ op: "delete_option", id: opt.id }); }),
      ]);
      const now = el("p", { className: "note", textContent: `Editing: ${d.active_label || "Main Model"}` });
      host.append(el("div", { className: "opt-layout" }, [t, col]), now);
    };
    openModal("Design Options", (body, foot, close) => {
      host = el("div", {});
      host.style.cssText = "min-height:260px";
      body.append(host);
      foot.append(button("Close", close, true));
      draw();
    }, { wide: true });
  }

  // Add to Set: the selection goes into the options ticked (the first takes the elements, the others copies)
  async function addToSet() {
    if (!state.sets.length) { setPrompt("Make an option set first (Manage > Design Options).", true); return; }
    const ids = app.selection.filter((id) => state.member[String(id)] == null);
    if (!ids.length) { setPrompt("Select main-model elements, then Add to Set.", true); return; }
    let setId = state.sets[0].id;
    openModal("Add to Design Option Set", (body, foot, close) => {
      const host = el("div", {});
      const draw = () => {
        const s = state.sets.find((x) => x.id === setId);
        const pick = el("select", { onchange: () => { setId = Number(pick.value); draw(); } });
        for (const x of state.sets) { const o = new Option(x.name, String(x.id)); o.selected = x.id === setId; pick.add(o); }
        const boxes = s.options.map((o) => el("input", { type: "checkbox", checked: true, dataset: { id: String(o.id) } }));
        host.replaceChildren(
          el("div", { className: "row" }, [el("span", { textContent: "Option Set: " }), pick]),
          ...s.options.map((o, i) => el("label", { className: "row" }, [boxes[i], el("span", { textContent: ` ${o.name}${o.primary ? " (primary)" : ""}` })])),
          el("p", { className: "note", textContent: "Each ticked option gets the elements: the first keeps the originals, the others get copies in place, so the options can change independently. A wall's doors and windows go with it." }),
        );
        host.boxes = boxes;
      };
      draw();
      body.append(host);
      foot.append(button("OK", async () => {
        const options = host.boxes.filter((b) => b.checked).map((b) => Number(b.dataset.id));
        if (!options.length) { setPrompt("Tick at least one option.", true); return; }
        const r = await run("add_to_set", { ids, options });
        if (!r) return;
        close();
        await load();
        await refresh(true);
        setPrompt(`Added to ${options.length} option${options.length === 1 ? "" : "s"}. Elements in options are greyed out of reach while Exclude Options is on.`);
      }, true), button("Cancel", close));
    });
  }

  // Pick to Edit: click an element in a design option to edit that option
  function pickToEdit() {
    if (!state.sets.length) { setPrompt("The project has no design options yet (Manage > Design Options).", true); return; }
    const sel = app.selection.find((id) => state.member[String(id)] != null);
    if (sel != null) { run("pick_to_edit", { id: sel }).then((r) => { if (r) { state = r; renderBar(); refresh(true); setPrompt(`Editing ${r.active_label}.`); } }); return; }
    picking = true;
    setPrompt("Click an element in a design option to edit that option. Esc cancels.");
  }

  // the selection made while picking: edit its option
  function onSelect(ids) {
    if (!picking || !ids.length) return false;
    picking = false;
    const id = ids.find((x) => state.member[String(x)] != null);
    if (id == null) { setPrompt("That element is in the main model.", true); return false; }
    run("pick_to_edit", { id }).then((r) => { if (r) { state = r; renderBar(); refresh(true); setPrompt(`Editing ${r.active_label}.`); } });
    return true;
  }
  function cancelPick() { picking = false; }

  return { load, locked, open, addToSet, pickToEdit, onSelect, cancelPick, state: () => state };
})();
