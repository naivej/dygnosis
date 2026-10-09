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

At a macro directive name after a line-start `@#`, completion returns the 15
directive names. The label, filter text, and plain inserted text are the
lowercase name, such as `define`; the edit replaces the name and retains
`@#`. The macro scanner selects the site, including indented directives.
Completion triggers are `(`, `,`, and `#`. A request triggered by `#` outside
that site returns no items; a manual request keeps ordinary completion.
Signature-help triggers remain `(`, `,`, and `=`.

## Written model information

The initialize response advertises the following under `capabilities.experimental.dygnosis`:

```json
{
  "modelInfo": {"command": "dynare/modelInfo", "schema_version": 1, "model_locals": true},
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
{"schema_version": 1, "root_uri": "file:///C:/models/project/main.mod", "revision": "opaque-model-revision", "input_revision": "opaque-diagnostic-input-revision"}
```

A missing root has `revision: null`. Re-request model information for affected roots after changes to includes, settings, overlays, or reported disk files. A client should forward watched-file events, cancel superseded requests, and discard old revisions and document versions. Standard semantic-token and inlay-hint refresh requests are sent only when the client advertises support; document-symbol refresh needs client handling.

### Model-local information and editor bindings

Complete `dynare_model_info` and LSP `dynare/modelInfo` responses add
`model_locals`, with separate `declarations` and `definitions` arrays:

| Array | Required fields | Optional source and metadata fields |
|---|---|---|
| `declarations` | `name` | `tex_name`, `origin`, `origin_frames`; MCP can also include `origin_uri`. |
| `definitions` | `name`, `dimension`, `text`, `expression`, `idents` | `origin`, `origin_frames`; MCP can also include `origin_uri`. |

Declarations are explicit top-level `model_local_variable` rows, including
unused declarations. Definitions are completed accepted `#` rows. Both arrays
follow parser execution order; repeated macro executions remain separate even
when their written origins match. `dimension: null` selects the aggregate
model; a dimension name selects its heterogeneous model. Several written
blocks of the same model share that definition scope. A declaration alone has
no expression. Complete input without locals returns two empty arrays;
incomplete model information withholds the inventory. Older engines can omit
the field. LSP advertises the addition with `modelInfo.model_locals: true`
without changing `schema_version: 1`.

`text` is the parsed `#` row and `expression` is its parsed right side.
Definition `idents` use the equation identifier fields: `name`, `timing`,
`dynare_timing`, `class`, and optional `timing_class`. Recognized local uses in
definition `idents` and `dynare_equations` have `class: "model_local"` and no
endogenous timing class. Their offsets retain the existing written timing
contract, including shifted local uses. Local rows stay outside equation
counts and indexes. Presentation settings do not remove MCP inventory data.

An `origin` appears only for one verified written source segment. MCP `origin` uses
one-based Unicode-scalar coordinates; `origin_uri` names the supplied file key
when present, and `origin_frames` retains the existing macro frame format.
LSP `origin` is a `Location` with `uri` and a zero-based UTF-16 `range`;
its `origin_frames` use `kind`, `variable`, `value`, and `segments` containing
written `Location` objects. LSP model-local facts share the enclosing response's
root, revision, and document-version rules. An absent origin is not a target at
line 1.

Editor local completion is limited to model expressions and uses available
declaration/definition facts in the selected model. It follows the macro
directive-name branch, so a model-local `#` trigger still returns no value
list. Local items use `VARIABLE`, `model-local variable` detail, and the name
for label, filter, and inserted text. Hover can show the defining expression
and a written definition link; macro-changed expressions are marked expanded.

Local definition navigation targets the selected scope's `#` target.
Declaration navigation prefers an explicit declaration. References honor
`includeDeclaration` for both target forms; highlights mark them as writes and
bound uses as reads. Rename proposes only verified declaration, definition,
and bound-use edits from current inputs. Ambiguous owners, a declaration
shared by different local bindings, uneditable macro text, or name collisions
withhold the rename. Comments, strings, unrelated roots, and other local scopes
stay unchanged.

MCP `dynare_find_references` and `dynare_rename` match identifier spelling;
they do not resolve these editor bindings. `dynare_extract` retains the selected
model's required nested local definitions and explicit metadata, including
definitions in earlier blocks. A different dimension's same-name definition
is not that dependency. No MCP completion tool is added.

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

