# Dygnosis workflow

Read this for MCP tool choice, source inputs, result coverage and proposed edits.
If the tools are unavailable, use `dygnosis-setup.md` first. Runtime tool schemas
own exact arguments. The server uses tool contract 3.

## Choose the task

| Need | Tool |
|---|---|
| Check one model, several roots or saved folders | `dynare_diagnose` |
| Understand a model, includes and companion files | `dynare_model_info` |
| Inspect one name and its accepted uses | `dynare_symbol_info` |
| Find equations, timing and source locations | `dynare_equations` |
| Read expanded text and written origins | `dynare_expand` |
| Look up commands, blocks, options, functions, operators or macros | `dynare_syntax` |
| Find a diagnostic code or read its correction guidance | `dynare_diagnostic_info` |
| Discover source actions or propose proven diagnostic fixes | `dynare_actions` |
| Rename one binding | `dynare_rename` |
| Format source | `dynare_format` |
| Produce an equation fragment with required context | `dynare_extract` |
| Compare model versions and captured source | `dynare_compare_models` |

A known unique name selects Symbol Info, Equations or Rename directly. Use
Symbol Info first when the task needs its definition or uses. Ambiguous names
return binding candidates; use their typed selector and original revision.

For syntax, context and parent have distinct meanings:

```jsonl
{"name":"pac_model"}
{"name":"diff","context":"model_expression"}
{"name":"irf","kind":"option","parent":"stoch_simul"}
```

Diagnostic Info accepts an exact `code` for its explanation or search filters
for summaries. Its `shared`, `added` and `skipped` classifications describe
relation to official Dynare, not problems fired on a model.

## Supply the model

Single-model requests use one explicit `source`:

- Text: `{"kind":"text","text":"..."}`. Unnamed text has no file identity.
- Map: `{"kind":"map","root":"main.mod","files":{"main.mod":"...","parts.inc":"..."}}`.
  The root and optional `owner_roots` must be exact supplied keys. `root_text`
  overlays the existing root, including with empty text. `include_dirs` adds
  virtual search folders.
- Disk: `{"kind":"disk","root":"C:/models/main.mod"}`. The root, owners and
  include folders are absolute paths on the MCP server host. It reads saved
  files and required dependencies; editor buffers are not inputs.

Maps read only supplied keys. Lookup normalizes slash direction and dot
segments, rejects colliding keys, then searches the including file's folder
and explicit or executed macro include folders in engine order. Supply all
required executed source. Unrelated `.mod` keys are not inferred as owners.

Model Info defaults to scalar counts and model kind. Select `sources` for
includes and companions, `structure` for timing lists and block flags, or
`declarations`, `assignments`, `model_locals`, `statements`, `blocks`, `includes`
or `scope` for detail. Relationships need map or disk identity. A listed MATLAB
companion is a location, not evidence that its code is correct.

For several roots, Diagnose takes `batch` instead of `source`:

```jsonl
{"batch":{"kind":"map","files":{"main.mod":"..."},"roots":["main.mod"]}}
{"batch":{"kind":"paths","paths":["C:/models/project"],"exclude_paths":["C:/models/project/archive"]}}
{"batch":{"kind":"sources","sources":[{"kind":"text","text":"..."},{"kind":"disk","root":"C:/models/other.mod"}]}}
```

Map roots are exact keys; a missing root remains a failed report. Saved path
discovery skips generated `+<model>` folders. Single and batch Diagnose share
`data.summary`, `data.roots`, `data.revision` and `data.root_revisions`. Read each
root's `ok`, `partial`, `failed` or `unfinished` status. Unnamed text has the
input identity `input:1`; it does not acquire an invented file path.

## Select names and equations

Symbol Info, Equations and Rename share one typed `symbol` selector:

```jsonl
{"symbol":{"kind":"name","name":"rho"}}
{"symbol":{"kind":"target","file":"parts.inc","line":12,"column":3}}
{"symbol":{"kind":"binding","binding_id":"<returned identity>"},"expected_revision":"<original revision>"}
```

Default Symbol Info includes `data.binding` and bounded `data.references`.
Select `definition`, `metadata`, `assignments` or `references`; references-only
works directly. `include_declaration` defaults to true; `occurrences` adds
expanded uses. Comments, unrelated strings, native MATLAB and inactive source
are excluded.

Equation name, tags, scope/dimension and symbol filters combine with AND.
Extraction combines names with OR and tags with AND. A typed `equation`
selects a returned identity or zero-based scoped row:

```jsonl
{"equation":{"kind":"id","equation_id":"<returned identity>"},"expected_revision":"<original revision>"}
{"equation":{"kind":"index","scope":"aggregate","index":0}}
{"equation":{"kind":"index","scope":"dimension","dimension":"households","index":0}}
```

