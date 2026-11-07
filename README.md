<img width="400" alt="Logo" src="https://github.com/user-attachments/assets/a8d24118-092c-4e25-93fd-096e7065c8b0" />

# OpenArch BIM

A free, open-source building information modelling (BIM) application for architects. Windows, Mac and Linux. Built in Rust.

**Status: early development. Not ready for real project work.** Project files will change between versions until a stable file format is announced.

## What it does today

- Levels and grids, with datum alignment locks and a project base point and survey point
- Layered walls with wall types, mitred and butted joins and join cleanup, plus curved walls and curtain walls (grids, panels, mullions, doors)
- Floors, roofs and ceilings drawn from sketch boundaries, with slopes and slope arrows
- Stairs with runs and automatic landings, ramps, and railings
- Toposolids for the site: sketched or imported from a survey points file, shaped with points, with contours by type and cut through their layers in sections
- Doors and windows from loadable families, with tags and mark numbering
- A family editor: reference planes, parameters, types, lookup tables, nested families, and extrusions, blends, sweeps and voids
- Plan, elevation, section and 3D views, with jogged sections, depth cueing, view ranges, templates, graphic display options, and hide and isolate
- Permanent and temporary dimensions, spot elevations, coordinates and slopes, text notes, detail lines and filled regions
- Rooms and areas (with colour fills), and schedules with calculated columns, key schedules, material takeoffs and cross-schedule totals
- Sheets with schedules on them, and export of sheets and views to PDF and DXF
- IFC4 export and IFC import
- Groups, arrays, copy and paste, Match Type, and Move, Copy, Rotate and Mirror in the 3D view
- Undo and redo, and a single-file project format (`.oab`) with loadable family files (`.oaf`)

## What is planned

- Roofs by extrusion and by face, plus soffits, fascias and gutters
- Legends and 2D drafting views
- Columns, beams and other structural elements
- Linked models in the app, on top of the engine already built for large masterplans
- More site tools: excavation by buildings, sub-divisions, graded regions with cut and fill, property lines, parking and kerbs
- Phases, view filters and scope boxes
- Sun path, shadows and render export
- Worksharing, structural and MEP tools, once the architectural basics are in

Australian standards and metric units come first. Other regions are welcome later.

## Principles

- **Open source.** Read it, fork it, build on it.
- **Native.** A Rust core with a WebGPU viewport (WebGL2 where the webview has no WebGPU, as on Linux). No cloud round trips between you and your files.
- **Familiar.** Concepts and workflows that working architects already know, so there is little to relearn.
- **Built for big models.** Linked models and large projects are a design goal, not an afterthought.

## Is this "vibe-coded"?

Yes, AI was used to create this app. However, unlike most other vibe-coded apps, I am taking an iterative approach to development. I have been using other BIM programs for over 15 years, so I know what feels right and wrong in the app. My approach involves implementing one feature at a time and troubleshooting and bug-fixing as I go. So rather than immediately having a full app with all of the tools that only kind of works, there is a lot more time involved up-front before there is even a usable version of the app. But when we do finally launch, it will be a much more polished experience.

## Independence

OpenArch BIM is an independent project. It is not affiliated with, endorsed by or sponsored by any software vendor. All product names and trademarks belong to their owners.

OpenArch BIM is written from scratch from published documentation, open standards and the authors' own professional experience. No code, binaries or other proprietary material from any other software product has been copied, decompiled or reverse engineered.

## Repository layout

- `crates/openarch-params` - formula language, parameter dependency graph, schedules
- `crates/openarch-links` - shared definitions and cheap placements for linked models, spatial index, level-of-detail batching
- `crates/openarch-model` - elements, joins, undo and redo, geometry, display meshes, the project file
- `crates/openarch-api` - JSON command interface used by the UI
- `crates/openarch-dev` - dev server that runs the UI and the Rust core in a normal browser
- `apps/desktop` - Tauri shell with a ribbon, Properties palette, Project Browser and the 3D viewport (WebGPU or WebGL2)

## Build and run

Run the tests:

    cargo test --workspace

Run the whole app in a browser during development:

    cargo run -p openarch-dev

then open http://127.0.0.1:8765. Windows previews use the Explorer file picker; other browser preview hosts use an in-app path dialog.

Selecting a section line shows its view properties. Edit Type selects a Section Tag; Marker Type Properties edits or duplicates its head and tail symbol choices. Elevation view types similarly select an Elevation Mark. Section end grips lengthen or shorten along the current cut plane, with a perpendicular range box throughout the drag; Align and Rotate can change its angle. New projects include separate Section Head and Section Tail families, anchored where their reference planes cross. The editable source families are in `crates/openarch-model/families`. Existing project families keep their saved artwork and type choices.

Projects saved with this build use file format 14, which adds toposolids (format 13 added project instance parameters, key schedules, rooms, revisions, door and window tags, ceilings, areas and colour fill schemes). Older projects are upgraded on opening, preserving existing marker choices, but earlier app builds cannot open files saved in this format.

For a key schedule, choose Schedule keys in New Schedule, select a category and name the key selector. Add instance fields or use Add Parameter for a project/shared parameter. Insert Data Row creates a key; edit its values, then assign it to elements in Properties or an ordinary schedule. Set the selector to (none) to unlock the values while keeping them.

Rooms: Architecture > Room places a room with a click inside an enclosed area; its Modify tab panels turn Tag on Placement on or off, place rooms in every enclosed area, open Area and Volume Computations, and pick an unplaced room to place instead of a new one. Delete in a view leaves a room in the project, unplaced; Delete Row in a room schedule removes it. A room schedule's Embedded Schedule tab lists one category's elements under each room.

Tags: Annotate > Tag by Category tags the door or window you click, Tag All tags every untagged door, window or room in the view, and Place Door and Place Window tag as they place (Tag on Placement on their Modify tab). Spacebar turns a selected tag between horizontal and vertical. A tag follows its door or window when it moves.

To export schedule values to IFC, tick Enable this schedule for IFC export on its IFC Export tab (where fields can be renamed or sent to a quantity set), then tick Export schedules as property and quantity sets in Export > IFC.

Run the desktop app (needs Rust and your platform's webview libraries):

    cd apps/desktop && npm install && npm run dev

On Ubuntu or Debian the webview libraries are:

    sudo apt install libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

Installers for Windows, Mac and Linux are built by GitHub Actions for tagged releases. Linux comes as a .deb (Ubuntu 22.04 and later, Debian 12 and later), an .rpm (Fedora and similar) and an AppImage (most other distributions: make it executable and run it). On Linux the 3D view draws with WebGL2, because the system webview (WebKitGTK) does not offer WebGPU yet. With the NVIDIA proprietary driver the app turns off WebKitGTK's DMA-BUF renderer, which otherwise leaves the window blank.

Mac builds are ad-hoc signed but not notarized, so a dmg downloaded in a browser is quarantined and macOS says "damaged". Fix once in Terminal, then open normally:

    xattr -dr com.apple.quarantine ~/Downloads/"OpenArch BIM_"*.dmg      # before opening the dmg
    xattr -cr "/Applications/OpenArch BIM.app"                           # after copying the app out

## Contributing

Contribution guidelines are coming before the first public release. Until then, please open an issue before sending a pull request.

## Licence

OpenArch BIM is free software under the GNU Affero General Public License v3.0 (AGPL-3.0-only). See [LICENSE](LICENSE). If you run a modified version as a network service, you must offer its source to the users of that service.
