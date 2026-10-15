# OpenArch BIM - notes for Claude

- Project: BIM authoring app (open-source alternative to the incumbent BIM tools), internal to Lawrence Associates first. Australian standards and metric units first, by default.
- Stack: Rust core, Tauri 2 shell, WebGPU viewport. Avoid GPL dependencies (commercial use stays possible).
- Plan (plan-v0.2) and the feature parity list live in the private claude.ai Project for this app, not in this repo; check the parity list before building a feature and add notes from the public help documentation for that feature.
- The Rust crates have no GUI dependencies and are tested on Linux (`cargo test --workspace`). `apps/desktop/src-tauri` is excluded from the workspace and only builds on CI (Windows/Mac) or with platform webview libraries.
- Crates: openarch-params (formulas, schedules), openarch-links (instanced links), openarch-model (elements, undo/redo, geometry, project file), openarch-api (JSON commands used by the UI), openarch-dev (browser dev server).
- UI testing: `cargo run -p openarch-dev -- --port 8777`, then drive http://localhost:8777 with Playwright. The same Rust API runs behind it. Headless Chromium gets software WebGPU with `--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader --use-angle=swiftshader --ignore-gpu-blocklist`, so 3D picking and screenshots work in tests (slow, and not a substitute for a real GPU check).
- Writing style for any prose: no em dashes, plain direct wording.
- Phase 0 spikes still open: OpenCascade from Rust, IFC round trip, macOS WebGPU check on a real Mac.
- Parity rule: everything built must already allow for the industry-standard feature it will grow into (see the parity list in the Project). Store the data the future tool or UI will edit from the start, with serde defaults so older project files still open. Do not ship a simplified model that will need reshaping later. Check the parity list before starting any feature.
