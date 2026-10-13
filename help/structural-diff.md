# Review model changes

With your model active, run **Dygnosis: Show mod changes** from the Command
Palette, then choose one of the entries below. Your open model appears under
**After**; the model you select appears under **Before**.
**Dygnosis: Show mod changes with .mod file…** opens the file picker directly.

| Baseline | Comparison input |
|---|---|
| With previous revision | HEAD for a Working model; the first parent for an open historical model. |
| With revision… | A local commit or revision. |
| With branch or tag… | The fixed commit of a local branch, remote-tracking branch or tag. |
| With .mod file… | Another current `.mod` or `.dyn` model. |

The **Working** label means saved files together with open unsaved edits.
Models from Git history use their main file and active includes from the same
fixed commit. The Source Control
action compares the Working model even when invoked from the staged list.

## Available views and actions

The comparison tab shows added, removed and replaced model content side by side:
names and declarations, parameter values, equations and model locals, shocks,
state assignments, priors, data, commands, forecasts, moments, policy and other
supported model settings. Aggregate and heterogeneous equations appear in
Equations. Direct equation references show where changed names are used.

| Action or control | Result |
|---|---|
| Click a group on the left, such as Equations or Commands | Shows that group's changes. |
| Type in Find changes | Filters the rows by the text you enter. |
| Open Type of changes and select checkboxes | Shows Added, Removed or Replaced rows. [Default change types](settings:dynare.diff.defaultChangeKinds) sets the initial selection for a new comparison. |
| Click a filename and line number in a Before or After row | Opens that side's model file at the displayed line. When a row is absent on one side, the link can lead to the start of the corresponding declaration, command or block. Files from Git history open read-only. |
| Text diff / Text diff… | Opens VS Code's text comparison for the main model or lets you choose a changed main model or include. It includes formatting and comment changes. |
| Change comparison… | Another baseline for the model that opened the view. |
| Swap sides | Exchanges Before and After. |
| Refresh | Updates the comparison with current Working text. Selected historical commits stay fixed. |

## Comparison scope

A model comparison describes source facts, not numerical or algebraic
equivalence. Uncertain equation matches appear as separate Removed and Added
rows. Active filters affect the rows shown.

Text diff covers the main model and the included files used in that comparison.
It does not include
inactive child files, external MATLAB functions or data-file contents.

Git comparisons read local history without a fetch or checkout. They use the
launching model's current [include directories](settings:dynare.searchPaths),
not historical VS Code settings. Historical inputs use files stored directly
in that repository. A model inside a checked-out submodule uses that
submodule's repository.
