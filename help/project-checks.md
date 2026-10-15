# Check project models

Opening a folder in VS Code automatically checks its unopened saved `.mod` and
`.dyn` root models in the background (skipping generated `+` directories) and
lists their diagnostics in **Problems**. Toggle background checking in [Dynare:
Project Diagnostics](settings:dynare.projectDiagnostics) while keeping live
checks for open files.

Click **`Dynare project: ...`** on the status bar (for example, **`Dynare
project: 3/3 checked`**) or run **Dygnosis: Show project checks** from the
Command Palette to focus **Dynare project checks** in Explorer:

- **Model rows** (`.mod`, `.dyn`) show their check state—such as `Checked · 0 Errors, 0 Warnings`, `Checking`, `Pending`, `Incomplete`, `Failed` or `Excluded`—and open the model when clicked.
- **Include rows** (`.inc`) are labeled **Include file** and open the file when clicked. Their diagnostics appear in **Problems** when the file is open or included by a checked model. Folders without root models display **No root .mod or .dyn models**.
- **View toolbar buttons**: Use the icons in the **Dynare project checks** title bar (or the Command Palette) for **Recheck project** (`$(refresh)`), **Cancel project checks** (`$(debug-stop)`), **Configure project exclusions** (`$(settings-gear)`) and **Open Help** (`$(question)`).

Note that **`3/3 checked`** on the status bar means background analysis
finished for all three models; those models may still report Errors or Warnings
in **Problems**.

## Scope and exclusions

To exclude a folder from background checks, hover over a workspace folder or
subfolder row in **Dynare project checks** and click **Ignore this folder**
(`$(exclude)`). This adds `<folder path>/**` (or `**` for a workspace root) to
[Dynare: Project Exclude Paths](settings:dynare.projectExcludePaths).

You can also click **Configure project exclusions** (`$(settings-gear)`) in the
view title bar to **Add exclusion…**, **Remove exclusion…**, **Reset folder
exclusions** or **Open project settings**. Patterns are relative to the
workspace folder: `*` and `?` match within a folder or file name, `**` matches
across subdirectories, and a trailing `/` matches everything inside that
folder.

Exclusions only skip background root discovery under those paths. Saved `.inc`
files remain listed in the tree, and excluded `.mod` or `.dyn` files are still
checked whenever you open them in the editor or include them from another
checked model.
