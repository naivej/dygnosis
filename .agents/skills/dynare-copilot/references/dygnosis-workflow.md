# Dygnosis workflow

Read this when you use the Dygnosis MCP tools on a Dynare model: which tool fits a need, how to pass the
model, how to read the result, and how to apply proposed edits. If the tools are not connected, read
`dygnosis-setup.md` first.

Read the runtime tool schemas for exact arguments. This page gives the rules that the schemas do not.

## What Dygnosis does

Dygnosis checks `.mod` source against the Dynare 7.2 language: the problems that Dynare reports before
MATLAB runs, plus its own warnings and notes. It does not run Dynare, MATLAB or Octave.

- **Error:** Dynare would refuse the file before MATLAB runs.
- **Warning:** Dynare would accept the file, but something looks wrong. This includes Dynare's own
  warnings.
- **Information:** nothing is wrong; confirm that it is intended. I208 (equation without a `name` tag),
  I209 (declaration without `long_name`) and I210 (number written in an equation) are writing
  preferences, not refusals.

`dynare_explain` gives the meaning and the fix of a code. `dynare_list_diagnostic_codes` lists all codes
with a `kind`: `shared` (Dynare reports the same problem), `added` (only Dygnosis reports it), or
`skipped` (a Dynare refusal that Dygnosis does not report).

An empty diagnostic list is static evidence only. It does not show a steady state, the Blanchard-Kahn
conditions, determinacy, correct economics or a successful experiment. Those need a Dynare run
(`debugging.md`, "Run-and-fix loop").

## Choose a tool

| Need | Tool and use |
|---|---|
| Check the current model | `dynare_diagnose`. Run it before a change (baseline) and after each substantive change. |
| Check several saved models or a folder | `dynare_workspace_diagnose`. Report each failed root separately. |
| Symbols, written timing, blocks, counts | `dynare_model_info`: `endogenous`, `predetermined`, `forward_looking`, `static`, `mixed`, counts, block flags. |
| Find an equation and its source location; check the equation count | `dynare_equations`: rows with tags, `idents` (class, lead or lag, timing class), `origin`, and `count_gap`. |
| See text produced by `@#include` and macros | `dynare_expand`: `effective_text`, `complete`, navigation back to the written source. |
| Find includes and companion files (steady-state file, helper files) | `dynare_related_files`, then read those files. A listed companion is not proof that its MATLAB code is correct. |
| Understand a diagnostic | `dynare_explain`; `dynare_list_diagnostic_codes` to find a code. |
| Check the options of a command | `dynare_list_options`, then the Dynare manual for meaning. An unknown command returns `known: false`. Do not invent options. |
| Rename a symbol | `dynare_find_references` (comments are skipped), then `dynare_rename`. |
| Apply a known fix, or format | `dynare_auto_fix`, `dynare_format` (`formatIndent`: `"tab"` or 1–8). |
| Review the structural effect of an edit | `dynare_compare_models` with the before and after text and the includes of each side. It is not a numerical equivalence test. |
| Isolate a mechanism or make a small reproducer | `dynare_extract` by equation `names` or `tags` (all tags must match), optionally in one heterogeneity `dimension`. |

Call the tools that help the task. Do not call every tool on every task.

## Pass the model

**One file.** Pass the complete text as `file_content`.

**A file with includes.** Pass `active_file` (the root) and a `files` map from file key to complete
text. Include the text of each executed `@#include` target.

- The `active_file` value must match a key in `files` exactly (case and path form).
- `file_content`, when you also pass it, replaces the root's entry for that request.
- The map is text that you supply. It does not give the tool permission to read files on disk, and it
  is not the editor's unsaved buffer. Read each file with your own file tools first.
- Find the includes with `dynare_related_files`. An include with `resolved: false` is missing from your
  map: read it and add it.

**Confirm that the root resolved.** If `active_file` does not match a key, the tools return an empty
result without an error: `[]` from `dynare_diagnose`, zero counts from `dynare_model_info`, `{}` from
`dynare_rename`. This looks like a clean model. After you build a map, check that `dynare_expand`
returns `root_file` equal to your key, or that `dynare_model_info` returns your declarations.

**Saved files on disk.** `dynare_workspace_diagnose` has two modes. Do not combine them.

- Map mode: a `files` map and a `roots` list of keys. A root that is not in the map is reported as
  failed.
