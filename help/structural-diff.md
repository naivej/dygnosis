# Review model changes

**Show mod changes** compares the open model (**After**) with a selected
baseline (**Before**). **Show mod changes with .mod file…** opens file comparison
directly.

| Baseline | Comparison input |
|---|---|
| With previous revision | HEAD for a Working model; the first parent for an open historical model. |
| With revision… | A local commit or revision. |
| With branch or tag… | The fixed commit of a local branch, remote-tracking branch or tag. |
| With .mod file… | Another current `.mod` or `.dyn` model. |

**Working** includes saved files and open unsaved edits. Historical inputs use
their root and active includes from the same fixed commit. The Source Control
action compares the Working model even when invoked from the staged list.

## Available views and actions

**Model changes** shows added, removed and replaced model facts side by side:
names and declarations, parameter values, equations and model locals, shocks,
state assignments, priors, data, commands, forecasts, moments, policy and other
supported model settings. Aggregate and heterogeneous equations appear in
Equations. Direct equation references show where changed names are used.

| Action or control | Result |
|---|---|
| Change groups, Find changes and Type of changes | Filter the model rows. [Default change types](settings:dynare.diff.defaultChangeKinds) sets the initial Added, Removed and Replaced selection for a new comparison. |
| Source links | Each side's written source. When a row is absent on one side, its link can lead to the start of the corresponding declaration, command or block. Historical source is read-only. |
| Text diff… | VS Code text comparison for captured roots and active includes, including formatting and comment changes. |
| Change comparison… | Another baseline for the model that opened the view. |
| Swap sides | Exchanges Before and After. |
| Refresh | Current Working text; selected historical commits stay fixed. |

## Comparison limits

A model comparison describes source facts, not numerical or algebraic
equivalence. Uncertain equation matches appear as separate Removed and Added
rows. A view with no model changes can still have text changes or unavailable
facts. Active filters also affect the rows shown.
Notes below the shown changes explain missing values or highlights. Use
**Text diff…** to review the file text when details could not be compared.

Text diff covers captured roots and executed includes. It does not include
inactive child files, external MATLAB functions or data-file contents.
Changed Working inputs mark the comparison **Out of date**; source actions
need a refresh.

Git comparisons read local history without a fetch or checkout. They use the
launching model's current [include directories](settings:dynare.searchPaths),
not historical VS Code settings. Historical includes outside the repository,
through symlinks or across submodules are unavailable. A model inside a
checked-out submodule uses that submodule's repository.
