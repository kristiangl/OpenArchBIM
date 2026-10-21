// Ribbon and tool icons. One 24 px set in one style: dark outlines 1.5 wide with round ends, and a warm accent
// for the part that says what the tool does. The building icons (wall, door, window, floor, roof, ceiling, column,
// component, ramp, level, grid, room) are the supplied set; the rest are drawn to match.
// Classes: a = accent line, a2 = accent line 2 wide, af = accent fill, ff = accent fill without outline,
// f = dark fill, ok / no = finish and cancel, mk = a masking fill, void = dashed (void forms).

const ICONS = {
  // ---- supplied set ----
  wall: '<rect x="3" y="5" width="18" height="14" rx="1"/><path d="M3 9.67h18M3 14.33h18M9 5v4.67M15 5v4.67M12 9.67v4.66M9 14.33V19M15 14.33V19"/><rect class="af" x="12" y="9.67" width="9" height="4.66"/>',
  door: '<path d="M2 19h5M19 19h3"/><path class="a2" d="M7 19V7"/><path d="M7 7a12 12 0 0 1 12 12" stroke-width="1.25"/>',
  window: '<rect x="5" y="3" width="14" height="14.5" rx="1"/><path d="M12 3v14.5M5 10.25h14"/><path class="a2" d="M3 20.5h18"/><path class="a" d="M7.2 8l2.3-2.3"/>',
  floor: '<path class="af" d="M12 5l9 5-9 5-9-5z"/><path d="M12 5l9 5-9 5-9-5z"/><path d="M3 10v3l9 5 9-5v-3M12 15v3"/>',
  roof: '<rect x="6" y="13" width="12" height="8" rx="1"/><path class="a2" d="M2 14L12 4.5 22 14"/>',
  ceiling: '<rect x="3" y="3" width="18" height="18" rx="1"/><path d="M9 3v18M15 3v18M3 9h18M3 15h18"/><rect class="af" x="9" y="9" width="6" height="6"/>',
  column: '<path class="ff" d="M9.14 4.7l3.14 1.67L12 21.6l-3.2-1.8z"/><path d="M12 2.4l3.2 1.8v15.6L12 21.6l-3.2-1.8V4.2z"/><path d="M8.8 4.2L12 6l3.2-1.8M12 6v15.6"/>',
  component: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/><path class="af" d="M4 7.5L12 3l8 4.5-8 4.5z"/>',
  // stair seen from the side: treads and risers rising to the right, the arrow up the flight
  stair: '<path d="M3 20h18V5h-3.6v3h-3.6v3h-3.6v3H6.6v3H3z"/><path class="a2" d="M3.5 12.5L12 4.5M12 4.5H8.5M12 4.5V8"/>',
  ramp: '<path d="M3 19h18V8z"/><path class="a2" d="M6.5 16.5l8-4.6"/>',
  level: '<path d="M2 9h20" stroke-dasharray="6 2 1.5 2"/><path class="af" d="M14 9h8l-4 6z"/><path d="M2 20h20"/>',
  grid: '<circle class="a2" cx="15" cy="5" r="2.8"/><circle class="a2" cx="5" cy="15" r="2.8"/><path d="M15 8v13M8 15h13" stroke-dasharray="5 2"/>',
  room: '<rect x="3" y="3" width="18" height="18" rx="1"/><path class="af" d="M12 8.5l3.5 3.5-3.5 3.5L8.5 12z"/>',

  // ---- build, model, room, datum, work plane ----
  curtain: '<rect x="4" y="3" width="16" height="18"/><path d="M4 9h16M4 15h16M9.3 3v18M14.7 3v18"/><rect class="af" x="9.3" y="9" width="5.4" height="6"/>',
  railing: '<path class="a2" d="M3 7h18"/><path d="M5 7v13M12 7v13M19 7v13M3 20h18M5 13.5h14"/>',
  modelLine: '<path class="a2" d="M3 16l7-7 4 4 7-7"/><circle class="f" cx="3" cy="16" r="1.6"/><circle class="f" cx="21" cy="6" r="1.6"/>',
  modelText: '<path d="M4 4h12v3.5h-4.25V20h-3.5V7.5H4z"/><path class="a2" d="M16 4l3 2.5V10M11.75 20l3 1.5V9.5"/>',
  modelGroup: '<rect x="3" y="3" width="18" height="18" rx="1" stroke-dasharray="3 2"/><rect x="6" y="6" width="5" height="5"/><circle class="a2" cx="15.5" cy="15.5" r="3"/>',
  roomSep: '<path d="M3 4v16h18V4"/><path class="a2" d="M12 4v16" stroke-dasharray="3 2"/>',
  tag: '<rect x="8" y="4" width="13" height="8" rx="4"/><path class="a2" d="M8 9L4 19"/><circle class="f" cx="4" cy="19.5" r="1.6"/>',
  tagRoom: '<rect x="3" y="7" width="18" height="10" rx="5"/><path class="af" d="M12 9.5l2.5 2.5-2.5 2.5L9.5 12z"/>',
  tagAll: '<rect x="3" y="3" width="11" height="7" rx="3.5"/><rect x="10" y="14" width="11" height="7" rx="3.5"/><path class="a2" d="M8.5 10v3.5h4"/>',
  tagMat: '<rect x="9" y="4" width="12" height="8" rx="4"/><path class="a2" d="M9 9l-3 4"/><rect x="2.5" y="13" width="7" height="7"/><path class="a" d="M2.5 17l4-4M5.5 20l4-4"/>',
  area: '<rect x="3" y="3" width="18" height="18" rx="1"/><path class="a" d="M3 9l6-6M3 15L15 3M3 21L21 3M9 21l12-12M15 21l6-6"/>',
  setWp: '<path d="M3 20l5-7h13l-5 7z"/><path class="a2" d="M12 16V4M9.5 6.5L12 4l2.5 2.5"/>',
  showWp: '<path d="M2.5 18l5-9h14l-5 9z"/><path class="a" d="M6.3 13.5h12.4M10.8 9l-2.6 9M15.2 9l-2.6 9"/>',
  refPlane: '<path class="a2" d="M4 20L20 4" stroke-dasharray="5 2 1 2"/><path d="M2 17l5 5M17 2l5 5"/>',
  refLine: '<path class="a2" d="M4 20L20 4"/><circle class="f" cx="4" cy="20" r="1.7"/><circle class="f" cx="20" cy="4" r="1.7"/><path d="M9 19l-4-4M19 9l-4-4" stroke-dasharray="1.5 1.5"/>',
  viewer: '<path d="M3 21l4-6h14l-4 6z"/><path d="M5 9c3-4.5 11-4.5 14 0-3 4.5-11 4.5-14 0z"/><circle class="af" cx="12" cy="9" r="1.8"/>',

  // ---- structure ----
  beam: '<rect x="3" y="9" width="18" height="6"/><path d="M3 12h18"/><path class="a2" d="M2 7h4M2 17h4M4 7v10"/>',
  brace: '<path d="M4 4v16M20 4v16M4 4h16"/><path class="a2" d="M4 20L20 4"/>',
  sColumn: '<path class="a2" d="M6 4h12M6 20h12"/><path d="M12 4v16"/><path d="M8 4v1M16 4v1M8 19v1M16 19v1"/>',
  truss: '<path d="M3 17h18M5 9h14"/><path class="a" d="M3 17l2-8 4 8 3-8 3 8 4-8 2 8"/>',
  footing: '<rect x="3" y="15" width="18" height="5"/><rect class="af" x="9.5" y="4" width="5" height="11"/>',
  stripFooting: '<path d="M3 20h18v-4h-6V4H9v12H3z"/><path class="a2" d="M3 20h18"/>',
  slabEdge: '<path d="M3 7h18v5H9.5v5H3z"/><path class="a2" d="M3 17h6.5v-5"/>',

  // ---- insert ----
  link: '<path d="M10.5 13.5a4 4 0 0 1 0-5.66l3-3a4 4 0 0 1 5.66 5.66l-1.6 1.6"/><path class="a2" d="M13.5 10.5a4 4 0 0 1 0 5.66l-3 3a4 4 0 0 1-5.66-5.66l1.6-1.6"/>',
  linkIfc: '<path d="M9 3l6 3.4v6.8L9 16.6 3 13.2V6.4z"/><path d="M3 6.4l6 3.4 6-3.4M9 9.8v6.8"/><path class="a2" d="M14 20l7-7M16 13h5v5"/>',
  linkDxf: '<path d="M4 3h8l4 4v8"/><path d="M4 3v15h7"/><path d="M12 3v4h4"/><path class="a2" d="M14 20l7-7M16 13h5v5"/>',
  importDxf: '<path d="M5 3h9l5 5v13H5z"/><path d="M14 3v5h5"/><path class="a2" d="M12 10v7M9 14l3 3 3-3"/>',
  image: '<rect x="3" y="5" width="18" height="14" rx="1"/><path d="M3 16l5-5 4 4 3-3 6 6"/><circle class="af" cx="16" cy="9" r="1.8"/>',
  loadFamily: '<path d="M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/><path class="a2" d="M12 10.5v6.5M9 14l3 3 3-3"/>',

  // ---- annotate ----
  dimAligned: '<path d="M9 21L21 9"/><path d="M9 21l-4-4M21 9l-4-4"/><path class="a2" d="M3.5 15.5L15.5 3.5M2.5 17.5l3-3M14.5 5.5l3-3"/>',
  dimLinear: '<path d="M5 21V8M19 21V8"/><path class="a2" d="M5 11h14M3.5 12.5l3-3M17.5 12.5l3-3"/><path d="M8 17h8"/>',
  dimAngular: '<path d="M4 20h17M4 20L15 5"/><path class="a2" d="M14 20A10 10 0 0 0 9.9 11.9"/>',
  dimRadial: '<circle cx="10" cy="14" r="7"/><circle class="f" cx="10" cy="14" r="1.2"/><path class="a2" d="M10 14l4.5-5.4M14.5 8.6l-.4 2.9M14.5 8.6l-2.8.9"/><path d="M14.5 8.6L20 3"/>',
  dimDiameter: '<circle cx="12" cy="12" r="8"/><path class="a2" d="M6.3 17.7L17.7 6.3M17.7 6.3l-3.4.4M17.7 6.3l-.4 3.4M6.3 17.7l3.4-.4M6.3 17.7l.4-3.4"/>',
  dimArc: '<path d="M7 21A14 14 0 0 1 21 7"/><path class="a2" d="M3 21A18 18 0 0 1 21 3"/><path d="M3 21h4M21 3v4"/>',
  spotElev: '<path d="M3 19h18"/><path class="af" d="M8 19l-3-5h6z"/><path class="a2" d="M8 14l5-6h8"/>',
  spotCoord: '<circle cx="7" cy="17" r="3.5"/><path d="M7 12v10M2 17h10"/><path class="a2" d="M9.5 14.5L14 9h7"/><path d="M14 5h7"/>',
  spotSlope: '<path d="M3 20h18V10z"/><path class="a2" d="M7 14.5l9-5M16 9.5l-3.4-.6M16 9.5l-1.6 3"/>',
  editWitness: '<path d="M5 21V9M14 21V9"/><path class="a2" d="M5 6h9M19 12v6M16 15h6"/>',
  detailLine: '<path class="a2" d="M3 21L13 11"/><path d="M13 11l6-6 2.5 2.5-6 6-3.5 1z"/>',
  region: '<path d="M4 6l7-3 9 4-2 11-9 3-5-7z"/><path class="a" d="M6 16l9-9M8.5 19l9-9M5 11l5-5"/>',
  detailComp: '<rect x="3" y="8" width="18" height="8"/><path class="a" d="M3 16l8-8M9 16l8-8M15 16l6-6"/>',
  insulation: '<path d="M3 5h18M3 19h18"/><path class="a2" d="M4 19c0-7 4-7 4-14 0 7 4 7 4 14 0-7 4-7 4-14 0 7 4 7 4 14"/>',
  revCloud: '<path class="a2" d="M7 18.5a3 3 0 0 1-1-5.8 3 3 0 0 1 4-3.7 3.5 3.5 0 0 1 6.5 0 3 3 0 0 1 3.5 4 3 3 0 0 1-1.5 5.5z"/>',
  text: '<path d="M5 19L12 4l7 15M7.6 13.5h8.8"/><path class="a2" d="M4 21.5h16"/>',
  keynote: '<rect x="12" y="3" width="9" height="9"/><path class="a2" d="M12 12L4 20M4 20h3.5M4 20v-3.5"/>',
  symbol: '<circle cx="12" cy="12" r="8"/><path class="af" d="M12 7l4.5 8h-9z"/>',
  north: '<path d="M12 3l6 18-6-4-6 4z"/><path class="af" d="M12 3v14l-6 4z"/>',
  scaleBar: '<path d="M3 15h18v4H3z"/><path class="af" d="M3 15h4.5v4H3zM12 15h4.5v4H12z"/><path d="M3 11.5V15M12 11.5V15M21 11.5V15"/>',

  // ---- site ----
  topoPoints: '<path d="M3 17c4-6 8 2 12-4s5-5 6-6"/><circle class="af" cx="6" cy="13.5" r="1.6"/><circle class="af" cx="12" cy="14.5" r="1.6"/><circle class="af" cx="17.5" cy="9" r="1.6"/>',
  topoFile: '<path d="M5 3h9l5 5v13H5z"/><path d="M14 3v5h5"/><path class="a2" d="M7.5 16.5c2-3 4 0 6-2s2.5-3 3.5-3M7.5 12.5c2-2 4 0 6-2"/>',
  surface: '<path d="M2 18l5-7 4 4 4-6 7 9z"/><path class="ff" d="M11 15l4-6 3.2 4.1L16 16.5z"/>',
  grading: '<path d="M3 20h18"/><path d="M3 15c5 0 8-6 18-6"/><path class="a2" d="M8 12.5V18M14 10.2V18M8 18l-1.6-1.6M8 18l1.6-1.6M14 18l-1.6-1.6M14 18l1.6-1.6"/>',
  cutFill: '<path d="M3 13h18"/><path d="M3 8c4 0 6 9 9 9s5-9 9-9"/><path class="ff" d="M3 8c1.8 0 3.2 2 4.4 5H3z"/><path class="ff" d="M21 8c-1.8 0-3.2 2-4.4 5H21z"/>',
  parking: '<path d="M3 4v16M10.5 4v16M18 4v16"/><path class="a2" d="M3 4h18"/><rect class="af" x="5" y="9" width="3.5" height="7" rx="1"/>',
  lineMark: '<path d="M5 3v18M19 3v18"/><path class="a2" d="M12 3v3.5M12 10.25v3.5M12 17.5V21"/>',
  kerb: '<path d="M3 18h7c1.4 0 1.2-6 2.6-6H21"/><path class="a2" d="M3 21h18"/>',
  kerbRamp: '<path d="M3 18h5.5M15.5 12H21M3 12h5.5"/><path class="a2" d="M8.5 18l7-6"/><path d="M3 21h18"/>',

  // ---- view ----
  planView: '<rect x="3" y="3" width="18" height="18" rx="1"/><path d="M11 3v8H3M11 15v6"/><path class="a2" d="M11 15h4a4 4 0 0 0-4-4"/>',
  ceilingPlan: '<rect x="3" y="3" width="18" height="18" rx="1"/><path d="M9 3v18M15 3v18M3 9h18M3 15h18"/><path class="a2" d="M12 16.5v-9M9.5 10L12 7.5l2.5 2.5"/>',
  view3d: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/><path class="af" d="M4 7.5L12 12v9l-8-4.5z"/>',
  // a section line split into two runs joined by a square jog
  splitSegment: '<path d="M2 7h9v10h11"/><path class="a2" d="M11 7v10"/><circle class="f" cx="11" cy="7" r="1.5"/><circle class="f" cx="11" cy="17" r="1.5"/>',
  section: '<circle cx="7" cy="7" r="4"/><path class="af" d="M3 7a4 4 0 0 1 8 0z"/><path d="M7 11v10" stroke-dasharray="3 2"/><path class="a2" d="M7 21h6l-2-2"/>',
  elevation: '<circle cx="12" cy="13.5" r="5.5"/><path class="af" d="M12 2.5l5.5 7h-11z"/>',
  drafting: '<rect x="3" y="3" width="18" height="18" rx="1"/><path class="a2" d="M7 17L17 7"/><path d="M7 17h4M17 7v4"/>',
  legend: '<rect class="af" x="4" y="5" width="4" height="4"/><rect x="4" y="15" width="4" height="4"/><path d="M11 7h9M11 17h9"/>',
  schedule: '<rect x="3" y="4" width="18" height="16" rx="1"/><path d="M3 14h18M9 4v16"/><path class="a2" d="M3 9h18"/>',
  sheet: '<rect x="3" y="5" width="18" height="14" rx="1"/><path d="M15 5v14M15 15h6"/><rect class="af" x="5" y="7" width="8" height="5"/>',
  dupView: '<rect x="7" y="7" width="14" height="14" rx="1"/><path d="M17 7V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h3"/><path class="a2" d="M14 10.5v7M10.5 14h7"/>',
  visibility: '<path d="M2 12c3-5 7-7 10-7s7 2 10 7c-3 5-7 7-10 7s-7-2-10-7z"/><circle class="af" cx="12" cy="12" r="3.2"/>',
  filters: '<path d="M3 4h18l-7 8.5V20l-4-2v-5.5z"/><path class="a2" d="M3 4h18"/>',
  template: '<rect x="3" y="3" width="13" height="13" rx="1"/><rect x="8" y="8" width="13" height="13" rx="1"/><path class="a2" d="M11 12h7M11 15h7M11 18h4"/>',

  // ---- manage ----
  materials: '<circle cx="12" cy="12" r="9"/><path class="ff" d="M12 3a9 9 0 0 1 0 18 12 12 0 0 0 0-18z"/>',
  units: '<rect x="2.5" y="8" width="19" height="8" rx="1"/><path d="M6 8v3M9.5 8v2M13 8v3M16.5 8v2"/><path class="a2" d="M20 8v4"/>',
  params: '<path d="M4 6h16M4 12h16M4 18h16"/><circle class="af" cx="9" cy="6" r="2"/><circle class="af" cx="15" cy="12" r="2"/><circle class="af" cx="7" cy="18" r="2"/>',
  shared: '<circle cx="6" cy="12" r="2.5"/><circle cx="18" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path class="a2" d="M8.2 10.8l7.6-3.6M8.2 13.2l7.6 3.6"/>',
  styles: '<path d="M4 6h16" stroke-width="3"/><path d="M4 12h16"/><path class="a2" d="M4 18h16" stroke-dasharray="4 2"/>',
  phases: '<circle cx="5" cy="12" r="2.5"/><circle class="af" cx="12" cy="12" r="2.5"/><circle cx="19" cy="12" r="2.5"/><path d="M7.5 12h2M14.5 12h2"/>',
  revisions: '<path d="M5 18a3 3 0 0 1 .5-5.9A4 4 0 0 1 13 9.5a3.5 3.5 0 0 1 6 3 2.8 2.8 0 0 1 0 5.5z"/><path class="af" d="M12 11l3 5H9z"/>',
  snaps: '<path d="M6 4v8a6 6 0 0 0 12 0V4h-4v8a2 2 0 0 1-4 0V4z"/><path class="af" d="M6 4h4v3H6zM14 4h4v3h-4z"/>',
  lineWeights: '<path d="M4 6h16" stroke-width="1"/><path d="M4 12h16" stroke-width="2"/><path class="a" d="M4 18h16" stroke-width="3.5"/>',
  linePatterns: '<path d="M4 6h16" stroke-dasharray="4 2"/><path d="M4 12h16" stroke-dasharray="6 2 1 2"/><path class="a" d="M4 18h16" stroke-dasharray="1 2.5"/>',
  fillPatterns: '<rect x="3" y="3" width="18" height="18" rx="1"/><path class="a" d="M3 9l6-6M3 15L15 3M3 21L21 3M9 21l12-12M15 21l6-6M9 3l12 12M3 9l12 12M15 3l6 6M3 15l6 6"/>',
  tempDim: '<path d="M4 8v10M20 8v10"/><path class="a2" d="M4 13h16" stroke-dasharray="3 2"/><rect x="9" y="4" width="6" height="5" rx="1"/>',
  annFamily: '<circle cx="12" cy="9" r="6"/><path d="M6 9h12"/><path class="a2" d="M12 15v6"/>',
  families: '<rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="3" width="8" height="8" rx="1"/><rect x="3" y="13" width="8" height="8" rx="1"/><rect class="af" x="13" y="13" width="8" height="8" rx="1"/>',
  exportSheets: '<rect x="3" y="6" width="13" height="15" rx="1"/><path d="M7 3h11a1 1 0 0 1 1 1v6"/><path class="a2" d="M10 14h11M18 11l3 3-3 3"/>',

  // ---- modify ----
  select: '<path class="af" d="M6 3l12 9-5.5 1.2L16 20l-2.6 1.2-2.6-6.6L6 18z"/><path d="M6 3l12 9-5.5 1.2L16 20l-2.6 1.2-2.6-6.6L6 18z"/>',
  move: '<path d="M12 3v18M3 12h18"/><path class="a2" d="M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="1"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/><path class="a2" d="M14 11v6M11 14h6"/>',
  rotate: '<path d="M20 12a8 8 0 1 1-2.34-5.66"/><path class="a2" d="M20 3.5v4.5h-4.5"/><circle class="af" cx="12" cy="12" r="1.5"/>',
  mirror: '<path class="a2" d="M12 2v20" stroke-dasharray="2.5 2"/><path d="M9 6L3 18h6zM15 6l6 12h-6z"/>',
  align: '<path class="a2" d="M4 3v18"/><path d="M8 6h11v4H8zM8 14h7v4H8z"/>',
  offset: '<path d="M4 18c0-7 5-12 12-12h4"/><path class="a2" d="M8.5 21c0-5 4-9 9-9H20"/>',
  trim: '<path d="M6 21V10M10 6h11"/><path class="a2" d="M6 10V6h4"/>',
  trimSingle: '<path d="M3 8h18"/><path class="a2" d="M12 8v13"/><path d="M12 3v2.5" stroke-dasharray="1 1.5"/>',
  split: '<path d="M3 12h7M14 12h7"/><path class="a2" d="M13.5 6l-3 12"/>',
  array: '<rect x="3" y="9" width="5" height="6" rx=".5"/><rect x="9.5" y="9" width="5" height="6" rx=".5"/><rect class="af" x="16" y="9" width="5" height="6" rx=".5"/>',
  scale: '<rect x="3" y="11" width="10" height="10" rx="1"/><path d="M13 21h6a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6" stroke-dasharray="2 2"/><path class="a2" d="M10 14l8-8M13 6h5v5"/>',
  pin: '<path d="M9 3h6l-1 5 4 4H6l4-4z"/><path class="a2" d="M12 12v9"/>',
  unpin: '<path d="M9 3h6l-1 5 4 4H6l4-4z"/><path d="M12 12v9"/><path class="a2" d="M4 4l16 16"/>',
  delete: '<path d="M4.5 7h15M10 7V4h4v3M7 7l1 13h8l1-13"/><path class="a2" d="M10 11l4 5M14 11l-4 5"/>',
  measure: '<path d="M3 14h18v5H3z"/><path d="M6 14v2.5M9 14v2.5M12 14v3.5M15 14v2.5M18 14v2.5"/><path class="a2" d="M4 9h16M4 7v4M20 7v4"/>',
  editBoundary: '<path d="M4 4h12v4" stroke-dasharray="3 2"/><path d="M4 4v16h8" stroke-dasharray="3 2"/><path class="a2" d="M13 21l1-4 5.5-5.5 3 3L17 20z"/>',
  wallJoins: '<path d="M3 5h16v16M3 11h10v10"/><rect class="af" x="13" y="5" width="6" height="6"/>',
  join: '<rect x="3" y="8" width="11" height="11"/><rect class="a2" x="10" y="4" width="11" height="11"/>',
  cut: '<rect x="3" y="6" width="18" height="13" rx="1"/><circle class="a2" cx="12" cy="12.5" r="3.5" stroke-dasharray="2 1.6"/>',
  edit: '<path d="M4 20h7"/><path class="a2" d="M14 5l5 5-9 9H5v-5z"/>',
  flip: '<path class="a2" d="M4 12h16M7 9l-3 3 3 3M17 9l3 3-3 3"/><path d="M12 5v14" stroke-dasharray="2 2"/>',
  lock: '<rect x="5" y="11" width="14" height="10" rx="1.5"/><path class="a2" d="M8 11V8a4 4 0 0 1 8 0v3"/>',
  unlock: '<rect x="5" y="11" width="14" height="10" rx="1.5"/><path class="a2" d="M8 11V8a4 4 0 0 1 7.6-1.7"/>',
  editWp: '<path d="M2 20l4-6h11l-4 6z"/><path class="a2" d="M13 11l1-3.5 5-5 2.5 2.5-5 5z"/>',
  pickNew: '<path d="M2 18l4-7h12l-4 7z"/><path class="af" d="M13 9l8 6-3.4.8 2 4-1.8.9-2-4L13 19z"/>',
  equal: '<path d="M4 9v10M12 9v10M20 9v10"/><path class="a2" d="M4 14h16"/><path class="a" d="M6 5h4M6 7h4M14 5h4M14 7h4"/>',
  finish: '<path class="ok" d="M4 12.5l5 5L20 6.5"/>',
  cancel: '<path class="no" d="M6 6l12 12M18 6L6 18"/>',
  check: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.5 15.5L21 21"/><path class="a2" d="M7.5 10.5l2 2 3.5-4"/>',

  // ---- sketch draw tools ----
  pickWalls: '<path d="M2 6h14M2 11h14"/><path class="af" d="M13 11l8 6-3.4.8 2 4-1.8.9-2-4L13 21z"/>',
  pickLines: '<path d="M3 15L15 3"/><path class="af" d="M13 11l8 6-3.4.8 2 4-1.8.9-2-4L13 21z"/>',
  pickFaces: '<path d="M2 9l7-5h9l-7 5z"/><path d="M2 9v6l7 0V9"/><path class="af" d="M13 11l8 6-3.4.8 2 4-1.8.9-2-4L13 21z"/>',
  line: '<path class="a2" d="M4.5 19.5L19.5 4.5"/><circle class="f" cx="4.5" cy="19.5" r="1.7"/><circle class="f" cx="19.5" cy="4.5" r="1.7"/>',
  rectangle: '<rect class="a2" x="4" y="6" width="16" height="12"/><circle class="f" cx="4" cy="6" r="1.7"/><circle class="f" cx="20" cy="18" r="1.7"/>',
  polygonIn: '<circle cx="12" cy="12" r="9" stroke-dasharray="2 2"/><path class="a2" d="M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9z"/>',
  polygonOut: '<circle cx="12" cy="12" r="7.5" stroke-dasharray="2 2"/><path class="a2" d="M12 3.3l7.5 4.35v8.7L12 20.7l-7.5-4.35v-8.7z"/>',
  circle: '<circle class="a2" cx="12" cy="12" r="8"/><circle class="f" cx="12" cy="12" r="1.5"/><path d="M12 12h8" stroke-dasharray="2 1.5"/>',
  arc3pt: '<path class="a2" d="M4 18A9 9 0 0 1 20 18"/><circle class="f" cx="4" cy="18" r="1.7"/><circle class="f" cx="20" cy="18" r="1.7"/><circle cx="12" cy="4.9" r="1.6"/>',
  arcCentre: '<path class="a2" d="M4 16A8 8 0 0 1 20 16"/><circle class="f" cx="12" cy="16" r="1.6"/><path d="M12 16L17.7 10.3" stroke-dasharray="2 1.5"/><path d="M4 19.5h16" stroke-dasharray="1 2"/>',
  arcTangent: '<path d="M3 18h8"/><path class="a2" d="M11 18a6 6 0 0 0 6-6V5"/><circle class="f" cx="11" cy="18" r="1.6"/>',
  fillet: '<path d="M4 21v-9M12 4h9"/><path d="M4 12V4h8" stroke-dasharray="1.5 2"/><path class="a2" d="M4 12a8 8 0 0 1 8-8"/>',
  points: '<path class="a2" d="M4 19l4-11 7 4 5-7"/><circle class="f" cx="4" cy="19" r="1.6"/><circle class="f" cx="8" cy="8" r="1.6"/><circle class="f" cx="15" cy="12" r="1.6"/><circle class="f" cx="20" cy="5" r="1.6"/>',
  spline: '<path class="a2" d="M3 17c3-9 6-9 9-3s6 6 9-6"/><circle class="f" cx="3" cy="17" r="1.6"/><circle class="f" cx="21" cy="8" r="1.6"/><circle cx="12" cy="14" r="1.5"/>',
  ellipse: '<ellipse class="a2" cx="12" cy="12" rx="9" ry="5.5"/><circle class="f" cx="12" cy="12" r="1.4"/>',
  partialEllipse: '<path class="a2" d="M3 13a9 5.5 0 0 1 18 0"/><path d="M3 13h18" stroke-dasharray="2 2"/><circle class="f" cx="3" cy="13" r="1.6"/><circle class="f" cx="21" cy="13" r="1.6"/>',
  slopeArrow: '<path class="a2" d="M4 19L18 7" stroke-dasharray="3 2"/><path class="a2" d="M18 7h-5.5M18 7v5.5"/>',
  spanDir: '<path d="M4 4v16M20 4v16"/><path class="a2" d="M4 12h16M8 9l-4 3 4 3M16 9l4 3-4 3"/>',

  // ---- family editor ----
  category: '<path d="M3 5h7l2 2h9v12H3z"/><path class="af" d="M12 10.5l3.2 1.8v3.6L12 17.7l-3.2-1.8v-3.6z"/>',
  familyTypes: '<rect x="3" y="4" width="18" height="16" rx="1"/><path d="M3 9h18M3 14h18M10 4v16"/><circle class="af" cx="6.5" cy="11.5" r="1.4"/><circle class="af" cx="6.5" cy="16.8" r="1.4"/>',
  extrusion: '<path d="M4 9l5-4h11v10l-5 4H4z"/><path d="M15 9l5-4M15 9v10"/><path class="af" d="M4 9h11v10H4z"/>',
  blend: '<ellipse class="a2" cx="12" cy="18" rx="8" ry="3"/><ellipse cx="12" cy="6" rx="4" ry="1.6"/><path d="M4 18L8 6M20 18L16 6"/>',
  revolve: '<path class="a2" d="M12 2v20" stroke-dasharray="3 2"/><ellipse cx="12" cy="7" rx="7" ry="2.2"/><ellipse cx="12" cy="17" rx="7" ry="2.2"/><path d="M5 7v10M19 7v10"/>',
  sweep: '<path class="a2" d="M6 18c7 0 5-12 15-12"/><rect x="2" y="14.5" width="7" height="7" rx=".5"/>',
  sweptBlend: '<path class="a2" d="M7 17c6 0 4-9 12-9"/><rect x="2" y="13" width="7" height="8" rx=".5"/><rect x="17.5" y="5.5" width="4" height="4" rx=".5"/>',
  opening: '<rect x="3" y="4" width="18" height="16" rx="1"/><rect class="a2" x="8" y="8" width="8" height="8" stroke-dasharray="2.5 1.8"/>',
  control: '<path class="a2" d="M4 12h16M7 8.5L3.5 12 7 15.5M17 8.5l3.5 3.5-3.5 3.5"/>',
  connector: '<circle cx="9" cy="12" r="5"/><circle class="f" cx="9" cy="12" r="1.5"/><path class="a2" d="M14 12h7M18 9l3 3-3 3"/>',
  connElectrical: '<circle cx="12" cy="12" r="9"/><path class="af" d="M13 4.5l-5.5 8.5h4.5l-1 6.5 5.5-8.5H12z"/>',
  connDuct: '<rect x="3" y="7" width="12" height="10"/><path class="a2" d="M15 12h6M18 9l3 3-3 3"/>',
  connPipe: '<circle cx="8.5" cy="12" r="5"/><circle cx="8.5" cy="12" r="2.4"/><path class="a2" d="M13.5 12h7.5M18 9l3 3-3 3"/>',
  connTray: '<path d="M3 8v8h12V8"/><path class="a2" d="M15 12h6M18 9l3 3-3 3"/>',
  connConduit: '<ellipse cx="6" cy="12" rx="2" ry="4"/><path d="M6 8h8M6 16h8M14 8v8"/><path class="a2" d="M14 12h7M18 9l3 3-3 3"/>',
  loadIntoProject: '<path d="M8 3l5 2.8v5.6L8 14.2 3 11.4V5.8z"/><path d="M3 5.8l5 2.8 5-2.8M8 8.6v5.6"/><path class="a2" d="M11 18h9M17 15l3 3-3 3"/>',
  loadAndClose: '<path d="M8 3l5 2.8v5.6L8 14.2 3 11.4V5.8z"/><path d="M3 5.8l5 2.8 5-2.8M8 8.6v5.6"/><path class="a2" d="M10 18h8M15 15l3 3-3 3"/><path class="no" d="M17 3.5l4 4M21 3.5l-4 4"/>',
  symbolicLine: '<rect x="3" y="3" width="18" height="18" rx="1" stroke-dasharray="2 2"/><path class="a2" d="M7 17L17 7"/>',
  masking: '<path d="M2 8h20M2 16h20"/><rect class="mk" x="7" y="5" width="10" height="14"/>',
  planeNames: '<path class="a2" d="M3 16h11" stroke-dasharray="4 2"/><rect x="14" y="12.5" width="7" height="7" rx="1"/><path d="M16 16h3"/>',
  lookup: '<rect x="3" y="3" width="14" height="14" rx="1"/><path d="M3 8h14M8 3v14"/><circle class="a2" cx="16.5" cy="16.5" r="3.5"/><path class="a2" d="M19 19l2.5 2.5"/>',
  furniture: '<path d="M3 9h18"/><path d="M5 9v11M19 9v11"/><path class="a2" d="M3 9l2-4h14l2 4"/>',
  lighting: '<path d="M9 18h6M10 21h4"/><path d="M12 3a6 6 0 0 0-3.5 10.9V16h7v-2.1A6 6 0 0 0 12 3z"/><path class="a2" d="M12 8v4"/>',
  plumbing: '<path d="M4 9h9a4 4 0 0 1 4 4v1"/><path d="M4 6v6M13 9V5h-3"/><path class="a2" d="M17 17v2M15.5 21h3"/>',
};

