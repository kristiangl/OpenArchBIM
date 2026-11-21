// Project Units (Manage > Project Units): how lengths, areas, volumes and angles are written across the project
// (dimensions whose type uses project settings, room and area tags, Properties and schedules), with the decimal
// symbol and digit grouping. Each row shows a sample as it will read. OK is one undo step (the core: units.rs).
const ProjUnits = (() => {
  const LENGTH = [["millimetres", "Millimetres"], ["centimetres", "Centimetres"], ["metres", "Metres"], ["feet_inches", "Feet and fractional inches"]];
  const AREA = [["square_metres", "Square metres"], ["square_millimetres", "Square millimetres"], ["square_feet", "Square feet"]];
  const VOLUME = [["cubic_metres", "Cubic metres"], ["cubic_feet", "Cubic feet"]];
  const FRACTIONS = [2, 4, 8, 16, 32, 64];

  // the samples, written as the core will (units.rs): grouping and the decimal symbol last
  function local(u, s) {
    if (!u.decimal_comma && !u.digit_grouping) return s;
    const point = u.decimal_comma ? "," : ".", group = u.decimal_comma ? "." : ",";
    return s.replace(/(\d+)(?:\.(\d+))?/g, (m, whole, dec) => {
      const w = u.digit_grouping && whole.length > 3 ? whole.replace(/\B(?=(\d{3})+(?!\d))/g, group) : whole;
      return dec != null ? `${w}${point}${dec}` : w;
    });
  }
  function fixed(v, d, suppress) {
    let s = v.toFixed(d);
    if (suppress && d > 0) s = s.replace(/0+$/, "").replace(/\.$/, "");
    return s;
  }
  function length(u, mm) {
    const f = u.length;
    if (f.unit === "feet_inches") {
      const den = f.fraction || 16, units = Math.round((mm / 25.4) * den), feet = Math.floor(units / (12 * den)), rem = units % (12 * den);
      const whole = Math.floor(rem / den), fr = rem % den, g = (a, b) => (b ? g(b, a % b) : a), k = fr ? g(fr, den) : 1;
      return `${feet}' - ${whole}${fr ? ` ${fr / k}/${den / k}` : ""}"`;
    }
    const [div, sym] = { millimetres: [1, " mm"], centimetres: [10, " cm"], metres: [1000, " m"] }[f.unit];
    return local(u, fixed(mm / div, f.decimals, f.suppress_trailing_zeros) + (f.symbol ? sym : ""));
  }
  function area(u, mm2) {
    const [div, sym] = { square_metres: [1e6, " m²"], square_millimetres: [1, " mm²"], square_feet: [304.8 * 304.8, " ft²"] }[u.area.unit];
    return local(u, fixed(mm2 / div, u.area.decimals, false) + (u.area.symbol ? sym : ""));
  }
  function volume(u, mm3) {
    const [div, sym] = { cubic_metres: [1e9, " m³"], cubic_feet: [304.8 ** 3, " ft³"] }[u.volume.unit];
    return local(u, fixed(mm3 / div, u.volume.decimals, false) + (u.volume.symbol ? sym : ""));
  }

  async function open() {
    const r = await run("project_units", {});
    if (!r) return;
    const u = JSON.parse(JSON.stringify(r));
    openModal("Project Units", (body, foot, close) => {
      const sample = {};
      const show = () => {
        sample.length.textContent = length(u, 12345.6);
        sample.area.textContent = area(u, 1234.5e6);
        sample.volume.textContent = volume(u, 3456.78e9);
        sample.angle.textContent = local(u, `${fixed(22.5, u.angle_decimals, true)}°`);
      };
      const sel = (opts, now, set) => { const s = el("select", { onchange: () => { set(s.value); show(); } }, opts.map(([v, l]) => new Option(l, v, false, v === now))); return s; };
      const places = (now, set) => { const n = el("input", { type: "number", min: "0", max: "6", step: "1", value: now, style: "width:56px", onchange: () => { const v = Math.max(0, Math.min(6, Math.round(Number(n.value) || 0))); n.value = v; set(v); show(); } }); return el("label", {}, ["Places ", n]); };
      const tick = (label, now, set) => { const c = el("input", { type: "checkbox", checked: !!now, onchange: () => { set(c.checked); show(); } }); return el("label", {}, [c, ` ${label}`]); };
      // a row of the units grid: name, unit, places, options, then the sample
      const row = (name, key, parts) => { sample[key] = el("span", { className: "muted", style: "text-align:right;white-space:nowrap" }); return el("div", { style: "display:grid;grid-template-columns:60px 180px 92px minmax(0,1fr) 120px;gap:8px;align-items:center;margin:6px 0" }, [el("strong", { textContent: name }), ...parts, sample[key]]); };
      const opts = (...xs) => el("div", { style: "display:flex;gap:4px 12px;flex-wrap:wrap" }, xs.map((x) => { x.style.whiteSpace = "nowrap"; return x; }));
      const frac = sel(FRACTIONS.map((d) => [String(d), `1/${d}"`]), String(u.length.fraction || 16), (v) => { u.length.fraction = Number(v); });
      body.append(
        el("h4", { textContent: "Units", style: "margin:4px 0 6px" }),
        row("Length", "length", [
          sel(LENGTH, u.length.unit, (v) => { u.length.unit = v; }),
          places(u.length.decimals, (v) => { u.length.decimals = v; }),
          opts(tick("Suppress trailing 0s", u.length.suppress_trailing_zeros, (v) => { u.length.suppress_trailing_zeros = v; }), tick("Unit symbol", u.length.symbol, (v) => { u.length.symbol = v; }), el("label", {}, ["Inches to ", frac])),
        ]),
        row("Area", "area", [sel(AREA, u.area.unit, (v) => { u.area.unit = v; }), places(u.area.decimals, (v) => { u.area.decimals = v; }), opts(tick("Unit symbol", u.area.symbol, (v) => { u.area.symbol = v; }))]),
        row("Volume", "volume", [sel(VOLUME, u.volume.unit, (v) => { u.volume.unit = v; }), places(u.volume.decimals, (v) => { u.volume.decimals = v; }), opts(tick("Unit symbol", u.volume.symbol, (v) => { u.volume.symbol = v; }))]),
        row("Angle", "angle", [el("span", { textContent: "Degrees" }), places(u.angle_decimals, (v) => { u.angle_decimals = v; }), el("span")]),
        el("h4", { textContent: "Numbers", style: "margin:12px 0 6px" }),
        el("div", { style: "display:flex;gap:18px;align-items:center" }, [
          el("label", {}, ["Decimal symbol ", sel([["dot", "123.45 (full stop)"], ["comma", "123,45 (comma)"]], u.decimal_comma ? "comma" : "dot", (v) => { u.decimal_comma = v === "comma"; })]),
          tick("Digit grouping (12,000)", u.digit_grouping, (v) => { u.digit_grouping = v; }),
        ]),
        el("p", { className: "muted", textContent: "Dimension types set to Use Project Settings write lengths this way; room and area tags, Properties and schedules write areas and volumes this way.", style: "margin-top:12px;max-width:560px" }),
      );
      show();
      foot.append(button("Cancel", close), button("OK", async () => {
        if (await run("set_project_units", { units: u })) {
          close();
          await refresh();
          if (typeof refreshView === "function") await refreshView(true);
          renderProperties();
          setPrompt("Project units changed.");
        }
      }, true));
    }, { wide: true });
  }
  return { open, local, length, area, volume };
})();
