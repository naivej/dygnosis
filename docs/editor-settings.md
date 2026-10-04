# Editor settings and LSP contracts

Dygnosis supplies these contracts through the language server. Configure an LSP client to launch `dygnosis` over stdio. The VS Code extension adds native Settings, a [count bar](vscode-status.md), [model view](vscode-model-view.md), [colors](vscode-colors.md), [CodeLens](vscode-lenses.md), [diagnostic controls](vscode-diagnostics.md), [model Diff](vscode-diff.md), [origin navigation](preview-origins.md), and [project checks](project-diagnostics.md). See [distribution](distribution.md) for host packages and runtime requirements.

## Settings

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

| Setting | Default | Effect |
|---|---|---|
| `searchPaths` | `[]` | Additional include lookup directories for that root |
| `formatIndent` | `"tab"` | A tab or 1–8 spaces for formatting |
| `nameDetails.longName` | `true` | Show written `long_name` metadata in hover, completion, and Outline |
| `nameDetails.tex` | `true` | Show written TeX metadata as literal text |
| `outline.sections` | All five entries above | Choose file-local Outline content; `[]` hides all sections |
| `outline.equationNumbers` | `true` | Label safely mapped counted equations with Dygnosis numbers |
| `parameterValueHints` | `true` | Show proven expression values through LSP inlay hints |

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

Migrate rules that targeted a specific old Dynare role as follows:

| Old selector | New selector |
|---|---|
| `variable:dynare` for endogenous names | `dynareEndogenous:dynare` |
| `type:dynare` for exogenous names | `dynareExogenous:dynare` |
| `macro:dynare` for model parameters | `dynareParameter:dynare` |
| `parameter:dynare` for model locals | `dynareModelLocal:dynare` |

The VS Code client registers `variable` as the parent of all four custom types, so a generic `variable` rule can style every role there. Keep role-specific selectors when that is the intended scope. Other LSP clients supply their own token registrations and theme rules.

## Expression value hints

Hints show finite arithmetic values proven at each top-level assignment in execution order, including assignments to helper names. Plain numbers with an optional sign stay quiet. An expression such as `beta = 1 / 1.04;` can have a hint; it is arithmetic folding, not MATLAB execution.

Unknown inputs, non-finite arithmetic, opaque native execution, and commands that may change values withhold knowledge until later explicit assignments establish it again. A repeated macro site has one hint only when every occurrence has the same proven finite value. Incomplete expansion, ambiguous owners, or an unverified written position have no authoritative hint. Use `parameterValueHints` and the client's native inlay-hint visibility control to hide hints. Hover and existing comparison values keep their established behavior.

## Diagnostic locations and fixes

Applicable duplicate diagnostics link to the first occurrence through LSP `relatedInformation` and MCP's additive `related` rows. Include cycles retain the written include edges. LSP coordinates are zero-based UTF-16; MCP keeps one-based scalar coordinates. Each related location is mapped independently to its own source file and macro occurrence.

Published fixes remain available for an unopened include when its checked source snapshot is current. Edits to open files carry their actual document versions; unopened files use an unversioned identifier. Stale source or root revisions withhold the action. W020 and W022 carry the standard LSP Unnecessary tag; E021 does not.
