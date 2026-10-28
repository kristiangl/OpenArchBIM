// Data-driven ribbon. Commands are registered here once; the UI renders from this.
// `big` items get a large button, others stack as small rows (up to 3 per stack).
// Icons come from icons.js by command id (a monogram shows where there is none yet).

// ---- the Modify tab, the same in every document ---------------------------------
// The Modify tab keeps one layout: Select, Properties, Clipboard, Geometry, Controls, Modify, View,
// Measure and Create, in that order. Each button names the command its document runs for it; a document
// with no command for an item still draws the button, greyed out, so the tab never moves. `iconOnly`
// buttons keep their name in the tooltip.
const modifyItem = (label, icon, ids = null, extra = {}) => ({ label, icon, ids, ...extra });
const MODIFY_TAB = [
  { label: "Select", items: [
    modifyItem("Modify", "select", { project: "sel", family: "f-select", annotation: "a-select" }, { big: true }) ] },
  { label: "Properties", items: [
    modifyItem("Properties", "properties", { project: "props", family: "f-props", annotation: "a-props" }, { big: true }) ] },
  { label: "Clipboard", items: [
    modifyItem("Paste", "paste", null, { big: true }),
    modifyItem("Cut", "cut", null),
    modifyItem("Copy", "copy", { project: "cp", family: "f-cp" }),
    modifyItem("Match Type", "matchType", null) ] },
  { label: "Geometry", items: [
    modifyItem("Join", "join", null), modifyItem("Unjoin", "unjoin", null),
    modifyItem("Cut", "cut", null), modifyItem("Uncut", "uncut", null),
    modifyItem("Wall Joins", "wallJoins", { project: "wjoin" }), modifyItem("Paint", "paint", null) ] },
  { label: "Controls", items: [
    modifyItem("Activate", "activate", null, { big: true }) ] },
  { label: "Modify", items: [
    modifyItem("Align", "align", { project: "al", family: "f-align" }, { iconOnly: true }),
    modifyItem("Move", "move", { project: "mv", family: "f-mv" }, { iconOnly: true }),
    modifyItem("Copy", "copy", { project: "cp", family: "f-cp" }, { iconOnly: true }),
    modifyItem("Rotate", "rotate", { project: "ro", family: "f-ro" }, { iconOnly: true }),
    modifyItem("Mirror", "mirror", null, { iconOnly: true, menu: [
      modifyItem("Mirror - Pick Axis", "mirror", { project: "mi-pick", family: "f-mi-pick" }),
      modifyItem("Mirror - Draw Axis", "mirror", { project: "mi-draw", family: "f-mi-draw" }) ] }),
    modifyItem("Offset", "offset", { project: "of", family: "f-of" }, { iconOnly: true }),
    modifyItem("Pin", "pin", { project: "pin" }, { iconOnly: true }),
    modifyItem("Unpin", "unpin", { project: "unpin" }, { iconOnly: true }),
    modifyItem("Delete", "delete", { project: "del", family: "f-del", annotation: "a-del" }, { iconOnly: true }),
    modifyItem("Split", "split", { project: "sp" }, { iconOnly: true }),
    modifyItem("Trim/Extend", "trim", { project: "tr" }, { iconOnly: true }),
    modifyItem("Array", "array", { project: "ar" }, { iconOnly: true }),
    modifyItem("Scale", "scale", { project: "sc" }, { iconOnly: true }) ] },
  { label: "View", items: [
    modifyItem("Show Hidden", "showHidden", null, { iconOnly: true }),
    modifyItem("Reveal Hidden", "revealHidden", null, { iconOnly: true }),
    modifyItem("Temporary Hide/Isolate", "tempHide", null, { iconOnly: true }) ] },
  { label: "Measure", items: [
    modifyItem("Measure", "measure", { project: "me" }, { big: true }) ] },
  { label: "Create", items: [
    modifyItem("Create Assembly", "assembly", null),
    modifyItem("Create Group", "modelGroup", { project: "gp" }),
    modifyItem("Create Similar", "createSimilar", null, { big: true }) ] },
];

