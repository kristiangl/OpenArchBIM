// Graphic Display Options: one box for how a 3D view, elevation or section is drawn (Revit's), opened from the
// view's Properties. Model Display (style, edges, transparency, silhouettes), Shadows, Lighting (Sun Settings for
// the sun itself), Background (3D) and Depth Cueing (elevations and sections). OK writes them as one undo step;
// the sun and its intensities stay with Sun Settings (sunview.js).
const GDO = (() => {
  const STYLES = [["wireframe", "Wireframe"], ["hidden_line", "Hidden Line"], ["shaded", "Shaded"], ["consistent_colors", "Consistent Colours"], ["realistic", "Realistic"]];
  async function open(viewId) {
    const r = await run("graphic_display", { view_id: viewId });
    if (!r) return;
    if (r.locked) { setPrompt("The view's template sets its graphic display. Change it in the template, or take Graphic Display out of it.", true); return; }
    const g = { ...r.graphic_display }, three = r.kind === "three_d";
    let style = r.visual_style;
    openModal("Graphic Display Options", (body, foot, close) => {
      const check = (label, key) => { const c = el("input", { type: "checkbox", checked: !!g[key], onchange: () => { g[key] = c.checked; } }); return el("label", { className: "new-row" }, [el("span", { textContent: label }), c]); };
      const pct = (label, key) => { const n = el("input", { type: "number", min: "0", max: "100", step: "5", value: g[key] ?? 0, style: "width:70px", onchange: () => { g[key] = Math.max(0, Math.min(100, Math.round(Number(n.value) || 0))); n.value = g[key]; } }); return el("label", { className: "new-row" }, [el("span", { textContent: label }), n]); };
      const head = (t) => el("h4", { textContent: t, style: "margin:12px 0 4px" });
      const st = el("select", { onchange: () => { style = st.value; } }, STYLES.map(([v, l]) => new Option(l, v, false, v === style)));
      body.append(
        head("Model Display"),
        el("label", { className: "new-row" }, [el("span", { textContent: "Style" }), st]),
        check("Show Edges", "show_edges"),
        ...(three ? [pct("Transparency (%)", "transparency")] : []),
        check("Show Silhouettes", "silhouettes"),
        head("Shadows"),
        check("Cast Shadows", "cast_shadows"),
        check("Ambient Shadows", "ambient_shadows"),
      );
      if (three) {
        const bg = el("select", { onchange: () => { g.background = bg.value; } }, [["none", "None"], ["sky", "Sky"], ["gradient", "Gradient"]].map(([v, l]) => new Option(l, v, false, v === g.background)));
        body.append(
          head("Lighting"),
          pct("Sun (%)", "sun_intensity"),
          pct("Ambient Light (%)", "ambient_intensity"),
          el("div", { className: "new-row" }, [el("span", { textContent: "" }), button("Sun Settings...", () => { close(); SunUI.open(); })]),
          head("Background"),
          el("label", { className: "new-row" }, [el("span", { textContent: "Background" }), bg]),
        );
      } else {
        body.append(
          head("Depth Cueing"),
          check("Show Depth", "depth_cue"),
          pct("Fade Start (%)", "depth_cue_start"),
          pct("Fade End (%)", "depth_cue_end"),
          pct("Fade Limit (%)", "depth_cue_fade"),
        );
      }
      foot.append(button("Cancel", close), button("OK", async () => {
        const keys = ["show_edges", "transparency", "silhouettes", "cast_shadows", "ambient_shadows", "sun_intensity", "ambient_intensity", "background", "depth_cue", "depth_cue_start", "depth_cue_end", "depth_cue_fade"];
        const patch = {};
        for (const k of keys) if (g[k] !== r.graphic_display[k]) patch[k] = g[k];
        const settings = {};
        if (Object.keys(patch).length) settings.graphic_display = patch;
        if (style !== r.visual_style) settings.visual_style = style;
        if (!Object.keys(settings).length) { close(); return; }
        if (await run("patch_view", { id: viewId, patch: { settings } })) {
          close();
          await refresh();
          if (typeof refreshView === "function") await refreshView(true);
          renderProperties();
        }
      }, true));
    });
  }
  return { open };
})();
