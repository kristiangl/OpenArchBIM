// One call() for the Rust core. In the desktop app it goes through Tauri;
// in a browser (dev server) it goes over HTTP. The Rust side is identical.
const Backend = {
  isDesktop: !!(window.__TAURI__ && window.__TAURI__.core),

  async call(method, params = {}) {
    // the desktop app queues the request (in the order made) and then waits for its answer, so a long one does not
    // hold the window up
    if (this.isDesktop) {
      const ticket = await window.__TAURI__.core.invoke("rpc_send", { method, params });
      return window.__TAURI__.core.invoke("rpc_wait", { ticket });
    }
    const r = await fetch("/rpc", { method: "POST", body: JSON.stringify({ method, params }) });
    const j = await r.json();
    if (j.err !== undefined) throw new Error(j.err);
    return j.ok;
  },

  // Desktop app: page errors and unhandled promise rejections go into the same log file as the core calls
  // (Help > Open Log Folder). In a browser they stay in the console.
  log(level, message) {
    if (!this.isDesktop) return;
    try {
      window.__TAURI__.core.invoke("ui_log", { level, message: String(message) }).catch(() => {});
    } catch {}
  },

  async fileDialog(kind, options) {
    if (this.isDesktop) return window.__TAURI__.core.invoke("plugin:dialog|" + kind, { options });
    const response = await fetch("/dialog", { method: "POST", body: JSON.stringify({ kind, options }) });
    const result = await response.json();
    if (result.err !== undefined) throw new Error(result.err);
    if (result.unsupported) return this.pathPrompt(options.title || (kind === "save" ? "Save As" : "Open"), options.defaultPath);
    return result.ok;
  },
  pathPrompt(title, current = "") {
    return new Promise((resolve) => openModal(title, (body, foot, close) => {
      const input = document.createElement("input"); input.type = "text"; input.value = current || ""; input.style.width = "100%";
      body.append(input);
      const done = (value) => { close(); resolve(value); };
      body.closest(".modal-back").onEscape = () => done(null);
      foot.append(button("Cancel", () => done(null)), button("OK", () => done(input.value.trim() || null), true));
      input.onkeydown = (e) => { if (e.key === "Enter") done(input.value.trim() || null); };
      setTimeout(() => { input.focus(); input.select(); }, 0);
    }));
  },

  // Native dialogs in the desktop app and Windows browser preview; path dialog on other preview hosts.
  async pickSavePath(current) {
    return this.fileDialog("save", { defaultPath: current || "Untitled.oab", filters: [{ name: "OpenArch BIM project", extensions: ["oab"] }] });
  },

  async pickOpenPath() {
    return this.fileDialog("open", { multiple: false, filters: [{ name: "OpenArch BIM project or IFC file", extensions: ["oab", "ironbark", "ifc"] }, { name: "IFC file", extensions: ["ifc"] }] });
  },

  // Family files (.oaf; .ibfam is the old extension and still opens)
  async pickSaveFamily(current) {
    return this.fileDialog("save", { defaultPath: current || "Family.oaf", filters: [{ name: "OpenArch BIM family", extensions: ["oaf"] }] });
  },

  // Shared parameter files (.txt)
  async pickOpenShared() {
    return this.fileDialog("open", { multiple: false, filters: [{ name: "Shared parameter file", extensions: ["txt"] }] });
  },
  async pickSaveShared(current) {
    return this.fileDialog("save", { defaultPath: current || "Shared Parameters.txt", filters: [{ name: "Shared parameter file", extensions: ["txt"] }] });
  },

  // IFC export (.ifc)
  async pickSaveIfc(current) {
    return this.fileDialog("save", { defaultPath: current || "Project.ifc", filters: [{ name: "IFC4 file", extensions: ["ifc"] }] });
  },

  // Lookup table files (.csv)
  async pickOpenCsv() {
    return this.fileDialog("open", { multiple: false, filters: [{ name: "CSV lookup table", extensions: ["csv"] }] });
  },
  async pickSaveCsv(current) {
    return this.fileDialog("save", { defaultPath: current || "Table.csv", filters: [{ name: "CSV lookup table", extensions: ["csv"] }] });
  },

  async pickFolder() {
    return this.fileDialog("open", { directory: true, multiple: false });
  },
  async pickSaveJson(current) {
    return this.fileDialog("save", { defaultPath: current, filters: [{ name: "Export settings", extensions: ["json"] }] });
  },
  async pickOpenJson() {
    return this.fileDialog("open", { multiple: false, filters: [{ name: "Export settings", extensions: ["json"] }] });
  },

  async pickOpenFamily() {
    return this.fileDialog("open", { multiple: false, filters: [{ name: "OpenArch BIM family", extensions: ["oaf", "ibfam"] }] });
  },
};

if (Backend.isDesktop) {
  window.addEventListener("error", (e) => {
    if (!e.message) return; // a failed image or script load, not an error in the page's code
    const where = e.filename ? ` (${e.filename}:${e.lineno}:${e.colno})` : "";
    Backend.log("error", e.message + where + (e.error && e.error.stack ? "\n" + e.error.stack : ""));
  });
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason;
    Backend.log("unhandled rejection", r && r.stack ? r.stack : String(r));
  });
  // A heartbeat the desktop shell watches: when the page's script stops (a loop that never ends) the beats stop and the
  // shell writes that to the log. Each beat also asks for one animation frame; a frame that has not come by the next
  // beat, with the window showing, means drawing has stopped while the script still runs (the GPU or the compositor).
  let frameDue = 0, framesStuck = false;
  setInterval(() => {
    const now = Date.now();
    if (frameDue && document.visibilityState === "visible" && now - frameDue > 4000 && !framesStuck) {
      framesStuck = true;
      Backend.log("warn", `no frame drawn for ${Math.round((now - frameDue) / 1000)} s while the page script runs`);
    }
    if (!frameDue) {
      frameDue = now;
      requestAnimationFrame(() => {
        if (framesStuck) Backend.log("warn", `frames drawn again after ${Math.round((Date.now() - frameDue) / 1000)} s`);
        frameDue = 0; framesStuck = false;
      });
    }
    try { window.__TAURI__.core.invoke("ui_alive").catch(() => {}); } catch {}
  }, 2000);
}
