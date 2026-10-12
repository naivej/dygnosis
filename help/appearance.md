# Read labels, hints and colors

Dygnosis provides name colors, block backgrounds, written name details and
expression value hints. Presentation settings apply to the displayed file and
can differ by workspace folder.

## Name details

Hover, completion and Outline can show written `long_name` and TeX labels.
[Long names](settings:dynare.nameDetails.longName) and
[TeX names](settings:dynare.nameDetails.tex) can be hidden independently.
TeX remains literal text.

## Block backgrounds

Block backgrounds distinguish recognized Dynare blocks and update while typing.
[Block appearance](settings:dynare.blockTint.enabled) provides a main switch
and per-block Off, Subtle or Model-strength styles.
[Dimension overrides](settings:dynare.blockTint.heterogeneousModels) give named
heterogeneity dimensions their own styles. A background is not a model check.

Colors follow the current theme. For custom colors, the keys
`dynare.blockTint.modelBackground` and `dynare.blockTint.subtleBackground`
belong to `workbench.colorCustomizations`; their high-contrast defaults are
transparent.

## Expression value hints

An assignment such as `beta = 1 / 1.04;` can show its known value. Plain numbers
need no hint. Unknown inputs or MATLAB execution (except simple arithmetic) can make a value unavailable.

[Value hints](settings:dynare.parameterValueHints) controls these annotations,
together with native `editor.inlayHints.enabled`.

## Custom colors

Name colors distinguish variable roles when enabled by the theme and native
`editor.semanticHighlighting.enabled`. These selectors are available for theme
customization. No separate timing style is applied by default.

| Role | Semantic token selector |
|---|---|
| Endogenous variable | `dynareEndogenous:dynare` |
| Exogenous variable, including deterministic exogenous | `dynareExogenous:dynare` |
| Parameter | `dynareParameter:dynare` |
| Model-local variable | `dynareModelLocal:dynare` |
| Forward-looking endogenous variable | `dynareEndogenous.forwardLooking:dynare` |
| Predetermined endogenous variable | `dynareEndogenous.predetermined:dynare` |

Mixed timing can carry both timing modifiers. Themes can display several roles
alike.
