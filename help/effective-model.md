# Inspect macro expansion

With a model or included file active, run **Dygnosis: Expand macros** from the
Command Palette, or click **Expand macros** in the editor title bar.

Dygnosis opens a read-only editor tab beside your file with `@#include` files,
`@#for` loops, `@#if` branches and `@{...}` substitutions expanded before
Dynare's internal equation transformations.

For model structure, open VS Code's **Outline** while the preview is active.

## Choose the analysis configuration

The source editor and macro expansion preview share one analysis configuration
per model root. Run **Dynare: Select Analysis Configuration** from either view
or the model status: **Source** (written macro inputs only), **Set macro
inputs…**, or a saved preset for this root.

| Command | Use |
|---|---|
| **Dynare: Override Macro Definition for Analysis** | On a macro definition: substitute an expression or **Skip this definition**. |
| **Dynare: Preview Macro Branch** | In an `@#if` chain: preview another arm or **No branch**. |
| **Dynare: Reset Analysis Configuration** | Return to **Source**. |
| **Dynare: Save Analysis Configuration** | Save current choices on the workspace folder ([Dynare: Analysis Configurations](settings:dynare.analysisConfigurations); not auto-applied in new sessions). |

Overrides and branch previews affect analysis and the preview only, not saved
files. The model status hover and preview title show the active selection. To
compare two configurations, see [Review model changes](structural-diff.md).
Counts and other model facts follow the selected configuration; for written code
outside it, see [Inactive source](navigate-code.md#inactive-source).

| Action | Shortcut | Result |
|---|---|---|
| **Dygnosis: Go to written source** | `Ctrl+Alt+G` (`Cmd+Alt+G` on macOS) | Jumps from the cursor or selection in the preview to the line in your model or include file that produced it (for example, `beta_1` jumps to `beta_@{j}`). |
| **Dygnosis: Refresh macro expansion** | `Ctrl+Alt+R` (`Cmd+Alt+R` on macOS) | Updates the preview after you edit the model or an included file. |

Both actions also appear in the preview title bar and editor right-click menu.
Optional: [Macro Tint](settings:dynare.effectiveModel.macroTint), [Editor
Actions](settings:dynare.editorActions), and [Macro Work
Limit](settings:dynare.macroWorkLimit).

`@#echo` and `@#echomacrovars` messages are written to the **Dygnosis** channel
in [Output](action:output) when the preview opens or refreshes, while
`@#echomacrovars(save)` inserts assignment lines directly into the preview.
