// Global parameters (Manage > Global Parameters): the dialog that makes and edits them, the picker that labels a
// dimension or associates a family parameter with one. The model owns the values, formulas and what they drive
// (crates/openarch-model/src/globalparam.rs); this file only asks.

const GLOBAL_KINDS = [["length", "Length"], ["number", "Number"], ["integer", "Integer"], ["angle", "Angle"], ["yes_no", "Yes/No"], ["text", "Text"]];
const globalKindName = (k) => (GLOBAL_KINDS.find(([v]) => v === k) || [k, k])[1];
const globalErr = (e) => (e && e.message) || String(e);

// The whole dialog is one undo step: OK keeps the edits, Cancel takes them all back.
async function openGlobalParams() {
  let mark;
  try { mark = (await Backend.call("edit_begin", {})).mark; } catch { return; }
  let list = [];
  openModal("Global Parameters", (body, foot, close) => {
    const host = el("div", { className: "vg-host gp-host" }), msg = el("div", { className: "modal-msg" });
    const reload = async () => { list = (await Backend.call("global_params", {})).params || []; draw(); };
    const call = async (m, p) => {
      const err = await Backend.call(m, p).then(() => null, globalErr);
      msg.textContent = err || "";
      await reload();
      return !err;
    };
    const patch = (id, p) => call("patch_global_param", { id, patch: p });
    const finish = async (how) => {
      if (how === "cancel") { try { await Backend.call("edit_cancel", { mark }); } catch { /* nothing changed */ } }
      else mark = (await Backend.call("edit_commit", { mark, name: "Global Parameters" })).mark;
      await refresh();
      if (how !== "apply") close();
    };
    body.closest(".modal-back").onEscape = () => finish("cancel");

    function valueCell(g) {
      const td = el("td");
      const fixed = g.reporting || g.formula;
      if (g.kind === "yes_no") {
        const cb = el("input", { type: "checkbox", checked: !!g.value, disabled: !!fixed });
        cb.onchange = () => patch(g.id, { value: cb.checked });
        td.append(cb);
      } else if (fixed) {
        td.append(el("span", { className: "gp-fixed", textContent: String(g.value), title: g.reporting ? "Read from the dimension it labels" : "Worked out from the formula" }));
      } else {
        const numeric = g.kind !== "text";
        const inp = el("input", { type: numeric ? "number" : "text", value: g.value, step: "any", className: "gp-value" });
        inp.onchange = () => patch(g.id, { value: numeric ? parseFloat(inp.value) : inp.value });
        td.append(inp);
      }
      return td;
    }
    function draw() {
      const t = el("table", { className: "vg gp" });
      t.append(el("tr", {}, ["Name", "Type", "Value", "Formula", "Reporting", "Group", "Used by", ""].map((x) => el("th", { textContent: x }))));
      const groups = [...new Set(list.map((g) => g.group || "Other"))].sort();
      for (const grp of groups) {
        t.append(el("tr", { className: "gp-group" }, [el("td", { colSpan: 8, textContent: grp })]));
        for (const g of list.filter((x) => (x.group || "Other") === grp)) {
          const name = el("input", { type: "text", value: g.name, className: "gp-name" });
          name.onchange = () => patch(g.id, { name: name.value });
          const formula = el("input", { type: "text", value: g.formula || "", placeholder: g.reporting ? "" : "e.g. [Building Width] / 3", disabled: g.reporting, className: "gp-formula" });
          formula.onchange = () => patch(g.id, { formula: formula.value });
          const rep = el("input", { type: "checkbox", checked: g.reporting, disabled: !["length", "angle"].includes(g.kind) });
          rep.title = "Reporting: reads its value from the one dimension it labels";
          rep.onchange = () => patch(g.id, { reporting: rep.checked });
          const group = el("input", { type: "text", value: g.group || "", className: "gp-group-in" });
          group.onchange = () => patch(g.id, { group: group.value });
          const uses = [g.labels.length ? `${g.labels.length} dimension${g.labels.length === 1 ? "" : "s"}` : "", g.associated ? `${g.associated} family value${g.associated === 1 ? "" : "s"}` : ""].filter(Boolean).join(", ") || "-";
          const del = button("Delete", () => call("delete_global_param", { id: g.id }));
          const tr = el("tr", { className: g.error ? "gp-bad" : "" }, [el("td", {}, [name]), el("td", { textContent: globalKindName(g.kind) }), valueCell(g), el("td", {}, [formula]), el("td", {}, [rep]), el("td", {}, [group]), el("td", { textContent: uses }), el("td", {}, [del])]);
          if (g.error) tr.title = g.error;
          t.append(tr);
        }
      }
      if (!list.length) t.append(el("tr", {}, [el("td", { className: "empty", colSpan: 8, textContent: "No global parameters yet. Add one below." })]));
      // a new one: name, type, driving or reporting
      const nName = el("input", { type: "text", placeholder: "Name", className: "gp-name" });
      const nKind = el("select", {}, GLOBAL_KINDS.map(([v, l]) => new Option(l, v)));
      const nRep = el("input", { type: "checkbox" });
      const add = button("New Parameter", async () => {
        if (!nName.value.trim()) { msg.textContent = "Give the new parameter a name."; return; }
        await call("add_global_param", { name: nName.value.trim(), kind: nKind.value, reporting: nRep.checked, group: "Dimensions" });
      });
      const adder = el("div", { className: "gp-add" }, [nName, nKind, el("label", {}, [nRep, " Reporting"]), add]);
      host.replaceChildren(t, adder);
    }
    body.append(el("p", { className: "note", textContent: "Driving parameters set the dimensions they label; reporting ones read the one dimension they label. A formula names other parameters in square brackets." }), host, msg);
    foot.append(button("Cancel", () => finish("cancel")), button("Apply", () => finish("apply")), button("OK", () => finish("ok"), true));
    reload();
  }, { wide: true });
}

