# Check the model

Dygnosis underlines issues in open `.mod`, `.dyn` and `.inc` files and lists
them in VS Code's **Problems** panel (**View > Problems**). [Workspace
checks](workspace-checks.md) add issues from unopened saved models in your
workspace.

## Find a problem

Press `Alt+F8` to step through problems in the active file with details
expanded, or `F8` to move across files.

| Severity | Meaning |
|---|---|
| Error | A source problem that causes the official Dynare preprocessor to refuse the model. |
| Warning | A likely modeling mistake or inconsistency that Dynare accepts with a warning or without stopping. |
| Information | A style or completeness note, such as missing equation `name` tags (`I208`), missing `long_name` labels (`I209`) or incomplete macro expansion (`I211`). |

Search Help for a code such as `W031` or browse the
[diagnostic code list](reference.md). Hidden checks and unchecked models
can still have problems. Verify steady states and solver conditions in
Dynare.

## Follow a related location

When a diagnostic references another line—such as a duplicate declaration
([W031](check:W031))—expand the item in **Problems** (or press `Alt+F8`) and
click the linked location to jump to the original declaration:

```dynare
var y;
var y;
```

## Actions

Press `Ctrl+.` on the underlined text to open **Quick Fix...**.

| Action | Result |
|---|---|
| Repair action | Applies an available source fix for the selected diagnostic. |
| **Ignore this check (`<code>`)** | Hides all diagnostics with that code in the current window ([Dynare › Diagnostic Actions: Ignore](settings:dynare.diagnosticActions.ignore)). |
| **Dygnosis: Show a hidden check** / **Dygnosis: Show all hidden checks** | Restores one or all hidden check codes from the Command Palette. |

The **`hidden check`** count in the status bar also restores hidden checks.