// Ribbon ids, sketch tool ids and old two-letter monograms that use each icon
const ICON_ALIAS = {
  // architecture
  "model-line": "modelLine", "model-text": "modelText", "model-group": "modelGroup", "room-sep": "roomSep", "tag-room": "tagRoom",
  "set-wp": "setWp", "show-wp": "showWp", "ref-plane": "refPlane",
  // structure
  "s-beam": "beam", "s-brace": "brace", "s-column": "sColumn", "s-truss": "truss", "s-isolated": "footing", "s-strip": "stripFooting", "s-slab": "slabEdge",
  // insert
  "link-ifc": "linkIfc", "link-model": "link", "link-dxf": "linkDxf", "import-dxf": "importDxf", "import-image": "image", "load-family": "loadFamily",
  // annotate
  "dim-aligned": "dimAligned", "dim-linear": "dimLinear", "dim-angular": "dimAngular", "dim-radial": "dimRadial", "dim-diameter": "dimDiameter",
  "dim-arc": "dimArc", "dim-elev": "spotElev", "dim-coord": "spotCoord", "dim-slope": "spotSlope", "edit-witness": "editWitness",
  "det-line": "detailLine", "det-region": "region", "det-comp": "detailComp", "det-insulation": "insulation", "det-revcloud": "revCloud",
  keynote: "keynote", "tag-all": "tagAll", "tag-cat": "tag", "tag-mat": "tagMat", "scale-bar": "scaleBar",
  // site
  "topo-points": "topoPoints", "topo-file": "topoFile", "site-surface": "surface", grading: "grading", cutfill: "cutFill",
  parking: "parking", linemark: "lineMark", kerb: "kerb", "kerb-ramp": "kerbRamp",
  // view
  "v-plan": "planView", "v-floorplan": "planView", "v-ceilingplan": "ceilingPlan", "v-3d": "view3d", "v-section": "section", "v-elev": "elevation",
  "v-draft": "drafting", "v-legend": "legend", "v-sched": "schedule", "v-sheet": "sheet", "v-dup": "dupView", "v-vg": "visibility",
  "v-filters": "filters", "v-template": "template",
  // manage
  "m-materials": "materials", "m-units": "units", "m-params": "params", "m-shared": "shared", "m-styles": "styles", "m-phases": "phases",
  "m-revisions": "revisions", "m-snaps": "snaps", "m-lineweights": "lineWeights", "m-linepatterns": "linePatterns", "m-fillpatterns": "fillPatterns",
  "m-tempdim": "tempDim", "m-annfam": "annFamily", "m-families": "families", "m-sheets": "exportSheets",
  // modify
  mv: "move", cp: "copy", ro: "rotate", mi: "mirror", al: "align", of: "offset", tr: "trim", sp: "split", ar: "array", sc: "scale",
  del: "delete", me: "measure", "edit-boundary": "editBoundary", wjoin: "wallJoins", "split-segment": "splitSegment",
  // sketch tools
  pick_walls: "pickWalls", pick_lines: "pickLines", pick_faces: "pickFaces", polygon_in: "polygonIn", polygon_out: "polygonOut",
  arc_3pt: "arc3pt", arc_centre: "arcCentre", arc_tangent: "arcTangent", partial_ellipse: "partialEllipse",
  trim_corner: "trim", trim_single: "trimSingle", slope: "slopeArrow", span: "spanDir",
  // family editor
  "f-select": "select", "f-category": "category", "f-types": "familyTypes", "f-ext": "extrusion", "f-blend": "blend", "f-revolve": "revolve",
  "f-sweep": "sweep", "f-swblend": "sweptBlend", "f-void-menu": "void:extrusion", "f-void-ext": "void:extrusion", "f-void-blend": "void:blend",
  "f-void-revolve": "void:revolve", "f-void-sweep": "void:sweep", "f-void-swblend": "void:sweptBlend", "f-void": "void:extrusion",
  "f-mline": "modelLine", "f-comp": "component", "f-mtext": "modelText", "f-opening": "opening", "f-mgroup": "modelGroup", "f-control": "control",
  "f-conn": "connector", "f-conn-electrical": "connElectrical", "f-conn-duct": "connDuct", "f-conn-pipe": "connPipe", "f-conn-cable_tray": "connTray",
  "f-conn-conduit": "connConduit", "f-refline": "refLine", "f-plane": "refPlane", "f-wpset": "setWp", "f-wpshow": "showWp", "f-wpviewer": "viewer",
  "f-load": "loadIntoProject", "f-loadclose": "loadAndClose", "f-loadfam": "loadFamily", "f-impcad": "importDxf", "f-impimg": "image",
  "f-dim": "dimAligned", "f-eq": "equal", "f-sym": "symbolicLine", "f-region": "region", "f-mask": "masking", "f-text": "text",
  "f-3d": "view3d", "f-sec": "section", "f-names": "planeNames", "f-dims": "dimLinear", "f-vg": "visibility",
  "f-sharedparams": "shared", "f-styles": "styles", "f-materials": "materials", "f-lookup": "lookup",
  "f-align": "align", "f-mv": "move", "f-cp": "copy", "f-del": "delete",
  // monograms used by contextual buttons
  Se: "select", Mv: "move", Cp: "copy", Ro: "rotate", Mi: "mirror", Al: "align", Of: "offset", Tc: "trim", Ts: "trimSingle", Sl: "split",
  De: "delete", Dl: "delete", Sa: "slopeArrow", Sd: "spanDir", Ck: "check", Ed: "edit", Vs: "visibility", Wp: "editWp", Pk: "pickNew",
  Fl: "flip", Lk: "lock", EQ: "equal", Rc: "rectangle", Ci: "circle", Pg: "points", "✓": "finish", "✕": "cancel",
};
// labels of contextual buttons that have no id
const ICON_BY_LABEL = {
  "finish edit mode": "finish", "cancel edit mode": "cancel", "check sketch": "check", modify: "select", "visibility settings": "visibility",
  "edit work plane": "editWp", "pick new": "pickNew", flip: "flip", lock: "lock", unlock: "unlock", "delete loop": "delete", "rotate 90": "rotate",
  "load family": "loadFamily", "place on work plane": "setWp", "place on face": "pickFaces", "place on vertical face": "pickFaces",
  "label dimension": "dimAligned", "add parameter": "params", equal: "equal",
};

