# OpenCADStudio - Unreleased (2026-10-18 to Present)

> **Summary:** Continued the Block Editor and dynamic block work with native Block Properties Table grips, reference-side row selection, and clearer authoring and preview behavior.

### 🌟 Notable Release Notes

* **Block Properties Table Editing:** Added native table grips and must-match behavior, reference-side row selection, and a popup that reflects the current authoring selection. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Block Editor Authoring Improvements:** Improved parameter editing and preview, graph item management, blank-pick selection clearing, legacy base-point preview, and the editor viewport appearance. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Lookup Rotation Matching:** Lookup rows now compare rotation values correctly across the angle wrap. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **HATCH Cell Trimming:** TRIM now removes the clicked filled cell from multi-edge partitions, retaining exact curved boundaries and safely refusing ambiguous or lossy results. *(Contributed by [@YsDirector](https://github.com/YsDirector))*

### 🚀 New Features

* **Native Block Properties Table Grips:** Author and edit table values through native grips, with must-match row behavior.
* **Reference-Side Row Selection:** Select a Block Properties Table row from a dynamic block reference and see its authoring state in the Block Editor.
* **Authoring Selection and Preview:** The Block Editor now presents table popup and selection state, edits and previews parameters, and manages parameter and base-point graph items more consistently.
* **HATCH Cell Trimming:** Classify the fill arrangement and remove only the clicked subdivision, including multiple dividers and analytic curved boundaries; retain holes, islands, OCS placement and the original HATCH identity. *(Contributed by [@YsDirector](https://github.com/YsDirector))*
* **Plugin API v11–v13:** Plugins can read and drive entity grips, place their own grip points and react to dragging them, create constraints, and run commands quietly. Plugins now learn which entities changed instead of rescanning the whole drawing, read and write records in batches, and start up more reliably. *(Contributed by [@schoeller](https://github.com/schoeller))*

### 🐛 Bug Fixes

* **Legacy Base-Point Preview:** Preserved the existing preview behavior for legacy block base points.
* **Blank-Pick Selection:** Cleared the authoring selection when the user picks a blank area.
* **Lookup Angle Wrap:** Compared rotation values consistently when equivalent angles straddle the wrap point.
* **Stable One-to-One Command Replacements:** Update same-type edits in place so reusing an entity handle cannot duplicate owning-block membership or serialized DXF records. *(Contributed by [@YsDirector](https://github.com/YsDirector))*
* **Rapid Drawing Responsiveness:** Point clicks snap once, at the button event's own position, so quick clicks no longer land on an earlier hover. While a plain point pick moves, discrete object snaps update at up to 60 Hz and the full snap pass runs once the cursor settles; Ortho, Polar, grid snap and tracking keep the full pass on every move. Simple line previews draw over the cached scene instead of re-rendering it. *(Contributed by [@Lukreynol01](https://github.com/Lukreynol01))*

### 👥 Contributors

* **Karim Jerbi** ([@KarimJerbi](https://github.com/KarimJerbi)) — Block Properties Table authoring and grips, reference-side row selection, and Block Editor selection, parameter, preview, and rotation matching improvements.
* **Luciano Naranjo Altunar** ([@Lukreynol01](https://github.com/Lukreynol01)) — Snap and preview work reduction during rapid drawing.
