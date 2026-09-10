# Check fixtures

Small original `.mod` files that prove a diagnostic fires or stays quiet.

A complete working model is **not** here. Use `.agents/skills/dynare-copilot/references/` (`model-archive/` then `examples/`) for that. Trees and how tests assert inventory: [`dev_logs/0.1/0.1.0/masterplan.md`](../../../dev_logs/0.1/0.1.0/masterplan.md) (Fixtures).

## Which folder

Folders follow the diagnostic code. File names start with the same code (`w070_betta.mod`). The first line is `// inventory: …` and lists every trigger that file covers. To find a code, search [`inventory.json`](inventory.json) and open the `fixture` path.

| Folder | Open this when you want |
|--------|--------------------------|
| `e001/` | a file that does not parse (missing `end;`, missing `;`, a keyword typo). Auto-fix uses these too. |
| `e010/` | too many or too few equations vs variables |
| `e020/` | a name used in the model that was never declared |
| `e030/` | the same name declared twice (for example `var` and `varexo`) |
| `shape/` | equation or block shape: duplicate equations, missing `initval`, no steady-state block (`E050`–`E053`, `W042`, `W050`–`W053`, `I050`) |
| `e060/` | `@#include` and `@#if` / `@#for` mistakes. Some cases are a **directory** of files (`cycle/`, `missing/`, `nested/`, `w061_*`), not one `.mod`. |
| `w010/` | unused or never-assigned parameters and variables |
| `w070/` | a parameter value that looks wrong (discount, depreciation, …) |
| `w090/` | estimation and observed variables |
| `w100/` | Ramsey, OSR, planner objective |
| `w110/` | shock standard errors and correlations |
| `w120/` | determinacy, leads/lags, `Inf` / `NaN` |
| `w130/` | steady-state assignment order, `model(linear)`, `bytecode` / `simul` options |
| `format/` | ugly spacing / macros / `verbatim`. Pair is `name.mod` plus `name.formatted.mod`. No `// inventory:` line. |

`e001/` files stop later checks (`analyze()` stops at a parse error). Do not use them to prove a W* warning.

## Inventory

[`inventory.json`](inventory.json) is the list of triggers. One row per trigger, not one row per code letter. Every `known_codes()` key has a row. Out codes (`E040`, `W040`, `W041`, `I041`, `W071`, `I070`, `I071`, `W080`, `W081`, `DYNR`) have none.

| Field | Meaning |
|-------|---------|
| `id` | Stable trigger id |
| `codes` | Codes for this trigger (`[]` only for harness rows that are not a diagnostic) |
| `kind` | `fire` / `quiet` / `archive` / `harness` / `documented-only` |
| `package` | Family folder, or `other` for transports that stay on archives |
| `fixture` | Path from repo root. Required for `fire` / `quiet` / `archive`. A directory is allowed for include graphs. |
| `source` | Test (`file.rs::fn_name`) |
| `cluster` | File or directory under this folder. Several rows may share one. |
| `notes` | What the file must do |

Coverage: `cargo test --test check_inventory`.

## Adding a file

Do not copy an archive here. Do not paste one and tweak it. Write a short original model (about 30–80 lines). Do not change `src/` analysis code to make a fixture pass; if a fixture uncovers a real mismatch, open a different log.

- LF only (`\n`). Tests still strip `\r\n` → `\n` after `read`.
- First line: `// inventory: id[, id…]`.
- Name the file after the cluster (`w140_ops.mod`), not after an archive.
- Put related **fire** triggers in one file when they still fire together. Keep a separate file for a quiet/control, when two triggers cannot coexist (`model(linear)` vs `model;`), or for a multi-file include graph (the inventory `fixture` is that directory).
- `I050` on `sims_wu_2019` / `lk2024` is `archive`. `govt_rbc_irf_matching` already has E001 (`method_of_moments`); reuse that archive; do not add a W130 mutation of it.

Add a row, write or reuse a `.mod`, load it from the family test. Assert codes, messages, and spans. Do not rebuild the case with in-memory string edits.

Format cases go in `format/` as an input `.mod` and, when the formatter must change the text, a sibling `.formatted.mod`. Empty and whitespace-only inputs are asserted in the test, not as files.

