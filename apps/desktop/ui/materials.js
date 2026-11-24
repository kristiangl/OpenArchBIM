// The Material Manager's appearance side, used by the Materials window (styles.js): the Appearance tab (a standard
// appearance with linked image maps, or a parametric one made by the generator), its preview, baking a generated
// material to image files, and the user material library (a folder of material files shared between projects).
const MatUI = (() => {
  const h = (tag, props, ...kids) => {
    const e = document.createElement(tag);
    // `list` (an input's datalist) is an attribute only
    const { list, ...rest } = props || {};
    Object.assign(e, rest);
    if (list) e.setAttribute("list", list);
    e.append(...kids.filter((k) => k !== null && k !== undefined));
    return e;
  };
  const errText = (e) => (e && e.message) || String(e);
  let presets = null;
  const getPresets = async () => presets || (presets = await Backend.call("material_presets"));

  const LABEL = {
    tile: "Ceramic / Stone Tiles", wood_plank: "Wood Planks", wood_solid: "Wood Solid", masonry: "Masonry", metal: "Architectural Metal",
    stack_bond: "Stack Bond", running_bond: "Running Bond", herringbone: "Herringbone", chevron: "Chevron", basketweave: "Basketweave", versailles: "Versailles (French pattern)",
    flemish: "Flemish Bond", english: "English Bond", strip: "Strip (random lengths)", board_and_batten: "Board and Batten",
    regular: "Regular offset", random: "Random", seamless: "Seamless Grain", glulam: "Glue-Laminated Strips", corrugated: "Corrugated", trapezoidal: "Trapezoidal Ribbed",
    standing_seam: "Standing Seam", cassette: "Flat Cassette Panels", perforated: "Perforated Sheet",
    matte: "Matte", satin: "Satin", high_gloss: "High Gloss", textured_stone: "Textured Stone", natural: "Natural (oiled)", lacquered: "Lacquered",
    mill_finish: "Mill Finish", brushed: "Brushed", powder_coated: "Powder-Coated Matte", patina: "Patina",
    flush: "Flush", raked: "Raked", concave: "Concave", weathered: "Weathered",
  };
  const label = (k) => LABEL[k] || k;
  // a class name picks the archetype a new parametric appearance starts as
  const archetypeFor = (cls) => /wood|timber/i.test(cls) ? "wood_plank" : /mason|brick|block/i.test(cls) ? "masonry" : /metal|steel|alum/i.test(cls) ? "metal" : "tile";

  // generated maps by parameters, so a redraw of the window does not make them again
  const cache = new Map();
  async function mapsFor(params) {
    const key = JSON.stringify(params);
    if (!cache.has(key)) {
      if (cache.size > 24) cache.delete(cache.keys().next().value);
      cache.set(key, Backend.call("material_maps", { params, maps: ["albedo", "normal", "height", "roughness"] }).then(async (r) => {
        const img = (url) => new Promise((ok, bad) => { const i = new Image(); i.onload = () => ok(i); i.onerror = bad; i.src = url; });
        return { ...r, img: { albedo: await img(r.albedo), normal: await img(r.normal), height: await img(r.height), roughness: await img(r.roughness) } };
      }).catch((e) => { cache.delete(key); throw e; }));
    }
    return cache.get(key);
  }

  // albedo lit by its normal map from the upper left: what the relief looks like
  function litImage(albedo, normal) {
    const w = albedo.naturalWidth, hh = albedo.naturalHeight;
    const c = document.createElement("canvas"); c.width = w; c.height = hh;
    const x = c.getContext("2d");
    x.drawImage(normal, 0, 0, w, hh);
    const n = x.getImageData(0, 0, w, hh);
    x.drawImage(albedo, 0, 0);
    const a = x.getImageData(0, 0, w, hh);
    const L = [-0.45, 0.55, 0.7], ll = Math.hypot(...L);
    for (let i = 0; i < a.data.length; i += 4) {
      const nx = n.data[i] / 127.5 - 1, ny = n.data[i + 1] / 127.5 - 1, nz = n.data[i + 2] / 127.5 - 1;
      const d = Math.max(0, (nx * L[0] + ny * L[1] + nz * L[2]) / ll);
      const k = 0.38 + 0.75 * d;
      a.data[i] = Math.min(255, a.data[i] * k); a.data[i + 1] = Math.min(255, a.data[i + 1] * k); a.data[i + 2] = Math.min(255, a.data[i + 2] * k);
    }
    x.putImageData(a, 0, 0);
    return c;
  }

  // pattern lines (model mm, y up) over a canvas showing [0, vw] x [0, vh] at k px a mm
  function drawHatch(ctx, lines, vw, vh, k, colour) {
    ctx.save();
    ctx.strokeStyle = colour; ctx.lineWidth = 1.2;
    const corners = [[0, 0], [vw, 0], [vw, vh], [0, vh]];
    for (const L of lines) {
      const a = (L.angle_deg * Math.PI) / 180, dx = Math.cos(a), dy = Math.sin(a), nx = -dy, ny = dx, sp = L.spacing;
      const v0 = L.origin[0] * nx + L.origin[1] * ny, u0 = L.origin[0] * dx + L.origin[1] * dy;
      const vs = corners.map(([x, y]) => x * nx + y * ny), us = corners.map(([x, y]) => x * dx + y * dy);
      const k0 = Math.ceil((Math.min(...vs) - v0) / sp), k1 = Math.floor((Math.max(...vs) - v0) / sp), umin = Math.min(...us), umax = Math.max(...us);
      if (k1 - k0 > 400) continue;
      const dash = (L.dashes || []).map((d) => Math.max(d * k, 0.5)), period = dash.reduce((t, d) => t + d, 0);
      ctx.setLineDash(dash);
      for (let n = k0; n <= k1; n++) {
        const v = v0 + n * sp;
        const P = (u) => [(u * dx + v * nx) * k, (vh - (u * dy + v * ny)) * k];
        if (period) ctx.lineDashOffset = ((((umin - (u0 + n * (L.shift || 0))) * k) % period) + period) % period;
        const [x0, y0] = P(umin), [x1, y1] = P(umax);
        ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke();
      }
    }
    ctx.restore();
  }

  let previewMode = "lit";

  /** The preview: a few tiles of the generated maps, lit or one map at a time, with the matching hatch to check by, turned by `rot` degrees as the material is. */
  function preview(params, rot = 0) {
    const W = 360, H = 220;
    const canvas = h("canvas", { width: W, height: H, className: "matgen-preview" });
    const info = h("div", { className: "muted", textContent: "Generating..." });
    const modes = h("div", { className: "modal-tabs matgen-modes" }, ...[["lit", "Lit"], ["albedo", "Albedo"], ["normal", "Normal"], ["height", "Height"], ["roughness", "Roughness"], ["hatch", "Surface Pattern"]].map(([k, t]) => {
      const b = h("button", { textContent: t, className: k === previewMode ? "active" : "" });
      b.onclick = () => { previewMode = k; [...modes.children].forEach((c) => c.classList.toggle("active", c === b)); draw(); };
      return b;
    }));
    let got = null;
    async function draw() {
      if (!got) return;
      const ctx = canvas.getContext("2d");
      // whole tiles across and up, at least 600 mm across
      const nx = Math.max(1, Math.ceil(Math.max(600, 2 * got.width_mm) / got.width_mm));
      const vw = nx * got.width_mm, k = W / vw;
      const ny = Math.max(1, Math.ceil(H / k / got.height_mm)), vh = ny * got.height_mm;
      const src = previewMode === "lit" || previewMode === "hatch" ? (got.lit ||= litImage(got.img.albedo, got.img.normal)) : got.img[previewMode];
      ctx.clearRect(0, 0, W, H);
      const pat = ctx.createPattern(src, "repeat");
      const sw = src.naturalWidth || src.width, sh = src.naturalHeight || src.height;
      // turned about the pattern's origin, the preview's bottom left (a tile corner)
      const oy = vh * k;
      pat.setTransform(new DOMMatrix().translateSelf(0, oy).rotateSelf(-rot).translateSelf(0, -oy).scaleSelf((got.width_mm * k) / sw, (got.height_mm * k) / sh));
      ctx.fillStyle = pat; ctx.fillRect(0, 0, W, H);
      if (previewMode === "hatch") {
        ctx.fillStyle = "rgba(255,255,255,0.55)"; ctx.fillRect(0, 0, W, H);
        const r = (rot * Math.PI) / 180, c = Math.cos(r), s = Math.sin(r);
        const lines = (got.hatch || []).map((L) => ({ ...L, angle_deg: L.angle_deg + rot, origin: [L.origin[0] * c - L.origin[1] * s, L.origin[0] * s + L.origin[1] * c] }));
        drawHatch(ctx, lines, vw, vh, k, "#000");
      }
      const notes = (got.notes || []).join("; ");
      info.textContent = `Tile ${+got.width_mm.toFixed(1)} x ${+got.height_mm.toFixed(1)} mm, relief ${+got.depth_mm.toFixed(1)} mm, ${got.hatch_lines} pattern lines. Preview ${+vw.toFixed(0)} mm across.` + (notes ? ` Note: ${notes}.` : "");
    }
    mapsFor(params).then((r) => { got = r; draw(); }, (e) => { info.textContent = errText(e); });
    return h("div", { className: "matgen-preview-box" }, canvas, modes, info);
  }

  // which generator fields an archetype and layout show: [key, label, unit, {min, max, step}]
  function fieldsFor(g) {
    const a = g.archetype, l = g.layout, out = [];
    const add = (k, t, u = "mm", o = {}) => out.push([k, t, u, o]);
    const sized = !["corrugated", "trapezoidal", "standing_seam", "perforated"].includes(l);
    if (sized) {
      const names = { tile: ["Tile width", "Tile height"], wood_plank: ["Plank length", "Plank width"], wood_solid: ["Sample length", "Sample height"], masonry: ["Unit length", "Unit height"], metal: ["Panel width", "Panel height"] }[a];
      if (l === "versailles") add("unit_height", "Small square tile", "mm", { min: 1 });
      else { add("unit_length", names[0], "mm", { min: 1 }); add("unit_height", names[1], "mm", { min: 1 }); }
    }
    if (!["seamless", "corrugated", "trapezoidal", "standing_seam", "perforated", "glulam"].includes(l)) {
      add("joint_width", { tile: "Grout joint width", masonry: "Mortar joint thickness", metal: "Reveal joint width" }[a] || "Seam / gap width", "mm", { min: 0 });
      add("joint_depth", { tile: "Grout depth", masonry: "Mortar depth" }[a] || "Joint depth", "mm", { min: 0 });
    }
    const randomEnds = l === "running_bond" && a !== "masonry" && g.random_stagger;
    if (["running_bond", "cassette"].includes(l) && !randomEnds) add("offset", "Course offset", g.offset_percent ? "%" : "mm");
    if (randomEnds) add("min_stagger", "Least end-joint stagger", "mm", { min: 0 });
    if (l === "strip") add("min_stagger", "Least end-joint stagger", "mm", { min: 0 });
    if (["flemish", "english"].includes(l)) add("header_length", "Header length (0 auto)", "mm", { min: 0 });
    if (l === "chevron") add("chevron_angle", "Chevron angle", "deg", { min: 15, max: 75 });
    if (l === "board_and_batten") { add("batten_width", "Batten width"); add("batten_depth", "Batten projection"); }
    if (l === "glulam") add("lamination_width", "Lamination width");
    if (a === "wood_solid") { add("ring_spacing", "Growth ring spacing"); add("grain_angle", "Grain rotation", "deg"); }
    if (["corrugated", "trapezoidal"].includes(l)) { add("pitch", "Pitch", "mm", { min: 5 }); add("rib_depth", "Rib depth", "mm", { min: 1 }); }
    if (l === "standing_seam") { add("pan_width", "Pan width", "mm", { min: 50 }); add("seam_height", "Seam height", "mm", { min: 1 }); }
    if (l === "perforated") { add("perforation_diameter", "Hole diameter", "mm", { min: 0.5 }); add("perforation_pitch", "Hole pitch", "mm", { min: 2 }); }
    if (["tile", "wood_plank", "metal"].includes(a) && sized) add("bevel", "Edge bevel", "mm", { min: 0 });
    add("tone_variance", a === "masonry" ? "Tone variance" : "Per-unit tone variance", "%", { min: 0, max: 100 });
    if (a === "masonry") { add("pitting", "Roughness / pitting", "%", { min: 0, max: 100 }); add("burn_variance", "Kiln burn variance", "%", { min: 0, max: 100 }); }
    if (a === "wood_plank" || a === "wood_solid") { add("grain_density", "Grain density", "%", { min: 0, max: 100 }); }
    if (a === "wood_plank") { add("knot_frequency", "Knot frequency", "%", { min: 0, max: 100 }); add("plank_variation", "Plank colour variation", "%", { min: 0, max: 100 }); }
    if (a === "metal") add("brushed", "Brushed grain", "%", { min: 0, max: 100 });
    return out;
  }

  const sect = (t) => h("tr", { className: "sect" }, h("td", { colSpan: 3, textContent: t }));
  const row = (t, el, unit = "") => h("tr", {}, h("td", { textContent: t }), h("td", {}, el), h("td", { className: "muted", textContent: unit }));
  const selectOf = (opts, value, onchange) => {
    const s = h("select");
    for (const o of opts) s.add(new Option(label(o), o));
    s.value = value; s.onchange = () => onchange(s.value);
    return s;
  };
  const check = (v, onchange, text) => {
    const i = h("input", { type: "checkbox", checked: !!v });
    i.onchange = () => onchange(i.checked);
    return text ? h("label", { className: "check" }, i, " " + text) : i;
  };

  /**
   * The Appearance tab. ctx: { mat (the material), id, patch (merge patch, redraws), colorInput, numberInput, msg }.
   */
  async function appearancePane(pane, ctx) {
    const P = await getPresets();
    const m = ctx.mat, a = m.appearance || {};
    const t = h("table", { className: "range" });
    const kind = selectOf(["standard", "parametric"], a.kind || "standard", (v) => {
      if (v === "parametric") {
        const g = a.generator || P.archetypes.find((x) => x.id === archetypeFor(m.class)).preset;
        ctx.patch({ appearance: { kind: "parametric", generator: g }, graphics: { sync_with_appearance: true, pattern_from_appearance: true } });
      } else ctx.patch({ appearance: { kind: "standard" } });
    });
    kind.options[0].text = "Standard (image maps)"; kind.options[1].text = "Parametric (generated)";
    t.append(row("Type", kind));
    const num = (v, on, o = {}) => ctx.numberInput(v, on, { width: "90px", ...o });
    const ap = (patchObj) => ctx.patch({ appearance: patchObj });
    if ((a.kind || "standard") === "parametric") {
      const g = a.generator;
      const gp = (o) => ap({ generator: o });
      pane.append(preview(g, a.uv_rotation_deg || 0));
      t.append(sect("Generator"));
      t.append(row("Archetype", selectOf(P.archetypes.map((x) => x.id), g.archetype, (v) => gp(P.archetypes.find((x) => x.id === v).preset))));
      const arch = P.archetypes.find((x) => x.id === g.archetype);
      t.append(row("Layout", selectOf(arch.layouts, g.layout, (v) => gp({ layout: v }))));
      // the whole material turned (herringbone at 45 degrees, boards running up a wall); the pattern turns with it
      const turn = h("span", {}, num(a.uv_rotation_deg || 0, (v) => ap({ uv_rotation_deg: v }), { width: "70px", step: 15 }), " ",
        ...[0, 45, 90].map((d) => h("button", { textContent: String(d), title: `Turn the material ${d} degrees`, onclick: () => ap({ uv_rotation_deg: d }) })));
      t.append(row("Rotation", turn, "deg"));
      if (g.layout === "running_bond" && g.archetype !== "masonry") {
        t.append(row("End joints", selectOf(["regular", "random"], g.random_stagger ? "random" : "regular", (v) => gp({ random_stagger: v === "random" }))));
      }
      const sizes = P.unit_presets[g.archetype];
      if (sizes && !["corrugated", "trapezoidal", "standing_seam", "perforated", "versailles"].includes(g.layout)) {
        const s = h("select");
        s.add(new Option("Custom", ""));
        for (const [n, l, hh] of sizes) s.add(new Option(n, `${l},${hh}`));
        s.value = sizes.some(([, l, hh]) => l === g.unit_length && hh === g.unit_height) ? `${g.unit_length},${g.unit_height}` : "";
        s.onchange = () => { if (!s.value) return; const [l, hh] = s.value.split(",").map(Number); gp({ unit_length: l, unit_height: hh }); };
        t.append(row("Size preset", s));
      }
      for (const [k, txt, unit, o] of fieldsFor(g)) {
        if (k === "offset") {
          const box = h("span", {}, num(g.offset, (v) => gp({ offset: v })), " ", check(g.offset_percent, (on) => gp({ offset_percent: on }), "% of a unit"));
          t.append(row(txt, box, ""));
          continue;
        }
        t.append(row(txt, num(g[k], (v) => gp({ [k]: o.min !== undefined ? Math.max(o.min, o.max !== undefined ? Math.min(o.max, v) : v) : v }), o), unit));
        if (k === "unit_height" && g.layout === "versailles") {
          const m = g.unit_height + g.joint_width, sz = (a, b) => `${Math.round(a * m - g.joint_width)} x ${Math.round(b * m - g.joint_width)}`;
          t.append(row("", h("span", { className: "muted", textContent: `Also ${sz(2, 2)}, ${sz(1, 2)} and ${sz(2, 3)}; repeats every ${Math.round(6 * m)} mm` })));
        }
      }
      if (g.archetype === "masonry") t.append(row("Mortar profile", selectOf(P.mortar_profiles, g.mortar_profile, (v) => gp({ mortar_profile: v }))));
      t.append(row("Surface finish", selectOf(P.finishes.map((f) => f.id), g.finish, (v) => gp({ finish: v }))));
      const seed = h("span", {}, num(g.seed, (v) => gp({ seed: Math.max(0, Math.round(v)) }), { width: "70px", min: 0, step: 1 }), " ", h("button", { textContent: "New", onclick: () => gp({ seed: Math.floor(Math.random() * 100000) }) }));
      t.append(row("Variation seed", seed));
      t.append(sect("Colour"));
      t.append(row("Primary base colour", ctx.colorInput(g.primary_color, (c) => gp({ primary_color: c }))));
      t.append(row({ masonry: "Mortar colour", tile: "Grout colour" }[g.archetype] || "Joint / reveal colour", ctx.colorInput(g.joint_color, (c) => gp({ joint_color: c }))));
      t.append(row("Brightness", num(g.brightness, (v) => gp({ brightness: Math.max(0, Math.min(200, v)) }), { min: 0, max: 200 }), "%"));
      t.append(row("Saturation", num(g.saturation, (v) => gp({ saturation: Math.max(0, Math.min(200, v)) }), { min: 0, max: 200 }), "%"));
      t.append(row("Hue shift", num(g.hue_shift, (v) => gp({ hue_shift: Math.max(-180, Math.min(180, v)) }), { min: -180, max: 180 }), "deg"));
    } else {
      t.append(row("Colour", ctx.colorInput(a.color || [190, 190, 190], (c) => ap({ color: c }))));
      t.append(sect("Image Maps"));
      const maps = a.maps || {};
      const slot = (key, txt, extra) => {
        const img = maps[key];
        const load = h("button", { textContent: img ? "Replace..." : "Load..." });
        load.onclick = async () => {
          const path = await Backend.fileDialog("open", { multiple: false, filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }] });
          if (!path) return;
          try { await Backend.call("material_load_map", { id: ctx.id, slot: key, path }); ctx.msg(""); } catch (e) { ctx.msg(errText(e)); }
          ctx.patch({});
        };
        const clear = img ? h("button", { textContent: "Clear", onclick: () => ap({ maps: { [key]: null } }) }) : null;
        const name = h("span", { className: img ? "" : "muted", textContent: img ? `${img.file || "image"} (${img.px[0]} x ${img.px[1]})` : "None" });
        t.append(h("tr", {}, h("td", { textContent: txt }), h("td", {}, name, " ", load, " ", clear, extra || null), h("td")));
      };
      slot("albedo", "Albedo / diffuse");
      slot("normal", "Normal / bump", h("div", {}, check(maps.bump_is_height, (on) => ap({ maps: { bump_is_height: on } }), "Greyscale bump, not a normal map")));
      slot("height", "Height (parallax)");
      slot("roughness", "Roughness");
      slot("cutout", "Cutout");
      t.append(sect("Sample Size"));
      const uv = a.uv_size_mm || [1000, 1000];
      t.append(row("Width", num(uv[0], (v) => ap({ uv_size_mm: [Math.max(1, v), uv[1]] }), { min: 1 }), "mm"));
      t.append(row("Height", num(uv[1], (v) => ap({ uv_size_mm: [uv[0], Math.max(1, v)] }), { min: 1 }), "mm"));
      const alb = maps.albedo;
      if (alb && alb.px[0] > 0) {
        const keep = h("button", { textContent: "Match image proportions", onclick: () => ap({ uv_size_mm: [uv[0], +(uv[0] * alb.px[1] / alb.px[0]).toFixed(1)] }) });
        t.append(row("", keep));
      }
      t.append(sect("Shader"));
      t.append(row("Roughness", num(a.roughness, (v) => ap({ roughness: Math.max(0, Math.min(1, v)) }), { min: 0, max: 1, step: 0.05 }), "0 to 1"));
      t.append(row("Metalness", num(a.metalness, (v) => ap({ metalness: Math.max(0, Math.min(1, v)) }), { min: 0, max: 1, step: 0.05 }), "0 to 1"));
      t.append(row("Reflectivity", num(a.reflectivity, (v) => ap({ reflectivity: Math.max(0, Math.min(1, v)) }), { min: 0, max: 1, step: 0.01 }), "at 0 deg"));
      t.append(row("Parallax depth", num(a.pom_depth_mm, (v) => ap({ pom_depth_mm: Math.max(0, v) }), { min: 0 }), "mm"));
    }
    t.append(sect("Placement"));
    const off = a.uv_offset_mm || [0, 0];
    t.append(row("Offset across", num(off[0], (v) => ap({ uv_offset_mm: [v, off[1]] })), "mm"));
    t.append(row("Offset up", num(off[1], (v) => ap({ uv_offset_mm: [off[0], v] })), "mm"));
    if (a.kind !== "parametric") t.append(row("Rotation", num(a.uv_rotation_deg, (v) => ap({ uv_rotation_deg: v })), "deg"));
    pane.append(t);
    if (a.kind === "parametric") {
      pane.append(h("div", { className: "row" }, h("button", { textContent: "Bake to Images...", title: "Export the generated maps as image files for other renderers", onclick: () => bake(m) })));
    }
  }

  async function bake(m) {
    openModal(`Bake "${m.name}" to Images`, (body, foot, close) => {
      const res = h("select"); for (const r of [1024, 2048, 4096]) res.add(new Option(`${r} px`, r)); res.value = "2048";
      const shape = h("select"); shape.add(new Option("Square", "square")); shape.add(new Option("Tile proportions", "aspect"));
      const fmt = h("select"); fmt.add(new Option("PNG (16-bit height)", "png")); fmt.add(new Option("TIFF (16-bit height)", "tiff")); fmt.add(new Option("EXR height (32-bit float), PNG maps", "exr"));
      const msg = h("div", { className: "modal-msg" });
      body.append(h("table", { className: "range" }, row("Resolution", res), row("Shape", shape), row("Format", fmt)),
        h("p", { className: "note", textContent: "Writes Albedo, Normal, Height_POM, Roughness and Metalness images and a Metadata file with the real tile size and parallax depth." }), msg);
      foot.append(button("Choose Folder and Bake", async () => {
        const dir = await Backend.pickFolder();
        if (!dir) return;
        msg.textContent = "Baking...";
        try {
          const r = await Backend.call("material_export", { name: m.name, dir, res: +res.value, square: shape.value === "square", format: fmt.value });
          msg.textContent = `Wrote ${r.files.length} files (${r.px[0]} x ${r.px[1]}) to ${dir}.`;
        } catch (e) { msg.textContent = errText(e); }
      }, true), button("Close", close));
    });
  }

  // ------------------------------------------------------------------ the user library

  const LIB_KEY = "openarch.materialLibrary";
  async function libDir() {
    try { const d = localStorage.getItem(LIB_KEY); if (d) return d; } catch { /* storage can be unavailable */ }
    return (await Backend.call("material_library_dir")).dir;
  }
  const setLibDir = (d) => { try { localStorage.setItem(LIB_KEY, d); } catch { /* storage can be unavailable */ } };

  const libSel = new Set(); // "category/name"
  const libKey = (i) => `${i.category}/${i.name}`;

  /**
   * The library side of the Materials window's left panel. ctx: { list (element), pane, msg, afterImport, toHex }.
   */
  async function libraryList(ctx) {
    const dir = await libDir();
    let lib;
    try { lib = await Backend.call("material_library_list", { dir }); } catch (e) { ctx.msg(errText(e)); return; }
    const q = (ctx.query || "").toLowerCase();
    const kids = [];
    for (const cat of lib.categories) {
      const items = lib.items.filter((i) => i.category === cat && i.name.toLowerCase().includes(q));
      kids.push(h("div", { className: "mat-cat", textContent: cat }));
      for (const i of items) {
        const b = h("button", { className: "mat-item" + (libSel.has(libKey(i)) ? " active" : "") });
        const chip = h("span", { className: i.thumbnail ? "mat-thumb" : "mat-chip" });
        if (i.thumbnail) chip.style.backgroundImage = `url(${i.thumbnail})`; else chip.style.background = ctx.toHex(i.color);
        b.append(chip, i.name, ...(i.kind === "parametric" ? [h("span", { className: "mat-tag", textContent: "P" })] : []));
        b.title = `${i.class || "Generic"}${i.description ? ": " + i.description : ""}\n${i.path}`;
        b.onclick = (e) => {
          if (!e.ctrlKey && !e.shiftKey && !e.metaKey) libSel.clear();
          libSel.has(libKey(i)) ? libSel.delete(libKey(i)) : libSel.add(libKey(i));
          libraryList(ctx);
        };
        b.ondblclick = () => doImport(ctx, [i]);
        kids.push(b);
      }
    }
    if (!lib.items.length) kids.push(h("p", { className: "note", textContent: lib.exists ? "No materials in the library yet. Save one from the project with Save to Library." : "The library folder does not exist yet; saving a material makes it." }));
    ctx.list.replaceChildren(...kids);
    // the right side: what is picked and what can be done with it
    const pane = ctx.pane;
    pane.replaceChildren();
    const picked = lib.items.filter((i) => libSel.has(libKey(i)));
    pane.append(h("p", {}, h("b", { textContent: "User Material Library" })), h("p", { className: "note", textContent: lib.dir }));
    pane.append(h("p", { textContent: picked.length ? `${picked.length} selected: ${picked.map((i) => i.name).join(", ")}` : "Select materials to import (Ctrl+click for more than one, double-click to import one)." }));
    const replace = h("input", { type: "checkbox" });
    const imp = h("button", { textContent: `Import into Project${picked.length > 1 ? ` (${picked.length})` : ""}`, disabled: !picked.length, onclick: () => doImport(ctx, picked, replace.checked) });
    const del = h("button", { textContent: "Delete from Library", disabled: !picked.length, onclick: async () => {
      if (!(await confirmBox(`Delete ${picked.length} material${picked.length > 1 ? "s" : ""} from the library? Projects that use them keep their own copies.`))) return;
      for (const i of picked) { try { await Backend.call("material_library_delete", { dir, category: i.category, name: i.name }); } catch (e) { ctx.msg(errText(e)); } }
      libSel.clear(); libraryList(ctx);
    } });
    const folder = h("button", { textContent: "Library Folder...", title: "Choose the folder (local or network) the library is kept in", onclick: async () => {
      const d = await Backend.pickFolder();
      if (d) { setLibDir(d); libSel.clear(); libraryList(ctx); }
    } });
    const newCat = h("button", { textContent: "New Folder...", onclick: async () => {
      const n = await Backend.pathPrompt("New library folder (category)", "");
      if (!n) return;
      try { await Backend.call("material_library_add_category", { dir, category: n }); } catch (e) { ctx.msg(errText(e)); }
      libraryList(ctx);
    } });
    const move = h("button", { textContent: "Move to Folder...", disabled: !picked.length, onclick: () => openModal("Move to Folder", (body, foot, close) => {
      const to = h("input", { value: lib.categories.find((c) => !picked.some((i) => i.category === c)) || "", list: "matlib-move" });
      const dl = h("datalist", { id: "matlib-move" }, ...lib.categories.map((c) => h("option", { value: c })));
      const msg = h("div", { className: "modal-msg" });
      body.append(h("table", { className: "range" }, row("Folder", h("span", {}, to, dl))), h("p", { className: "note", textContent: "A new name makes a new folder. Project materials that came from these keep naming their old folder." }), msg);
      foot.append(button("Move", async () => {
        try {
          const r = await Backend.call("material_library_move", { dir, to: to.value.trim(), items: picked.map((i) => ({ category: i.category, name: i.name })) });
          close(); libSel.clear();
          ctx.msg(r.skipped.length ? `Moved ${r.moved.length}; already in ${to.value.trim()}: ${r.skipped.join(", ")}.` : `Moved ${r.moved.length} to ${to.value.trim()}.`);
          libraryList(ctx);
        } catch (e) { msg.textContent = errText(e); }
      }, true), button("Cancel", close));
    }) });
    pane.append(h("div", { className: "row" }, imp, del, move), h("label", { className: "check" }, replace, " Replace project materials of the same name"), h("div", { className: "row" }, folder, newCat));
  }

  async function doImport(ctx, items, replace = false) {
    try {
      const r = await Backend.call("material_library_import", { dir: await libDir(), items: items.map((i) => ({ category: i.category, name: i.name })), replace });
      const bits = [];
      if (r.added.length) bits.push(`Imported ${r.added.join(", ")}.`);
      if (r.replaced.length) bits.push(`Replaced ${r.replaced.join(", ")}.`);
      if (r.skipped.length) bits.push(`Already in the project (left as they are): ${r.skipped.join(", ")}.`);
      ctx.msg(bits.join(" "));
      await ctx.afterImport(r.added[0] || r.replaced[0]);
    } catch (e) { ctx.msg(errText(e)); }
  }

  function confirmBox(text) {
    return new Promise((ok) => openModal("Confirm", (body, foot, close) => {
      body.append(h("p", { textContent: text }));
      foot.append(button("OK", () => { close(); ok(true); }, true), button("Cancel", () => { close(); ok(false); }));
    }));
  }

  /** Save to Library and Update Library Asset buttons for a project material. ctx: { mat, id, msg, refresh }. */
  async function libraryButtons(ctx) {
    const m = ctx.mat;
    const save = h("button", { textContent: "Save to Library...", title: "Copy this material into the user material library" });
    save.onclick = async () => {
      const dir = await libDir();
      let lib = { categories: [] };
      try { lib = await Backend.call("material_library_list", { dir }); } catch { /* a new library */ }
      openModal("Save to Library", (body, foot, close) => {
        const cat = h("input", { value: (m.library_source && m.library_source.category) || lib.categories[0] || m.class || "Materials", list: "matlib-cats" });
        const dl = h("datalist", { id: "matlib-cats" }, ...lib.categories.map((c) => h("option", { value: c })));
        const name = h("input", { value: m.name });
        const replace = h("input", { type: "checkbox" });
        const msg = h("div", { className: "modal-msg" });
        body.append(h("table", { className: "range" }, row("Folder", h("span", {}, cat, dl)), row("Name", name), row("", h("label", { className: "check" }, replace, " Replace one of the same name"))), h("p", { className: "note", textContent: dir }), msg);
        foot.append(button("Save", async () => {
          try {
            await Backend.call("material_library_save", { dir, id: ctx.id, category: cat.value.trim(), as_name: name.value.trim(), replace: replace.checked });
            close(); ctx.msg(`Saved to the library folder ${cat.value.trim()}.`); ctx.refresh();
          } catch (e) { msg.textContent = errText(e); }
        }, true), button("Cancel", close));
      });
    };
    const upd = h("button", { textContent: "Update Library Asset", disabled: !m.library_source, title: m.library_source ? `Write this material over ${m.library_source.category} / ${m.library_source.name} in the library` : "This material did not come from the library" });
    upd.onclick = async () => {
      if (!(await confirmBox(`Overwrite ${m.library_source.category} / ${m.library_source.name} in the library with this project's version?`))) return;
      try { await Backend.call("material_library_update", { dir: await libDir(), id: ctx.id }); ctx.msg("Library asset updated."); } catch (e) { ctx.msg(errText(e)); }
    };
    const src = m.library_source ? h("span", { className: "muted", textContent: ` From library: ${m.library_source.category} / ${m.library_source.name}` }) : null;
    return h("div", { className: "row" }, save, upd, src);
  }

  return { appearancePane, libraryList, libraryButtons, label };
})();
