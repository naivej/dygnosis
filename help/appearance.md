# Read labels, hints and colors

Dygnosis uses metadata labels, inline value hints, block shading and semantic
colors to make models easier to read. Click any setting link below to open it
in VS Code's Settings editor.

## Name details

Hover, completion and **Outline** can show a symbol's `long_name` and literal
TeX label (such as `$\beta$`). Toggle them with [Dynare › Name Details: Long
Name](settings:dynare.nameDetails.longName) and [Dynare › Name Details:
Tex](settings:dynare.nameDetails.tex).

## Block backgrounds

Toggle block shading with [Dynare › Block Tint:
Enabled](settings:dynare.blockTint.enabled). Under **Dygnosis: Appearance**, set
individual block types to **Off**, **Subtle** or **Model-strength**, and use
[Dynare › Block Tint: Heterogeneous
Models](settings:dynare.blockTint.heterogeneousModels) for per-dimension
overrides (for example `{"households": "off"}`).

## Expression value hints

Top-level parameter expressions such as `beta = 1 / 1.04;` can show an inline
value hint when Dygnosis evaluates them statically (plain numbers omit the
hint). Toggle with [Dynare: Parameter Value
Hints](settings:dynare.parameterValueHints); VS Code's **Editor › Inlay Hints:
Enabled** (`editor.inlayHints.enabled`) must also allow inlay hints.

## Custom colors

Semantic highlighting distinguishes Dynare symbol roles and follows your theme
plus `editor.semanticHighlighting.enabled`. Native MATLAB lines and `verbatim`
blocks keep their own highlighting.

Run **Dygnosis: Edit in settings.json** to customize. Symbol colors use
`editor.semanticTokenColorCustomizations` (for example
`dynareParameter:dynare`); block and comparison accents use
`workbench.colorCustomizations` keys such as
`dynare.blockTint.modelBackground`, `dynare.blockTint.subtleBackground`,
`dynare.effectiveModel.macroBackground` and `dynare.diff.changedForeground`.

```json
{
  "editor.semanticTokenColorCustomizations": {
    "rules": {
      "dynareParameter:dynare": "#D19A66"
    }
  }
}
```

Common semantic selectors: `dynareEndogenous:dynare`, `dynareExogenous:dynare`,
`dynareParameter:dynare`, `dynareModelLocal:dynare`, plus
`dynareEndogenous.forwardLooking:dynare` and
`dynareEndogenous.predetermined:dynare`.
