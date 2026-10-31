// Spot elevations, spot coordinates and spot slopes. Each view makes its own (`makeSpots`): the plan's is `Spots`; an
// elevation or section has one for spot elevations, standing on the lines it names (tops, bottoms, sills, heads, levels).
// The model owns the values (crates/openarch-model/src/spot.rs): it finds the faces under a point, measures them and
// draws the text. This file picks the face, places the spot, draws what the model returns and moves its grips.
// Sizes in the spot type are paper millimetres, so they are scaled by the view scale and the zoom.

function makeSpots() {
  let H = null; // host: set by the view
  let kind = null; // placing tool: "elevation" | "coordinate" | "slope"
  let stage = 0; // 0: pick the point, 1: place the text
  let tgt = null; // the point once picked: { p: {x,y}, host, surface, label }
  let cands = [], idx = 0; // faces under the cursor, best first; Tab steps through them
  let cur = null; // cursor in model mm
  let busy = false, again = false;
  let leader = true;
  let ghost = null, ghostErr = "", ghostBusy = false, ghostAgain = false;
  let boxes = []; // grips of the selected spot, in screen pixels
  let drag = null; // { id, type: "shoulder" | "bend", sx, sy, start, d, moved }
  const NAMES = { elevation: "Spot Elevation", coordinate: "Spot Coordinate", slope: "Spot Slope" };
  const CATEGORY = { elevation: "Spot Elevations", coordinate: "Spot Coordinates", slope: "Spot Slopes" };
  const SURFACE = { top: "Top", core: "Core", bottom: "Bottom" };

  const r3 = (v) => Math.round(v * 1000) / 1000;
  const attach = (h) => { H = h; };
  const active = () => kind !== null;
  const tdef = { text_size_mm: 2.5, text_offset_mm: 0.8, symbol: "target", symbol_size_mm: 3 };
  const lineStyle = { line: { color: "#000000", width_mm: 0.18, pattern: "solid" } };

  // ---- tool ---------------------------------------------------------------------------------
  function start(k) { kind = k; stage = 0; tgt = null; cands = []; idx = 0; ghost = null; ghostErr = ""; drag = null; H.prompt(prompt()); }
  function stop() { kind = null; stage = 0; tgt = null; cands = []; ghost = null; ghostErr = ""; drag = null; }
  function prompt() {
    if (!kind) return "";
    if (stage === 1) return `${NAMES[kind]}: click where the text goes. Esc goes back.`;
    // an elevation or section measures the lines it shows
    if (H.elevation) return `${NAMES[kind]}: click a level, or the top, bottom, sill or head of something. Tab changes the line, L turns the leader ${leader ? "off" : "on"}.`;
    const what = kind === "coordinate" ? "a point (a floor, roof or wall face to follow it, or any empty spot)" : kind === "slope" ? "a floor or roof face" : "a floor, roof or wall face";
    return `${NAMES[kind]}: click ${what}. Tab changes the face, L turns the leader ${leader ? "off" : "on"}.`;
  }
  // Esc or right-click: one step back, then out of the tool. Returns whether it consumed the key.
  function cancel() {
    if (!kind) return false;
    if (stage === 1) { stage = 0; tgt = null; ghost = null; H.prompt(prompt()); H.render(); fetchCands(); return true; }
    stop(); H.setTool("select");
    return true;
  }

  const chosen = () => (cands.length ? cands[idx % cands.length] : null);
  // where the text goes when the user does not say: beside the symbol with no leader, up and to the right with one
  const defaultShoulder = (ty) => {
    const m = H.paperPx() / H.scale(); // model mm per paper mm
    const t = ty || tdef;
    return leader ? [8 * m, 6 * m] : [(t.symbol_size_mm / 2 + 1) * m, 0];
  };

  async function fetchCands() {
    if (busy) { again = true; return; }
    busy = true;
    do {
      again = false;
      const p = cur;
      if (!p || stage !== 0) break;
      try {
        // the tolerance only matters in an elevation or section, where the lines have no inside to click in
        const r = await Backend.call("spot_surfaces", { view_id: H.viewId(), point: [r3(p.x), r3(p.y)], tol: 8 / H.scale() });
        let next = r.surfaces || [];
        if (kind === "slope") next = next.filter((h) => Math.abs(h.normal[2]) > 0.01);
        const sig = (l) => l.map((h) => `${h.host}:${h.surface}`).join();
        if (sig(next) !== sig(cands)) idx = 0;
        cands = next;
      } catch { cands = []; }
    } while (again);
    busy = false;
    fetchGhost();
    H.render();
  }

  // the model draws the spot as it would be placed, so the ghost shows the real value or says why there is none
  async function fetchGhost() {
    if (!kind || !cur) { ghost = null; return; }
    if (ghostBusy) { ghostAgain = true; return; }
    ghostBusy = true;
    do {
      ghostAgain = false;
      let target, host = null, surface = "top", shoulder;
      if (stage === 1) {
        target = tgt.p; host = tgt.host; surface = tgt.surface;
        shoulder = [r3(cur.x - target.x), r3(cur.y - target.y)];
      } else {
        const c = chosen();
        target = cur;
        if (c) { host = c.host; surface = c.surface; }
        shoulder = defaultShoulder().map(r3);
      }
      try {
        const r = await Backend.call("preview_spot", { view_id: H.viewId(), kind, target: [r3(target.x), r3(target.y)], host, surface, shoulder, leader });
        ghost = { ...r.spot, style: lineStyle }; ghostErr = "";
      } catch (e) { ghost = null; ghostErr = String((e && e.message) || e); }
    } while (ghostAgain);
    ghostBusy = false;
    H.render();
  }

  function move(sx, sy) {
    cur = H.toWorld(sx, sy);
    if (!kind) return;
    if (stage === 0) fetchCands(); else fetchGhost();
    H.render();
  }

  async function place(shoulder) {
    const t = tgt;
    const r = await H.call("add_spot", { view_id: H.viewId(), kind, target: [r3(t.p.x), r3(t.p.y)], host: t.host, surface: t.surface, shoulder: shoulder.map(r3), leader });
    stage = 0; tgt = null; ghost = null;
    if (r) { await H.refresh(); H.prompt(`${NAMES[kind]} placed. Click the next point, or Esc to stop.`); }
    else H.prompt(prompt());
    H.render();
    fetchCands();
  }

  function click(sx, sy) {
    cur = H.toWorld(sx, sy);
    if (!kind) return;
    if (stage === 1) {
      const s = [cur.x - tgt.p.x, cur.y - tgt.p.y];
      place(Math.hypot(s[0], s[1]) < 1e-6 ? defaultShoulder() : s);
      return;
    }
    const c = chosen();
    if (kind !== "coordinate" && !c) { H.prompt(`There is nothing to put a ${NAMES[kind].toLowerCase()} on here.`); return; }
    if (ghostErr) { H.prompt(ghostErr); return; }
    tgt = { p: { x: cur.x, y: cur.y }, host: c ? c.host : null, surface: c ? c.surface : "top", label: c ? c.label : "" };
    if (!leader) { place(defaultShoulder()); return; }
    stage = 1; ghost = null;
    H.prompt(prompt());
    fetchGhost();
    H.render();
  }

  function key(e) {
    if (!kind) return false;
    if (e.key === "Escape") return cancel();
    if (e.key === "Tab") {
      if (stage === 0 && cands.length > 1) { idx = (idx + (e.shiftKey ? -1 : 1) + cands.length * 4) % cands.length; fetchGhost(); H.render(); }
      return true;
    }
    if ((e.key === "l" || e.key === "L") && !e.ctrlKey && !e.metaKey && !e.altKey) {
      leader = !leader;
      if (stage === 1 && !leader) { place(defaultShoulder()); return true; }
      H.prompt(prompt()); fetchGhost(); H.render();
      return true;
    }
    return false;
  }

  // ---- drawing ------------------------------------------------------------------------------
  // The layout of a spot in screen pixels. The text stands on the shoulder line, lines stacked upwards.
  function layout(sp, shoulderAt, bendAt) {
    const ctx = H.ctx(), k = H.paperPx(), ty = sp.type || tdef;
    const size = ty.text_size_mm * k, off = ty.text_offset_mm * k, lh = size * 1.25;
    const S = (p) => H.toScreen({ x: p[0], y: p[1] });
    const T = S(sp.target), Sh = S(shoulderAt || sp.shoulder), Bd = (bendAt || sp.bend) ? S(bendAt || sp.bend) : null;
    ctx.save();
    ctx.font = `${Math.max(1, size)}px system-ui, sans-serif`;
    const lines = sp.lines || [];
    const widths = [...lines, sp.above, sp.below].map((s) => (s ? ctx.measureText(s).width : 0));
    ctx.restore();
    const w = Math.max(4, ...widths) + size * 0.6;
    const from = Bd || T;
    const side = Sh.x < from.x - 1e-9 ? -1 : 1;
    const n = lines.length + (sp.above ? 1 : 0);
    const box = { x0: Math.min(Sh.x, Sh.x + side * w), x1: Math.max(Sh.x, Sh.x + side * w), y0: Sh.y - off - n * lh, y1: Sh.y + (sp.below ? off + lh : 0) };
    return { k, ty, size, off, lh, T, Sh, Bd, side, w, box, rad: ty.symbol_size_mm * k / 2 };
  }

  function drawSymbol(ctx, L, sym, col) {
    const { T, rad } = L;
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.setLineDash([]);
    if (sym === "target") {
      ctx.beginPath(); ctx.arc(T.x, T.y, rad, 0, Math.PI * 2); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(T.x - rad, T.y); ctx.lineTo(T.x + rad, T.y); ctx.moveTo(T.x, T.y - rad); ctx.lineTo(T.x, T.y + rad); ctx.stroke();
      // two opposite quarters filled
      ctx.beginPath(); ctx.moveTo(T.x, T.y); ctx.arc(T.x, T.y, rad, -Math.PI / 2, 0); ctx.closePath(); ctx.fill();
      ctx.beginPath(); ctx.moveTo(T.x, T.y); ctx.arc(T.x, T.y, rad, Math.PI / 2, Math.PI); ctx.closePath(); ctx.fill();
    } else if (sym === "cross") {
      ctx.beginPath(); ctx.moveTo(T.x - rad, T.y - rad); ctx.lineTo(T.x + rad, T.y + rad); ctx.moveTo(T.x - rad, T.y + rad); ctx.lineTo(T.x + rad, T.y - rad); ctx.stroke();
    } else if (sym === "triangle") {
      ctx.beginPath(); ctx.moveTo(T.x, T.y); ctx.lineTo(T.x - rad * 0.8, T.y - rad * 1.7); ctx.lineTo(T.x + rad * 0.8, T.y - rad * 1.7); ctx.closePath(); ctx.fill();
    }
    ctx.restore();
  }

  function drawArrow(ctx, L, sp, col) {
    // a slope spot points downhill: the model gives the direction in the plan (y up), the screen has y down
    const a = sp.arrow;
    if (!a) return;
    const len = 8 * L.k, hd = 1.6 * L.k, e = { x: a[0], y: -a[1] }, nrm = { x: -e.y, y: e.x };
    const A = { x: L.T.x + e.x * L.rad * 1.2, y: L.T.y + e.y * L.rad * 1.2 }, B = { x: A.x + e.x * len, y: A.y + e.y * len };
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = Math.max(1, Pen.w(0.18, L.k));
    ctx.beginPath(); ctx.moveTo(A.x, A.y); ctx.lineTo(B.x, B.y); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(B.x, B.y); ctx.lineTo(B.x - e.x * hd * 1.6 + nrm.x * hd * 0.6, B.y - e.y * hd * 1.6 + nrm.y * hd * 0.6); ctx.lineTo(B.x - e.x * hd * 1.6 - nrm.x * hd * 0.6, B.y - e.y * hd * 1.6 - nrm.y * hd * 0.6); ctx.closePath(); ctx.fill();
    ctx.restore();
  }

  function drawOne(sp, o) {
    const ctx = H.ctx(), col = o.col;
    let sh = null, bd = null;
    if (drag && drag.moved && drag.id === sp.id) {
      if (drag.type === "shoulder") sh = [sp.shoulder[0] + drag.d.x, sp.shoulder[1] + drag.d.y];
      else bd = [sp.target[0] + drag.off[0], sp.target[1] + drag.off[1]];
    }
    const L = layout(sp, sh, bd);
    const { T, Sh, Bd, size, off, lh } = L;
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col; ctx.setLineDash([]);
    ctx.lineWidth = (H.thin() ? 1 : Math.max(1, Pen.w(sp.style && sp.style.line ? sp.style.line.width_mm : 0.18, L.k))) + (o.selected ? 0.8 : 0);
    if (o.ghost) ctx.globalAlpha = 0.85;
    if (sp.invalid) {
      ctx.strokeStyle = "#c0392b"; ctx.fillStyle = "#c0392b"; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(T.x, T.y - 9); ctx.lineTo(T.x + 9, T.y + 7); ctx.lineTo(T.x - 9, T.y + 7); ctx.closePath(); ctx.stroke();
      ctx.fillRect(T.x - 1, T.y - 3, 2, 6); ctx.fillRect(T.x - 1, T.y + 4, 2, 2);
      ctx.font = "11px system-ui, sans-serif"; ctx.textAlign = "left"; ctx.textBaseline = "middle";
      ctx.fillText(sp.invalid, T.x + 14, T.y);
      ctx.restore();
      return;
    }
    ctx.restore();
    drawSymbol(ctx, L, sp.symbol || (sp.type && sp.type.symbol) || "target", col);
    if (sp.kind === "slope") drawArrow(ctx, L, sp, col);
    ctx.save();
    ctx.strokeStyle = col; ctx.fillStyle = col;
    ctx.lineWidth = (H.thin() ? 1 : Math.max(1, Pen.w(sp.style && sp.style.line ? sp.style.line.width_mm : 0.18, L.k))) + (o.selected ? 0.8 : 0);
    if (o.ghost) ctx.globalAlpha = 0.85;
    // leader: from the edge of the symbol, through the bend, to the shoulder line
    if (sp.leader) {
      const pts = [T, ...(Bd ? [Bd] : []), Sh];
      const dx = pts[1].x - T.x, dy = pts[1].y - T.y, dl = Math.hypot(dx, dy) || 1;
      const gap = sp.symbol === "none" ? 0 : L.rad;
      ctx.beginPath(); ctx.moveTo(T.x + dx / dl * Math.min(gap, dl), T.y + dy / dl * Math.min(gap, dl));
      for (const q of pts.slice(1)) ctx.lineTo(q.x, q.y);
      ctx.stroke();
    }
    // shoulder line with the text standing on it
    if ((sp.lines || []).length || sp.above || sp.below) {
      ctx.beginPath(); ctx.moveTo(Sh.x, Sh.y); ctx.lineTo(Sh.x + L.side * L.w, Sh.y); ctx.stroke();
      if (size >= 4) {
        ctx.font = `${size}px system-ui, sans-serif`;
        ctx.textBaseline = "middle"; ctx.textAlign = L.side > 0 ? "left" : "right";
        const tx = Sh.x + L.side * size * 0.3;
        const n = sp.lines.length;
        sp.lines.forEach((s, i) => ctx.fillText(s, tx, Sh.y - off - (n - 1 - i) * lh - size / 2));
        if (sp.above) ctx.fillText(sp.above, tx, Sh.y - off - n * lh - size / 2);
        if (sp.below) ctx.fillText(sp.below, tx, Sh.y + off + size / 2);
      }
    }
    ctx.restore();
    if (o.live && o.selected) {
      // grips: the end of the shoulder line, and the bend (or the middle of a straight leader to make one)
      const gripAt = (x, y, type, real) => {
        ctx.save(); ctx.fillStyle = real ? col : H.css("--view-bg"); ctx.strokeStyle = col; ctx.lineWidth = 1.2;
        ctx.fillRect(x - 4, y - 4, 8, 8); ctx.strokeRect(x - 4, y - 4, 8, 8); ctx.restore();
        boxes.push({ type, id: sp.id, x, y, r: 7 });
      };
      gripAt(Sh.x, Sh.y, "shoulder", true);
      if (sp.leader) {
        if (Bd) gripAt(Bd.x, Bd.y, "bend", true);
        else gripAt((T.x + Sh.x) / 2, (T.y + Sh.y) / 2, "bend", false);
      }
      boxes.push({ type: "text", id: sp.id, x0: L.box.x0, x1: L.box.x1, y0: L.box.y0, y1: L.box.y1 });
    }
  }

  function draw(list, sel, hovered) {
    boxes = [];
    const accent = H.css("--accent"), text = H.css("--text");
    for (const sp of list || []) {
      const selected = sel.has(sp.id), hov = hovered && hovered.includes(sp.id);
      const base = sp.style && sp.style.line ? sp.style.line.color : "#000000";
      const col = selected || hov ? accent : sp.invalid ? "#c0392b" : base === "#000000" ? text : base;
      const c = H.ctx();
      c.save();
      if (c.oaTag) c.oaTag({ cat: sp.category || "Spot Elevations", sub: null, kind: "dims" });
      drawOne(sp, { col, selected, live: true });
      c.restore();
    }
  }

  // the face under the cursor, and the spot as it would be placed
  function drawTool() {
    if (!kind) return;
    const ctx = H.ctx(), accent = H.css("--accent");
    if (ghost) drawOne(ghost, { col: accent, ghost: true, live: false });
    if (cur && stage === 0) {
      const q = H.toScreen(cur), c = chosen();
      const text = ghostErr && kind !== "coordinate" ? ghostErr : c ? `${c.label}${cands.length > 1 ? " (Tab for more)" : ""}` : kind === "coordinate" ? "Free point" : "";
      if (text) H.label(text, { x: q.x + 14 + ctx.measureText(text).width / 2, y: q.y - 18 }, ghostErr && kind !== "coordinate" ? "#c0392b" : accent);
    } else if (cur && stage === 1 && tgt) {
      const A = H.toScreen(tgt.p);
      ctx.save(); ctx.fillStyle = accent; ctx.beginPath(); ctx.arc(A.x, A.y, 3, 0, Math.PI * 2); ctx.fill(); ctx.restore();
    }
  }

  // ---- hit testing and selection -----------------------------------------------------------------
  // outlines the Select tool can pick a spot by: the symbol, the leader and the text
  function hitRings(sp) {
    const w = 4 / H.scale();
    const band = (a, b) => {
      const dx = b[0] - a[0], dy = b[1] - a[1], l = Math.hypot(dx, dy) || 1, nx = -dy / l * w, ny = dx / l * w;
      return [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]];
    };
    const L = layout(sp);
    const toW = (x, y) => { const p = H.toWorld(x, y); return [p.x, p.y]; };
    const r = Math.max(L.rad, 5);
    const sym = [toW(L.T.x - r, L.T.y - r), toW(L.T.x + r, L.T.y - r), toW(L.T.x + r, L.T.y + r), toW(L.T.x - r, L.T.y + r)];
    const rings = [sym];
    if (sp.leader) {
      const pts = [sp.target, ...(sp.bend ? [sp.bend] : []), sp.shoulder];
      for (let i = 0; i + 1 < pts.length; i++) rings.push(band(pts[i], pts[i + 1]));
    }
    const b = L.box;
    rings.push([toW(b.x0, b.y0), toW(b.x1, b.y0), toW(b.x1, b.y1), toW(b.x0, b.y1)]);
    return { ring: rings[rings.length - 1], rings };
  }

  // ---- editing a selected spot ---------------------------------------------------------------------
  const selectedSpot = () => { const s = H.selection(); return s.length === 1 ? H.spots().find((d) => d.id === s[0]) : null; };
  const inBox = (b, sx, sy) => sx >= b.x0 - 2 && sx <= b.x1 + 2 && sy >= b.y0 - 2 && sy <= b.y1 + 2;

  function down(sx, sy) {
    if (active()) return false;
    const sp = selectedSpot();
    if (!sp) return false;
    const g = boxes.find((b) => b.id === sp.id && b.type !== "text" && Math.hypot(sx - b.x, sy - b.y) <= b.r);
    const t = !g && boxes.find((b) => b.id === sp.id && b.type === "text" && inBox(b, sx, sy));
    const hit = g || (t && { type: "shoulder" });
    if (!hit) return false;
    if (sp.pinned) return false;
    drag = { id: sp.id, type: hit.type, sx, sy, start: H.toWorld(sx, sy), d: { x: 0, y: 0 }, off: [0, 0], moved: false };
    return true;
  }
  function dragMove(sx, sy) {
    if (!drag) return false;
    if (!drag.moved && Math.hypot(sx - drag.sx, sy - drag.sy) > 5) drag.moved = true;
    if (drag.moved) {
      const p = H.toWorld(sx, sy);
      drag.d = { x: p.x - drag.start.x, y: p.y - drag.start.y };
      const sp = H.spots().find((d) => d.id === drag.id);
      if (sp) drag.off = [p.x - sp.target[0], p.y - sp.target[1]];
      H.render();
    }
    return true;
  }
  async function up(sx, sy) {
    if (!drag) return false;
    const g = drag; drag = null;
    const sp = H.spots().find((d) => d.id === g.id);
    if (sp && g.moved) {
      const p = H.toWorld(sx, sy);
      if (g.type === "shoulder") await H.call("patch_spot", { id: sp.id, patch: { shoulder: [r3(sp.shoulder[0] - sp.target[0] + p.x - g.start.x), r3(sp.shoulder[1] - sp.target[1] + p.y - g.start.y)] } });
      else await H.call("patch_spot", { id: sp.id, patch: { bend: [r3(p.x - sp.target[0]), r3(p.y - sp.target[1])] } });
      await H.refresh();
    }
    H.render();
    return true;
  }
  // a double click on a bend takes the bend out of the leader
  function dblclick(sx, sy) {
    if (active()) return false;
    const sp = selectedSpot();
    if (!sp) return false;
    const g = boxes.find((b) => b.id === sp.id && b.type === "bend" && Math.hypot(sx - b.x, sy - b.y) <= b.r);
    if (!g || !sp.bend) return false;
    H.call("patch_spot", { id: sp.id, patch: { bend: null } }).then(() => H.refresh());
    return true;
  }

  return {
    attach, active, start, stop, cancel, prompt, key, move, click, draw, drawTool, hitRings,
    down, dragMove, up, dblclick,
    category: (k) => CATEGORY[k],
    get kind() { return kind; },
    get stage() { return stage; },
    get dragging() { return !!drag; },
    // test hooks
    grips: () => boxes.filter((b) => b.type !== "text").map((b) => ({ ...b })),
    candidates: () => cands.map((c) => c.label),
    error: () => ghostErr,
    surfaceName: (s) => SURFACE[s],
  };
}
const Spots = makeSpots();
