# Inspect macro expansion

With a model or included file active, run **Dygnosis: Expand macros** from the
Command Palette, or click **Expand macros** in the editor title bar
(`$(open-preview)`). The same action is also on the editor context menu and,
when enabled, as a CodeLens above the first `model` block.

Dygnosis opens a read-only editor tab beside your file with `@#include` files,
`@#for` loops, `@#if` branches and `@{...}` substitutions expanded before
Dynare's internal equation transformations. Open **Outline** on the preview for
structure.

## Choose the analysis configuration

The source editor and macro expansion preview share one analysis configuration
for the model root. Change it with **Dynare: Select Analysis Configuration**
(or the model status):

| Choice | Effect |
|---|---|
| **Source** | Macro inputs as written in the model. |
| **Set macro inputs…** | Enter initial macro values for this root. |
| A saved configuration | Reuse presets listed in [Dynare: Analysis Configurations](settings:dynare.analysisConfigurations). |

For one definition, place the cursor on it and run **Dynare: Override Macro
Definition for Analysis** (replacement expression, or **Skip this definition**).
For another conditional arm, place the cursor in its chain and run **Dynare:
Preview Macro Branch**. These choices affect analysis and preview only; they do
not edit the files.

**Dynare: Reset Analysis Configuration** returns to **Source**. **Dynare: Save
Analysis Configuration** stores the current choices for a model in a workspace
folder (they do not auto-apply in a new session). To compare two configurations,
see [Review model changes](structural-diff.md).

Counts and other model facts follow the selected configuration. For written
code outside that selection, see [Inactive source](navigate-code.md#inactive-source).

| Action | Shortcut | Result |
|---|---|---|
| **Dygnosis: Go to written source** | `Ctrl+Alt+G` (`Cmd+Alt+G` on macOS) | Jumps from the preview to the written line that produced it (for example, `beta_1` → `beta_@{j}`). |
| **Dygnosis: Refresh macro expansion** | `Ctrl+Alt+R` (`Cmd+Alt+R` on macOS) | Updates the preview after you edit the model or an include. |

Both actions also appear in the preview title bar and context menu. Related
settings: [Dynare › Effective Model: Macro Tint](settings:dynare.effectiveModel.macroTint),
[Dynare: Editor Actions](settings:dynare.editorActions), and
[Dynare: Macro Work Limit](settings:dynare.macroWorkLimit).

`@#echo` and `@#echomacrovars` messages go to the **Dygnosis** channel in
[Output](action:output) when the preview opens or refreshes;
`@#echomacrovars(save)` inserts assignment lines into the preview.
