# Follow equations and names

Navigation uses the current model and its known includes.

| Feature | What it offers |
|---|---|
| Go to Definition / Peek Definition | The source declaration or model-local definition. |
| Go to Declaration / Go to Type Definition | The available declaration target. |
| Find All References | Known occurrences across the model and its includes. |
| Document highlights | Occurrences in the current file. |
| Rename Symbol | Renaming across known source locations. |
| Show Call Hierarchy | Equations that use a variable. |
| Outline, breadcrumbs and Go to Symbol in Editor | Structure in the current written file, with configurable [sections](settings:dynare.outline.sections) and [equation number labels](settings:dynare.outline.equationNumbers). |
| Go to Symbol in Workspace | Symbols known to the language server. |
| Folding | Complete Dynare blocks and written macro loops and conditionals. Multiline declarations, commands and Dynare assignments fold by statement. |

[Equation navigation](model-and-includes.md#equations) reaches equations across
the model, including repeated macro occurrences. Rename is unavailable where
source edits are ambiguous or the new name conflicts with an existing name.
It does not infer aliases in MATLAB code.

## Outline

Two or more consecutive top-level Dynare assignments and MATLAB statements
shown in Outline form a collapsible **MATLAB expressions** group. Parameter
assignments stay named children, for example `rho = 0.9`. A single statement
stays a separate row. Declarations, Dynare commands and blocks start a new
group, even when their Outline section is hidden. Comments and blank lines
do not split a group. The [commands section](settings:dynare.outline.sections)
controls these groups.

Model equations and assignments inside blocks keep their current places.
Each Outline row opens its written source; repeated macro copies do not add
duplicate rows for the same statement.

Heterogeneous `model`, `var` and `varexo` headings show their written option,
for example `model(heterogeneity=households)` and
`var(heterogeneity=households)`. An aggregate heading keeps its plain name.
A later type change keeps each declared name under its written declaration.
One written statement that expands into different dimensions shows
**multiple expansions** instead of selecting one dimension.

Equation rows keep their numbers before transformation. Complete model
headings have no extra detail. When current source structure is incomplete
or has no single model owner, equation rows omit counted numbers. Any model
heading in that structure shows **recovered written structure**.

## Model-local variables

**Go to Definition** opens the `#name` definition. **Go to Declaration** prefers
an explicit `model_local_variable` declaration. References and rename follow
that definition's uses; a local with the same spelling in another model or
heterogeneity dimension remains separate. A declaration without a definition
has no definition target.
