# Inspect macro expansion

With a model or included file active, open the macro expansion preview from any
of these places:

- Click **Expand macros** in the top-right editor title bar (`$(open-preview)`).
- Right-click in the editor and choose **Expand macros**.
- Click the **Expand macros** CodeLens above the first `model` block (when enabled).
- Run **Dygnosis: Expand macros** from the Command Palette.

Dygnosis opens a read-only editor tab beside your file with `@#include` files,
`@#for` loops, `@#if` branches and `@{...}` substitutions expanded before
Dynare's internal equation transformations.

| Action | Shortcut | Result |
|---|---|---|
| **Dygnosis: Go to written source** | `Ctrl+Alt+G` (`Cmd+Alt+G` on macOS) | Jumps from the cursor or selection in the preview to the line in your model or include file that produced it (for example, `beta_1` jumps to `beta_@{j}`). |
| **Dygnosis: Refresh macro expansion** | `Ctrl+Alt+R` (`Cmd+Alt+R` on macOS) | Updates the preview after you edit the model or an included file. |

Both actions also appear as buttons in the preview's top-right editor title bar
and in the editor right-click menu.

- [Dynare › Effective Model: Macro Tint](settings:dynare.effectiveModel.macroTint) shades macro-expanded ranges in the preview (hovering over shaded text confirms it is macro-expanded).
- [Dynare: Editor Actions](settings:dynare.editorActions) controls whether model actions appear in the editor `toolbar` and `contextMenu`.
- [Dynare: Macro Work Limit](settings:dynare.macroWorkLimit) increases the macro evaluation budget for large macro programs.

`@#echo` and `@#echomacrovars` messages are written to the **Dygnosis** channel
in [Output](action:output) when the preview opens or refreshes, while
`@#echomacrovars(save)` inserts assignment lines directly into the preview.
