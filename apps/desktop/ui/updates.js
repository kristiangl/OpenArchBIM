// Updates from the project's releases on GitHub: Help > Check for Updates, and a quiet check when the program
// starts (File > Options turns that off). The desktop shell does the work (update_check, update_install): it
// reads latest.json from the latest release, and an installer is only run once its signature has been checked
// against the key built into the program. The project is saved, or the person asked, before anything installs.
const Updates = (() => {
  const SKIP = "openarch.skipVersion";
  const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
  const skipped = () => { try { return localStorage.getItem(SKIP) || ""; } catch { return ""; } };
  const skip = (v) => { try { localStorage.setItem(SKIP, v); } catch { /* storage off */ } };
  let busy = false;
  const mb = (n) => `${(n / 1048576).toFixed(1)} MB`;

  // downloading and installing, with how far it has got; the program starts again when it is done
  function install(version) {
    openModal(`Installing OpenArch BIM ${version}`, (body, foot, close) => {
      const msg = el("div", { className: "modal-msg", textContent: "Downloading..." });
      const bar = el("progress", { max: 1, value: 0 });
      bar.style.cssText = "width:100%;min-width:360px;margin-top:8px";
      body.append(msg, bar);
      body.closest(".modal-back").onEscape = () => {};
      let off = null;
      window.__TAURI__.event.listen("update-progress", (e) => {
        const { done, total } = e.payload || {};
        if (total) { bar.value = Math.min(1, done / total); msg.textContent = `Downloading: ${mb(done)} of ${mb(total)}`; if (done >= total) msg.textContent = "Installing. The program will start again."; }
        else msg.textContent = `Downloading: ${mb(done || 0)}`;
      }).then((f) => { off = f; });
      invoke("update_install").catch((e) => {
        if (off) off();
        bar.remove();
        msg.textContent = String(e.message || e);
        foot.append(button("Close", () => { busy = false; close(); }, true));
      });
    });
  }

  // a newer version is there: install now (saving first), later, or never ask about this one again
  function offer(info, manual) {
    openModal("Update Available", (body, foot, close) => {
      const done = () => { busy = false; close(); };
      body.append(el("div", { className: "modal-msg confirm", textContent: `OpenArch BIM ${info.version} is available. You have ${info.current}.` }));
      if (info.notes && info.notes.trim()) {
        const notes = el("div", { className: "modal-note", textContent: info.notes.trim() });
        notes.style.cssText = "white-space:pre-wrap;max-height:40vh;overflow:auto;max-width:560px;margin-top:8px";
        body.append(notes);
      }
      body.append(el("div", { className: "modal-note", textContent: "Installing closes the program and starts it again. You are asked about unsaved work first." }));
      body.closest(".modal-back").onEscape = done;
      const now = button("Install Now", async () => {
        close();
        // the same question closing the program asks: save, don't save, or cancel
        if (!(await confirmExit())) { busy = false; return; }
        install(info.version);
      }, true);
      foot.append(now, button("Later", done));
      if (!manual) foot.append(button("Skip This Version", () => { skip(info.version); done(); }));
      setTimeout(() => now.focus(), 0);
    });
  }

  // Help > Check for Updates (manual: always says what it found), or the quiet check at start-up
  async function check(manual = true) {
    if (!Backend.isDesktop) { if (manual) setPrompt("Updates are for the installed desktop program. This is the browser version.", true); return; }
    if (busy) return;
    busy = true;
    let info;
    try { info = await invoke("update_check"); }
    catch (e) {
      busy = false;
      if (manual) openModal("Check for Updates", (body, foot, close) => { body.append(el("div", { className: "modal-msg", textContent: String(e.message || e) })); foot.append(button("Close", close, true)); });
      return;
    }
    if (!info.version) {
      busy = false;
      if (manual) openModal("Check for Updates", (body, foot, close) => { body.append(el("div", { className: "modal-msg", textContent: `OpenArch BIM ${info.current} is the latest version.` })); foot.append(button("Close", close, true)); });
      return;
    }
    if (!manual && skipped() === info.version) { busy = false; return; }
    // the start-up check never comes up over another dialog: it waits for it to close
    if (!manual) for (let i = 0; i < 600 && document.querySelector(".modal-back, .ctx-menu"); i++) await new Promise((r) => setTimeout(r, 1000));
    offer(info, manual);
  }

  // the version for the Help menu; null in a browser
  async function version() {
    if (!Backend.isDesktop) return null;
    try { return await invoke("app_version"); } catch { return null; }
  }

  // a little after start-up, so the window is up first; only builds that carry the update key ask at all
  if (Backend.isDesktop) setTimeout(async () => {
    if (typeof SaveOptions !== "undefined" && SaveOptions.options().checkUpdates === false) return;
    const v = await version();
    if (v && v.updates) check(false);
  }, 8000);

  return { check, version };
})();
