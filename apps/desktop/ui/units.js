// Lengths typed into dimension boxes. A bare number is millimetres (the project unit); mm, cm and m
// suffixes and feet and inches (5' 6", 5ft 6in, 66") are understood. NaN when it is not a length.
function parseLengthMM(text) {
  let s = String(text).trim().toLowerCase();
  if (!s) return NaN;
  s = /^\d{1,3}(,\d{3})+(\.\d+)?(?!\d)/.test(s) ? s.replace(/,/g, "") : s.replace(",", ".");
  const num = "(\\d+(?:\\.\\d+)?|\\.\\d+)";
  let m = s.match(new RegExp(`^${num}\\s*(?:'|ft|feet)\\s*-?\\s*(?:${num}\\s*(?:"|in|inch|inches)?)?$`));
  if (m) return (parseFloat(m[1]) * 12 + (m[2] ? parseFloat(m[2]) : 0)) * 25.4;
  m = s.match(new RegExp(`^${num}\\s*(?:"|in|inch|inches)$`));
  if (m) return parseFloat(m[1]) * 25.4;
  m = s.match(new RegExp(`^${num}\\s*(mm|cm|m)?$`));
  if (m) return parseFloat(m[1]) * { mm: 1, cm: 10, m: 1000 }[m[2] || "mm"];
  return NaN;
}
