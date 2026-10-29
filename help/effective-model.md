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

Run **Dygnosis: Select analysis configuration** from the editor title gear,
Command Palette or editor context menu to test different macro values or
equation selections without modifying your `.mod` file.

### Macro inputs and previews

In **Select analysis configuration**, choose **Add macro input…** to supply a
top-level macro value such as `FLAG=true`, matching Dynare's [`-D` command-line
option](https://www.dynare.org/manual/running-dynare.html#-DMACRO_VARIABLE%5B=MACRO_EXPRESSION%5D).

Because an in-file `@#define` replaces a `-D` input when reached:

```dynare
@#define FLAG = false
```

you can also place the cursor directly on a macro directive in the editor and
run:

- **Dygnosis: Override macro definition for analysis** to substitute a different
  value (such as `true`) at that definition.
- **Dygnosis: Skip macro definition for analysis** to ignore that `@#define`.
- **Dygnosis: Preview macro branch** on an `@#if`, `@#elseif` or `@#else` arm to
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

- In **Select analysis configuration**, choose **Save current configuration**
  to save your choices in [Dynare: Analysis Configurations](settings:dynare.analysisConfigurations),
  or **Source** to reset them. You can also run **Dygnosis: Save analysis
  configuration** or **Dygnosis: Reset analysis configuration** from the
  Command Palette.
- **Compare configurations**: Run **Dygnosis: Show model changes** and choose
  **config…** to diff two configurations ([Review model
  changes](structural-diff.md)). For navigating inactive macro branches, see
  [Inactive source](navigate-code.md#inactive-source).
