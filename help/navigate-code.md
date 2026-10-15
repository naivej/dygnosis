# Follow equations and names

Navigation uses the current model and its known includes.

Place the cursor on a name, then right-click to find **Go to Definition**,
**Find All References** and **Rename symbol**. You can also press F1 and search
for a command's title. **Go to Definition** opens the declaration in an editor;
**Peek Definition** shows it in a small view inside the current editor.

| Action | What you see |
|---|---|
| Go to Definition / Peek Definition | The source declaration or model-local definition. |
| Go to Declaration / Go to Type Definition | Opens the name's declaration. |
| Find All References | A list of the name's uses across the model and its includes. Click a result to open its line. |
| Click a declared name | Its occurrences are highlighted in the current file. |
| **Rename symbol** | Rename a symbol across the model and its known includes. Press F2 or choose **Rename symbol** from the editor context menu. |
| Show Call Hierarchy | Equations that use a variable. |
| Go to Symbol in Editor... | A searchable list of declarations, equations and other entries in the current file. |
| Go to Symbol in Workspace... | A searchable list of symbols in models that Dygnosis has analyzed. |
| Click a folding arrow beside a line number | Collapses a complete Dynare block, macro loop or conditional branch, or a multiline declaration, command or Dynare assignment. Hover beside the line numbers to see the arrows. |

[Equation navigation](model-and-includes.md#equations) reaches equations across
the model, including repeated macro occurrences.

Rename follows model ownership across `.mod`, `.dyn` and included source
files, and supports untitled Dynare documents. Type the new name in the box
titled **Dygnosis: Rename symbol**, then press Enter. MATLAB companion text is
outside its scope.

Declared heterogeneity dimensions, trends, named subsamples, and VAR/PAC model
names can also be navigated and renamed when their full source relationship is
known. A trend's TeX label remains metadata when its name changes. Database and
external function names also have definition and reference navigation. Built-in
operators such as `EXPECTATION` and `STEADY_STATE` have call references and help;
they have no user declaration to rename.

Rename also updates literal references in an equation's `endogenous` or
`mcp` tag. Existing quotes around the tag value stay in place.
Equation `name` tags and symbol `long_name` labels remain unchanged.

## Outline

Open **Outline** in Explorer and click an entry to open its line in the editor.
The symbol trail above the editor is called **breadcrumbs**; click a name in
that trail to browse nearby entries. [Outline sections](settings:dynare.outline.sections)
and [equation number labels](settings:dynare.outline.equationNumbers) control
which entries and numbers appear.

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

Named macro definitions appear as Outline entries. Declared dimensions, trends,
external functions and named VAR/PAC model identities appear under their
declarations. Expression operators such as `diff`, `log`, `SUM`,
`EXPECTATION` and `STEADY_STATE` remain part of their expression row.

Heterogeneous `model`, `var` and `varexo` headings show their written option,
for example `model(heterogeneity=households)` and
`var(heterogeneity=households)`. An aggregate heading keeps its plain name.
A later type change keeps each declared name under its written declaration.
One written statement that expands into different dimensions shows
**multiple expansions** instead of selecting one dimension.

## Model-local variables

**Go to Definition** opens the `#name` definition. **Go to Declaration** prefers
an explicit `model_local_variable` declaration. **Find All References** and
**Rename symbol** follow that definition's uses; a local with the same spelling
in another model or
heterogeneity dimension remains separate.