Every routed diagnostic carries `data.root` and `data.input_revision` for its
checked owner and inputs. `dynare/modelInfoChanged` adds optional
`input_revision` with that same diagnostic token. Its existing `revision`
is the model-view token and also covers presentation settings; do not compare
these two tokens. Notifications follow the diagnostic push. A client can
therefore retain matching pushed facts and retire only an older owner's rows.
Older engines can omit this provenance; local input and document-version
guards still apply.

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

A code-action request with no selected diagnostic can discover each current fix,
and that action names its check. A request that already names a diagnostic
returns a fix only for that diagnostic. A different code, a missing or stale
root, revision, or writing context, or another root is not a substitute.
`context.only` keeps Quick Fix and refactor apart. Equal edit text does not
combine roots into one action. Refactor templates carry the open document
version, the same way stored fixes do. An unopened file stays unversioned.

The VS Code language client drops `TextDocumentEdit.version` when it builds a
`WorkspaceEdit`. The extension therefore removes that edit from the code
action and applies its private copy from `dygnosis.applyDiagnosticEdit`. The
command argument is only an opaque id. The extension applies the copy when
the offered document versions, the owning diagnostics, and the server instance
are still current. A closed, reopened, or replaced document does not revive
it. A file-dependency change drops it. A notification that repeats an action's
own root and input revision does not. A revision notification drops a refactor
template or an older-engine fix that carries no root or revision, because that
checked input cannot be proved. Changing the model owner drops a captured edit
while the previous diagnostics are still on screen. Withdrawing a check, or
ignoring it, drops its captured edit. Publishing or showing that check again
requires a new action.

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

The capability also advertises `readable_layout: true`, `source_layout: true`, `source_navigation_schema_version: 1`, and `macro_ranges: true`. Pass `{ "root_uri": "file:///.../model.mod", "layout": "source" }` to receive a display copy that keeps written spaces and line breaks while expanding macros and includes in place. A source-layout response adds `source_navigation_schema_version: 1` and `source_navigation`. Each region has `id`, a half-open `effective_range` in that display copy, one `written_location`, and `kind` (`copy`, `substitution`, or `identifier`). An `identifier` region is one macro-built name, such as `beta_1`, and its written location is the complete written name, such as `beta_@{j}`. A `copy` region projects a cursor or contained selection onto the written subrange. A `substitution` region selects the whole written interpolation. Regions that cross a file boundary are separate. Incomplete expansion returns an empty `source_navigation` array. Readable layout and older engines omit `source_navigation`; their equation `navigation` is unchanged. A source-layout response also adds `macro_ranges`: sorted, non-overlapping, half-open ranges of macro-expanded text in that same display copy. Incomplete expansion returns an empty array. Readable layout and older engines omit `macro_ranges`. Pass `layout: "readable"` for the older indented display copy. Its `navigation.effective_range` values refer to that exact returned copy. Use the same layout for opening, refreshing, and revalidating navigation. The VS Code preview requests `source` when advertised, otherwise `readable`, and otherwise the compact path. An older engine without `source_layout` keeps the indented preview, which does not preserve written spacing. Requests without this opt-in, MCP expansion, stored expansion, equation text, comparison, extraction, and Format Document retain their existing output.

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

`macro_messages` is an array on `dynare_expand` and `dynare/showEffectiveModel`, in execution order, including repeated loop output and messages printed before an abort. Each entry has `kind` (`echo` or `macrovars`) and `message`. MCP locations use `location` with one-based `line`, `column`, `end_line`, and `end_column`, plus `file` when the message is in a mapped include. LSP locations use a zero-based UTF-16 `range`, plus `uri` when the message is in another file. `@#echomacrovars(save)` is not a message; its assignment text is part of `effective_text` and is not executed as a macro. The saved line number is the line in the written file that contains the directive. The messages are not diagnostics. `experimental.dygnosis.effectivePreview.macro_messages` is true when the engine sends this array. An older engine omits the array; clients treat that as no messages. A message-size limit, and any other macro limit, stops expansion with I211. A fatal macro error, missing include, or `@#error` also stops the root: later messages are absent, `complete` is false, and MCP and LSP set `status` to `incomplete`. A complete payload omits `status`.

## Comparison navigation

The engine adds a `navigation` object to LSP `dynare/compareModels` and MCP `dynare_compare_models`. Existing comparison fields, values, ordering, equation pairing, shock locations, and Markdown retain their meanings. The LSP initialize response advertises `experimental.dygnosis.compareModels.navigation_schema_version: 1`.

