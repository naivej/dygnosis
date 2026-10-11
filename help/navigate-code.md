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
| Folding | Complete block and macro regions. |

[Equation navigation](model-and-includes.md#equations) reaches equations across
the model, including repeated macro occurrences. Rename is unavailable where
source edits are ambiguous or the new name conflicts with an existing name.
It does not infer aliases in MATLAB code.

## Model-local variables

**Go to Definition** opens the `#name` definition. **Go to Declaration** prefers
an explicit `model_local_variable` declaration. References and rename follow
that definition's uses; a local with the same spelling in another model or
heterogeneity dimension remains separate. A declaration without a definition
has no definition target.
