# Check the model

Dygnosis underlines issues in open `.mod`, `.dyn` and `.inc` files and lists
them in VS Code's **Problems** panel (**View > Problems**). [Workspace
checks](workspace-checks.md) add issues from unopened saved models in your
workspace.

## Find a problem

Click an entry in **Problems** to jump to its file and line, or hover over an
underlined token in the editor to read the message. Press `Alt+F8` to step
through problems in the active file with details expanded, or `F8` to move
across files.

| Severity | Meaning |
|---|---|
| Error | A source problem that causes the official Dynare preprocessor to refuse the model. |
| Warning | A likely modeling mistake or inconsistency that Dynare accepts with a warning or without stopping. |
| Information | A style or completeness note, such as missing equation `name` tags (`I208`), missing `long_name` labels (`I209`) or incomplete macro expansion (`I211`). |

Each diagnostic has a code such as `W031`. Search for the code in Help or
browse the [diagnostic code list](reference.md). A clean **Problems** panel shows
no visible issues. Hidden checks and unchecked models can still have problems.
Run official Dynare in MATLAB or Octave to verify steady states and solver
conditions.

## Follow a related location

When a diagnostic references another line—such as a duplicate declaration
([W031](check:W031))—expand the item in **Problems** (or press `Alt+F8`) and
click the linked location to jump to the original declaration:

```dynare
var y;
var y;
```

## Actions

Place the cursor on an underlined diagnostic and press `Ctrl+.` (or click the
lightbulb) to open **Quick Fix...**.

| Action | Result |
|---|---|
| Repair action | Applies an available source fix for the selected diagnostic. |
| **Ignore this check (`<code>`)** | Hides all diagnostics with that code in the current window ([Dynare › Diagnostic Actions: Ignore](settings:dynare.diagnosticActions.ignore)). |
| **Dygnosis: Show a hidden check** / **Dygnosis: Show all hidden checks** | Restores one or all hidden check codes from the Command Palette. |

Hidden checks apply only to the current window and can also be restored by
clicking the **`hidden check`** count in the status bar.
