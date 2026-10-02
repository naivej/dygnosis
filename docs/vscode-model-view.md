# Model view and equation navigation

Open **Dynare model** in VS Code's Explorer to inspect the selected model's
aggregate counts and timing, each heterogeneity dimension, and related files.
All facts come from the Dygnosis engine. Aggregate counts and dimension counts
stay separate. Expand a timing class to see its variables.

Use the view's native title menu to hide or move it. Use
**Dygnosis: Open Settings** and search for `modelView.sections` to choose its
content. The resource-scoped setting defaults to:

```json
"dynare.modelView.sections": ["counts", "timing", "dimensions", "relatedFiles"]
```

The list order sets the section order. An empty list clears the content.
Preferences apply to the displayed file, so workspace-folder settings can
differ even when two files belong to the same model. Reset the setting through
the Settings editor to restore the defaults.

**Dygnosis: Go to equation N** asks for a positive whole equation number. When
the model contains several equation scopes, choose Aggregate or a heterogeneity
dimension first. **Dygnosis: Jump to named equation** searches names, equation
numbers, scopes, written files, and macro occurrences. Both commands appear in
the Command Palette and the model view's toolbar.

Equation numbers describe surviving counted equations before transformation.
Static-only rows and model locals have no counted number. Dynare's later
transformation can change numbering for MATLAB or Octave. Navigation opens the
engine's verified written source, including equations in included files. A
repeated macro equation has a separate picker entry for each expansion even
when the expansions share a written location. If a location is unavailable,
the command explains that and leaves the editor where it is.

Click a resolved related file to open it with the selected model's owner
context. While focusing an include, the view continues to show that model.
If an include has several known owners, use **Dygnosis: Choose model owner**
to select one explicitly. Open the owner model first when no owner is known.
A `.mod` or `.dyn` opened as an include keeps that owner until you use
**Dygnosis: Treat this file as a model root**.

The view clears outdated rows while refreshing. Incomplete expansion displays
an explanation and withholds counts and equation navigation. Picker actions
recheck the input revision and engine instance after the picker and after
loading the source file. A change of model, active file, owner, or source version
during the jump keeps the editor where it is. If an older engine cannot supply model information, use
**Dygnosis: Restart language server**, inspect **Dygnosis: Show Output**, or
clear `dynare.serverPath` to return to the bundled binary.

Outline, breadcrumbs, quick outline, definition, references, and call hierarchy
use VS Code's native LSP features. Outline shows the current written file's
structure; equation navigation reaches mapped locations across the whole model.
To see equations using a variable, use the native call hierarchy action.
