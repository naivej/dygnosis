# Changelog

## v0.1.1

Audit against the two principles: the preprocessor is ground truth for the `.mod` language, and a diagnostic is added only if it earns a place.

Honesty tests lock that agreement with the latest stable preprocessor (skip if it is missing). After a preprocessor run, the safety gate applies: on refuse, same-ground Errors yield to its wording; on accept, no Error of ours remains; when they warn, same-ground Warnings yield to theirs; extra Warnings stay.

MCP is the nine tools, in this order: `dynare_diagnose`, `dynare_model_info`, `dynare_compare_models`, `dynare_find_references`, `dynare_rename`, `dynare_auto_fix`, `dynare_explain`, `dynare_list_diagnostic_codes`, `dynare_list_options`. `dynare_diagnose` is the only full check. These names are gone (not aliases): `dynare_diagnose_workspace`, `dynare_parse_summary`, `dynare_find_references_workspace`, `dynare_rename_workspace`, `dynare_run_preprocessor`.

Diagnostic codes renamed so the letter matches severity (old → new):

| Old | New |
|-----|-----|
| `E010` | `W013` |
| `E050` | `W054` |
| `E051` | `W055` |
| `E052` | `W056` |
| `E053` | `W057` |
| `W021` | `E021` |
| `W050` | `E058` |
| `W053` | `E059` |
| `W090` | `E090` |
| `W093` | `E093` |
| `W095` | `E095` |
| `W100` | `E100` |
| `W101` | `E101` |
| `W103` | `E103` |
| `W111` | `E111` |
| `W130` | `E130` |

## v0.1.0

This release of dygnosis is a Rust rewrite of the thin analysis core from [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) (Python).

### Behaviour vs the Python origin

We aimed to keep the useful diagnostic **codes and messages**, not a line-for-line port of Python internals. Notable differences:

- **Native analysis** — identifiers and spans come from a real expression tree, not regex over masked text. Spans usually underline the bad token, not the whole equation or block.
- **Macros** — inactive `@#if` branches are dropped after expand; `@#for` unrolls fully (Python sometimes kept only the first list value or left dead text in slices).
- **Fewer false alarms** — e.g. call names are not treated as undeclared variables; `1/0` is non-finite (W122), not a “missing value” warning; parameter-range checks look at top-level assignments only, not values inside `steady_state_model`.
- **Where Python was wrong, Rust ships the fix** — e.g. I050 is “no `initval` / `steady_state_model` block” (no fake “compute steady state” solver path); some E001/auto-fix cases report only the missing `;`.
- **One wording across transports** — LSP and MCP no longer disagree on the same warning text where Python did.

### What we left out (numerical / Dynare compute)

These stay with Dynare, not this binary:

- Numerical steady-state solve and “compute steady state” / auto-solve on edit
- Blanchard–Kahn / eigenvalue checks
- Identification and static Jacobian singularity
- Per-equation numerical residuals
- Running a full Dynare/MATLAB session from the language server
- MCP/LSP surfaces that only wrapped those numerics
- Structural block partitioning that needed numeric graph tools (Dulmage–Mendelsohn)