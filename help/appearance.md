# Read labels, hints and colors

Dygnosis uses metadata labels, inline value hints, block shading and semantic
colors to make models easier to read.

## Name details

`long_name` and TeX labels from the model appear in hovers, completion and
**Outline**. Toggle them with [Dynare › Name Details: Long
Name](settings:dynare.nameDetails.longName) and [Dynare › Name Details:
Tex](settings:dynare.nameDetails.tex).

## Block backgrounds

Toggle block shading with [Dynare › Block Tint:
Enabled](settings:dynare.blockTint.enabled). Run **Dygnosis: Open Settings** and
filter by `blockTint` under **Dygnosis: Appearance** to set each block type to
**Off**, **Subtle** or **Model-strength**. Per-dimension overrides:
[Dynare › Block Tint: Heterogeneous
Models](settings:dynare.blockTint.heterogeneousModels).

## Expression value hints

Top-level parameter expressions such as `beta = 1 / 1.04;` show an inline value
hint (`= 0.961538...`) when Dygnosis can evaluate the expression statically
(plain numbers such as `beta = 0.99;` omit the hint).

Toggle these hints with [Dynare: Parameter Value
Hints](settings:dynare.parameterValueHints). VS Code's **Editor › Inlay Hints:
Enabled** (`editor.inlayHints.enabled`) must also allow inlay hints.

## Custom colors

Semantic highlighting follows your theme and `editor.semanticHighlighting.enabled`.
Run **Dygnosis: Edit in settings.json** to customize symbol colors under
`editor.semanticTokenColorCustomizations`:

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