// Choose a global parameter of a kind (for a label or an association). Resolves to its id, null for none, or
// undefined when the person cancels. "New..." makes one on the way.
function pickGlobal(title, kind, current, allowReporting) {
  return new Promise(async (resolve) => {
    const all = ((await Backend.call("global_params", {}).catch(() => ({ params: [] }))).params || []).filter((g) => g.kind === kind || (["number", "integer"].includes(kind) && ["number", "integer"].includes(g.kind)));
    const fit = all.filter((g) => allowReporting || !g.reporting);
    openModal(title, (body, foot, close) => {
      const sel = el("select", { size: Math.min(10, fit.length + 2), className: "gp-pick" }, [new Option("<None>", ""), ...fit.map((g) => new Option(g.reporting ? `${g.name} (reporting)` : `${g.name} = ${g.value}`, g.id, false, g.id === current))]);
      if (current == null) sel.value = "";
      const done = (v) => { close(); resolve(v); };
      const nName = el("input", { type: "text", placeholder: "New parameter name" });
      const add = button("New...", async () => {
        const name = nName.value.trim();
        if (!name) { nName.focus(); return; }
        try { const r = await Backend.call("add_global_param", { name, kind: kind === "integer" ? "integer" : kind, group: "Dimensions" }); done(r.id); } catch (e) { msg.textContent = globalErr(e); }
      });
      const msg = el("div", { className: "modal-msg" });
      sel.ondblclick = () => done(sel.value === "" ? null : Number(sel.value));
      body.append(el("p", { className: "note", textContent: `${globalKindName(kind)} parameters` }), sel, el("div", { className: "gp-add" }, [nName, add]), msg);
      body.closest(".modal-back").onEscape = () => done(undefined);
      foot.append(button("Cancel", () => done(undefined)), button("OK", () => done(sel.value === "" ? null : Number(sel.value)), true));
    });
  });
}

// Properties rows: the button beside a family parameter that a global can set, and beside a dimension's Label
function globalRowButton(r, target) {
  if (r.associate || r.global) {
    const a = r.global || r.associate;
    const b = el("button", { className: "gp-assoc" + (r.global ? " on" : ""), textContent: "≡", title: r.global ? `Set by the global parameter ${r.global.name}. Click to change or let go.` : "Associate Global Parameter" });
    b.onclick = async () => {
      const kind = r.associate ? r.associate.kind : null;
      const g = await pickGlobal(`Associate Global Parameter: ${a.param}`, kind || "length", r.global ? r.global.id : null, false);
      if (g === undefined) return;
      if (await run("associate_global", { id: a.owner, param: a.param, global: g })) { await refresh(); if (typeof renderProperties === "function") renderProperties(); }
    };
    return b;
  }
  if (r.add_global && target && target.method === "patch_dimension") {
    const b = el("button", { className: "gp-assoc", textContent: "+", title: "Make a global parameter and label this segment with it" });
    b.onclick = async () => {
      const g = await pickGlobal("Label with Global Parameter", "length", null, true);
      if (g == null) return;
      if (await run("label_dimension", { id: target.id, segment: r.add_global.index, param: g })) await refresh();
    };
    return b;
  }
  return null;
}
