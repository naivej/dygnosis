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
- **Information:** nothing is wrong; confirm that it is intended. I208 (equation without a `name` tag)
  and I209 (declaration without `long_name`) are writing preferences, not refusals.

`dynare_explain` gives the meaning and the fix of a code. `dynare_list_diagnostic_codes` lists all codes
with a `kind`: `shared` (Dynare reports the same problem), `added` (only Dygnosis reports it), or
`skipped` (a Dynare refusal that Dygnosis does not report).

An empty diagnostic list is static evidence only. It does not show a steady state, the Blanchard-Kahn
conditions, determinacy, correct economics or a successful experiment. Those need a Dynare run
(`debugging.md`, "Run-and-fix loop").

## Choose a tool

| Need | Tool and use |
|---|---|
| Check the current model | `dynare_diagnose`. Run it before a change (baseline) and after each edit to a declaration, an equation, or a command. |
| Check several saved models or a folder | `dynare_workspace_diagnose`. Report each failed root separately. |
| Symbols, timing classes, blocks, counts | `dynare_model_info`: `endogenous`, `predetermined`, `forward_looking`, `static`, `mixed`, counts, block flags. |
| Inspect one symbol's meaning | `dynare_symbol_info`: select one binding by name, target or returned `binding_id`; ambiguous names return scoped candidates. |
| Find accepted uses of a symbol | `dynare_find_references`: select a semantic binding; comments, unrelated strings, native MATLAB and inactive source are excluded. |
| Inspect `#` helper expressions or explicit model-local metadata | `dynare_model_info`: `model_locals.declarations` and `model_locals.definitions`. |
| Find an equation and its source location; check the equation count | `dynare_equations`: rows with tags, `idents` (written `timing`, `dynare_timing`, timing class), `origin`, and `count_gap`. |
| See text produced by `@#include` and macros | `dynare_expand`: `effective_text`, `complete`, navigation back to the written source. |
| Find includes and companion files (steady-state file, helper files) | `dynare_related_files`, then read those files. A listed companion is not proof that its MATLAB code is correct. |
| Understand a diagnostic | `dynare_explain`; `dynare_list_diagnostic_codes` to find a code. |
| Check the options of a command | `dynare_list_options`, then `references/dynare-manual.md` for meaning. An unknown command returns `known: false`. Do not invent options. |
| Rename a model symbol | `dynare_rename`: select exactly one `old_name`, target or `binding_id`. Ambiguous bindings and incomplete or incompatible owners withhold the complete proposal. |
| Apply a known fix, source action, or format | `dynare_auto_fix`, `dynare_code_actions`, `dynare_format` (`formatIndent`: `"tab"` or 1–8). Each returns proposed edits and never writes files. |
| Review the structural effect of an edit | `dynare_compare_models` with supplied text and includes, or with explicit repository inputs. It is not a numerical equivalence test. |
| Isolate a mechanism or make a small reproducer | `dynare_extract` by equation `names` or `tags` (all tags must match), optionally in one heterogeneity `dimension`. |

Call the tools that help the task. Do not call every tool on every task.

## Pass the model

Single-model tools accept one explicit `source` object:

- **Text:** `{"kind":"text","text":"..."}`. This is unnamed text; it cannot read includes or companions from disk.
- **Map:** `{"kind":"map","root":"main.mod","files":{"main.mod":"...","parts.inc":"..."}}`. Supply the exact root and all required include text. `root_text` can overlay the existing root, including with an empty string. `include_dirs` adds virtual folders; `owner_roots` selects additional roots to check.
- **Disk:** `{"kind":"disk","root":"C:/models/main.mod"}`. The MCP server reads saved host files. The root, `owner_roots` and `include_dirs` must be absolute host paths; editor buffers are not read.

The `source` object is exclusive with legacy `file_content`, `active_file` and `files`. Legacy single-file calls use `file_content`. Legacy map calls require `active_file` to match an exact existing key in nonempty `files`; an explicit `file_content` overlays that root even when empty. `dynare_related_files` needs map or disk identity.

Map keys preserve their spelling. Include lookup normalizes slash direction and dot segments lexically, rejects colliding keys, and never reads an unsupplied host file or falls back to a matching filename. It searches the including file's folder, then explicit and executed macro include folders in engine order. Use an exact key or explicit search folder for non-sibling includes. The checked owner set is the selected root plus explicit `owner_roots`; unrelated `.mod` keys are not inferred as owners.

Use `dynare_related_files` to inspect include and companion relationships for a named map or disk root. For an incomplete result, supply missing executed source before treating facts or edit proposals as complete.

