# Third-party notices

OpenArch BIM is licensed under the GNU Affero General Public License v3.0 only (see LICENSE). Some of its code is
ported from other open-source projects under licences that allow it, and some libraries it is built with ask for a
notice. Each such project, what was taken from it or how it is used, and its licence notice are listed here.

## BIMCraft

- Source: https://github.com/IPedrax/BIMCraft, at commit c269c8c.
- Used under the MIT licence (BIMCraft is MIT OR Apache-2.0).
- Ported: the IFC4 export (`crates/core/src/ifc.rs`) into `crates/openarch-model/src/ifc.rs`. This covers the STEP
  writer (string escaping, reals, GlobalId compression, entity numbering), the spatial tree, swept bodies, the
  opening and fill relations, the material layer sets and the file header. It was rewritten to work with OpenArch's
  model.
- Ported: the STEP file reader (`crates/core/src/step.rs`) into `crates/openarch-model/src/step.rs`, nearly as it is.
- Ported: parts of the IFC import (`crates/core/src/import.rs`) into `crates/openarch-model/src/ifc_import.rs`. This
  covers the geometry reader (placements, profiles, curves, extrusions, B-reps, face sets, mapped items), the units,
  the relations between elements, the outline of a slab's top, the shift for models far from the origin and the
  joining of walls that nearly meet. The mapping onto OpenArch's levels, walls, floors, doors and windows is new.

```
MIT License

Copyright (c) 2026 IPedrax and the BIMCraft contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## acadrust

- Source: https://github.com/hakanaktt/acadrust, version 0.6.8 as published on crates.io
  (https://crates.io/crates/acadrust/0.6.8). Copyright Hakan AK and the acadrust contributors.
- Licence: Mozilla Public License 2.0. The licence text is at https://www.mozilla.org/en-US/MPL/2.0/ and in the
  crate's `LICENSE` file.
- Used unmodified, as a separate library: `crates/openarch-cad` calls it to read DXF and DWG files for Import CAD and
  Link CAD. None of its source is copied into OpenArch BIM. Its source code is available from the two addresses above.
- acadrust's own notices say its DWG reader and writer include code ported from ACadSharp, under the MIT licence:

```
MIT License

Copyright (c) 2021 Albert Domenech

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## rvt-rs

- Source: https://github.com/DrunkOnJava/rvt-rs, at commit 62cc17e (version 0.25.0). Copyright 2026 Griffin Long.
- Licence: Apache License 2.0. The licence text is at https://www.apache.org/licenses/LICENSE-2.0 and in the
  repository's `LICENSE` file.
- Used unmodified, as a separate library (the `rvt` crate with its `unstable-research` feature): `crates/openarch-rfa`
  calls it to read Revit family files (.rfa) for Load Family. None of its source is copied into OpenArch BIM. Its
  source code is available from the address above. Its NOTICE file:

```
rvt-rs — open reader for Autodesk Revit files
Copyright 2026 Griffin Long

Licensed under the Apache License, Version 2.0 (see LICENSE).

==============================================================================
Trademark and attribution notices
==============================================================================

"Autodesk" and "Revit" are registered trademarks of Autodesk, Inc.
This project is not affiliated with, endorsed by, or sponsored by
Autodesk. All references to "Autodesk" and "Revit" in this project's
source, tests, documentation, and output are nominative fair use —
they identify the file format this reader parses, and no more.

This project neither uses nor distributes any Autodesk source code,
object code, proprietary SDK components, or header files. All
file-format observations documented here were obtained by inspecting
the bytes of files written by Autodesk Revit — publicly shipped
sample content — using only public tools (the Microsoft Compound
File Binary Format specification [MS-CFB], RFC 1952 gzip, and
standard Rust crates).

==============================================================================
Interoperability basis
==============================================================================

Reverse engineering for the sole purpose of achieving interoperability
between independently-developed software and the Autodesk Revit file
format is recognised as lawful fair use under:

  * United States — Sega Enterprises v. Accolade, 977 F.2d 1510
    (9th Cir. 1992); Sony Computer Entertainment v. Connectix, 203
    F.3d 596 (9th Cir. 2000).
  * European Union — Article 6 of Directive 2009/24/EC (Software
    Directive).
  * Australia — Copyright Act 1968, Section 47D.

File formats themselves are not copyrightable subject matter (Baker
v. Selden, 101 U.S. 99 (1879); Lotus Development v. Borland, 516
U.S. 233 (1996)). The Microsoft Compound File Binary Format is a
published public specification; the compression (RFC 1951 DEFLATE)
is an Internet standard.

No access-control or rights-management technology has been
circumvented by this project — the underlying Revit files are not
encrypted and carry no technological protection measures.

==============================================================================
Third-party dependencies (runtime)
==============================================================================

  cfb           — MIT OR Apache-2.0
  flate2        — MIT OR Apache-2.0
  encoding_rs   — Apache-2.0 OR MIT
  quick-xml     — MIT
  serde         — MIT OR Apache-2.0
  serde_json    — MIT OR Apache-2.0
  clap          — MIT OR Apache-2.0
  anyhow        — MIT OR Apache-2.0
  thiserror     — MIT OR Apache-2.0

See each dependency's own license for full terms. All runtime
dependencies are permissively licensed and compatible with
Apache-2.0 redistribution.

==============================================================================
Test-corpus attribution
==============================================================================

The integration test suite uses the publicly-available
`rac_basic_sample_family` RFA fixture family distributed by
Autodesk as part of every Revit release, and curated into a
Git-LFS-hosted repository by Andreas Philipp at
https://github.com/phi-ag/rvt under the Unlicense. The sample
family files themselves are Autodesk property; this project
consumes them only for interoperability-verification testing and
does not redistribute them (the phi-ag/rvt test corpus is pulled
via LFS from their repo at `git clone` time; it is not vendored
here).
```
