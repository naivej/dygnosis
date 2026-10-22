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

For model structure, open VS Code's **Outline** while the preview is active.

## Choose the analysis configuration

The source editor and macro expansion preview use one analysis configuration
for the model root. To change it, run **Dynare: Select Analysis Configuration**
from either view or use the model status. The selector starts with **Source**,
which uses the macro inputs written in the model. Choose **Set macro inputs…**
to enter initial values, or select a saved configuration for this root.

To change one written macro definition, place the cursor on it in the source
editor and run **Dynare: Override Macro Definition for Analysis**. Enter a
replacement expression or choose **Skip this definition**. To inspect another
arm of a conditional, place the cursor in its chain and run **Dynare: Preview
Macro Branch**; choose a listed arm or **No branch**. These choices affect the
analysis and preview. They do not change the `.mod` or include file. The model
status and preview title identify the selected configuration; previews are
labelled.

Run **Dynare: Reset Analysis Configuration** to return to **Source**. To save
the current choices for later, run **Dynare: Save Analysis Configuration**
from a model in a workspace folder. Saved configurations are available from
the selector, but do not start automatically in a new session. The folder's
saved presets are listed in [Dynare: Analysis Configurations](settings:dynare.analysisConfigurations).
For comparison steps, see [Review model changes](structural-diff.md).

Counts and other model facts come from the selected configuration. To browse
written code outside that selection, see [Inactive source](navigate-code.md#inactive-source).

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
