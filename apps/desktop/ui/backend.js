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

  // File dialogs: native in the desktop app, a plain prompt in a browser.
  async pickSavePath(current) {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.save({ defaultPath: current || "Untitled.oab", filters: [{ name: "OpenArch BIM project", extensions: ["oab"] }] });
    }
    return window.prompt("Save as (file path)", current || "untitled.oab");
  },

  async pickOpenPath() {
    if (this.isDesktop && window.__TAURI__.dialog) {
      // .ironbark is the old extension (before the rename); those files still open
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "OpenArch BIM project or IFC file", extensions: ["oab", "ironbark", "ifc"] }, { name: "IFC file", extensions: ["ifc"] }] });
    }
    return window.prompt("Open (file path)");
  },

  // Family files (.oaf; .ibfam is the old extension and still opens)
  async pickSaveFamily(current) {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.save({ defaultPath: current || "Family.oaf", filters: [{ name: "OpenArch BIM family", extensions: ["oaf"] }] });
    }
    return window.prompt("Save family as (file path)", current || "family.oaf");
  },

  // Shared parameter files (.txt)
  async pickOpenShared() {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "Shared parameter file", extensions: ["txt"] }] });
    }
    return window.prompt("Shared parameter file (path)");
  },
  async pickSaveShared(current) {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.save({ defaultPath: current || "Shared Parameters.txt", filters: [{ name: "Shared parameter file", extensions: ["txt"] }] });
    }
    return window.prompt("Create shared parameter file at (path)", current || "shared parameters.txt");
  },

  // IFC export (.ifc)
  async pickSaveIfc(current) {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.save({ defaultPath: current || "Project.ifc", filters: [{ name: "IFC4 file", extensions: ["ifc"] }] });
    }
    return window.prompt("Export IFC as (file path)", current || "project.ifc");
  },

  // Lookup table files (.csv)
  async pickOpenCsv() {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "CSV lookup table", extensions: ["csv"] }] });
    }
    return window.prompt("Import lookup table (CSV file path)");
  },
  async pickSaveCsv(current) {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.save({ defaultPath: current || "Table.csv", filters: [{ name: "CSV lookup table", extensions: ["csv"] }] });
    }
    return window.prompt("Export lookup table as (file path)", current || "table.csv");
  },

  async pickFolder() {
    if (this.isDesktop && window.__TAURI__.dialog) return window.__TAURI__.dialog.open({ directory: true, multiple: false });
    return window.prompt("Output folder");
  },
  async pickSaveJson(current) {
    if (this.isDesktop && window.__TAURI__.dialog) return window.__TAURI__.dialog.save({ defaultPath: current, filters: [{ name: "Export settings", extensions: ["json"] }] });
    return window.prompt("Save settings as (file path)", current);
  },
  async pickOpenJson() {
    if (this.isDesktop && window.__TAURI__.dialog) return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "Export settings", extensions: ["json"] }] });
    return window.prompt("Load settings (file path)");
  },

  async pickOpenFamily() {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "OpenArch BIM family", extensions: ["oaf", "ibfam"] }] });
    }
    return window.prompt("Load family (file path)");
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
