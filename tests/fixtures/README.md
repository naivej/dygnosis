# Check fixtures

Small original `.mod` files that prove a diagnostic fires or stays quiet.

A complete working model is **not** here. Use `.agents/skills/dynare-copilot/references/` (`model-archive/` then `examples/`) for that. Trees and how tests assert inventory: [`dev_logs/0.1/0.1.0/masterplan.md`](../../../dev_logs/0.1/0.1.0/masterplan.md) (Fixtures).

## Which folder

Folders follow the diagnostic code. File names start with the same code (`w070_betta.mod`). The first line is `// inventory: …` and lists every trigger that file covers. To find a code, search [`inventory.json`](inventory.json) and open the `fixture` path.

| Folder | Open this when you want |
|--------|--------------------------|
| `e001/` | a file that does not parse (missing `end;`, missing `;`, a keyword typo, a declared variable named after a block opener, a non-ASCII declaration name). Auto-fix uses these too. The `opener_var_*` group is 0.6.0 06: a declared opener name is a legal symbol (quiet files), and the fire files are the shapes 7.1 refuses. `opener_var_declared_bare_row*.mod` is the `declared_before` clause on its own; `opener_var_end_no_semi.mod` is an `end` written without its `;`. `opener_var_reserved_ident.mod` is `var y end;`, a declaration 7.1 accepts at check. `non_ascii_trailing.mod` and `non_ascii_ident.mod` are the declaration characters Dynare's lexer refuses; `unicode_display_quiet.mod` keeps those characters in comments, `long_name`, TeX, and an equation name. |
| `e010/` | too many or too few equations vs variables, including Ramsey/discretionary −N |
| `e020/` | a name used in the model that was never declared |
| `e030/` | the same name declared twice (for example `var` and `varexo`) |
| `shape/` | equation or block shape: duplicate equations, missing `initval`, no steady-state block (`W054`–`W057`, `W042`, `E058`, `W051`, `W052`, `E059`, `I050`) |
| `e060/` | `@#include` and `@#if` / `@#for` mistakes. Cycle is extra Warning `W062`. Some cases are a **directory** of files (`cycle/`, `missing/`, `nested/`, `w061_*`), not one `.mod`. |
| `w010/` | unused or never-assigned parameters and variables |
| `w070/` | a parameter value that looks wrong (discount, depreciation, …) |
| `w090/` | estimation and observed variables |
| `w100/` | Ramsey, OSR, planner objective |
| `w110/` | shock standard errors and correlations |
| `w120/` | determinacy, leads/lags, `Inf` / `NaN` |
| `w130/` | steady-state assignment order, `model(linear)`, `bytecode` / `simul` options |
| `format/` | ugly spacing / macros / `verbatim`. Pair is `name.mod` plus `name.formatted.mod`. No `// inventory:` line. |
| `check_dir/` | a tree for `dygnosis check DIR` (recurse `*.mod`, skip `+` directories; explicit `.inc` still as FILE) |
| `equations/` | counted equation object (library reader + MCP duplicate `[name=]` tag) |
| `writing/` | I208–I210 writing summaries: equation name tags, declaration `long_name`, and literals in equations |
| `compare/` | aggregate equation compare pairs (names, tags, regimes) and symbol kind or metadata pairs. No `// inventory:` line. |
| `lsp/` | document-symbol outline (timing-class groups; empty group omitted) |
| `companions/` | related files next to a `.mod` (convention SS / run script, catalog `datafile` / `mode_file`, leftover quotes, ident helpers). Named unresolved fire W160; sibling `_steadystate.m` quiets I050. |
| `expand/` | library `expand_report` (effective text + origin map). Not a diagnostic. |
| `clash/` | written-clash fires (`E026`–`E028`, `E104`, `E113`, `E179`) and their quiets |
| `d_check/` | check-class fires (`E200`–`E218`, `W200`) and their quiets |
| `occbin/` | library OccBin structure (`occbin_constraints`, tag map, `⟂` / `_|_`, `shocks(surprise)`) and D-occbin fire files (`E170`–`E177`, `E180`–`E185`, `W170`). `e178_surprise.mod` is the E178 honesty fire (`periods`/`values`). |
| `mom/` | the five moment / IRF-calibration blocks and the `method_of_moments` statement. The seven structure files (0.6.0 01) are legal shapes 7.1 accepts. Fire files cover **E382**–**E392**, the handed-over **E001**, and the syntax 0.6.0 07 keeps (one shape each: a bad option word, a period or lag the grammar cannot spell, a row-keyword shape, a bad row beside a stored one, an expression slot of the wrong kind, a dotted moment row). |
| `d_pac/` | Dynare 7.2 semi-structural parse, PAC check, and written transform refusals; accepted neighbours; and a quiet control for an unmapped E193 rewrite. Each fire tests one cause at its official catching step. |
| `pin72/` | Dynare 7.2 accepted repetitions in separate blocks, removed `histval_file` option fires, and retained `initval_file` controls. |
| `lists/` | the trailing symbol lists. Fire files cover **E239** / **E240** on the nine commands 0.6.0 03b starts walking (`forecast`, `rplot`, `dynasave`, `dynatype`, the four decomposition commands) and on the audit's six trigger edges, one problem each; quiet files cover the legal list on each command, `rplot e;`, the aux pass-over, and the `osr_params AUX_EXPECT_1` abort. |

`e001/` files stop later checks (`analyze()` stops at a parse error). Do not use them to prove a W* warning.

## Inventory

[`inventory.json`](inventory.json) is the list of triggers. One row per trigger, not one row per code letter. Every **emit** and **added** `known_codes()` key has a row (`E999` is `documented-only`). Skip keys (`E186`–`E194`, `W186`–`W187`) have none. Out / vacated codes (`E040`, `W040`, `W041`, `I041`, `W071`, `I070`, `I071`, `W080`, `W081`, `DYNR`, `E060`) have none.

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
- `I050` on `sims_wu_2019` / `lk2024` is `archive`. govt option lists are parse-clean; do not add a W130 mutation of it.

Add a row, write or reuse a `.mod`, load it from the family test. Assert codes, messages, and spans. Do not rebuild the case with in-memory string edits.

Format cases go in `format/` as an input `.mod` and, when the formatter must change the text, a sibling `.formatted.mod`. Empty and whitespace-only inputs are asserted in the test, not as files.

