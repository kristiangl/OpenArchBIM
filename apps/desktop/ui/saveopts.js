// File > Options, and the reminder to save.
//
// Check for updates at start-up: kept on this computer (updates.js does the checking).
// Save reminder interval: kept on this computer (it is how a person likes to work, not part of a project).
// Backups to keep: kept in the project file. Each save over the project's file first keeps the file as it was as
// Name.0001.oab, Name.0002.oab and so on beside it, and removes the oldest past this number (the core does this,
// see save_keeping_backups in store.rs).
const SaveOptions = (() => {
  const KEY = "openarch.options";
  const DEFAULTS = { saveReminderMin: 30, checkUpdates: true };
  const INTERVALS = [[15, "15 minutes"], [30, "30 minutes"], [60, "One hour"], [120, "Two hours"], [240, "Four hours"], [0, "No reminders"]];
  const load = () => { try { return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) || "{}") }; } catch { return { ...DEFAULTS }; } };
  const store = (o) => { try { localStorage.setItem(KEY, JSON.stringify(o)); } catch { /* storage off: kept for this session only */ } };
  let options = load();

  // When the project was last seen with nothing to save, or the person last answered the reminder. The reminder
  // counts from here, so it only ever comes up while there is unsaved work.
  let since = Date.now();
  let asking = false;
  const inProject = () => app.doc !== "family" && app.doc !== "annotation";
  const fileName = () => (app.info && app.info.path ? app.info.path.split(/[\\/]/).pop() : "this project");
  const minutesText = (m) => (m >= 120 ? `${Math.round(m / 60)} hours` : m >= 60 ? "hour" : `${Math.round(m)} minutes`);

  function remind() {
    asking = true;
    const waited = (Date.now() - since) / 60000;
    openModal("Changes Not Saved", (body, foot, close) => {
      const done = () => { asking = false; since = Date.now(); close(); };
      body.append(el("div", { className: "modal-msg confirm", textContent: `You have not saved ${fileName()} in the last ${minutesText(Math.max(waited, options.saveReminderMin))}.` }));
      body.closest(".modal-back").onEscape = done;
      const yes = button("Save", async () => { done(); await save(false); }, true);
      foot.append(yes, button("Options...", () => { done(); open(); }), button("Not Now", done));
      setTimeout(() => yes.focus(), 0);
    });
  }

  // Looked at every few seconds: cheap, and a reminder a few seconds late does not matter.
  function tick() {
    if (!app.info || !app.info.dirty) { since = Date.now(); return; }
    const every = Number(options.saveReminderMin) || 0;
    if (!every || asking || !inProject()) return;
    if (Date.now() - since < every * 60000) return;
    // never over another dialog, a menu, or a drag in progress: it waits for them to finish
    if (document.querySelector(".modal-back, .ctx-menu") || (typeof Plan !== "undefined" && Plan.busy && Plan.busy())) return;
    remind();
  }

  function open() {
    openModal("Options", (body, foot, close) => {
      const msg = el("div", { className: "modal-msg" });
      const every = el("select", {}, INTERVALS.map(([v, l]) => new Option(l, v, false, Number(options.saveReminderMin) === v)));
      const most = (app.info && app.info.most_backups) || 99;
      const keep = el("input", { type: "number", min: 0, max: most, step: 1, value: app.info ? app.info.max_backups : 5 });
      keep.style.width = "70px";
      const updates = el("input", { type: "checkbox", checked: options.checkUpdates !== false });
      const row = (label, control) => { const r = el("label", { className: "new-row" }, [el("span", { textContent: label }), control]); r.style.gridTemplateColumns = "170px 1fr"; return r; };
      body.append(
        el("div", { className: "modal-sub", textContent: "This computer" }),
        row("Save reminder interval", every),
        el("div", { className: "modal-note", textContent: "Asks you to save when the project has had unsaved changes for this long." }),
        row("Check for updates at start-up", updates),
        el("div", { className: "modal-note", textContent: "Asks the project's releases page whether there is a newer version, and offers to install it. Help > Check for Updates asks at any time." }),
        el("div", { className: "modal-sub", textContent: "This project" }),
        row("Backups to keep", keep),
        el("div", { className: "modal-note", textContent: "Each save keeps the file as it was beside the project, as Name.0001.oab, Name.0002.oab and so on, and removes the oldest past this number. 0 keeps none. The number is saved with the project." }),
        msg);
      for (const s of body.querySelectorAll(".modal-sub")) { s.style.fontWeight = "600"; s.style.margin = "10px 0 2px"; }
      foot.append(button("Cancel", close), button("OK", async () => {
        const count = Number(keep.value);
        if (!Number.isInteger(count) || count < 0 || count > most) { msg.textContent = `Backups to keep is a whole number from 0 to ${most}.`; return; }
        if (app.info && count !== app.info.max_backups) {
          try { await Backend.call("set_max_backups", { count }); } catch (e) { msg.textContent = String(e.message || e); return; }
        }
        options = { ...options, saveReminderMin: Number(every.value), checkUpdates: updates.checked };
        store(options);
        since = Date.now();
        close();
        await refresh();
      }, true));
    });
  }

  setInterval(tick, 5000);
  return { open, options: () => ({ ...options }), tick, /* for tests */ _set: (o) => { options = { ...options, ...o }; }, _since: (t) => { since = t; } };
})();
