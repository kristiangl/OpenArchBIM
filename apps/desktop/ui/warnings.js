// Review Warnings: Manage > Warnings. The problems the project has now (overlapping walls, rooms that are not
// enclosed or are redundant, stairs, ramps and railings that break their rules), each with the elements it is
// about; Select picks them. The core gathers them (crates/openarch-api/src/warnings.rs).
const Warnings = (() => {
  async function open() {
    const r = await run("review_warnings", {});
    if (!r) return;
    openModal(`Warnings (${r.count})`, (body, foot, close) => {
      if (!r.count) body.append(el("p", { textContent: "The project has no warnings." }));
      else {
        const t = el("table", { className: "grid-table" });
        t.append(el("tr", {}, ["Warning", "Elements", ""].map((h) => el("th", { textContent: h }))));
        for (const w of r.warnings) {
          t.append(el("tr", {}, [
            el("td", { textContent: w.message }),
            el("td", { textContent: w.labels.join(", ") }),
            el("td", {}, [button("Select", async () => { close(); await selectIds(w.ids); })]),
          ]));
        }
        body.append(t);
      }
      foot.append(button("Close", close, true));
    }, { wide: true });
  }
  return { open };
})();