function iconMarkup(key) {
  if (!key) return null;
  if (key.startsWith("void:")) {
    const inner = ICONS[key.slice(5)];
    return inner ? `<g class="void">${inner}</g>` : null;
  }
  return ICONS[key] || null;
}

// The icon for a ribbon item: by its icon name, its id, its old monogram or its label
function iconKey(item) {
  if (!item) return null;
  const tries = [item.icon, item.id && ICON_ALIAS[item.id], item.id, item.label && ICON_BY_LABEL[String(item.label).toLowerCase()], item.ico && ICON_ALIAS[item.ico]];
  if (item.label && /^edit /i.test(item.label)) tries.push("edit");
  return tries.find((k) => k && iconMarkup(k)) || null;
}

function svgIcon(markup) {
  return `<svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${markup}</svg>`;
}

// Fill an icon holder: the drawn icon when there is one, else the two-letter monogram
function setIcon(el, item) {
  const m = iconMarkup(iconKey(item));
  if (m) { el.innerHTML = svgIcon(m); el.classList.add("svg"); return true; }
  el.classList.remove("svg");
  el.textContent = (item && (item.ico || (item.label || "").split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase())) || "";
  return false;
}

// The icon that stands for a category (or a system family name) in the type selector
function iconForCategory(cat) {
  const c = String(cat || "").toLowerCase();
  // views show their kind
  if (c.includes("ceiling plan")) return "ceilingPlan";
  if (c.includes("floor plan") || c.includes("site plan")) return "planView";
  if (c.includes("3d view")) return "view3d";
  if (c.startsWith("elevation")) return "elevation";
  if (c.startsWith("section")) return "section";
  if (c.includes("schedule")) return "schedule";
  if (c.includes("wall")) return "wall";
  if (c.includes("door")) return "door";
  if (c.includes("window")) return "window";
  if (c.includes("floor")) return "floor";
  if (c.includes("roof")) return "roof";
  if (c.includes("ceiling")) return "ceiling";
  if (c.includes("level")) return "level";
  if (c.includes("grid")) return "grid";
  if (c.includes("column")) return "column";
  if (c.includes("stair")) return "stair";
  if (c.includes("furniture") || c.includes("casework")) return "furniture";
  if (c.includes("lighting")) return "lighting";
  if (c.includes("plumbing")) return "plumbing";
  return "component";
}