Expansion selects an equation identity or text range. Inclusive `line_range`
and exclusive-end `expanded_range` refer to expanded text. Returned navigation
points to verified written source. Edit that written source with its actual
file and macro context.

## Read and continue results

Each result has `schema_version: 3`, `outcome`, `inputs`, `coverage`,
`selection`, `data`, `proposal` and `limits`. Structured clients read
`structuredContent`; text clients parse the single JSON item. MCP editions
before 2025-06-18 use the same contract through JSON text only.

Read outcome causes and coverage before relying on facts. `changed` means
proposed source differs; no file has been saved. Partial and refused results
state available and omitted work. A complete page remains `ok`; paging and
incomplete analysis have different meanings.

Collections default to 25 rows shared across the response. `page_size` can be
1–1000. `selection.collections` gives returned, available and total matches,
with `total_matches: null` when complete totals are unavailable. Continue only
when the task needs more rows and `selection.continuation` is non-null. Keep
source, selectors, sections, filters and page size, with the original guard:

- Single-model requests: `expected_revision` from `inputs[0].revision`.
- Single-source Diagnose: `expected_revision` from its `data.root_revisions`.
- Batch Diagnose: `expected_revisions` keyed by returned root identity.
- Compare: `expected_revision_a` and `expected_revision_b` from its two inputs.

`INPUT_CHANGED` requires a new selection from current source. Pages,
identities and proposals belong to their original snapshot. Proposals are
returned completely and are never paged.

Dygnosis checks static `.mod` source against Dynare 7.2. An empty diagnostic
list does not establish a steady state, determinacy, the economics or a
successful experiment. Run official Dynare under MATLAB for those claims
(`debugging.md`, “Run-and-fix loop”).

Counts describe written source before transformations and are authoritative
only with complete coverage. Model locals stay outside equation counts and
retain execution and dimension scope. Per-use `timing` is the written offset;
`dynare_timing` and timing classes apply the `predetermined_variables`
convention. Static replacement rows do not contribute dynamic timing.

MCP positions are one-based Unicode-scalar columns with exclusive ends.
Missing origins are unavailable targets. Obtain current locations after edits.
Source limits are 1,000,000 UTF-8 bytes per file, 8 MiB combined source, 256
files or lookup candidates and 64 checked roots. Expansion and output have
separate 8 MiB limits; `macro_work_limit` selects finite analysis work.

## Propose and apply changes

Actions has two selections:

- Omit `fixes` for bounded action summaries. Select an `action_id` with its
  original `expected_revision` for that alternative's complete proposal.
- Set `fixes: true` for compatible proven diagnostic fixes, optionally by
  `code`, written `target` or returned `diagnostic_ids`. A known code or target
  works directly. Reused diagnostic identities require `expected_revision`.

Alternatives remain separate. Bulk fixes preserve verification, staged repair
and conflicts, and report skipped and remaining work. Rename and Format have
their own direct tools. Unsafe Rename inputs withhold its complete proposal.

Before applying, verify the proposal revision, source policy, checked owners,
include folders, macro work limit, every input guard and every edit's
`expected_text`. Guards cover unchanged dependencies and missing candidates,
with identity, hashes and length. For saved disk input, repeat the proposal
request with its original `expected_revision` first. Apply the complete edit
set with external file tools, refresh source and run Diagnose. A revision
check does not reserve files against later changes.

Extraction returns a fragment. Supply setup in its omitted context before
presenting it as a runnable experiment.

## Compare versions

Compare takes one typed `comparison`:

```jsonl
{"comparison":{"kind":"sources","before":{"kind":"text","text":"..."},"after":{"kind":"text","text":"..."}}}
{"comparison":{"kind":"repository","repository_path":"C:/models/project","before":{"kind":"git","root_file":"main.mod","ref":"HEAD"},"after":{"kind":"working","root_file":"main.mod"},"search_paths":["common"]}}
```

Each explicit source retains its text/map/disk policy and include folders.
Before is the baseline. Git refs resolve once before source reads; report the
returned commits. Roots are exact repository-relative paths. Working reads
saved host files. Relative search folders resolve against the repository.
Historical dependencies come only from the selected commit; outside sources,
symlinks and gitlinks fail explicitly. No Git fetch occurs.

Follow navigation with its side, input identity, captured source and commit.
Comparison reports retained source/fact changes, not algebraic or numerical
equivalence. Coverage can remain partial for retained-history limits even when
both source captures are complete.

## Report

Name the tools used, roots and includes, codes, coverage and omitted work.
Keep static findings separate from numerical results. If tools were
unavailable, say that the Dygnosis checks were not run.