`navigation.schema_version` is `1`. `before` and `after` each contain `root_uri`, an opaque `revision`, and `complete`. MCP uses the supplied active file as `root_uri`, or null when none was supplied. Each revision covers that side's root, included files, resolved search candidates, and analysis settings; LSP uses the same revision as `dynare/modelInfo`. A revision is for equality checks within the running engine, not a persistent hash.

`navigation.rows` contains source facts for every existing symbol list item, changed parameter, changed symbol, equation add/remove/change and unmatched row, and shock change. Each row's `id` is an exact JSON pointer into the existing result, for example `/changed_equations/0`, `/heterogeneous_equations/0/changed/1`, or `/unmatched_same_name/0/removed/0`. Consumers use that pointer to associate the row; they must not pair rows by their display name or text. An equation can appear in both an add/remove list and an unmatched group, with a separate pointer for each appearance.

Rows carry `kind` (`symbol`, `parameter`, `equation`, or `shock`) and the relevant name or shock form/role. Equation rows also carry `domain`, `dimension` (null for aggregate), and side-specific counted `index_old`/`index_new` (null when absent).

Each row's `before` and `after` are either null or `{occurrence_id, domain, dimension, written_locations}`. Null means that side has no row or its written target could not be verified. `occurrence_id` distinguishes expanded occurrences within that input revision. Repeated macro copies can have different identities and the same written ranges. Each side's scope is explicit, including for a symbol whose dimension changes. Changed parameters point at the last assignment used by the existing comparison; changed symbol metadata points at the declaration supplying that metadata. Several contributing files produce separate verified locations. Incomplete inputs withhold source targets.

LSP locations are `{uri, range}` using zero-based UTF-16 positions. MCP locations are `{file, line, column, end_line, end_column}` with one-based Unicode-scalar columns and the caller's file key when available; `file` is null for the unnamed supplied root. The ranges are exclusive at their ends. Navigation does not change the legacy shock `location` coordinate convention.

LSP `dynare/compareModels` uses live overlays and each root's settings. It observes disk dependencies even without a watched-file event. If files change while the engine is reading them, it returns `INPUT_CHANGED` and asks the client to refresh. A client must disable source actions once either revision is stale, then compare again before jumping. Supplied-text MCP comparison uses only caller-provided text and file maps. Older engines can still supply the original comparison without navigation.

### Semantic comparison, Source and Coverage

Both LSP comparison capabilities add `semantic_schema_version: 1`,
`source_changes_schema_version: 1` and `coverage_schema_version: 1`.
`compareModels` retains navigation schema 1 and has no outer `schema_version`;
`compareModelSnapshots` retains outer schema 1 and navigation schema 2. An
engine without the additive fields supports the existing structural fallback.
Partial advertisements or unsupported versions must fail visibly.

The shared diff adds `comparison_versions: {semantic: 1, source_changes: 1,
coverage: 1}`, `semantic`, `source_changes` and `coverage`. Current-file LSP,
supplied-text MCP and repository MCP place them beside the legacy arrays.
Snapshot LSP places them inside `diff`. Each successful transport returns
`sources: {before: {file_key: text}, after: {file_key: text}}`; snapshot LSP
places that registry in its result envelope. Empty text is a captured file.
Keys are opaque identities in their input namespace. Current-file LSP uses
native workspace keys; a client maps these to its registered file URIs.
Snapshot and supplied-map keys must not be reinterpreted as live disk paths.

Each additive object has `schema_version: 1`, `availability` and `limits`.
Availability is `complete`, `partial`, `not_available` or `limit_exceeded`.
A limit has `code`, `reason`, `owner` and nullable positive `omitted` count.
Read field and coverage limits even when there are no changed rows.

