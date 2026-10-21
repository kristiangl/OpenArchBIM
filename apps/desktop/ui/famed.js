// The annotation family editor: the drawing of a level head or a marker on its template, with label parameters.
// The template has two reference lines through the origin. The horizontal one is what the symbol sits on (the
// level line for a head); the vertical one is where the symbol flips: a head is drawn on its right for the right
// hand end of a level line and mirrored about it for the left hand end. All sizes are paper millimetres.

const FamEd = (() => {
  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const snap = (v) => Math.round(v * 2) / 2; // half a millimetre
  const KIND_NOTE = {
    level_head: "Level head: the level line arrives from the left and ends at the vertical reference line.",
    elevation_marker: "Elevation marker: the body is drawn once. The pointer is drawn for each view, turned to point along +x here.",
    section_marker: "Section marker: the body is drawn once. The pointer is drawn for the view direction, along +x here.",
  };

  async function open(familyId, onSaved) {
    let raw;
    try { raw = await Backend.call("annotation_family", { id: familyId }); } catch (e) { setPrompt(String(e.message || e), true); return; }
    let fam = JSON.parse(JSON.stringify(raw));
    const params = raw.params;
    const hasPointer = fam.family_kind !== "level_head";
    let tool = "select", layer = "shapes", sel = null, flipped = false, draft = null, dragging = null, dirty = false;
    let mouse = null;
    const view = { k: 14, ox: 0, oy: 0 };

    openModal(`Family Editor: ${fam.name}`, (body, foot, close) => {
      const wrap = document.createElement("div");
      wrap.className = "famed";
      const left = document.createElement("div");
      left.className = "famed-left";
      const bar = document.createElement("div");
      bar.className = "famed-tools";
      const canvas = document.createElement("canvas");
      canvas.width = 600; canvas.height = 420; canvas.className = "famed-canvas";
      const note = document.createElement("div");
      note.className = "famed-note";
      note.textContent = KIND_NOTE[fam.family_kind];
      left.append(bar, canvas, note);
      const right = document.createElement("div");
      right.className = "famed-right";
      wrap.append(left, right);
      const msg = document.createElement("div");
      msg.className = "modal-msg";
      body.append(wrap, msg);
      const ctx = canvas.getContext("2d");
      view.ox = fam.family_kind === "level_head" ? 130 : 300;
      view.oy = 210;

      const items = () => [
        ...fam.shapes.map((s, i) => ({ layer: "shapes", i, s })),
        ...(hasPointer ? fam.pointer.map((s, i) => ({ layer: "pointer", i, s })) : []),
        ...fam.labels.map((s, i) => ({ layer: "labels", i, s })),
      ];
      const get = (it) => (it ? fam[it.layer][it.i] : null);
      const toScreen = (x, y) => ({ x: view.ox + x * view.k, y: view.oy - y * view.k });
      const toPaper = (sx, sy) => ({ x: (sx - view.ox) / view.k, y: (view.oy - sy) / view.k });
      const sampleLines = (lb) => lb.lines.map((l) => `${l.prefix || ""}${l.sample || ""}${l.suffix || ""}`);
      const mark = () => { dirty = true; };

      // ---- drawing ------------------------------------------------------------------------
      function render() {
        const text = css("--text"), accent = css("--accent"), bg = css("--view-bg"), muted = css("--muted");
        const W = canvas.width, H = canvas.height;
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.fillStyle = bg; ctx.fillRect(0, 0, W, H);
        // paper grid, every 5 mm
        ctx.strokeStyle = css("--line"); ctx.lineWidth = 0.5; ctx.globalAlpha = 0.5;
        ctx.beginPath();
        for (let x = Math.ceil((0 - view.ox) / view.k / 5) * 5; toScreen(x, 0).x < W; x += 5) { const p = toScreen(x, 0); ctx.moveTo(p.x, 0); ctx.lineTo(p.x, H); }
        for (let y = Math.ceil((view.oy - H) / view.k / 5) * 5; toScreen(0, y).y > 0; y += 5) { const p = toScreen(0, y); ctx.moveTo(0, p.y); ctx.lineTo(W, p.y); }
        ctx.stroke(); ctx.globalAlpha = 1;
        // reference lines
        const o = toScreen(0, 0);
        ctx.save(); ctx.strokeStyle = "#3b7fd1"; ctx.lineWidth = 1; ctx.setLineDash([6, 4]);
        ctx.beginPath(); ctx.moveTo(0, o.y); ctx.lineTo(W, o.y); ctx.moveTo(o.x, 0); ctx.lineTo(o.x, H); ctx.stroke(); ctx.restore();
        ctx.fillStyle = "#3b7fd1"; ctx.font = "11px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "bottom";
        ctx.fillText(fam.family_kind === "level_head" ? "Reference line (level line)" : "Reference line", 6, o.y - 3);
        ctx.textAlign = "left"; ctx.textBaseline = "top";
        ctx.fillText("Flip line", o.x + 4, 4);
        if (fam.family_kind === "level_head") {
          // the level line itself, on the left of the flip line
          ctx.save(); ctx.strokeStyle = muted; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(0, o.y); ctx.lineTo(o.x, o.y); ctx.stroke(); ctx.restore();
        }
        const flip = flipped ? { flip: true } : {};
        const base = { x: o.x, y: o.y, k: view.k, ...flip };
        const pen = Math.max(1, (fam.pen_mm || 0.25) * view.k);
        fam.shapes.forEach((s, i) => Ann.shapes(ctx, [s], base, sel && sel.layer === "shapes" && sel.i === i ? accent : text, bg, pen));
        if (hasPointer) fam.pointer.forEach((s, i) => Ann.shapes(ctx, [s], base, sel && sel.layer === "pointer" && sel.i === i ? accent : "#a0522d", bg, pen));
        fam.labels.forEach((lb, i) => {
          const on = sel && sel.layer === "labels" && sel.i === i;
          Ann.labels(ctx, [{ ...lb, lines: sampleLines(lb) }], base, on ? accent : text);
          const e = Ann.extent({ labels: [lb] });
          const a = toScreen(flipped ? -e.maxX : e.minX, e.maxY), b = toScreen(flipped ? -e.minX : e.maxX, e.minY);
          ctx.save(); ctx.strokeStyle = on ? accent : muted; ctx.lineWidth = on ? 1.5 : 0.7; ctx.setLineDash([2, 3]); ctx.strokeRect(a.x, a.y, b.x - a.x, b.y - a.y); ctx.restore();
          const anchor = toScreen(flipped ? -lb.x : lb.x, lb.y);
          ctx.fillStyle = on ? accent : muted; ctx.fillRect(anchor.x - 2, anchor.y - 2, 4, 4);
        });
        // the shape being drawn
        if (draft && mouse) {
          const p = toPaper(mouse.x, mouse.y), q = { x: snap(p.x), y: snap(p.y) };
          const prev = draftShape(q);
          if (prev) Ann.shapes(ctx, [prev], base, accent, null, pen);
        }
        if (sel && sel.layer !== "labels") {
          const sh = get(sel);
          for (const [x, y] of handles(sh)) { const s = toScreen(flipped ? -x : x, y); ctx.fillStyle = accent; ctx.fillRect(s.x - 3, s.y - 3, 6, 6); }
        }
      }
      const handles = (sh) => (sh.type === "line" ? [sh.a, sh.b] : sh.type === "polygon" ? sh.pts : sh.type === "circle" ? [sh.c] : [sh.c]);

      // ---- drawing tools -------------------------------------------------------------------
      function draftShape(q, closing) {
        if (!draft) return null;
        if (draft.tool === "line") return { type: "line", a: draft.pts[0], b: [q.x, q.y] };
        if (draft.tool === "circle") return { type: "circle", c: draft.pts[0], r: Math.max(0.1, Math.hypot(q.x - draft.pts[0][0], q.y - draft.pts[0][1])), filled: false };
        if (draft.tool === "polygon") return draft.pts.length ? { type: "polygon", pts: closing ? draft.pts : [...draft.pts, [q.x, q.y]], filled: false } : null;
        if (draft.tool === "arc") {
          const c = draft.pts[0];
          if (draft.pts.length === 1) return { type: "line", a: c, b: [q.x, q.y] };
          const r = Math.hypot(draft.pts[1][0] - c[0], draft.pts[1][1] - c[1]);
          const a0 = (Math.atan2(draft.pts[1][1] - c[1], draft.pts[1][0] - c[0]) * 180) / Math.PI;
          let a1 = (Math.atan2(q.y - c[1], q.x - c[0]) * 180) / Math.PI;
          while (a1 <= a0) a1 += 360;
          return { type: "arc", c, r, start_deg: a0, end_deg: a1 };
        }
        return null;
      }
      function finishDraft(q, closing) {
        const sh = draftShape(q, closing);
        const t = draft.tool;
        draft = null;
        if (!sh) { render(); return; }
        const target = layer === "pointer" && hasPointer ? "pointer" : "shapes";
        fam[target].push(sh);
        sel = { layer: target, i: fam[target].length - 1 };
        mark();
        setTool("select");
        buildRight();
        render();
      }

      function setTool(t) {
        tool = t; draft = null;
        for (const b of bar.querySelectorAll("button[data-tool]")) b.classList.toggle("on", b.dataset.tool === t);
        canvas.style.cursor = t === "select" ? "default" : "crosshair";
        render();
      }

      // ---- hit testing and dragging ------------------------------------------------------------
      const distSeg = (p, a, b) => {
        const dx = b.x - a.x, dy = b.y - a.y, l2 = dx * dx + dy * dy;
        const t = l2 ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2)) : 0;
        return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
      };
      function hit(sx, sy) {
        const P = { x: sx, y: sy };
        const sc = (x, y) => toScreen(flipped ? -x : x, y);
        let best = null;
        const consider = (it, d) => { if (d <= 7 && (!best || d < best.d)) best = { it, d }; };
        for (const it of items()) {
          const s = it.s;
          if (it.layer === "labels") {
            const e = Ann.extent({ labels: [s] }), a = sc(e.minX, e.maxY), b = sc(e.maxX, e.minY);
            const x0 = Math.min(a.x, b.x), x1 = Math.max(a.x, b.x), y0 = Math.min(a.y, b.y), y1 = Math.max(a.y, b.y);
            if (sx >= x0 - 3 && sx <= x1 + 3 && sy >= y0 - 3 && sy <= y1 + 3) consider(it, 2);
          } else if (s.type === "line") consider(it, distSeg(P, sc(...s.a), sc(...s.b)));
          else if (s.type === "circle") { const c = sc(...s.c); consider(it, s.filled && Math.hypot(sx - c.x, sy - c.y) < s.r * view.k ? 1 : Math.abs(Math.hypot(sx - c.x, sy - c.y) - s.r * view.k)); }
          else if (s.type === "arc") { const c = sc(...s.c); consider(it, Math.abs(Math.hypot(sx - c.x, sy - c.y) - s.r * view.k)); }
          else if (s.type === "polygon") {
            const pts = s.pts.map((p) => sc(...p));
            let d = Infinity;
            for (let i = 0; i < pts.length; i++) d = Math.min(d, distSeg(P, pts[i], pts[(i + 1) % pts.length]));
            consider(it, d);
          }
        }
        return best ? best.it : null;
      }
      function translate(sh, dx, dy) {
        if (sh.type === "line") { sh.a = [sh.a[0] + dx, sh.a[1] + dy]; sh.b = [sh.b[0] + dx, sh.b[1] + dy]; }
        else if (sh.type === "circle" || sh.type === "arc") sh.c = [sh.c[0] + dx, sh.c[1] + dy];
        else if (sh.type === "polygon") sh.pts = sh.pts.map((p) => [p[0] + dx, p[1] + dy]);
        else { sh.x += dx; sh.y += dy; }
      }

      canvas.addEventListener("pointermove", (e) => {
        const r = canvas.getBoundingClientRect();
        mouse = { x: ((e.clientX - r.left) * canvas.width) / r.width, y: ((e.clientY - r.top) * canvas.height) / r.height };
        if (dragging) {
          const p = toPaper(mouse.x, mouse.y);
          const px = flipped ? -p.x : p.x;
          const dx = snap(px) - dragging.last.x, dy = snap(p.y) - dragging.last.y;
          if (dx || dy) { translate(get(sel), dx, dy); dragging.last = { x: dragging.last.x + dx, y: dragging.last.y + dy }; mark(); }
        }
        render();
      });
      canvas.addEventListener("pointerdown", (e) => {
        const r = canvas.getBoundingClientRect();
        mouse = { x: ((e.clientX - r.left) * canvas.width) / r.width, y: ((e.clientY - r.top) * canvas.height) / r.height };
        if (tool === "select") {
          const h = hit(mouse.x, mouse.y);
          sel = h ? { layer: h.layer, i: h.i } : null;
          if (h) { const p = toPaper(mouse.x, mouse.y); dragging = { last: { x: snap(flipped ? -p.x : p.x), y: snap(p.y) } }; canvas.setPointerCapture(e.pointerId); }
          buildRight(); render();
          return;
        }
        const p = toPaper(mouse.x, mouse.y), q = { x: snap(flipped ? -p.x : p.x), y: snap(p.y) };
        if (tool === "label") {
          fam.labels.push({ x: q.x, y: q.y, halign: "left", valign: "middle", size_mm: 2.5, bold: false, lines: [{ param: params[0], prefix: "", suffix: "", sample: "Sample" }] });
          sel = { layer: "labels", i: fam.labels.length - 1 };
          mark(); setTool("select"); buildRight(); render();
          return;
        }
        if (!draft) { draft = { tool, pts: [[q.x, q.y]] }; render(); return; }
        if (draft.tool === "polygon") {
          const first = draft.pts[0];
          if (draft.pts.length >= 3 && Math.hypot(q.x - first[0], q.y - first[1]) < 0.6) { finishDraft({ x: first[0], y: first[1] }, true); return; }
          draft.pts.push([q.x, q.y]); render(); return;
        }
        if (draft.tool === "arc" && draft.pts.length === 1) { draft.pts.push([q.x, q.y]); render(); return; }
        finishDraft(q);
      });
      canvas.addEventListener("pointerup", () => { dragging = null; buildRight(); });
      canvas.addEventListener("dblclick", () => {
        if (draft && draft.tool === "polygon" && draft.pts.length >= 3) { draft.pts.pop(); finishDraft({ x: 0, y: 0 }, true); }
      });
      canvas.addEventListener("wheel", (e) => {
        e.preventDefault();
        const r = canvas.getBoundingClientRect();
        const mx = ((e.clientX - r.left) * canvas.width) / r.width, my = ((e.clientY - r.top) * canvas.height) / r.height;
        const before = toPaper(mx, my);
        view.k = Math.min(60, Math.max(4, view.k * Math.exp(-e.deltaY * 0.0012)));
        view.ox = mx - before.x * view.k; view.oy = my + before.y * view.k;
        render();
      }, { passive: false });
      body.closest(".modal-back").addEventListener("keydown", (e) => {
        if (e.target.tagName === "INPUT" || e.target.tagName === "SELECT") return;
        if (e.key === "Delete" || e.key === "Backspace") { removeSel(); e.preventDefault(); }
        if (e.key === "Escape") { if (draft || tool !== "select") { setTool("select"); e.stopPropagation(); } }
      }, true);

      function removeSel() {
        if (!sel) return;
        fam[sel.layer].splice(sel.i, 1);
        sel = null; mark(); buildRight(); render();
      }

      // ---- the properties column -------------------------------------------------------------
      const field = (label, input) => {
        const row = document.createElement("label");
        row.className = "famed-row";
        const s = document.createElement("span");
        s.textContent = label;
        row.append(s, input);
        return row;
      };
      const numInput = (value, onChange, step = 0.5) => {
        const i = document.createElement("input");
        i.type = "number"; i.step = String(step); i.value = String(Math.round(value * 1000) / 1000);
        i.onchange = () => { const v = parseFloat(i.value); if (Number.isFinite(v)) { onChange(v); mark(); render(); } };
        return i;
      };
      const textInput = (value, onChange) => {
        const i = document.createElement("input");
        i.type = "text"; i.value = value;
        i.oninput = () => { onChange(i.value); mark(); render(); };
        return i;
      };
      const selectInput = (value, opts, onChange) => {
        const s = document.createElement("select");
        for (const [v, l] of opts) s.add(new Option(l, v, false, v === value));
        s.onchange = () => { onChange(s.value); mark(); render(); };
        return s;
      };
      const checkInput = (value, onChange) => {
        const i = document.createElement("input");
        i.type = "checkbox"; i.checked = !!value;
        i.onchange = () => { onChange(i.checked); mark(); render(); };
        return i;
      };
      const title = (t) => { const h = document.createElement("div"); h.className = "famed-h"; h.textContent = t; return h; };

      function buildRight() {
        right.replaceChildren();
        right.append(title("Family"));
        right.append(field("Name", textInput(fam.name, (v) => { fam.name = v; })));
        right.append(field("Pen width (mm)", numInput(fam.pen_mm, (v) => { fam.pen_mm = v; }, 0.05)));
        if (fam.family_kind === "section_marker") {
          // how the hidden middle of a broken section line shows, and what a reference section's Reference Label reads
          right.append(field("Broken Section Display Style", selectInput(fam.broken_style || "gapped", [["gapped", "Gapped"], ["continuous", "Continuous"]], (v) => { fam.broken_style = v; })));
          right.append(field("Reference Label", textInput(fam.reference_label == null ? "SIM" : fam.reference_label, (v) => { fam.reference_label = v; })));
        }
        const s = sel ? get(sel) : null;
        if (!s) {
          const hint = document.createElement("div");
          hint.className = "famed-hint";
          hint.textContent = "Select a shape or label to edit it, or choose a tool above to draw. Drag to move. Delete removes the selection. Scroll to zoom.";
          right.append(hint);
          return;
        }
        if (sel.layer === "labels") {
          right.append(title("Label"));
          right.append(field("X (mm)", numInput(s.x, (v) => { s.x = v; })), field("Y (mm)", numInput(s.y, (v) => { s.y = v; })));
          right.append(field("Text height (mm)", numInput(s.size_mm, (v) => { s.size_mm = v; }, 0.25)));
          right.append(field("Horizontal", selectInput(s.halign, [["left", "Left"], ["center", "Centre"], ["right", "Right"]], (v) => { s.halign = v; })));
          right.append(field("Vertical", selectInput(s.valign, [["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"]], (v) => { s.valign = v; })));
          right.append(field("Bold", checkInput(s.bold, (v) => { s.bold = v; })));
          right.append(title("Label parameters (one per line)"));
          s.lines.forEach((ln, k) => {
            const box = document.createElement("div");
            box.className = "famed-line";
            box.append(
              field("Parameter", selectInput(ln.param, params.map((p) => [p, p]), (v) => { ln.param = v; })),
              field("Prefix", textInput(ln.prefix, (v) => { ln.prefix = v; })),
              field("Suffix", textInput(ln.suffix, (v) => { ln.suffix = v; })),
              field("Sample", textInput(ln.sample, (v) => { ln.sample = v; })),
            );
            if (s.lines.length > 1) box.append(button("Remove line", () => { s.lines.splice(k, 1); mark(); buildRight(); render(); }));
            right.append(box);
          });
          right.append(button("Add parameter line", () => { s.lines.push({ param: params[0], prefix: "", suffix: "", sample: "" }); mark(); buildRight(); render(); }));
        } else {
          right.append(title(`${s.type[0].toUpperCase()}${s.type.slice(1)}${sel.layer === "pointer" ? " (pointer)" : ""}`));
          if (s.type === "line") {
            right.append(field("X1", numInput(s.a[0], (v) => { s.a[0] = v; })), field("Y1", numInput(s.a[1], (v) => { s.a[1] = v; })), field("X2", numInput(s.b[0], (v) => { s.b[0] = v; })), field("Y2", numInput(s.b[1], (v) => { s.b[1] = v; })));
          } else if (s.type === "circle" || s.type === "arc") {
            right.append(field("Centre X", numInput(s.c[0], (v) => { s.c[0] = v; })), field("Centre Y", numInput(s.c[1], (v) => { s.c[1] = v; })), field("Radius", numInput(s.r, (v) => { s.r = Math.max(0.1, v); })));
            if (s.type === "circle") right.append(field("Filled", checkInput(s.filled, (v) => { s.filled = v; })));
            else right.append(field("Start (deg)", numInput(s.start_deg, (v) => { s.start_deg = v; }, 5)), field("End (deg)", numInput(s.end_deg, (v) => { s.end_deg = v; }, 5)));
          } else if (s.type === "polygon") {
            right.append(field("Filled", checkInput(s.filled, (v) => { s.filled = v; })));
            s.pts.forEach((p, k) => right.append(field(`Point ${k + 1}`, (() => {
              const d = document.createElement("span"); d.className = "famed-pt";
              d.append(numInput(p[0], (v) => { p[0] = v; }), numInput(p[1], (v) => { p[1] = v; }));
              return d;
            })())));
          }
        }
        right.append(button("Delete", removeSel));
      }

      // ---- the tool bar ----------------------------------------------------------------------
      const tools = [["select", "Select"], ["line", "Line"], ["circle", "Circle"], ["arc", "Arc"], ["polygon", "Polygon"], ["label", "Label"]];
      for (const [id, label] of tools) {
        const b = button(label, () => setTool(id));
        b.dataset.tool = id;
        bar.append(b);
      }
      if (hasPointer) {
        const ls = selectInput("shapes", [["shapes", "Draw on: body"], ["pointer", "Draw on: pointer"]], (v) => { layer = v; });
        ls.className = "famed-layer";
        bar.append(ls);
      }
      const fl = document.createElement("label");
      fl.className = "famed-flip";
      const fc = document.createElement("input");
      fc.type = "checkbox";
      fc.onchange = () => { flipped = fc.checked; render(); };
      fl.append(fc, " Preview flipped");
      if (fam.family_kind === "level_head") bar.append(fl);

      const save = async () => {
        try {
          await Backend.call("set_annotation_family", { id: familyId, family: fam });
        } catch (e) { msg.textContent = String(e.message || e); return; }
        close();
        if (onSaved) await onSaved();
      };
      foot.append(button("Save", save, true), button("Cancel", () => { if (!dirty || confirm("Discard the changes to this family?")) close(); }));
      setTool("select");
      buildRight();
      render();
    }, { wide: true });
  }

  // Manage > Annotation Families: every family in the project, with edit, duplicate and delete
  async function list() {
    openModal("Annotation Families", (body, foot, close) => {
      const msg = document.createElement("div");
      msg.className = "modal-msg";
      const box = document.createElement("div");
      body.append(box, msg);
      const load = async () => {
        await refresh();
        const fams = app.info.annotation_families;
        const kinds = [["level_head", "Level Heads"], ["elevation_marker", "Elevation Markers"], ["section_marker", "Section Markers"]];
        box.replaceChildren();
        for (const [kind, label] of kinds) {
          const h = document.createElement("div");
          h.className = "famed-h";
          h.textContent = label;
          box.append(h);
          for (const f of fams.filter((x) => x.kind === kind)) {
            const row = document.createElement("div");
            row.className = "famed-list-row";
            const n = document.createElement("span");
            n.textContent = f.name;
            row.append(
              n,
              button("Edit...", () => open(f.id, load)),
              button("Duplicate...", () => duplicateTypePrompt(f.id, async () => { await load(); }, "duplicate_annotation_family")),
              button("Delete", async () => {
                try { await Backend.call("delete_annotation_family", { id: f.id }); msg.textContent = ""; } catch (e) { msg.textContent = String(e.message || e); return; }
                await load();
              }),
            );
            box.append(row);
          }
        }
      };
      foot.append(button("Close", close, true));
      load();
    }, { wide: false });
  }

  return { open, list };
})();
