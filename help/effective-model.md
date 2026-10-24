# Inspect macro expansion

With a model or included file active, run **Dygnosis: Expand macros** from the
Command Palette, or click **Expand macros** in the editor title bar
(`$(open-preview)`). The same action is available from the editor context menu
and as a CodeLens above the first `model` block.

Dygnosis opens a read-only preview beside your file with `@#include` files,
`@#for` loops, `@#if` branches and `@{...}` substitutions expanded before
Dynare's internal equation transformations.

| Action | Shortcut | Result |
|---|---|---|
| **Dygnosis: Go to written source** | `Ctrl+Alt+G` (`Cmd+Alt+G` on macOS) | Jumps from the preview to the written line that produced it (for example `beta_1` → `beta_@{j}`). |
| **Dygnosis: Refresh macro expansion** | `Ctrl+Alt+R` (`Cmd+Alt+R` on macOS) | Updates the preview after you edit the model or an include. |

`@#echo` and `@#echomacrovars` output appears in the **Dygnosis** channel in
[Output](action:output) when the preview opens or refreshes;
`@#echomacrovars(save)` inserts assignment lines into the preview. Related
settings: [Dynare › Effective Model: Macro
Tint](settings:dynare.effectiveModel.macroTint), [Dynare: Editor
Actions](settings:dynare.editorActions), and [Dynare: Macro Work
Limit](settings:dynare.macroWorkLimit).

## Choose the analysis configuration

Run **Dynare: Select Analysis Configuration** from the editor title gear,
Command Palette or editor context menu to test different macro values or
equation selections without modifying your `.mod` file.

### Macro inputs and previews

In **Select Analysis Configuration**, choose **Add macro input…** to supply a
top-level macro value such as `FLAG=true`, matching Dynare's [`-D` command-line
option](https://www.dynare.org/manual/running-dynare.html#-DMACRO_VARIABLE%5B=MACRO_EXPRESSION%5D).

Because an in-file `@#define` replaces a `-D` input when reached:

```dynare
@#define FLAG = false
```

you can also place the cursor directly on a macro directive in the editor and
run:

- **Dynare: Override Macro Definition for Analysis** to substitute a different
  value (such as `true`) at that definition.
- **Dynare: Skip Macro Definition for Analysis** to ignore that `@#define`.
- **Dynare: Preview Macro Branch** on an `@#if`, `@#elseif` or `@#else` arm to
  preview that branch.

See the [Dynare manual on macro
directives](https://www.dynare.org/manual/the-model-file.html#macro-directives).

### Include or exclude equations

Choose **Include equations…** or **Exclude equations…** to filter model
equations using Dynare's [`include_eqs` and `exclude_eqs` tag
rules](https://www.dynare.org/manual/running-dynare.html#exclude-eqs). Enter a
tag expression with single-quoted values, such as `name='policy'`,
`endogenous='z'` or `name=('policy','demand')` (selector files are not
supported).

```dynare
var y z;
model;
  [name='demand'] y = z;
  [name='policy', endogenous='z'] z = 0;
end;
```

Excluding `name='policy'` removes that equation and converts its matched
variable `z` into an exogenous variable (since `demand` still uses `z`; if no
remaining equation uses `z`, it is removed too). Dynare identifies the matched
endogenous variable from the removed equation's `[endogenous='...']` tag, or
from its left-hand side when the LHS contains a single endogenous variable.

Equation selection updates model counts, variable types and diagnostics. The
macro expansion preview still displays the full expanded text before equation
filtering.

### Save, reset and compare

- **Dynare: Save Analysis Configuration**: Saves the current choices in
  [Dynare: Analysis Configurations](settings:dynare.analysisConfigurations) so
  you can switch back to them later.
- **Dynare: Reset Analysis Configuration**: Returns to the default written
  **Source** configuration.
- **Compare configurations**: Run **Dygnosis: Show mod changes** and choose
  **config…** to diff two configurations ([Review model
  changes](structural-diff.md)). For navigating inactive macro branches, see
  [Inactive source](navigate-code.md#inactive-source).