const RIBBON = {
  Architecture: [
    { label: "Build", items: [
      { id: "wall", label: "Wall", big: true }, { id: "door", label: "Door", big: true },
      { id: "window", label: "Window", big: true }, { id: "component", label: "Component", big: true },
      { id: "column", label: "Column", big: true },
      { id: "roof", label: "Roof", big: true }, { id: "ceiling", label: "Ceiling", big: true },
      { id: "floor", label: "Floor", big: true }, { id: "curtain", label: "Curtain System", big: true },
      { id: "stair", label: "Stair", big: true }, { id: "ramp", label: "Ramp", big: true },
      { id: "railing", label: "Railing", big: true } ] },
    { label: "Circulation", items: [] },
    { label: "Model", items: [
      { id: "model-line", label: "Model Line" }, { id: "model-text", label: "Model Text" }, { id: "model-group", label: "Model Group" } ] },
    { label: "Room & Area", items: [
      { id: "room", label: "Room" }, { id: "room-sep", label: "Room Separator" }, { id: "tag-room", label: "Tag Room" },
      { id: "area", label: "Area" } ] },
    { label: "Datum", items: [
      { id: "level", label: "Level", big: true }, { id: "grid", label: "Grid", big: true } ] },
    { label: "Work Plane", items: [
      { id: "set-wp", label: "Set" }, { id: "show-wp", label: "Show" }, { id: "ref-plane", label: "Ref Plane" }, { id: "viewer", label: "Viewer" } ] },
  ],
  Structure: [
    { label: "Structure", items: [
      { id: "s-beam", label: "Beam", big: true }, { id: "s-brace", label: "Brace", big: true },
      { id: "s-column", label: "Column", big: true }, { id: "s-truss", label: "Truss", big: true } ] },
    { label: "Foundation", items: [
      { id: "s-isolated", label: "Isolated Footing", big: true }, { id: "s-strip", label: "Strip Footing", big: true },
      { id: "s-slab", label: "Slab / Edge Thickening", big: true } ] },
  ],
  Insert: [
    { label: "Link", items: [
      { id: "link-ifc", label: "Link IFC", big: true }, { id: "link-model", label: "Link Model", big: true },
      { id: "link-dxf", label: "Link DXF", big: true } ] },
    { label: "Import", items: [
      { id: "import-dxf", label: "Import DXF", big: true }, { id: "import-image", label: "Image", big: true } ] },
    { label: "Load from Library", items: [
      { id: "load-family", label: "Load Family", big: true } ] },
  ],
  Annotate: [
    { label: "Dimension", items: [
      { id: "dim-aligned", label: "Aligned", big: true }, { id: "dim-linear", label: "Linear" }, { id: "dim-angular", label: "Angular" },
      { id: "dim-radial", label: "Radial" }, { id: "dim-diameter", label: "Diameter" }, { id: "dim-arc", label: "Arc Length" }, { id: "dim-elev", label: "Spot Elevation" }, { id: "dim-coord", label: "Spot Coordinate" }, { id: "dim-slope", label: "Spot Slope" } ] },
    { label: "Detail", items: [
      { id: "det-line", label: "Detail Line", big: true }, { id: "det-region", label: "Region", big: true, menu: [
        { id: "det-filled", label: "Filled Region" }, { id: "det-masking", label: "Masking Region" } ] },
      { id: "det-comp", label: "Component", big: true }, { id: "det-insulation", label: "Insulation", big: true },
      { id: "det-revcloud", label: "Revision Cloud", big: true } ] },
    { label: "Text", items: [
      { id: "text", label: "Text", big: true }, { id: "keynote", label: "Keynote", big: true } ] },
    { label: "Tag", items: [
      { id: "tag-all", label: "Tag All", big: true }, { id: "tag-cat", label: "Tag by Category", big: true },
      { id: "tag-mat", label: "Material Tag", big: true } ] },
    { label: "Symbol", items: [
      { id: "symbol", label: "Symbol", big: true }, { id: "north", label: "North Arrow", big: true }, { id: "scale-bar", label: "Scale Bar", big: true } ] },
  ],
  Site: [
    { label: "Surface", items: [
      { id: "topo-points", label: "Topo from Points", big: true }, { id: "topo-file", label: "Topo from 3D File", big: true },
      { id: "site-surface", label: "Add Surface", big: true }, { id: "grading", label: "Auto Grade", big: true },
      { id: "cutfill", label: "Cut / Fill", big: true } ] },
    { label: "Traffic", items: [
      { id: "parking", label: "Parking Bay", big: true }, { id: "linemark", label: "Line Marking", big: true },
      { id: "kerb", label: "Kerb", big: true }, { id: "kerb-ramp", label: "Kerb Ramp", big: true } ] },
  ],
  View: [
    { label: "Create", items: [
      { id: "v-plan", label: "Plan Views", big: true, menu: [
        { id: "v-floorplan", label: "Floor Plan" }, { id: "v-ceilingplan", label: "Reflected Ceiling Plan" } ] }, { id: "v-3d", label: "3D View", big: true },
      { id: "v-section", label: "Section", big: true }, { id: "v-elev", label: "Elevation", big: true },
      { id: "v-draft", label: "Drafting View", big: true }, { id: "v-legend", label: "Legend", big: true },
      { id: "v-sched", label: "Schedules", big: true }, { id: "v-sheet", label: "Sheet", big: true },
      { id: "v-dup", label: "Duplicate View", big: true } ] },
    { label: "Graphics", items: [
      { id: "v-vg", label: "Visibility / Graphics", big: true }, { id: "v-filters", label: "Filters", big: true },
      { id: "v-template", label: "View Templates", big: true } ] },
  ],
  Manage: [
    { label: "Settings", items: [
      { id: "m-materials", label: "Materials", big: true }, { id: "m-units", label: "Project Units", big: true },
      { id: "m-params", label: "Project Parameters", big: true }, { id: "m-globals", label: "Global Parameters", big: true }, { id: "m-shared", label: "Shared Parameters", big: true },
      { id: "m-styles", label: "Object Styles", big: true }, { id: "m-phases", label: "Phasing", big: true },
      { id: "m-revisions", label: "Revisions", big: true }, { id: "m-snaps", label: "Snaps", big: true } ] },
    { label: "Additional Settings", items: [
      { id: "m-linestyles", label: "Line Styles", big: true },
      { id: "m-lineweights", label: "Line Weights", big: true }, { id: "m-linepatterns", label: "Line Patterns", big: true },
      { id: "m-fillpatterns", label: "Fill Patterns", big: true }, { id: "m-tempdim", label: "Temporary Dimensions", big: true },
      { id: "m-families", label: "Families", big: true } ] },
    { label: "Export", items: [
      { id: "m-sheets", label: "Sheet Exporter", big: true } ] },
  ],
  Modify: MODIFY_TAB,
  Help: [
    { label: "Logging", items: [
      { id: "open-log", label: "Open Log Folder", big: true } ] },
  ],
};

