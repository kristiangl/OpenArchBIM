// View templates: View > View Templates opens Manage View Templates. On the left the project's templates; on the
// right the chosen one's name and its Include list (what it controls in the views bound to it), with each value in
// words. A template's graphics are edited by setting a view up and bringing the template up to date from it
// (Update from Current View); its filters have their own box. For the current view: bind it to the template,
// stamp the template on once (no link kept), or put the template over it for now (Temporary View Properties: a
// violet frame, nothing saved, ended before an export). Private notes: view-templates-filters.md.
const Templates = (() => {
  const LABEL = {
    scale: "View Scale", display_model: "Display Model", detail_level: "Detail Level", parts_visibility: "Parts Visibility",
    vg_model: "V/G Overrides Model", vg_annotation: "V/G Overrides Annotation", vg_import: "V/G Overrides Import", vg_filters: "V/G Overrides Filters",
    visual_style: "Model Display", show_hidden_lines: "Show Hidden Lines", view_range: "View Range", orientation: "Orientation",
    phase_filter: "Phase Filter", discipline: "Discipline", wall_join_display: "Wall Join Display", color_scheme: "Color Scheme",
    underlay_orientation: "Underlay Orientation", depth_clipping: "Depth Clipping", sun_path: "Sun Path", graphic_display: "Graphic Display Options",
  };
  const KIND = { floor_plan: "Floor Plan", ceiling_plan: "Ceiling Plan", three_d: "3D View", elevation: "Elevation", section: "Section", area_plan: "Area Plan" };
  const words = (v) => (v == null ? "" : String(v).replace(/_/g, " ").replace(/^\w/, (c) => c.toUpperCase()));

  // ---- Temporary View Properties: which views have a template over them for now ----
  const temp = new Map(); // view id -> { id, name }
  let band = null;
  function mark() {
    const on = app.view && temp.get(app.view.viewId);
    for (const id of ["plan", "viewport"]) { const c = document.getElementById(id); if (c) c.style.outline = on ? "4px solid #8a4fd6" : ""; if (c) c.style.outlineOffset = on ? "-4px" : ""; }
    if (!on) { band?.remove(); band = null; return; }
    if (!band) {
      band = el("div", {});
      band.style.cssText = "position:fixed;top:132px;left:50%;transform:translateX(-50%);z-index:30;background:#8a4fd6;color:#fff;padding:3px 10px;border-radius:0 0 4px 4px;font-size:12px;display:flex;gap:10px;align-items:center";
      document.body.append(band);
    }
    const end = button("Restore View Properties", () => endTemp(app.view.viewId));
    end.style.cssText = "padding:1px 8px;cursor:pointer";
    band.replaceChildren(el("span", { textContent: `Temporary View Properties: ${on.name}` }), end);
  }
  async function startTemp(view, t) {
    if (!(await run("set_temp_view_template", { view_id: view, template_id: t.id }))) return false;
    temp.set(view, { id: t.id, name: t.name });
    app.sceneRevision = null;
    await refresh(true);
    mark();
    setPrompt(`${t.name} is over this view for now. Nothing is saved; Restore View Properties ends it.`);
    return true;
  }
  async function endTemp(view) {
    await run("set_temp_view_template", { view_id: view, template_id: null });
    temp.delete(view);
    app.sceneRevision = null;
    await refresh(true);
    mark();
    setPrompt("The view is back to its own properties.");
  }
  // before printing or exporting: every view as it is saved
  async function endAllTemp() {
    if (!temp.size) return;
    await run("clear_temp_view_templates", {});
    temp.clear();
    app.sceneRevision = null;
    await refresh(true);
    mark();
    setPrompt("Temporary View Properties ended for the export: views go out as they are saved.");
  }
  setInterval(() => { if (temp.size || band) mark(); }, 700); // follows the view being switched

  async function manage() {
    let info = await run("view_templates_info", {});
    if (!info) return;
    const v = curView();
    let picked = (v && v.template_id && info.templates.some((t) => t.id === v.template_id) ? v.template_id : info.templates[0]?.id) ?? null;
    let draft = null; // { name, included: Set }
    openModal("Manage View Templates", (body, foot, close) => {
      const host = el("div", {});
      host.style.cssText = "display:flex;gap:14px;align-items:flex-start;min-width:760px;min-height:360px";
      const msg = el("div", { className: "modal-msg" });
      body.append(host, msg);
      const say = (t) => { msg.textContent = t || ""; };
      const cur = () => info.templates.find((t) => t.id === picked) || null;
      const reload = async (keep) => { info = (await run("view_templates_info", {})) || info; pick(keep ?? picked); };
      function pick(id) {
        picked = info.templates.some((t) => t.id === id) ? id : info.templates[0]?.id ?? null;
        const t = cur();
        draft = t ? { name: t.name, included: new Set(t.included) } : null;
        render();
      }
      const call = async (method, params, done) => {
        try { const r = await Backend.call(method, params); say(""); return (await done?.(r)) ?? r; } catch (e) { say(String(e.message || e)); return null; }
      };

      function render() {
        const t = cur();
        const names = el("select", { size: 16 }, info.templates.map((x) => new Option(`${x.name}  (${KIND[x.view_kind] || words(x.view_kind)})`, x.id, false, x.id === picked)));
        names.style.width = "250px";
        names.onchange = () => pick(Number(names.value));
        const fromView = button("New from Current View", () => { close(); createTemplate(); });
        fromView.disabled = !v;
        const left = el("div", {}, [names, el("div", {}, [
          fromView,
          button("Duplicate", () => t && call("duplicate_view_template", { id: t.id }, async (r) => { await refresh(); await reload(r.id); })),
          button("Delete", async () => {
            if (!t) return;
            if (t.used_by && !(await askConfirm(`${t.name} controls ${t.used_by} view${t.used_by === 1 ? "" : "s"}. Delete it? They keep their settings as they are and become free.`))) return;
            await call("delete", { ids: [t.id] }, async () => { await refresh(true); await reload(null); });
          }),
        ])]);
        left.lastChild.style.cssText = "display:flex;gap:4px;flex-wrap:wrap;margin-top:6px;width:250px";
        const right = el("div", {});
        right.style.cssText = "flex:1;min-width:0";
        if (!t) right.append(el("p", { className: "note", textContent: "No view templates yet. Set a view up the way you want, then New from Current View." }));
        else {
          const name = el("input", { type: "text", value: draft.name });
          name.style.width = "280px";
          name.oninput = () => { draft.name = name.value; };
          right.append(el("label", { className: "new-row" }, [el("span", { textContent: "Name" }), name]),
            el("p", { className: "note", textContent: `${KIND[t.view_kind] || words(t.view_kind)} template. ${t.used_by ? `${t.used_by} view${t.used_by === 1 ? " is" : "s are"} bound to it.` : "No view is bound to it."} A ticked property is set by the template and locked in those views; an unticked one stays free in each view.` }));
          const table = el("table", { className: "vg" });
          const head = table.createTHead().insertRow();
          for (const h of ["Include", "Property", "Value in the template"]) head.append(el("th", { textContent: h }));
          const tb = table.createTBody();
          for (const p of info.all_properties) {
            const tr = tb.insertRow();
            const c = el("input", { type: "checkbox", checked: draft.included.has(p) });
            c.onchange = () => { c.checked ? draft.included.add(p) : draft.included.delete(p); };
            tr.insertCell().append(c);
            tr.insertCell().textContent = LABEL[p] || words(p);
            tr.insertCell().textContent = t.values[p] != null ? words(t.values[p]) : "";
          }
          const box = el("div", {}, [table]);
          box.style.cssText = "max-height:300px;overflow:auto;border:1px solid var(--line)";
          right.append(box);
          const sameKind = v && v.view_kind === t.view_kind;
          const why = !v ? "Open a view first" : sameKind ? "" : `The current view is not a ${KIND[t.view_kind] || "view of that kind"}`;
          const forView = (label, title, fn) => { const b = button(label, fn); b.disabled = !sameKind; b.title = why || title; return b; };
          const edit = el("div", {}, [
            button("Save", () => call("patch_view_template", { id: t.id, patch: { name: draft.name.trim(), included: [...draft.included] } }, async () => { await refresh(true); await reload(); setPrompt(`View template ${draft.name} saved.`); }), true),
            forView("Update from Current View", "Set the included properties to what the current view shows", () => call("update_view_template_from_view", { template_id: t.id, view_id: v.id }, async () => { await refresh(true); await reload(); setPrompt(`${t.name} now matches ${v.name}.`); })),
            button("Filters...", () => { close(); Filters.open("view", { template: t }); }),
          ]);
          const use = el("div", {}, [
            el("span", { textContent: v ? `For ${v.name}: ` : "" }),
            forView("Bind to Template", "The view follows the template from now on", () => call("patch_view", { id: v.id, patch: { template: t.id } }, async () => { await refresh(true); await reload(); setPrompt(`${v.name} is now controlled by ${t.name}.`); })),
            forView("Apply Once", "Stamp the template's values on the view; no link is kept", () => call("apply_view_template_once", { view_id: v.id, template_id: t.id }, async (r) => { await refresh(true); setPrompt(`${r.taken} propert${r.taken === 1 ? "y" : "ies"} of ${t.name} applied to ${v.name}. The view stays free.`); })),
            forView("Apply Temporarily", "Temporary View Properties: over the view for now, nothing saved", async () => { if (await startTemp(v.id, t)) close(); }),
          ]);
          for (const d of [edit, use]) d.style.cssText = "display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-top:8px";
          right.append(edit, use);
        }
        host.replaceChildren(left, right);
      }
      foot.append(button("Close", close, true));
      pick(picked);
    }, { wide: true });
  }

  return { manage, endAllTemp, endTemp, mark };
})();
