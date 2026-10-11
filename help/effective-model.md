# Inspect macro expansion

**Expand macros** opens a read-only model preview with includes, macro loops,
conditions and substitutions expanded. Written spacing stays in place around
the expanded text. The preview is before Dynare's equation transformations.
[Macro tint](settings:dynare.effectiveModel.macroTint) can shade the expanded
regions. [Editor actions](settings:dynare.editorActions) controls where model
actions appear in title bars and context menus; commands remain in the Command
Palette.

| Action | Result |
|---|---|
| Go to written source | The written text behind the cursor or selection. A generated name such as `beta_1` leads to `beta_@{j}`. |
| Refresh macro expansion | A new preview from the current model and includes. |

Source changes make the preview out of date. Source jumps remain unavailable
until refresh, and a selection spanning several source regions may have no
single target. Incomplete expansion shows its reason and has no source jumps.

`@#echo` and `@#echomacrovars` messages appear in
[Dygnosis Output](action:output) when the preview opens or refreshes.
`@#echomacrovars(save)` adds assignment text to the preview. These messages do
not appear as diagnostics.