const PROJECT_TREE = [
  { name: "Views (all)", open: true, children: [
    { name: "Floor Plans", open: true, children: [{ name: "Ground", sel: true }, { name: "Level 1" }, { name: "Site" }] },
    { name: "Ceiling Plans", children: [{ name: "Ground" }] },
    { name: "3D Views", children: [{ name: "{3D}" }] },
    { name: "Elevations", children: [{ name: "North" }, { name: "South" }, { name: "East" }, { name: "West" }] },
    { name: "Sections", children: [] },
    { name: "Drafting Views", children: [] },
    { name: "Legends", children: [] },
    { name: "Schedules / Quantities", children: [{ name: "Room Areas" }] },
    { name: "Sheets", children: [] },
  ] },
  { name: "Families", children: [{ name: "Walls" }, { name: "Doors" }, { name: "Windows" }, { name: "Structural Framing" }] },
  { name: "Groups", children: [] },
  { name: "Links", children: [] },
];

// The family editor's ribbon (the Create / Insert / Annotate / View / Manage / Modify for a family document).
// Ids start with "f-" and are run by FamModel.commands. Buttons without a command show as "not built yet".
const FAM_EDITOR_GROUP = { label: "Family Editor", items: [
  { id: "f-load", label: "Load into Project", big: true }, { id: "f-loadclose", label: "Load into Project and Close", big: true } ] };
// The ribbon of an annotation family document (a level head, an elevation mark, a section mark). Ids start with
// "a-" and are run by AnnModel.commands.
const ANN_EDITOR_GROUP = { label: "Family Editor", items: [
  { id: "a-load", label: "Load into Project", big: true }, { id: "a-loadclose", label: "Load into Project and Close", big: true } ] };
