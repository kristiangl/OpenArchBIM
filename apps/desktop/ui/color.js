// Colour fields and the Color dialog. A colour field is a button showing the colour and its RGB values; it behaves like
// a colour input (a `value` of "#rrggbb", and `input` and `change` events), so it drops in where one was. The dialog
// has the basic colours, custom colours the person keeps (saved in this browser, shared by every project), a
// saturation and lightness square with a hue strip, and RGB and hex values.
const ColorPick = (() => {
  const BASIC = [
    "#ff8080", "#ffff80", "#80ff80", "#00ff80", "#80ffff", "#0080ff", "#ff80c0", "#ff80ff",
    "#ff0000", "#ffff00", "#80ff00", "#00ff40", "#00ffff", "#0080c0", "#8080c0", "#ff00ff",
    "#804040", "#ff8040", "#00ff00", "#008080", "#004080", "#8080ff", "#800040", "#ff0080",
    "#800000", "#ff8000", "#008000", "#008040", "#0000ff", "#0000a0", "#800080", "#8000ff",
    "#400000", "#804000", "#004000", "#004040", "#000080", "#000040", "#400040", "#400080",
    "#000000", "#808000", "#808040", "#808080", "#408080", "#c0c0c0", "#400040", "#ffffff",
  ];
  const KEY = "openarch.customColors", SLOTS = 16;
  const loadCustom = () => {
    try { const v = JSON.parse(localStorage.getItem(KEY) || "[]"); return Array.from({ length: SLOTS }, (_, i) => (typeof v[i] === "string" ? v[i] : null)); }
    catch { return Array(SLOTS).fill(null); }
  };
  const saveCustom = (list) => { try { localStorage.setItem(KEY, JSON.stringify(list)); } catch { /* storage off: kept for this session only */ } };

  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const toHex = (rgb) => "#" + rgb.map((x) => clamp(Math.round(x), 0, 255).toString(16).padStart(2, "0")).join("");
  const toRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const norm = (hex) => {
    const m = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.exec(String(hex || "").trim());
    if (!m) return null;
    const h = m[1].length === 3 ? m[1].split("").map((c) => c + c).join("") : m[1];
    return "#" + h.toLowerCase();
  };
  // hue (degrees), saturation and value (0 to 1)
  function toHsv([r, g, b]) {
    r /= 255; g /= 255; b /= 255;
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
    let h = 0;
    if (d) h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return [((h * 60) + 360) % 360, mx ? d / mx : 0, mx];
  }
  function fromHsv(h, s, v) {
    const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
    const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
    return [(r + m) * 255, (g + m) * 255, (b + m) * 255].map(Math.round);
  }
  const rgbText = (hex) => { const n = norm(hex); return n ? "RGB " + toRgb(n).map((x) => String(x).padStart(3, "0")).join("-") : ""; };

  const h = (tag, props, ...kids) => { const e = document.createElement(tag); Object.assign(e, props || {}); e.append(...kids.filter((k) => k != null)); return e; };

  /** The Color dialog: resolves to the colour picked ("#rrggbb"), or null when cancelled. */
  function open(start) {
    return new Promise((resolve) => {
      const original = norm(start) || "#000000";
      let cur = original, hsv = toHsv(toRgb(cur)), custom = loadCustom(), slot = custom.findIndex((c) => !c);
      openModal("Color", (body, foot, close) => {
        const done = (v) => { close(); resolve(v); };
        body.closest(".modal-back").onEscape = () => done(null);
        const basic = h("div", { className: "cp-grid" }), mine = h("div", { className: "cp-grid" });
        const sv = h("canvas", { className: "cp-sv", width: 220, height: 160 }), hue = h("canvas", { className: "cp-hue", width: 220, height: 14 });
        const was = h("div", { className: "cp-swatch" }), now = h("div", { className: "cp-swatch" });
        const num = (label) => { const i = h("input", { type: "number", min: 0, max: 255, step: 1, className: "cp-num" }); return [i, h("label", { className: "cp-field" }, label, i)]; };
        const [rI, rL] = num("R"), [gI, gL] = num("G"), [bI, bL] = num("B");
        const hexI = h("input", { className: "cp-hex", maxLength: 7, spellcheck: false }), hexL = h("label", { className: "cp-field" }, "Hex", hexI);
        const addB = h("button", { textContent: "Add to Custom Colors" }), delB = h("button", { textContent: "Remove" });

        const swatch = (c, onclick, on) => { const s = h("button", { className: "cp-cell" + (on ? " on" : "") + (c ? "" : " empty"), title: c ? `${c.toUpperCase()}  ${rgbText(c)}` : "Empty" }); if (c) s.style.background = c; s.onclick = onclick; return s; };
        function drawGrids() {
          basic.replaceChildren(...BASIC.map((c) => swatch(c, () => set(c, true), c === cur)));
          mine.replaceChildren(...custom.map((c, i) => swatch(c, () => { slot = i; if (c) set(c, true); else drawGrids(); }, i === slot)));
          delB.disabled = slot < 0 || !custom[slot];
        }
        function drawPickers() {
          const g = sv.getContext("2d"), W = sv.width, H = sv.height;
          g.fillStyle = toHex(fromHsv(hsv[0], 1, 1)); g.fillRect(0, 0, W, H);
          const white = g.createLinearGradient(0, 0, W, 0); white.addColorStop(0, "#fff"); white.addColorStop(1, "rgba(255,255,255,0)");
          g.fillStyle = white; g.fillRect(0, 0, W, H);
          const black = g.createLinearGradient(0, 0, 0, H); black.addColorStop(0, "rgba(0,0,0,0)"); black.addColorStop(1, "#000");
          g.fillStyle = black; g.fillRect(0, 0, W, H);
          const x = hsv[1] * W, y = (1 - hsv[2]) * H;
          g.lineWidth = 2; g.strokeStyle = hsv[2] > 0.5 ? "#000" : "#fff"; g.beginPath(); g.arc(x, y, 6, 0, Math.PI * 2); g.stroke();
          const k = hue.getContext("2d"), hw = hue.width, hh = hue.height, grad = k.createLinearGradient(0, 0, hw, 0);
          for (let i = 0; i <= 6; i++) grad.addColorStop(i / 6, toHex(fromHsv(i * 60 % 360, 1, 1)));
          k.fillStyle = grad; k.fillRect(0, 0, hw, hh);
          const hx = (hsv[0] / 360) * hw;
          k.strokeStyle = "#fff"; k.lineWidth = 3; k.strokeRect(hx - 3, 0.5, 6, hh - 1); k.strokeStyle = "#000"; k.lineWidth = 1; k.strokeRect(hx - 3, 0.5, 6, hh - 1);
        }
        function showValues(skip) {
          const [r, g, b] = toRgb(cur);
          if (skip !== rI) rI.value = r;
          if (skip !== gI) gI.value = g;
          if (skip !== bI) bI.value = b;
          if (skip !== hexI) hexI.value = cur.toUpperCase();
          now.style.background = cur;
        }
        // a new colour; `fromRgb` works its hue out again (picking from the square or strip keeps the hue)
        function set(c, fromRgb, skip) {
          const n = norm(c);
          if (!n) return;
          cur = n;
          if (fromRgb) { const t = toHsv(toRgb(n)); hsv = t[1] === 0 ? [hsv[0], 0, t[2]] : t; }
          showValues(skip); drawPickers(); drawGrids();
        }
        const drag = (canvas, fn) => {
          const at = (e) => { const r = canvas.getBoundingClientRect(); fn(clamp((e.clientX - r.left) / r.width, 0, 1), clamp((e.clientY - r.top) / r.height, 0, 1)); };
          canvas.onpointerdown = (e) => { canvas.setPointerCapture(e.pointerId); at(e); canvas.onpointermove = at; };
          canvas.onpointerup = () => { canvas.onpointermove = null; };
        };
        drag(sv, (u, v) => { hsv = [hsv[0], u, 1 - v]; set(toHex(fromHsv(...hsv)), false); });
        drag(hue, (u) => { hsv = [Math.min(359.9, u * 360), hsv[1], hsv[2]]; set(toHex(fromHsv(...hsv)), false); });
        for (const i of [rI, gI, bI]) i.oninput = () => { const v = [rI, gI, bI].map((x) => clamp(parseInt(x.value, 10) || 0, 0, 255)); set(toHex(v), true, i); };
        hexI.oninput = () => { if (norm(hexI.value)) set(hexI.value, true, hexI); };
        hexI.onblur = () => showValues();
        addB.onclick = () => {
          // into the custom colour picked, else the first empty one; when all are full the oldest goes
          let i = slot >= 0 ? slot : custom.findIndex((c) => !c);
          if (i < 0) { custom = [...custom.slice(1), null]; i = SLOTS - 1; }
          custom[i] = cur; saveCustom(custom);
          slot = custom.findIndex((c) => !c);
          drawGrids();
        };
        delB.onclick = () => { if (slot < 0) return; custom[slot] = null; saveCustom(custom); drawGrids(); };
        was.style.background = original; was.title = "Original"; was.onclick = () => set(original, true);
        body.append(h("div", { className: "cp" },
          h("div", { className: "cp-left" },
            h("div", { className: "cp-title", textContent: "Basic colors" }), basic,
            h("div", { className: "cp-title", textContent: "Custom colors" }), mine,
            h("div", { className: "cp-row" }, addB, delB)),
          h("div", { className: "cp-right" }, sv, hue,
            h("div", { className: "cp-row" }, h("div", { className: "cp-pair" }, h("span", { textContent: "Original" }), was), h("div", { className: "cp-pair" }, h("span", { textContent: "New" }), now)),
            h("div", { className: "cp-row" }, rL, gL, bL),
            h("div", { className: "cp-row" }, hexL))));
        foot.append(button("OK", () => done(cur), true), button("Cancel", () => done(null)));
        set(cur, true);
      });
    });
  }

  /** A colour field: a button with the colour and its RGB values that opens the Color dialog. */
  function field(hex) {
    const b = h("button", { type: "button", className: "color-field" });
    const sw = h("span", { className: "color-swatch" }), tx = h("span", { className: "color-text" });
    b.append(sw, tx);
    let value = norm(hex) || "#000000";
    const show = () => { sw.style.background = value; tx.textContent = rgbText(value); b.title = value.toUpperCase(); };
    Object.defineProperty(b, "value", { get: () => value, set: (v) => { value = norm(v) || value; show(); } });
    b.addEventListener("click", async (e) => {
      e.preventDefault();
      if (b.disabled) return;
      const v = await open(value);
      if (v == null || v === value) return;
      value = v; show();
      b.dispatchEvent(new Event("input", { bubbles: true }));
      b.dispatchEvent(new Event("change", { bubbles: true }));
    });
    show();
    return b;
  }

  return { open, field, toHex, toRgb };
})();
