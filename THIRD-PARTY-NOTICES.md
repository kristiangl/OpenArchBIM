# Third-party notices

OpenArch BIM is licensed under the GNU Affero General Public License v3.0 only (see LICENSE). Some of its code is
ported from other open-source projects under licences that allow it. Each such project, what was taken from it and
its licence notice are listed here.

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
