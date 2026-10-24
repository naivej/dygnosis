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
Enabled](settings:dynare.blockTint.enabled) and adjust individual block types in
Settings. For [heterogeneous-agent models](https://www.dynare.org/manual/the-model-file.html#heterogeneity)
(`heterogeneity=`), use [Dynare › Block Tint: Heterogeneous
Models](settings:dynare.blockTint.heterogeneousModels) to set shading per
`heterogeneity_dimension` (for example `{"households": "off"}`).

## Expression value hints

Top-level parameter expressions such as `beta = 1 / 1.04;` show an inline value
hint when Dygnosis can evaluate them statically. Toggle hints with [Dynare:
Parameter Value Hints](settings:dynare.parameterValueHints) (requires VS Code's
`editor.inlayHints.enabled`).

## Custom colors

Semantic highlighting distinguishes Dynare symbol roles and follows your theme
plus `editor.semanticHighlighting.enabled`. Native MATLAB lines and `verbatim`
blocks keep their own highlighting.

Run **Dygnosis: Edit in settings.json** to customize colors:

```json
{
  "editor.semanticTokenColorCustomizations": {
    "rules": {
      "dynareParameter:dynare": "#D19A66"
    }
  }
}
```

- **Semantic token selectors** (`editor.semanticTokenColorCustomizations`):
  `dynareEndogenous:dynare`, `dynareExogenous:dynare`,
  `dynareParameter:dynare`, `dynareModelLocal:dynare`,
  `dynareEndogenous.forwardLooking:dynare` and
  `dynareEndogenous.predetermined:dynare`.
- **Workbench color keys** (`workbench.colorCustomizations`):
  `dynare.blockTint.modelBackground`, `dynare.blockTint.subtleBackground`,
  `dynare.effectiveModel.macroBackground` and `dynare.diff.changedForeground`.