**Saved files on disk.** `dynare_workspace_diagnose` has two modes. Do not combine them.

- Map mode: a `files` map and a `roots` list of keys. A root that is not in the map is reported as
  failed.
- Path mode: `paths`, a list of files or folders on the machine that runs the server. It reads the
  saved files, walks folders, and skips the generated `+<model>` folders. It does not see unsaved edits.

When one include belongs to several root files, choose the roots explicitly (Dygnosis W061 reports an
ambiguous parent).

**Two versions.** `dynare_compare_models` has two modes. Do not combine them.

- Supplied text: pass `file_content_a` and `file_content_b`. For includes, give
  `active_file_a`/`files_a` and `active_file_b`/`files_b`, or one shared `files` map.
- Repository: pass an absolute `repository_path` on the server host, and explicit `before` and
  `after` inputs. Each input has `kind` (`git` or `working`) and a repository-relative `root_file`.
  Every Git input also requires `ref`. Working reads saved files; it does not see editor buffers.

For example, this compares HEAD with the saved model:

```json
{
  "repository_path": "C:/models/project",
  "before": { "kind": "git", "root_file": "main.mod", "ref": "HEAD" },
  "after": { "kind": "working", "root_file": "main.mod" },
  "search_paths": ["common"]
}
```

Before is the baseline; After is the model under review. Git refs resolve once before source reads.
Use the returned commit ids when reporting a result or repeating a fixed comparison. A renamed root
needs its exact path on each side. Relative `search_paths` resolve against `repository_path`.
Historical roots and executed includes use only their selected commit trees. Git never fetches missing
objects. A required source outside that repository, a symlink, or a gitlink fails explicitly.

## Read the result

Every MCP result includes a canonical object with `schema_version: 2`.
Structured clients read tool facts from `structuredContent.data`; `outcome`,
`inputs`, `coverage`, `selection`, `proposal` and `limits` sit beside `data`.
Text clients can parse the final JSON content item. Existing tools keep their
default leading content items in order. `detail: "compact"` omits those legacy
mirrors. Editions from 2025-06-18 advertise `outputSchema` and
`structuredContent`; older editions receive the canonical JSON text only.

`outcome.status` is `ok`, `changed`, `unchanged`, `partial` or `refused`.
Read its causes and per-owner `coverage` before relying on facts. A page of
complete analysis remains `ok`; `selection` describes the page independently.
`changed` means an edit proposal changes text, not that a file was saved.

**Completeness.** An unresolved include or an unsupported macro construct makes the result partial.

| Tool | Sign of a partial result |
|---|---|
| `dynare_diagnose` | `outcome.status: "partial"`; diagnostic rows remain in `data`. |
| `dynare_model_info` | `outcome.status: "partial"`; inspect its data and owner coverage for withheld facts. |
| `dynare_equations` | `outcome.status: "partial"`; incomplete models withhold authoritative rows and count gaps. |
| `dynare_expand` | `outcome.status: "partial"`; inspect `data.complete` and source coverage. |
| `dynare_related_files` | `outcome.status: "partial"`; inspect relationship rows and coverage. |
| `dynare_find_references`, `dynare_symbol_info` | `outcome.status: "partial"` when accepted occurrences lack verified written targets. |
| `dynare_rename`, `dynare_code_actions`, `dynare_auto_fix`, `dynare_format` | Refused or partial outcomes explain why a complete proposal is unavailable. |
| `dynare_compare_models` | Existing comparison status and source coverage; incomplete comparisons make no removed/added claims. |
| `dynare_workspace_diagnose` | Per-root status distinguishes complete, partial, failed and unfinished work. |

A partial result cannot support a claim that the model is clean. Fix the inputs, or report what was not
covered.

**Equation count.** `dynare_equations` returns `count_gap` (`n_equations`, `n_endogenous`, `delta`,
`unreferenced_endogenous`) only for a complete model. An unresolved include, incomplete parse, or
incomplete macro expansion returns `status: "incomplete"` with `count_gap: null`. The counts describe
the written model before Dynare transforms it; Dynare's `AUX_*` auxiliary variables are not in them.
With `ramsey_model` or `discretionary_policy` and `instruments` listing N names, the expected `delta`
is −N.

**Timing lists.** `predetermined`, `forward_looking`, `static`, `mixed` and the counts
(`n_jumpers`, `n_state_variables`) in `dynare_model_info`, and `timing_class` in `dynare_equations`,
use Dynare timing after the `predetermined_variables` convention conversion. Per-use `timing` remains
the written offset; `dynare_timing` is the converted offset. Use the returned class and counts for
classification. Older engines omit `dynare_timing`; do not invent it.

