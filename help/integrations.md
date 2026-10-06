# Connect an LSP client

Configure an LSP client to launch `dygnosis` over stdio, without arguments. The [executable](get-started.md#executable) section lists every launch. The VS Code extension adds native Settings, a [count bar](model-and-includes.md), [model view](model-and-includes.md), [colors](appearance.md), [CodeLens](model-and-includes.md), [diagnostic controls](diagnostics.md), [model Diff](structural-diff.md), [origin navigation](effective-model.md), and [project checks](project-checks.md). See [distribution](get-started.md) for host packages and runtime requirements.

## LSP configuration messages

Send settings in `initialize.initializationOptions`, then in `workspace/didChangeConfiguration.params.settings` when they change. Existing clients can keep the single-folder form:

```json
{
  "dynare": {
    "searchPaths": ["includes"],
    "formatIndent": 4,
    "nameDetails": {"longName": true, "tex": true},
    "outline": {
      "sections": ["declarations", "blocks", "commands", "dimensions", "equations"],
      "equationNumbers": true
    },
    "parameterValueHints": true
  }
}
```

For several workspace folders, send a complete configuration snapshot with resolved values for each folder and defaults for loose files:

```json
{
  "dynare": {
    "configuration": {
      "schemaVersion": 1,
      "loose": {
        "searchPaths": [],
        "formatIndent": "tab",
        "nameDetails": {"longName": true, "tex": true},
        "outline": {
          "sections": ["declarations", "blocks", "commands", "dimensions", "equations"],
          "equationNumbers": true
        },
        "parameterValueHints": true
      },
      "folders": [
        {
          "uri": "file:///C:/models/project",
          "settings": {
            "searchPaths": ["includes"],
            "formatIndent": 4,
            "nameDetails": {"longName": true, "tex": false},
            "outline": {
              "sections": ["declarations", "blocks", "dimensions", "equations"],
              "equationNumbers": true
            },
            "parameterValueHints": true
          }
        }
      ]
    }
  }
}
```

Register the same folders through `initialize.workspaceFolders` and report additions/removals through `workspace/didChangeWorkspaceFolders`. The deepest containing folder supplies a file's settings. A registered folder without its own snapshot entry uses `loose`. Each supplied folder entry starts from server defaults; the client should send its complete resolved settings. A new snapshot replaces the previous one, so omitted values and removed entries reset. The older form remains a partial update.

For native setting values, use the feature's native Settings link. The examples above show the LSP wire shape.

Relative search paths resolve against the containing workspace folder, or the loose file's directory. Untitled and other virtual documents have no disk base for includes. Open document text is shared across roots, while each root retains its own include paths and cached inputs. The legacy `searchPathsByRoot` map remains supported without combining one root's paths with another's.

Presentation settings use the displayed document. Analysis and include settings use the model root. Hiding metadata, Outline rows, or hints leaves the shared model, MCP data, and comparison facts intact. Invalid values fall back safely and are explained through the client's log channel. An unsupported `schemaVersion` leaves the current settings in place.

Completion keeps the identifier as its label and insert text. A client supporting completion label details gets the long name beside the label; other clients get it in the item's detail. TeX stays literal text. Empty `model`, `steady_state_model`, `initval`, `endval`, and `shocks` skeletons use snippet placeholders only when supported; other clients receive plain empty skeletons.

## Written model information

The initialize response advertises the following under `capabilities.experimental.dygnosis`:

```json
{
  "modelInfo": {"command": "dynare/modelInfo", "schema_version": 1},
  "modelInfoChanged": true,
  "configuration": {"schema_version": 1}
}
```

Call `workspace/executeCommand` with:

```json
{
  "command": "dynare/modelInfo",
  "arguments": [{
    "root_uri": "file:///C:/models/project/main.mod",
    "document_uri": "file:///C:/models/project/equations.inc"
  }]
}
```

Omit `document_uri` to use the root itself. Choose an available `.mod` or `.dyn` root with a `file` or `untitled` URI; an included document must belong to that root. Invalid choices return an `error` and `code`, with known `owner_roots` when a root choice is needed. This read-only request does not select an owner globally for other LSP requests.

The response contains the shared MCP model counts and timing fields, plus `schema_version`, `root_uri`, `document_uri`, `document_version`, `revision`, `complete`, and `owner_roots`. `document_version` is null for a document that is not open. The opaque revision covers the root's settings and current input paths and contents, including unsaved overlays, disk dependencies, and missing candidates. Opening or closing an editor overlay with identical text keeps the revision; changed text, dependency availability, or settings invalidate it. Treat revisions as equality tokens within the current server instance.

Timing classes, lists, counts, and displayed offset sets use the shared
predetermined-variable convention conversion. For a valid marked aggregate
endogenous name in a dynamic equation or model-local definition, the Dynare
offset is the written offset minus one. Static-only replacement equations do
not contribute to dynamic timing; heterogeneous model equations keep their
written offsets. This conversion does not reproduce the other Transform steps
or predict a solver result.

In MCP `dynare_equations`, each identifier's existing `timing` remains its
**written offset**. The additive `dynare_timing` is its **offset after the
predetermined-variable convention conversion**. Unshifted uses have equal
values. Use the returned `timing_class` and model-info summary counts for
classification. Older engines omit `dynare_timing`; clients must not invent a
shifted value when it is absent. Equation text, comparisons, extraction, and
effective-model text keep written offsets.

Additive `statements`, `declarations`, and `equations` describe the whole chosen root. Source records carry verified `location`, `segments`, `anchor`, and macro context where available. A span split across files has separate segments rather than an invented continuous range. Statements also provide safe `lens_anchor` values for client actions. `related_files` lists includes and companion files; `block_categories` supplies the recognized category registry and defaults.

Equation numbers are **Dygnosis numbers before transformation**. Aggregate equations and each heterogeneity dimension have their own sequence. Removed equations and static replacement rows are not counted. These numbers describe the file being edited and may differ from numbers after Dynare adds auxiliary equations for MATLAB.

When `complete` is false, authoritative counts and equation numbers are withheld. Recovered file-local structure can remain visible. In the normal Outline, included equations belong to their own file; a request on an include uses a unique known owner when available. Several owners retain only common local structure without choosing a root's numbers.

To receive invalidations, set `initialize.capabilities.experimental.dygnosis.modelInfoChanged` to `true`. The server then sends `dynare/modelInfoChanged` with:

```json
{"schema_version": 1, "root_uri": "file:///C:/models/project/main.mod", "revision": "opaque-input-revision"}
```

A missing root has `revision: null`. Re-request model information for affected roots after changes to includes, settings, overlays, or reported disk files. A client should forward watched-file events, cancel superseded requests, and discard old revisions and document versions. Standard semantic-token and inlay-hint refresh requests are sent only when the client advertises support; document-symbol refresh needs client handling.

## Semantic token migration

The server negotiates four role types against the types and modifiers advertised by the client at initialize. Both full and range token responses use the returned legend.

| Role | Token type | Unsupported custom-type fallback |
|---|---|---|
| Endogenous variable | `dynareEndogenous` | Supported standard `variable` |
| Exogenous or deterministic exogenous variable | `dynareExogenous` | Supported standard `variable` |
| Model parameter | `dynareParameter` | Supported standard `variable` |
| `#` model-local variable | `dynareModelLocal` | Supported standard `variable` |

If neither the custom type nor `variable` is supported, that role is omitted. Initialize without semantic-token capabilities yields an empty legend and no semantic role tokens. Unsupported modifiers are omitted independently. Clients can advertise `declaration`, `forwardLooking`, and `predetermined` to retain their existing meanings. Distinct roles may still have the same color in a theme or in a standard-only client.

[Appearance](appearance.md) owns the VS Code selector migration and styling examples. Other LSP clients supply their own token registrations and theme rules.

## Expression value hints

The LSP inlay-hint provider returns values at verified written source positions. [Appearance](appearance.md#expression-value-hints) owns the folding limits and visibility controls.

## Diagnostic locations and fixes

Applicable duplicate diagnostics link to the first occurrence through LSP `relatedInformation` and MCP's additive `related` rows. Include cycles retain the written include edges. LSP coordinates are zero-based UTF-16; MCP keeps one-based scalar coordinates. Each related location is mapped independently to its own source file and macro occurrence.

Published fixes remain available for an unopened include when its checked source snapshot is current. Edits to open files carry their actual document versions; unopened files use an unversioned identifier. Stale source or root revisions withhold the action. W020 and W022 carry the standard LSP Unnecessary tag; E021 does not.

I208 and I209 carry `data.writing_context` with `root`, `input_revision`,
`statement_ids`, and `rows`. The normalized root path and opaque revision bind
the note to one checked input. `statement_ids` lists its owning expanded
statement occurrences. `rows` is `{ "kind": "equations", "row_ids": [...] }`
for I208 or `{ "kind": "declarations", "row_ids": [...] }` for I209. Row ids
are expanded token positions within that revision, not written offsets or
displayed equation numbers. Repeated macro occurrences retain distinct ids
even when their written keyword range is shared.

Send the diagnostic's complete `data` back in a code-action request. The server
requires a current matching note; missing, stale, or altered contexts cannot
select another note's rows. I208 edits only safely mapped equations from its
row set. Its keyword can be in the root while the edits belong to an include.
I209 exposes the same ownership contract for declaration rows. A code or
written range alone cannot identify either scope across roots or statements.

## Project diagnostics

Project diagnostics check unopened saved `.mod` models beneath file-backed workspace folders. Discovery uses the recursive saved-model walk, including its generated `+` directory skip. Open `.dyn`, excluded models, loose files, and untitled documents retain ordinary editor diagnostics.

## Configuration

`dynare.projectDiagnostics` is a window boolean. Send it in `dynare.configuration.loose.projectDiagnostics` in the existing schema-version 1 complete configuration snapshot. The legacy `dynare.projectDiagnostics` route is also accepted. Folder entries do not override this window switch.

`dynare.projectExcludePaths` is a resource list of strings. Send the resolved list in each folder entry's `settings.projectExcludePaths`; loose settings provide the fallback. The most specific containing workspace folder supplies exclusions and search paths. Patterns are relative to that folder; `*` and `?` stay within one path component, `**` crosses directories, `**/` also matches no directory, and a trailing slash includes the directory's descendants. Both slash styles are accepted; Windows matching ignores case. Invalid list values fall back to no exclusions; invalid entries are ignored and explained in the LSP log.

Exclusions select background roots only. An excluded include remains a dependency of its owners. Opening an excluded root still checks it. Turning project diagnostics off cancels work and clears project contributions while retaining reports belonging to open compilation units.

## Capability and protocol

Check `initialize`'s `capabilities.experimental.dygnosis.projectDiagnostics` before using this feature:

```json
{
  "schema_version": 1,
  "status_command": "dynare/projectStatus",
  "recheck_command": "dynare/recheckProject",
  "cancel_command": "dynare/cancelProject",
  "active_model_notification": "dynare/activeModelChanged",
  "status_notification": "dynare/projectStatusChanged",
  "typing_pause_ms": 250
}
```

Opt in to status notifications with client `capabilities.experimental.dygnosis.projectStatusChanged = true`. Notifications and command results have the same schema. Reject unsupported schemas rather than inferring a layout. On reconnect, execute `dynare/projectStatus` with no arguments.

Send `dynare/activeModelChanged` with `{"root_uri":"file:///.../chosen.mod"}` or `{"root_uri":null}`. The URI identifies the explicitly chosen model owner; clients must not silently choose an include's owner. That choice stays authoritative until updated or cleared. Opening, editing, saving, hovering, or querying another model updates only the fallback. Null restores recent-input priority; a server restart resets the explicit choice. Project checking starts after `initialized`, including a folder-only session.

Execute `dynare/recheckProject` to rerun discovery and all selected roots. Execute `dynare/cancelProject` to stop this pass; completed results remain, pending models remain pending, and cancelled jobs cannot publish. Cancellation lasts until a subsequent file edit/change or Recheck. It does not change the persistent setting. Recheck while off leaves the feature disabled.

The schema-version 1 status contains:

| Field | Meaning |
| --- | --- |
| `pass_revision` | Increasing identifier for the current pass in this server instance. |
| `enabled` | Persistent project switch. |
| `discovery` | `pending`, `discovering`, `complete`, `cancelled`, or `disabled`. |
| `cancelled` | The current pass has been cancelled. |
| `complete` | Discovery and queued checking have finished; failed/incomplete roots may remain. |
| `coverage_complete` | Complete with no discovery failure, incomplete root, or failed root. This does not mean no diagnostic Errors. |
| `counts` | Counts keyed by `pending`, `checking`, `checked`, `incomplete`, `failed`, and `excluded`. |
| `roots` | Root entries described below, sorted by URI. |
| `discovery_failures` | Folder URI and infrastructure failure text for unsuccessful walks. |
| `metrics` | `discovery_ms`, `analysis_ms`, `completed_jobs`, `reused_jobs`, and `elapsed_ms` for performance measurement. `analysis_ms` covers completed jobs, including compact input validation on reused reports. Wall timings are not solver work. |

Each root has `root_uri`, `state`, `revision` (nullable), `errors`, `warnings`, `failure` (nullable), and `dependency_candidates` (sorted file URIs). Watch every dependency candidate regardless of extension; this includes missing include search candidates, companion candidates, includepath directories, and loader files. Send ordinary `workspace/didChangeWatchedFiles` events for their creation, change, or deletion. Also watch root `.mod` creation/rename/deletion beneath workspace folders. The client needs no include parser or search resolver.

`checked` includes models that have diagnostic Errors. `incomplete` describes incomplete include/macro expansion; `failed` is an infrastructure/read failure. Neither pending nor incomplete nor failed roots may be presented as clean. Excluded entries have no background contribution. Counts and project coverage are separate from active-model counts.

## Reports and input lifetime

The server publishes one contribution per root whether it is open or discovered. Push diagnostics and workspace pull read the same merged report store. Changed inputs withdraw affected contributions immediately, so old ranges cannot acquire a newer document version. Open owners rebuild normally; unaffected owners retain their contributions. Pending roots have no current report or authoritative result counts. A prior report may remain privately cached and is reusable only after its exact input stamps are validated on the worker. Related locations and fixes retain written-file and root context. Closing a root or include removes its overlay and schedules its saved owners. Native path aliases replace the same overlay/version; Unix and virtual URI case remains distinct.

Changed inputs coalesce after the advertised `typing_pause_ms`. Unchanged checked reports can be reused; Recheck always computes again. The `metrics` field reports work and timing only; it does not claim a performance limit.

## Effective-preview navigation

`dynare/showEffectiveModel` retains its existing `uri`, `effective_text`, and counted-equation `origins`. Its additive navigation contract is advertised as `experimental.dygnosis.effectivePreview`, with command `dynare/showEffectiveModel` and `navigation_schema_version: 1`.

Pass `{ "root_uri": "file:///.../model.mod" }`; the existing `{ "uri": ... }` and URI-string arguments also work. Choose an explicit `.mod` or `.dyn` root when viewing an include. The root need not be open: the server uses disk files, live include overlays, and that root's settings.

The capability also advertises `readable_layout: true` and `source_layout: true`. Pass `{ "root_uri": "file:///.../model.mod", "layout": "source" }` to receive a display copy that keeps written spaces and line breaks while expanding macros and includes in place. Pass `layout: "readable"` for the older indented display copy. Its `navigation.effective_range` values refer to that exact returned copy. Use the same layout for opening, refreshing, and revalidating navigation. The VS Code preview requests `source` when advertised, otherwise `readable`, and otherwise the compact path. An older engine without `source_layout` keeps the indented preview, which does not preserve written spacing. Requests without this opt-in, MCP expansion, stored expansion, equation text, comparison, extraction, and Format Document retain their existing output.

The response adds `root_uri`, `revision`, `document_version` (null for an unopened root), `complete`, `navigation_schema_version`, `navigation`, and `dependency_candidates`. The last field lists exact file URI inputs, including missing candidates, from the same snapshot used by `dynare/modelInfo`; the advertised preview capability also sets `dependency_candidates: true`. Clients watch these paths without repeating include lookup. The revision covers source overlays, disk dependencies, and root settings. Subscribe to the existing model-info invalidation notification and discard a preview after an input, settings, or server-instance change. A disk change during response construction returns `success: false`, `code: "INPUT_CHANGED"`; refresh to obtain a new snapshot.

Each navigation row contains:

- `id` and `statement_id`, unique within the response revision;
- `effective_range`, a half-open range in the exact emitted `effective_text`, recorded while joining tokens and mapped through display whitespace edits for readable layout, or through the source-layout copy for `layout: "source"`;
- `written_locations`, verified equation-token portions in their written files;
- `macro_frames`, in execution order, with `kind`, nullable `variable`/`value`, `directive_locations`, and independently clipped `body_locations`;
- `kind` (`equation`, `local`, or `static`), `active`, nullable `number`, `scope` (`aggregate` or `heterogeneous`), and nullable `dimension`.

Ranges cover the parser's model row, including tags and local markers and excluding the statement semicolon. Locals and static-only rows have no counted number. Removed or replaced rows remain navigable when their text remains in the compilation-unit preview; `active` distinguishes them from surviving rows. Unrelated preview text has no navigation row. Repeated copies may share written targets but retain distinct IDs and emitted ranges. A partial-row macro expansion can have several iteration frames.

LSP ranges use zero-based lines and UTF-16 characters. Each written or frame location has `uri`, `range`, and nullable `document_version`. Several verified locations describe a cross-file row; none stretches one file's span into another file. Missing source text, invalid spans, and unsupported target URIs yield no location. An empty location list means the source action is unavailable, not position zero. Incomplete expansion, invalid macro block structure, parse recovery, or required includes return `complete: false`, `status: "incomplete"`, and empty navigation. Macro blocks must close within the root or executed include file; an included closer cannot repair a missing caller terminator. Expansion and navigation share the executed include graph: dormant include contents are not read, diagnosed, or emitted, and dormant include paths do not affect lookup. Include-path expressions use the current macro definitions, including definitions from executed files. Loop indices retain their last executed value; an empty collection leaves prior bindings unchanged. Unsupported macro state withholds include authority rather than inventing a missing-file Error. Counted origins retain their coordinate meanings.

MCP `dynare_expand` shares the row facts and schema. It adds nullable `root_file`, input `revision`, and `complete`. All MCP effective and written ranges retain one-based lines and Unicode-scalar columns: `{ "line", "column", "end_line", "end_column" }`. Written targets have `range` and `file` when a mapped input supplies a file identity; without a file map, a target belongs to the supplied `file_content`. MCP revisions cover the supplied content and include-map inputs; they are independent of LSP revisions. Existing effective text, equation counts, indices, and `origins` keep their meanings.

## Comparison navigation

The engine adds a `navigation` object to LSP `dynare/compareModels` and MCP `dynare_compare_models`. Existing comparison fields, values, ordering, equation pairing, shock locations, and Markdown retain their meanings. The LSP initialize response advertises `experimental.dygnosis.compareModels.navigation_schema_version: 1`.

`navigation.schema_version` is `1`. `before` and `after` each contain `root_uri`, an opaque `revision`, and `complete`. MCP uses the supplied active file as `root_uri`, or null when none was supplied. Each revision covers that side's root, included files, resolved search candidates, and analysis settings; LSP uses the same revision as `dynare/modelInfo`. A revision is for equality checks within the running engine, not a persistent hash.

`navigation.rows` contains source facts for every existing symbol list item, changed parameter, changed symbol, equation add/remove/change and unmatched row, and shock change. Each row's `id` is an exact JSON pointer into the existing result, for example `/changed_equations/0`, `/heterogeneous_equations/0/changed/1`, or `/unmatched_same_name/0/removed/0`. Consumers use that pointer to associate the row; they must not pair rows by their display name or text. An equation can appear in both an add/remove list and an unmatched group, with a separate pointer for each appearance.

Rows carry `kind` (`symbol`, `parameter`, `equation`, or `shock`) and the relevant name or shock form/role. Equation rows also carry `domain`, `dimension` (null for aggregate), and side-specific counted `index_old`/`index_new` (null when absent).

Each row's `before` and `after` are either null or `{occurrence_id, domain, dimension, written_locations}`. Null means that side has no row or its written target could not be verified. `occurrence_id` distinguishes expanded occurrences within that input revision. Repeated macro copies can have different identities and the same written ranges. Each side's scope is explicit, including for a symbol whose dimension changes. Changed parameters point at the last assignment used by the existing comparison; changed symbol metadata points at the declaration supplying that metadata. Several contributing files produce separate verified locations. Incomplete inputs withhold source targets.

LSP locations are `{uri, range}` using zero-based UTF-16 positions. MCP locations are `{file, line, column, end_line, end_column}` with one-based Unicode-scalar columns and the caller's file key when available; `file` is null for the unnamed supplied root. The ranges are exclusive at their ends. Navigation does not change the legacy shock `location` coordinate convention.

Both sides use live overlays and their own root settings. Compare observes disk dependencies even without a watched-file event. If files change while the engine is reading them, it returns `INPUT_CHANGED` and asks the client to refresh. A client must disable source actions once either revision is stale, then compare again before jumping. Older engines can still supply the original comparison without navigation.

## MCP input schemas

`dynare_equations` uses the model-info completeness decision for required
includes, parsing, and macro expansion. Incomplete input returns
`status: "incomplete"`, an explanatory `message`, `equations: []`, and
`count_gap: null`, including filtered requests. `dynare_compare_models`
returns only the incomplete status and message when either side is incomplete;
it supplies no diff arrays, Markdown, or navigation claims. A missing include
uses model-expansion wording.

For `dynare_diagnose`, `dynare_model_info`, `dynare_equations`,
`dynare_expand`, `dynare_related_files`, `dynare_find_references`, and
`dynare_rename`, a nonempty `files` map requires `active_file` to match a key
exactly. A missing key returns JSON-RPC invalid parameters (`-32602`):
`"<key>" is not in the file map`. Omitting the key returns
`active_file is required with a nonempty files map`. These errors return no
successful model result. With a matching key, omitted `file_content` uses
the map text; an explicit empty string overlays it with empty text. An absent
or empty map retains single-file behavior.

Dygnosis advertises its parameterized tools with an explicit JSON Schema
draft-07 declaration. Tool names, arguments, required fields, nullable values,
defaults, and valid-input results retain their existing meanings. The tool with no
arguments retains its empty object schema.

The minimum supported editor, VS Code 1.102.0, validates draft-07 schemas
without downloading schema metadata. Its native MCP discovery rejects the
default 2020-12 declaration. An explicit alternative dialect is allowed by the
[MCP schema dialect contract](https://modelcontextprotocol.io/specification/2025-11-25/basic#schema-dialect).
