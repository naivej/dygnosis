# Understand the model and files

## Model roots and includes

A **model root** is the main `.mod` or `.dyn` file. An included file uses the
context of the root that includes it, called its **model owner**.
**Choose model owner** selects the context of an include shared by several
known models. If no owner is known, open the root first.
**Treat this file as a model root** makes an included `.mod` or `.dyn` its own
root again.

Related files include recognized include, parameter, data and external-function
files. Resolved links open those files; unresolved links remain visible.
[Additional include directories](settings:dynare.searchPaths) extend the model
root's file lookup. Relative paths use its workspace folder, or its directory
for a loose saved file.

## Model structure and counts

**Dynare model** in Explorer shows aggregate counts, variable timing,
heterogeneity dimensions and related files. Its [section settings](settings:dynare.modelView.sections)
choose the visible sections and their order. Each heterogeneity dimension has
separate counts.

The [model status bar](settings:dynare.statusBar.enabled) shows aggregate
endogenous, exogenous and equation counts for the active `.mod` or `.dyn` file.
Its [count preferences](settings:dynare.statusBar.counts) choose which counts
appear and in what order. These presentation choices apply to the displayed file.
Incomplete expansion withholds counts and equation navigation and shows the
reason.

## Equations

| Action | Available result |
|---|---|
| Go to equation N | An equation by number, within the aggregate model or a selected dimension. |
| Jump to named equation | A searchable list of equations, names, scopes and source files. |
| Jump to equation / Jump to N equations | A CodeLens above a model block that lists its counted equations. The [equation action](settings:dynare.codeLens.modelEquations) can be hidden. |
| Find references | An optional [CodeLens at declarations](settings:dynare.codeLens.declarationReferences). |
| Expand macros | An optional [CodeLens for the macro preview](settings:dynare.codeLens.effectiveModel). |
| Refresh model view | Updated model information. |

Native `editor.codeLens` also controls CodeLens visibility.

Equation numbers are assigned before Dynare's equation transformations and can
differ from numbers in MATLAB or Octave results. Model locals, static-only
replacement rows and removed equations have no counted number. Repeated macro
occurrences can have distinct numbers but the same written source.

## Timing classes

| Class | Equation use |
|---|---|
| Static | No lead or lag. |
| Predetermined | Lags, no leads. |
| Forward-looking | Leads, no lags. |
| Mixed | Both leads and lags. |

Aggregate timing uses Dynare's end-of-period convention. For a valid
`predetermined_variables k;` declaration, written `k(+1)` is current `k` for
timing analysis, and written `k` is `k(-1)`. Heterogeneous equations keep their
written offsets. A name with no equation use also has no lead or lag. These
classes do not establish model stability.
