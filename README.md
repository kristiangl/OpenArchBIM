# OpenArch BIM

A free, open-source building information modelling (BIM) application for architects. Windows and Mac. Built in Rust.

**Status: early development. Not ready for real project work.** Project files will change between versions until a stable file format is announced.

## What it does today

- Levels and grids, with datum alignment locks and a project base point and survey point
- Layered walls with wall types, mitred and butted joins, and join cleanup
- Floors and roofs drawn from sketch boundaries, with slope-defining edges and slope arrows
- Doors and windows from loadable families, with a family editor (reference planes, parameters, types, nested geometry)
- Plan, elevation and section views, with view ranges, templates and graphic display options
- Permanent and temporary dimensions, spot elevations and spot coordinates
- Schedules with calculated columns and cross-schedule totals
- Linked models built for scale: shared definitions with cheap placements, aimed at masterplans with many linked buildings
- Undo and redo, and a single-file project format (`.oab`) with loadable family files (`.oaf`)

## What is planned

- Sheets and title blocks (a generic A0 to A3 set will ship first)
- PDF, DXF and IFC export, plus batch sheet export

Australian standards and metric units come first. Other regions are welcome later.

## Principles

- **Open source.** Read it, fork it, build on it.
- **Native.** A Rust core with a WebGPU viewport. No cloud round trips between you and your files.
- **Familiar.** Concepts and workflows that working architects already know, so there is little to relearn.
- **Built for big models.** Linked models and large projects are a design goal, not an afterthought.

## Independence

OpenArch BIM is an independent project. It is not affiliated with, endorsed by or sponsored by any software vendor. All product names and trademarks belong to their owners.

OpenArch BIM is written from scratch from published documentation, open standards and the authors' own professional experience. No code, binaries or other proprietary material from any other software product has been copied, decompiled or reverse engineered.

## Repository layout

- `crates/openarch-params` - formula language, parameter dependency graph, schedules
- `crates/openarch-links` - shared definitions and cheap placements for linked models, spatial index, level-of-detail batching
- `crates/openarch-model` - elements, joins, undo and redo, geometry, display meshes, the project file
- `crates/openarch-api` - JSON command interface used by the UI
- `crates/openarch-dev` - dev server that runs the UI and the Rust core in a normal browser
- `apps/desktop` - Tauri shell with a ribbon, Properties palette, Project Browser and WebGPU viewport

## Build and run

Run the tests:

    cargo test --workspace

Run the whole app in a browser during development:

    cargo run -p openarch-dev

then open http://127.0.0.1:8765.

Run the desktop app (needs Rust and your platform's webview libraries):

    cd apps/desktop && npm install && npm run dev

Installers for Windows and Mac are built by GitHub Actions for tagged releases. Mac builds are ad-hoc signed but not notarized, so a dmg downloaded in a browser is quarantined and macOS says "damaged". Fix once in Terminal, then open normally:

    xattr -dr com.apple.quarantine ~/Downloads/"OpenArch BIM_"*.dmg      # before opening the dmg
    xattr -cr "/Applications/OpenArch BIM.app"                           # after copying the app out

## Contributing

Contribution guidelines are coming before the first public release. Until then, please open an issue before sending a pull request.

## Licence

OpenArch BIM is free software under the GNU Affero General Public License v3.0 (AGPL-3.0-only). See [LICENSE](LICENSE). If you run a modified version as a network service, you must offer its source to the users of that service.
