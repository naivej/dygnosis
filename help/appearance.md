# Read labels, hints and colors

Dygnosis uses metadata labels, inline value hints, block shading and semantic
colors to make models easier to read. Click any setting link below to open it
in VS Code's Settings editor.

## Name details

Hovering over a declared symbol, viewing it in the completion list, or browsing
**Outline** shows its `long_name` and literal TeX label (such as `$\beta$`)
from the model file. Toggle them independently with [Dynare › Name Details:
Long Name](settings:dynare.nameDetails.longName) and [Dynare › Name Details:
Tex](settings:dynare.nameDetails.tex).

## Block backgrounds

Dygnosis shades recognized Dynare blocks while you edit. Toggle block shading
with [Dynare › Block Tint: Enabled](settings:dynare.blockTint.enabled), or run
**Dygnosis: Open Settings** and filter by `blockTint` under **Dygnosis:
Appearance** to set individual block types to **Off**, **Subtle** or
**Model-strength**.

Use [Dynare › Block Tint: Heterogeneous
Models](settings:dynare.blockTint.heterogeneousModels) to override the shading
style for specific heterogeneity dimensions (for example `{"households":
"off"}`).

## Expression value hints

Top-level parameter expressions such as `beta = 1 / 1.04;` show an inline value
hint (`= 0.961538...`) when Dygnosis can evaluate the expression statically
(plain numbers such as `beta = 0.99;` omit the hint).

Toggle these hints with [Dynare: Parameter Value
Hints](settings:dynare.parameterValueHints). VS Code's **Editor › Inlay Hints:
Enabled** (`editor.inlayHints.enabled`) must also allow inlay hints.

## Custom colors

Semantic highlighting distinguishes endogenous variables, exogenous variables,
parameters and model-local variables, as well as declared dimensions, trends,
external functions and named models. It follows your active color theme and VS
Code's `editor.semanticHighlighting.enabled` setting. Native MATLAB lines and
`verbatim` blocks keep their own highlighting.

To customize symbol colors, run **Dygnosis: Edit in settings.json** and add
rules under `editor.semanticTokenColorCustomizations`:

```json
{
  "editor.semanticTokenColorCustomizations": {
    "rules": {
      "dynareParameter:dynare": "#D19A66"
    }
  }
}
```

| Role | Semantic token selector |
|---|---|
| Endogenous variable | `dynareEndogenous:dynare` |
| Exogenous variable (including `varexo_det`) | `dynareExogenous:dynare` |
| Parameter | `dynareParameter:dynare` |
| Model-local variable | `dynareModelLocal:dynare` |
| Forward-looking endogenous variable | `dynareEndogenous.forwardLooking:dynare` |
| Predetermined endogenous variable | `dynareEndogenous.predetermined:dynare` |

To customize background shading and comparison accent colors, add keys under
`workbench.colorCustomizations` via **Dygnosis: Edit in settings.json**:

| Theme color key | Used for |
|---|---|
| `dynare.blockTint.modelBackground` | Whole-line background for **Model-strength** blocks. |
| `dynare.blockTint.subtleBackground` | Whole-line background for **Subtle** blocks. |
| `dynare.effectiveModel.macroBackground` | Background highlight for macro-expanded ranges in the macro expansion preview. |
| `dynare.diff.changedForeground` | Accent color for paired **Replaced** rows in **Dygnosis Changes**. |
