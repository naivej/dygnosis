# Read labels, hints and colors

Dygnosis uses colors, labels and value hints to make model code easier to read.
Click a settings link below to open its control in VS Code. These settings
apply to the displayed file and can differ by workspace folder.

## Name details

Hover over a declared name, select it in the suggestion list, or look at its
entry in Outline to see its `long_name` and TeX label from the model file.
[Long names](settings:dynare.nameDetails.longName) and [TeX names](settings:dynare.nameDetails.tex) can be hidden independently.
For example, a TeX label `$\beta$` appears as text rather than a rendered formula.

## Block backgrounds

The editor can shade the lines inside recognized Dynare blocks. The shading
updates while you type. [Block tint](settings:dynare.blockTint.enabled) turns it
on or off. Run **Dygnosis: Open Settings** and search for `blockTint` to choose
**Off**, **Subtle** or **Model-strength** for individual block types.
[Dimension overrides](settings:dynare.blockTint.heterogeneousModels) give named heterogeneity dimensions their own styles.

Colors follow the current theme. To set your own background colors, run
**Dygnosis: Edit in settings.json** and add `dynare.blockTint.modelBackground`
or `dynare.blockTint.subtleBackground` under `workbench.colorCustomizations`.
Their high-contrast defaults are transparent.

## Expression value hints

An assignment such as `beta = 1 / 1.04;` can show a small value hint beside the
expression in the editor. A plain number such as `beta = 0.99;` needs no hint.
Unknown inputs or MATLAB execution, except simple arithmetic, can make a
value unavailable.

Turn [Value hints](settings:dynare.parameterValueHints) on or off to control
these annotations. VS Code's `editor.inlayHints.enabled` setting must also
permit them; search for that setting in the Settings editor.

## Custom colors

Name colors can distinguish endogenous variables, exogenous variables and
parameters. They depend on your theme and VS Code's
`editor.semanticHighlighting.enabled` setting.

Accepted PAC and VAR syntax, macro directives, model operators, and declared
dimensions, trends, external functions and named models also receive syntax or
name highlighting. Native MATLAB and `verbatim` text keep their own language
context.

To change a name color, run **Dygnosis: Edit in settings.json** and put a rule
under `editor.semanticTokenColorCustomizations`. For example:

```json
{
  "editor.semanticTokenColorCustomizations": {
    "rules": {
      "dynareParameter:dynare": "#D19A66"
    }
  }
}
```

The rule's key selects which names it applies to. Use the keys below to choose
other roles. Timing classes have no separate style by default.

| Role | Semantic token selector |
|---|---|
| Endogenous variable | `dynareEndogenous:dynare` |
| Exogenous variable, including deterministic exogenous | `dynareExogenous:dynare` |
| Parameter | `dynareParameter:dynare` |
| Model-local variable | `dynareModelLocal:dynare` |
| Forward-looking endogenous variable | `dynareEndogenous.forwardLooking:dynare` |
| Predetermined endogenous variable | `dynareEndogenous.predetermined:dynare` |

Mixed timing can carry both timing modifiers. Themes can display several roles alike.