- Path mode: `paths`, a list of files or folders on the machine that runs the server. It reads the
  saved files, walks folders, and skips the generated `+<model>` folders. It does not see unsaved edits.

When one include belongs to several root files, choose the roots explicitly (Dygnosis W061 reports an
ambiguous parent).

**Two versions.** `dynare_compare_models` needs `file_content_a` and `file_content_b` in every call. For
includes, give `active_file_a`/`files_a` and `active_file_b`/`files_b`, or one shared `files` map. Keep
the inputs of the two sides separate.

## Read the result

**Completeness.** An unresolved include or an unsupported macro construct makes the result partial.

| Tool | Sign of a partial result |
|---|---|
| `dynare_diagnose` | E061 (an `@#include` target could not be opened), I211 (macro expansion incomplete) |
| `dynare_model_info` | `{"status": "incomplete", …}` |
| `dynare_expand` | `complete: false` |
| `dynare_related_files` | a row with `resolved: false` |
| `dynare_extract` | `status: "unsupported_context"`, `fragment: null` |
| `dynare_compare_models` | `navigation.before.complete` or `navigation.after.complete` is `false`; equations of the partial side can show as removed |
| `dynare_workspace_diagnose` | a root with `status: "failed"` and a `failure` message |

A partial result cannot support a claim that the model is clean. Fix the inputs, or report what was not
covered.

**Equation count.** `dynare_equations` returns `count_gap` (`n_equations`, `n_endogenous`, `delta`,
`unreferenced_endogenous`). It can return a count for a model with an unresolved include without
marking it incomplete; the missing include then looks like missing equations. Confirm completeness with
`dynare_model_info` or `dynare_expand` before you use `count_gap`. The counts describe the written model
before Dynare transforms it; Dynare's `AUX_*` auxiliary variables are not in them. With
`ramsey_model` or `discretionary_policy` and `instruments` listing N names, the expected `delta` is −N.

**Timing lists.** `predetermined`, `forward_looking`, `static`, `mixed` and the counts
(`n_jumpers`, `n_state_variables`) in `dynare_model_info`, and `timing_class` in `dynare_equations`,
follow the written leads and lags. They do not apply `predetermined_variables`: a variable declared
there and written as `k(+1)` is listed as forward-looking, although Dynare treats it as predetermined.
Do not compare these counts with the Blanchard-Kahn count in such a file.

**Equation numbers.** The `index` of a `dynare_equations` row starts at **zero**, and the `index` filter
uses the same numbering (the schema text says "starting at one"; the behavior is zero-based). Prefer the
`name` filter. If you use `index`, take the value from an earlier unfiltered result; do not count
equations from one. An equation number in Dygnosis output is not an equation number in Dynare's
transformed model or in MATLAB output.

**Macro-generated tags.** Dygnosis does not substitute `@{…}` inside a quoted tag: for
`[name='eq@{j}']` in an `@#for` loop it reports the name `eq@{j}` for each copy, while Dynare writes
`eq1`, `eq2`, …. The `name` filter does not find `eq2`. Use the unfiltered rows (`origin_frames` gives
the loop value) or `dynare_expand` navigation.

**Locations.** Lines and columns are one-based; columns count Unicode scalar values. A location belongs
to the file named in the row (`file`, `origin_uri`), or to the root when no file is named. After an
edit, query again; do not reuse old locations. Edit the written source at the returned location, not
the expanded text.

## Apply edits

The editing tools return proposed text. They do not save files.

| Tool | Returns |
|---|---|
| `dynare_rename` | With a map: an object with only the changed files. Without a map: the new text. An illegal new name leaves the text unchanged. |
| `dynare_auto_fix` | The text with the stored fixes applied. |
| `dynare_format` | `status: "changed"` with `formatted_text`, or `status: "unchanged"` with `formatted_text: null`. |
| `dynare_extract` | `fragment` with the declarations it needs, and `omitted_context` (for example `calibration`). `status: "empty"` when nothing matched. |

1. Inspect the returned text.
2. Apply only the intended change with your file tools. Keep unrelated edits by the user.
3. Refresh the model inputs (the `files` map) and run `dynare_diagnose` again.

An extracted fragment is not a runnable model. Before you present it as one, add what
`omitted_context` names: parameter values, shocks, steady state, and the experiment.

## Report

Name the tools you ran, the root and the includes you passed, the codes returned, and the completeness
of the result. Report static findings apart from numerical results. If the tools were not available,
say that the Dygnosis checks were not run.
