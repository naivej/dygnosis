# Understand the model and files

## Model roots and includes

A **model root** is the main `.mod` or `.dyn` file. An included file uses the
context of the root that includes it, called its **model owner**.
**Choose model owner** in the Command Palette selects the context of an
include shared by several known models. If no owner is known, open the root first.
**Treat this file as a model root** makes an included `.mod` or `.dyn` its own
root again.

Related files include recognized include, parameter, data and external-function
files. Resolved links open those files; unresolved links remain visible.
[Additional include directories](settings:dynare.searchPaths) extend the model
root's file lookup. Relative paths use its workspace folder, or its directory
for a loose saved file.

## Model structure and counts

The [model status bar](settings:dynare.statusBar.enabled) shows aggregate
endogenous, exogenous and equation counts for the active `.mod` or `.dyn` file.
Click it to focus Outline. Hover shows an aggregate count block followed by
one count block for each heterogeneity dimension. Each endogenous line includes
static, predetermined, forward-looking and mixed counts, including zero.
The accessibility label states the same counts.

Its [count preferences](settings:dynare.statusBar.counts) choose which count
lines appear and their order in every block and on the status bar. Hiding
endogenous also hides its class counts. These choices apply to the displayed
file. Outline shows model structure and heterogeneity dimensions; variable
hover and declaration detail show timing. Parameter counts remain available
to agents through `dynare_model_info`.

Incomplete expansion withholds counts and equation navigation and shows the
reason.

## Equations

| Action | Available result |
|---|---|
| Jump to equation... in the Command Palette | One searchable list of every counted equation in the active model, including unnamed equations. |
| Jump to equation... above a model block | One searchable list of that opener's counted equations across every macro occurrence and scope. The [equation action](settings:dynare.codeLens.modelEquations) can be hidden. |
| Find references | An optional [CodeLens at declarations](settings:dynare.codeLens.declarationReferences). |
| Expand macros | An optional [CodeLens for the macro preview](settings:dynare.codeLens.effectiveModel). |

Each equation row shows its number and name, scope, source file, macro origin
and equation text. Type any of those fields to filter the list. Choose a row
to open its verified written source; cancel to keep the cursor where it is.
Even a list with one equation waits for your choice. An empty model or opener
explains that it has no counted equations.

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

Variable hover shows the timing class. For a valid `predetermined_variables k;`
declaration, it adds the convention:

Timing: **predetermined**, end-of-period convention

The class still comes from the equations, so it can be static, forward-looking
or mixed. Written `k(+1)` is current `k` for timing analysis, and written `k` is `k(-1)`.
