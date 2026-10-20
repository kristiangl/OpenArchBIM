// Type Properties and Edit Assembly.
//
// One Type Properties dialog serves every kind of type: loaded families, walls, floors, roofs,
// grids and levels. Edits reach the model as they are made, so the dialog can show what follows
// from them (a wall's width and R value after Edit Assembly), but the views only redraw on Apply or
// OK, and Cancel (or Escape) undoes everything since the last Apply, duplicates and renames
// included. When the dialog was opened for a selection or a placing tool, OK and Apply also put
// that selection or tool on the type shown.

function namePrompt(title, initial, ok) {
  openModal(title, (body, foot, close) => {
    const inp = document.createElement("input");
    inp.type = "text"; inp.id = "type-name"; inp.style.width = "100%"; inp.value = initial || "";
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(inp, msg);
    const go = async () => {
      try { await ok(inp.value.trim()); } catch (e) { msg.textContent = String(e.message || e); return; }
      close();
    };
    inp.onkeydown = (e) => { if (e.key === "Enter") go(); };
    foot.append(button("OK", go, true), button("Cancel", close));
    inp.focus(); inp.select();
  });
}

async function openTypeProperties(typeId, { onType = null } = {}) {
  if (!typeId) return;
  let mark;
  try { mark = (await Backend.call("edit_begin", {})).mark; } catch (e) { return; }
  let id = typeId, appliedId = typeId, dirty = false, d = null;
  openModal("Type Properties", (body, foot, closeModal) => {
    const head = document.createElement("div");
    head.className = "tp-head";
    const content = document.createElement("div");
    content.className = "tp-params";
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    body.append(head, content, msg);
    const okB = button("OK", () => commit(true), true);
    const cancelB = button("Cancel", () => cancel());
    const applyB = button("Apply", () => commit(false));
    applyB.id = "tp-apply";
    const setDirty = (v) => { dirty = v; applyB.disabled = !dirty && id === appliedId; };
    foot.append(okB, cancelB, applyB);

    const commit = async (close) => {
      try {
        if (id !== appliedId && onType) await onType(id);
        mark = (await Backend.call("edit_commit", { mark, name: "Type Properties" })).mark;
      } catch (e) { msg.textContent = String(e.message || e); return; }
      appliedId = id;
      setDirty(false);
      await refresh();
      if (close) closeModal(); else await load();
    };
    const cancel = async () => {
      try { await Backend.call("edit_cancel", { mark }); } catch (e) { /* nothing to undo */ }
      closeModal();
      await refresh();
    };
    // Escape and the dialog's own close go through Cancel
    body.closest(".modal-back").onEscape = cancel;

    const changed = async (fn) => {
      try { await fn(); msg.textContent = ""; setDirty(true); } catch (e) { msg.textContent = String(e.message || e); }
      await load();
    };
    const target = {
      apply: (patch) => changed(() => Backend.call(d.patch, { id, patch })),
      dialogs: { assembly: () => openEditAssembly(d, (patch) => target.apply(patch)) },
    };

    const load = async () => {
      try { d = await Backend.call("describe_any_type", { id }); } catch (e) { msg.textContent = String(e.message || e); return; }
      head.replaceChildren();
      // Family: the families that could stand in (loaded families of the same category)
      const famSel = document.createElement("select");
      famSel.id = "tp-family";
      const fams = d.family_id != null ? (app.info.families || []).filter((f) => f.category === (app.info.families.find((x) => x.id === d.family_id) || {}).category) : [];
      if (fams.length) {
        for (const f of fams) famSel.add(new Option(f.name, f.id, false, f.id === d.family_id));
        famSel.onchange = () => {
          const t = (app.info.family_types || []).find((x) => x.family_id === Number(famSel.value));
          if (t) { id = t.id; setDirty(dirty); load(); }
        };
      } else {
        famSel.add(new Option(d.family || "", ""));
        famSel.disabled = true;
      }
      const typeSel = document.createElement("select");
      typeSel.id = "tp-type";
      for (const s of d.siblings) typeSel.add(new Option(s.name, s.id, false, s.id === id));
      typeSel.onchange = () => { id = Number(typeSel.value); msg.textContent = ""; setDirty(dirty); load(); };
      const dup = button("Duplicate...", () => {
        const base = d.name || "";
        namePrompt("Name", /\d+$/.test(base) ? base.replace(/\d+$/, (n) => String(Number(n) + 1)) : `${base} 2`, async (name) => {
          const r = await Backend.call(d.duplicate, { id, name });
          id = r.id; setDirty(true); await load();
        });
      });
      dup.id = "tp-duplicate";
      const ren = button("Rename...", () => namePrompt("Rename", d.name, async (name) => {
        await Backend.call("rename_type", { id, name }); setDirty(true); await load();
      }));
      ren.id = "tp-rename";
      const row = (label, ctl, ...btns) => {
        const r = document.createElement("div");
        r.className = "tp-row";
        const l = document.createElement("label");
        l.textContent = label;
        const b = document.createElement("div");
        b.className = "tp-btns";
        b.append(...btns);
        r.append(l, ctl, b);
        return r;
      };
      const extra = [];
      // families made in set sizes list them in a catalogue
      const family = d.family_id != null ? app.info.families.find((f) => f.id === d.family_id) : null;
      const have = new Set(d.siblings.map((x) => x.name));
      const missing = family && family.catalog ? family.catalog.filter((n) => !have.has(n)) : [];
      if (missing.length) extra.push(button("Add from catalogue...", () => catalogPrompt(family, missing, async (newId) => { id = newId; setDirty(true); await load(); })));
      if (d.kind === "LevelType" && d.head) extra.push(button("Edit Symbol Family...", () => FamEd.open(d.head, async () => { setDirty(true); await load(); })));
      head.append(row("Family:", famSel), row("Type:", typeSel, dup, ren, ...extra));
      const cols = document.createElement("table");
      cols.className = "props tp-cols";
      const hr = cols.insertRow();
      for (const t of ["Parameter", "Value"]) { const c = hr.insertCell(); c.textContent = t; }
      content.replaceChildren(cols, ...d.groups.map((g) => groupTable(g.name, g.rows, target)));
      setDirty(dirty);
    };
    load();
  }, { wide: false });
}