| Object | Fields and meaning |
|---|---|
| `semantic` | `budgets`, `rows`, `references`; typed retained model facts. |
| Semantic row | `pointer`, `family`, `change`, `name`, `count_unit`, `facets`, nullable `before`/`after`, `fields`, `expressions`, `timing`, `references`, `limits`. |
| Row side | `name`, `scope`, nullable `occurrence` and `equation_index`, nullable accepted statement `context`. Scope has `domain`, nullable `dimension` and `block`. Context adds accepted kind/name, execution order, scope and nullable supporting pointer. These display fields are not navigation proof. |
| Field change | `name`, `label`, side states, `changed`, `comparison_availability`, nullable finite `numeric_difference`. States are `absent`, `empty`, `unknown` or `present`; typed values are text, number, integer, boolean, ordered list or named record. Absent/unknown have null values; empty is explicit empty text. |
| Expression | `field`, nullable sides with exact `text` and `runs`, `highlight_basis`, `availability`, nullable `reason`. Runs have text and `unchanged`/`added`/`removed` role and reconstruct that side's text exactly. Basis is `paired_expression`, `unpaired_text_only` or `none`; unavailable alignment keeps plain text, basis none and a reason. |
| Timing | Nullable sides with `name`, `class`, `written_offset`, `converted_offset`, `occurrence`. Written timing and predetermined-variable conversion are distinct. |
| Reference | `pointer`, `symbol`, `side`, `equation_pointer`, `equation_index`, `label`, `scope`, `occurrence`, `timing`. It describes a direct counted-equation use, including unchanged text; it adds no model row. |
| `source_changes` | `files`; each has `pointer`, `change`, `correspondence`, nullable sides, `availability`, `hunks`, nullable `omitted_hunks`, `limits`. A side has nullable `input_id`, exact `file_key` and `exact_text_available`. Current/supplied inputs use null IDs; snapshots use selected IDs. |
| Source hunk | One-based `before_start`/`after_start`, `before_lines`/`after_lines`, and ordered text/role `lines`. Lines are text-diff display positions, not source targets. |
| `coverage` | `source_boundary`, `families`; each family has availability, compared `fields` and exact limits. External functions, data contents and unexecuted children are outside captured roots and executed includes. Supplied mode compares only supplied text. |

Families are `symbols`, `parameters`, `equations`, `shocks`, `steady_state`,
`priors`, `commands`, `observables`, `data`, `occbin`, `policy`,
`semi_structural`, `moments`, `ms_sbvar`, `heterogeneity`, `external_functions`,
`trends`, `operations` and `macro_context`. `change` is `added`, `removed`,
`changed` or `unpaired`. `count_unit` is `final_fact`, `accepted_occurrence`
or `operation`; written history and final settings can differ. Each fact has
one owner. Supporting statement context and references do not count again.
Source file and hunk counts remain separate from model row appearances.

Existing owners retain their exact legacy pointers. New rows use
`/semantic/rows/N`; references use `/semantic/references/N`; files use
`/source_changes/files/N`, with N their index in that array. Semantic navigation
entries have `kind: "semantic"`, `family` and `name`. Reference entries have
`kind: "reference"`, `name`, `equation_pointer` and a target only on the stated
side. An equation pointer names an actual retained equation owner or the
reference itself for unchanged equation context. Validate pointer ownership,
schema versions, side/input identity, field states, token reconstruction and
captured registry membership before display or actions. Private parser receipts
are not serialized. Missing proof yields a null target; display offsets and
hunk lines cannot create one.

`semantic.budgets` reports comparison-wide token alignment cells (250,000),
source alignment cells (1,000,000), references per side (2,000), source hunks
(2,000) and optional serialized detail bytes (8,388,608). Limits retain exact
expression or complete captured-file text when available and report omissions;
legacy arrays survive optional-detail limits. References cannot outlive an
omitted semantic equation row. Retained-tree materialization also has bounded
work and field-specific limits. Unknown evaluation, unavailable retention and
budget-unavailable comparison are distinct. No algebraic, solved-state,
posterior or simulation claim follows from these facts.

### Isolated comparison inputs

LSP `dynare/compareModelSnapshots` is advertised as
`experimental.dygnosis.compareModelSnapshots` with `schema_version: 1`,
`navigation_schema_version: 2`, `needs_sources: true`, `max_manifest_files`,
and `max_source_bytes`. It takes one object with `schema_version: 1`, `before`, and `after`.
Each side has a distinct caller-supplied `input_id`.

- Working: `{kind: "working", input_id, root_uri, expected_revision, search_paths?}`.
  The expected revision is the current `dynare/modelInfo.revision` for that root and server instance.
  Capture uses editor buffers and saved dependencies. Optional search paths are absolute host folders.
