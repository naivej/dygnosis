# Changelog

## v0.1.1

Audit of v0.1.0 against two principles:

- A check is built in instead of delegated to the Dynare preprocessor if (1) it is new; (2) it powers an editor feature; or (3) it helps while typing. The audit finds no check to drop.
- Where checks exist in both dygnosis and the preprocessor, the latter is ground truth. Severity must match (Error or Warning in both), and parity is pinned by honesty tests.

Other changes:

- A check that exists only in dygnosis can only be a Warning or Information. A Warning means the preprocessor would accept the file, but something looks wrong.
- MCP tools are simplified to nine.

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