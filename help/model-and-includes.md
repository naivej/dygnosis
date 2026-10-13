# Understand the model and files

## Model roots and includes

A **model root** is the main `.mod` or `.dyn` file. An included file uses the
context of the root that includes it, called its **model owner**.
Open a root to make its include context available.
For example, if `main.mod` includes `equations.inc`, open `main.mod` before
working in `equations.inc` so its checks can use the declarations in `main.mod`.
With an include active, run **Dygnosis: Choose model owner** from the Command
Palette and select the main model from the list.
**Dygnosis: Treat this file as a model root** makes an included `.mod` or `.dyn`
its own main model again.

Related files include recognized include, parameter, data and external-function
files. Hover over an include filename or a recognized file reference in the
editor. If the file is found, VS Code shows a link that opens it; use the
modifier key shown in the hover to follow the link.
[Additional include directories](settings:dynare.searchPaths) extend the model
root's file lookup. Relative paths use its workspace folder, or its directory
for a loose saved file.

## Model structure and counts

The [model counts](settings:dynare.statusBar.enabled) appear as numbers beside
variable and equation icons on the left of the status bar for the active `.mod`
or `.dyn` file. Hover over those numbers to see counts under **Aggregate model
before transformation** and each dimension name, such as **Heterogeneous model:
households**. Click the numbers to open **Outline**.
Each endogenous line in the hover includes
static, predetermined, forward-looking and mixed counts, including zero.
Screen readers announce the same counts.

The [count setting](settings:dynare.statusBar.counts) chooses which counts
appear and their order in the hover and on the status bar. Hiding
endogenous also hides its class counts. These choices apply to the displayed
file. Outline shows model structure and heterogeneity dimensions; variable
hover and declaration detail show timing.

## Equations

| Action | Available result |
|---|---|
| Dygnosis: Jump to equation... in the Command Palette | Opens a searchable list of every counted equation in the active model, including unnamed equations. |
| Click Jump to equation... above a model block | Opens a list of that block's counted equations, including equations repeated by macros. The [equation action](settings:dynare.codeLens.modelEquations) can be hidden. |
| Click Find references above a declaration | Opens the references list for that name. If the declaration lists several names, choose one first. The [declaration action](settings:dynare.codeLens.declarationReferences) is optional. |
| Click Expand macros above the first model block | Opens the macro preview. The [preview action](settings:dynare.codeLens.effectiveModel) is optional. |

Each equation row shows its number and name, **Aggregate** or a dimension name,
filename and line, and equation text. A macro loop can also show a value such as
`j=1`. Type a number, name or part of the equation to filter the list. Choose a
row to open the line in your model file.

These clickable labels above code are called **CodeLens** in VS Code. To show
them, enable **Editor: Code Lens** in Settings and the relevant Dygnosis setting.

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