- Git: `{kind: "git", input_id, repository_uri, commit, requested_ref?, root_file,
  search_paths, manifest, sources}`. `commit` is a full resolved commit id; `root_file` and manifest
  keys are exact repository-relative paths with `/` separators. The full manifest maps each path to
  `{mode, object_id}`. `sources` maps loaded paths to `{kind: "text", text}` or
  `{kind: "failure", code, message}`. It cannot contain a path absent from the manifest.

The host acquires Git objects. The engine executes macros and selects include candidates from the
manifest in normal lookup order. Inactive includes need no bodies. An in-tree absolute source path
is resolved lexically within the selected tree. A reached outside candidate, symlink, or gitlink
is unsupported; current disk cannot supply historical text.

The response has `state`:

- `needs_sources`: `requests` lists `{side, input_id, file_keys}`. Supply those exact bodies and
  repeat the request with the same identities, commit, manifest, and Working expected revisions.
- `failure`: `code`, `message`, optional `side` and `file_key`; no authoritative change arrays.
- `result`: `diff` keeps the existing comparison fields. `inputs` has `schema_version: 1` and
  resolved `before`/`after` facts. `sources` contains the exact captured text of each side's files.
  `navigation.schema_version` is 2.

Each input fact gives `input_id`, `kind`, `root_file`, opaque `revision`, `complete`, `search_paths`,
`file_keys`, `dependency_candidates`, and `source_policy`. Git also gives `repository_uri`, `commit`,
and `requested_ref`; Working gives `root_uri` and uses `source_policy: "editor_buffers_and_disk"`.
Working dependency candidates include missing host file URIs. Watch those paths and revalidate the
model revision before navigation. Git inputs remain fixed when a branch or HEAD moves.

Schema-2 navigation keeps row JSON pointers, occurrence ids, and comparison pairing. Side facts
use `input_id` and `root_file`. LSP written targets use `{input_id, file_key, commit?, range}` with
zero-based UTF-16 ranges. Map the key through that input's captured text; do not treat it as a live
disk path. Historical target `commit` identifies the selected tree. Compare only model revisions
with model revisions; the separate notification `input_revision` is a diagnostic token.

The extension uses a read-only custom editor with view type `dygnosis.changes`. Its versioned
`dygnosis-changes:` resource stores selectors and the invoking anchor for reload. A shared document
owns capture and cancellation; split views keep separate display choices. Historical written text
uses read-only `dygnosis-history:` documents whose identities include repository, commit, and key.
Working captured Source text uses read-only `dygnosis-captured:` documents with
one capture identity; an absent side of a captured-file diff is explicit empty text.
An open source tab retains its captured text after the comparison closes. Neither resource scheme
is an ordinary analysis document.

### MCP repository comparison

`dynare_compare_models` accepts either its existing supplied-text fields or repository fields:

```json
{
  "repository_path": "C:/models/project",
  "before": { "kind": "git", "root_file": "main.mod", "ref": "HEAD" },
  "after": { "kind": "working", "root_file": "main.mod" },
  "search_paths": ["common"]
}
```

`repository_path` must be absolute on the MCP server host. Both roots use exact repository-relative
paths; a rename is explicit. Each Git selector requires a ref, resolved to a full commit before any
body reads. Working reads saved files only. Relative search folders resolve against the repository;
saved Working dependencies may lie outside it. Historical lookup retains the tree limits above.
Git reads do not fetch, prompt for credentials, or change the index, refs, or working files.

Successful repository calls retain the existing top-level diff arrays and add schema-1 `inputs`
and schema-2 `navigation`. Source policies are `git_tree` and `saved_files`. Input facts also give
`repository_path`, the requested root path, and resolved search folders where applicable. MCP
written targets use `{input_id, file_key, commit?, line, column, end_line, end_column}` with one-based
Unicode-scalar coordinates. Incomplete or failed calls retain the selected-input envelope, report
`status` and an explanation, and omit authoritative diff arrays. Mixed or malformed modes return
JSON-RPC invalid parameters (`-32602`). Supplied-text calls keep schema-1 navigation and never read
repository files.

## MCP input schemas

`dynare_equations` uses the model-info completeness decision for required
includes, parsing, and macro expansion. Incomplete input returns
`status: "incomplete"`, an explanatory `message`, `equations: []`, and
`count_gap: null`, including filtered requests. `dynare_compare_models`
in supplied-text mode returns only the incomplete status and message when either side is incomplete;
it supplies no diff arrays, Markdown, or navigation claims. Repository mode also returns its input
envelope on incomplete or failed acquisition. A missing include
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
