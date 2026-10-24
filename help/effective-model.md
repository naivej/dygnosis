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

Run **Dynare: Select Analysis Configuration** from the editor title gear,
Command Palette or editor context menu. These choices apply to the model root
and its macro expansion preview without changing files or your Dynare run.

### Macro inputs and previews

**Add macro input…** accepts an assignment such as `FLAG=true`, like Dynare's
[`-D` option](https://www.dynare.org/manual/running-dynare.html#-DMACRO_VARIABLE%5B=MACRO_EXPRESSION%5D).
A later definition in the file replaces that input:

```dynare
@#define FLAG = false
```

To preview `true` at this definition, place the cursor on it, run **Dynare:
Override Macro Definition for Analysis**, and enter `true`. **Skip Macro
Definition for Analysis** skips that definition; **Preview Macro Branch**
previews another conditional arm. See Dynare's [macro
directives](https://www.dynare.org/manual/the-model-file.html#macro-directives).

### Include or exclude equations

**Include equations…** keeps the matching equations; **Exclude equations…**
removes them from analysis. Both follow Dynare's
[`include_eqs` and `exclude_eqs` rules](https://www.dynare.org/manual/running-dynare.html#exclude-eqs).
Enter a tag expression such as `name='policy'`, `endogenous='z'` or
`name=('policy','demand')`. Tag values use single quotes; selector files are
not supported.

```dynare
var y z;
model;
  [name='demand'] y = z;
  [name='policy', endogenous='z'] z = 0;
end;
```

Excluding `name='policy'` removes its equation and makes `z` exogenous because
the demand equation still uses it. If no remaining equation uses that variable,
it leaves the model. Dynare chooses the variable from the removed equation's
`endogenous` tag; without that tag, its left side must contain exactly one
distinct endogenous variable. For example, `2*z` qualifies; `y+z` needs a tag.
Restoring the equations restores their variable types.

Model counts use the selected equations. The macro expansion preview shows
the text before equation selection, and source checks still cover excluded
equations. Choose **Clear equation selection** to keep the other inputs.

### Save, reset and compare

**Dynare: Save Analysis Configuration** stores choices in
[Dynare: Analysis Configurations](settings:dynare.analysisConfigurations).
Choose a saved configuration to reuse it; each new session starts with
**Source**. **Dynare: Reset Analysis Configuration** returns to Source.
To compare configurations, run **Show mod changes** and choose **config…**;
see [Review model changes](structural-diff.md). To browse code outside the
selection, see [Inactive source](navigate-code.md#inactive-source).

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
