# Effective-preview navigation

`dynare/showEffectiveModel` retains its existing `uri`, `effective_text`, and counted-equation `origins`. Its additive navigation contract is advertised as `experimental.dygnosis.effectivePreview`, with command `dynare/showEffectiveModel` and `navigation_schema_version: 1`.

Pass `{ "root_uri": "file:///.../model.mod" }`; the existing `{ "uri": ... }` and URI-string arguments also work. Choose an explicit `.mod` or `.dyn` root when viewing an include. The root need not be open: the server uses disk files, live include overlays, and that root's settings.

The response adds `root_uri`, `revision`, `document_version` (null for an unopened root), `complete`, `navigation_schema_version`, `navigation`, and `dependency_candidates`. The last field lists exact file URI inputs, including missing candidates, from the same snapshot used by `dynare/modelInfo`; the advertised preview capability also sets `dependency_candidates: true`. Clients watch these paths without repeating include lookup. The revision covers source overlays, disk dependencies, and root settings. Subscribe to the existing model-info invalidation notification and discard a preview after an input, settings, or server-instance change. A disk change during response construction returns `success: false`, `code: "INPUT_CHANGED"`; refresh to obtain a new snapshot.

Each navigation row contains:

- `id` and `statement_id`, unique within the response revision;
- `effective_range`, a half-open range in the exact emitted `effective_text`, recorded while joining tokens rather than found by searching text;
- `written_locations`, verified equation-token portions in their written files;
- `macro_frames`, in execution order, with `kind`, nullable `variable`/`value`, `directive_locations`, and independently clipped `body_locations`;
- `kind` (`equation`, `local`, or `static`), `active`, nullable `number`, `scope` (`aggregate` or `heterogeneous`), and nullable `dimension`.

Ranges cover the parser's model row, including tags and local markers and excluding the statement semicolon. Locals and static-only rows have no counted number. Removed or replaced rows remain navigable when their text remains in the compilation-unit preview; `active` distinguishes them from surviving rows. Unrelated preview text has no navigation row. Repeated copies may share written targets but retain distinct IDs and emitted ranges. A partial-row macro expansion can have several iteration frames.

LSP ranges use zero-based lines and UTF-16 characters. Each written or frame location has `uri`, `range`, and nullable `document_version`. Several verified locations describe a cross-file row; none stretches one file's span into another file. Missing source text, invalid spans, and unsupported target URIs yield no location. An empty location list means the source action is unavailable, not position zero. Incomplete expansion, invalid macro block structure, parse recovery, or required includes return `complete: false`, `status: "incomplete"`, and empty navigation. Macro blocks must close within the root or executed include file; an included closer cannot repair a missing caller terminator. Proven dormant include contents do not refuse navigation or supply targets. Navigation uses a verified projection of executed includes; disagreement with the legacy emitted token sequence withholds navigation. Legacy text, counts, and counted origins retain their prior meanings.

MCP `dynare_expand` shares the row facts and schema. It adds nullable `root_file`, input `revision`, and `complete`. All MCP effective and written ranges retain one-based lines and Unicode-scalar columns: `{ "line", "column", "end_line", "end_column" }`. Written targets have `range` and `file` when a mapped input supplies a file identity; without a file map, a target belongs to the supplied `file_content`. MCP revisions cover the supplied content and include-map inputs; they are independent of LSP revisions. Existing effective text, equation counts, indices, and `origins` keep their meanings.
