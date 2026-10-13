# Inspect macro expansion

With your model active, run **Dygnosis: Expand macros** from the Command Palette.
It opens a read-only editor tab with includes, macro loops,
conditions and substitutions expanded. Written spacing stays in place around
the expanded text. The preview is before Dynare's equation transformations.
[Macro tint](settings:dynare.effectiveModel.macroTint) can shade the expanded
regions. [Editor actions](settings:dynare.editorActions) controls where model
actions appear in title bars and context menus; commands remain in the Command
Palette.

| Action | Result |
|---|---|
| Dygnosis: Go to written source | With the preview active, opens the line in your model file that produced the text at the cursor or selection. A generated name such as `beta_1` leads to `beta_@{j}`. |
| Dygnosis: Refresh macro expansion | With the preview active, updates it from the current model and includes. Use this after editing a model file. |

`@#echo` and `@#echomacrovars` messages appear in
[Output](action:output) when the preview opens or refreshes. In that panel,
select **Dygnosis** from the channel list to read them.
`@#echomacrovars(save)` adds assignment text to the preview. These messages do
not appear as diagnostics.
