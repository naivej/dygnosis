# Check the model

Dygnosis reports checks in the editor and Problems panel for open models and
their includes. [Project checks](project-checks.md) also cover unopened saved
models. Related locations link to declarations or include sites that explain
a problem.

| Severity | Meaning |
|---|---|
| Error | An identified source problem that would make the official Dynare preprocessor refuse the model. |
| Warning | A potential problem that does not itself cause that refusal. |
| Information | A point to review, such as missing equation names or incomplete analysis. |

The [check reference](reference.md) gives each code's explanation.
A clean Problems panel does not establish that Dynare can accept or solve the
model; see [About Dygnosis](about.md).

## Actions

| Action | Result |
|---|---|
| Quick Fix | An available repair for the selected check. |
| Explain this check / Explain a check | The explanation for a diagnostic code. Its [Quick Fix entry](settings:dynare.diagnosticActions.explain) is optional; the command remains available. |
| Ignore this check | Hides every diagnostic with that code in this window, including its attached fixes. Its [Quick Fix entry](settings:dynare.diagnosticActions.ignore) is optional. |
| Show a hidden check / Show all hidden checks | Restores one hidden code or all hidden codes. |

Hiding affects only this window's display. It does not change model files or
agent results. The hidden-code status item shows how many codes are hidden.
The list clears when the window reloads or workspace folders change; restarting
the language server keeps it.