const ANN_RIBBON = {
  Create: [
    { label: "Select", items: [{ id: "a-select", label: "Modify", big: true }] },
    { label: "Properties", items: [
      { id: "a-category", label: "Family Category and Parameters", big: true }, { id: "a-types", label: "Family Types", big: true } ] },
    { label: "Detail", items: [
      { id: "a-line", label: "Line", big: true }, { id: "a-region", label: "Filled Region", big: true }, { id: "a-mask", label: "Masking Region", big: true },
      { id: "a-symbol", label: "Symbol", big: true } ] },
    { label: "Text", items: [{ id: "a-label", label: "Label", big: true }, { id: "a-text", label: "Text", big: true }] },
    { label: "Datum", items: [{ id: "a-refline", label: "Reference Line", big: true }] },
  ],
  View: [
    { label: "Graphics", items: [{ id: "a-flip", label: "Preview Flipped", big: true }, { id: "a-fit", label: "Zoom to Fit", big: true }] },
  ],
  Modify: MODIFY_TAB,
};

const FAM_RIBBON = {
  Create: [
    { label: "Select", items: [{ id: "f-select", label: "Select", big: true }] },
    { label: "Properties", items: [
      { id: "f-category", label: "Family Category and Parameters", big: true }, { id: "f-types", label: "Family Types", big: true } ] },
    { label: "Forms", items: [
      { id: "f-ext", label: "Extrusion", big: true }, { id: "f-blend", label: "Blend", big: true }, { id: "f-revolve", label: "Revolve", big: true },
      { id: "f-sweep", label: "Sweep", big: true }, { id: "f-swblend", label: "Swept Blend", big: true },
      { id: "f-void-menu", label: "Void Forms", big: true, menu: [
        { id: "f-void-ext", label: "Void Extrusion" }, { id: "f-void-blend", label: "Void Blend" }, { id: "f-void-revolve", label: "Void Revolve" },
        { id: "f-void-sweep", label: "Void Sweep" }, { id: "f-void-swblend", label: "Void Swept Blend" } ] } ] },
    { label: "Model", items: [
      { id: "f-mline", label: "Model Line" }, { id: "f-comp", label: "Component" }, { id: "f-mtext", label: "Model Text" },
      { id: "f-opening", label: "Opening" }, { id: "f-mgroup", label: "Model Group" } ] },
    { label: "Control", items: [{ id: "f-control", label: "Control", big: true }] },
    { label: "Connectors", items: [{ id: "f-conn", label: "Connectors", big: true, menu: [
      { id: "f-conn-electrical", label: "Electrical Connector" }, { id: "f-conn-duct", label: "Duct Connector" }, { id: "f-conn-pipe", label: "Pipe Connector" },
      { id: "f-conn-cable_tray", label: "Cable Tray Connector" }, { id: "f-conn-conduit", label: "Conduit Connector" } ] }] },
    { label: "Datum", items: [{ id: "f-refline", label: "Reference Line", big: true }, { id: "f-plane", label: "Reference Plane", big: true }] },
    { label: "Work Plane", items: [{ id: "f-wpset", label: "Set" }, { id: "f-wpshow", label: "Show" }, { id: "f-wpviewer", label: "Viewer" }] },
  ],
  Insert: [
    { label: "Load from Library", items: [{ id: "f-loadfam", label: "Load Family", big: true }] },
    { label: "Import", items: [{ id: "f-impcad", label: "Import CAD", big: true }, { id: "f-impimg", label: "Image", big: true }] },
  ],
  Annotate: [
    { label: "Dimension", items: [{ id: "f-dim", label: "Aligned", big: true }] },
    { label: "Detail", items: [
      { id: "f-sym", label: "Symbolic Line", big: true }, { id: "f-region", label: "Filled Region", big: true },
      { id: "f-mask", label: "Masking Region", big: true }, { id: "f-text", label: "Text", big: true } ] },
  ],
  View: [
    { label: "Create", items: [{ id: "f-3d", label: "3D View", big: true }, { id: "f-sec", label: "Section", big: true }] },
    { label: "Graphics", items: [
      { id: "f-names", label: "Plane Names", big: true }, { id: "f-dims", label: "Dimensions", big: true }, { id: "f-vg", label: "Visibility / Graphics", big: true } ] },
  ],
  Manage: [
    { label: "Settings", items: [
      { id: "f-sharedparams", label: "Shared Parameters", big: true }, { id: "f-styles", label: "Object Styles", big: true }, { id: "f-materials", label: "Materials", big: true } ] },
    { label: "Lookup", items: [{ id: "f-lookup", label: "Manage Lookup Tables", big: true }] },
  ],
  Modify: MODIFY_TAB,
};