// ---- Edit Assembly -----------------------------------------------------------------
// a layer's function, and the join priority it gives unless the layer sets its own (a membrane has none)
const LAYER_FUNCTIONS = [
  ["structure", "Structure", 1], ["substrate", "Substrate", 2], ["thermal_air", "Thermal/Air Layer", 3],
  ["finish1", "Finish 1", 4], ["finish2", "Finish 2", 5], ["membrane", "Membrane Layer", null],
];

async function openEditAssembly(t, onOk) {
  const kind = t.sys_kind;
  const wall = kind === "wall";
  let std = null;
  try { std = await Backend.call("standards"); } catch (e) { std = { materials: [] }; }
  const mats = std.materials.map((m) => m.material);
  const shade = (name) => { const m = mats.find((x) => x.name === name); const c = m && m.graphics && m.graphics.shading_color; return c ? `rgb(${c[0]},${c[1]},${c[2]})` : "#d8d8d8"; };
  // the rows: layers, with the two core boundaries in among them
  const rows = [];
  const firstCore = t.layers.findIndex((l) => l.core);
  const lastCore = t.layers.length - 1 - [...t.layers].reverse().findIndex((l) => l.core);
  t.layers.forEach((l, i) => {
    if (i === (firstCore < 0 ? 0 : firstCore)) rows.push({ b: "top" });
    rows.push({ l: JSON.parse(JSON.stringify(l)) });
    if (i === (firstCore < 0 ? t.layers.length - 1 : lastCore)) rows.push({ b: "bottom" });
  });
  const opts = { wrap_ends: t.wall ? t.wall.wrap_ends : "none", wrap_inserts: t.wall ? t.wall.wrap_inserts : "none", sample_height_mm: t.wall ? t.wall.sample_height_mm : 6000 };
  let selRow = rows.findIndex((r) => r.l);
  let info = null, previewOn = false, previewView = wall ? "plan" : "section";
  const layers = () => {
    const out = [];
    let core = false;
    for (const r of rows) {
      if (r.b) { core = r.b === "top"; continue; }
      out.push({ ...r.l, core });
    }
    return out;
  };

  openModal("Edit Assembly", (body, foot, close) => {
    body.closest(".modal").classList.add("assembly");
    const wrap = document.createElement("div");
    wrap.className = "asm";
    const pv = document.createElement("div");
    pv.className = "asm-preview";
    pv.hidden = true;
    const canvas = document.createElement("canvas");
    canvas.width = 420; canvas.height = 460;
    canvas.id = "asm-canvas";
    pv.append(canvas);
    const main = document.createElement("div");
    main.className = "asm-main";
    wrap.append(pv, main);
    const msg = document.createElement("div");
    msg.className = "modal-msg";
    msg.id = "asm-msg";
    body.append(wrap, msg);

    const facts = document.createElement("div");
    facts.className = "asm-facts";
    const table = document.createElement("table");
    table.className = "asm-layers";
    table.id = "asm-layers";
    const sideTop = document.createElement("div"), sideBot = document.createElement("div");
    sideTop.className = sideBot.className = "asm-side";
    sideTop.textContent = t.sides[0]; sideBot.textContent = t.sides[1];
    const btns = document.createElement("div");
    btns.className = "asm-btns";
    const bInsert = button("Insert", () => { rows.splice(Math.max(0, selRow), 0, { l: { material: "", thickness_mm: 0, is_void: false, function: "structure", core: false, wraps: true, structural_material: false, variable: false, priority: null } }); update(); });
    const bDup = button("Duplicate", () => { const r = rows[selRow]; if (!r || !r.l) return; rows.splice(selRow + 1, 0, { l: { ...r.l, structural_material: false, variable: false } }); selRow++; update(); });
    const bDel = button("Delete", () => { if (rows[selRow] && rows[selRow].l) { rows.splice(selRow, 1); selRow = Math.min(selRow, rows.length - 1); update(); } });
    const move = (dir) => {
      const j = selRow + dir;
      if (j < 0 || j >= rows.length) return;
      if (rows[selRow].b && rows[j].b) return; // the core keeps at least its place
      [rows[selRow], rows[j]] = [rows[j], rows[selRow]];
      selRow = j;
      update();
    };
    const bUp = button("Up", () => move(-1)), bDown = button("Down", () => move(1));
    bInsert.id = "asm-insert"; bDup.id = "asm-duplicate"; bDel.id = "asm-delete"; bUp.id = "asm-up"; bDown.id = "asm-down";
    btns.append(bInsert, bDup, bDel, bUp, bDown);
    main.append(facts, sideTop, table, sideBot, btns);

    if (wall) {
      const wrapBox = document.createElement("fieldset");
      wrapBox.className = "asm-box";
      wrapBox.innerHTML = "<legend>Default Wrapping</legend>";
      const sel = (id, label, key, choices) => {
        const l = document.createElement("label");
        l.textContent = label + " ";
        const s = document.createElement("select");
        s.id = id;
        for (const [v, n] of choices) s.add(new Option(n, v, false, opts[key] === v));
        s.onchange = () => { opts[key] = s.value; update(); };
        l.append(s);
        return l;
      };
      wrapBox.append(sel("asm-wrap-inserts", "At Inserts:", "wrap_inserts", [["none", "Do not wrap"], ["exterior", "Exterior"], ["interior", "Interior"], ["both", "Both"]]),
        sel("asm-wrap-ends", "At Ends:", "wrap_ends", [["none", "None"], ["exterior", "Exterior"], ["interior", "Interior"]]));
      const vert = document.createElement("fieldset");
      vert.className = "asm-box";
      vert.innerHTML = "<legend>Modify Vertical Structure (Section Preview only)</legend>";
      for (const n of ["Modify", "Merge Regions", "Sweeps", "Assign Layers", "Split Region", "Reveals"]) {
        const b = button(n, () => {});
        b.disabled = true;
        b.title = "Vertically compound walls are not available yet";
        vert.append(b);
      }
      main.append(wrapBox, vert);
    }

    const fnSelect = (l) => {
      const s = document.createElement("select");
      for (const [v, n] of LAYER_FUNCTIONS) s.add(new Option(n, v, false, l.function === v));
      // a new function brings its own priority
      s.onchange = () => { l.function = s.value; l.priority = null; if (l.function === "membrane") { l.thickness_mm = 0; l.variable = false; } update(); };
      return s;
    };
    const prioSelect = (l) => {
      const def = (LAYER_FUNCTIONS.find((f) => f[0] === l.function) || [])[2];
      if (def == null) return document.createTextNode("");
      const s = document.createElement("select");
      s.className = "prio";
      for (let k = 1; k <= 5; k++) s.add(new Option(String(k), k, false, (l.priority ?? def) === k));
      s.onchange = () => { const k = Number(s.value); l.priority = k === def ? null : k; update(); };
      return s;
    };
    const matSelect = (l) => {
      const s = document.createElement("select");
      s.add(new Option("<By Category>", "", false, !l.is_void && !l.material));
      for (const m of mats) s.add(new Option(m.name, m.name, false, !l.is_void && l.material === m.name));
      // a cavity carries no material
      s.add(new Option("<Air gap>", "\u0000void", false, !!l.is_void));
      if (!l.is_void && l.material && !mats.some((m) => m.name === l.material)) s.add(new Option(l.material, l.material, false, true));
      s.onchange = () => {
        if (s.value === "\u0000void") { l.is_void = true; l.material = l.material || "Cavity"; l.structural_material = false; } else { l.is_void = false; l.material = s.value; }
        update();
      };
      return s;
    };
    const check = (on, enabled, set) => {
      const c = document.createElement("input");
      c.type = "checkbox"; c.checked = !!on; c.disabled = !enabled;
      c.onchange = () => { set(c.checked); update(); };
      return c;
    };

    const renderTable = () => {
      table.replaceChildren();
      const hr = table.insertRow();
      hr.className = "head";
      for (const h of ["", "Function", "Priority", "Material", "Thickness", "Wraps", "Structural Material", "Variable"]) { const c = document.createElement("th"); c.textContent = h; hr.append(c); }
      let core = false;
      rows.forEach((r, i) => {
        const tr = table.insertRow();
        tr.className = (i === selRow ? "sel " : "") + (r.b ? "boundary" : core ? "core" : "");
        tr.onclick = (e) => { if (selRow !== i) { selRow = i; if (!["INPUT", "SELECT", "OPTION"].includes(e.target.tagName)) renderTable(); else table.querySelectorAll("tr").forEach((x, k) => x.classList.toggle("sel", k - 1 === i)); updateButtons(); } };
        const num = tr.insertCell();
        num.textContent = String(i + 1);
        num.className = "n";
        if (r.b) {
          core = r.b === "top";
          tr.insertCell().textContent = "Core Boundary";
          tr.insertCell();
          tr.insertCell().textContent = r.b === "top" ? "Layers Above Wrap" : "Layers Below Wrap";
          tr.insertCell().textContent = "0.0";
          for (let k = 0; k < 3; k++) tr.insertCell();
          return;
        }
        const l = r.l;
        tr.insertCell().append(fnSelect(l));
        tr.insertCell().append(prioSelect(l));
        tr.insertCell().append(matSelect(l));
        const th = document.createElement("input");
        th.type = "number"; th.step = "any"; th.min = "0"; th.value = l.thickness_mm; th.className = "thk";
        th.disabled = l.function === "membrane";
        th.onchange = () => { const v = parseFloat(th.value); if (Number.isFinite(v)) { l.thickness_mm = v; update(); } };
        tr.insertCell().append(th);
        tr.insertCell().append(check(l.wraps, wall && !core, (v) => { l.wraps = v; }));
        tr.insertCell().append(check(l.structural_material, core && !l.is_void, (v) => { for (const o of rows) if (o.l) o.l.structural_material = false; l.structural_material = v; }));
        // walls have no variable layer: the column is there, greyed out
        tr.insertCell().append(check(l.variable, !wall && l.function !== "membrane", (v) => { for (const o of rows) if (o.l) o.l.variable = false; l.variable = v; }));
      });
      updateButtons();
    };
    const updateButtons = () => {
      const r = rows[selRow];
      bDup.disabled = bDel.disabled = !r || !!r.b;
      bUp.disabled = selRow <= 0 || (r && r.b && rows[selRow - 1].b);
      bDown.disabled = selRow < 0 || selRow >= rows.length - 1 || (r && r.b && rows[selRow + 1].b);
    };

    const fmt = (x, n) => (x == null ? "" : x.toFixed(n));
    const renderFacts = () => {
      const v = info ? info.values : t.values;
      facts.replaceChildren();
      const line = (label, value, id) => { const r = document.createElement("div"); const a = document.createElement("span"); a.textContent = label; const b = document.createElement("span"); b.textContent = value; if (id) b.id = id; r.append(a, b); return r; };
      facts.append(line("Family:", t.family), line("Type:", t.name), line(wall ? "Total thickness:" : "Total thickness:", `${fmt(v.thickness_mm, 1)}`, "asm-thickness"),
        line("Resistance (R):", v.r_m2k_w == null ? "" : `${fmt(v.r_m2k_w, 4)} (m²·K)/W`, "asm-r"),
        line("Thermal Mass:", v.thermal_mass_kj_m2k == null ? "" : `${fmt(v.thermal_mass_kj_m2k, 2)} kJ/(m²·K)`, "asm-mass"));
      if (wall) {
        const r = document.createElement("div");
        const a = document.createElement("span"); a.textContent = "Sample Height:";
        const inp = document.createElement("input"); inp.type = "number"; inp.value = opts.sample_height_mm; inp.id = "asm-sample";
        inp.onchange = () => { const x = parseFloat(inp.value); if (Number.isFinite(x)) { opts.sample_height_mm = x; drawPreview(); } };
        r.append(a, inp);
        facts.append(r);
      }
      if (v.missing_thermal && v.missing_thermal.length) {
        const n = document.createElement("div");
        n.className = "note";
        n.textContent = `No thermal data for: ${v.missing_thermal.join(", ")}`;
        facts.append(n);
      }
    };

    // ---- preview: the plan sample (walls) or a section through the layers
    let zoom = 1, pan = [0, 0];
    const drawPreview = () => {
      if (pv.hidden) return;
      const ctx = canvas.getContext("2d");
      const W = canvas.width, H = canvas.height;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, W, H);
      const ls = layers();
      const total = ls.reduce((s, l) => s + l.thickness_mm, 0);
      if (!total) return;
      let polys = [];
      let bounds;
      if (previewView === "plan" && info && info.plan) {
        polys = info.plan.layers.map((g) => ({ g, rings: g.shapes.flat() }));
        const [l, r] = info.plan.extents;
        bounds = [-150, -r - 150, info.plan.length + 150, l + 150];
      } else if (wall) {
        // a vertical section: exterior on the left, sample height tall
        let x = 0;
        const h = Math.max(100, opts.sample_height_mm);
        polys = ls.map((l) => { const ring = [[x, 0], [x + l.thickness_mm, 0], [x + l.thickness_mm, h], [x, h]]; x += l.thickness_mm; return { g: l, rings: [ring] }; });
        bounds = [-total, -200, total * 2, h + 200];
      } else {
        // floors and roofs: a slice, top face up
        let y = 0;
        const len = Math.max(1200, total * 3);
        polys = ls.map((l) => { const ring = [[0, y], [len, y], [len, y - l.thickness_mm], [0, y - l.thickness_mm]]; y -= l.thickness_mm; return { g: l, rings: [ring] }; });
        bounds = [-100, -total - len * 0.3, len + 100, len * 0.3];
      }
      const bw = bounds[2] - bounds[0], bh = bounds[3] - bounds[1];
      const s = Math.min(W / bw, H / bh) * zoom;
      const ox = W / 2 - ((bounds[0] + bounds[2]) / 2) * s + pan[0], oy = H / 2 + ((bounds[1] + bounds[3]) / 2) * s + pan[1];
      const P = (p) => [ox + p[0] * s, oy - p[1] * s];
      const path = (ring) => { ctx.beginPath(); ring.forEach((p, k) => { const q = P(p); if (k) ctx.lineTo(q[0], q[1]); else ctx.moveTo(q[0], q[1]); }); ctx.closePath(); };
      for (const { g, rings } of polys) {
        ctx.fillStyle = g.is_void ? "#ffffff" : shade(g.material);
        for (const r of rings) { path(r); ctx.fill(); }
      }
      ctx.strokeStyle = "#444"; ctx.lineWidth = 0.75;
      for (const { rings } of polys) for (const r of rings) { path(r); ctx.stroke(); }
      // the core boundaries, dashed
      ctx.setLineDash([6, 3]); ctx.strokeStyle = "#2e6f9e"; ctx.lineWidth = 1;
      const coreEdges = [];
      let acc = 0;
      ls.forEach((l, i) => { if (l.core && (i === 0 || !ls[i - 1].core)) coreEdges.push(acc); acc += l.thickness_mm; if (l.core && (i === ls.length - 1 || !ls[i + 1].core)) coreEdges.push(acc); });
      for (const c of coreEdges) {
        ctx.beginPath();
        if (previewView === "plan" && info && info.plan) { const y = info.plan.extents[0] - c; const a = P([bounds[0], y]), b = P([bounds[2], y]); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); }
        else if (wall) { const a = P([c, bounds[1]]), b = P([c, bounds[3]]); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); }
        else { const a = P([bounds[0], -c]), b = P([bounds[2], -c]); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); }
        ctx.stroke();
      }
      ctx.setLineDash([]);
      ctx.fillStyle = "#333"; ctx.font = "11px sans-serif";
      if (previewView === "plan") { ctx.fillText("Exterior", 8, 14); ctx.fillText("Interior", 8, H - 8); }
      else if (wall) { ctx.fillText("Exterior", 8, 14); ctx.fillText("Interior", W - 52, 14); }
      else { ctx.fillText("Top", 8, 14); ctx.fillText("Bottom", 8, H - 8); }
    };
    canvas.onwheel = (e) => { e.preventDefault(); zoom *= e.deltaY < 0 ? 1.2 : 1 / 1.2; drawPreview(); };
    let drag = null;
    canvas.onmousedown = (e) => { drag = [e.clientX - pan[0], e.clientY - pan[1]]; };
    window.addEventListener("mouseup", () => { drag = null; });
    canvas.onmousemove = (e) => { if (drag) { pan = [e.clientX - drag[0], e.clientY - drag[1]]; drawPreview(); } };

    let seq = 0;
    const update = async () => {
      renderTable();
      const n = ++seq;
      try { info = await Backend.call("assembly_info", { kind, layers: layers(), wrap_ends: opts.wrap_ends, wrap_inserts: opts.wrap_inserts }); } catch (e) { info = null; msg.textContent = String(e.message || e); return; }
      if (n !== seq) return;
      msg.textContent = info.error || "";
      renderFacts();
      drawPreview();
    };

    const pvBtn = button("Preview >>", () => {
      previewOn = !previewOn;
      pv.hidden = !previewOn;
      pvBtn.textContent = previewOn ? "<< Preview" : "Preview >>";
      viewSel.hidden = !previewOn;
      drawPreview();
    });
    pvBtn.id = "asm-preview";
    const viewSel = document.createElement("select");
    viewSel.id = "asm-view";
    viewSel.hidden = true;
    if (wall) viewSel.add(new Option("View: Floor Plan", "plan", false, true));
    viewSel.add(new Option("View: Section", "section", false, !wall));
    viewSel.onchange = () => { previewView = viewSel.value; zoom = 1; pan = [0, 0]; drawPreview(); };
    const spacer = document.createElement("span");
    spacer.style.flex = "1";
    const ok = async () => {
      if (info && info.error) { msg.textContent = info.error; return; }
      const patch = { layers: layers() };
      if (wall) Object.assign(patch, { wrap_ends: opts.wrap_ends, wrap_inserts: opts.wrap_inserts, sample_height_mm: opts.sample_height_mm });
      close();
      await onOk(patch);
    };
    const okB = button("OK", ok, true);
    okB.id = "asm-ok";
    foot.append(pvBtn, viewSel, spacer, okB, button("Cancel", close));
    renderFacts();
    update();
  }, { wide: true });
}