**Model-local variables.** `model_locals.declarations` lists explicit
`model_local_variable` declarations and optional TeX labels.
`model_locals.definitions` lists completed accepted `#` definitions, each with
its `name`, `dimension`, parsed `text`, right-side `expression`, and `idents`.
`dimension: null` is the aggregate model; a name selects that heterogeneity
dimension. Several blocks of the same model share their definitions. Both
arrays follow execution order, so macro copies can have the same written
origin and different rows. A declaration alone gives no expression or value.
Incomplete model info withholds this inventory; older engines can omit it.

An identifier with `class: "model_local"` is a recognized local use. Its
`timing` remains the written offset, including shifted helper uses; it has no
endogenous timing class. Follow verified `origin` and `origin_frames` when
editing a definition produced by macros. Read an absent origin as an
unavailable source target. The inventory does not substitute helpers or
evaluate them. `#` rows stay outside counted equations.

**Equation numbers.** The `index` of a `dynare_equations` row starts at **zero**, and the `index` filter
uses the same numbering. Prefer the `name` filter when tags are available. An equation number in
Dygnosis output is not an equation number in Dynare's transformed model or in MATLAB output.

**Macro-generated tags.** Quoted `@{…}` values expand: `[name='eq@{j}']` in an `@#for` loop becomes
`eq1`, `eq2`, …, and the `name` filter finds those tags. Use `origin_frames` or `dynare_expand`
navigation when you need the loop copy that produced a row.

**Locations.** Lines and columns are one-based; columns count Unicode scalar values. A location belongs
to the file named in the row (`file`, `origin_uri`), or to the root when no file is named. After an
edit, query again; do not reuse old locations. Edit the written source at the returned location, not
the expanded text.

Input targets use `file`, `line` and `column`; a null or omitted file selects
the root. Output ranges use `line`, `column`, `end_line` and `end_column`; the
end is exclusive. Sections and filters use the runtime schema. Independent
equation filters combine with AND, while extraction names combine with OR and
tags combine with AND. Set `page_size` from 1 to 1000 for a bounded collection;
continue with the returned token, the same query and its `expected_revision`.
Do not page complete edit proposals. Source limits are 1,000,000 UTF-8 bytes
per file, 8 MiB total source text, 256 files or lookup candidates and 64 checked
roots. Macro expansion and serialized output each retain separate 8 MiB limits.

Repository comparison adds `inputs` with `schema_version: 1`. Read each side's root, resolved commit,
source policy, search paths, and completeness before making a change claim. Its navigation has
`schema_version: 2`; each written target carries `input_id`, `file_key`, and the commit for a historical
source. Read that file at that commit when following a historical target. Supplied-text calls retain
navigation schema 1 and their original file keys.

## Apply edits

`dynare_rename`, `dynare_code_actions`, `dynare_auto_fix` and `dynare_format`
return proposals; they do not write files. Each proposal contains complete
non-overlapping source edits, expected old text, the source revision and
SHA-256 guards for all checked inputs, including unchanged dependencies and
missing lookup candidates. Do not apply part of a proposal.

The editing tools return proposed text. They do not save files.

| Tool | Returns |
|---|---|
| `dynare_rename` | A semantic Rename proposal selected by one name, written target or binding identity. |
| `dynare_code_actions` | Available source actions at a target; select one `action_id` and repeat with its `expected_revision` for its complete proposal. |
| `dynare_auto_fix` | A proposed source revision with applied/skipped fixes and remaining diagnostics. |
| `dynare_format` | A proposed whole-file or selected-line revision. |
| `dynare_extract` | `fragment` with the declarations it needs, and `omitted_context` (for example `calibration`). `status: "empty"` when nothing matched. |

For disk inputs, repeat the proposal request with its `expected_revision`. Verify every guard, apply the complete proposal with your file tools, refresh the inputs and run `dynare_diagnose` again. The revision check does not reserve files against later changes.
3. Refresh the model inputs (the `files` map) and run `dynare_diagnose` again.

An extracted fragment is not a runnable model. Complete setup the result marks
as omitted before presenting it as a runnable experiment.

Extraction retains needed nested local definitions and explicit metadata from
the selected model, including definitions in its earlier blocks. A definition
with the same name in another heterogeneity dimension is a different dependency.

## Report

Name the tools you ran, the root and the includes you passed, the codes returned, and the completeness
of the result. Report static findings apart from numerical results. If the tools were not available,
say that the Dygnosis checks were not run.
