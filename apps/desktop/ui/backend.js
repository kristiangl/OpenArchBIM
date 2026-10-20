// One call() for the Rust core. In the desktop app it goes through Tauri;
// in a browser (dev server) it goes over HTTP. The Rust side is identical.
const Backend = {
  isDesktop: !!(window.__TAURI__ && window.__TAURI__.core),

  async call(method, params = {}) {
    if (this.isDesktop) return window.__TAURI__.core.invoke("rpc", { method, params });
    const r = await fetch("/rpc", { method: "POST", body: JSON.stringify({ method, params }) });
    const j = await r.json();
    if (j.err !== undefined) throw new Error(j.err);
    return j.ok;
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
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "OpenArch BIM project", extensions: ["oab", "ironbark"] }] });
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

  async pickOpenFamily() {
    if (this.isDesktop && window.__TAURI__.dialog) {
      return window.__TAURI__.dialog.open({ multiple: false, filters: [{ name: "OpenArch BIM family", extensions: ["oaf", "ibfam"] }] });
    }
    return window.prompt("Load family (file path)");
  },
};
