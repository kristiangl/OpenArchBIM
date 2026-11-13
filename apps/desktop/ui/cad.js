// CAD drawings: Insert > Import CAD and Link CAD (DXF and DWG), how a floor plan draws an import under (or over) the
// model, picking and snapping to its lines, its Layers box and Reload. The core reads the file and keeps the drawing;
// the page fetches it once (cad_drawing) and keeps it until its stamp changes. Coordinates arrive in the drawing's own
// units about the middle of the drawing, so a survey far from its origin still draws exactly.
const Cad = (() => {
  const list = (d) => d?.cad_imports || [];
  const cache = new Map(); // import id -> { stamp, d, groups, texts, points, segs, segLayer }
  const fetching = new Set();
  const placed = new Map(); // import id -> { key, segs (model mm), box, snap }
  const SNAP_LIMIT = 150000; // segments past this are drawn and picked but not snapped to
  const ACCENT = "#2e6f9e";

  function select(id) {
    app.selection = [id]; Plan.setSelection([id]); app.vp?.setSelection([id]);
    renderProperties();
    updateRibbonContext();
  }

  // ---- the drawing, fetched and sorted for drawing ----------------------------------------------------------------
  // lines and arcs of one layer and colour are stroked as one path
  function build(d) {
    const groups = new Map(), texts = [], points = [], segs = [], segLayer = [];
    const group = (layer, col) => {
      const key = layer + "|" + (col ? col.join(",") : "");
      let g = groups.get(key);
      if (!g) { g = { layer, col, strokes: [], solids: [], pales: [], path: null, solid: null, pale: null }; groups.set(key, g); }
      return g;
    };
    const seg = (layer, x0, y0, x1, y1) => { segs.push(x0, y0, x1, y1); segLayer.push(layer); };
    const round = (layer, x, y, r, a0, sweep) => {
      const n = Math.max(4, Math.ceil(Math.abs(sweep) / (Math.PI * 2) * 32));
      for (let i = 0; i < n; i++) {
        const a = a0 + sweep * i / n, b = a0 + sweep * (i + 1) / n;
        seg(layer, x + r * Math.cos(a), y + r * Math.sin(a), x + r * Math.cos(b), y + r * Math.sin(b));
      }
    };
    for (const s of d.shapes) {
      const [kind, layer, col] = s;
      if (kind === 0) {
        const p = s[4], n = p.length / 2;
        group(layer, col).strokes.push(s);
        for (let i = 0; i + 1 < n; i++) seg(layer, p[2 * i], p[2 * i + 1], p[2 * i + 2], p[2 * i + 3]);
        if (s[3] && n > 2) seg(layer, p[2 * n - 2], p[2 * n - 1], p[0], p[1]);
      } else if (kind === 1) {
        group(layer, col).strokes.push(s);
        round(layer, s[3], s[4], s[5], 0, Math.PI * 2);
      } else if (kind === 2) {
        group(layer, col).strokes.push(s);
        let sweep = (s[7] - s[6]) % (Math.PI * 2); if (sweep <= 0) sweep += Math.PI * 2;
        round(layer, s[3], s[4], s[5], s[6], sweep);
      } else if (kind === 3) {
        const lines = String(s[9]).split("\n");
        texts.push({ layer, col, x: s[3], y: s[4], h: s[5], a: s[6], ha: s[7], va: s[8], lines });
        // picked along its first line
        const len = s[5] * 0.6 * Math.max(1, lines[0].length), off = s[7] === 1 ? -len / 2 : s[7] === 2 ? -len : 0;
        seg(layer, s[3] + off * Math.cos(s[6]), s[4] + off * Math.sin(s[6]), s[3] + (off + len) * Math.cos(s[6]), s[4] + (off + len) * Math.sin(s[6]));
      } else if (kind === 4) {
        (s[3] ? group(layer, col).pales : group(layer, col).solids).push(s);
      } else if (kind === 5) {
        points.push({ layer, col, x: s[3], y: s[4] });
        seg(layer, s[3], s[4], s[3], s[4]);
      }
    }
    return { stamp: d.stamp, d, groups: [...groups.values()], texts, points, segs: Float64Array.from(segs), segLayer: Int32Array.from(segLayer) };
  }

  // a shape's outline on a canvas or a path (both take moveTo, lineTo, arc and closePath)
  function emit(t, s) {
    const kind = s[0];
    if (kind === 0) {
      const p = s[4];
      t.moveTo(p[0], p[1]);
      for (let i = 2; i < p.length; i += 2) t.lineTo(p[i], p[i + 1]);
      if (s[3]) t.closePath();
    } else if (kind === 1) {
      t.moveTo(s[3] + s[5], s[4]); t.arc(s[3], s[4], s[5], 0, Math.PI * 2, false);
    } else if (kind === 2) {
      t.moveTo(s[3] + s[5] * Math.cos(s[6]), s[4] + s[5] * Math.sin(s[6])); t.arc(s[3], s[4], s[5], s[6], s[7], false);
    } else if (kind === 4) {
      for (const p of s[4]) {
        t.moveTo(p[0], p[1]);
        for (let i = 2; i < p.length; i += 2) t.lineTo(p[i], p[i + 1]);
        t.closePath();
      }
    }
  }
  const pathOf = (shapes) => { const p = new Path2D(); for (const s of shapes) emit(p, s); return p; };

  function ensure(c) {
    const have = cache.get(c.id);
    if (have && have.stamp === c.stamp) return have;
    const key = c.id + ":" + c.stamp;
    if (!fetching.has(key)) {
      fetching.add(key);
      Backend.call("cad_drawing", { id: c.id }).then((d) => {
        cache.set(c.id, build(d));
        placed.delete(c.id);
        // the plan is given its data again, so the drawing shows and its lines can be snapped to
        const now = Plan.viewData();
        if (now && list(now).some((x) => x.id === c.id)) Plan.setData(now);
      }).catch((e) => setPrompt(`${c.name}: ${e.message || e}`, true)).finally(() => fetching.delete(key));
    }
    return have || null;
  }
  // imports that are gone leave the cache
  function prune(d) {
    const ids = new Set(list(d).map((c) => c.id));
    if (app.view.kind !== "plan") return;
    for (const id of placed.keys()) if (!ids.has(id)) placed.delete(id);
  }

  // ---- where an import sits ----------------------------------------------------------------------------------------
  // a point of the fetched drawing (about its middle) in the model, mm
  function placer(c, base) {
    const a = c.angle_deg * Math.PI / 180, cs = Math.cos(a) * c.scale, sn = Math.sin(a) * c.scale;
    const ox = c.at[0] + base[0] * cs - base[1] * sn, oy = c.at[1] + base[0] * sn + base[1] * cs;
    return { cs, sn, ox, oy, at: (x, y) => [ox + x * cs - y * sn, oy + x * sn + y * cs] };
  }
  // its shown lines in the model, for picking and snapping
  function inModel(c) {
    const k = cache.get(c.id);
    if (!k || k.stamp !== c.stamp) return null;
    const key = [c.stamp, c.at[0], c.at[1], c.angle_deg, c.scale, (c.hidden || []).join("\u0001")].join("|");
    let p = placed.get(c.id);
    if (p && p.key === key) return p;
    const hidden = new Set((c.hidden || []).map((n) => k.d.layers.findIndex((l) => l.name === n)));
    const { cs, sn, ox, oy } = placer(c, k.d.base);
    const n = k.segLayer.length, out = new Float64Array(n * 4);
    let m = 0, x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (let i = 0; i < n; i++) {
      if (hidden.has(k.segLayer[i])) continue;
      for (let j = 0; j < 2; j++) {
        const x = k.segs[4 * i + 2 * j], y = k.segs[4 * i + 2 * j + 1];
        const wx = ox + x * cs - y * sn, wy = oy + x * sn + y * cs;
        out[4 * m + 2 * j] = wx; out[4 * m + 2 * j + 1] = wy;
        if (wx < x0) x0 = wx; if (wx > x1) x1 = wx; if (wy < y0) y0 = wy; if (wy > y1) y1 = wy;
      }
      m++;
    }
    p = { key, segs: out.subarray(0, m * 4), box: [x0, y0, x1, y1], snap: null };
    placed.set(c.id, p);
    return p;
  }
  // how far a model point is from the import's nearest line; Infinity past `tol`
  function distance(c, pt, tol) {
    const p = inModel(c);
    if (!p || pt.x < p.box[0] - tol || pt.x > p.box[2] + tol || pt.y < p.box[1] - tol || pt.y > p.box[3] + tol) return Infinity;
    const s = p.segs;
    let best = Infinity;
    for (let i = 0; i < s.length; i += 4) {
      const ax = s[i], ay = s[i + 1], bx = s[i + 2], by = s[i + 3];
      if ((pt.x < ax - tol && pt.x < bx - tol) || (pt.x > ax + tol && pt.x > bx + tol) || (pt.y < ay - tol && pt.y < by - tol) || (pt.y > ay + tol && pt.y > by + tol)) continue;
      const dx = bx - ax, dy = by - ay, l2 = dx * dx + dy * dy;
      const t = l2 ? Math.max(0, Math.min(1, ((pt.x - ax) * dx + (pt.y - ay) * dy) / l2)) : 0;
      const d = Math.hypot(pt.x - ax - t * dx, pt.y - ay - t * dy);
      if (d < best) best = d;
    }
    return best <= tol ? best : Infinity;
  }

  // what the plan picks: the box round the drawing for outlines and boxes, its lines for a click
  function elements(d) {
    return list(d).filter((c) => c.ring).map((c) => ({
      id: c.id, kind: "cadimport", category: "Imported Drawings", ring: c.ring, pinned: c.pinned, display: "normal",
      hit: (pt, tol) => distance(c, pt, tol),
    }));
  }
  // segments [x0, y0, x1, y1] (mm) for the snap index
  function snapSegs(d) {
    const out = [];
    for (const c of list(d)) {
      const p = inModel(c);
      if (!p || p.segs.length / 4 > SNAP_LIMIT) continue;
      if (!p.snap) { p.snap = []; for (let i = 0; i < p.segs.length; i += 4) p.snap.push([p.segs[i], p.segs[i + 1], p.segs[i + 2], p.segs[i + 3]]); }
      for (const s of p.snap) out.push(s);
    }
    return out;
  }

  // ---- drawing -----------------------------------------------------------------------------------------------------
  const rgb = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;
  // white lines are for a black screen: on paper they are black
  const ink = (c, black) => (black || !c || (c[0] > 235 && c[1] > 235 && c[2] > 235) ? "#000" : rgb(c));

  // `front` draws the imports set to Foreground, after the model; the rest go under it
  // `thin` is the view's Thin Lines: every line of the drawing one pixel wide, whatever its layer's weight
  function draw(ctx, d, toScreen, k, sel = new Set(), tag = null, front = false, thin = false) {
    prune(d);
    const scale = d?.view?.scale || 100;
    const recording = typeof ctx.oaTag === "function"; // PDF and DXF export: paths go out as lines, not Path2D
    for (const c of list(d)) {
      if (!!c.foreground !== front) continue;
      const got = ensure(c);
      if (!got || got.stamp !== c.stamp) continue;
      const on = sel.has(c.id);
      const pl = placer(c, got.d.base);
      const S = (x, y) => { const q = pl.at(x, y); return toScreen({ x: q[0], y: q[1] }); };
      const o = S(0, 0), ux = S(1, 0), uy = S(0, 1);
      const m = [ux.x - o.x, ux.y - o.y, uy.x - o.x, uy.y - o.y, o.x, o.y];
      const px = Math.hypot(m[0], m[1]); // screen pixels to one drawing unit
      if (!(px > 0) || !isFinite(px)) continue;
      const hidden = new Set(c.hidden || []);
      const mmPx = scale * k; // screen pixels to a millimetre on paper
      const shown = got.groups.filter((g) => !hidden.has(got.d.layers[g.layer]?.name));
      ctx.save();
      ctx.lineCap = "butt"; ctx.lineJoin = "round";
      ctx.save();
      ctx.transform(m[0], m[1], m[2], m[3], m[4], m[5]);
      // fills under the lines
      for (const g of shown) {
        if (!g.solids.length && !g.pales.length) continue;
        const L = got.d.layers[g.layer];
        if (tag) tag({ cat: "Imported Drawings", sub: L.name, kind: "pattern" });
        ctx.fillStyle = on ? ACCENT : ink(g.col || L.color, c.black);
        for (const [shapes, alpha, slot] of [[g.solids, 1, "solid"], [g.pales, 0.22, "pale"]]) {
          if (!shapes.length) continue;
          ctx.globalAlpha = alpha;
          if (recording) { ctx.beginPath(); for (const s of shapes) emit(ctx, s); ctx.fill("evenodd"); }
          else ctx.fill(g[slot] || (g[slot] = pathOf(shapes)), "evenodd");
        }
        ctx.globalAlpha = 1;
      }
      for (const g of shown) {
        if (!g.strokes.length) continue;
        const L = got.d.layers[g.layer];
        if (tag) tag({ cat: "Imported Drawings", sub: L.name, kind: "projection" });
        ctx.strokeStyle = on ? ACCENT : ink(g.col || L.color, c.black);
        const w = thin && !recording ? 1 : Math.max(recording ? 0.13 * mmPx : 1, (L.weight_mm || 0.18) * mmPx);
        ctx.lineWidth = w / px;
        const period = (L.dash || []).reduce((a, b) => a + b, 0) * px;
        // a dot is a dash too short to see: give it the line's width
        ctx.setLineDash(period >= 5 ? L.dash.map((v) => Math.max(v, w / px)) : []);
        if (recording) { ctx.beginPath(); for (const s of g.strokes) emit(ctx, s); ctx.stroke(); }
        else ctx.stroke(g.path || (g.path = pathOf(g.strokes)));
      }
      ctx.restore();
      ctx.setLineDash([]);
      // text and points are set out on the screen, so the text reads the right way up under the flipped y axis
      const P = (x, y) => ({ x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5] });
      const cv = recording ? null : document.getElementById("plan"), far = cv ? Math.hypot(cv.clientWidth, cv.clientHeight) : Infinity;
      const mid = cv ? { x: cv.clientWidth / 2, y: cv.clientHeight / 2 } : { x: 0, y: 0 };
      for (const t of got.texts) {
        const L = got.d.layers[t.layer];
        if (hidden.has(L?.name)) continue;
        const size = t.h * px / 0.72; // a drawing's text height is its capital height
        if (!recording && size < 3) continue;
        const q = P(t.x, t.y);
        if (Math.hypot(q.x - mid.x, q.y - mid.y) > far + size * 40) continue;
        if (tag) tag({ cat: "Imported Drawings", sub: L.name, kind: "text" });
        const e = P(t.x + Math.cos(t.a), t.y + Math.sin(t.a));
        ctx.save();
        ctx.translate(q.x, q.y); ctx.rotate(Math.atan2(e.y - q.y, e.x - q.x));
        ctx.font = `${size}px Arial, sans-serif`;
        ctx.textAlign = ["left", "center", "right"][t.ha] || "left";
        ctx.textBaseline = ["alphabetic", "bottom", "middle", "top"][t.va] || "alphabetic";
        ctx.fillStyle = on ? ACCENT : ink(t.col || L.color, c.black);
        const lh = size * 1.2, n = t.lines.length;
        const y0 = t.va === 3 ? 0 : t.va === 2 ? -(n - 1) * lh / 2 : -(n - 1) * lh;
        t.lines.forEach((line, i) => ctx.fillText(line, 0, y0 + i * lh));
        ctx.restore();
      }
      if (got.points.length) {
        ctx.lineWidth = 1;
        for (const t of got.points) {
          const L = got.d.layers[t.layer];
          if (hidden.has(L?.name)) continue;
          const q = P(t.x, t.y);
          if (Math.hypot(q.x - mid.x, q.y - mid.y) > far) continue;
          if (tag) tag({ cat: "Imported Drawings", sub: L.name, kind: "projection" });
          ctx.strokeStyle = on ? ACCENT : ink(t.col || L.color, c.black);
          ctx.beginPath(); ctx.moveTo(q.x - 3, q.y); ctx.lineTo(q.x + 3, q.y); ctx.moveTo(q.x, q.y - 3); ctx.lineTo(q.x, q.y + 3); ctx.stroke();
        }
      }
      if (tag) tag(null);
      ctx.restore();
    }
  }

  // ---- Insert > Import CAD, Link CAD -------------------------------------------------------------------------------
  const plural = (n, one) => `${n.toLocaleString()} ${one}${n === 1 ? "" : "s"}`;
  function report(r, verb) {
    const c = r.counts, left = Object.entries(r.left_out || {});
    const unit = r.scale_from === "file" ? `drawn in ${r.unit}` : r.scale_from === "guessed" ? `no units in the file, taken as ${r.scale === 1000 ? "metres" : "millimetres"} (change Scale in Properties if that's wrong)` : `1 unit = ${r.scale} mm`;
    const parts = [plural(c.lines, "line"), plural(c.arcs, "arc"), plural(c.texts, "text"), c.fills ? plural(c.fills, "fill") : null, c.points ? plural(c.points, "point") : null].filter(Boolean).join(", ");
    const out = left.length ? ` Left out: ${left.map(([k, v]) => `${v} ${k}`).join(", ")}.` : "";
    setPrompt(`${verb} ${r.name} (${r.format}${r.version ? " " + r.version : ""}, ${unit}): ${parts} on ${plural(c.layers, "layer")}.${out}${(r.notes || []).map((n) => " " + n).join("")}`);
  }

  async function start(link) {
    if (app.view.kind !== "plan") { setPrompt("Open a floor plan to bring a CAD drawing into.", true); return; }
    const path = await Backend.pickOpenCad();
    if (!path) return;
    const file = String(path).split(/[\\/]/).pop();
    openModal(`${link ? "Link" : "Import"} CAD: ${file}`, (body, foot, close) => {
      const pick = (opts) => el("select", {}, opts.map(([v, t]) => new Option(t, v)));
      const colors = pick([["preserve", "Preserve"], ["black", "Black and White"]]);
      const layers = pick([["visible", "Visible"], ["all", "All"]]);
      const units = pick([["auto", "Auto-Detect"], ["mm", "Millimetres"], ["cm", "Centimetres"], ["m", "Metres"], ["inches", "Inches"], ["feet", "Feet"]]);
      const position = pick([["center", "Centre of the drawing to the centre of the view"], ["origin", "Drawing origin to the project origin"]]);
      const only = el("input", { type: "checkbox" });
      const msg = el("div", { className: "modal-msg" });
      const row = (l, c) => el("label", { className: "new-row" }, [el("span", { textContent: l }), c]);
      body.append(row("Colours", colors), row("Layers", layers), row("Import Units", units), row("Positioning", position), row("Current View Only", only),
        el("div", { className: "modal-note", textContent: link ? "A linked drawing keeps its file's path: Reload reads it again after the file changes." : "An imported drawing is kept in the project; the file isn't needed again." }), msg);
      let busy = false;
      const ok = async () => {
        if (busy) return;
        busy = true; msg.textContent = `Reading ${file}...`;
        try {
          const centre = Plan.center();
          const r = await Backend.call("import_cad", {
            path, view_id: app.view.viewId, link, current_view_only: only.checked, units: units.value, layers: layers.value, colors: colors.value,
            position: position.value, at: position.value === "center" ? [centre.x, centre.y] : [0, 0],
          });
          close();
          await refresh(true);
          select(r.id);
          Plan.frame([r.id]);
          report(r, link ? "Linked" : "Imported");
        } catch (e) { msg.textContent = String(e.message || e); busy = false; }
      };
      foot.append(button("Cancel", close), button("OK", ok, true));
    });
  }

  async function reload(id) {
    setPrompt("Reading the file again...");
    const r = await run("reload_cad", { id });
    if (!r) return;
    await refresh(true);
    report(r, "Reloaded");
  }

  // the layers of an import, each with a tick: shown or hidden wherever the import is drawn
  async function layersDialog(id) {
    const info = await run("cad_layers", { id });
    if (!info) return;
    openModal(`Layers: ${info.name}`, (body, foot, close) => {
      const msg = el("div", { className: "modal-msg" });
      const box = el("div", {});
      box.style.cssText = "max-height:50vh;overflow:auto;border:1px solid var(--line);padding:4px 8px;min-width:360px";
      const ticks = info.layers.map((l) => {
        const t = el("input", { type: "checkbox", checked: l.shown });
        const sw = el("span", {});
        sw.style.cssText = `display:inline-block;width:12px;height:12px;border:1px solid #888;margin:0 6px;vertical-align:-2px;background:${rgb(l.color)}`;
        const line = el("label", {}, [t, sw, el("span", { textContent: `${l.name}  (${l.shapes})` })]);
        line.style.cssText = "display:block;white-space:nowrap;padding:1px 0";
        if (!l.shapes) line.style.opacity = "0.55";
        box.append(line);
        return { t, l };
      });
      const set = (f) => () => ticks.forEach((x) => { x.t.checked = f(x.l); });
      const bar = el("div", {}, [button("All", set(() => true)), button("None", set(() => false)), button("As in the File", set((l) => l.on_in_file))]);
      bar.style.cssText = "display:flex;gap:6px;margin-bottom:6px";
      body.append(bar, box, msg);
      const ok = async () => {
        try {
          await Backend.call("patch_cad_import", { id, patch: { hidden_layers: ticks.filter((x) => !x.t.checked).map((x) => x.l.name) } });
          close(); await refresh(true);
        } catch (e) { msg.textContent = String(e.message || e); }
      };
      foot.append(button("Cancel", close), button("OK", ok, true));
    });
  }

  // Create from CAD: a toposolid through the contours and survey points on layers of an import. With no id it takes
  // the selected import, or the only one the plan shows.
  async function toToposolid(id = null) {
    if (id == null) {
      const all = app.view.kind === "plan" ? list(Plan.viewData()) : [];
      const c = all.find((x) => app.selection.length === 1 && x.id === app.selection[0]) || (all.length === 1 ? all[0] : null);
      if (!c) { setPrompt(all.length ? "Select the CAD drawing with the contours, then choose Create from CAD again." : "Bring the survey in first with Insert > Import CAD or Link CAD, in a floor plan."); return; }
      id = c.id;
    }
    if (!(app.info.toposolid_types || []).length) {
      if (!(await run("ensure_toposolid_types", {}))) return;
      await refresh();
    }
    const info = await run("cad_heights", { id });
    if (!info) return;
    const unit = info.unit || (info.scale === 1000 ? "m" : info.scale === 1 ? "mm" : "units");
    const fmt = (v) => (Math.abs(v) >= 1000 ? v.toFixed(0) : v.toFixed(2));
    openModal(`Create Toposolid from CAD: ${info.name}`, (body, foot, close) => {
      const msg = el("div", { className: "modal-msg" });
      const box = el("div", {});
      box.style.cssText = "max-height:40vh;overflow:auto;border:1px solid var(--line);padding:4px 8px;min-width:420px;margin-bottom:8px";
      // layers with heights first; flat ones after, unticked
      const rows = info.layers.filter((l) => l.raised || l.flat || l.level_texts).sort((a, b) => (b.raised > 0) - (a.raised > 0));
      const ticks = rows.map((l) => {
        const t = el("input", { type: "checkbox", checked: l.raised > 0 && l.shown });
        const sw = el("span", {});
        sw.style.cssText = `display:inline-block;width:12px;height:12px;border:1px solid #888;margin:0 6px;vertical-align:-2px;background:${rgb(l.color)}`;
        const texts = l.level_texts ? `; ${l.level_texts} level text${l.level_texts === 1 ? "" : "s"}` : "";
        const what = (l.raised ? `${l.raised} with heights, ${fmt(l.lo)} to ${fmt(l.hi)} ${unit}${l.flat ? `; ${l.flat} at 0` : ""}` : `${l.flat} at height 0`) + texts;
        const line = el("label", {}, [t, sw, el("span", { textContent: `${l.name}  (${what})` })]);
        line.style.cssText = "display:block;white-space:nowrap;padding:1px 0";
        if (!l.raised) line.style.opacity = "0.55";
        box.append(line);
        return { t, l };
      });
      if (!rows.some((l) => l.raised)) msg.textContent = info.layers.some((l) => l.level_texts) ? "Nothing in this drawing is drawn at a height, but it has levels written as text: tick their layers and Read level text as heights." : "Nothing in this drawing has a height: its contours are drawn flat. Ask the surveyor for a 3D file, or use a points file.";
      const skip = el("input", { type: "checkbox", checked: true });
      // spot levels written as text (12.45) on the ticked layers; off by default, since a survey with level points
      // as well would give each level twice, the text a little to one side
      const anyText = info.layers.some((l) => l.level_texts);
      const texts = el("input", { type: "checkbox", checked: anyText && !rows.some((l) => l.raised), disabled: !anyText });
      const edge = el("select", {}, [new Option("Around the points", ""), ...info.layers.filter((l) => l.closed).map((l) => new Option(`Largest closed line on ${l.name}`, l.name))]);
      const base = el("select", {}, [new Option("Survey Point (shared elevation)", "survey_point"), new Option("Project Base Point", "project_base_point"), new Option("Internal Origin", "internal_origin")]);
      const levels = app.info.levels || [];
      const level = el("select", {}, levels.map((l) => new Option(l.name, l.id, false, l.id === info.level_id)));
      const types = el("select", {}, (app.info.toposolid_types || []).map((t) => new Option(t.name, t.id, false, t.id === Topo.typeId())));
      const row = (l, c) => el("label", { className: "new-row" }, [el("span", { textContent: l }), c]);
      body.append(el("div", { className: "modal-msg", textContent: "Layers with the contours and survey points:" }), box,
        row("Leave out what lies at height 0", skip), row("Read level text as heights", texts), row("Boundary", edge), row("The drawing's height 0 is at", base), row("Level", level), row("Type", types), msg);
      const ok = async () => {
        const layers = ticks.filter((x) => x.t.checked).map((x) => x.l.name);
        if (!layers.length) { msg.textContent = "Tick at least one layer."; return; }
        let r;
        try {
          r = await Backend.call("toposolid_from_cad", { id, layers, skip_zero: skip.checked, text_levels: texts.checked, boundary_layer: edge.value || null, base: base.value, level_id: Number(level.value), type_id: Number(types.value) });
        } catch (e) { msg.textContent = String(e.message || e); return; }
        close();
        app.topoTypeId = Number(types.value);
        await refresh();
        select(r.id);
        setPrompt(edge.value ? `Toposolid created inside the boundary on ${edge.value}, with ${r.kept} points.` : `Toposolid created from ${r.read} point${r.read === 1 ? "" : "s"}${r.kept < r.read ? ` (thinned to ${r.kept})` : ""}.`);
      };
      foot.append(button("Cancel", close), button("OK", ok, true));
    });
  }

  // Modify | a selected import
  function selectionContext() {
    if (app.view.kind !== "plan" || app.selection.length !== 1 || Plan.tool !== "select") return null;
    const c = list(Plan.viewData()).find((x) => x.id === app.selection[0]);
    if (!c) return null;
    return { auto: true, key: `cad:${c.id}:${c.linked}`, title: c.name, groups: [
      { label: "Import Instance", labelled: true, items: [
        { label: "Layers", icon: "importDxf", big: true, onclick: () => layersDialog(c.id) },
        ...(c.linked ? [{ label: "Reload", icon: "linkDxf", big: true, onclick: () => reload(c.id) }] : []),
      ] },
      { label: "Site", labelled: true, items: [
        { label: "Toposolid", icon: "topoFile", big: true, title: "Create a toposolid from the drawing's contours and survey points", onclick: () => toToposolid(c.id) },
      ] },
    ] };
  }

  return { start, draw, elements, snapSegs, selectionContext, layersDialog, reload, toToposolid };
})();
