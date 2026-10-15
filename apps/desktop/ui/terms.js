// The one place the word "Family" is chosen for the interface. Code, files and the stored data keep
// "family"; only what is shown changes. To rename it for good, edit DEFAULT below (a one-line change).
// To try a name without editing code, run in the browser console:
//   localStorage.setItem("openarch.term", "Kit|Kits"); location.reload();
// and localStorage.removeItem("openarch.term") to go back.
// Settings saved under the old name (ironbark.*) move to the new one (openarch.*) the first time this runs.
(function () {
  try {
    for (const k of Object.keys(localStorage)) {
      if (!k.startsWith("ironbark.")) continue;
      const n = "openarch." + k.slice("ironbark.".length);
      if (localStorage.getItem(n) === null) localStorage.setItem(n, localStorage.getItem(k));
      localStorage.removeItem(k);
    }
  } catch (e) { /* storage can be blocked */ }
})();
(function () {
const DEFAULT = { singular: "Family", plural: "Families" };

const TERM = (() => {
  try {
    const v = localStorage.getItem("openarch.term");
    if (v && v.includes("|")) { const [s, p] = v.split("|").map((x) => x.trim()); if (s && p) return { singular: s, plural: p }; }
  } catch (e) { /* storage can be blocked */ }
  return DEFAULT;
})();

// Text shown to the user with the word swapped, keeping the capital letter.
function T(text) {
  if (typeof text !== "string" || (TERM.singular === "Family" && TERM.plural === "Families")) return text;
  const low = (w) => w.charAt(0).toLowerCase() + w.slice(1);
  return text
    .replace(/\bFamilies\b/g, TERM.plural)
    .replace(/\bfamilies\b/g, low(TERM.plural))
    .replace(/\bFamily\b/g, TERM.singular)
    .replace(/\bfamily\b/g, low(TERM.singular));
}

// Swap it everywhere the interface writes text: text nodes and the title, placeholder and aria-label attributes,
// now and as the page changes, and in the browser's own message boxes.
if (TERM.singular !== "Family" || TERM.plural !== "Families") {
  const ATTRS = ["title", "placeholder", "aria-label"];
  const fixNode = (n) => {
    if (n.nodeType === 3) {
      const p = n.parentNode && n.parentNode.nodeName;
      if (p === "SCRIPT" || p === "STYLE" || p === "TEXTAREA") return;
      const t = T(n.nodeValue);
      if (t !== n.nodeValue) n.nodeValue = t;
    } else if (n.nodeType === 1) {
      for (const a of ATTRS) { const v = n.getAttribute(a); if (v) { const t = T(v); if (t !== v) n.setAttribute(a, t); } }
      n.childNodes.forEach(fixNode);
    }
  };
  const start = () => {
    fixNode(document.body);
    new MutationObserver((muts) => {
      for (const m of muts) {
        if (m.type === "childList") m.addedNodes.forEach(fixNode);
        else if (m.type === "characterData") fixNode(m.target);
        else if (m.type === "attributes") fixNode(m.target);
      }
    }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRS });
    document.title = T(document.title);
  };
  if (document.body) start(); else document.addEventListener("DOMContentLoaded", start);
  const confirm0 = window.confirm.bind(window), alert0 = window.alert.bind(window), prompt0 = window.prompt.bind(window);
  window.confirm = (m) => confirm0(T(m));
  window.alert = (m) => alert0(T(m));
  window.prompt = (m, d) => prompt0(T(m), d);
}
window.TERM = TERM;
window.T = T;
})();
