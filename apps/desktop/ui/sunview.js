// The sun in 3D views (core: sun.rs; private notes: sun.md). Each 3D view has its own sun: lighting over the
// viewer's shoulder, a moment at the project's location, or a single-day or multi-day solar study stepped frame by
// frame. Cast Shadows and the Sun Path diagram are switched on the view bar; the sun bar over the view sets the time
// or steps the study; Sun Settings sets the rest (and the project's latitude, longitude and time zone).
const SunUI = (() => {
  let s = null; // the core's answer for the open view ("sun")
  let labels = [], disc = null, centre = null;
  let shown = null; // the study frame shown while playing (not saved until stopped)
  let timer = null;

  const viewId = () => (app.view.kind === "3d" ? app.view.viewId : null);
  const fmtTime = (m) => `${String(Math.floor(m / 60) % 24).padStart(2, "0")}:${String(Math.round(m) % 60).padStart(2, "0")}`;

  async function load() {
    const id = viewId();
    if (id == null || !app.vp) { s = null; syncButtons(); renderBar(); draw(); return; }
    try { s = await Backend.call("sun", { view_id: id }); } catch { s = null; }
    shown = null;
    apply();
  }

  // a compass bearing and altitude as the direction toward the sun in the model (true north turned as the core does)
  function dirOf(alt, az) {
    const north = (s?.place?.true_north || 0) * Math.PI / 180, b = az * Math.PI / 180, a = alt * Math.PI / 180;
    const e = Math.sin(b), n = Math.cos(b), x = e * Math.cos(north) - n * Math.sin(north), y = e * Math.sin(north) + n * Math.cos(north);
    return [x * Math.cos(a), y * Math.cos(a), Math.sin(a)];
  }

  function current() {
    if (!s || !s.frames.length) return null;
    return s.frames[Math.min(shown ?? s.frame, s.frames.length - 1)];
  }

  function apply() {
    const vp = app.vp;
    if (!vp) return;
    if (!s) { vp.setSun(null); vp.setSunLines(null); labels = []; disc = null; syncButtons(); renderBar(); draw(); return; }
    const f = current();
    const relative = s.settings.mode === "lighting";
    vp.setSun({
      dir: relative || !f ? null : dirOf(f.altitude_deg, f.azimuth_deg),
      relative: { az: s.lighting.azimuth, alt: s.lighting.altitude },
      intensity: s.sun_intensity, ambient: s.ambient_intensity,
      shadows: s.cast_shadows, shadowStrength: s.shadow_intensity / 100, groundZ: s.ground_z_mm / 1000,
    });
    labels = []; disc = null; centre = null;
    if (s.path) {
      const [lo, hi] = vp.bounds();
      const c = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, s.ground_z_mm / 1000];
      // round the model, at the ground plane
      const R = Math.max(5, (Math.hypot(hi[0] - lo[0], hi[1] - lo[1]) / 2) * 1.3) * (s.path_size_pct / 100);
      const P = (q) => [c[0] + q[0] * R, c[1] + q[1] * R, c[2] + q[2] * R];
      const dark = matchMedia("(prefers-color-scheme: dark)").matches;
      const grey = dark ? [0.6, 0.62, 0.66] : [0.45, 0.47, 0.5], faint = dark ? [0.38, 0.4, 0.44] : [0.75, 0.76, 0.78];
      const out = [];
      const poly = (pts, col) => { for (let i = 0; i + 1 < pts.length; i++) out.push(...P(pts[i]), ...col, ...P(pts[i + 1]), ...col); };
      poly(s.path.ring, grey);
      for (const [a, b] of s.path.ticks) out.push(...P(a), ...grey, ...P(b), ...grey);
      for (const l of s.path.analemmas) poly(l, faint);
      for (const l of s.path.summer) poly(l, [0.85, 0.3, 0.25]);
      for (const l of s.path.winter) poly(l, [0.25, 0.45, 0.85]);
      for (const l of s.path.day) poly(l, [0.95, 0.6, 0.1]);
      // the sun itself, while a study plays where it is now
      const sunPt = f && !relative ? P(dirOf(f.altitude_deg, f.azimuth_deg)) : null;
      if (sunPt && f.altitude_deg > 0) { out.push(...c, 0.95, 0.75, 0.2, ...sunPt, 0.95, 0.75, 0.2); disc = sunPt; }
      labels = s.path.labels.map((l) => ({ ...l, at: P(l.at) }));
      centre = c;
      vp.setSunLines(new Float32Array(out));
    } else vp.setSunLines(null);
    syncButtons(); renderBar(); vp.poke();
  }

  // compass and hour names, the sun disc, over the view
  function draw() {
    const ov = document.getElementById("vp-sun");
    if (!ov) return;
    const on = app.view.kind === "3d" && !!s && (labels.length || disc);
    ov.classList.toggle("on", !!on);
    if (!on || !app.vp) return;
    const cv = $("#viewport");
    if (ov.width !== cv.clientWidth || ov.height !== cv.clientHeight) { ov.width = cv.clientWidth; ov.height = cv.clientHeight; }
    const c = ov.getContext("2d");
    c.clearRect(0, 0, ov.width, ov.height);
    const pr = app.vp.projector();
    c.font = "12px system-ui, sans-serif"; c.textAlign = "center"; c.textBaseline = "middle";
    const dark = matchMedia("(prefers-color-scheme: dark)").matches;
    for (const l of labels) {
      const p = pr(...l.at);
      if (!p) continue;
      c.fillStyle = l.kind === "compass" ? (dark ? "#dfe3e8" : "#30343a") : (dark ? "#c9a46a" : "#9a5a12");
      c.font = l.kind === "compass" ? "bold 13px system-ui, sans-serif" : "11px system-ui, sans-serif";
      c.fillText(l.text, p.x, p.y);
    }
    if (disc) {
      const p = pr(...disc);
      if (p) { c.beginPath(); c.arc(p.x, p.y, 9, 0, Math.PI * 2); c.fillStyle = "#ffcc33"; c.fill(); c.strokeStyle = "#d98a00"; c.lineWidth = 2; c.stroke(); }
    }
  }

  async function set(patch) {
    const id = viewId();
    if (id == null) return null;
    const r = await run("set_sun", { view_id: id, patch });
    if (r) { s = r; shown = null; apply(); }
    return r;
  }

  function syncButtons() {
    const is3d = app.view.kind === "3d" && !!s;
    const sp = $("#vb-sunpath"), sh = $("#vb-shadows"), st = $("#vb-sunset");
    if (!sp) return;
    for (const b of [sp, sh, st]) b.disabled = !is3d || (b !== st && !!s?.locked);
    sp.classList.toggle("on", is3d && s.sun_path);
    sh.classList.toggle("on", is3d && s.cast_shadows);
  }

  // the sun bar over a 3D view: the moment shown, the time or the study's frames
  function renderBar() {
    const bar = document.getElementById("sun-bar");
    if (!bar) return;
    const on = app.view.kind === "3d" && !!s && (s.sun_path || s.cast_shadows);
    bar.hidden = !on;
    if (!on) return;
    const mode = s.settings.mode, f = current();
    const kids = [];
    if (mode === "lighting") {
      kids.push(el("span", { textContent: "Lighting: relative to the view" }));
    } else if (mode === "still" && !s.settings.manual) {
      const date = el("input", { type: "date", value: s.settings.date, onchange: () => { if (date.value) set({ date: date.value }); } });
      const lab = el("span", { className: "sun-time", textContent: fmtTime(s.settings.time_min) });
      const t = el("input", { type: "range", min: "0", max: "1435", step: "5", value: String(s.settings.time_min),
        oninput: () => { lab.textContent = fmtTime(Number(t.value)); }, onchange: () => set({ time_min: Number(t.value) }) });
      kids.push(date, t, lab);
    } else if (mode === "still") {
      kids.push(el("span", { textContent: f ? f.label : "" }));
    } else {
      const n = s.frames.length, i = Math.min(shown ?? s.frame, n - 1);
      const go = (k) => { stop(); const j = (k + n) % n; set({ frame: j }); };
      kids.push(
        button("◀", () => go(i - 1)),
        el("span", { className: "sun-time", textContent: `${i + 1} / ${n}` }),
        button("▶", () => go(i + 1)),
        button(timer ? "Stop" : "Play", () => (timer ? stop(true) : play())),
      );
    }
    if (f && mode !== "lighting") kids.push(el("span", { className: "sun-where", textContent: `${f.label}${f.altitude_deg > 0 ? `: altitude ${Math.round(f.altitude_deg)}°, azimuth ${Math.round(f.azimuth_deg)}°` : ": the sun is down"}` }));
    if (s.place.assumed && mode !== "lighting") kids.push(el("span", { className: "sun-where", textContent: "(location not set: Perth assumed)" }));
    kids.push(button("Sun Settings…", open));
    bar.replaceChildren(...kids);
  }

  // a study played in the view: frame after frame, then saved where it stops
  function play() {
    if (!s || !s.frames.length) return;
    shown = s.frame;
    timer = setInterval(() => { shown = (shown + 1) % s.frames.length; apply(); }, 700);
    renderBar();
  }
  function stop(save) {
    if (!timer) return;
    clearInterval(timer); timer = null;
    const at = shown;
    if (save && at != null) set({ frame: at }); else renderBar();
  }

  // Sun Settings
  async function open() {
    if (!s) return;
    const st = { ...s.settings };
    const loc = { lat: s.place.assumed ? "" : String(s.place.latitude), lon: s.place.assumed ? "" : String(s.place.longitude), tz: s.place.assumed ? "" : String(s.place.utc_hours) };
    const gd = { sun_intensity: s.sun_intensity, ambient_intensity: s.ambient_intensity, lighting_azimuth: s.lighting.azimuth, lighting_altitude: s.lighting.altitude };
    openModal("Sun Settings", (body, foot, close) => {
      const host = el("div", { className: "sun-dlg" });
      const row = (label, ...kids) => el("div", { className: "row" }, [el("label", { textContent: label }), ...kids]);
      const inp = (obj, key, type = "text", extra = {}) => el("input", { type, value: obj[key] ?? "", ...extra, onchange: (e) => { obj[key] = type === "number" ? Number(e.target.value) : e.target.value; } });
      const time = (key) => el("input", { type: "time", value: fmtTime(st[key]), onchange: (e) => { const [h, m] = e.target.value.split(":").map(Number); st[key] = h * 60 + m; } });
      const sel = (key, opts, num) => { const x = el("select", { onchange: () => { st[key] = num ? Number(x.value) : x.value; draw2(); } }); for (const [v, l] of opts) { const o = new Option(l, String(v)); o.selected = String(st[key]) === String(v); x.add(o); } return x; };
      const chk = (key, label) => el("label", { className: "row" }, [el("input", { type: "checkbox", checked: !!st[key], onchange: (e) => { st[key] = e.target.checked; draw2(); } }), el("span", { textContent: ` ${label}` })]);
      const draw2 = () => {
        const parts = [row("Mode", sel("mode", [["lighting", "Lighting (relative to the view)"], ["still", "Still"], ["single_day", "Single-Day study"], ["multi_day", "Multi-Day study"]]))];
        if (st.mode === "lighting") {
          parts.push(row("Azimuth", inp(gd, "lighting_azimuth", "number", { step: "5" })), row("Altitude", inp(gd, "lighting_altitude", "number", { min: "0", max: "90", step: "5" })),
            el("p", { className: "note", textContent: "The light turns with the camera: azimuth 135 and altitude 35 come from over your left shoulder." }));
        } else {
          parts.push(el("div", { className: "opt-head", textContent: "Location" }),
            row("Latitude", inp(loc, "lat", "number", { step: "0.0001", placeholder: "-31.9523" })),
            row("Longitude", inp(loc, "lon", "number", { step: "0.0001", placeholder: "115.8613" })),
            row("Time zone (UTC +/- hours)", inp(loc, "tz", "number", { step: "0.5", placeholder: "8" })),
            el("p", { className: "note", textContent: s.place.assumed ? "The project has no location yet: Perth (UTC+8) is assumed until one is set. South latitudes and west longitudes are negative." : "South latitudes and west longitudes are negative. Daylight saving is not applied: give the time zone in force on the date." }),
            el("div", { className: "opt-head", textContent: "When" }));
          if (st.mode === "still") {
            parts.push(chk("manual", "Set the azimuth and altitude directly"));
            if (st.manual) parts.push(row("Azimuth (from true north)", inp(st, "azimuth_deg", "number", { step: "5" })), row("Altitude", inp(st, "altitude_deg", "number", { min: "-90", max: "90", step: "5" })));
            else parts.push(row("Date", inp(st, "date", "date")), row("Time", time("time_min")));
          } else if (st.mode === "single_day") {
            parts.push(row("Date", inp(st, "date", "date")), chk("sunrise_to_sunset", "Sunrise to sunset"));
            if (!st.sunrise_to_sunset) parts.push(row("From", time("start_min")), row("To", time("end_min")));
            parts.push(row("Every", sel("interval_min", [[15, "15 minutes"], [30, "30 minutes"], [60, "hour"]], true)));
          } else {
            parts.push(row("From", inp(st, "start_date", "date")), row("To", inp(st, "end_date", "date")), row("Time", time("time_min")),
              row("Every", sel("step_days", [[1, "day"], [7, "week"], [30, "month"]], true)));
          }
        }
        const lv = el("select", { onchange: () => { st.ground_level = lv.value ? Number(lv.value) : null; } });
        lv.add(new Option("Lowest level", ""));
        for (const l of s.levels) { const o = new Option(l.name, String(l.id)); o.selected = st.ground_level === l.id; lv.add(o); }
        parts.push(el("div", { className: "opt-head", textContent: "Ground plane and lighting" }),
          row("Ground plane at", lv),
          row("Sun intensity (0-100)", inp(gd, "sun_intensity", "number", { min: "0", max: "100" })),
          row("Ambient light (0-100)", inp(gd, "ambient_intensity", "number", { min: "0", max: "100" })),
          row("Shadow intensity (0-100)", inp(st, "shadow_intensity", "number", { min: "0", max: "100" })),
          row("Sun path size (100-300 %)", inp(st, "path_size_pct", "number", { min: "100", max: "300", step: "25" })));
        host.replaceChildren(...parts);
      };
      draw2();
      body.append(host);
      foot.append(button("OK", async () => {
        // the location belongs to the project; the rest to this view
        const lat = loc.lat === "" ? null : Number(loc.lat), lon = loc.lon === "" ? null : Number(loc.lon), tz = loc.tz === "" ? null : Number(loc.tz);
        if (st.mode !== "lighting" && (lat !== null || lon !== null || tz !== null) && !(s.place.assumed && lat === null && lon === null && tz === null)) {
          if ((lat === null) !== (lon === null)) { setPrompt("Give both the latitude and the longitude.", true); return; }
          if (!(await run("patch_project_location", { patch: { latitude_deg: lat, longitude_deg: lon, time_zone_hours: tz } }))) return;
        }
        const patch = { ...st, ...gd };
        delete patch.frame;
        if (await set(patch)) close();
      }, true), button("Cancel", close));
    }, { wide: false });
  }

  return { load, draw, set, open, syncButtons, stop: () => stop(false) };
})();
