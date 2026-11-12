# Saved projects

Project files as earlier builds wrote them. The test `tests/saved_files.rs` opens each one with the build of today,
checks it still holds and measures what it did, draws every view, saves it and opens it again.

- One file per build that people have used (each installer build at least), named by the date and the commit that
  saved it: `2026-11-10-f856f94.oab`.
- Beside it a `.json` with what it held when it was saved: `format_version`, `kinds` (how many elements of each
  kind) and `room_areas` (each room's id and the Area its Properties show).
- Never replace or re-save a file here. The point is that today's build reads the bytes an old build wrote.
- A file should use as much of the program as it can: every kind of element the build can make.

When the format version is raised (`FORMAT_VERSION` in `crates/openarch-model/src/store.rs`), save a new file with
the last build before the change, so the step across it stays tested.
